//! Software-secret unlock support for Jnoccio Fusion.
//!
//! The 128-character `jnoccio-fusion.unlock` file is not itself a git-crypt
//! key. It decrypts a small AES-GCM envelope that contains the git-crypt key,
//! which is then written to a short-lived file just long enough to invoke
//! `git-crypt unlock`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use scrypt::{scrypt, Params};

const SECRET_LEN: usize = 128;
const AAD: &str = "jnoccio-fusion-git-crypt-key-v1";
const SCRYPT_LOG_N: u8 = 18;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;

const ENVELOPE_SALT: &str = "p7kKIm3yOgV1ztwDz-zh-Q";
const ENVELOPE_IV: &str = "LSqJhuHtPp4QrGXW";
const ENVELOPE_TAG: &str = "RRI9y87haWJ39q4TuMZ-eA";
const ENVELOPE_CIPHERTEXT: &str = "rLX1c53q2pGv7yNokmAXN-Tl5ObbnhrO8-V5vDoiQeIUTEErk7eRlEPhBDfnlyJBn1J-70hQ4jMj5o7xeYnP5mk3LVZP_Dm_S8uwP-T13NKWPLICpeGj2bkf9DMJzw58o655S3AwNw5bMUctJt2IRm0cturz09W30kQ-TcocGBjrvRE-NwTewLgRGLBKkIfrNNdEFA";

/// Result of applying the software unlock to a repository checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretUnlockReport {
    /// Repository root passed to git-crypt.
    pub repo_root: PathBuf,
    /// True when plaintext Jnoccio files were readable after the unlock.
    pub plaintext: bool,
}

/// Normalize terminal/paste noise out of a Jnoccio software unlock secret.
pub fn normalize_unlock_secret(input: &str) -> String {
    let mut compact = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            let _ = chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            compact.push(ch);
        }
    }
    if compact.len() == SECRET_LEN + 6 && compact.starts_with("200") && compact.ends_with("201") {
        compact[3..compact.len() - 3].to_string()
    } else {
        compact
    }
}

/// Return true when a normalized unlock secret has the expected shape.
pub fn is_valid_unlock_secret(input: &str) -> bool {
    input.len() == SECRET_LEN
        && input
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Read, normalize, and validate a Jnoccio software unlock secret from disk.
pub fn read_unlock_secret(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let secret = normalize_unlock_secret(&text);
    if !is_valid_unlock_secret(&secret) {
        bail!(
            "unlock secret at {} must be exactly 128 ASCII characters from [A-Za-z0-9_-]",
            path.display()
        );
    }
    Ok(secret)
}

/// Decrypt the embedded git-crypt key envelope with a normalized secret.
pub fn decrypt_git_crypt_key(secret: &str) -> Result<Vec<u8>> {
    if !is_valid_unlock_secret(secret) {
        bail!("unlock secret must be exactly 128 ASCII characters from [A-Za-z0-9_-]");
    }

    let salt = URL_SAFE_NO_PAD
        .decode(ENVELOPE_SALT)
        .context("decode Jnoccio unlock salt")?;
    let iv = URL_SAFE_NO_PAD
        .decode(ENVELOPE_IV)
        .context("decode Jnoccio unlock iv")?;
    let tag = URL_SAFE_NO_PAD
        .decode(ENVELOPE_TAG)
        .context("decode Jnoccio unlock tag")?;
    let ciphertext = URL_SAFE_NO_PAD
        .decode(ENVELOPE_CIPHERTEXT)
        .context("decode Jnoccio unlock ciphertext")?;

    let params =
        Params::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P, 32).context("build scrypt params")?;
    let mut key = [0u8; 32];
    scrypt(secret.as_bytes(), &salt, &params, &mut key).context("derive Jnoccio unlock key")?;

    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("build AES-256-GCM cipher"))?;
    let mut encrypted = ciphertext;
    encrypted.extend_from_slice(&tag);
    cipher
        .decrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: &encrypted,
                aad: AAD.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("unlock secret was not valid"))
}

/// Unlock a repository checkout using a software secret file.
pub fn unlock_repo_with_secret_file(
    repo_root: &Path,
    secret_path: &Path,
) -> Result<SecretUnlockReport> {
    let secret = read_unlock_secret(secret_path)?;
    unlock_repo_with_secret(repo_root, &secret)
}

/// Unlock a repository checkout using a normalized software secret.
pub fn unlock_repo_with_secret(repo_root: &Path, secret: &str) -> Result<SecretUnlockReport> {
    unlock_repo_with_secret_options(repo_root, secret, false)
}

/// Unlock a repository checkout, optionally refreshing `jnoccio-fusion/` from
/// the index after the key is installed.
pub fn unlock_repo_with_secret_options(
    repo_root: &Path,
    secret: &str,
    force_refresh_checkout: bool,
) -> Result<SecretUnlockReport> {
    let raw_key = decrypt_git_crypt_key(secret)?;
    install_git_crypt_key(repo_root, &raw_key)?;
    let temp_dir = temp_key_dir()?;
    let temp_key = temp_dir.join("jnoccio-fusion.key");
    fs::write(&temp_key, &raw_key).with_context(|| format!("write {}", temp_key.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp_key, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod {}", temp_key.display()))?;
    }

    let output = Command::new("git-crypt")
        .arg("unlock")
        .arg(&temp_key)
        .current_dir(repo_root)
        .output()
        .context("spawn git-crypt unlock")?;

    let _ = fs::remove_dir_all(&temp_dir);

    if !output.status.success() {
        if crate::unlock::has_plaintext_signals(repo_root) {
            return Ok(SecretUnlockReport {
                repo_root: repo_root.to_path_buf(),
                plaintext: true,
            });
        }
        if force_refresh_checkout {
            refresh_jnoccio_checkout(repo_root)?;
            if crate::unlock::has_plaintext_signals(repo_root) {
                return Ok(SecretUnlockReport {
                    repo_root: repo_root.to_path_buf(),
                    plaintext: true,
                });
            }
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        bail!("git-crypt unlock failed: {message}");
    }

    let mut plaintext = crate::unlock::has_plaintext_signals(repo_root);
    if !plaintext && force_refresh_checkout {
        refresh_jnoccio_checkout(repo_root)?;
        plaintext = crate::unlock::has_plaintext_signals(repo_root);
    }
    if !plaintext {
        bail!("git-crypt reported success, but Jnoccio Fusion files are still locked");
    }

    Ok(SecretUnlockReport {
        repo_root: repo_root.to_path_buf(),
        plaintext,
    })
}

/// Refresh protected Jnoccio files from the index after the key is installed.
///
/// This overwrites local changes under `jnoccio-fusion/`, so callers should
/// only expose it behind an explicit user flag.
pub fn refresh_jnoccio_checkout(repo_root: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["ls-files", "-s", "--", "jnoccio-fusion"])
        .current_dir(repo_root)
        .output()
        .context("list Jnoccio files for checkout refresh")?;
    if !output.status.success() {
        bail!("git ls-files jnoccio-fusion failed");
    }
    let text = String::from_utf8(output.stdout).context("parse Jnoccio file list")?;
    let files = text
        .lines()
        .filter_map(parse_ls_files_stage_line)
        .collect::<Vec<_>>();
    if files.is_empty() {
        bail!("no tracked Jnoccio files found to refresh");
    }

    let smudge_dir = temp_key_dir()?;
    for (index, file) in files.into_iter().enumerate() {
        let encrypted = Command::new("git")
            .args(["cat-file", "-p", &format!(":{}", file.path)])
            .current_dir(repo_root)
            .output()
            .with_context(|| format!("read encrypted blob for {}", file.path))?;
        if !encrypted.status.success() {
            let stderr = String::from_utf8_lossy(&encrypted.stderr);
            bail!("git cat-file failed for {}: {}", file.path, stderr.trim());
        }

        let encrypted_path = smudge_dir.join(format!("{index}.enc"));
        let plaintext_path = smudge_dir.join(format!("{index}.plain"));
        fs::write(&encrypted_path, &encrypted.stdout)
            .with_context(|| format!("write {}", encrypted_path.display()))?;
        let encrypted_file = fs::File::open(&encrypted_path)
            .with_context(|| format!("open {}", encrypted_path.display()))?;
        let plaintext_file = fs::File::create(&plaintext_path)
            .with_context(|| format!("create {}", plaintext_path.display()))?;

        let smudged = Command::new("git-crypt")
            .arg("smudge")
            .current_dir(repo_root)
            .stdin(Stdio::from(encrypted_file))
            .stdout(Stdio::from(plaintext_file))
            .stderr(Stdio::piped())
            .output()
            .with_context(|| format!("smudge {}", file.path))?;
        if !smudged.status.success() {
            let stderr = String::from_utf8_lossy(&smudged.stderr);
            bail!(
                "git-crypt smudge failed for {}: {}",
                file.path,
                stderr.trim()
            );
        }

        let path = repo_root.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let plaintext = fs::read(&plaintext_path)
            .with_context(|| format!("read {}", plaintext_path.display()))?;
        fs::write(&path, plaintext).with_context(|| format!("write {}", path.display()))?;
        set_tracked_mode(&path, &file.mode)?;
    }
    let _ = fs::remove_dir_all(&smudge_dir);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackedFile {
    mode: String,
    path: String,
}

fn parse_ls_files_stage_line(line: &str) -> Option<TrackedFile> {
    let (metadata, path) = line.split_once('\t')?;
    let mode = metadata.split_whitespace().next()?;
    Some(TrackedFile {
        mode: mode.to_string(),
        path: path.to_string(),
    })
}

fn set_tracked_mode(path: &Path, mode: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = match mode {
            "100755" => fs::Permissions::from_mode(0o755),
            _ => fs::Permissions::from_mode(0o644),
        };
        fs::set_permissions(path, perms).with_context(|| format!("chmod {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

fn install_git_crypt_key(repo_root: &Path, raw_key: &[u8]) -> Result<()> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(repo_root)
        .output()
        .context("resolve git dir for Jnoccio unlock")?;
    if !output.status.success() {
        bail!("could not resolve git dir for {}", repo_root.display());
    }
    let text = String::from_utf8(output.stdout).context("parse git dir path")?;
    let git_dir = PathBuf::from(text.trim());
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        repo_root.join(git_dir)
    };
    let key_dir = git_dir.join("git-crypt").join("keys");
    fs::create_dir_all(&key_dir).with_context(|| format!("create {}", key_dir.display()))?;
    let key_path = key_dir.join("default");
    fs::write(&key_path, raw_key).with_context(|| format!("write {}", key_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod {}", key_path.display()))?;
    }
    Ok(())
}

fn temp_key_dir() -> Result<PathBuf> {
    let base = std::env::temp_dir();
    let pid = std::process::id();
    for attempt in 0..16u32 {
        let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos(),
            Err(_) => 0,
        };
        let candidate = base.join(format!("jnoccio-unlock-{pid}-{nanos}-{attempt}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err).context("create short-lived Jnoccio unlock directory"),
        }
    }
    bail!("could not create short-lived Jnoccio unlock directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bracketed_paste_digits() {
        let secret = "A".repeat(SECRET_LEN);
        assert_eq!(normalize_unlock_secret(&format!("200{secret}201")), secret);
    }

    #[test]
    fn strips_escape_and_noise() {
        let secret = "B".repeat(SECRET_LEN);
        assert_eq!(
            normalize_unlock_secret(&format!("\u{1b}[200~\n{secret}\n\u{1b}[201~")),
            secret
        );
    }

    #[test]
    fn validates_secret_shape() {
        assert!(is_valid_unlock_secret(&"a".repeat(SECRET_LEN)));
        assert!(!is_valid_unlock_secret(&"a".repeat(SECRET_LEN - 1)));
        assert!(!is_valid_unlock_secret(&format!(
            "{}!",
            "a".repeat(SECRET_LEN - 1)
        )));
    }
}

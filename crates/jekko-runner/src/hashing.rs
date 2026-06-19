//! Shared hashing helpers for storage-safe receipts.

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Return a lowercase SHA-256 hex digest for raw bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Return a stable SHA-256 digest over JSON serialization.
pub fn sha256_json<T: Serialize>(value: &T, error_prefix: &str) -> String {
    let bytes = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(error) => {
            let type_name = std::any::type_name::<T>();
            format!("{error_prefix}_serialization_error:{type_name}:{error}").into_bytes()
        }
    };
    sha256_hex(&bytes)
}

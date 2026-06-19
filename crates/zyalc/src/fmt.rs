//! `zyalfmt` — a conservative, idempotent ZYAL source formatter (M17-cont).
//!
//! It normalizes WHITESPACE only — it never restructures the document, so
//! comments, the `# zyal:` pragma, key order, and generated IDs are all
//! preserved byte-for-byte except for the normalizations below. Restructuring
//! (key sorting / duration normalization) would require a comment-aware YAML
//! round-trip that serde_yaml can't provide, so it is deliberately out of scope.
//!
//! Normalizations (all idempotent):
//!   * leading tabs → 2 spaces each (YAML forbids tab indentation),
//!   * trailing whitespace stripped,
//!   * runs of ≥2 blank lines collapsed to 1,
//!   * leading/trailing blank lines removed,
//!   * exactly one trailing newline.
//!
//! Idempotency holds: a formatted document has no leading tabs, no trailing
//! whitespace, no double blanks, and a single trailing newline, so `format(x)`
//! is a fixed point.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// Format ZYAL source text. Pure + idempotent.
pub fn format_zyal(src: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut pending_blank = false;
    for raw in src.lines() {
        let expanded = expand_leading_tabs(raw);
        let line = expanded.trim_end();
        if line.is_empty() {
            pending_blank = true;
            continue;
        }
        // emit at most ONE blank line between content lines (and never leading).
        if pending_blank && !out.is_empty() {
            out.push(String::new());
        }
        pending_blank = false;
        out.push(line.to_string());
    }
    let mut s = out.join("\n");
    s.push('\n'); // exactly one trailing newline (empty input → just "\n"? no — see below)
    if out.is_empty() {
        return String::new();
    }
    s
}

/// Expand ONLY leading tabs to 2 spaces each (YAML disallows tab indentation);
/// tabs inside values are left untouched.
fn expand_leading_tabs(line: &str) -> String {
    let indent_end = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    let (indent, rest) = line.split_at(indent_end);
    let mut expanded = String::with_capacity(line.len());
    for c in indent.chars() {
        if c == '\t' {
            expanded.push_str("  ");
        } else {
            expanded.push(c);
        }
    }
    expanded.push_str(rest);
    expanded
}

/// The outcome of formatting one file.
#[derive(Debug, PartialEq, Eq)]
pub enum FmtOutcome {
    /// Already formatted — no change.
    Unchanged,
    /// Rewrote the file to the formatted form.
    Formatted,
    /// `--check` mode: the file is NOT formatted (would change).
    Drift,
}

/// Format a `.zyal` file in place (or, in `check` mode, report drift without
/// writing).
pub fn fmt_file(path: &Path, check: bool) -> Result<FmtOutcome> {
    let src = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let formatted = format_zyal(&src);
    if formatted == src {
        return Ok(FmtOutcome::Unchanged);
    }
    if check {
        return Ok(FmtOutcome::Drift);
    }
    fs::write(path, &formatted).with_context(|| format!("write {}", path.display()))?;
    Ok(FmtOutcome::Formatted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_trailing_ws_and_collapses_blank_runs() {
        // trailing whitespace removed; a run of 3 blank lines collapses to 1.
        assert_eq!(
            format_zyal("key: 1  \n\n\n\nother: 2\n"),
            "key: 1\n\nother: 2\n"
        );
    }

    #[test]
    fn expands_leading_tabs_to_two_spaces() {
        // a tab-indented child becomes 2-space-indented under its parent.
        assert_eq!(format_zyal("job:\n\tname: y\n"), "job:\n  name: y\n");
        assert_eq!(format_zyal("a:\n\t\tdeep: 1\n"), "a:\n    deep: 1\n");
    }

    #[test]
    fn is_idempotent() {
        let inputs = [
            "a: 1\n  b: 2\n",
            "# comment\n\n\n\nx: 1\t\n",
            "\t\tnested: deep\n",
            "no-trailing-newline: true",
            "",
        ];
        for input in inputs {
            let once = format_zyal(input);
            let twice = format_zyal(&once);
            assert_eq!(once, twice, "format must be a fixed point for {input:?}");
        }
    }

    #[test]
    fn preserves_comments_and_pragma_and_content() {
        let input =
            "# zyal: declarative target=flowgraph schema=zyal/flowgraph@1\n# a note\nid: keep_me\n";
        let out = format_zyal(input);
        assert!(out.contains("# zyal: declarative target=flowgraph"));
        assert!(out.contains("# a note"));
        assert!(out.contains("id: keep_me"));
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(format_zyal(""), "");
        assert_eq!(format_zyal("\n\n\n"), "");
    }
}

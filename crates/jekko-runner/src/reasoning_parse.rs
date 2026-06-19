//! Best-effort JSON extraction for structured model outputs.
//!
//! Models routinely wrap their JSON answers in prose, markdown code fences,
//! or trailing commentary. `parse_structured_model_json` tries the bare-text
//! parse first, then walks the buffer looking for the first balanced
//! `{...}` or `[...]` span that parses cleanly. Split out of
//! `reasoning_io.rs` to keep that file under the 500-LOC ceiling.

/// Parse JSON from a model response, falling back to first-balanced-span
/// extraction when the text is wrapped in prose.
pub(crate) fn parse_structured_model_json(text: &str) -> serde_json::Result<serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => Ok(value),
        Err(primary) => {
            for (start, ch) in text.char_indices() {
                if !matches!(ch, '{' | '[') {
                    continue;
                }
                let Some(end) = find_balanced_json_end(text, start) else {
                    continue;
                };
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text[start..=end]) {
                    return Ok(value);
                }
            }
            Err(primary)
        }
    }
}

/// Walk a buffer from `start` (a `{` or `[`) and return the byte index of the
/// matching close brace, or `None` if the structure is unbalanced. Respects
/// string literals + escaped chars so `}` inside a string doesn't close the
/// outer span.
fn find_balanced_json_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        let idx = start + offset;
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::parse_structured_model_json;

    #[test]
    fn parse_structured_model_json_accepts_wrapped_object() {
        let text =
            "Here is the JSON: {\"answer\":true,\"count\":2}\nExtra notes: ignore this {not json}";
        let value = parse_structured_model_json(text).expect("wrapped JSON should parse");
        assert_eq!(value["answer"], true);
        assert_eq!(value["count"], 2);
    }
}

use serde_json::Value;

use crate::adapter::ProviderRequest;

#[derive(Debug, Clone)]
pub(super) struct TemplateContext {
    last_user_text: String,
    first_path: String,
}

impl TemplateContext {
    pub(super) fn from_request(req: &ProviderRequest) -> Self {
        let last_user_text = last_message_text(&req.messages, "user").unwrap_or_default();
        let first_path = match first_absolute_path(&last_user_text) {
            Some(path) => path,
            None => String::from("README.md"),
        };
        Self {
            last_user_text,
            first_path,
        }
    }

    pub(super) fn expand_str(&self, input: &str) -> String {
        input
            .replace("{{last_user_text}}", &self.last_user_text)
            .replace("{{first_path}}", &self.first_path)
    }

    pub(super) fn expand_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.expand_str(s)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.expand_value(v)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.expand_value(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

fn last_message_text(messages: &[Value], role: &str) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some(role))
        .and_then(|message| content_text(message.get("content")?))
}

fn content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                if let Some(s) = part
                    .get("text")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
                {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(s);
                }
            }
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

pub(super) fn has_tool_result(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("tool")
            || message
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| part.get("type").and_then(Value::as_str) == Some("tool-result"))
                })
    })
}

fn first_absolute_path(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| {
            token
                .trim_matches(|c: char| matches!(c, ',' | '.' | ':' | ';' | ')' | '(' | '"' | '\''))
        })
        .find(|token| token.starts_with('/') && token.len() > 1)
        .map(str::to_string)
}

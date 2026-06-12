use super::*;
use crate::adapter::{ProviderCredential, ProviderTool};
use crate::providers::shared::test_request_with;

fn req() -> ProviderRequest {
    test_request_with(
        "anthropic/claude-sonnet-4-5",
        "claude-sonnet-4-5-20250901",
        ProviderCredential::ApiKey {
            key: "anthropic-sample-key".into(),
        },
        None,
        Some(0.5),
    )
}

#[test]
fn body_contains_required_fields() {
    let a = AnthropicAdapter::new();
    let body = a.build_body(&req());
    assert_eq!(body["model"], "claude-sonnet-4-5-20250901");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(body["stream"], true);
    assert_eq!(body["system"][0]["type"], "text");
    assert_eq!(body["system"][0]["text"], "sys");
}

#[test]
fn headers_include_x_api_key() {
    let a = AnthropicAdapter::new();
    let h = a.build_headers(&req()).unwrap();
    assert_eq!(h.get("x-api-key").unwrap(), "anthropic-sample-key");
    assert_eq!(h.get("anthropic-version").unwrap(), "2023-06-01");
}

#[test]
fn body_omits_credential_field() {
    let a = AnthropicAdapter::new();
    let body = a.build_body(&req());
    let body_obj = body.as_object().unwrap();
    assert!(!body_obj.contains_key("credential"));
    assert!(!body_obj.contains_key("x-api-key"));
}

#[test]
fn tool_definition_maps_correctly() {
    let mut r = req();
    r.tools.push(ProviderTool {
        name: "Read".into(),
        description: Some("Read file".into()),
        input_schema: json!({ "type": "object", "properties": { "path": { "type": "string" } } }),
    });
    r.tool_choice = Some("auto".into());
    let a = AnthropicAdapter::new();
    let body = a.build_body(&r);
    assert_eq!(body["tools"][0]["name"], "Read");
    assert_eq!(body["tools"][0]["description"], "Read file");
    assert_eq!(body["tool_choice"]["type"], "auto");
}

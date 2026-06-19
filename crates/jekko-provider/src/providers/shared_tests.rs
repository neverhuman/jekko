use std::collections::BTreeMap;

use super::*;
use crate::adapter::{ProviderCredential, ProviderRequest};

#[test]
fn build_openai_style_helpers_cover_body_and_headers() {
    let req = ProviderRequest {
        model: "openai/gpt-4.1".into(),
        api_model_id: "gpt-4.1".into(),
        session_id: "sess-1".into(),
        system: vec!["system prompt".into()],
        messages: vec![json!({ "role": "user", "content": "hi" })],
        tools: vec![],
        tool_choice: None,
        options: serde_json::Map::new(),
        headers: BTreeMap::new(),
        max_output_tokens: 256,
        temperature: Some(0.2),
        top_p: None,
        top_k: None,
        credential: Some(ProviderCredential::ApiKey {
            key: "demo-key".into(),
        }),
        base_url: None,
    };

    let body = build_openai_style_body(&req);
    assert_eq!(body["model"], "gpt-4.1");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "system prompt");

    let headers = build_openai_style_headers(&req, "openai").unwrap();
    assert_eq!(
        headers
            .get(reqwest::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer demo-key")
    );
}

#[test]
fn quality_band_in_options_lands_at_request_top_level() {
    // FIX-CAND-M: ZYAL declares quality_band on a stage's model_policy
    // role; jekko-runner sets JEKKO_RUN_QUALITY_BAND; jekko run
    // injects it into provider_options; the body builder must hoist
    // it to a top-level field so fusion's RequestProfile reads it
    // from `extra`.
    let mut options = serde_json::Map::new();
    options.insert(
        "quality_band".into(),
        serde_json::Value::String("top20".into()),
    );
    let req = ProviderRequest {
        model: "jnoccio/jnoccio-fusion".into(),
        api_model_id: "jnoccio-fusion".into(),
        session_id: "s-1".into(),
        system: vec![],
        messages: vec![json!({"role":"user","content":"hi"})],
        tools: vec![],
        tool_choice: None,
        options,
        headers: BTreeMap::new(),
        max_output_tokens: 256,
        temperature: None,
        top_p: None,
        top_k: None,
        credential: None,
        base_url: None,
    };
    let body = build_openai_style_body(&req);
    assert_eq!(body["quality_band"], "top20");
}

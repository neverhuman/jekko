use super::*;
use futures_util::StreamExt;
use serde_json::{json, Map};

fn request(model: &str) -> ProviderRequest {
    ProviderRequest {
        model: format!("scripted_agent/{model}"),
        api_model_id: model.to_string(),
        session_id: "sess-1".into(),
        system: vec![],
        messages: vec![json!({ "role": "user", "content": "inspect /tmp/example.txt" })],
        tools: vec![],
        tool_choice: None,
        options: Map::new(),
        headers: Default::default(),
        max_output_tokens: 1024,
        temperature: None,
        top_p: None,
        top_k: None,
        credential: None,
        base_url: None,
    }
}

#[test]
fn fixtures_are_strictly_valid() {
    let loaded = scenarios().unwrap();
    assert_eq!(loaded.len(), 3);
    assert!(loaded.iter().any(|scenario| scenario.id == "basic"));
    assert!(loaded.iter().any(|scenario| scenario.id == "tool-read"));
    assert!(loaded.iter().any(|scenario| scenario.id == "failure"));
}

#[tokio::test]
async fn basic_scenario_is_deterministic() {
    let adapter = ScriptedAgentAdapter::new();
    let mut first = adapter
        .stream(request("basic"), CancellationToken::new())
        .await
        .unwrap();
    let mut second = adapter
        .stream(request("basic"), CancellationToken::new())
        .await
        .unwrap();

    let mut first_text = String::new();
    while let Some(event) = first.next().await {
        if let ProviderEventKind::TextDelta { text } = event.unwrap().kind {
            first_text.push_str(&text);
        }
    }
    let mut second_text = String::new();
    while let Some(event) = second.next().await {
        if let ProviderEventKind::TextDelta { text } = event.unwrap().kind {
            second_text.push_str(&text);
        }
    }
    assert_eq!(first_text, second_text);
    assert!(first_text.contains("inspect /tmp/example.txt"));
}

#[tokio::test]
async fn tool_scenario_expands_first_path() {
    let adapter = ScriptedAgentAdapter::new();
    let mut stream = adapter
        .stream(request("tool-read"), CancellationToken::new())
        .await
        .unwrap();
    let mut input = None;
    while let Some(event) = stream.next().await {
        if let ProviderEventKind::ToolCallEnd { input: value, .. } = event.unwrap().kind {
            input = Some(value);
        }
    }
    assert_eq!(input.unwrap()["filePath"], json!("/tmp/example.txt"));
}

#[tokio::test]
async fn failure_scenario_yields_provider_error() {
    let adapter = ScriptedAgentAdapter::new();
    let mut stream = adapter
        .stream(request("failure"), CancellationToken::new())
        .await
        .unwrap();
    let mut saw_error = false;
    while let Some(event) = stream.next().await {
        if let Err(ProviderError::ProviderEvent(message)) = event {
            assert!(message.contains("scripted failure"));
            saw_error = true;
        }
    }
    assert!(saw_error);
}

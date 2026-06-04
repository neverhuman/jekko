//! Integration tests for the agent runtime boundary.
//!
//! These tests live in a dedicated module so the per-seam files stay focused
//! on production code.

#![cfg(test)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream;
use jekko_provider::adapter::ProviderRequest;
use jekko_provider::stream::{ProviderEvent, ProviderEventKind};
use serde_json::json;
use serial_test::serial;
use tempfile::tempdir;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::agent::provider::{build_model, provider_adapter};
use crate::agent::{
    AgentExecutor, AgentTurnRequest, AgentTurnResult, ProviderAdapterResolver,
    ProviderAgentExecutor, RunRequest,
};
use crate::error::RuntimeResult;
use crate::Runtime;

#[derive(Debug, Default)]
struct MockExecutor {
    seen: Mutex<Vec<AgentTurnRequest>>,
}

#[async_trait]
impl AgentExecutor for MockExecutor {
    async fn execute(&self, request: AgentTurnRequest) -> RuntimeResult<AgentTurnResult> {
        self.seen.lock().await.push(request);
        Ok(AgentTurnResult {
            provider_id: "mock".into(),
            model_id: "mock-model".into(),
            assistant_text: "mock reply".into(),
            reasoning_text: "mock reasoning".into(),
            tool_calls: vec![json!({"id":"tool-1","name":"Read","input":{"path":"README.md"}})],
            credential_source_policy: None,
            selected_credential_user_id: None,
            credential_user_id: None,
            router_metadata: None,
        })
    }
}

#[derive(Debug, Default)]
struct ScriptedProviderAdapter {
    calls: AtomicUsize,
    requests: Mutex<Vec<ProviderRequest>>,
    tool_path: Mutex<Option<String>>,
}

#[async_trait]
impl jekko_provider::ProviderAdapter for ScriptedProviderAdapter {
    async fn stream(
        &self,
        req: ProviderRequest,
        _abort: CancellationToken,
    ) -> jekko_provider::ProviderResult<jekko_provider::ProviderStream> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().await.push(req.clone());
        let tool_path = self
            .tool_path
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| "/tmp/tool-loop.txt".into());
        if call == 0 {
            let events = vec![
                Ok(ProviderEvent::new(ProviderEventKind::StreamStart {
                    model: None,
                })),
                Ok(ProviderEvent::new(ProviderEventKind::ToolCallEnd {
                    id: "call-1".into(),
                    name: "read".into(),
                    input: json!({ "filePath": tool_path }),
                })),
                Ok(ProviderEvent::new(ProviderEventKind::StreamEnd {
                    stop_reason: None,
                })),
            ];
            return Ok(Box::pin(stream::iter(events)) as jekko_provider::ProviderStream);
        }

        let serialized = serde_json::to_string(&req.messages).unwrap();
        assert!(serialized.contains("tool-result"));
        let events = vec![
            Ok(ProviderEvent::new(ProviderEventKind::StreamStart {
                model: None,
            })),
            Ok(ProviderEvent::new(ProviderEventKind::TextDelta {
                text: "done".into(),
            })),
            Ok(ProviderEvent::new(ProviderEventKind::StreamEnd {
                stop_reason: None,
            })),
        ];
        Ok(Box::pin(stream::iter(events)) as jekko_provider::ProviderStream)
    }

    fn capabilities(&self) -> jekko_provider::stream::ProviderCapabilities {
        jekko_provider::stream::ProviderCapabilities {
            streaming: true,
            cache_control: false,
            tool_streaming: true,
        }
    }
}

#[derive(Debug)]
struct ScriptedResolver {
    adapter: Arc<ScriptedProviderAdapter>,
}

impl ProviderAdapterResolver for ScriptedResolver {
    fn resolve(
        &self,
        _provider_id: &str,
    ) -> RuntimeResult<Arc<dyn jekko_provider::ProviderAdapter>> {
        Ok(self.adapter.clone())
    }
}

#[tokio::test]
async fn run_oneshot_persists_prompt_and_turn() {
    let exec = Arc::new(MockExecutor::default());
    let rt = Runtime::with_agent_executor(exec);
    let result = rt
        .run_oneshot(RunRequest {
            prompt: "please check @planner and @/tmp/example.rs".into(),
            cwd: PathBuf::from("/work"),
            agent: Some("review".into()),
            provider: Some("anthropic".into()),
            model: Some("claude-sonnet-4-5".into()),
            ephemeral: false,
        })
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(result.parsed_prompt.agents, vec!["planner"]);
    assert_eq!(result.parsed_prompt.files, vec!["/tmp/example.rs"]);
    assert!(result.session.is_some());
    assert!(result.message.is_some());
    assert!(result.assistant_message.is_some());
    assert_eq!(result.assistant_text.as_deref(), Some("mock reply"));
    assert_eq!(result.provider_id.as_deref(), Some("mock"));
    assert_eq!(result.model_id.as_deref(), Some("mock-model"));
    assert_eq!(result.tool_calls.len(), 1);
}

#[tokio::test]
async fn run_oneshot_ephemeral_still_executes() {
    let exec = Arc::new(MockExecutor::default());
    let rt = Runtime::with_agent_executor(exec);
    let result = rt
        .run_oneshot(RunRequest {
            prompt: "check".into(),
            cwd: PathBuf::from("/work"),
            agent: None,
            provider: Some("anthropic".into()),
            model: Some("claude-sonnet-4-5".into()),
            ephemeral: true,
        })
        .await
        .unwrap();

    assert!(result.session.is_none());
    assert!(result.message.is_none());
    assert!(result.assistant_message.is_none());
    assert_eq!(result.assistant_text.as_deref(), Some("mock reply"));
    assert!(result.accepted);
}

#[tokio::test]
#[serial(jekko_mock_llm_env)]
async fn provider_executor_runs_a_tool_loop() {
    let dir = tempdir().unwrap();
    let tool_path = dir.path().join("tool-loop.txt");
    std::fs::write(&tool_path, "alpha\nbeta\n").unwrap();

    let mut rt = Runtime::new();
    let adapter = Arc::new(ScriptedProviderAdapter::default());
    *adapter.tool_path.lock().await = Some(tool_path.to_string_lossy().to_string());
    let resolver = Arc::new(ScriptedResolver {
        adapter: adapter.clone(),
    });
    let executor =
        ProviderAgentExecutor::with_resolver(rt.permissions.clone(), rt.sessions.clone(), resolver);
    rt.agent_executor = Arc::new(executor);

    let result = rt
        .run_oneshot(RunRequest {
            prompt: format!("please inspect {}", tool_path.display()),
            cwd: dir.path().to_path_buf(),
            agent: Some("review".into()),
            provider: Some("openai".into()),
            model: Some("gpt-4o-mini".into()),
            ephemeral: true,
        })
        .await
        .unwrap();

    assert_eq!(result.assistant_text.as_deref(), Some("done"));
    assert!(result.tool_calls.is_empty());
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
    let requests = adapter.requests.lock().await;
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn provider_executor_runs_dummy_agent_llm_without_credentials() {
    let rt = Runtime::new();
    let result = rt
        .run_oneshot(RunRequest {
            prompt: "summarize deterministic behavior".into(),
            cwd: PathBuf::from("/work"),
            agent: None,
            provider: Some("dummy_agent_llm".into()),
            model: None,
            ephemeral: true,
        })
        .await
        .unwrap();

    assert_eq!(result.provider_id.as_deref(), Some("dummy_agent_llm"));
    assert_eq!(result.model_id.as_deref(), Some("basic"));
    let assistant_text = result.assistant_text.unwrap();
    assert!(assistant_text.contains("dummy_agent_llm/basic"));
    assert!(assistant_text.contains("summarize deterministic behavior"));
    assert!(result.accepted);
}

#[tokio::test]
async fn provider_executor_runs_dummy_agent_llm_tool_read_loop() {
    let dir = tempdir().unwrap();
    let tool_path = dir.path().join("dummy-tool-loop.txt");
    std::fs::write(&tool_path, "alpha\nbeta\n").unwrap();

    let rt = Runtime::new();
    let result = rt
        .run_oneshot(RunRequest {
            prompt: format!("please read {}", tool_path.display()),
            cwd: dir.path().to_path_buf(),
            agent: Some("review".into()),
            provider: Some("dummy_agent_llm".into()),
            model: Some("tool-read".into()),
            ephemeral: true,
        })
        .await
        .unwrap();

    assert_eq!(
        result.assistant_text.as_deref(),
        Some("dummy_agent_llm/tool-read: tool result received.")
    );
    assert_eq!(result.provider_id.as_deref(), Some("dummy_agent_llm"));
    assert_eq!(result.model_id.as_deref(), Some("tool-read"));
    assert!(result.tool_calls.is_empty());
}

#[test]
fn build_model_supports_jekko_provider() {
    let model = build_model("jekko", "big-pickle").unwrap();
    assert_eq!(model.provider_id.as_str(), "jekko");
    assert_eq!(model.api.npm, "@ai-sdk/openai-compatible");
    assert_eq!(model.api.url, "https://api.jekko.ai");
}

#[test]
fn build_model_supports_dummy_agent_llm_provider() {
    let model = build_model("dummy_agent_llm", "tool-read").unwrap();
    assert_eq!(model.provider_id.as_str(), "dummy_agent_llm");
    assert_eq!(model.api.id, "tool-read");
    assert_eq!(model.api.npm, "dummy_agent_llm");
    assert_eq!(model.api.url, "dummy://local");
    assert_eq!(model.cost.input, 0.0);
    assert_eq!(model.cost.output, 0.0);
}

#[test]
fn provider_adapter_supports_jekko() {
    let adapter = provider_adapter("jekko").unwrap();
    assert!(adapter.capabilities().streaming);
}

#[test]
fn provider_adapter_supports_dummy_agent_llm() {
    let adapter = provider_adapter("dummy_agent_llm").unwrap();
    let caps = adapter.capabilities();
    assert!(caps.streaming);
    assert!(caps.tool_streaming);
}

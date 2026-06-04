/// Default model used when no override is supplied via flag or env.
///
/// This MUST be the model the local jnoccio-fusion gateway exposes on
/// `/v1/chat/completions`. The gateway publishes a single visible model
/// (`jnoccio/jnoccio-fusion`) and routes internally across the keyed provider
/// pool; requesting any other id (e.g. a raw `anthropic/...` model) makes the
/// gateway answer `404 model_not_found`, which the TUI surfaces as
/// `jnoccio gateway returned non-200 status: 404`. Override with
/// [`MODEL_ENV`] only with another id the gateway actually exposes.
pub const DEFAULT_MODEL: &str = "jnoccio/jnoccio-fusion";

/// Environment variable consulted for the default model when the caller does
/// not pass one explicitly. Mirrors the rest of the jekko stack.
pub const MODEL_ENV: &str = "JEKKO_CHAT_MODEL";

/// Wrapper backend that posts to the local jnoccio-fusion gateway over the
/// SSE worker defined in [`crate::chat_bridge`].
pub struct ChatBridgeBackend {
    config: ChatBridgeConfig,
    /// T-INLINE-CLUSTER #11 / T-SANDBOX-ENF sidecar — see [`ChatBridgeRuntimePolicy`].
    policy: ChatBridgeRuntimePolicy,
    recall_hook: Option<Arc<dyn RecallHook>>,
}

/// Optional memory hook for session recall and turn observation.
pub trait RecallHook: Send + Sync {
    /// Return a recalled memory block for `prompt`, or `None` when no relevant
    /// memory exists.
    fn recall_block(&self, prompt: &str) -> Option<String>;
    /// Observe the current user prompt after recall has been computed.
    fn observe_user(&self, prompt: &str);
    /// Observe the final assistant text for the turn.
    fn observe_assistant(&self, text: &str);
}

/// Configuration knobs surfaced to callers. Wire format / gateway URL are
/// resolved inside `chat_bridge` from process env (`JEKKO_CHAT_GATEWAY_*`) so
/// this struct only carries the bits the runtime itself needs.
#[derive(Clone, Debug)]
pub struct ChatBridgeConfig {
    pub model: String,
}

impl Default for ChatBridgeConfig {
    fn default() -> Self {
        let model = match std::env::var(MODEL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            Some(value) => value,
            None => DEFAULT_MODEL.to_string(),
        };
        Self { model }
    }
}

/// T-INLINE-CLUSTER #11 / T-SANDBOX-ENF coordination knobs. Kept as a
/// separate, non-breaking sidecar so the existing `ChatBridgeConfig` initialiser
/// (`ChatBridgeConfig { model }`) at every caller continues to compile while
/// T3-A4b owns CLI plumbing. The T-SANDBOX-ENF agent reads from here when
/// constructing the runner-level `SandboxPolicy`.
#[derive(Clone, Debug, Default)]
pub struct ChatBridgeRuntimePolicy {
    /// Raw CLI value of `--sandbox`. `None` ⇒ runner picks the platform default.
    pub sandbox_profile: Option<String>,
    /// Raw CLI value of `--ask-for-approval`. `None` ⇒ runner picks default.
    pub approval_mode: Option<String>,
    /// Raw CLI value of `--permission-mode` (Claude-compatible).
    pub permission_mode: Option<String>,
}

impl ChatBridgeBackend {
    pub fn new(config: ChatBridgeConfig) -> Self {
        Self {
            config,
            policy: ChatBridgeRuntimePolicy::default(),
            recall_hook: None,
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(ChatBridgeConfig::default())
    }

    /// T-INLINE-CLUSTER #11: attach the sandbox/approval/permission policy
    /// derived from `--sandbox` / `--ask-for-approval` / `--permission-mode`.
    /// T-SANDBOX-ENF consumes this when it builds the runner.
    pub fn with_runtime_policy(mut self, policy: ChatBridgeRuntimePolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Attach live session memory recall/observation.
    pub fn with_recall_hook(mut self, hook: Arc<dyn RecallHook>) -> Self {
        self.recall_hook = Some(hook);
        self
    }

    /// T-INLINE-CLUSTER #11 / T-SANDBOX-ENF: expose the resolved policy.
    pub fn policy(&self) -> &ChatBridgeRuntimePolicy {
        &self.policy
    }
}

impl ChatBackend for ChatBridgeBackend {
    fn start_turn(&mut self, prompt: String, cancel: CancellationToken) -> Receiver<ChatEvent> {
        let (action_tx, action_rx) = mpsc::channel::<Action>();
        let (event_tx, event_rx) = mpsc::channel::<ChatEvent>();
        let recalled_context = self
            .recall_hook
            .as_ref()
            .and_then(|hook| hook.recall_block(&prompt))
            .filter(|block| !block.trim().is_empty());
        if let Some(hook) = &self.recall_hook {
            hook.observe_user(&prompt);
        }

        spawn_chat_request(
            prompt,
            self.config.model.clone(),
            action_tx,
            cancel,
            recalled_context,
        );

        let recall_hook = self.recall_hook.clone();
        std::thread::Builder::new()
            .name("jekko-chat-bridge-translator".into())
            .spawn(move || {
                let mut tool_stdout: HashMap<String, String> = HashMap::new();
                let mut assistant_text = String::new();
                for action in action_rx {
                    let events = translate_action_stateful(action, &mut tool_stdout);
                    let mut hit_terminal = false;
                    for evt in events {
                        if let ChatEvent::AssistantDelta(text) = &evt {
                            assistant_text.push_str(text);
                        }
                        let terminal =
                            matches!(evt, ChatEvent::TurnComplete | ChatEvent::TurnFailed(_));
                        if matches!(evt, ChatEvent::TurnComplete) {
                            if let Some(hook) = &recall_hook {
                                if !assistant_text.trim().is_empty() {
                                    hook.observe_assistant(&assistant_text);
                                }
                            }
                        }
                        if event_tx.send(evt).is_err() {
                            hit_terminal = true;
                            break;
                        }
                        if terminal {
                            hit_terminal = true;
                            break;
                        }
                    }
                    if hit_terminal {
                        break;
                    }
                }
            })
            .ok();

        event_rx
    }
}

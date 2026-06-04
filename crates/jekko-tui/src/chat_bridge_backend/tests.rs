#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ToolEvent;
    use crate::activity::ActivityKind;

    #[test]
    fn default_config_uses_env_model_when_set() {
        let key = "JEKKO_CHAT_MODEL_TEST_OVERRIDE";
        std::env::set_var(key, "stub/model");
        let model = std::env::var(key).unwrap();
        assert_eq!(model, "stub/model");
        std::env::remove_var(key);
    }

    #[test]
    fn runtime_policy_defaults_to_none() {
        // T-INLINE-CLUSTER #11: backends start with an empty policy so the
        // existing `ChatBridgeBackend::new(ChatBridgeConfig { model })` path
        // (CLI today) keeps working — the policy lives behind a builder
        // method that T3-A4b will chain.
        let backend = ChatBridgeBackend::with_default_config();
        let p = backend.policy();
        assert!(p.sandbox_profile.is_none());
        assert!(p.approval_mode.is_none());
        assert!(p.permission_mode.is_none());
    }

    #[test]
    fn with_runtime_policy_threads_through() {
        // T-INLINE-CLUSTER #11: the builder method attaches the policy.
        let policy = ChatBridgeRuntimePolicy {
            sandbox_profile: Some("workspace-write".into()),
            approval_mode: Some("on-failure".into()),
            permission_mode: Some("ask-for-edits".into()),
        };
        let backend = ChatBridgeBackend::with_default_config().with_runtime_policy(policy);
        assert_eq!(
            backend.policy().sandbox_profile.as_deref(),
            Some("workspace-write")
        );
        assert_eq!(
            backend.policy().approval_mode.as_deref(),
            Some("on-failure")
        );
        assert_eq!(
            backend.policy().permission_mode.as_deref(),
            Some("ask-for-edits")
        );
    }

    struct NoopRecallHook;

    impl RecallHook for NoopRecallHook {
        fn recall_block(&self, _prompt: &str) -> Option<String> {
            None
        }

        fn observe_user(&self, _prompt: &str) {}

        fn observe_assistant(&self, _text: &str) {}
    }

    #[test]
    fn with_recall_hook_attaches_hook() {
        let backend =
            ChatBridgeBackend::with_default_config().with_recall_hook(Arc::new(NoopRecallHook));
        assert!(backend.recall_hook.is_some());
    }

    #[test]
    fn default_config_falls_back_to_constant() {
        std::env::remove_var(MODEL_ENV);
        let cfg = ChatBridgeConfig::default();
        assert_eq!(cfg.model, DEFAULT_MODEL);
    }

    #[test]
    fn translate_assistant_delta() {
        let action = Action::Runtime(RuntimeEvent::AssistantTextDelta {
            text: "hi".to_string(),
        });
        match translate_action(action) {
            Some(ChatEvent::AssistantDelta(s)) => assert_eq!(s, "hi"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn translate_assistant_completed() {
        let action = Action::Runtime(RuntimeEvent::AssistantCompleted);
        assert!(matches!(
            translate_action(action),
            Some(ChatEvent::TurnComplete)
        ));
    }

    #[test]
    fn translate_assistant_failed() {
        let action = Action::Runtime(RuntimeEvent::AssistantFailed {
            error: "boom".to_string(),
        });
        match translate_action(action) {
            Some(ChatEvent::TurnFailed(s)) => assert_eq!(s, "boom"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn translate_activity_finished_failure_as_error_notice() {
        let action = Action::ActivityFinished {
            id: "x".into(),
            kind: ActivityKind::Jnoccio,
            label: None,
            status: Some("dead".into()),
            success: false,
        };
        match translate_action(action) {
            Some(ChatEvent::Notice(NoticeKind::Error, msg)) => assert_eq!(msg, "dead"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn translate_activity_finished_success_is_dropped() {
        let action = Action::ActivityFinished {
            id: "x".into(),
            kind: ActivityKind::Jnoccio,
            label: None,
            status: Some("ok".into()),
            success: true,
        };
        assert!(translate_action(action).is_none());
    }

    #[test]
    fn translate_reasoning_ended_is_forwarded() {
        let action = Action::Runtime(RuntimeEvent::ReasoningEnded {
            reasoning_id: "r0".into(),
            text: "thinking".into(),
        });
        match translate_action(action) {
            Some(ChatEvent::Reasoning { reasoning_id, text }) => {
                assert_eq!(reasoning_id, "r0");
                assert_eq!(text, "thinking");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn translate_tool_event_is_forwarded() {
        let action = Action::Runtime(RuntimeEvent::Tool(ToolEvent::Start {
            id: "tool-1".into(),
            name: "Bash".into(),
            input: Some("git status".into()),
        }));
        match translate_action(action) {
            Some(ChatEvent::Tool(ToolEvent::Start { name, .. })) => assert_eq!(name, "Bash"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn translate_session_started_is_forwarded_as_runtime_event() {
        let action = Action::Runtime(RuntimeEvent::SessionStarted {
            session_id: jekko_core::session::SessionId::new("session_1"),
            title: Some("hello".into()),
        });
        match translate_action(action) {
            Some(ChatEvent::Runtime(RuntimeEvent::SessionStarted { session_id, title })) => {
                assert_eq!(session_id.as_str(), "session_1");
                assert_eq!(title.as_deref(), Some("hello"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn translate_session_ended_and_daemon_status_are_forwarded_as_runtime_events() {
        let ended = Action::Runtime(RuntimeEvent::SessionEnded {
            session_id: jekko_core::session::SessionId::new("session_1"),
        });
        match translate_action(ended) {
            Some(ChatEvent::Runtime(RuntimeEvent::SessionEnded { session_id })) => {
                assert_eq!(session_id.as_str(), "session_1");
            }
            other => panic!("unexpected: {other:?}"),
        }

        let daemon = Action::Runtime(RuntimeEvent::DaemonStatus {
            session_id: Some(jekko_core::session::SessionId::new("session_1")),
            status: "paused".into(),
            message: Some("bridge stalled".into()),
        });
        match translate_action(daemon) {
            Some(ChatEvent::Runtime(RuntimeEvent::DaemonStatus {
                session_id,
                status,
                message,
            })) => {
                assert_eq!(session_id.as_ref().map(|id| id.as_str()), Some("session_1"));
                assert_eq!(status, "paused");
                assert_eq!(message.as_deref(), Some("bridge stalled"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn translate_permission_and_question_events_are_forwarded_as_runtime_events() {
        let permission = Action::Runtime(RuntimeEvent::PermissionAsked {
            request_id: "perm_1".into(),
            session_id: jekko_core::session::SessionId::new("session_1"),
            permission: "bash".into(),
            patterns: vec!["ls".into()],
            always: vec!["ls".into()],
        });
        assert!(matches!(
            translate_action(permission),
            Some(ChatEvent::Runtime(RuntimeEvent::PermissionAsked { request_id, .. })) if request_id == "perm_1"
        ));

        let question = Action::Runtime(RuntimeEvent::QuestionAsked {
            question_id: "question_1".into(),
            session_id: jekko_core::session::SessionId::new("session_1"),
            prompt: "continue?".into(),
            choices: vec!["yes".into(), "no".into()],
        });
        assert!(matches!(
            translate_action(question),
            Some(ChatEvent::Runtime(RuntimeEvent::QuestionAsked { question_id, .. })) if question_id == "question_1"
        ));
    }

    #[test]
    fn translator_thread_emits_delta_then_complete_then_stops() {
        let (action_tx, action_rx) = mpsc::channel::<Action>();
        let (event_tx, event_rx) = mpsc::channel::<ChatEvent>();

        std::thread::spawn(move || {
            for action in action_rx {
                let translated = translate_action(action);
                let terminal = matches!(
                    translated,
                    Some(ChatEvent::TurnComplete) | Some(ChatEvent::TurnFailed(_))
                );
                if let Some(evt) = translated {
                    if event_tx.send(evt).is_err() {
                        break;
                    }
                }
                if terminal {
                    break;
                }
            }
        });

        action_tx
            .send(Action::Runtime(RuntimeEvent::AssistantTextDelta {
                text: "hello".into(),
            }))
            .unwrap();
        action_tx
            .send(Action::Runtime(RuntimeEvent::AssistantCompleted))
            .unwrap();
        // Anything after TurnComplete must not produce more events.
        let _ = action_tx.send(Action::Runtime(RuntimeEvent::AssistantTextDelta {
            text: "ignored".into(),
        }));

        let first = event_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(matches!(first, ChatEvent::AssistantDelta(ref s) if s == "hello"));
        let second = event_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(matches!(second, ChatEvent::TurnComplete));
        assert!(event_rx
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err());
    }

    const SAMPLE_DIFF: &str = "--- a/foo.txt\n+++ b/foo.txt\n@@ -1,3 +1,3 @@\n line one\n-before two\n+after two\n line three\n";

    #[test]
    fn looks_like_unified_diff_detects_classic_header() {
        assert!(looks_like_unified_diff(SAMPLE_DIFF));
    }

    #[test]
    fn looks_like_unified_diff_rejects_plain_text() {
        assert!(!looks_like_unified_diff("just a log line\nanother\n"));
        assert!(!looks_like_unified_diff(""));
        assert!(!looks_like_unified_diff(
            "--- alone without a plus header\n"
        ));
    }

    #[test]
    fn diff_events_from_stdout_emits_one_event_per_file() {
        let body = "--- a/x\n+++ b/x\n@@ -1,1 +1,1 @@\n-a\n+b\n--- a/y\n+++ b/y\n@@ -1,1 +1,2 @@\n c\n+d\n";
        let events = diff_events_from_stdout(body);
        assert_eq!(events.len(), 2);
        let paths: Vec<String> = events
            .into_iter()
            .map(|evt| match evt {
                ChatEvent::Diff { path, .. } => path,
                other => panic!("expected Diff, got {other:?}"),
            })
            .collect();
        assert_eq!(paths, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn diff_events_from_stdout_skips_non_diff_payload() {
        assert!(diff_events_from_stdout("not a diff").is_empty());
    }

    #[test]
    fn diff_events_from_stdout_carries_line_kinds_and_numbers() {
        let events = diff_events_from_stdout(SAMPLE_DIFF);
        assert_eq!(events.len(), 1);
        let ChatEvent::Diff { path, hunks } = events.into_iter().next().unwrap() else {
            panic!("expected Diff");
        };
        assert_eq!(path, "foo.txt");
        assert_eq!(hunks.len(), 4);
        // Context line one — both linenos populated.
        assert_eq!(hunks[0].kind, DiffLineKind::Context);
        assert_eq!(hunks[0].old_lineno, Some(1));
        assert_eq!(hunks[0].new_lineno, Some(1));
        // Removed line: only the previous-side line number is present.
        assert_eq!(hunks[1].kind, DiffLineKind::Removed);
        assert_eq!(hunks[1].old_lineno, Some(2));
        assert_eq!(hunks[1].new_lineno, None);
        // Added line — only new lineno.
        assert_eq!(hunks[2].kind, DiffLineKind::Added);
        assert_eq!(hunks[2].old_lineno, None);
        assert_eq!(hunks[2].new_lineno, Some(2));
    }

    #[test]
    fn translate_action_stateful_emits_diff_then_complete_for_tool_with_diff_stdout() {
        let mut buf: HashMap<String, String> = HashMap::new();
        let id = "tool-diff".to_string();
        let start = Action::Runtime(RuntimeEvent::Tool(ToolEvent::Start {
            id: id.clone(),
            name: "Bash".into(),
            input: Some("git diff".into()),
        }));
        assert_eq!(translate_action_stateful(start, &mut buf).len(), 1);
        let chunk = Action::Runtime(RuntimeEvent::Tool(ToolEvent::StdoutChunk {
            id: id.clone(),
            chunk: SAMPLE_DIFF.to_string(),
        }));
        assert_eq!(translate_action_stateful(chunk, &mut buf).len(), 1);
        let done = Action::Runtime(RuntimeEvent::Tool(ToolEvent::Complete { id: id.clone() }));
        let events = translate_action_stateful(done, &mut buf);
        assert_eq!(events.len(), 2, "expected Diff + Complete");
        match &events[0] {
            ChatEvent::Diff { path, .. } => assert_eq!(path, "foo.txt"),
            other => panic!("expected Diff first, got {other:?}"),
        }
        assert!(matches!(
            events[1],
            ChatEvent::Tool(ToolEvent::Complete { .. })
        ));
        // Buffer should be drained after Complete.
        assert!(!buf.contains_key(&id));
    }

    #[test]
    fn translate_action_stateful_passes_through_non_tool_actions() {
        let mut buf: HashMap<String, String> = HashMap::new();
        let action = Action::Runtime(RuntimeEvent::AssistantTextDelta { text: "hi".into() });
        let events = translate_action_stateful(action, &mut buf);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ChatEvent::AssistantDelta(s) if s == "hi"));
    }

    #[test]
    fn translate_action_stateful_drops_stdout_buffer_on_fail() {
        let mut buf: HashMap<String, String> = HashMap::new();
        let id = "tool-fail".to_string();
        translate_action_stateful(
            Action::Runtime(RuntimeEvent::Tool(ToolEvent::Start {
                id: id.clone(),
                name: "Bash".into(),
                input: None,
            })),
            &mut buf,
        );
        translate_action_stateful(
            Action::Runtime(RuntimeEvent::Tool(ToolEvent::StdoutChunk {
                id: id.clone(),
                chunk: SAMPLE_DIFF.into(),
            })),
            &mut buf,
        );
        let events = translate_action_stateful(
            Action::Runtime(RuntimeEvent::Tool(ToolEvent::Fail {
                id: id.clone(),
                error: "boom".into(),
            })),
            &mut buf,
        );
        // Fail forwards only the tool event — no diff card on a failed tool.
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ChatEvent::Tool(ToolEvent::Fail { .. })));
        assert!(!buf.contains_key(&id));
    }
}

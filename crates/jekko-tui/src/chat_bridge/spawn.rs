/// Spawn a background worker that posts `prompt` to the local jnoccio-fusion
/// gateway and forwards each streamed text delta back through `action_tx`.
/// Returns immediately; the worker shuts down once the SSE stream closes or
/// on first I/O error. Retries once on 5xx to handle transient upstream blips.
pub fn spawn_chat_request(
    prompt: String,
    model: String,
    action_tx: Sender<Action>,
    cancel: CancellationToken,
    recalled_context: Option<String>,
) {
    std::thread::Builder::new()
        .name("jekko-tui-chat-bridge".into())
        .spawn(move || {
            let result = wait_for_gateway_ready(GATEWAY_HOST, GATEWAY_PORT, &cancel)
                .and_then(|_| {
                    run_chat(
                        &prompt,
                        &model,
                        &action_tx,
                        &cancel,
                        recalled_context.as_deref(),
                    )
                })
                .or_else(|e| {
                    if !cancel.is_cancelled() && e.to_string().contains("non-200") {
                        std::thread::sleep(Duration::from_secs(2));
                        run_chat(
                            &prompt,
                            &model,
                            &action_tx,
                            &cancel,
                            recalled_context.as_deref(),
                        )
                    } else {
                        Err(e)
                    }
                });
            if let Err(e) = result {
                let _ = action_tx.send(Action::Runtime(RuntimeEvent::AssistantFailed {
                    error: if cancel.is_cancelled() {
                        "cancelled".to_string()
                    } else {
                        e.to_string()
                    },
                }));
            }
            let _ = action_tx.send(Action::Runtime(RuntimeEvent::AssistantCompleted));
        })
        .ok();
}

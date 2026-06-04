#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;

    fn fake_sse_response() -> String {
        let chunks = [
            r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#,
            "",
            r#"data: {"choices":[{"delta":{"content":" world"}}]}"#,
            "",
            "data: [DONE]",
            "",
        ];
        let body = format!("{}\n", chunks.join("\n"));
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }

    #[test]
    fn run_chat_parses_sse_deltas() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
            let _ = sock.read(&mut buf);
            sock.write_all(fake_sse_response().as_bytes()).unwrap();
        });

        let (tx, rx) = mpsc::channel::<Action>();
        run_chat_custom(addr.ip().to_string().as_str(), addr.port(), "hi", &tx).unwrap();

        let deltas: Vec<String> = rx
            .try_iter()
            .filter_map(|a| match a {
                Action::Runtime(RuntimeEvent::AssistantTextDelta { text }) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, vec!["Hello".to_string(), " world".to_string()]);
    }

    #[test]
    fn request_body_includes_recalled_memory_as_system_context() {
        let body = chat_request_body(
            "continue the session",
            DEFAULT_MODEL,
            Some("=== RECALLED MEMORY (CONTEXT ONLY) ===\nprior fact"),
        );
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        let messages = value["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains(RECALLED_MEMORY_POLICY));
        assert!(messages[0]["content"].as_str().unwrap().contains("prior fact"));
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "continue the session");
    }

    #[test]
    fn request_body_omits_system_context_when_recall_empty() {
        let body = chat_request_body("fresh question", DEFAULT_MODEL, Some("   "));
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        let messages = value["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn wait_for_gateway_ready_retries_until_listener_arrives() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let accepted = std::sync::Arc::new(AtomicBool::new(false));
        let accepted_thread = accepted.clone();
        let server = thread::spawn(move || {
            thread::sleep(Duration::from_millis(120));
            let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
            let (mut sock, _) = listener.accept().unwrap();
            accepted_thread.store(true, Ordering::SeqCst);
            let mut buf = [0u8; 1];
            let _ = sock.read(&mut buf);
        });

        let cancel = CancellationToken::new();
        let started = Instant::now();
        wait_for_gateway_ready_with_budget(
            "127.0.0.1",
            port,
            &cancel,
            Duration::from_millis(1_500),
            Duration::from_millis(25),
        )
        .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(100));
        let deadline = Instant::now() + Duration::from_secs(1);
        while !accepted.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < deadline,
                "listener never accepted the probe connection"
            );
            thread::sleep(Duration::from_millis(10));
        }
        server.join().unwrap();
    }

    #[test]
    fn wait_for_gateway_ready_respects_cancellation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(60));
            cancel_clone.cancel_hard();
        });

        let err = wait_for_gateway_ready_with_budget(
            "127.0.0.1",
            port,
            &cancel,
            Duration::from_millis(1_500),
            Duration::from_millis(200),
        )
        .unwrap_err();

        assert_eq!(err.kind(), ErrorKind::Interrupted);
        canceller.join().unwrap();
    }

    fn run_chat_custom(
        host: &str,
        port: u16,
        prompt: &str,
        tx: &Sender<Action>,
    ) -> std::io::Result<()> {
        let body = serde_json::json!({
            "model": DEFAULT_MODEL,
            "stream": true,
            "messages": [{ "role": "user", "content": prompt }]
        })
        .to_string();
        let mut stream = TcpStream::connect((host, port))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let request = format!(
            "POST {p} HTTP/1.1\r\nHost: {h}:{port}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nContent-Length: {l}\r\nConnection: close\r\n\r\n{b}",
            p = GATEWAY_PATH,
            h = host,
            port = port,
            l = body.len(),
            b = body
        );
        stream.write_all(request.as_bytes())?;
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        reader.read_line(&mut status)?;
        loop {
            let mut h = String::new();
            let n = reader.read_line(&mut h)?;
            if n == 0 || h == "\r\n" || h == "\n" {
                break;
            }
        }
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break;
            }
            let t = line.trim_end_matches(['\r', '\n']);
            let payload = match t.strip_prefix("data: ") {
                Some(p) => p,
                None => continue,
            };
            if payload == "[DONE]" {
                break;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(delta) = v
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("delta"))
                    .and_then(|d| d.get("content"))
                    .and_then(|t| t.as_str())
                {
                    let _ = tx.send(Action::Runtime(RuntimeEvent::AssistantTextDelta {
                        text: delta.to_string(),
                    }));
                }
            }
        }
        Ok(())
    }
}

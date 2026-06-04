/// Drive the inline runtime to completion. Returns when the user exits
/// (Ctrl+C / Esc with empty composer). Restores the terminal on every path.
///
/// Wraps the async [`run_loop_async`] in a current-thread tokio runtime so the
/// sync `cmd::chat::run` entry point doesn't need to know about tokio.
pub fn run_inline<B: ChatBackend>(mut backend: B, mut opts: InlineRuntimeOptions) -> Result<()> {
    let terminal_opts = AgentTerminalOptions {
        no_alt_screen: opts.no_alt_screen,
        mouse: opts
            .ui_config
            .as_ref()
            .and_then(|cfg| cfg.ui.mouse)
            .unwrap_or(false),
        ..AgentTerminalOptions::default()
    };
    let mut guard = TerminalRestoreGuard::for_agent(&terminal_opts);
    let mut terminal = enter_agent_terminal(&terminal_opts)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .context("build inline tokio runtime")?;
    let boot_rx = opts.jnoccio_boot_rx.take();
    let result = rt.block_on(run_loop_async(&mut terminal, &mut backend, &opts, boot_rx));

    let _ = leave_agent_terminal(terminal, &terminal_opts);
    guard.mark_restored();
    result
}

/// Bridge a per-turn `std::sync::mpsc::Receiver<ChatEvent>` from the existing
/// `ChatBackend` impls onto a tokio mpsc channel the async loop can `select!`
/// over. Runs the blocking recv on a dedicated blocking task; exits when the
/// backend signals turn completion.
fn bridge_chat_channel(std_rx: Receiver<ChatEvent>) -> tokio::sync::mpsc::Receiver<ChatEvent> {
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    tokio::task::spawn_blocking(move || {
        for evt in std_rx {
            let terminal_event = matches!(evt, ChatEvent::TurnComplete | ChatEvent::TurnFailed(_));
            if tx.blocking_send(evt).is_err() {
                return;
            }
            if terminal_event {
                return;
            }
        }
    });
    rx
}

async fn run_loop_async<B: ChatBackend>(
    terminal: &mut Tty,
    backend: &mut B,
    opts: &InlineRuntimeOptions,
    mut jnoccio_boot_rx: Option<Receiver<JnoccioBootStatus>>,
) -> Result<()> {
    let ctx = BootContext::detect();
    let term_width = terminal.size()?.width;
    let index_root = current_dir_or_dot();
    let index = FileIndex::build(&index_root, FILE_INDEX_MAX_ENTRIES);
    let catalog = SlashCatalog::new().with_user_commands(&index_root);
    let mode = if opts.no_alt_screen {
        RuntimeMode::NoAltScreen
    } else {
        RuntimeMode::Fullscreen
    };
    let mut transcript = Transcript::default();
    let mut scroll = ScrollState::default();
    let mut agent_panel = AgentPanelState::new();
    agent_panel.upsert(AgentRun::new_main("main", "awaiting first prompt"));
    let mut focus = FocusArea::Composer;

    // The splash overlay IS the welcome surface: the prior
    // `render_inline_boot_block` ("Welcome to JEKKO") gets pushed into the
    // transcript via `dismiss_splash_into_transcript` on first prompt
    // submit, so the wordmark scrolls UP into history instead of being
    // replaced by a plain-text header. `opts.boot_visible` left in place
    // for `--no-alt-screen` callers that don't render the splash overlay.
    if opts.boot_visible && opts.no_alt_screen {
        let boot = render_inline_boot_block(&ctx, term_width);
        emit_lines(terminal, mode, &mut transcript, &mut scroll, &boot)?;
    }
    if let Some(notice) = &opts.initial_notice {
        let lines = render_system_notice(NoticeKind::Info, notice);
        emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
    }

    let mut composer = ComposerState::default();
    let mut composer_empty_hint: Option<String> = None;
    let mut in_flight: Option<InFlight> = None;
    let mut chat_rx: Option<tokio::sync::mpsc::Receiver<ChatEvent>> = None;
    let mut splash = SplashState::new(SplashContext::detect());
    let mut jnoccio_boot = JnoccioBootRuntime::new(opts.jnoccio_boot_status.clone());
    // T-INLINE-CLUSTER #2: Pager overlay state. When `Some`, the runtime
    // intercepts ALL key events first and forwards them to the pager.
    let mut pager: Option<PagerState> = None;
    // T-INLINE-CLUSTER #2/#3/#4: Toast stack rendered into the lower-right
    // corner. Mutated by /copy, mouse-selection-end, and pager yank paths.
    let mut toasts = ToastStack::default();
    // T-INLINE-CLUSTER #3: Mouse selection start in cell coords. The end is
    // resolved at SelectionEnded; we don't need to retain it between events
    // because selection highlighting isn't rendered yet (follow-up).
    let mut selection_start: Option<(u16, u16)> = None;
    // T-INLINE-CLUSTER #9: Per-turn chat-history sidecar. Each completed turn
    // is recorded as `(user, assistant)` so `/compact` can replay the last N
    // verbatim and summarize the older ones through the backend.
    let mut chat_history: Vec<ChatTurnRecord> = Vec::new();
    let mut pending_user_prompt: Option<String> = None;
    // T-INLINE-CLUSTER #10: Idle redraw gate. `true` means the next loop
    // iteration must draw; ticks while idle skip the `terminal.draw` call.
    // Initial draw is always required so the splash shows up.
    let mut dirty = true;
    // T-INLINE-CLUSTER #2: cache the transcript rect across iterations so
    // mouse events emitted between draws can still resolve their target.
    let mut transcript_rect = Rect::default();
    // T-BG-COUNT-MANAGER: runtime-owned background-job registry. Drives the
    // working-strip `N background terminal(s) running` segment via
    // [`BackgroundJobManager::count`] (refreshed into `opts.background_count`
    // each frame below) and powers `/ps` + `/stop <id>`. `Arc` so future
    // spawners (e.g. `/run --background`) can hold their own handle without
    // a longer-lived borrow over the runtime state.
    let bg_manager: Arc<BackgroundJobManager> = Arc::new(BackgroundJobManager::new());
    // T-BG-COUNT-MANAGER: throttle the per-tick `sweep_completed` call so we
    // only trim every ~5s instead of every tick (which would acquire the
    // mutex on every frame for zero benefit).
    let mut last_bg_sweep = Instant::now();

    let mut input = EventStream::new();
    let mut ticker = tokio::time::interval(STREAM_FRAME_BUDGET);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // T-PERMISSIONS-PLUMB: runtime-owned permission/sandbox/approval state.
    // Seeded once from the CLI/loader-resolved `InlineRuntimeOptions` so the
    // chrome rail, `/permissions`, and `/sandbox` all read from a single source
    // of truth. Mutated by `/permissions` cycling (each invocation rotates
    // through `BypassPermissions → AskBeforeWrite → ReadOnly`).
    let mut permission_state = PermissionState::from_opts(opts)?;
    let motion_enabled = motion_enabled_with_cfg(opts.ui_config.as_ref());

    loop {
        if jnoccio_boot.drain_updates(&mut jnoccio_boot_rx) {
            dirty = true;
        }
        splash.ensure_started(Instant::now());
        // T-INLINE-CLUSTER #10 (refined T-INLINE-WAVE2): skip the
        // `terminal.draw` call when nothing has changed and nothing is
        // animating. Anything that mutates draw-relevant state must set
        // `dirty = true` somewhere along its handling path.
        //
        // `animation_active` triggers per-tick redraws (~30 fps). Pager,
        // toasts, and the startup splash are static surfaces — their
        // mutations are event-driven and handlers set `dirty = true`, so they
        // do not require per-frame redraws between events. This keeps idle CPU
        // near zero (COWBOY.md K5: < 0.5%).
        let animation_active = in_flight.is_some();
        if dirty || animation_active {
            // T-BG-COUNT-MANAGER: drive the working strip's background-count
            // segment from the runtime-owned [`BackgroundJobManager`] rather
            // than the static CLI-supplied seed. Falls back to
            // `opts.background_count` when no bg jobs are registered so the
            // CLI-side scaffold (which seeds 0 today) still behaves correctly.
            let live_bg_count = bg_manager.count() as u32;
            let resolved_bg_count = if live_bg_count > 0 {
                live_bg_count
            } else {
                opts.background_count
            };
            let jnoccio_boot_label = jnoccio_boot.footer_label();
            let draw_ctx = DrawContext {
                pager: pager.as_ref(),
                toasts: &toasts,
                // T-PERMISSIONS-PLUMB: derive the chrome rail label from the
                // runtime-owned [`PermissionState`] instead of the
                // hardcoded "bypass permissions" fallback. `/permissions`
                // cycling mutates `permission_state.mode` so this label updates
                // on the next frame.
                permission_mode_label: permission_state.mode.label(),
                profile: opts.profile.as_deref(),
                jnoccio_boot_label: jnoccio_boot_label.as_deref(),
                ui_config: opts.ui_config.as_ref(),
                // T-BG-COUNT-MANAGER: sourced from the runtime
                // background-job manager (see `bg_manager` above); falls
                // back to `InlineRuntimeOptions::background_count` for tests
                // / compatibility CLI-seeded scenarios when the manager is empty.
                background_count: resolved_bg_count,
                motion_enabled,
            };
            transcript_rect = draw(
                terminal,
                mode,
                &transcript,
                &scroll,
                &composer,
                in_flight.as_ref(),
                &agent_panel,
                focus,
                &index,
                &catalog,
                ctx.branch.as_deref(),
                &ctx,
                &splash,
                composer_empty_hint.as_deref(),
                draw_ctx,
            )?;
            dirty = false;
        }

        let event = tokio::select! {
            biased;
            // Streaming chat events take priority so deltas coalesce promptly.
            maybe_chat = async {
                match chat_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => LoopEvent::Chat(maybe_chat),
            maybe_input = input.next() => LoopEvent::Input(maybe_input),
            _ = ticker.tick() => LoopEvent::Tick,
        };

        match event {
            LoopEvent::Tick => {
                // T-BG-COUNT-MANAGER: trim completed/cancelled bg jobs every
                // ~5s so `/ps` stays focused. Runs unconditionally (independent
                // of `in_flight`) so background work that finishes while the
                // user isn't actively in a turn still gets swept; the call is
                // O(N) over a tiny N and acquires the mutex once, well below
                // the idle-CPU budget.
                if last_bg_sweep.elapsed() >= BG_SWEEP_INTERVAL {
                    bg_manager.sweep_completed(BG_RETAIN_AFTER_FINISH);
                    last_bg_sweep = Instant::now();
                }
                // T-INLINE-WAVE2 (idle gate refinement): if nothing animates,
                // skip the tick entirely. No `dirty = true`, no fall-through
                // work; the next loop iteration will hit the
                // `dirty || animation_active` guard at the top and skip the
                // `terminal.draw` call. Re-engages instantly on any key / mouse
                // / chat event because those arms set `dirty = true` directly.
                let Some(state) = in_flight.as_mut() else {
                    continue;
                };
                state.tick_cancel();
                // T-INLINE-CLUSTER #10: while a turn is streaming the spinner
                // needs ticks; mark dirty so the next iteration repaints.
                dirty = true;
            }
            LoopEvent::Chat(None) => {
                // Bridge closed. Treat as disconnect if a turn was in flight.
                if let Some(state) = in_flight.take() {
                    finalize_turn(terminal, mode, &mut transcript, &mut scroll, &state, false)?;
                    let lines =
                        render_system_notice(NoticeKind::Warn, "backend disconnected mid-turn");
                    emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                    pending_user_prompt = None;
                }
                chat_rx = None;
                dirty = true;
            }
            LoopEvent::Chat(Some(evt)) => {
                match evt {
                    ChatEvent::AssistantDelta(delta) => {
                        let Some(state) = in_flight.as_mut() else {
                            continue;
                        };
                        state.buffer.push_str(&delta);
                        state.last_delta = Instant::now();
                        update_main_agent_output(&mut agent_panel, &delta);
                        dirty = true;
                    }
                    ChatEvent::Runtime(runtime_evt) => {
                        match runtime_evt {
                            crate::action::RuntimeEvent::SessionStarted { session_id, title } => {
                                composer_empty_hint = None;
                                let mut body = format!("session started: {}", session_id.as_str());
                                if let Some(title) = title {
                                    if !title.trim().is_empty() {
                                        body.push_str(&format!(" · {title}"));
                                    }
                                }
                                let lines = render_system_notice(NoticeKind::Info, &body);
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                            }
                            crate::action::RuntimeEvent::SessionEnded { session_id } => {
                                composer_empty_hint = None;
                                let lines = render_system_notice(
                                    NoticeKind::Info,
                                    &format!("session ended: {}", session_id.as_str()),
                                );
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                            }
                            crate::action::RuntimeEvent::AssistantTextDelta { text } => {
                                let Some(state) = in_flight.as_mut() else {
                                    continue;
                                };
                                state.buffer.push_str(&text);
                                state.last_delta = Instant::now();
                                update_main_agent_output(&mut agent_panel, &text);
                            }
                            crate::action::RuntimeEvent::DaemonStatus {
                                session_id,
                                status,
                                message,
                            } => {
                                let kind = match status.as_str() {
                                    s if s.eq_ignore_ascii_case("failed") => NoticeKind::Error,
                                    s if s.eq_ignore_ascii_case("paused")
                                        || s.eq_ignore_ascii_case("degraded") =>
                                    {
                                        NoticeKind::Warn
                                    }
                                    _ => NoticeKind::Info,
                                };
                                let mut body = if let Some(session_id) = session_id.as_ref() {
                                    format!("daemon {status}: {}", session_id.as_str())
                                } else {
                                    format!("daemon {status}")
                                };
                                if let Some(message) = message {
                                    if !message.trim().is_empty() {
                                        body.push_str(&format!(" · {message}"));
                                    }
                                }
                                let lines = render_system_notice(kind, &body);
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                            }
                            crate::action::RuntimeEvent::PermissionAsked {
                                request_id,
                                session_id,
                                permission,
                                patterns,
                                always,
                            } => {
                                let mut lines = render_permission_chip(
                                    &request_id,
                                    session_id.as_str(),
                                    &permission,
                                    &patterns,
                                    &always,
                                );
                                if lines.is_empty() {
                                    lines = render_system_notice(
                                        NoticeKind::Info,
                                        &format!(
                                            "permission asked: {permission} {}",
                                            if patterns.is_empty() {
                                                String::new()
                                            } else {
                                                format!("· {}", patterns.join(", "))
                                            }
                                        ),
                                    );
                                }
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                            }
                            crate::action::RuntimeEvent::PermissionReplied {
                                request_id,
                                session_id,
                                reply,
                            } => {
                                let lines = render_system_notice(
                                    NoticeKind::Info,
                                    &format!(
                                        "permission replied: {reply} · {request_id} · {}",
                                        session_id.as_str()
                                    ),
                                );
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                            }
                            crate::action::RuntimeEvent::QuestionAsked {
                                question_id,
                                session_id,
                                prompt,
                                choices,
                            } => {
                                if choices.is_empty() {
                                    composer_empty_hint = Some(prompt.clone());
                                    let lines = render_system_notice(
                                        NoticeKind::Info,
                                        &format!(
                                            "question asked: {prompt} · answer in composer · {question_id} · {}",
                                            session_id.as_str()
                                        ),
                                    );
                                    emit_lines(
                                        terminal,
                                        mode,
                                        &mut transcript,
                                        &mut scroll,
                                        &lines,
                                    )?;
                                } else {
                                    composer_empty_hint = None;
                                    let lines = render_question_chip(
                                        &question_id,
                                        session_id.as_str(),
                                        &prompt,
                                        &choices,
                                    );
                                    emit_lines(
                                        terminal,
                                        mode,
                                        &mut transcript,
                                        &mut scroll,
                                        &lines,
                                    )?;
                                }
                            }
                            crate::action::RuntimeEvent::QuestionReplied {
                                question_id,
                                session_id,
                                answer,
                            } => {
                                composer_empty_hint = None;
                                let lines = render_system_notice(
                                    NoticeKind::Info,
                                    &format!(
                                        "question replied: {answer} · {question_id} · {}",
                                        session_id.as_str()
                                    ),
                                );
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                            }
                            crate::action::RuntimeEvent::AssistantCompleted => {
                                // Some adapters collapse assistant completion
                                // into the runtime channel; the dedicated
                                // `ChatEvent::TurnComplete` path still owns
                                // the turn finalization lifecycle.
                            }
                            crate::action::RuntimeEvent::AssistantFailed { error } => {
                                let lines = render_system_notice(
                                    NoticeKind::Error,
                                    &format!("turn failed: {error}"),
                                );
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                            }
                            crate::action::RuntimeEvent::ReasoningEnded { reasoning_id, text } => {
                                let lines = render_reasoning(&text);
                                emit_lines(
                                    terminal,
                                    mode,
                                    &mut transcript,
                                    &mut scroll,
                                    TranscriptEvent::reasoning(Some(reasoning_id), text, lines),
                                )?;
                            }
                            crate::action::RuntimeEvent::ReasoningStarted { .. }
                            | crate::action::RuntimeEvent::ReasoningDelta { .. }
                            | crate::action::RuntimeEvent::Tick => {}
                            crate::action::RuntimeEvent::Tool(event) => {
                                let Some(state) = in_flight.as_mut() else {
                                    continue;
                                };
                                if let Some(chip) = state.apply_tool_event(event) {
                                    let ok = matches!(chip.status, ToolChipStatus::Success);
                                    let lines = render_completed_tool_card(&chip, ok);
                                    let tool_id = chip._id.clone();
                                    let tool_name = chip.name.clone();
                                    let tool_input = chip.input.clone();
                                    let tool_output: Vec<String> =
                                        chip.output.lines().map(|l| l.to_string()).collect();
                                    let tool_status = if ok {
                                        ActionStatus::Success
                                    } else {
                                        match chip.status {
                                            ToolChipStatus::Failure => ActionStatus::Failure,
                                            _ => ActionStatus::Cancelled,
                                        }
                                    };
                                    let buffer = Arc::new(chip.build_output_buffer());
                                    transcript.record_tool_buffer(tool_id.clone(), buffer);
                                    emit_lines(
                                        terminal,
                                        mode,
                                        &mut transcript,
                                        &mut scroll,
                                        TranscriptEvent::tool_with_output(
                                            tool_id,
                                            tool_name,
                                            tool_input,
                                            tool_output,
                                            tool_status,
                                            12,
                                            lines,
                                        ),
                                    )?;
                                }
                            }
                        }
                        dirty = true;
                    }
                    ChatEvent::Reasoning { reasoning_id, text } => {
                        let lines = render_reasoning(&text);
                        emit_lines(
                            terminal,
                            mode,
                            &mut transcript,
                            &mut scroll,
                            TranscriptEvent::reasoning(Some(reasoning_id), text, lines),
                        )?;
                        dirty = true;
                    }
                    ChatEvent::Tool(event) => {
                        let Some(state) = in_flight.as_mut() else {
                            continue;
                        };
                        // T-SEMANTIC-TRANSCRIPT-A: `apply_tool_event` returns
                        // the chip when a tool terminates (Complete/Fail) so
                        // multi-tool turns each get their own transcript card
                        // *and* a persistent OutputBuffer keyed by tool id.
                        if let Some(chip) = state.apply_tool_event(event) {
                            let ok = matches!(chip.status, ToolChipStatus::Success);
                            let lines = render_completed_tool_card(&chip, ok);
                            let tool_id = chip._id.clone();
                            let tool_name = chip.name.clone();
                            let tool_input = chip.input.clone();
                            // T-SEMANTIC-TRANSCRIPT-B: capture the same
                            // {output, status, max} triple `render_completed_tool_card`
                            // computed so the resize-aware cache can re-run
                            // `render_tool_call` at the current viewport width
                            // without losing the chip's terminal state.
                            let tool_output: Vec<String> =
                                chip.output.lines().map(|l| l.to_string()).collect();
                            let tool_status = if ok {
                                ActionStatus::Success
                            } else {
                                match chip.status {
                                    ToolChipStatus::Failure => ActionStatus::Failure,
                                    _ => ActionStatus::Cancelled,
                                }
                            };
                            let buffer = Arc::new(chip.build_output_buffer());
                            transcript.record_tool_buffer(tool_id.clone(), buffer);
                            emit_lines(
                                terminal,
                                mode,
                                &mut transcript,
                                &mut scroll,
                                TranscriptEvent::tool_with_output(
                                    tool_id,
                                    tool_name,
                                    tool_input,
                                    tool_output,
                                    tool_status,
                                    12,
                                    lines,
                                ),
                            )?;
                        }
                        dirty = true;
                    }
                    ChatEvent::TurnComplete => {
                        let Some(state) = in_flight.as_mut() else {
                            continue;
                        };
                        // T-INLINE-CLUSTER #9: capture the completed turn for
                        // /compact replay before clearing the in-flight slot.
                        let assistant_text = state.buffer.clone();
                        if let Some(user) = pending_user_prompt.take() {
                            chat_history.push(ChatTurnRecord {
                                user,
                                assistant: assistant_text,
                            });
                        }
                        finalize_turn(terminal, mode, &mut transcript, &mut scroll, state, true)?;
                        update_main_agent_turn(
                            &mut agent_panel,
                            AgentStatus::Done,
                            "assistant turn complete",
                            None,
                            Some("done"),
                        );
                        in_flight = None;
                        chat_rx = None;
                        dirty = true;
                    }
                    ChatEvent::TurnFailed(err) => {
                        let Some(state) = in_flight.as_mut() else {
                            continue;
                        };
                        finalize_turn(terminal, mode, &mut transcript, &mut scroll, state, false)?;
                        let lines =
                            render_system_notice(NoticeKind::Error, &format!("turn failed: {err}"));
                        emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                        update_main_agent_turn(
                            &mut agent_panel,
                            AgentStatus::Failed,
                            &format!("turn failed: {err}"),
                            None,
                            Some("failed"),
                        );
                        in_flight = None;
                        chat_rx = None;
                        pending_user_prompt = None;
                        dirty = true;
                    }
                    ChatEvent::Diff { path, hunks } => {
                        let lines = render_diff_lines_from_payload(&path, &hunks);
                        // T-SEMANTIC-TRANSCRIPT-B: stash the owned hunks
                        // alongside the cached lines so the resize-aware
                        // cache (`render_event_at_width`) can re-run
                        // `render_diff` at the new viewport width and refresh
                        // the dual-line-number gutter.
                        emit_lines(
                            terminal,
                            mode,
                            &mut transcript,
                            &mut scroll,
                            TranscriptEvent::diff_with_hunks(path, hunks, lines),
                        )?;
                        dirty = true;
                    }
                    ChatEvent::Notice(kind, msg) => {
                        let lines = render_system_notice(kind, &msg);
                        emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                        dirty = true;
                    }
                }
            }
            LoopEvent::Input(None) => return Ok(()),
            LoopEvent::Input(Some(Err(_))) => continue,
            LoopEvent::Input(Some(Ok(event))) => match event {
                Event::Key(key_event) => {
                    let KeyEvent {
                        code, modifiers, ..
                    } = key_event;
                    // T-INLINE-CLUSTER #2: while the pager overlay is active,
                    // intercept every key event first. The pager owns scroll +
                    // search; its `PagerAction` tells us when to exit (Esc/q)
                    // or when to copy a yank payload through OSC52.
                    if let Some(state) = pager.as_mut() {
                        let viewport_h = transcript_rect.height.max(1) as usize;
                        match pager_handle_key(state, key_event, viewport_h) {
                            PagerAction::Exit => {
                                pager = None;
                                dirty = true;
                            }
                            PagerAction::Yank(text) => {
                                let bytes = text.len();
                                let _ = osc52::copy_to_clipboard(&text);
                                toasts.push(Toast::success(format!(
                                    "yanked {bytes} bytes to clipboard"
                                )));
                                dirty = true;
                            }
                            PagerAction::None => {
                                dirty = true;
                            }
                        }
                        continue;
                    }
                    match (code, modifiers) {
                        // T-INLINE-CLUSTER #5: Ctrl+Shift+C copies the full
                        // transcript via OSC52, mirroring the /copy slash. Match
                        // ahead of the bare Ctrl+C interrupt arm so the shift
                        // modifier wins. crossterm folds the printable letter to
                        // uppercase when shift is held, so we accept either case.
                        (KeyCode::Char('c'), m) | (KeyCode::Char('C'), m)
                            if m.contains(KeyModifiers::CONTROL)
                                && m.contains(KeyModifiers::SHIFT) =>
                        {
                            let (text, byte_count) =
                                transcript_as_text(&transcript, in_flight.as_ref());
                            let _ = osc52::copy_to_clipboard(&text);
                            toasts.push(Toast::success(format!(
                                "copied {byte_count} bytes to clipboard"
                            )));
                            dirty = true;
                        }
                        // T-INLINE-CLUSTER #2: Ctrl+O enters the pager for the
                        // active tool's output buffer. T-SEMANTIC-TRANSCRIPT-A
                        // now also keeps a persistent OutputBuffer sidecar on
                        // the transcript so completed tools remain pageable
                        // after the turn finalizes (Wave 13 picks up the full
                        // pager-from-scrollback wiring; here we surface the
                        // freshest in-flight tool first).
                        (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                            // Pager priority: in-flight latest tool first (live
                            // streaming output), then fall back to the most
                            // recently completed tool's persistent buffer from
                            // the transcript sidecar (T-SEMANTIC-TRANSCRIPT-A).
                            let live = in_flight
                                .as_ref()
                                .and_then(|state| state.latest_tool())
                                .map(|tool| {
                                    tool.output
                                        .lines()
                                        .map(|l| l.to_string())
                                        .collect::<Vec<_>>()
                                });
                            #[allow(clippy::manual_unwrap_or_default)]
                            let lines = match live {
                                Some(lines) if !lines.is_empty() => lines,
                                _ => match transcript.last_tool_buffer_lines() {
                                    Some(lines) => lines,
                                    None => Vec::new(),
                                },
                            };
                            if !lines.is_empty() {
                                pager = Some(PagerState::new(lines));
                                dirty = true;
                            } else {
                                toasts.push(Toast::warning(
                                    "no tool output to page (run a tool first)",
                                ));
                                dirty = true;
                            }
                        }
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            if let Some(state) = in_flight.as_mut() {
                                let level = state.cancel_on_interrupt();
                                let lines = render_system_notice(
                                    NoticeKind::Warn,
                                    &format!(
                                        "cancellation requested: {}",
                                        cancel_level_label(level)
                                    ),
                                );
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                                update_main_agent_turn(
                                    &mut agent_panel,
                                    AgentStatus::Cancelled,
                                    "cancellation requested",
                                    None,
                                    Some("cancelled"),
                                );
                                dirty = true;
                            } else if composer.text.is_empty() {
                                let bye =
                                    render_session_header(&ctx.version, "session ended", None);
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &bye)?;
                                return Ok(());
                            } else {
                                composer.clear_all();
                                dirty = true;
                            }
                        }
                        (KeyCode::Esc, _) => {
                            if matches!(focus, FocusArea::Agents) {
                                focus = FocusArea::Composer;
                                agent_panel.set_focus(false);
                                continue;
                            }
                            if composer.slash.active {
                                // Esc backs out of a child submenu before closing the popup.
                                if !composer.slash.pop_submenu() {
                                    composer.slash = SlashState::default();
                                }
                            } else if composer.mention.active {
                                composer.mention = MentionState::default();
                            } else if let Some(state) = in_flight.as_mut() {
                                let level = state.cancel_on_interrupt();
                                let lines = render_system_notice(
                                    NoticeKind::Warn,
                                    &format!(
                                        "cancellation requested: {}",
                                        cancel_level_label(level)
                                    ),
                                );
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &lines)?;
                                update_main_agent_turn(
                                    &mut agent_panel,
                                    AgentStatus::Cancelled,
                                    "cancellation requested",
                                    None,
                                    Some("cancelled"),
                                );
                            } else if composer.text.is_empty() {
                                let bye =
                                    render_session_header(&ctx.version, "session ended", None);
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &bye)?;
                                return Ok(());
                            } else {
                                composer.clear_all();
                            }
                        }
                        (KeyCode::Up, _) if composer.slash.active => {
                            composer.slash.move_prev();
                        }
                        (KeyCode::Down, _) if composer.slash.active => {
                            composer.slash.move_next(&catalog);
                        }
                        (KeyCode::Up, _)
                            if composer.mention.active && composer.mention.cursor > 0 =>
                        {
                            composer.mention.cursor -= 1;
                        }
                        (KeyCode::Down, _) if composer.mention.active => {
                            let max = composer.mention.filtered.len().saturating_sub(1);
                            if composer.mention.cursor < max {
                                composer.mention.cursor += 1;
                            }
                        }
                        (KeyCode::Tab, _) if composer.slash.active => {
                            if let Some(cmd) = composer.slash.current_command(&catalog) {
                                composer.text = format!("/{}", cmd.id());
                                composer.sync_popups(&index, &catalog);
                            }
                        }
                        (KeyCode::Tab, _) if composer.mention.active => {
                            composer.accept_mention();
                            composer.sync_popups(&index, &catalog);
                        }
                        (KeyCode::Enter, KeyModifiers::SHIFT) => {
                            composer.text.push('\n');
                            composer.sync_popups(&index, &catalog);
                        }
                        (KeyCode::Enter, _) => {
                            if matches!(focus, FocusArea::Agents) {
                                if let Some(agent) = agent_panel.selected_agent() {
                                    let lines = render_system_notice(
                                        NoticeKind::Info,
                                        &format!(
                                            "{} · {} · {} · {}",
                                            agent.kind.label(),
                                            agent.status.label(),
                                            elapsed_label(agent.runtime(Instant::now())),
                                            agent.summary
                                        ),
                                    );
                                    emit_lines(
                                        terminal,
                                        mode,
                                        &mut transcript,
                                        &mut scroll,
                                        &lines,
                                    )?;
                                }
                                focus = FocusArea::Composer;
                                agent_panel.set_focus(false);
                                continue;
                            }
                            if in_flight.is_some() {
                                continue;
                            }
                            let raw_line_for_args = composer.text.clone();
                            if let Some(cmd_id) = raw_line_for_args
                                .trim_start()
                                .strip_prefix('/')
                                .and_then(|rest| rest.split_whitespace().next())
                            {
                                let has_args = raw_line_for_args.trim_start().contains(char::is_whitespace);
                                if has_args && matches!(cmd_id, "run" | "stop") {
                                    let action = catalog.action_for(cmd_id);
                                    composer.clear_all();
                                    match action {
                                        SlashAction::Run => {
                                            let raw = slash_args(&raw_line_for_args, &cmd_id);
                                            let (background, cmd) = parse_run_args(&raw);
                                            if cmd.is_empty() {
                                                let lines = render_system_notice(
                                                    NoticeKind::Warn,
                                                    "/run requires a command (e.g. /run --background sleep 30)",
                                                );
                                                emit_lines(
                                                    terminal,
                                                    mode,
                                                    &mut transcript,
                                                    &mut scroll,
                                                    &lines,
                                                )?;
                                            } else if background {
                                                let (id, token) =
                                                    bg_manager.register(cmd.to_string(), None);
                                                let bg_for_task = Arc::clone(&bg_manager);
                                                let cmd_for_task = cmd.to_string();
                                                tokio::spawn(async move {
                                                    let status = run_background_shell(
                                                        cmd_for_task,
                                                        token,
                                                    )
                                                    .await;
                                                    bg_for_task.finalize(id, status);
                                                });
                                                let lines = render_system_notice(
                                                    NoticeKind::Info,
                                                    &format!(
                                                        "[{id}] backgrounded \"{cmd}\". Use /ps to list, /stop {id} to cancel."
                                                    ),
                                                );
                                                emit_lines(
                                                    terminal,
                                                    mode,
                                                    &mut transcript,
                                                    &mut scroll,
                                                    &lines,
                                                )?;
                                            } else {
                                                let lines = render_system_notice(
                                                    NoticeKind::Warn,
                                                    &format!(
                                                        "/run {cmd}: foreground execution is a follow-up; use /run --background to detach."
                                                    ),
                                                );
                                                emit_lines(
                                                    terminal,
                                                    mode,
                                                    &mut transcript,
                                                    &mut scroll,
                                                    &lines,
                                                )?;
                                            }
                                        }
                                        SlashAction::Stop => {
                                            let arg = slash_args(&raw_line_for_args, &cmd_id);
                                            if !arg.is_empty() {
                                                match arg.parse::<JobId>() {
                                                    Ok(id) => {
                                                        let (kind, msg) = if bg_manager.stop(id)
                                                        {
                                                            (
                                                                NoticeKind::Info,
                                                                format!(
                                                                    "stopped background job [{id}]"
                                                                ),
                                                            )
                                                        } else {
                                                            (
                                                                NoticeKind::Warn,
                                                                format!(
                                                                    "no background job with id [{id}]"
                                                                ),
                                                            )
                                                        };
                                                        let lines =
                                                            render_system_notice(kind, &msg);
                                                        emit_lines(
                                                            terminal,
                                                            mode,
                                                            &mut transcript,
                                                            &mut scroll,
                                                            &lines,
                                                        )?;
                                                    }
                                                    Err(_) => {
                                                        let lines = render_system_notice(
                                                            NoticeKind::Warn,
                                                            &format!(
                                                                "invalid job id: {arg:?} (expected an integer)"
                                                            ),
                                                        );
                                                        emit_lines(
                                                            terminal,
                                                            mode,
                                                            &mut transcript,
                                                            &mut scroll,
                                                            &lines,
                                                        )?;
                                                    }
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                    dirty = true;
                                    continue;
                                }
                            }
                            if composer.mention.active && !composer.mention.filtered.is_empty() {
                                composer.accept_mention();
                                composer.sync_popups(&index, &catalog);
                                continue;
                            }
                            if composer.slash.active {
                                if let Some((_parent_id, shell_base, item)) =
                                    composer.slash.selected_subcommand(&catalog)
                                {
                                    let msg = submenu_child_notice_for(shell_base, item);
                                    composer.clear_all();
                                    let lines = render_system_notice(NoticeKind::Info, &msg);
                                    emit_lines(
                                        terminal,
                                        mode,
                                        &mut transcript,
                                        &mut scroll,
                                        &lines,
                                    )?;
                                    continue;
                                }
                                let Some(cmd_id) = composer
                                    .slash
                                    .current_command(&catalog)
                                    .map(|c| c.id().to_string())
                                else {
                                    continue;
                                };
                                if composer.slash.open_submenu(&catalog, &cmd_id) {
                                    composer.text = format!("/{cmd_id}");
                                    continue;
                                }
                                // T-BG-COUNT-MANAGER: capture the slash-line
                                // text BEFORE `clear_all()` so `/stop <id>`
                                // and future arg-bearing slashes can parse
                                // their trailing args from `slash_args_raw`.
                                let slash_line_raw = composer.text.clone();
                                composer.clear_all();
                                let action = catalog.action_for(&cmd_id);
                                match action {
                                    SlashAction::Help => {
                                        let lines = render_system_notice(
                                        NoticeKind::Info,
                                        "shortcuts: ⏎ send · ⇧⏎ newline · / commands · @ files · Ctrl+C clear/quit · Esc clear/close popup",
                                    );
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Quit => {
                                        let bye = render_session_header(
                                            &ctx.version,
                                            "session ended",
                                            None,
                                        );
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &bye,
                                        )?;
                                        return Ok(());
                                    }
                                    SlashAction::NewSession => {
                                        let lines = render_session_header(
                                            &ctx.version,
                                            &ctx.cwd_display,
                                            ctx.branch.as_deref(),
                                        );
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            TranscriptEvent::session_header(
                                                ctx.version.clone(),
                                                ctx.cwd_display.clone(),
                                                ctx.branch.clone(),
                                                lines,
                                            ),
                                        )?;
                                    }
                                    SlashAction::Clear => {
                                        let term_w = terminal.size()?.width as usize;
                                        // T-GLYPH-WAVE2: divider chrome glyph
                                        // defers to GlyphMode (`-` in ASCII).
                                        let rule =
                                            glyph_set::current().divider.repeat(term_w.max(40));
                                        let divider = vec![Line::from(Span::styled(
                                            rule,
                                            Style::default().fg(theme::codex_rule()),
                                        ))];
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            TranscriptEvent::divider("clear".to_string(), divider),
                                        )?;
                                    }
                                    SlashAction::Echo => {
                                        let lines = render_system_notice(
                                            NoticeKind::Warn,
                                            &format!("/{} is not wired in this build", cmd_id),
                                        );
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Panels => {
                                        let body = render_panels_snapshot(
                                            &ctx,
                                            in_flight.as_ref(),
                                            transcript.block_count(),
                                            &jnoccio_boot,
                                            &agent_panel,
                                            &toasts,
                                            permission_state.mode.label(),
                                            motion_enabled,
                                            terminal.size()?.width,
                                        );
                                        let lines = render_system_notice(NoticeKind::Info, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::UserDefined { id, body } => {
                                        let preview = body.trim();
                                        if preview.is_empty() {
                                            let lines = render_system_notice(
                                                NoticeKind::Info,
                                                &format!("user-defined /{id} (no body)"),
                                            );
                                            emit_lines(
                                                terminal,
                                                mode,
                                                &mut transcript,
                                                &mut scroll,
                                                &lines,
                                            )?;
                                        } else {
                                            let prompt = preview.to_string();
                                            if splash.visible() {
                                                let snap = splash_snapshot_lines(
                                                    &splash.ctx,
                                                    terminal.size()?.width,
                                                );
                                                emit_lines(
                                                    terminal,
                                                    mode,
                                                    &mut transcript,
                                                    &mut scroll,
                                                    &snap,
                                                )?;
                                            }
                                            splash.on_first_submit();
                                            let user_lines = render_user(&prompt);
                                            emit_lines(
                                                terminal,
                                                mode,
                                                &mut transcript,
                                                &mut scroll,
                                                TranscriptEvent::user(prompt.clone(), user_lines),
                                            )?;
                                            update_main_agent_turn(
                                                &mut agent_panel,
                                                AgentStatus::Running,
                                                &prompt,
                                                Some(estimate_input_tokens(&prompt)),
                                                Some(id.as_str()),
                                            );
                                            focus = FocusArea::Composer;
                                            let state = InFlight::new();
                                            let rx = backend
                                                .start_turn(prompt.clone(), state.cancel_token());
                                            chat_rx = Some(bridge_chat_channel(rx));
                                            in_flight = Some(state);
                                            pending_user_prompt = Some(prompt);
                                        }
                                    }
                                    SlashAction::Ps => {
                                        // T-BG-COUNT-MANAGER: snapshot the
                                        // background-job registry and render
                                        // as an info notice. The body helper
                                        // (`render_ps_body`) is unit tested
                                        // separately so the formatting has
                                        // assertions without spinning up the
                                        // full runtime.
                                        let jobs = bg_manager.list();
                                        let body = render_ps_body(&jobs);
                                        let lines = render_system_notice(NoticeKind::Info, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Stop => {
                                        // T-BG-COUNT-MANAGER: `/stop` now
                                        // accepts an optional bg-job id. Parse
                                        // the trailing arg from `slash_line_raw`
                                        // (captured before `clear_all()` above).
                                        // If present + parseable as `JobId`,
                                        // cancel that specific job; otherwise
                                        // fall through to the compatibility in-flight
                                        // turn cancel behavior.
                                        let arg = slash_args(&slash_line_raw, &cmd_id);
                                        if !arg.is_empty() {
                                            match arg.parse::<JobId>() {
                                                Ok(id) => {
                                                    let (kind, msg) = if bg_manager.stop(id) {
                                                        (
                                                            NoticeKind::Info,
                                                            format!(
                                                                "stopped background job [{id}]"
                                                            ),
                                                        )
                                                    } else {
                                                        (
                                                            NoticeKind::Warn,
                                                            format!(
                                                                "no background job with id [{id}]"
                                                            ),
                                                        )
                                                    };
                                                    let lines = render_system_notice(kind, &msg);
                                                    emit_lines(
                                                        terminal,
                                                        mode,
                                                        &mut transcript,
                                                        &mut scroll,
                                                        &lines,
                                                    )?;
                                                }
                                                Err(_) => {
                                                    let lines = render_system_notice(
                                                        NoticeKind::Warn,
                                                        &format!(
                                                            "invalid job id: {arg:?} (expected an integer)"
                                                        ),
                                                    );
                                                    emit_lines(
                                                        terminal,
                                                        mode,
                                                        &mut transcript,
                                                        &mut scroll,
                                                        &lines,
                                                    )?;
                                                }
                                            }
                                        } else if let Some(state) = in_flight.as_mut() {
                                            let level = state.cancel_on_stop();
                                            let lines = render_system_notice(
                                                NoticeKind::Warn,
                                                &format!(
                                                    "cancellation requested: {}",
                                                    cancel_level_label(level)
                                                ),
                                            );
                                            emit_lines(
                                                terminal,
                                                mode,
                                                &mut transcript,
                                                &mut scroll,
                                                &lines,
                                            )?;
                                            update_main_agent_turn(
                                                &mut agent_panel,
                                                AgentStatus::Cancelled,
                                                "cancelled by /stop",
                                                None,
                                                Some("cancelled"),
                                            );
                                        } else {
                                            let lines = render_system_notice(
                                                NoticeKind::Info,
                                                "no active turn",
                                            );
                                            emit_lines(
                                                terminal,
                                                mode,
                                                &mut transcript,
                                                &mut scroll,
                                                &lines,
                                            )?;
                                        }
                                    }
                                    // ── Phase M tier-1 wired actions ────────────────
                                    // `/jankurai` = mutating full cycle
                                    // (audit → runner --once → re-audit → improved/regressed).
                                    SlashAction::Jankurai => {
                                        let state = InFlight::new();
                                        match spawn_jankurai_cycle_turn(state.cancel_token()) {
                                            Ok(new_rx) => {
                                                chat_rx = Some(new_rx);
                                                in_flight = Some(state);
                                            }
                                            Err(msg) => {
                                                let lines =
                                                    render_system_notice(NoticeKind::Error, &msg);
                                                emit_lines(
                                                    terminal,
                                                    mode,
                                                    &mut transcript,
                                                    &mut scroll,
                                                    &lines,
                                                )?;
                                            }
                                        }
                                    }
                                    // `/audit` and `/audit-check` = read-only
                                    // single-shot advisory audit.
                                    SlashAction::Audit | SlashAction::AuditCheck => {
                                        let state = InFlight::new();
                                        match spawn_jankurai_turn(state.cancel_token()) {
                                            Ok(new_rx) => {
                                                chat_rx = Some(new_rx);
                                                in_flight = Some(state);
                                            }
                                            Err(msg) => {
                                                let lines =
                                                    render_system_notice(NoticeKind::Error, &msg);
                                                emit_lines(
                                                    terminal,
                                                    mode,
                                                    &mut transcript,
                                                    &mut scroll,
                                                    &lines,
                                                )?;
                                            }
                                        }
                                    }
                                    SlashAction::JankuraiStatus | SlashAction::Score => {
                                        let (kind, body) = render_jankurai_status();
                                        let lines = render_system_notice(kind, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Doctor => {
                                        let body = render_doctor_snapshot(&ctx);
                                        let lines = render_system_notice(NoticeKind::Info, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Status => {
                                        let body = render_status_snapshot(
                                            &ctx,
                                            in_flight.is_some(),
                                            transcript.block_count(),
                                            &jnoccio_boot,
                                        );
                                        let lines = render_system_notice(NoticeKind::Info, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Export => {
                                        // T-INLINE-CLUSTER #6: write the full
                                        // transcript (not just in-flight) to a
                                        // timestamped file in the current cwd.
                                        let (kind, body) =
                                            export_transcript_full(&transcript, in_flight.as_ref());
                                        let lines = render_system_notice(kind, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Init => {
                                        let (kind, body) = init_claude_md();
                                        let lines = render_system_notice(kind, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Copy => {
                                        // T-INLINE-CLUSTER #4: OSC52 the full
                                        // transcript (joins every block). Toast
                                        // on success so the user sees the byte
                                        // count even when the terminal eats the
                                        // OSC52 silently (Terminal.app, etc.).
                                        let (text, bytes) =
                                            transcript_as_text(&transcript, in_flight.as_ref());
                                        let _ = osc52::copy_to_clipboard(&text);
                                        toasts.push(Toast::success(format!(
                                            "copied {bytes} bytes to clipboard"
                                        )));
                                        let lines = render_system_notice(
                                            NoticeKind::Info,
                                            &format!(
                                                "/copy: sent {bytes} bytes via OSC52 (toast below)"
                                            ),
                                        );
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Permissions => {
                                        // T-PERMISSIONS-PLUMB: each /permissions
                                        // invocation cycles
                                        // BypassPermissions → AskBeforeWrite →
                                        // ReadOnly. We still emit the full state
                                        // notice so the operator sees the new
                                        // label + sandbox + approval combo. Full
                                        // modal infra with Shift+Tab cycling is
                                        // a follow-up (depends on focus rework).
                                        permission_state.mode = permission_state.mode.cycle();
                                        let body = permissions_modal_body(
                                            permission_state.mode.label(),
                                            opts.permission_mode.as_deref(),
                                            agent_panel.local_running_count(),
                                        );
                                        let lines = render_system_notice(NoticeKind::Info, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                        let change = render_system_notice(
                                            NoticeKind::Info,
                                            &format!(
                                                "Permission mode → {}",
                                                permission_state.mode.label()
                                            ),
                                        );
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &change,
                                        )?;
                                    }
                                    SlashAction::Sandbox => {
                                        // T-PERMISSIONS-PLUMB: extends the
                                        // T-INLINE-CLUSTER #8 status notice with
                                        // the runtime-owned permission/sandbox/
                                        // approval triple from `permission_state`
                                        // (so any /permissions cycling shows up
                                        // here too) plus net/allowed_paths
                                        // deferred fields documented out of scope
                                        // for the inline runtime.
                                        let body = sandbox_modal_body(
                                            permission_state.sandbox_profile.as_deref(),
                                            permission_state.approval_mode.as_deref(),
                                            &ctx,
                                        );
                                        let lines = render_system_notice(NoticeKind::Info, &body);
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                    SlashAction::Compact => {
                                        // T-INLINE-CLUSTER #9: preserve the last
                                        // 5 turns verbatim; ask the backend to
                                        // summarise the older ones. Tool / diff /
                                        // reasoning / system events are NOT
                                        // compacted (per Codex 01:55Z split).
                                        match compact_history(
                                            &mut chat_history,
                                            &mut transcript,
                                            &mut scroll,
                                            terminal,
                                            mode,
                                            in_flight.as_ref(),
                                            backend,
                                        ) {
                                            Some((new_rx, state, prompt)) => {
                                                chat_rx = Some(new_rx);
                                                in_flight = Some(state);
                                                pending_user_prompt = Some(prompt);
                                            }
                                            None => {
                                                let lines = render_system_notice(
                                                    NoticeKind::Info,
                                                    "/compact: no older turns to compact",
                                                );
                                                emit_lines(
                                                    terminal,
                                                    mode,
                                                    &mut transcript,
                                                    &mut scroll,
                                                    &lines,
                                                )?;
                                            }
                                        }
                                    }
                                    SlashAction::Run => {
                                        // T-PERMISSIONS-PLUMB / T-BG-RUN: `/run`
                                        // optionally accepts a leading
                                        // `--background` or `--bg` flag. When the
                                        // background flag is set, register the
                                        // job with `bg_manager`, spawn an async
                                        // task that runs the command via
                                        // `tokio::process::Command`, and finalize
                                        // the job when the child exits.
                                        // Foreground `/run` is still a parity
                                        // notice until the per-tool runner lands
                                        // (out of scope for this packet).
                                        let raw = slash_args(&slash_line_raw, &cmd_id);
                                        let (background, cmd) = parse_run_args(&raw);
                                        if cmd.is_empty() {
                                            let lines = render_system_notice(
                                                NoticeKind::Warn,
                                                "/run requires a command (e.g. /run --background sleep 30)",
                                            );
                                            emit_lines(
                                                terminal,
                                                mode,
                                                &mut transcript,
                                                &mut scroll,
                                                &lines,
                                            )?;
                                        } else if background {
                                            let (id, token) =
                                                bg_manager.register(cmd.to_string(), None);
                                            let bg_for_task = Arc::clone(&bg_manager);
                                            let cmd_for_task = cmd.to_string();
                                            tokio::spawn(async move {
                                                let status =
                                                    run_background_shell(cmd_for_task, token).await;
                                                bg_for_task.finalize(id, status);
                                            });
                                            let lines = render_system_notice(
                                                NoticeKind::Info,
                                                &format!(
                                                    "[{id}] backgrounded \"{cmd}\". Use /ps to list, /stop {id} to cancel."
                                                ),
                                            );
                                            emit_lines(
                                                terminal,
                                                mode,
                                                &mut transcript,
                                                &mut scroll,
                                                &lines,
                                            )?;
                                        } else {
                                            let lines = render_system_notice(
                                                NoticeKind::Warn,
                                                &format!(
                                                    "/run {cmd}: foreground execution is a follow-up; use /run --background to detach."
                                                ),
                                            );
                                            emit_lines(
                                                terminal,
                                                mode,
                                                &mut transcript,
                                                &mut scroll,
                                                &lines,
                                            )?;
                                        }
                                    }
                                    SlashAction::Unknown => {}
                                    // WHY: every remaining variant is a parity slot whose
                                    // backend isn't wired yet. Surface a notice so users
                                    // see the command was recognized.
                                    _ => {
                                        let lines = render_system_notice(
                                            NoticeKind::Warn,
                                            &format!(
                                                "/{cmd_id} is recognized; run it from the shell for now"
                                            ),
                                        );
                                        emit_lines(
                                            terminal,
                                            mode,
                                            &mut transcript,
                                            &mut scroll,
                                            &lines,
                                        )?;
                                    }
                                }
                                dirty = true;
                                continue;
                            }
                            if composer.text.trim().is_empty() {
                                continue;
                            }
                            let prompt = composer.take_expanded_text();
                            composer.sync_popups(&index, &catalog);
                            // T1-V6b: first submit collapses the splash. Emit
                            // the wordmark snapshot into transcript FIRST so
                            // it scrolls up into history as the user types
                            // (instead of getting replaced by an empty
                            // transcript area or a redundant "welcome" block).
                            if splash.visible() {
                                let snap =
                                    splash_snapshot_lines(&splash.ctx, terminal.size()?.width);
                                emit_lines(terminal, mode, &mut transcript, &mut scroll, &snap)?;
                            }
                            splash.on_first_submit();
                            let user_lines = render_user(&prompt);
                            emit_lines(
                                terminal,
                                mode,
                                &mut transcript,
                                &mut scroll,
                                TranscriptEvent::user(prompt.clone(), user_lines),
                            )?;

                            update_main_agent_turn(
                                &mut agent_panel,
                                AgentStatus::Running,
                                &prompt,
                                Some(estimate_input_tokens(&prompt)),
                                None,
                            );
                            focus = FocusArea::Composer;
                            let state = InFlight::new();
                            let rx = backend.start_turn(prompt.clone(), state.cancel_token());
                            chat_rx = Some(bridge_chat_channel(rx));
                            in_flight = Some(state);
                            // T-INLINE-CLUSTER #9: stash the user prompt so the
                            // matching TurnComplete arm can record the completed
                            // (user, assistant) pair into `chat_history`.
                            pending_user_prompt = Some(prompt);
                            dirty = true;
                        }
                        (KeyCode::Backspace, _) => {
                            composer.text.pop();
                            composer.sync_popups(&index, &catalog);
                            dirty = true;
                        }
                        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
                            composer.text.push(c);
                            composer.sync_popups(&index, &catalog);
                            dirty = true;
                        }
                        (KeyCode::PageUp, _) => {
                            scroll.page_up(terminal.size()?.height);
                            dirty = true;
                        }
                        (KeyCode::PageDown, _) => {
                            scroll.page_down(terminal.size()?.height);
                            dirty = true;
                        }
                        (KeyCode::Home, KeyModifiers::CONTROL) => {
                            scroll.scroll_to_top(&transcript, terminal.size()?.width);
                            dirty = true;
                        }
                        (KeyCode::End, KeyModifiers::CONTROL) => {
                            scroll.scroll_to_bottom();
                            dirty = true;
                        }
                        (KeyCode::Down, _)
                            if !composer.slash.active && !composer.mention.active =>
                        {
                            if matches!(focus, FocusArea::Composer)
                                && !agent_panel.agents.is_empty()
                            {
                                focus = FocusArea::Agents;
                                agent_panel.set_focus(true);
                            } else if matches!(focus, FocusArea::Agents) {
                                agent_panel.select_next();
                            }
                            dirty = true;
                        }
                        (KeyCode::Up, _) if matches!(focus, FocusArea::Agents) => {
                            agent_panel.select_prev();
                            dirty = true;
                        }
                        _ => {}
                    }
                }
                Event::Paste(s) => {
                    composer.insert_paste(s, &index, &catalog);
                    dirty = true;
                }
                Event::Mouse(mouse) => {
                    // T-INLINE-CLUSTER #3: route through the pure mapper.
                    // Selection bookkeeping lives here; the OSC52 copy fires
                    // when the user releases the left button (SelectionEnded).
                    match map_mouse_event(mouse, transcript_rect) {
                        MouseAction::ScrollUp(n) => {
                            scroll.line_up_by(n);
                            dirty = true;
                        }
                        MouseAction::ScrollDown(n) => {
                            scroll.line_down_by(n);
                            dirty = true;
                        }
                        MouseAction::SelectionStarted { x, y } => {
                            selection_start = Some((x, y));
                        }
                        MouseAction::SelectionDragged { .. } => {
                            // Selection highlighting renderer is a follow-up;
                            // the end point is captured at SelectionEnded.
                        }
                        MouseAction::SelectionEnded { x, y } => {
                            let payload = selection_text(
                                &transcript,
                                transcript_rect,
                                scroll.offset_from_bottom,
                                selection_start,
                                Some((x, y)),
                            );
                            if !payload.is_empty() {
                                let bytes = payload.len();
                                let _ = osc52::copy_to_clipboard(&payload);
                                toasts.push(Toast::success(format!(
                                    "copied {bytes} bytes from selection"
                                )));
                                dirty = true;
                            }
                            selection_start = None;
                        }
                        MouseAction::StickToBottomToggle(_) | MouseAction::None => {}
                    }
                }
                Event::Resize(_, _) => {
                    scroll.clamp(&transcript, terminal.size()?.width);
                    dirty = true;
                }
                _ => {}
            },
        }
    }
}

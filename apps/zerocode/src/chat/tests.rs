    use super::*;

    /// Async env-lock for `#[tokio::test]` cases that resolve config through
    /// environment variables and hold the guard across await points. Serializes
    /// on a dedicated async mutex and, internally, on the shared sync
    /// `env_test_lock` so sync and async env tests never run concurrently.
    /// Lives here (not in `test_support`) because only the bin target has async
    /// env-dependent tests, so keeping it module-local avoids a lib-target
    /// dead-code suppression.
    async fn env_test_lock_async() -> (
        tokio::sync::MutexGuard<'static, ()>,
        std::sync::MutexGuard<'static, ()>,
    ) {
        static ASYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let async_guard = ASYNC_LOCK.lock().await;
        let sync_guard = crate::test_support::env_test_lock();
        (async_guard, sync_guard)
    }

    fn state() -> ChatState {
        ChatState::new(
            "sess-1".to_string(),
            "myagent".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        )
    }

    #[test]
    fn context_usage_clears_stale_capacity_when_next_route_omits_it() {
        let mut state = state();
        state.apply_update(SessionUpdate::ContextUsage {
            session_id: "sess-1".to_string(),
            input_tokens: Some(100_000),
            max_context_tokens: Some(180_000),
            model_context_window: Some(200_000),
        });
        assert_eq!(state.context_max_tokens, Some(180_000));
        assert_eq!(state.context_model_window, Some(200_000));

        state.apply_update(SessionUpdate::ContextUsage {
            session_id: "sess-1".to_string(),
            input_tokens: Some(12_000),
            max_context_tokens: Some(32_000),
            model_context_window: None,
        });
        assert_eq!(state.context_input_tokens, Some(12_000));
        assert_eq!(state.context_max_tokens, Some(32_000));
        assert_eq!(
            state.context_model_window, None,
            "a compatibility-fallback frame must clear the prior route's capacity"
        );
    }

    fn resume_entry(session_id: &str, agent_alias: &str, was_focused: bool) -> ResumeEntry {
        ResumeEntry {
            session_id: session_id.to_string(),
            agent_alias: agent_alias.to_string(),
            message_count: 0,
            was_focused,
            queue: ReconnectQueueState::default(),
            interrupted: false,
            recovery_required: false,
        }
    }

    fn active_chat() -> Chat {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(rpc));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(ChatState::new(
            "sess-1".to_string(),
            "myagent".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        )));
        chat
    }

    fn draw_todo_close(chat: &mut Chat) -> Rect {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| chat.draw(frame, frame.area()))
            .unwrap();
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected an active session");
        };
        let close = state.todo_close_hit_rect.expect("rendered close control");
        assert_eq!(
            terminal.backend().buffer()[(close.x, close.y)].symbol(),
            "✕"
        );
        close
    }

    #[tokio::test]
    async fn todo_primary_p_reopens_latest_plan_after_hidden_update() {
        use crossterm::event::KeyCode;

        let mut chat = active_chat();
        let ChatPhase::Active(state) = &mut chat.phase else {
            unreachable!()
        };
        state.todo_tracker.set_plan(vec![crate::wire::PlanEntry {
            content: "first".to_string(),
            status: crate::wire::PlanStatus::Pending,
            priority: crate::wire::PlanPriority::Medium,
            active_form: None,
        }]);
        let mut term: crate::config_manager::Term = ratatui::Terminal::with_options(
            crate::terminal_backend::WideCellCleanupBackend::new(std::io::stdout()),
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, 120, 40)),
            },
        )
        .unwrap();

        assert!(
            !chat
                .handle_key(
                    KeyEvent::new(
                        KeyCode::Char('p'),
                        crate::keymap::Chord::primary('p').effective_modifiers(),
                    ),
                    &mut term,
                )
                .await
        );
        let ChatPhase::Active(state) = &mut chat.phase else {
            unreachable!()
        };
        assert!(!state.todo_tracker.is_visible());
        state.todo_tracker.set_plan(vec![crate::wire::PlanEntry {
            content: "updated while hidden".to_string(),
            status: crate::wire::PlanStatus::InProgress,
            priority: crate::wire::PlanPriority::Medium,
            active_form: None,
        }]);
        assert!(!state.todo_tracker.is_visible());

        assert!(
            !chat
                .handle_key(
                    KeyEvent::new(
                        KeyCode::Char('p'),
                        crate::keymap::Chord::primary('p').effective_modifiers(),
                    ),
                    &mut term,
                )
                .await
        );
        let ChatPhase::Active(state) = &chat.phase else {
            unreachable!()
        };
        assert!(state.todo_tracker.is_visible());
        assert_eq!(
            state.todo_tracker.entries()[0].content,
            "updated while hidden"
        );
    }

    #[tokio::test]
    async fn todo_close_uses_rendered_hit_target_without_clearing_session_state() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let mut chat = active_chat();
        let selection = TranscriptSelection {
            anchor: CellPoint { column: 1, row: 2 },
            head: CellPoint { column: 4, row: 2 },
            dragged: true,
        };
        let ChatPhase::Active(state) = &mut chat.phase else {
            unreachable!()
        };
        state.todo_tracker.set_plan(vec![crate::wire::PlanEntry {
            content: "keep this plan".to_string(),
            status: crate::wire::PlanStatus::Pending,
            priority: crate::wire::PlanPriority::Medium,
            active_form: None,
        }]);
        state.input_bar.insert_text("keep this input");
        state.transcript_selection = Some(selection);
        assert!(state.composer_owns_text_input());

        let close = draw_todo_close(&mut chat);
        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: close.x,
                row: close.y,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        )
        .await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("close must keep the session active");
        };
        assert!(!state.todo_tracker.is_visible());
        assert_eq!(state.todo_tracker.entries()[0].content, "keep this plan");
        assert_eq!(state.input_bar.input(), "keep this input");
        assert_eq!(state.transcript_selection, Some(selection));
        assert!(state.composer_owns_text_input());
        assert!(state.todo_close_hit_rect.is_none());
    }

    fn command_action_from_initialize(
        response: serde_json::Value,
        command: &str,
    ) -> InputBarAction {
        let commands = crate::client::parse_initialize_response(&response)
            .expect("matching-version initialize response parses");
        let mut state = ChatState::with_shared_commands(
            "sess-1".to_string(),
            "myagent".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
            &commands.commands,
        );
        state.input_bar.insert_text(command);
        state.input_bar.submit_current_input_for_test()
    }

    #[test]
    fn old_daemon_without_command_catalogue_preserves_shared_actions() {
        let response = serde_json::json!({
            "server_version": env!("CARGO_PKG_VERSION")
        });

        assert!(matches!(
            command_action_from_initialize(response.clone(), "/help"),
            InputBarAction::OpenHelp
        ));
        assert!(matches!(
            command_action_from_initialize(response.clone(), "/model"),
            InputBarAction::OpenModelPicker
        ));
        assert!(matches!(
            command_action_from_initialize(response.clone(), "/new"),
            InputBarAction::RestartSession
        ));
        assert!(matches!(
            command_action_from_initialize(response, "/new-session"),
            InputBarAction::RestartSession
        ));
    }

    #[test]
    fn present_empty_command_catalogue_remains_authoritative() {
        let response = serde_json::json!({
            "server_version": env!("CARGO_PKG_VERSION"),
            "commands": []
        });

        for command in ["/help", "/model", "/new", "/new-session"] {
            match command_action_from_initialize(response.clone(), command) {
                InputBarAction::Submit { text, attachments } => {
                    assert_eq!(text.as_deref(), Some(command));
                    assert!(attachments.is_empty());
                }
                _ => panic!("present empty catalogue must submit {command} as ordinary input"),
            }
        }
    }

    fn transcript_snapshot(area: Rect, rows: &[&str]) -> TranscriptSnapshot {
        use unicode_width::UnicodeWidthChar;

        let mut cells = Vec::with_capacity(usize::from(area.width) * usize::from(area.height));
        for row in 0..area.height {
            let mut column = 0;
            for ch in rows
                .get(usize::from(row))
                .copied()
                .unwrap_or_default()
                .chars()
            {
                if column >= area.width {
                    break;
                }
                let width = (ch.width().unwrap_or(0) as u16)
                    .max(1)
                    .min(area.width - column);
                cells.push(TranscriptCell {
                    symbol: ch.to_string(),
                    span_start: column,
                });
                for _ in 1..width {
                    cells.push(TranscriptCell {
                        symbol: String::new(),
                        span_start: column,
                    });
                }
                column += width;
            }
            while column < area.width {
                cells.push(TranscriptCell {
                    symbol: " ".to_string(),
                    span_start: column,
                });
                column += 1;
            }
        }
        TranscriptSnapshot {
            area,
            cells,
            row_breaks: vec![TranscriptRowBreak::Hard; usize::from(area.height)],
        }
    }

    fn transcript_snapshot_with_row_breaks(
        area: Rect,
        rows: &[&str],
        row_breaks: &[TranscriptRowBreak],
    ) -> TranscriptSnapshot {
        let mut snapshot = transcript_snapshot(area, rows);
        snapshot.row_breaks = row_breaks.to_vec();
        snapshot
    }

    #[test]
    fn transcript_selection_extracts_visible_wrapped_text() {
        let snapshot = transcript_snapshot(Rect::new(10, 5, 8, 2), &["alpha be", "ta gamma"]);
        let forward = TranscriptSelection {
            anchor: CellPoint { column: 6, row: 0 },
            head: CellPoint { column: 1, row: 1 },
            dragged: true,
        };
        let reverse = TranscriptSelection {
            anchor: CellPoint { column: 1, row: 1 },
            head: CellPoint { column: 6, row: 0 },
            dragged: true,
        };
        let click = TranscriptSelection {
            anchor: CellPoint { column: 6, row: 0 },
            head: CellPoint { column: 6, row: 0 },
            dragged: false,
        };

        assert_eq!(snapshot.selected_text(forward).as_deref(), Some("be\nta"));
        assert_eq!(snapshot.selected_text(reverse).as_deref(), Some("be\nta"));
        assert_eq!(snapshot.selected_text(click), None);
    }

    #[test]
    fn copy_transcript_selection_rejoins_soft_wrapped_prose() {
        let snapshot = transcript_snapshot_with_row_breaks(
            Rect::new(0, 0, 12, 4),
            &["The drain", "keeps going", "1. First", "item wraps"],
            &[
                TranscriptRowBreak::Hard,
                TranscriptRowBreak::SoftSpace,
                TranscriptRowBreak::Hard,
                TranscriptRowBreak::SoftSpace,
            ],
        );
        let selection = TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 9, row: 3 },
            dragged: true,
        };

        assert_eq!(
            snapshot.selected_text(selection).as_deref(),
            Some("The drain keeps going\n1. First item wraps")
        );
    }

    #[test]
    fn copy_transcript_selection_rejoins_split_long_tokens_without_spaces() {
        let snapshot = transcript_snapshot_with_row_breaks(
            Rect::new(0, 0, 8, 2),
            &["abcdefgh", "ijkl"],
            &[TranscriptRowBreak::Hard, TranscriptRowBreak::SoftConcat],
        );
        let selection = TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 3, row: 1 },
            dragged: true,
        };

        assert_eq!(
            snapshot.selected_text(selection).as_deref(),
            Some("abcdefghijkl")
        );
    }

    #[test]
    fn copy_transcript_selection_restores_space_after_full_width_prose_row() {
        let snapshot = transcript_snapshot_with_row_breaks(
            Rect::new(0, 0, 10, 2),
            &["12345 6789", "abc"],
            &[TranscriptRowBreak::Hard, TranscriptRowBreak::SoftSpace],
        );
        let selection = TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 2, row: 1 },
            dragged: true,
        };

        assert_eq!(
            snapshot.selected_text(selection).as_deref(),
            Some("12345 6789 abc")
        );
    }

    #[test]
    fn copy_row_breaks_distinguish_spaces_tokens_and_logical_lines() {
        let lines = vec![Line::from("alpha beta"), Line::from("second")];

        assert_eq!(
            row_breaks_for_lines(&lines, 6),
            vec![
                TranscriptRowBreak::Hard,
                TranscriptRowBreak::SoftSpace,
                TranscriptRowBreak::Hard,
            ]
        );
        assert_eq!(
            row_breaks_for_line(&Line::from("abcdefghijkl"), 8),
            vec![TranscriptRowBreak::Hard, TranscriptRowBreak::SoftConcat,]
        );
        assert_eq!(
            row_breaks_for_line(&Line::from("abcdefgh ijkl"), 8),
            vec![TranscriptRowBreak::Hard, TranscriptRowBreak::SoftSpace,]
        );
        assert_eq!(
            row_breaks_for_line(&Line::from("界界界界界"), 8),
            vec![TranscriptRowBreak::Hard, TranscriptRowBreak::SoftConcat,]
        );
        assert_eq!(
            row_breaks_for_line(
                &Line::from(vec![Span::raw("abcdefgh"), Span::raw(" ijkl")]),
                8,
            ),
            vec![TranscriptRowBreak::Hard, TranscriptRowBreak::SoftSpace,]
        );
    }

    #[test]
    fn copy_rendered_selection_uses_source_derived_wrap_separators() {
        use ratatui::{Terminal, backend::TestBackend};

        fn selected_text(message: &str) -> String {
            let mut state = state();
            state
                .entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(message)));
            state.mark_dirty_full();

            let area = Rect::new(0, 0, 10, 8);
            let backend = TestBackend::new(area.width, area.height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render_conversation(frame, &mut state, area);
                })
                .expect("draw conversation");

            let snapshot = state
                .transcript_snapshot
                .as_ref()
                .expect("render captures transcript cells");
            let rows = snapshot
                .cells
                .chunks(usize::from(snapshot.area.width))
                .map(|cells| {
                    cells
                        .iter()
                        .map(|cell| cell.symbol.as_str())
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            let start_row = rows
                .iter()
                .position(|row| row.starts_with("abcdefgh"))
                .expect("first wrapped row") as u16;
            let end_row = start_row + 1;
            let end_column = snapshot
                .row_text_bounds(end_row)
                .expect("second wrapped row")
                .1;
            snapshot
                .selected_text(TranscriptSelection {
                    anchor: CellPoint {
                        column: 0,
                        row: start_row,
                    },
                    head: CellPoint {
                        column: end_column,
                        row: end_row,
                    },
                    dragged: true,
                })
                .expect("rendered selection text")
        }

        assert_eq!(selected_text("abcdefgh ijkl"), "abcdefgh ijkl");
        assert_eq!(selected_text("abcdefghijkl"), "abcdefghijkl");
        assert_eq!(
            selected_text("abcdefgh\u{00a0}ijkl"),
            "abcdefgh\u{00a0}ijkl"
        );
        assert_eq!(selected_text("abcdefgh\u{200b}ijkl"), "abcdefghijkl");
    }

    #[test]
    fn copy_context_menu_is_clamped_to_conversation_bounds() {
        use unicode_width::UnicodeWidthStr;

        let bounds = Rect::new(10, 5, 20, 8);
        let menu_width = (UnicodeWidthStr::width(context_menu_copy_label().as_str()) as u16 + 4)
            .min(bounds.width)
            .max(3);

        assert_eq!(
            context_menu_rect(29, 12, bounds, TRANSCRIPT_CONTEXT_ACTIONS),
            Some(Rect::new(
                bounds.x + bounds.width - menu_width,
                bounds.y + bounds.height - 3,
                menu_width,
                3,
            ))
        );
        assert_eq!(
            context_menu_rect(0, 0, bounds, TRANSCRIPT_CONTEXT_ACTIONS)
                .unwrap()
                .x,
            bounds.x
        );
        assert_eq!(
            context_menu_rect(0, 0, bounds, TRANSCRIPT_CONTEXT_ACTIONS)
                .unwrap()
                .y,
            bounds.y
        );
    }

    #[test]
    fn context_menu_targets_message_from_side_whitespace() {
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.transcript_snapshot = Some(transcript_snapshot(Rect::new(10, 5, 30, 3), &["hello"]));
        state.entry_rects.push((0, Rect::new(10, 5, 5, 1)));

        assert!(state.open_transcript_context_menu(35, 5));
        let menu = state.context_menu.as_ref().expect("menu opens");
        let ChatContextMenuTarget::Transcript(target) = &menu.target else {
            panic!("transcript target");
        };
        assert_eq!(target.kind, CopyHitKind::Message);
        assert_eq!(target.text.as_ref(), "hello");
    }

    #[test]
    fn context_menu_prefers_active_selected_text_until_selection_is_cleared() {
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.transcript_snapshot = Some(transcript_snapshot(Rect::new(10, 5, 10, 3), &["hello"]));
        state.entry_rects.push((0, Rect::new(10, 5, 5, 1)));
        state.transcript_selection = Some(TranscriptSelection {
            anchor: CellPoint { column: 1, row: 0 },
            head: CellPoint { column: 3, row: 0 },
            dragged: true,
        });

        assert!(state.open_transcript_context_menu(12, 5));
        let menu = state.context_menu.as_ref().expect("menu opens");
        let ChatContextMenuTarget::Transcript(target) = &menu.target else {
            panic!("transcript target");
        };
        assert_eq!(target.kind, CopyHitKind::Transcript);
        assert_eq!(target.text.as_ref(), "ell");

        state.dismiss_context_menu();
        assert!(state.open_transcript_context_menu(10, 5));
        let menu = state.context_menu.as_ref().expect("menu opens");
        let ChatContextMenuTarget::Transcript(target) = &menu.target else {
            panic!("transcript target");
        };
        assert_eq!(target.kind, CopyHitKind::Transcript);
        assert_eq!(target.text.as_ref(), "ell");

        state.dismiss_context_menu();
        state.clear_transcript_selection();
        assert!(state.open_transcript_context_menu(10, 5));
        let menu = state.context_menu.as_ref().expect("menu opens");
        let ChatContextMenuTarget::Transcript(target) = &menu.target else {
            panic!("transcript target");
        };
        assert_eq!(target.kind, CopyHitKind::Message);
        assert_eq!(target.text.as_ref(), "hello");
    }

    #[test]
    fn empty_character_selection_never_falls_back_to_another_copy_target() {
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.transcript_snapshot = Some(transcript_snapshot(
            Rect::new(10, 5, 10, 2),
            &["hello", "   "],
        ));
        state.entry_rects.push((0, Rect::new(10, 5, 5, 1)));
        state.browse_cursor = Some(0);
        state.transcript_selection = Some(TranscriptSelection {
            anchor: CellPoint { column: 0, row: 1 },
            head: CellPoint { column: 2, row: 1 },
            dragged: true,
        });

        assert!(state.current_selection_text().is_empty());
        assert!(!state.open_transcript_context_menu(10, 5));
        assert!(state.context_menu.is_none());
    }

    #[test]
    fn context_menu_prefers_code_block_over_containing_message() {
        let mut state = state();
        state.entries.push(ChatEntry::AgentMessage(Arc::<str>::from(
            "before\n```sh\necho hi\n```\nafter",
        )));
        state.transcript_snapshot = Some(transcript_snapshot(
            Rect::new(0, 0, 40, 6),
            &["before", "code", "echo hi", "", "after"],
        ));
        state.entry_rects.push((0, Rect::new(0, 0, 20, 5)));
        state.context_copy_regions.push(CopyHitRegion {
            rect: Rect::new(0, 1, 40, 3),
            text: Arc::<str>::from("echo hi"),
            kind: CopyHitKind::Code,
            group: 7,
        });

        assert!(state.open_transcript_context_menu(2, 2));
        let menu = state.context_menu.as_ref().expect("menu opens");
        let ChatContextMenuTarget::Transcript(target) = &menu.target else {
            panic!("transcript target");
        };
        assert_eq!(target.kind, CopyHitKind::Code);
        assert_eq!(target.text.as_ref(), "echo hi");
        assert_eq!(target.group, 7);
    }

    #[test]
    fn context_menu_dismissal_does_not_change_selection() {
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.transcript_snapshot = Some(transcript_snapshot(
            Rect::new(0, 0, 10, 3),
            &["hello", "", ""],
        ));
        state.entry_rects.push((0, Rect::new(0, 0, 5, 1)));
        state.browse_cursor = Some(0);
        state.dirty = LinesDirty::Clean;

        assert!(state.open_transcript_context_menu(1, 0));
        assert_eq!(state.dirty, LinesDirty::Clean);
        state.dismiss_context_menu();

        assert!(state.context_menu.is_none());
        assert_eq!(state.browse_cursor, Some(0));
        assert!(state.info_message.is_none());
        assert_eq!(state.copy_feedback, None);
        assert_eq!(state.dirty, LinesDirty::Clean);
    }

    #[test]
    fn context_menu_request_keeps_the_exact_transcript_target() {
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.transcript_snapshot = Some(transcript_snapshot(Rect::new(0, 0, 10, 1), &["hello"]));
        state.transcript_selection = Some(TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 1, row: 0 },
            dragged: true,
        });
        state.browse_cursor = Some(0);
        state.context_menu = Some(ChatContextMenu {
            rect: Rect::new(0, 0, 8, 3),
            target: ChatContextMenuTarget::Transcript(CopyHitRegion {
                rect: Rect::new(0, 0, 5, 1),
                text: Arc::<str>::from("hello"),
                kind: CopyHitKind::Message,
                group: 0,
            }),
            selected: 0,
        });

        let request = state.take_context_menu_request().expect("copy request");
        assert!(state.context_menu.is_none());
        assert!(matches!(
            request,
            ChatContextMenuRequest::CopyTranscript(CopyHitRegion {
                kind: CopyHitKind::Message,
                text,
                ..
            }) if text.as_ref() == "hello"
        ));
    }

    #[test]
    fn keyboard_copy_prefers_character_selection_then_browse_selection() {
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("whole message")));
        state.browse_cursor = Some(0);
        state.transcript_snapshot = Some(transcript_snapshot(
            Rect::new(0, 0, 12, 1),
            &["visible text"],
        ));
        state.transcript_selection = Some(TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 6, row: 0 },
            dragged: true,
        });

        assert_eq!(state.current_selection_text(), "visible");
        state.clear_transcript_selection();
        assert_eq!(state.current_selection_text(), "whole message");
        state.browse_cursor = None;
        assert!(state.current_selection_text().is_empty());
    }

    #[test]
    fn keyboard_copy_clears_character_and_browse_selection() {
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("whole message")));
        state.transcript_snapshot = Some(transcript_snapshot(
            Rect::new(0, 0, 12, 1),
            &["visible text"],
        ));
        state.transcript_selection = Some(TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 6, row: 0 },
            dragged: true,
        });
        state.dirty = LinesDirty::Clean;

        assert!(state.copy_current_selection());
        assert_eq!(state.transcript_selection, None);
        assert_eq!(state.browse_cursor, None);
        assert!(state.info_message.is_some());
        assert!(matches!(
            state.copy_feedback,
            Some(CopyFeedback {
                target: CopyFeedbackTarget::Overlay(_),
                ..
            })
        ));
        assert_eq!(state.dirty, LinesDirty::Clean);

        state.browse_cursor = Some(0);
        state.dirty = LinesDirty::Clean;
        state.copy_hit_regions.push(CopyHitRegion {
            rect: Rect::new(0, 0, 8, 1),
            text: Arc::<str>::from("whole message"),
            kind: CopyHitKind::Message,
            group: 0,
        });
        assert!(state.copy_current_selection());
        assert_eq!(state.browse_cursor, None);
        assert_eq!(state.dirty, LinesDirty::Full);
        assert!(matches!(
            state.copy_feedback,
            Some(CopyFeedback {
                target: CopyFeedbackTarget::Overlay(_),
                ..
            })
        ));
    }

    #[test]
    fn copy_shortcuts_do_not_swallow_normal_y_input() {
        use crate::keymap::ChatTabAction;
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut state = state();
        let y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        let command_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::SUPER);
        let terminal_copy = KeyEvent::new(
            KeyCode::Char('C'),
            KeyModifiers::CONTROL.union(KeyModifiers::SHIFT),
        );
        let bare_custom_copy_all = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE);

        assert!(!should_copy_current_selection(&state, &y));

        state.transcript_snapshot = Some(transcript_snapshot(Rect::new(0, 0, 5, 1), &["hello"]));
        state.transcript_selection = Some(TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 1, row: 0 },
            dragged: true,
        });
        assert!(!should_copy_current_selection(&state, &y));
        assert!(should_copy_current_selection(&state, &command_c));
        assert!(should_copy_current_selection(&state, &terminal_copy));
        assert!(!should_copy_action(
            &state,
            &bare_custom_copy_all,
            Some(ChatTabAction::CopyAllVisible),
        ));

        state.transcript_selection = None;
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.browse_cursor = Some(0);
        assert!(should_copy_current_selection(&state, &y));
    }

    #[test]
    fn transcript_selection_expands_wide_character_cells() {
        let snapshot = transcript_snapshot(Rect::new(0, 0, 4, 1), &["A界B"]);
        let selection = TranscriptSelection {
            anchor: CellPoint { column: 2, row: 0 },
            head: CellPoint { column: 3, row: 0 },
            dragged: true,
        };

        assert!(snapshot.has_text_at(CellPoint { column: 2, row: 0 }));
        assert_eq!(
            snapshot.selection_bounds(selection),
            Some((
                CellPoint { column: 1, row: 0 },
                CellPoint { column: 3, row: 0 }
            ))
        );
        assert_eq!(snapshot.selected_text(selection).as_deref(), Some("界B"));
    }

    #[test]
    fn transcript_selection_drag_is_limited_to_conversation_body() {
        let mut state = state();
        state.transcript_snapshot = Some(transcript_snapshot(
            Rect::new(10, 5, 8, 2),
            &["alpha be", "ta gamma"],
        ));

        assert!(!state.begin_transcript_drag(2, 1));
        assert_eq!(state.transcript_selection, None);

        assert!(state.begin_transcript_drag(16, 5));
        assert!(state.update_transcript_drag(11, 6));
        state.finish_transcript_drag();
        assert_eq!(state.transcript_selected_text().as_deref(), Some("be\nta"));
        assert_eq!(state.copy_feedback, None);
        assert!(state.info_message.is_none());
    }

    #[test]
    fn transcript_selection_drag_can_start_in_side_whitespace() {
        let mut state = state();
        state.transcript_snapshot = Some(transcript_snapshot(Rect::new(10, 5, 8, 1), &["alpha"]));

        assert!(state.begin_transcript_drag(17, 5));
        assert!(state.update_transcript_drag(16, 5));
        let snapshot = state.transcript_snapshot.as_ref().unwrap();
        let selection = state.transcript_selection.unwrap();
        assert_eq!(snapshot.selected_text(selection).as_deref(), Some("a"));
        assert_eq!(
            snapshot.selection_bounds(selection),
            Some((
                CellPoint { column: 4, row: 0 },
                CellPoint { column: 4, row: 0 }
            ))
        );

        assert!(state.update_transcript_drag(10, 5));
        state.finish_transcript_drag();

        assert_eq!(state.transcript_selected_text().as_deref(), Some("alpha"));
    }

    #[test]
    fn transcript_selection_side_whitespace_click_still_dismisses() {
        let mut state = state();
        state.transcript_snapshot = Some(transcript_snapshot(Rect::new(10, 5, 8, 1), &["alpha"]));

        assert!(state.begin_transcript_drag(17, 5));
        state.finish_transcript_drag();

        assert_eq!(state.transcript_selection, None);
    }

    #[test]
    fn transcript_selection_empty_row_cannot_start_drag() {
        let mut state = state();
        state.transcript_snapshot =
            Some(transcript_snapshot(Rect::new(10, 5, 8, 2), &["alpha", ""]));

        assert!(!state.begin_transcript_drag(17, 6));
        assert_eq!(state.transcript_selection, None);
    }

    #[test]
    fn transcript_selection_clears_on_scroll_and_session_reset() {
        let mut state = state();
        state.transcript_snapshot = Some(transcript_snapshot(Rect::new(0, 0, 5, 1), &["hello"]));
        assert!(state.begin_transcript_drag(0, 0));
        assert!(state.update_transcript_drag(1, 0));
        state.finish_transcript_drag();
        state.set_overlay_copy_feedback(Rect::new(0, 0, 5, 1));

        state.scroll_up(1);
        assert_eq!(state.transcript_selection, None);
        assert_eq!(state.copy_feedback, None);
        assert!(state.copy_hit_regions.is_empty());

        assert!(state.begin_transcript_drag(0, 0));
        assert!(state.update_transcript_drag(1, 0));
        state.finish_transcript_drag();
        state.scroll_to_top();
        assert_eq!(state.transcript_selection, None);

        assert!(state.begin_transcript_drag(0, 0));
        assert!(state.update_transcript_drag(1, 0));
        state.finish_transcript_drag();
        state.last_total_rows = 10;
        state.last_inner_height = 1;
        state.scroll_to_bottom();
        assert_eq!(state.transcript_selection, None);

        assert!(state.begin_transcript_drag(0, 0));
        assert!(state.update_transcript_drag(1, 0));
        state.finish_transcript_drag();
        state.enter_browse_mode();
        assert_eq!(state.transcript_selection, None);

        state.exit_browse_mode();
        assert!(state.begin_transcript_drag(0, 0));
        assert!(state.update_transcript_drag(1, 0));
        state.finish_transcript_drag();
        state.reset_for_session(
            "sess-2".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        assert_eq!(state.transcript_selection, None);
        assert!(state.transcript_snapshot.is_none());
    }

    #[test]
    fn transcript_selection_clears_when_snapshot_changes() {
        let replacements = [
            (
                "geometry",
                transcript_snapshot(Rect::new(0, 0, 6, 1), &["hello "]),
            ),
            (
                "content",
                transcript_snapshot(Rect::new(0, 0, 5, 1), &["hullo"]),
            ),
        ];

        for (case, replacement) in replacements {
            let mut state = state();
            state.transcript_snapshot =
                Some(transcript_snapshot(Rect::new(0, 0, 5, 1), &["hello"]));
            assert!(state.begin_transcript_drag(0, 0));
            assert!(state.update_transcript_drag(1, 0));
            state.copy_hit_regions.push(CopyHitRegion {
                rect: Rect::new(0, 0, 2, 1),
                text: Arc::<str>::from("he"),
                kind: CopyHitKind::Transcript,
                group: 0,
            });
            state.copy_feedback = Some(CopyFeedback {
                target: CopyFeedbackTarget::Overlay(Rect::new(0, 0, 2, 1)),
                shown_at: Instant::now(),
            });

            state.set_transcript_snapshot(replacement);

            assert_eq!(state.transcript_selection, None, "{case} selection");
            assert!(state.copy_hit_regions.is_empty(), "{case} copy regions");
            assert_eq!(state.copy_feedback, None, "{case} copy feedback");
        }
    }

    #[tokio::test]
    async fn transcript_selection_rendered_drag_excludes_chrome() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello world")));
        state.mark_dirty_full();

        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, &mut state, area, PaneKind::Chat))
            .expect("draw chat");

        let snapshot = state
            .transcript_snapshot
            .as_ref()
            .expect("render captures transcript cells");
        assert!(
            snapshot.area.y > area.y,
            "panel chrome stays outside snapshot"
        );
        let (text_row, text_col) = snapshot
            .cells
            .chunks(usize::from(snapshot.area.width))
            .enumerate()
            .find_map(|(row, cells)| {
                cells
                    .iter()
                    .map(|cell| cell.symbol.as_str())
                    .collect::<String>()
                    .find("hello")
                    .map(|column| (row as u16, column as u16))
            })
            .expect("rendered transcript contains message text");
        let start_col = snapshot.area.x + text_col;
        let start_row = snapshot.area.y + text_row;
        chat.phase = ChatPhase::Active(Box::new(state));

        for event in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            let column = if matches!(event, MouseEventKind::Down(_)) {
                start_col
            } else {
                start_col + 4
            };
            chat.handle_mouse(
                MouseEvent {
                    kind: event,
                    column,
                    row: start_row,
                    modifiers: KeyModifiers::NONE,
                },
                area,
            )
            .await;
        }

        let ChatPhase::Active(state) = &mut chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.transcript_selected_text().as_deref(), Some("hello"));
        assert!(!state.begin_transcript_drag(area.x, area.y));
        assert_eq!(state.transcript_selection, None);
    }

    #[tokio::test]
    async fn transcript_selection_copy_action_is_explicit() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state.transcript_snapshot = Some(transcript_snapshot(
            Rect::new(1, 1, 20, 1),
            &["hello               "],
        ));
        assert!(state.begin_transcript_drag(1, 1));
        assert!(state.update_transcript_drag(2, 1));
        state.finish_transcript_drag();

        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_transcript_copy_overlay(frame, &mut state))
            .expect("draw copy action");
        let region = state
            .copy_hit_regions
            .iter()
            .find(|region| region.kind == CopyHitKind::Transcript)
            .cloned()
            .expect("selection exposes transcript copy action");
        assert_eq!(region.text.as_ref(), "he");
        assert_eq!(state.copy_feedback, None);

        chat.phase = ChatPhase::Active(Box::new(state));
        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: region.rect.x,
                row: region.rect.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
        .await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.transcript_selection, None);
        assert!(matches!(
            state.copy_feedback,
            Some(CopyFeedback {
                target: CopyFeedbackTarget::Overlay(_),
                ..
            })
        ));
        assert!(state.info_message.is_some());
    }

    #[tokio::test]
    async fn copy_shift_or_option_click_extends_character_selection_from_original_anchor() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state.transcript_snapshot = Some(transcript_snapshot(
            Rect::new(10, 5, 12, 2),
            &["alpha beta", "gamma"],
        ));
        assert!(state.begin_transcript_drag(16, 5));
        assert!(state.update_transcript_drag(19, 5));
        state.finish_transcript_drag();

        chat.phase = ChatPhase::Active(Box::new(state));
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            chat.handle_mouse(
                MouseEvent {
                    kind,
                    column: 21,
                    row: 6,
                    modifiers: KeyModifiers::ALT,
                },
                Rect::new(0, 0, 80, 20),
            )
            .await;
        }

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(
            state.transcript_selection,
            Some(TranscriptSelection {
                anchor: CellPoint { column: 6, row: 0 },
                head: CellPoint { column: 11, row: 1 },
                dragged: true,
            })
        );
        assert_eq!(
            state.transcript_selected_text().as_deref(),
            Some("beta\ngamma")
        );

        for (column, row, expected) in [
            (
                11,
                5,
                Some(TranscriptSelection {
                    anchor: CellPoint { column: 6, row: 0 },
                    head: CellPoint { column: 1, row: 0 },
                    dragged: true,
                }),
            ),
            (16, 5, None),
        ] {
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                chat.handle_mouse(
                    MouseEvent {
                        kind,
                        column,
                        row,
                        modifiers: KeyModifiers::SHIFT,
                    },
                    Rect::new(0, 0, 80, 20),
                )
                .await;
            }

            let ChatPhase::Active(state) = &chat.phase else {
                panic!("expected active chat");
            };
            assert_eq!(state.transcript_selection, expected);
        }

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: 5,
                modifiers: KeyModifiers::SHIFT,
            },
            Rect::new(0, 0, 80, 20),
        )
        .await;
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(
            state.transcript_selection,
            Some(TranscriptSelection {
                anchor: CellPoint { column: 0, row: 0 },
                head: CellPoint { column: 0, row: 0 },
                dragged: false,
            })
        );

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 10,
                row: 5,
                modifiers: KeyModifiers::SHIFT,
            },
            Rect::new(0, 0, 80, 20),
        )
        .await;
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.transcript_selection, None);
    }

    #[tokio::test]
    async fn copy_shift_click_keeps_browse_mode_message_selection() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("first")));
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("second")));
        state.browse_cursor = Some(0);
        state.entry_rects = vec![(0, Rect::new(2, 3, 20, 1)), (1, Rect::new(2, 4, 20, 1))];
        chat.phase = ChatPhase::Active(Box::new(state));

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 2,
                row: 4,
                modifiers: KeyModifiers::SHIFT,
            },
            Rect::new(0, 0, 80, 20),
        )
        .await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.browse_anchor, Some(0));
        assert_eq!(state.browse_cursor, Some(1));
        assert_eq!(state.transcript_selection, None);
    }

    #[tokio::test]
    async fn scrollbar_drag_works_outside_browse_mode() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state.last_total_rows = 100;
        state.last_inner_height = 20;
        state.scrollbar_track_rect = Some(Rect::new(79, 2, 1, 10));
        chat.phase = ChatPhase::Active(Box::new(state));

        for (kind, row) in [
            (MouseEventKind::Down(MouseButton::Left), 4),
            (MouseEventKind::Drag(MouseButton::Left), 8),
        ] {
            chat.handle_mouse(
                MouseEvent {
                    kind,
                    column: 79,
                    row,
                    modifiers: KeyModifiers::NONE,
                },
                Rect::new(0, 0, 80, 20),
            )
            .await;
        }

        {
            let ChatPhase::Active(state) = &chat.phase else {
                panic!("expected active chat");
            };
            assert!(state.scrollbar_drag.is_some());
            assert!(state.scroll_offset > 0);
            assert_eq!(state.transcript_selection, None);
        }

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 79,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 80, 20),
        )
        .await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert!(state.scrollbar_drag.is_none());
    }

    #[test]
    fn hidden_tracker_leaves_full_area_for_body() {
        let t = crate::todo_tracker::TodoTracker::new(
            crate::todo_tracker::TodoLocation::Right,
            true,
            false,
        ); // hidden, no plan
        let full = Rect::new(0, 0, 100, 40);
        let (body, tracker) = carve_todo_area(&t, full);
        assert_eq!(body, full);
        assert!(tracker.is_none());
    }

    #[test]
    fn visible_right_tracker_carves_column() {
        let mut t = crate::todo_tracker::TodoTracker::new(
            crate::todo_tracker::TodoLocation::Right,
            true,
            true,
        );
        t.set_plan(vec![crate::wire::PlanEntry {
            content: "A".into(),
            status: crate::wire::PlanStatus::Pending,
            priority: crate::wire::PlanPriority::Medium,
            active_form: None,
        }]);
        let full = Rect::new(0, 0, 100, 40);
        let (body, tracker) = carve_todo_area(&t, full);
        let tracker = tracker.expect("visible tracker gets an area");
        assert_eq!(body.width + tracker.width, full.width);
        assert_eq!(tracker.width, 32);
        assert_eq!(body.height, full.height);
    }

    #[test]
    fn tracker_width_is_clamped_on_narrow_terminals() {
        let t = crate::todo_tracker::TodoTracker::new(
            crate::todo_tracker::TodoLocation::Right,
            true,
            true,
        );
        let full = Rect::new(0, 0, 40, 20); // narrow
        let (_body, tracker) = carve_todo_area(&t, full);
        let tracker = tracker.expect("side panel visible");
        assert!(tracker.width <= full.width / 2, "clamped to <= 50% width");
    }

    async fn next_rpc_request(rx: &mut mpsc::Receiver<String>, reason: &str) -> serde_json::Value {
        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap_or_else(|_| panic!("{reason}"))
            .expect("RPC request channel should stay open");
        serde_json::from_str(&line).expect("RPC request should be JSON")
    }

    /// Answer every follow-up request a transition emits (session/close,
    /// model identity refresh, history load) with a permissive empty result,
    /// until the pane goes quiet. Keeps transition tests focused on the
    /// behavior under test rather than on exact RPC choreography.
    async fn drain_pending_requests(rx: &mut mpsc::Receiver<String>, rpc: &RpcOutbound) {
        while let Ok(Some(line)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await
        {
            let request: serde_json::Value =
                serde_json::from_str(&line).expect("RPC request should be JSON");
            if let Some(id) = request["id"].as_str() {
                rpc.dispatch_response(
                    id,
                    Some(serde_json::json!({ "messages": [], "sessions": [] })),
                    None,
                );
            }
        }
    }

    fn respond_ok(rpc: &RpcOutbound, request: &serde_json::Value, result: serde_json::Value) {
        let id = request["id"]
            .as_str()
            .expect("RPC request should have an id");
        rpc.dispatch_response(id, Some(result), None);
    }

    fn respond_err(rpc: &RpcOutbound, request: &serde_json::Value, code: i32, message: &str) {
        let id = request["id"]
            .as_str()
            .expect("RPC request should have an id");
        rpc.dispatch_response(
            id,
            None,
            Some(crate::jsonrpc::JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        );
    }

    /// B1 risk #2, automated: a bounded-range off-by-one can still satisfy the
    /// line-count assertions the other tests make, and would surface only as
    /// mis-aimed clicks and wrong copy regions in a real terminal.
    ///
    /// This sweeps EVERY scroll offset over a wrapped history (prose plus code
    /// fences) and pins the coordinate invariants that scrolling, bottom
    /// anchoring, copy targets and `entry_rects` all read:
    ///   1. the resolved entry range covers the whole viewport window, so no
    ///      visible row is projected from outside the range;
    ///   2. the line-level window covers the viewport and `local_scroll`
    ///      indexes a real row of the slice;
    ///   3. the materialized line count is exactly the resolved line window.
    #[test]
    fn visible_range_coordinate_invariants_hold_at_every_scroll_offset() {
        let mut s = state();
        for i in 0..120 {
            if i % 5 == 0 {
                s.entries
                    .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                        "entry {i} with a deliberately long prose line that must wrap across \
                         several screen rows at this width so screen and line coordinates diverge"
                    ))));
            } else if i % 5 == 1 {
                s.entries
                    .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                        "```rust\nfn generated_{i}() -> usize {{\n    {i}\n}}\n```"
                    ))));
            } else {
                s.entries
                    .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                        "short {i}"
                    ))));
            }
        }
        s.mark_dirty_full();
        let width = 80u16;
        s.rebuild_lines(width);

        let total_rows = s.cached_total_rows;
        assert!(
            total_rows > 200,
            "expected a history far deeper than one viewport, got {total_rows} rows"
        );

        for height in [1u16, 7, 20, 41] {
            for scroll in 0..=total_rows {
                let window = s.visible_cached_window(scroll, height);
                let (slice, local_scroll) = s.visible_line_slice(scroll, &window);

                if window.entries.is_empty() {
                    assert!(
                        slice.is_empty(),
                        "empty entry range must project no lines \
                         (height={height}, scroll={scroll})"
                    );
                    continue;
                }

                // 1. The range must cover the viewport window it was resolved for.
                let screen_lo = s.cached_screen_ranges[window.entries.start].1;
                let screen_hi = s.cached_screen_ranges[window.entries.end - 1].2;
                let view_end = scroll.saturating_add(height).min(total_rows);
                assert!(
                    screen_lo <= scroll,
                    "range starts below the viewport top: screen_lo={screen_lo} > scroll={scroll} \
                     (height={height})"
                );
                assert!(
                    screen_hi >= view_end,
                    "range ends above the viewport bottom: screen_hi={screen_hi} < \
                     view_end={view_end} (height={height}, scroll={scroll})"
                );

                // 2. The line-level range and local scroll must cover the same
                // viewport without materializing the complete boundary entry.
                let line_screen_hi = s.cached_line_screen_ranges[window.lines.end - 1].1;
                assert!(window.screen_lo <= scroll);
                assert!(line_screen_hi >= view_end);
                assert!(
                    (local_scroll as usize) <= slice.len(),
                    "local_scroll={local_scroll} outside slice of {} rows \
                     (height={height}, scroll={scroll})",
                    slice.len()
                );
                assert_eq!(
                    local_scroll,
                    scroll - window.screen_lo,
                    "local_scroll must be the offset of the viewport into the first visible line \
                     (height={height}, scroll={scroll})"
                );

                // 3. Materialization must match the resolved line range.
                assert_eq!(
                    window.lines.len(),
                    slice.len(),
                    "line window spans {} lines but the renderer drew {} \
                     (height={height}, scroll={scroll})",
                    window.lines.len(),
                    slice.len()
                );
            }
        }
    }

    async fn apply_next_reattach_result(chat: &mut Chat, reason: &str) {
        let update = tokio::time::timeout(Duration::from_secs(2), chat.session_reattach_rx.recv())
            .await
            .unwrap_or_else(|_| panic!("{reason}"))
            .expect("session reattach result channel should stay open");
        chat.apply_session_reattach_result(update);
    }

    #[test]
    fn visible_line_slice_renders_only_the_viewport_not_the_whole_history() {
        let mut s = state();
        for i in 0..400 {
            s.entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                    "line entry number {i}"
                ))));
        }
        s.mark_dirty_full();
        let width = 80u16;
        s.rebuild_lines(width);

        let total = s.cached_lines.len();
        assert!(total > 100, "expected a deep history, got {total} lines");

        let height = 20u16;
        let max_scroll = s.cached_total_rows.saturating_sub(height);
        let mid_scroll = max_scroll / 2;

        let window = s.visible_cached_window(mid_scroll, height);
        let (slice, local_scroll) = s.visible_line_slice(mid_scroll, &window);

        assert!(
            slice.len() < total,
            "viewport slice ({}) must be smaller than full history ({total})",
            slice.len()
        );
        assert!(
            slice.len() <= (height as usize) + 8,
            "viewport slice ({}) should be bounded near the viewport height ({height}), not the history",
            slice.len()
        );
        assert!(
            local_scroll < height,
            "local scroll ({local_scroll}) must land inside the first visible entry, below viewport height ({height})"
        );
    }

    #[test]
    fn visible_line_slice_handles_top_and_bottom_extents() {
        let mut s = state();
        for i in 0..50 {
            s.entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                    "entry {i}"
                ))));
        }
        s.mark_dirty_full();
        s.rebuild_lines(80);
        let height = 12u16;

        let top_window = s.visible_cached_window(0, height);
        let (top, top_local) = s.visible_line_slice(0, &top_window);
        assert_eq!(top_local, 0, "scroll 0 keeps the first entry aligned");
        assert!(!top.is_empty());

        let max_scroll = s.cached_total_rows.saturating_sub(height);
        let bottom_window = s.visible_cached_window(max_scroll, height);
        let (bottom, _) = s.visible_line_slice(max_scroll, &bottom_window);
        assert!(!bottom.is_empty(), "bottom extent must still yield lines");
    }

    /// Transient-inclusive total row count (`cached_total_rows +
    /// overlay_rows`), computed the same way the transient branch of
    /// `render_conversation` does.
    fn transient_total_rows(s: &ChatState, overlay: &[Line<'static>], width: u16) -> u16 {
        let overlay_rows = Paragraph::new(overlay.iter().map(borrow_line).collect::<Vec<_>>())
            .wrap(Wrap { trim: false })
            .line_count(width) as u16;
        s.cached_total_rows.saturating_add(overlay_rows)
    }

    fn approval() -> PendingApproval {
        PendingApproval {
            request_id: "req-1".to_string(),
            tool_name: "shell".to_string(),
            arguments_summary: "ls".to_string(),
            timeout_secs: 30,
        }
    }

    #[test]
    fn visible_transient_slice_bounded_not_o_of_history() {
        let mut s = state();
        for i in 0..400 {
            s.entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                    "line entry number {i}"
                ))));
        }
        s.mark_dirty_full();
        let width = 80u16;
        s.rebuild_lines(width);
        s.streaming_text = "streaming reply in progress".to_string();
        s.pending_approval = Some(approval());

        let overlay = s.build_overlay_lines(width);
        let total_rows = transient_total_rows(&s, &overlay, width);
        let height = 20u16;
        let max_scroll = total_rows.saturating_sub(height);
        let mid_scroll = max_scroll / 2;

        let window = s.visible_cached_window(mid_scroll, height);
        let (slice, local_scroll) = s.visible_transient_slice(mid_scroll, height, &window, overlay);

        assert!(
            slice.len() <= (height as usize) + 8,
            "transient slice ({}) should be bounded near the viewport height ({height}), not the history",
            slice.len()
        );
        assert!(
            local_scroll < height + APPROVAL_OVERLAY_HEIGHT,
            "local scroll ({local_scroll}) must land inside the visible window"
        );
    }

    #[test]
    fn visible_transient_slice_bottom_anchored_includes_overlay() {
        let mut s = state();
        for i in 0..30 {
            s.entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                    "entry {i}"
                ))));
        }
        s.mark_dirty_full();
        let width = 80u16;
        s.rebuild_lines(width);
        s.streaming_text = "unique-streaming-marker currently in flight".to_string();
        s.pinned_to_bottom = true;

        let overlay = s.build_overlay_lines(width);
        let total_rows = transient_total_rows(&s, &overlay, width);
        let height = 12u16;
        let max_scroll = total_rows.saturating_sub(height);

        let window = s.visible_cached_window(max_scroll, height);
        let (slice, local_scroll) = s.visible_transient_slice(max_scroll, height, &window, overlay);

        let joined: String = slice
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(
            joined.contains("unique-streaming-marker"),
            "bottom-anchored slice must contain the streaming overlay text, got: {joined:?}"
        );
        assert!(
            (local_scroll as usize) < slice.len() + height as usize,
            "local scroll ({local_scroll}) must position the overlay's tail within the viewport"
        );
    }

    #[test]
    fn visible_transient_slice_mid_history_scroll_matches_idle_slice() {
        let mut s = state();
        for i in 0..400 {
            s.entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                    "line entry number {i}"
                ))));
        }
        s.mark_dirty_full();
        let width = 80u16;
        s.rebuild_lines(width);
        s.streaming_text = "should-not-appear-mid-history streaming text".to_string();
        s.pinned_to_bottom = false;

        let overlay = s.build_overlay_lines(width);
        let height = 20u16;
        // Scroll far above the bottom so the window sits entirely in history.
        let scroll = 10u16;

        let window = s.visible_cached_window(scroll, height);
        let (transient_slice, transient_local) =
            s.visible_transient_slice(scroll, height, &window, overlay);
        let (idle_slice, idle_local) = s.visible_line_slice(scroll, &window);

        assert_eq!(
            transient_slice, idle_slice,
            "a window entirely within history must match the idle-path slice"
        );
        assert_eq!(transient_local, idle_local);

        let joined: String = transient_slice
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(
            !joined.contains("should-not-appear-mid-history"),
            "a window entirely within history must not include overlay content"
        );
    }

    #[test]
    fn transient_total_rows_matches_full_concatenation() {
        let width_cases = [80u16, 24u16];
        for width in width_cases {
            for (streaming, thinking, approve) in [
                (true, false, false),
                (false, false, true),
                (true, true, true),
            ] {
                let mut s = state();
                for i in 0..10 {
                    s.entries
                        .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                            "entry {i}"
                        ))));
                }
                s.mark_dirty_full();
                s.rebuild_lines(width);
                if streaming {
                    s.streaming_text = "a long streaming reply that should wrap across a narrow column width when the terminal is small".to_string();
                }
                if thinking {
                    s.streaming_thought = "pondering the right approach".to_string();
                }
                if approve {
                    s.pending_approval = Some(approval());
                }

                let overlay = s.build_overlay_lines(width);
                let total_rows = transient_total_rows(&s, &overlay, width);

                let mut full: Vec<Line<'static>> = s.cached_lines.clone();
                full.extend(overlay);
                let authoritative = Paragraph::new(full)
                    .wrap(Wrap { trim: false })
                    .line_count(width) as u16;

                assert_eq!(
                    total_rows, authoritative,
                    "cached_total_rows + overlay_rows must equal the full concatenation's line_count \
                     (width={width}, streaming={streaming}, thinking={thinking}, approve={approve})"
                );
            }
        }
    }

    #[test]
    fn visible_transient_slice_empty_history_overlay_only() {
        let mut s = state();
        s.pending_approval = Some(approval());
        let width = 80u16;
        s.rebuild_lines(width);
        assert_eq!(s.cached_total_rows, 0, "no entries means no history rows");

        let overlay = s.build_overlay_lines(width);
        let total_rows = transient_total_rows(&s, &overlay, width);
        let height = 5u16;

        let window = s.visible_cached_window(0, height);
        let (slice, local_scroll) = s.visible_transient_slice(0, height, &window, overlay.clone());
        assert_eq!(
            slice, overlay,
            "overlay-only history renders the overlay verbatim"
        );
        assert_eq!(local_scroll, 0);
        assert!(total_rows >= APPROVAL_OVERLAY_HEIGHT);
    }

    #[test]
    fn visible_transient_slice_tiny_history_large_overlay() {
        let mut s = state();
        s.entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("one line")));
        s.mark_dirty_full();
        let width = 80u16;
        s.rebuild_lines(width);
        s.streaming_text = "overlay-marker some streaming reply text".to_string();
        s.pending_approval = Some(approval());

        let overlay = s.build_overlay_lines(width);
        let total_rows = transient_total_rows(&s, &overlay, width);
        assert!(
            total_rows > s.cached_total_rows,
            "overlay must contribute rows beyond the tiny history"
        );

        // Window starting past the tiny history sits entirely in the overlay.
        let height = 6u16;
        let scroll = s.cached_total_rows;
        let window = s.visible_cached_window(scroll, height);
        let (slice, local_scroll) = s.visible_transient_slice(scroll, height, &window, overlay);
        assert_eq!(local_scroll, 0);
        let joined: String = slice
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(joined.contains("overlay-marker"));
    }

    #[derive(Clone, Copy, Debug)]
    enum CompleteFrameCase {
        Idle,
        Streaming,
        VisibleThinking,
        Approval,
        OverlayOnly,
    }

    fn complete_frame_work(
        history_entries: usize,
        case: CompleteFrameCase,
    ) -> ConversationRenderWork {
        use ratatui::{Terminal, backend::TestBackend};

        let mut s = state();
        for _ in 0..history_entries {
            s.entries.push(ChatEntry::AgentMessage(Arc::<str>::from(
                "history entry with stable width",
            )));
        }
        s.mark_dirty_full();
        s.pinned_to_bottom = true;
        match case {
            CompleteFrameCase::Idle => {}
            CompleteFrameCase::Streaming => {
                s.streaming_text = "streaming reply in progress".to_string();
            }
            CompleteFrameCase::VisibleThinking => {
                s.show_thoughts = true;
                s.streaming_thought = "considering the next step".to_string();
            }
            CompleteFrameCase::Approval => {
                s.pending_approval = Some(approval());
            }
            CompleteFrameCase::OverlayOnly => {
                s.streaming_text = "overlay row ".repeat(500);
            }
        }

        let area = Rect::new(0, 0, 80, 24);
        // The steady-state draw contract starts with the authoritative caches
        // already clean; rebuilding them after transcript mutation is
        // intentionally history-sized and is not per-frame work.
        s.rebuild_lines(area.width.saturating_sub(2));
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut work = None;
        terminal
            .draw(|frame| {
                work = Some(render_conversation(frame, &mut s, area));
            })
            .expect("render complete conversation frame");
        work.expect("render records complete-frame work")
    }

    #[test]
    fn complete_frame_history_work_is_bounded_for_all_transient_modes() {
        for case in [
            CompleteFrameCase::Idle,
            CompleteFrameCase::Streaming,
            CompleteFrameCase::VisibleThinking,
            CompleteFrameCase::Approval,
            CompleteFrameCase::OverlayOnly,
        ] {
            let small = complete_frame_work(64, case);
            let large = complete_frame_work(1_000, case);

            assert_eq!(
                large, small,
                "{case:?} must visit and materialize the same committed-history window with 64 and 1,000 entries"
            );
            assert_eq!(
                large.visible_cached_entries, large.entry_rect_candidates,
                "{case:?} entry rectangles must reuse the single resolved range"
            );
            assert!(
                large.visible_cached_entries <= 24,
                "{case:?} cached entry work must stay bounded by the viewport, got {large:?}"
            );
        }

        let overlay_only = complete_frame_work(1_000, CompleteFrameCase::OverlayOnly);
        assert_eq!(
            overlay_only,
            ConversationRenderWork {
                visible_cached_entries: 0,
                transcript_cached_lines: 0,
                copy_cached_blocks: 0,
                entry_rect_candidates: 0,
            },
            "an overlay-only viewport must not visit, clone, or wrap committed history"
        );
    }

    #[test]
    fn complete_frame_slices_one_large_committed_fence_by_visible_lines() {
        use std::fmt::Write as _;

        use ratatui::{Terminal, backend::TestBackend};

        let mut response = String::from("```rust\n");
        for line in 0..2_000 {
            writeln!(&mut response, "let line_{line} = {line};").expect("write fixture line");
        }
        response.push_str("```\n");

        let mut s = state();
        s.entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from(response)));
        s.mark_dirty_full();
        s.streaming_text = "streaming reply in progress".to_string();
        s.pinned_to_bottom = false;

        let area = Rect::new(0, 0, 80, 24);
        s.rebuild_lines(area.width.saturating_sub(2));
        s.scroll_offset = s.cached_total_rows / 2;
        let cached_copy_text = Arc::clone(&s.cached_code_blocks[0].text);

        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut work = None;
        terminal
            .draw(|frame| {
                work = Some(render_conversation(frame, &mut s, area));
            })
            .expect("render large committed entry");

        let work = work.expect("render work");
        assert_eq!(work.visible_cached_entries, 1);
        assert!(
            work.transcript_cached_lines <= usize::from(area.height.saturating_sub(2)),
            "one large entry must materialize only viewport lines: {work:?}"
        );
        assert_eq!(work.copy_cached_blocks, 1);
        assert_eq!(work.entry_rect_candidates, 1);

        let context = s
            .context_copy_regions
            .first()
            .expect("visible fence keeps its context-copy target");
        assert!(
            Arc::ptr_eq(&cached_copy_text, &context.text),
            "steady-state copy projection must share cached text"
        );
        assert!(context.text.contains("let line_0 = 0;"));
        assert!(context.text.contains("let line_1999 = 1999;"));
    }

    #[test]
    fn long_transcript_navigation_reaches_oldest_entry_with_a_bounded_render_window() {
        let mut s = state();
        s.entries.clear();
        for i in 0..2_200 {
            s.entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(format!(
                    "entry {i}"
                ))));
        }
        s.mark_dirty_full();
        s.rebuild_lines(80);

        assert_eq!(s.cached_render_start, 1_200);
        assert_eq!(s.cached_entry_count, MAX_RENDERED_ENTRIES);

        s.last_total_rows = s.cached_total_rows;
        s.last_inner_height = 20;
        s.scroll_offset = 0;
        s.pinned_to_bottom = false;
        let previous_top = s.cached_render_start;

        s.page_up();

        assert_eq!(s.cached_render_start, 700);
        assert_eq!(s.cached_entry_count, MAX_RENDERED_ENTRIES);
        let previous_top_row = s
            .cached_screen_ranges
            .iter()
            .find(|(idx, _, _, _)| *idx == previous_top)
            .map(|(_, lo, _, _)| *lo)
            .expect("the previous top entry remains in the shifted window");
        assert_eq!(
            previous_top_row.saturating_sub(s.scroll_offset),
            s.last_inner_height,
            "Page Up preserves the prior top anchor before moving it down one viewport"
        );

        s.scroll_to_top();
        s.rebuild_lines(80);

        assert_eq!(s.cached_render_start, 0);
        assert_eq!(s.cached_entry_count, MAX_RENDERED_ENTRIES);
        assert_eq!(
            s.cached_screen_ranges.first().map(|(idx, _, _, _)| *idx),
            Some(0),
            "jump-to-start exposes the oldest transcript entry"
        );

        let mut browse = state();
        browse.entries = s.entries.clone();
        browse.mark_dirty_full();
        browse.rebuild_lines(80);
        browse.last_total_rows = browse.cached_total_rows;
        browse.last_inner_height = 20;
        browse.enter_browse_mode();
        browse.rebuild_lines(80);

        browse.browse_move_up(1_200, true);
        browse.rebuild_lines(80);

        let cursor = browse.browse_cursor.expect("browse cursor");
        assert_eq!(cursor, 999);
        assert_eq!(browse.cached_entry_count, MAX_RENDERED_ENTRIES);
        assert!(
            browse
                .cached_screen_ranges
                .iter()
                .any(|(idx, _, _, _)| *idx == cursor),
            "an extended selection wider than the cache keeps its active cursor rendered"
        );
    }

    #[test]
    fn title_shows_agent_uid_provider_model() {
        let mut s = ChatState::new(
            "9caf2a14-0e6d-4127-b016-357c0b757b87".to_string(),
            "personal_code".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        s.set_model_identity(Some("anthropic.personal_code"), Some("claude-opus-4-8"));
        assert_eq!(
            s.title(),
            "personal_code  9caf2a1  anthropic.personal_code  claude-opus-4-8"
        );
    }

    #[test]
    fn title_falls_back_before_identity_resolved() {
        let s = ChatState::new(
            "abcdef1234".to_string(),
            "myagent".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        assert_eq!(s.title(), "myagent  abcdef1");
    }

    #[test]
    fn set_model_identity_keeps_full_ref_and_updates_live() {
        let mut s = ChatState::new(
            "abcdef1234".to_string(),
            "ag".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        s.set_model_identity(Some("openai.work"), Some("gpt-5"));
        assert_eq!(s.title(), "ag  abcdef1  openai.work  gpt-5");
        s.set_model_identity(None, Some("gpt-5-mini"));
        assert_eq!(s.title(), "ag  abcdef1  openai.work  gpt-5-mini");
        s.set_model_identity(Some("anthropic.personal_code"), Some("claude-opus-4-8"));
        assert_eq!(
            s.title(),
            "ag  abcdef1  anthropic.personal_code  claude-opus-4-8"
        );
    }

    #[test]
    fn title_hit_rects_target_provider_and_model_segments() {
        let mut s = ChatState::new(
            "abcdef1234".to_string(),
            "ag".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        s.set_model_identity(Some("openai.work"), Some("gpt-5"));
        let area = Rect::new(10, 4, 80, 20);

        s.refresh_title_hit_rects(area);

        assert_eq!(
            s.title_hit_target_at(25, 4),
            Some(TitleHitTarget::ModelProvider)
        );
        assert_eq!(s.title_hit_target_at(38, 4), Some(TitleHitTarget::Model));
        assert_eq!(s.title_hit_target_at(12, 4), Some(TitleHitTarget::Agent));
        assert_eq!(s.title_hit_target_at(25, 5), None);
    }

    #[test]
    fn title_hit_rects_target_agent_before_model_identity_resolves() {
        let mut s = ChatState::new(
            "abcdef1234".to_string(),
            "ag".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );

        s.refresh_title_hit_rects(Rect::new(10, 4, 80, 20));

        assert_eq!(s.title_hit_rects.len(), 1);
        assert_eq!(s.title_hit_target_at(12, 4), Some(TitleHitTarget::Agent));
        assert_eq!(s.title_hit_target_at(16, 4), None);
    }

    #[test]
    fn title_hit_rects_clip_at_pane_edge() {
        let mut s = ChatState::new(
            "abcdef1234".to_string(),
            "ag".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        s.set_model_identity(Some("openai.work"), Some("gpt-5"));

        s.refresh_title_hit_rects(Rect::new(10, 4, 25, 20));

        assert_eq!(
            s.title_hit_target_at(33, 4),
            Some(TitleHitTarget::ModelProvider)
        );
        assert_eq!(s.title_hit_target_at(35, 4), None);
    }

    #[test]
    fn model_provider_picker_overlay_rows_are_hit_testable() {
        let mut s = state();
        s.model_picker =
            ModelPickerOverlay::ConfiguredProviderStage(crate::widgets::PickerState::new(
                vec!["openai.default".into(), "deepseek.default".into()],
                None,
            ));

        let area = Rect::new(0, 0, 80, 20);
        let modal = model_picker_overlay_area(&s.model_picker, area).unwrap();

        assert_eq!(
            mouse::list_click_index(modal.y + 1, modal, 0, s.model_picker.item_count()),
            Some(0)
        );
        assert_eq!(
            mouse::list_click_index(modal.y + 2, modal, 0, s.model_picker.item_count()),
            Some(1)
        );
        assert_eq!(
            mouse::list_click_index(modal.y, modal, 0, s.model_picker.item_count()),
            None
        );
    }

    #[test]
    fn model_picker_overlay_default_is_closed() {
        let s = state();
        assert!(!s.model_picker.is_open());
    }

    #[tokio::test]
    async fn model_picker_catalog_preserves_provider_alias_and_isolates_cache() {
        let (tx, mut requests) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(rpc.clone()));
        let mut chat = Chat::new(client.clone(), PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(state()));
        let (results_tx, mut results_rx) = mpsc::channel(1);

        for provider in ["custom.first", "custom.second", "anthropic.work"] {
            let model = format!("{provider}-model");
            let active = active_state(&mut chat);
            active.model_provider_ref = Some(provider.to_string());
            active.model = Some(model.clone());
            Chat::open_model_picker(&client, &results_tx, active).await;
            assert!(matches!(active.model_picker, ModelPickerOverlay::Loading));

            let request = next_rpc_request(&mut requests, "catalog request expected").await;
            assert_eq!(request["method"], "config/catalog-models");
            assert_eq!(request["params"]["model_provider"], provider);
            respond_ok(
                &rpc,
                &request,
                serde_json::json!({ "models": [model.clone()] }),
            );
            let result = tokio::time::timeout(Duration::from_secs(2), results_rx.recv())
                .await
                .expect("catalog response should complete")
                .expect("catalog result channel should remain open");
            chat.apply_model_fetch(result);

            let active = active_state(&mut chat);
            assert_eq!(active.input_bar.model_catalog_provider(), Some(provider));
            assert_eq!(active.input_bar.model_catalog(), &[model]);
            assert!(matches!(active.model_picker, ModelPickerOverlay::Model(_)));
            active.model_picker = ModelPickerOverlay::None;
            Chat::open_model_picker(&client, &results_tx, active).await;
            assert!(matches!(active.model_picker, ModelPickerOverlay::Model(_)));
            assert!(
                requests.try_recv().is_err(),
                "same alias should reuse its catalog"
            );
            active.model_picker = ModelPickerOverlay::None;
        }
    }

    #[test]
    fn model_picker_overlay_open_states_report_open() {
        let model =
            ModelPickerOverlay::Model(crate::widgets::PickerState::new(vec!["a".into()], None));
        assert!(model.is_open());
        let stage1 = ModelPickerOverlay::ConfiguredProviderStage(crate::widgets::PickerState::new(
            vec!["anthropic.personal_code".into()],
            None,
        ));
        assert!(stage1.is_open());
    }

    // ── Session-transition settings reload ──────────────────────────────────
    //
    // `zerocode-config.toml` is the single source of truth for TodoWrite
    // display. A fresh session obviously reloads it, but restart and
    // saved-session switch reuse the existing `ChatState`, so they must
    // re-resolve the file too — otherwise a Config-pane edit silently fails to
    // apply until zerocode is restarted. These drive the real transition
    // helpers (`restart_session_for_state` / `switch_to_session_entry`), not a
    // direct `reset_for_session` call.

    struct ConfigDirGuard(Option<String>);
    impl ConfigDirGuard {
        fn set(dir: &std::path::Path) -> Self {
            let prev = std::env::var("CLAWCREW_CONFIG_DIR").ok();
            // SAFETY: these tests serialize on `config_dir_test_lock()`.
            unsafe { std::env::set_var("CLAWCREW_CONFIG_DIR", dir) };
            Self(prev)
        }
    }
    impl Drop for ConfigDirGuard {
        fn drop(&mut self) {
            // SAFETY: these tests serialize on `config_dir_test_lock()`.
            match &self.0 {
                Some(v) => unsafe { std::env::set_var("CLAWCREW_CONFIG_DIR", v) },
                None => unsafe { std::env::remove_var("CLAWCREW_CONFIG_DIR") },
            }
        }
    }

    /// Write a `[todotracker]` section with a distinctive width/location.
    fn write_tracker_config(dir: &std::path::Path, width: u16, enabled_at_start: bool) {
        crate::config::persist_todotracker(
            dir,
            &crate::config::TodoTrackerSection {
                enabled: true,
                enabled_at_start,
                location: crate::config::TodoTrackerLocation::Bottom,
                width,
                max_height: 7,
            },
        )
        .expect("test config write should succeed");
    }

    #[tokio::test]
    async fn restart_reloads_local_todo_settings() {
        let _lock = env_test_lock_async().await;
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());

        // Session starts with the tracker configured one way...
        write_tracker_config(dir.path(), 30, false);
        let mut state = ChatState::new(
            "sess-1".to_string(),
            "myagent".to_string(),
            crate::config::ensure_and_load(dir.path())
                .unwrap()
                .resolve_todo_tracker(),
        );
        assert_eq!(state.todo_tracker.width(), 30);

        // ...then the user edits the Config pane mid-session.
        write_tracker_config(dir.path(), 44, true);

        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc_out = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc_out)));

        let restart = {
            let client = Arc::clone(&client);
            tokio::spawn(async move {
                Chat::restart_session_for_state(&client, PaneKind::Chat, &mut state).await;
                state
            })
        };

        // The restart opens the replacement session first, then closes the old
        // one; `refresh_model_identity` follows. Answer each in arrival order.
        let new = next_rpc_request(&mut rx, "restart should open a new session").await;
        assert_eq!(new["method"], method::SESSION_NEW);
        respond_ok(
            &rpc_out,
            &new,
            serde_json::json!({ "session_id": "sess-2", "workspace_dir": null }),
        );
        drain_pending_requests(&mut rx, &rpc_out).await;

        let state = tokio::time::timeout(Duration::from_secs(2), restart)
            .await
            .expect("restart should finish")
            .unwrap();

        assert_eq!(
            state.todo_tracker.width(),
            44,
            "restart must re-resolve zerocode-config.toml, not reuse the old layout"
        );
        assert!(
            state.todo_tracker.is_visible(),
            "restart must honor the newly saved enabled_at_start"
        );
    }

    #[tokio::test]
    async fn session_switch_reloads_local_todo_settings() {
        let _lock = env_test_lock_async().await;
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());

        write_tracker_config(dir.path(), 30, false);
        let state = ChatState::new(
            "sess-1".to_string(),
            "myagent".to_string(),
            crate::config::ensure_and_load(dir.path())
                .unwrap()
                .resolve_todo_tracker(),
        );
        assert_eq!(state.todo_tracker.width(), 30);

        write_tracker_config(dir.path(), 44, true);

        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc_out = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc_out)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.session_order.push(state.session_id.clone());
        chat.phase = ChatPhase::Active(Box::new(state));

        let entry = crate::client::SessionEntry {
            session_id: "sess-9".to_string(),
            session_key: "sess-9".to_string(),
            created_at: "2026-07-07T00:00:00Z".to_string(),
            last_activity: "2026-07-07T00:05:00Z".to_string(),
            message_count: 0,
            agent_alias: Some("myagent".to_string()),
            channel_id: None,
            name: Some("Other work".to_string()),
        };

        let switch = tokio::spawn(async move {
            chat.switch_to_session_entry(entry).await;
            chat
        });

        let rehydrate = next_rpc_request(&mut rx, "switch should rehydrate the target").await;
        assert_eq!(rehydrate["method"], method::SESSION_NEW);
        respond_ok(
            &rpc_out,
            &rehydrate,
            serde_json::json!({ "session_id": "sess-9", "workspace_dir": null }),
        );
        drain_pending_requests(&mut rx, &rpc_out).await;

        let chat = tokio::time::timeout(Duration::from_secs(2), switch)
            .await
            .expect("switch should finish")
            .unwrap();
        let ChatPhase::Active(state) = chat.phase else {
            panic!("switched session should be active");
        };

        assert_eq!(
            state.todo_tracker.width(),
            44,
            "session switch must re-resolve zerocode-config.toml"
        );
        assert!(
            state.todo_tracker.is_visible(),
            "session switch must honor the newly saved enabled_at_start"
        );
    }

    // A transition-time config load failure (here: an unknown, hard-erroring
    // `ZEROCODE_todotracker__*` override) must NOT silently reset the tracker
    // to built-in defaults. `resolve_todo_settings` returns the supplied
    // fallback so a restart/switch keeps the user's current layout.
    #[tokio::test]
    async fn resolve_todo_settings_preserves_fallback_on_load_error() {
        let _lock = env_test_lock_async().await;
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());

        // The in-use settings differ from the built-in defaults.
        let current = crate::todo_tracker::TodoTrackerSettings {
            enabled: true,
            enabled_at_start: true,
            location: crate::todo_tracker::TodoLocation::Bottom,
            width: 44,
            max_height: 7,
        };
        assert_ne!(
            current,
            crate::todo_tracker::TodoTrackerSettings::default(),
            "fallback must be distinguishable from defaults for this test to mean anything"
        );

        // An unknown override makes `ensure_and_load` hard-error.
        let _v = crate::test_support::EnvVarGuard::set("ZEROCODE_todotracker__nope", "1");
        assert!(
            crate::config::ensure_and_load(dir.path()).is_err(),
            "precondition: the bogus override should make resolution fail"
        );

        let resolved = Chat::resolve_todo_settings(current);
        assert_eq!(
            resolved, current,
            "a load error must keep the current settings, not reset to defaults"
        );
    }

    // A malformed on-disk `[todotracker]` section is tolerated by
    // `load_persisted` (defaults substituted), which previously let a botched
    // manual edit silently reset the live tracker on restart/switch. The
    // checked transition resolver must instead treat it as an error and keep
    // the current settings.
    #[tokio::test]
    async fn resolve_todo_settings_preserves_fallback_on_malformed_disk_section() {
        let _lock = env_test_lock_async().await;
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());

        // A hand-edited, malformed section on disk (width is not a number).
        std::fs::write(
            crate::config::config_path(dir.path()),
            "[todotracker]\nwidth = \"oops\"\n",
        )
        .unwrap();

        let current = crate::todo_tracker::TodoTrackerSettings {
            enabled: true,
            enabled_at_start: true,
            location: crate::todo_tracker::TodoLocation::Bottom,
            width: 44,
            max_height: 7,
        };

        let resolved = Chat::resolve_todo_settings(current);
        assert_eq!(
            resolved, current,
            "a malformed on-disk section must keep current settings, not reset to defaults"
        );
    }

    // An explicit zero dimension must fail visibly at the session boundary
    // rather than normalizing to 1. At *this* layer, "fail visibly" means the
    // transition keeps the user's current settings and logs the error instead
    // of silently rendering a collapsed 1-cell tracker.
    #[tokio::test]
    async fn resolve_todo_settings_preserves_fallback_on_zero_width_from_file() {
        let _lock = env_test_lock_async().await;
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());

        std::fs::write(
            crate::config::config_path(dir.path()),
            "[todotracker]\nwidth = 0\nmax_height = 5\n",
        )
        .unwrap();

        let current = crate::todo_tracker::TodoTrackerSettings {
            enabled: true,
            enabled_at_start: true,
            location: crate::todo_tracker::TodoLocation::Bottom,
            width: 44,
            max_height: 7,
        };

        let resolved = Chat::resolve_todo_settings(current);
        assert_eq!(
            resolved, current,
            "an explicit width = 0 must keep current settings, never resolve to a 1-cell tracker"
        );
        assert_ne!(resolved.width, 1, "the zero must not be normalized to 1");
    }

    // Same contract via the canonical environment surface.
    #[tokio::test]
    async fn resolve_todo_settings_preserves_fallback_on_zero_width_from_env() {
        let _lock = env_test_lock_async().await;
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());

        let _v = crate::test_support::EnvVarGuard::set("ZEROCODE_todotracker__width", "0");

        let current = crate::todo_tracker::TodoTrackerSettings {
            enabled: true,
            enabled_at_start: true,
            location: crate::todo_tracker::TodoLocation::Bottom,
            width: 44,
            max_height: 7,
        };

        let resolved = Chat::resolve_todo_settings(current);
        assert_eq!(
            resolved, current,
            "ZEROCODE_todotracker__width=0 must keep current settings, not normalize to 1"
        );
        assert_ne!(resolved.width, 1, "the zero must not be normalized to 1");
    }

    #[tokio::test]
    async fn open_picker_makes_chat_claim_text_input() {
        // While the picker is open the pane is modal (claims text-input so
        // global keys are suppressed and routed to the picker handler).
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(state()));
        if let ChatPhase::Active(s) = &mut chat.phase {
            s.model_picker = ModelPickerOverlay::Model(crate::widgets::PickerState::new(
                vec!["a".into(), "b".into()],
                None,
            ));
        }
        assert!(chat.wants_text_input());
    }

    #[tokio::test]
    async fn attachment_manager_makes_chat_claim_text_input() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active.input_bar.add_attachment(PendingAttachment {
            path: std::path::PathBuf::from("one.png"),
            mime_type: "image/png".into(),
            filename: "one.png".into(),
            size_bytes: 1,
            source: crate::attachment::AttachmentSource::File,
        });
        active.input_bar.insert_text("/attachments");
        assert!(matches!(
            active
                .input_bar
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            crate::input_bar::InputBarAction::Consumed
        ));
        chat.phase = ChatPhase::Active(Box::new(active));

        assert!(chat.wants_text_input());
    }

    #[tokio::test]
    async fn pending_elicitation_makes_chat_claim_text_input() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(state()));
        // Not modal before the prompt arrives (empty input → command mode).
        assert!(!chat.wants_text_input());
        if let ChatPhase::Active(s) = &mut chat.phase {
            s.set_pending_elicitation(single_elicitation());
        }
        assert!(
            chat.wants_text_input(),
            "an active pending elicitation must claim modal focus"
        );
    }

    #[tokio::test]
    async fn wants_quit_chord_tracks_in_flight_turn_state() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(state()));

        assert!(
            !chat.wants_quit_chord(),
            "idle pane must leave Ctrl+C to the quit modal"
        );

        if let ChatPhase::Active(s) = &mut chat.phase {
            s.turn_in_flight = true;
        }
        assert!(
            chat.wants_quit_chord(),
            "an in-flight turn must consume Ctrl+C to cancel before quit"
        );

        if let ChatPhase::Active(s) = &mut chat.phase {
            s.enter_cancelling();
        }
        assert!(
            !chat.wants_quit_chord(),
            "an already-cancelling turn must not re-consume Ctrl+C"
        );
    }

    #[tokio::test]
    async fn current_session_id_reports_active_session() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        // No session yet → None.
        assert_eq!(chat.current_session_id(), None);
        chat.phase = ChatPhase::Active(Box::new(state()));
        // Active → the live session id (the `state()` helper's id).
        assert!(chat.current_session_id().is_some());
    }

    #[tokio::test]
    async fn unmatched_resume_is_retained_for_reconnect_retry() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        chat.set_resume_sessions(vec![resume_entry("sess-prev", "ghost", true)]);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("init should request the agent list")
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        let id = request["id"].as_str().unwrap().to_string();
        // Two enabled agents → multi-agent picker, no auto-start.
        rpc.dispatch_response(
            &id,
            Some(serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0},
                    {"alias": "beta", "enabled": true, "live_sessions": 0, "persisted_sessions": 0}
                ]
            })),
            None,
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("init should finish")
            .unwrap();
        assert!(
            rx.try_recv().is_err(),
            "an unmatched resume must not be replaced by a fresh session"
        );
        assert_eq!(
            chat.resume_focused
                .as_ref()
                .map(|entry| entry.session_id.as_str()),
            Some("sess-prev"),
            "failed resume entries stay available for a later reconnect retry"
        );
        assert!(matches!(chat.phase, ChatPhase::Error(_)));
    }

    #[tokio::test]
    async fn failed_focused_resume_is_retained_for_reconnect_retry() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.set_resume_sessions(vec![resume_entry("sess-prev", "alpha", true)]);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });
        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 1, "persisted_sessions": 1}
                ]
            }),
        );

        let request = next_rpc_request(&mut rx, "resume should reattach the retained id").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["session_id"], "sess-prev");
        respond_err(&rpc, &request, -32000, "controlled reattach failure");

        let chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("failed resume should return")
            .unwrap();
        assert_eq!(
            chat.resume_focused
                .as_ref()
                .map(|entry| entry.session_id.as_str()),
            Some("sess-prev")
        );
        assert!(matches!(chat.phase, ChatPhase::Error(_)));
    }

    #[tokio::test]
    async fn transcript_failures_retain_resumes_and_promote_next_healthy_background() {
        let (tx, mut rx) = mpsc::channel::<String>(32);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut focused = resume_entry("sess-focused", "alpha", true);
        focused.queue.messages.push_back(QueuedMessage {
            id: 0,
            text: "focused queue".to_string(),
            attachments: Vec::new(),
            status: QueueItemStatus::Pending,
        });
        focused.queue.next_id = 1;
        focused.queue.paused = true;
        let mut bad_background = resume_entry("sess-bad", "beta", false);
        bad_background.queue.messages.push_back(QueuedMessage {
            id: 0,
            text: "bad background queue".to_string(),
            attachments: Vec::new(),
            status: QueueItemStatus::Pending,
        });
        bad_background.queue.next_id = 1;
        bad_background.queue.paused = true;
        let mut healthy_background = resume_entry("sess-good", "gamma", false);
        healthy_background.queue.messages.push_back(QueuedMessage {
            id: 0,
            text: "healthy background queue".to_string(),
            attachments: Vec::new(),
            status: QueueItemStatus::Pending,
        });
        healthy_background.queue.next_id = 1;
        healthy_background.queue.paused = true;
        chat.set_resume_sessions(vec![focused, bad_background, healthy_background]);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });
        let request = next_rpc_request(&mut rx, "init requests agents").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 1, "persisted_sessions": 1},
                    {"alias": "beta", "enabled": true, "live_sessions": 1, "persisted_sessions": 1},
                    {"alias": "gamma", "enabled": true, "live_sessions": 1, "persisted_sessions": 1}
                ]
            }),
        );

        for (session_id, transcript_ok) in [
            ("sess-focused", false),
            ("sess-bad", false),
            ("sess-good", true),
        ] {
            let request = next_rpc_request(&mut rx, "resume reattaches retained session").await;
            assert_eq!(request["method"], method::SESSION_NEW);
            assert_eq!(request["params"]["session_id"], session_id);
            respond_ok(
                &rpc,
                &request,
                serde_json::json!({ "session_id": session_id, "workspace_dir": "/w" }),
            );
            let request = next_rpc_request(&mut rx, "resume resolves model identity").await;
            assert_eq!(request["method"], method::CONFIG_LIST);
            respond_ok(&rpc, &request, serde_json::json!([]));
            let request = next_rpc_request(&mut rx, "resume reloads durable transcript").await;
            assert_eq!(request["method"], method::SESSION_MESSAGES);
            if transcript_ok {
                respond_ok(
                    &rpc,
                    &request,
                    serde_json::json!({
                        "messages": [{"role": "assistant", "content": "healthy transcript"}]
                    }),
                );
            } else {
                respond_err(
                    &rpc,
                    &request,
                    crate::jsonrpc::error_codes::INTERNAL_ERROR,
                    "controlled transcript failure",
                );
            }
        }

        let chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("a healthy background should keep the pane usable")
            .unwrap();
        assert_eq!(chat.current_session_id(), Some("sess-good"));
        let ChatPhase::Active(active) = &chat.phase else {
            panic!("healthy background must be promoted into focus");
        };
        assert_eq!(active.queue_len(), 1);
        assert!(active.queue_paused());
        assert!(active.entries.iter().any(|entry| matches!(
            entry,
            ChatEntry::AgentMessage(message) if message.as_ref() == "healthy transcript"
        )));
        assert_eq!(
            chat.resume_focused
                .as_ref()
                .map(|entry| (entry.session_id.as_str(), entry.queue.messages.len())),
            Some(("sess-focused", 1))
        );
        assert_eq!(
            chat.resume_backgrounds
                .iter()
                .map(|entry| (entry.session_id.as_str(), entry.queue.messages.len()))
                .collect::<Vec<_>>(),
            vec![("sess-bad", 1)]
        );
        assert_eq!(
            chat.resume_entries()
                .iter()
                .map(|entry| (
                    entry.session_id.as_str(),
                    entry.was_focused,
                    entry.queue.messages.len()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("sess-good", true, 1),
                ("sess-focused", false, 1),
                ("sess-bad", false, 1),
            ],
            "another transport reconnect must retain every failed entry and queue"
        );
    }

    #[tokio::test]
    async fn multi_agent_reconnect_reattaches_prior_agent_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.set_resume_sessions(vec![resume_entry("sess-prev", "beta", true)]);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        // First request: the agent list.
        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("init should request the agent list")
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        let id = request["id"].as_str().unwrap().to_string();
        rpc.dispatch_response(
            &id,
            Some(serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0},
                    {"alias": "beta", "enabled": true, "live_sessions": 1, "persisted_sessions": 0}
                ]
            })),
            None,
        );

        // Second request must be session_new_with_id carrying the prior id for
        // the prior agent — NOT a fresh pick / fresh session. This is the whole
        // fix: a multi-agent reconnect reattaches instead of minting fresh.
        //
        // No config/list fetch precedes it: TodoWrite tracker settings are
        // ZeroCode-local (`zerocode-config.toml`), resolved without a daemon
        // round-trip, so session start goes straight to `session/new`.
        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("reconnect should reattach the prior session")
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "session/new");
        let params = &request["params"];
        assert_eq!(params["agent_alias"], "beta");
        assert_eq!(params["session_id"], "sess-prev");
        assert_eq!(
            params["keep_siblings"], true,
            "zerocode tracks its sessions and must opt out of sibling eviction"
        );

        init.abort();
    }

    #[tokio::test]
    async fn reconnect_transfer_preserves_queue_and_invalidates_live_interactions() {
        let (tx, mut writer_rx) = mpsc::channel::<String>(16);
        let outbound = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(outbound.clone()));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut prior = state_for("sess-r", "alpha");
        let attachment_dir = tempfile::tempdir().expect("create attachment fixture directory");
        let file_path = attachment_dir.path().join("keep.txt");
        let clipboard_path = attachment_dir.path().join("drop.png");
        std::fs::write(&file_path, b"keep").expect("write user attachment");
        std::fs::write(&clipboard_path, b"drop").expect("write clipboard attachment");
        prior.input_bar.load_for_edit(
            "draft survives reconnect".to_string(),
            vec![
                PendingAttachment {
                    path: file_path.clone(),
                    mime_type: "text/plain".to_string(),
                    filename: "keep.txt".to_string(),
                    size_bytes: 4,
                    source: crate::attachment::AttachmentSource::File,
                },
                clipboard_att(&clipboard_path, "drop.png"),
            ],
        );
        prior
            .enqueue_message("keep queued".to_string(), Vec::new())
            .expect("queue message");
        prior.queue_paused = true;
        prior.turn_in_flight = true;
        prior.turn_status = TurnStatus::WaitingForApproval;
        prior.pending_approval = Some(PendingApproval {
            request_id: "approval-r".to_string(),
            tool_name: "shell".to_string(),
            arguments_summary: "pwd".to_string(),
            timeout_secs: 30,
        });
        prior.pending_elicitation = Some(PendingElicitation {
            request_id: serde_json::json!("elicitation-r"),
            session_id: "sess-r".to_string(),
            message: "Pick one".to_string(),
            choices: vec!["Yes".to_string()],
            multi: false,
            min_items: 1,
            max_items: 1,
            cursor: 0,
            selected: Vec::new(),
        });
        chat.phase = ChatPhase::Active(Box::new(prior));
        chat.session_order = vec!["sess-r".to_string()];

        let mut entries = chat.resume_entries();
        assert_eq!(entries.len(), 1);
        let entry = entries.remove(0);
        assert!(entry.interrupted);
        assert_eq!(entry.queue.messages.len(), 1);
        assert!(entry.queue.paused);
        assert_eq!(entry.queue.composer_text, "draft survives reconnect");
        assert_eq!(entry.queue.composer_attachments.len(), 1);
        assert_eq!(entry.queue.composer_attachments[0].filename, "keep.txt");
        assert_eq!(
            entry.queue.composer_attachments[0].source,
            crate::attachment::AttachmentSource::File
        );
        assert!(
            clipboard_path.exists(),
            "snapshotting must not clean temporary attachments before commit"
        );
        let ChatPhase::Active(prior) = &chat.phase else {
            panic!("old pane remains active until replacement construction succeeds");
        };
        assert!(prior.pending_approval.is_some());
        assert!(prior.pending_elicitation.is_some());
        assert!(
            writer_rx.try_recv().is_err(),
            "snapshotting reconnect state must not mutate or answer old interactions"
        );

        chat.commit_reconnect_handoff();
        assert!(
            !clipboard_path.exists(),
            "clipboard attachment must be cleaned at commit"
        );
        assert!(
            file_path.exists(),
            "user-selected file must remain untouched"
        );

        let mut rebuilt = state_for("sess-r", "alpha");
        rebuilt.load_history(
            vec![crate::client::MessageEntry {
                role: "assistant".to_string(),
                content: "durable answer".to_string(),
                ..Default::default()
            }],
            false,
        );
        rebuilt.restore_reconnect_state(entry.queue, entry.interrupted, entry.recovery_required);
        assert_eq!(rebuilt.queue_len(), 1);
        assert!(rebuilt.queue_paused());
        assert_eq!(rebuilt.input_bar.input(), "draft survives reconnect");
        assert_eq!(rebuilt.input_bar.pending_attachments().len(), 1);
        assert_eq!(
            rebuilt.input_bar.pending_attachments()[0].filename,
            "keep.txt"
        );
        assert!(!rebuilt.turn_in_flight);
        assert!(rebuilt.pending_approval.is_none());
        assert!(rebuilt.pending_elicitation.is_none());
        assert!(matches!(
            rebuilt.entries.last(),
            Some(ChatEntry::SystemMessage(message))
                if message.as_ref() == crate::i18n::t("zc-chat-reconnect-interrupted")
        ));

        // The old transport gets one best-effort denial and one elicitation
        // cancellation; the rebuilt pane never carries either modal forward.
        let mut methods = Vec::new();
        for _ in 0..2 {
            let line = tokio::time::timeout(Duration::from_secs(1), writer_rx.recv())
                .await
                .expect("interaction invalidation should write to old transport")
                .expect("writer remains open");
            let frame: serde_json::Value = serde_json::from_str(&line).expect("valid JSON-RPC");
            methods.push(
                frame
                    .get("method")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            );
            if frame["method"] == method::SESSION_APPROVE {
                respond_ok(&outbound, &frame, serde_json::json!({ "approved": false }));
            } else {
                assert_eq!(frame["id"], "elicitation-r");
                assert_eq!(frame["result"]["action"], "cancel");
            }
        }
        assert!(methods.contains(&Some(method::SESSION_APPROVE.to_string())));
        assert!(methods.contains(&None));
    }

    // ── Multi-session (agent sidebar) ────────────────────────────

    fn state_for(sid: &str, alias: &str) -> ChatState {
        ChatState::new(
            sid.to_string(),
            alias.to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        )
    }

    fn turn_complete(sid: &str, outcome: TurnEndOutcome, content: &str) -> SessionUpdate {
        SessionUpdate::TurnComplete {
            session_id: sid.to_string(),
            outcome,
            content: content.to_string(),
            client_turn_generation: None,
            message_count: None,
        }
    }

    /// Two-session Chat: `sess-a` focused, `sess-b` in background.
    fn two_session_chat(rpc: &Arc<RpcOutbound>) -> Chat {
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(state_for("sess-a", "alpha")));
        chat.background.push(state_for("sess-b", "beta"));
        chat.session_order = vec!["sess-a".to_string(), "sess-b".to_string()];
        chat
    }

    #[test]
    fn sidebar_status_maps_session_state_to_traffic_lights() {
        let mut s = state();
        assert_eq!(s.sidebar_status(), SidebarStatus::Ready);

        s.turn_in_flight = true;
        assert_eq!(s.sidebar_status(), SidebarStatus::Running);
        s.enter_cancelling();
        assert_eq!(
            s.sidebar_status(),
            SidebarStatus::Running,
            "a cancelling turn is still winding down"
        );

        s.pending_approval = Some(PendingApproval {
            request_id: "r".into(),
            tool_name: "shell".into(),
            arguments_summary: "ls".into(),
            timeout_secs: 30,
        });
        assert_eq!(
            s.sidebar_status(),
            SidebarStatus::NeedsHuman,
            "needs-human outranks running"
        );
        s.pending_approval = None;

        let elicitation = |sid: &str| PendingElicitation {
            request_id: serde_json::json!("id"),
            session_id: sid.to_string(),
            message: "pick".into(),
            choices: vec!["a".into()],
            multi: false,
            min_items: 1,
            max_items: 1,
            cursor: 0,
            selected: Vec::new(),
        };
        s.pending_elicitation = Some(elicitation("other-session"));
        assert_eq!(
            s.sidebar_status(),
            SidebarStatus::Running,
            "a stale modal for another session must not read as needs-human"
        );
        s.pending_elicitation = Some(elicitation(&s.session_id.clone()));
        assert_eq!(s.sidebar_status(), SidebarStatus::NeedsHuman);

        s.last_error = Some(SessionError::TurnFailed);
        assert_eq!(
            s.sidebar_status(),
            SidebarStatus::Errored,
            "error outranks everything"
        );
    }

    #[test]
    fn last_error_follows_turn_completion_outcomes() {
        let mut s = state();
        s.turn_in_flight = true;
        s.apply_update(turn_complete("sess-1", TurnEndOutcome::Failed, "boom"));
        assert_eq!(s.last_error, Some(SessionError::TurnFailed));

        // A new prompt supersedes the failure.
        s.push_user_message(Some("again".into()), Vec::new());
        assert_eq!(s.last_error, None);

        s.apply_update(turn_complete(
            "sess-1",
            TurnEndOutcome::Failed,
            "turn cancelled by daemon: session_not_found",
        ));
        assert_eq!(
            s.last_error,
            Some(SessionError::SessionLost),
            "the daemon's session-loss sentinel maps to SessionLost"
        );

        s.push_user_message(Some("retry".into()), Vec::new());
        s.apply_update(turn_complete("sess-1", TurnEndOutcome::Cancelled, "stop"));
        assert_eq!(s.last_error, None, "a cancel is not an error");

        s.last_error = Some(SessionError::TurnFailed);
        s.apply_update(turn_complete("sess-1", TurnEndOutcome::Completed, "done"));
        assert_eq!(s.last_error, None, "a completed turn clears the red dot");

        s.last_error = Some(SessionError::TurnFailed);
        let todo_settings = s.todo_tracker.settings();
        s.reset_for_session("sess-2".to_string(), None, todo_settings);
        assert_eq!(s.last_error, None);
    }

    #[tokio::test]
    async fn notifications_route_to_background_sessions() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);
        if let Some(b) = chat.background.first_mut() {
            b.turn_in_flight = true;
        }

        chat.rpc.push_notification_for_test(
            "session/update",
            serde_json::json!({
                "type": "agent_message_chunk",
                "session_id": "sess-b",
                "text": "hi",
            }),
        );
        chat.rpc.push_notification_for_test(
            "session/update",
            serde_json::json!({
                "type": "approval_request",
                "session_id": "sess-b",
                "request_id": "req-9",
                "tool_name": "shell",
                "arguments_summary": "rm -rf",
                "timeout_secs": 30,
            }),
        );
        chat.drain_notifications();

        let b = chat.background.first().expect("background session");
        assert_eq!(b.turn_status, TurnStatus::WaitingForApproval);
        assert_eq!(b.sidebar_status(), SidebarStatus::NeedsHuman);
        let ChatPhase::Active(a) = &chat.phase else {
            panic!("focused session must stay active");
        };
        assert_eq!(a.turn_status, TurnStatus::Idle, "focused session untouched");
        assert!(a.pending_approval.is_none());
    }

    #[tokio::test]
    async fn resume_snapshot_marks_inflight_resync_as_recovery_required() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(rpc));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(state()));
        chat.session_order.push("sess-1".to_string());
        chat.session_resync_in_flight.insert("sess-1".to_string());

        let entries = chat.resume_entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].recovery_required);
    }

    #[tokio::test]
    async fn prompt_transport_closure_preserves_interrupted_resume_recovery() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let outbound = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(outbound.clone()));
        let mut chat = Chat::new(client.clone(), PaneKind::Chat);
        let mut active = state();
        active
            .enqueue_message("in flight".to_string(), Vec::new())
            .expect("first prompt");
        active
            .enqueue_message("queued after disconnect".to_string(), Vec::new())
            .expect("queued follow-up");
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.session_order.push("sess-1".to_string());

        chat.pump_all_queues();
        let prompt = next_rpc_request(&mut rx, "first prompt should be sent").await;
        assert_eq!(prompt["method"], method::SESSION_PROMPT);
        client.disconnect_for_test("prompt transport closed");

        tokio::time::timeout(Duration::from_secs(1), async {
            while chat.prompt_completion_rx.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("transport closure should complete the pending request");
        chat.tick_transport_events();

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("session remains active after transport closure");
        };
        assert!(
            state.turn_in_flight,
            "closure must retain the interrupted turn"
        );
        assert_eq!(state.queue_len(), 1, "queued work remains client-owned");
        assert!(
            rx.try_recv().is_err(),
            "transport closure must not dispatch the queued follow-up"
        );

        let entry = chat
            .resume_entries()
            .into_iter()
            .next()
            .expect("interrupted session should be resumable");
        assert!(entry.interrupted);

        let (reconnect_tx, mut reconnect_rx) = mpsc::channel::<String>(16);
        let reconnect_rpc = Arc::new(RpcOutbound::new(reconnect_tx));
        let reconnect_client = Arc::new(RpcClient::with_rpc(reconnect_rpc.clone()));
        let mut reconnected = Chat::new(reconnect_client, PaneKind::Chat);
        reconnected.set_resume_sessions(vec![entry]);
        let reconnect = tokio::spawn(async move {
            reconnected.start_session("myagent", None).await;
            reconnected
        });

        let session_new = next_rpc_request(&mut reconnect_rx, "reconnect should reattach").await;
        respond_ok(
            &reconnect_rpc,
            &session_new,
            serde_json::json!({ "session_id": "sess-1", "workspace_dir": "/tmp/reconnected" }),
        );
        let config = next_rpc_request(&mut reconnect_rx, "reconnect refreshes identity").await;
        respond_ok(&reconnect_rpc, &config, serde_json::json!([]));
        let history = next_rpc_request(&mut reconnect_rx, "reconnect reloads history").await;
        respond_ok(
            &reconnect_rpc,
            &history,
            serde_json::json!({ "messages": [] }),
        );
        let recovery = next_rpc_request(
            &mut reconnect_rx,
            "interrupted resume must recover before dispatch",
        )
        .await;
        assert_eq!(recovery["method"], method::SESSION_CANCEL);
        reconnect.abort();
    }

    #[tokio::test]
    async fn notification_lag_recovers_dropped_turn_complete_before_queue_dispatch() {
        let (tx, mut writer_rx) = mpsc::channel::<String>(16);
        let outbound = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(outbound.clone()));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active.push_user_message(Some("in flight".to_string()), Vec::new());
        active
            .enqueue_message("send after recovery".to_string(), Vec::new())
            .expect("queue follow-up");
        active.pending_approval = Some(PendingApproval {
            request_id: "approval-lag".to_string(),
            tool_name: "shell".to_string(),
            arguments_summary: "pwd".to_string(),
            timeout_secs: 30,
        });
        active.pending_elicitation = Some(PendingElicitation {
            request_id: serde_json::json!("elicitation-lag"),
            session_id: "sess-1".to_string(),
            message: "Pick one".to_string(),
            choices: vec!["Yes".to_string()],
            multi: false,
            min_items: 1,
            max_items: 1,
            cursor: 0,
            selected: Vec::new(),
        });
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.session_order = vec!["sess-1".to_string()];

        // The test client's notification channel holds 64 frames. Put the
        // terminal frame first, then overflow it so `try_recv` reports Lagged
        // and the old implementation would have stranded `turn_in_flight`.
        chat.rpc.push_notification_for_test(
            "session/update",
            serde_json::json!({
                "type": "turn_complete",
                "session_id": "sess-1",
                "outcome": "completed",
                "content": "durable answer",
            }),
        );
        for _ in 0..64 {
            chat.rpc
                .push_notification_for_test("test/noop", serde_json::Value::Null);
        }
        chat.drain_notifications();

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("session remains active during recovery");
        };
        assert!(!state.turn_in_flight, "lag resets terminal turn state");
        assert_eq!(state.queue_len(), 1, "queued input remains client-owned");
        assert!(state.pending_approval.is_none());
        assert!(state.pending_elicitation.is_none());
        assert!(chat.session_resync_in_flight.contains("sess-1"));

        let mut saw_cancel = false;
        let mut saw_approval = false;
        let mut saw_elicitation = false;
        while !(saw_cancel && saw_approval && saw_elicitation) {
            let frame =
                next_rpc_request(&mut writer_rx, "resync should cancel and invalidate").await;
            match frame.get("method").and_then(serde_json::Value::as_str) {
                Some(method::SESSION_CANCEL) => {
                    saw_cancel = true;
                    respond_ok(
                        &outbound,
                        &frame,
                        serde_json::json!({ "session_id": "sess-1", "cancelled": true }),
                    );
                }
                Some(method::SESSION_APPROVE) => {
                    saw_approval = true;
                    respond_ok(&outbound, &frame, serde_json::json!({ "approved": false }));
                }
                None => {
                    saw_elicitation = true;
                    assert_eq!(frame["id"], "elicitation-lag");
                    assert_eq!(frame["result"]["action"], "cancel");
                }
                other => panic!("unexpected recovery frame: {other:?}"),
            }
        }

        let running = next_rpc_request(&mut writer_rx, "resync must confirm terminal state").await;
        assert_eq!(running["method"], method::SESSION_STATE);
        respond_ok(
            &outbound,
            &running,
            serde_json::json!({ "session_id": "sess-1", "state": "running" }),
        );
        assert!(
            writer_rx.try_recv().is_err(),
            "overflow recovery must not reload or dispatch while the old turn is running"
        );

        let idle = next_rpc_request(&mut writer_rx, "resync should poll until terminal").await;
        assert_eq!(idle["method"], method::SESSION_STATE);
        respond_ok(
            &outbound,
            &idle,
            serde_json::json!({
                "session_id": "sess-1",
                "state": "idle",
                "plan": [{
                    "content": "Authoritative recovery task",
                    "status": "in_progress",
                    "priority": "high",
                    "activeForm": "Recovering task"
                }]
            }),
        );

        let messages = next_rpc_request(&mut writer_rx, "terminal resync reloads transcript").await;
        assert_eq!(messages["method"], method::SESSION_MESSAGES);
        respond_ok(
            &outbound,
            &messages,
            serde_json::json!({
                "messages": [
                    { "role": "user", "content": "in flight" },
                    { "role": "assistant", "content": "durable answer" }
                ]
            }),
        );
        assert!(
            writer_rx.try_recv().is_err(),
            "queue dispatch stays gated until transcript reload completes"
        );

        tokio::time::timeout(Duration::from_secs(2), async {
            while chat.session_resync_rx.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("session/messages should finish resync");
        // The daemon's authoritative idle response is ordered after this old
        // terminal notification on the same RPC stream. Exercise the real
        // frame-tick order: the resync gate must discard the old completion
        // before its result releases the queued follow-up.
        chat.rpc.push_notification_for_test(
            "session/update",
            serde_json::json!({
                "type": "turn_complete",
                "session_id": "sess-1",
                "outcome": "completed",
                "content": "stale late completion",
            }),
        );
        chat.tick_transport_events();
        let prompt = next_rpc_request(&mut writer_rx, "recovery should release queued input").await;
        assert_eq!(prompt["method"], method::SESSION_PROMPT);
        assert_eq!(prompt["params"]["prompt"], "send after recovery");
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("session remains active after recovery");
        };
        assert_eq!(state.queue_len(), 0);
        assert!(state.turn_in_flight, "queued follow-up is now in flight");
        assert!(state.entries.iter().all(|entry| !matches!(
            entry,
            ChatEntry::AgentMessage(message) if message.as_ref() == "stale late completion"
        )));
        assert!(matches!(
            state.entries.get(1),
            Some(ChatEntry::AgentMessage(message)) if message.as_ref() == "durable answer"
        ));
        assert_eq!(state.todo_tracker.entries().len(), 1);
        assert_eq!(
            state.todo_tracker.entries()[0].content,
            "Authoritative recovery task"
        );
        assert!(state.entries.iter().any(|entry| matches!(
            entry,
            ChatEntry::SystemMessage(message)
                if message.as_ref() == crate::i18n::t("zc-chat-resynced")
        )));
    }

    #[tokio::test]
    async fn notification_resync_retries_a_transient_rpc_failure() {
        let (tx, mut writer_rx) = mpsc::channel::<String>(16);
        let outbound = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(outbound.clone()));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active.push_user_message(Some("interrupted".to_string()), Vec::new());
        active
            .enqueue_message("send after retry".to_string(), Vec::new())
            .expect("queue follow-up");
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.session_order = vec!["sess-1".to_string()];

        chat.begin_session_resync("sess-1".to_string());
        let cancel = next_rpc_request(&mut writer_rx, "first resync cancels the turn").await;
        assert_eq!(cancel["method"], method::SESSION_CANCEL);
        respond_ok(
            &outbound,
            &cancel,
            serde_json::json!({ "session_id": "sess-1", "cancelled": true }),
        );
        let state_request =
            next_rpc_request(&mut writer_rx, "first resync checks terminal state").await;
        assert_eq!(state_request["method"], method::SESSION_STATE);
        respond_err(
            &outbound,
            &state_request,
            crate::jsonrpc::error_codes::INTERNAL_ERROR,
            "transient state lookup failure",
        );

        assert!(chat.session_resync_in_flight.contains("sess-1"));
        let retry_cancel =
            next_rpc_request(&mut writer_rx, "resync must retry after a transient error").await;
        assert_eq!(retry_cancel["method"], method::SESSION_CANCEL);
        respond_ok(
            &outbound,
            &retry_cancel,
            serde_json::json!({ "session_id": "sess-1", "cancelled": false }),
        );
        let retry_state =
            next_rpc_request(&mut writer_rx, "retry checks terminal state again").await;
        assert_eq!(retry_state["method"], method::SESSION_STATE);
        respond_ok(
            &outbound,
            &retry_state,
            serde_json::json!({ "session_id": "sess-1", "state": "idle" }),
        );
        let messages = next_rpc_request(&mut writer_rx, "retry reloads the transcript").await;
        assert_eq!(messages["method"], method::SESSION_MESSAGES);
        respond_ok(
            &outbound,
            &messages,
            serde_json::json!({
                "messages": [
                    { "role": "user", "content": "interrupted" },
                    { "role": "assistant", "content": "durable answer" }
                ]
            }),
        );

        tokio::time::timeout(Duration::from_secs(2), async {
            while chat.session_resync_rx.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the retry must finish resynchronization");
        chat.tick_transport_events();

        let prompt = next_rpc_request(&mut writer_rx, "retry releases queued input").await;
        assert_eq!(prompt["method"], method::SESSION_PROMPT);
        assert_eq!(prompt["params"]["prompt"], "send after retry");
        assert!(!chat.session_resync_in_flight.contains("sess-1"));
    }

    #[tokio::test]
    async fn exhausted_resync_survives_reconnect_and_retries_without_losing_queue() {
        let (tx, mut writer_rx) = mpsc::channel::<String>(32);
        let outbound = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(outbound.clone()));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active
            .enqueue_message("exact queued prompt".to_string(), Vec::new())
            .expect("queue follow-up");
        active.todo_tracker.set_plan(vec![crate::wire::PlanEntry {
            content: "stale plan".to_string(),
            status: crate::wire::PlanStatus::InProgress,
            priority: crate::wire::PlanPriority::High,
            active_form: None,
        }]);
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.session_order = vec!["sess-1".to_string()];

        chat.begin_session_resync("sess-1".to_string());
        for _ in 0..SESSION_RECOVERY_MAX_ATTEMPTS {
            let cancel = next_rpc_request(&mut writer_rx, "each recovery attempt cancels").await;
            assert_eq!(cancel["method"], method::SESSION_CANCEL);
            respond_ok(
                &outbound,
                &cancel,
                serde_json::json!({ "session_id": "sess-1", "cancelled": false }),
            );
            let state_request =
                next_rpc_request(&mut writer_rx, "each recovery attempt checks state").await;
            assert_eq!(state_request["method"], method::SESSION_STATE);
            respond_err(
                &outbound,
                &state_request,
                crate::jsonrpc::error_codes::INTERNAL_ERROR,
                "controlled resync failure",
            );
        }

        tokio::time::timeout(Duration::from_secs(2), async {
            while chat.session_resync_rx.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("bounded recovery attempts should terminate");
        chat.tick_transport_events();

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("session remains visible after failed recovery");
        };
        assert_eq!(state.last_error, Some(SessionError::ResyncFailed));
        assert_eq!(
            state.queue_len(),
            1,
            "client-owned prompt must remain queued"
        );
        assert_eq!(state.todo_tracker.entries()[0].content, "stale plan");
        assert!(!chat.session_resync_in_flight.contains("sess-1"));
        assert_eq!(chat.session_summaries()[0].status, SidebarStatus::Errored);
        chat.rpc.push_notification_for_test(
            "session/update",
            serde_json::json!({
                "type": "agent_message_chunk",
                "session_id": "sess-1",
                "text": "untrusted partial update"
            }),
        );
        chat.tick_transport_events();
        assert!(
            writer_rx.try_recv().is_err(),
            "routine transport ticks must neither apply partial state nor retry forever"
        );

        let mut resume = chat.resume_entries();
        assert_eq!(resume.len(), 1);
        let entry = resume.remove(0);
        assert!(entry.recovery_required);
        assert!(!entry.interrupted);

        // Exercise the production reconnect path. Reattaching and loading the
        // durable history must automatically reinstall the recovery barrier;
        // the first post-reconnect operation is cancellation/reconciliation,
        // never the retained queued prompt.
        let mut reconnected = Chat::new(chat.rpc.clone(), PaneKind::Chat);
        reconnected.set_resume_sessions(vec![entry]);
        let reconnect = tokio::spawn(async move {
            reconnected.start_session("myagent", None).await;
            reconnected
        });

        let session_new = next_rpc_request(&mut writer_rx, "reconnect should reattach").await;
        assert_eq!(session_new["method"], method::SESSION_NEW);
        assert_eq!(session_new["params"]["session_id"], "sess-1");
        respond_ok(
            &outbound,
            &session_new,
            serde_json::json!({
                "session_id": "sess-1",
                "workspace_dir": "/tmp/reconnected"
            }),
        );
        let config = next_rpc_request(&mut writer_rx, "reconnect refreshes model identity").await;
        assert_eq!(config["method"], method::CONFIG_LIST);
        respond_ok(&outbound, &config, serde_json::json!([]));
        let history = next_rpc_request(&mut writer_rx, "reconnect loads durable history").await;
        assert_eq!(history["method"], method::SESSION_MESSAGES);
        respond_ok(
            &outbound,
            &history,
            serde_json::json!({ "messages": [], "total": 0, "start": 0 }),
        );

        let cancel = next_rpc_request(
            &mut writer_rx,
            "reconnect must reconcile before dispatching the queued prompt",
        )
        .await;
        assert_eq!(cancel["method"], method::SESSION_CANCEL);
        let mut chat = reconnect.await.unwrap();
        assert!(chat.session_resync_in_flight.contains("sess-1"));
        assert_eq!(
            chat.state_for_session("sess-1")
                .expect("reattached session")
                .queue_len(),
            1
        );
        respond_ok(
            &outbound,
            &cancel,
            serde_json::json!({ "session_id": "sess-1", "cancelled": false }),
        );
        let state_request = next_rpc_request(&mut writer_rx, "retry checks terminal state").await;
        respond_ok(
            &outbound,
            &state_request,
            serde_json::json!({ "session_id": "sess-1", "state": "idle", "plan": [] }),
        );
        let messages = next_rpc_request(&mut writer_rx, "retry reloads transcript").await;
        respond_ok(&outbound, &messages, serde_json::json!({ "messages": [] }));

        tokio::time::timeout(Duration::from_secs(2), async {
            while chat.session_resync_rx.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("explicit retry should finish");
        chat.tick_transport_events();

        let prompt =
            next_rpc_request(&mut writer_rx, "successful retry releases exact prompt").await;
        assert_eq!(prompt["method"], method::SESSION_PROMPT);
        assert_eq!(prompt["params"]["prompt"], "exact queued prompt");
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("session stays active after recovery");
        };
        assert!(state.todo_tracker.entries().is_empty());
        assert_eq!(state.last_error, None);
    }

    #[tokio::test]
    async fn background_turn_complete_marks_error_without_touching_focused() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);

        chat.rpc.push_notification_for_test(
            "session/update",
            serde_json::json!({
                "type": "turn_complete",
                "session_id": "sess-b",
                "outcome": "failed",
                "content": "provider exploded",
            }),
        );
        chat.drain_notifications();

        let summaries = chat.session_summaries();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].session_id, "sess-a");
        assert!(summaries[0].focused);
        assert_eq!(summaries[0].status, SidebarStatus::Ready);
        assert_eq!(summaries[1].session_id, "sess-b");
        assert!(!summaries[1].focused);
        assert_eq!(summaries[1].status, SidebarStatus::Errored);
    }

    #[tokio::test]
    async fn focus_session_swaps_states_and_preserves_transcripts() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);
        if let ChatPhase::Active(a) = &mut chat.phase {
            a.entries
                .push(ChatEntry::SystemMessage(Arc::<str>::from("from-a")));
            a.rebuild_lines(40);
        }
        if let Some(b) = chat.background.first_mut() {
            b.entries
                .push(ChatEntry::SystemMessage(Arc::<str>::from("from-b")));
            b.rebuild_lines(40);
        }

        let cached_a = chat
            .state_for_session("sess-a")
            .expect("focused session")
            .cached_lines
            .clone();
        let cached_b = chat
            .state_for_session("sess-b")
            .expect("background session")
            .cached_lines
            .clone();

        assert!(!chat.focus_session("nope").await, "unknown id is a no-op");
        assert!(chat.focus_session("sess-b").await);

        assert_eq!(chat.current_session_id(), Some("sess-b"));
        let ChatPhase::Active(b) = &chat.phase else {
            panic!("focus must activate the picked session");
        };
        assert!(matches!(&b.entries[0], ChatEntry::SystemMessage(m) if m.as_ref() == "from-b"));
        assert_eq!(b.dirty, LinesDirty::Clean);
        assert_eq!(b.cached_lines, cached_b);
        let a = chat
            .background
            .iter()
            .find(|s| s.session_id == "sess-a")
            .expect("previous session stays tracked");
        assert!(matches!(&a.entries[0], ChatEntry::SystemMessage(m) if m.as_ref() == "from-a"));
        assert_eq!(a.dirty, LinesDirty::Clean);
        assert_eq!(a.cached_lines, cached_a);

        assert!(chat.focus_session("sess-a").await);
        let ChatPhase::Active(a) = &chat.phase else {
            panic!("round-trip focus must reactivate the original session");
        };
        assert_eq!(a.dirty, LinesDirty::Clean);
        assert_eq!(a.cached_lines, cached_a);
        let b = chat
            .background
            .iter()
            .find(|s| s.session_id == "sess-b")
            .expect("second session stays tracked after round-trip focus");
        assert_eq!(b.dirty, LinesDirty::Clean);
        assert_eq!(b.cached_lines, cached_b);

        // Sidebar order is stable across focus changes.
        let ids: Vec<_> = chat
            .session_summaries()
            .into_iter()
            .map(|s| (s.session_id, s.focused))
            .collect();
        assert_eq!(
            ids,
            vec![("sess-a".to_string(), true), ("sess-b".to_string(), false)]
        );
    }

    #[tokio::test]
    async fn close_focused_session_calls_daemon_and_promotes_next() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);
        let promoted_cache = {
            let promoted = chat.background.first_mut().expect("background session");
            promoted
                .entries
                .push(ChatEntry::SystemMessage(Arc::<str>::from("from-b")));
            promoted.rebuild_lines(40);
            promoted.cached_lines.clone()
        };

        let handle = tokio::spawn(async move {
            let mut chat = chat;
            assert!(chat.close_session("sess-a").await);
            chat
        });

        let request = next_rpc_request(&mut rx, "close must tell the daemon").await;
        assert_eq!(request["method"], "session/close");
        assert_eq!(request["params"]["session_id"], "sess-a");
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({ "session_id": "sess-a", "closed": true }),
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("close should finish")
            .unwrap();
        assert_eq!(
            chat.current_session_id(),
            Some("sess-b"),
            "closing the focused session promotes the next tracked one"
        );
        let summaries = chat.session_summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].session_id, "sess-b");
        let ChatPhase::Active(promoted) = &chat.phase else {
            panic!("remaining session must be promoted");
        };
        assert_eq!(promoted.dirty, LinesDirty::Clean);
        assert_eq!(promoted.cached_lines, promoted_cache);
    }

    #[tokio::test]
    async fn failed_close_keeps_session_tracked_for_retry() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let chat = two_session_chat(&rpc);

        let handle = tokio::spawn(async move {
            let mut chat = chat;
            assert!(!chat.close_session("sess-a").await);
            chat
        });
        let request = next_rpc_request(&mut rx, "close must tell the daemon").await;
        respond_err(
            &rpc,
            &request,
            crate::jsonrpc::error_codes::INTERNAL_ERROR,
            "temporary failure",
        );

        let chat = handle.await.unwrap();
        assert_eq!(chat.current_session_id(), Some("sess-a"));
        assert_eq!(chat.session_summaries().len(), 2);
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("failed close must retain the focused session");
        };
        assert!(state.info_message.is_some());
    }

    #[tokio::test]
    async fn close_not_found_removes_stale_local_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let chat = two_session_chat(&rpc);

        let handle = tokio::spawn(async move {
            let mut chat = chat;
            assert!(chat.close_session("sess-a").await);
            chat
        });
        let request = next_rpc_request(&mut rx, "close must tell the daemon").await;
        respond_err(
            &rpc,
            &request,
            crate::jsonrpc::error_codes::SESSION_NOT_FOUND,
            "Session not found",
        );

        let chat = handle.await.unwrap();
        assert_eq!(chat.current_session_id(), Some("sess-b"));
        assert_eq!(chat.session_summaries().len(), 1);
    }

    #[tokio::test]
    async fn queued_messages_pump_for_background_sessions() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);
        if let Some(b) = chat.background.first_mut() {
            b.enqueue_message("queued for b".to_string(), Vec::new())
                .expect("enqueue");
        }

        chat.rpc.push_notification_for_test(
            "session/update",
            serde_json::json!({
                "type": "turn_complete",
                "session_id": "sess-b",
                "outcome": "completed",
                "content": "done",
            }),
        );
        chat.drain_notifications();

        let prompt = next_rpc_request(&mut rx, "background queue must dispatch").await;
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["session_id"], "sess-b");
        assert_eq!(prompt["params"]["prompt"], "queued for b");
        let b = chat.background.first().expect("background session");
        assert!(b.turn_in_flight, "dispatch marks the turn in flight");
    }

    #[tokio::test]
    async fn focused_next_send_reattaches_before_prompt_and_preserves_queue_on_failure() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);
        let ChatPhase::Active(state) = &mut chat.phase else {
            panic!("focused session must be active");
        };
        state.last_error = Some(SessionError::SessionLost);
        state
            .enqueue_message("retry focused".to_string(), Vec::new())
            .expect("enqueue retry");

        chat.pump_all_queues();
        let reattach = next_rpc_request(&mut rx, "lost session must reattach before prompt").await;
        assert_eq!(reattach["method"], "session/new");
        assert_eq!(reattach["params"]["session_id"], "sess-a");
        chat.pump_all_queues();
        assert!(
            rx.try_recv().is_err(),
            "session/prompt and duplicate reattachments must wait for the in-flight operation"
        );

        respond_err(&rpc, &reattach, -32000, "reattach failed");
        apply_next_reattach_result(&mut chat, "failed reattach must return to the pane").await;
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("focused session must remain active");
        };
        assert_eq!(state.queue_len(), 1, "failed reattach keeps the message");
        assert_eq!(state.last_error, Some(SessionError::SessionLost));
        assert!(!state.turn_in_flight);
        assert!(
            rx.try_recv().is_err(),
            "failure must not dispatch the prompt"
        );

        chat.pump_all_queues();
        let reattach = next_rpc_request(&mut rx, "explicit retry must reattach again").await;
        assert_eq!(reattach["method"], "session/new");
        respond_ok(
            &rpc,
            &reattach,
            serde_json::json!({ "session_id": "sess-a", "workspace_dir": null }),
        );
        apply_next_reattach_result(&mut chat, "successful reattach must return to the pane").await;

        let prompt = next_rpc_request(&mut rx, "successful reattach must release the prompt").await;
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["session_id"], "sess-a");
        assert_eq!(prompt["params"]["prompt"], "retry focused");
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("focused session must remain active");
        };
        assert_eq!(state.queue_len(), 0);
        assert_eq!(state.last_error, None);
        assert!(state.turn_in_flight);
    }

    #[tokio::test]
    async fn lost_background_session_reattaches_before_queue_dispatch() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);
        let background = chat.background.first_mut().expect("background session");
        background.last_error = Some(SessionError::SessionLost);
        background
            .enqueue_message("retry background".to_string(), Vec::new())
            .expect("enqueue retry");

        chat.pump_all_queues();
        let reattach = next_rpc_request(
            &mut rx,
            "lost background session must reattach before prompt",
        )
        .await;
        assert_eq!(reattach["method"], "session/new");
        assert_eq!(reattach["params"]["session_id"], "sess-b");
        assert!(
            rx.try_recv().is_err(),
            "background session/prompt must wait for reattachment"
        );
        respond_ok(
            &rpc,
            &reattach,
            serde_json::json!({ "session_id": "sess-b", "workspace_dir": null }),
        );
        apply_next_reattach_result(
            &mut chat,
            "successful background reattach must return to the pane",
        )
        .await;

        let prompt = next_rpc_request(&mut rx, "background prompt must follow reattach").await;
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["session_id"], "sess-b");
        assert_eq!(prompt["params"]["prompt"], "retry background");
        let background = chat.background.first().expect("background session");
        assert_eq!(background.queue_len(), 0);
        assert_eq!(background.last_error, None);
        assert!(background.turn_in_flight);
    }

    #[tokio::test]
    async fn reconnect_reattaches_background_sessions_after_focused() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.set_resume_sessions(vec![
            resume_entry("sess-f", "beta", true),
            resume_entry("sess-bg", "alpha", false),
        ]);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init requests the agent list").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0},
                    {"alias": "beta", "enabled": true, "live_sessions": 0, "persisted_sessions": 0}
                ]
            }),
        );

        // Focused entry reattaches first.
        let request = next_rpc_request(&mut rx, "focused resume").await;
        assert_eq!(request["method"], "session/new");
        assert_eq!(request["params"]["agent_alias"], "beta");
        assert_eq!(request["params"]["session_id"], "sess-f");
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({ "session_id": "sess-f", "workspace_dir": "/w" }),
        );
        let request = next_rpc_request(&mut rx, "model identity for focused").await;
        assert_eq!(request["method"], "config/list");
        respond_ok(&rpc, &request, serde_json::json!([]));
        let request = next_rpc_request(&mut rx, "history replay for focused").await;
        assert_eq!(request["method"], "session/messages");
        respond_ok(&rpc, &request, serde_json::json!({ "messages": [] }));

        // Then the background entry rehydrates without stealing focus.
        let request = next_rpc_request(&mut rx, "background resume").await;
        assert_eq!(request["method"], "session/new");
        assert_eq!(request["params"]["agent_alias"], "alpha");
        assert_eq!(request["params"]["session_id"], "sess-bg");
        assert_eq!(request["params"]["keep_siblings"], true);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({ "session_id": "sess-bg", "workspace_dir": "/w" }),
        );
        let request = next_rpc_request(&mut rx, "model identity for background").await;
        assert_eq!(request["method"], "config/list");
        respond_ok(&rpc, &request, serde_json::json!([]));
        let request = next_rpc_request(&mut rx, "history replay for background").await;
        assert_eq!(request["method"], "session/messages");
        respond_ok(&rpc, &request, serde_json::json!({ "messages": [] }));

        let chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("init should finish")
            .unwrap();
        let rows: Vec<_> = chat
            .session_summaries()
            .into_iter()
            .map(|s| (s.session_id, s.focused))
            .collect();
        assert_eq!(
            rows,
            vec![("sess-f".to_string(), true), ("sess-bg".to_string(), false)]
        );
    }

    #[tokio::test]
    async fn failed_background_resume_is_retained_for_reconnect_retry() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(state_for("sess-f", "beta")));
        chat.session_order.push("sess-f".to_string());
        chat.session_order.push("sess-bg".to_string());
        let mut retained = resume_entry("sess-bg", "alpha", false);
        retained.queue.messages.push_back(QueuedMessage {
            id: 0,
            text: "queued while disconnected".to_string(),
            attachments: Vec::new(),
            status: QueueItemStatus::Pending,
        });
        retained.queue.next_id = 1;
        chat.resume_backgrounds.push(retained);

        let retry = tokio::spawn(async move {
            chat.after_session_start().await;
            chat
        });
        let request = next_rpc_request(&mut rx, "background resume should reattach").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["session_id"], "sess-bg");
        respond_err(&rpc, &request, -32000, "controlled background failure");

        let mut chat = tokio::time::timeout(Duration::from_secs(2), retry)
            .await
            .expect("background retry should return")
            .unwrap();
        assert_eq!(
            chat.resume_backgrounds
                .iter()
                .map(|entry| entry.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["sess-bg"]
        );
        assert_eq!(chat.current_session_id(), Some("sess-f"));
        assert_eq!(
            chat.session_summaries()
                .into_iter()
                .map(|summary| (summary.session_id, summary.status, summary.focused))
                .collect::<Vec<_>>(),
            vec![
                ("sess-f".to_string(), SidebarStatus::Ready, true),
                ("sess-bg".to_string(), SidebarStatus::Errored, false),
            ],
            "a failed resume remains an explicit retryable sidebar row"
        );

        let retry = tokio::spawn(async move {
            let focused = chat.focus_session("sess-bg").await;
            (chat, focused)
        });
        let request =
            next_rpc_request(&mut rx, "sidebar retry should reattach exact session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["session_id"], "sess-bg");
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({ "session_id": "sess-bg", "workspace_dir": "/w" }),
        );
        let request = next_rpc_request(&mut rx, "sidebar retry refreshes model identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));
        let request = next_rpc_request(&mut rx, "sidebar retry reloads transcript").await;
        assert_eq!(request["method"], method::SESSION_MESSAGES);
        respond_ok(&rpc, &request, serde_json::json!({ "messages": [] }));

        let (chat, focused) = tokio::time::timeout(Duration::from_secs(2), retry)
            .await
            .expect("explicit retry should finish")
            .unwrap();
        assert!(focused);
        assert_eq!(chat.current_session_id(), Some("sess-bg"));
        assert_eq!(chat.last_focused_sid.as_deref(), Some("sess-f"));
        assert!(chat.resume_backgrounds.is_empty());
        let request = next_rpc_request(&mut rx, "retained queue should dispatch after retry").await;
        assert_eq!(request["method"], method::SESSION_PROMPT);
        assert_eq!(request["params"]["session_id"], "sess-bg");
        assert_eq!(request["params"]["prompt"], "queued while disconnected");
    }

    #[tokio::test]
    async fn retained_resume_entries_count_toward_the_session_cap() {
        let mut chat = active_chat();
        chat.session_order = vec!["sess-1".to_string()];
        for idx in 2..=MAX_TRACKED_SESSIONS_PER_PANE {
            let session_id = format!("sess-{idx}");
            chat.session_order.push(session_id.clone());
            chat.resume_backgrounds
                .push(resume_entry(&session_id, "agent", false));
        }

        assert_eq!(chat.tracked_session_count(), MAX_TRACKED_SESSIONS_PER_PANE);
    }

    #[tokio::test]
    async fn closing_a_focused_retained_session_promotes_a_live_background() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("resume failed".to_string());
        chat.background.push(state_for("sess-live", "beta"));
        chat.session_order = vec!["sess-failed".to_string(), "sess-live".to_string()];
        chat.resume_focused = Some(resume_entry("sess-failed", "alpha", true));

        let close = tokio::spawn(async move {
            let closed = chat.close_session("sess-failed").await;
            (chat, closed)
        });
        let request = next_rpc_request(&mut rx, "retained close should reach the daemon").await;
        assert_eq!(request["method"], method::SESSION_CLOSE);
        assert_eq!(request["params"]["session_id"], "sess-failed");
        respond_ok(&rpc, &request, serde_json::Value::Null);

        let (chat, closed) = close.await.unwrap();
        assert!(closed);
        assert!(chat.resume_focused.is_none());
        assert_eq!(chat.current_session_id(), Some("sess-live"));
        assert_eq!(chat.session_order, vec!["sess-live"]);
    }

    #[tokio::test]
    async fn retained_session_close_failure_surfaces_on_the_active_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Active(Box::new(state_for("sess-live", "beta")));
        chat.session_order = vec!["sess-live".to_string(), "sess-failed".to_string()];
        chat.resume_backgrounds
            .push(resume_entry("sess-failed", "alpha", false));

        let close = tokio::spawn(async move {
            let closed = chat.close_session("sess-failed").await;
            (chat, closed)
        });
        let request = next_rpc_request(&mut rx, "retained close should reach the daemon").await;
        assert_eq!(request["method"], method::SESSION_CLOSE);
        respond_err(
            &rpc,
            &request,
            crate::jsonrpc::error_codes::INTERNAL_ERROR,
            "controlled close failure",
        );

        let (chat, closed) = close.await.unwrap();
        assert!(!closed);
        assert_eq!(chat.resume_backgrounds.len(), 1);
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("the live session should remain active");
        };
        assert!(
            state
                .info_message
                .as_ref()
                .is_some_and(|message| message.text.contains("controlled close failure")),
            "a retained-row close failure must be visible on the surviving session"
        );
    }

    #[tokio::test]
    async fn acp_init_opens_recent_session_picker() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0},
                    {"alias": "beta", "enabled": true, "live_sessions": 0, "persisted_sessions": 1}
                ]
            }),
        );

        let request = next_rpc_request(&mut rx, "ACP init should request recent sessions").await;
        assert_eq!(request["method"], method::SESSION_LIST_ACP);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "sessions": [
                    {
                        "session_id": "sess-ghost",
                        "session_key": "sess-ghost",
                        "created_at": "2026-07-07T00:00:00Z",
                        "last_activity": "2026-07-07T00:10:00Z",
                        "message_count": 1,
                        "agent_alias": "ghost",
                        "channel_id": null,
                        "name": "Ghost"
                    },
                    {
                        "session_id": "sess-beta",
                        "session_key": "sess-beta",
                        "created_at": "2026-07-07T00:00:00Z",
                        "last_activity": "2026-07-07T00:05:00Z",
                        "message_count": 2,
                        "agent_alias": "beta",
                        "channel_id": null,
                        "name": "Beta work"
                    }
                ]
            }),
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("init should finish")
            .unwrap();
        let ChatPhase::PickSession {
            sessions,
            list_state,
            agents,
        } = chat.phase
        else {
            panic!("ACP init should show the saved-session picker");
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "sess-beta");
        assert_eq!(sessions[0].agent_alias.as_deref(), Some("beta"));
        assert_eq!(list_state.selected(), Some(0));
        assert_eq!(agents, vec!["alpha", "beta"]);
    }

    #[tokio::test]
    async fn acp_init_session_picker_enter_resumes_selected_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "beta", "enabled": true, "live_sessions": 0, "persisted_sessions": 1}
                ]
            }),
        );

        let request = next_rpc_request(&mut rx, "ACP init should request recent sessions").await;
        assert_eq!(request["method"], method::SESSION_LIST_ACP);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "sessions": [
                    {
                        "session_id": "sess-beta",
                        "session_key": "sess-beta",
                        "created_at": "2026-07-07T00:00:00Z",
                        "last_activity": "2026-07-07T00:05:00Z",
                        "message_count": 2,
                        "agent_alias": "beta",
                        "channel_id": null,
                        "name": "Beta work"
                    }
                ]
            }),
        );

        let mut chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("init should finish")
            .unwrap();
        assert!(matches!(chat.phase, ChatPhase::PickSession { .. }));

        let resume = tokio::spawn(async move {
            let entry = match &chat.phase {
                ChatPhase::PickSession { sessions, .. } => sessions[0].clone(),
                _ => panic!("expected saved-session picker"),
            };
            chat.resume_session_entry(entry).await;
            chat
        });

        let request = next_rpc_request(&mut rx, "Enter should resume selected session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        let params = &request["params"];
        assert_eq!(params["agent_alias"], "beta");
        assert_eq!(params["session_id"], "sess-beta");
        assert_eq!(params["chat_mode"], "acp");
        assert_eq!(params["exclude_memory"], true);
        assert!(params["cwd"].is_null());
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "session_id": "sess-beta",
                "workspace_dir": "/tmp/beta"
            }),
        );

        let request = next_rpc_request(&mut rx, "resume should refresh model identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        assert_eq!(request["params"]["prefix"], "agents.beta.model_provider");
        respond_ok(&rpc, &request, serde_json::json!([]));

        let request = next_rpc_request(&mut rx, "resume should load history").await;
        assert_eq!(request["method"], method::SESSION_MESSAGES);
        assert_eq!(request["params"]["session_id"], "sess-beta");
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "messages": [
                    {"role": "user", "content": "resume me"}
                ],
                "total": 1,
                "start": 0
            }),
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), resume)
            .await
            .expect("resume should finish")
            .unwrap();
        let ChatPhase::Active(state) = chat.phase else {
            panic!("Enter should enter the saved ACP session");
        };
        assert_eq!(state.session_id, "sess-beta");
        assert_eq!(state.agent_alias, "beta");
        assert_eq!(state.cwd.as_deref(), Some("/tmp/beta"));
    }

    #[tokio::test]
    async fn acp_init_session_picker_cancel_starts_fresh_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "beta", "enabled": true, "live_sessions": 1, "persisted_sessions": 1}
                ]
            }),
        );

        let request = next_rpc_request(&mut rx, "ACP init should request recent sessions").await;
        assert_eq!(request["method"], method::SESSION_LIST_ACP);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "sessions": [
                    {
                        "session_id": "sess-beta",
                        "session_key": "sess-beta",
                        "created_at": "2026-07-07T00:00:00Z",
                        "last_activity": "2026-07-07T00:05:00Z",
                        "message_count": 2,
                        "agent_alias": "beta",
                        "channel_id": null,
                        "name": "Beta work"
                    }
                ]
            }),
        );

        let mut chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("init should finish")
            .unwrap();
        assert!(matches!(chat.phase, ChatPhase::PickSession { .. }));

        let fresh = tokio::spawn(async move {
            let agents = match &chat.phase {
                ChatPhase::PickSession { agents, .. } => agents.clone(),
                _ => panic!("expected saved-session picker"),
            };
            chat.start_fresh_from_picker(agents).await;
            chat
        });

        let request = next_rpc_request(&mut rx, "Esc should start a fresh session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        let params = &request["params"];
        assert_eq!(params["agent_alias"], "beta");
        assert_eq!(params["chat_mode"], "acp");
        assert_eq!(params["exclude_memory"], true);
        assert!(params["session_id"].is_null());
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "session_id": "sess-fresh",
                "workspace_dir": "/tmp/fresh"
            }),
        );

        let request =
            next_rpc_request(&mut rx, "fresh session should refresh model identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));

        let chat = tokio::time::timeout(Duration::from_secs(2), fresh)
            .await
            .expect("fresh start should finish")
            .unwrap();
        let ChatPhase::Active(state) = chat.phase else {
            panic!("Esc should enter a fresh ACP session");
        };
        assert_eq!(state.session_id, "sess-fresh");
    }

    #[tokio::test]
    async fn acp_init_retains_carried_resume_for_disabled_agent() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        chat.resume_focused = Some(resume_entry("sess-prev", "beta", true));

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0}
                ]
            }),
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("init should finish")
            .unwrap();
        assert!(
            rx.try_recv().is_err(),
            "a missing owner must not replace the retained session with a fresh one"
        );
        assert_eq!(
            chat.resume_focused
                .as_ref()
                .map(|entry| (entry.session_id.as_str(), entry.agent_alias.as_str())),
            Some(("sess-prev", "beta"))
        );
        assert!(matches!(chat.phase, ChatPhase::Error(_)));
    }

    #[test]
    fn local_code_session_cwd_only_pins_local_acp() {
        assert_eq!(
            local_code_session_cwd(PaneKind::Chat, crate::client::Transport::Local),
            Ok(None)
        );
        assert_eq!(
            local_code_session_cwd(PaneKind::Chat, crate::client::Transport::Wss),
            Ok(None)
        );
        assert_eq!(
            local_code_session_cwd(PaneKind::Acp, crate::client::Transport::Wss),
            Ok(None)
        );
        let expected = std::env::current_dir()
            .expect("process cwd")
            .to_str()
            .expect("utf-8 cwd")
            .to_string();
        assert_eq!(
            local_code_session_cwd(PaneKind::Acp, crate::client::Transport::Local),
            Ok(Some(expected))
        );
    }

    #[test]
    fn local_code_cwd_capture_failure_is_an_error_not_an_omission() {
        // A failed capture must never look like the deliberate `None` used by
        // Chat and remote Code: omitting cwd here would silently root the
        // session at the agent workspace, i.e. a different project.
        let err = resolve_local_code_cwd(Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file or directory",
        )))
        .expect_err("cwd capture failure must be reported");
        let LocalCodeCwdError::Unavailable(msg) = &err else {
            panic!("expected Unavailable, got {err:?}");
        };
        assert!(msg.contains("no such file or directory"), "got {msg}");
        // And it renders as real localized text, not a `{key}` placeholder.
        let shown = err.localized();
        assert!(!shown.starts_with('{'), "unlocalized error text: {shown}");
        assert!(shown.contains("no such file or directory"), "got {shown}");
    }

    #[cfg(unix)]
    #[test]
    fn local_code_cwd_rejects_non_utf8_launch_path() {
        use std::os::unix::ffi::OsStrExt;
        // 0xFF is never valid UTF-8, so this models a launch directory that
        // cannot be sent as a JSON-RPC `cwd` string.
        let raw = std::ffi::OsStr::from_bytes(b"/tmp/proj-\xFF");
        let err = resolve_local_code_cwd(Ok(std::path::PathBuf::from(raw)))
            .expect_err("non-UTF-8 cwd must be reported");
        let LocalCodeCwdError::NotUtf8(shown_path) = &err else {
            panic!("expected NotUtf8, got {err:?}");
        };
        assert!(shown_path.contains("proj-"), "got {shown_path}");
        let shown = err.localized();
        assert!(!shown.starts_with('{'), "unlocalized error text: {shown}");
    }

    #[test]
    fn local_code_cwd_accepts_utf8_launch_path() {
        assert_eq!(
            resolve_local_code_cwd(Ok(std::path::PathBuf::from("/tmp/project"))),
            Ok("/tmp/project".to_string())
        );
    }

    #[tokio::test]
    async fn fresh_local_chat_session_omits_cwd_so_agent_workspace_wins() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        // `with_rpc` defaults to Local transport — the path that used to leak
        // the TUI's launch directory into `session/new`.
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0}
                ]
            }),
        );

        let request = next_rpc_request(&mut rx, "fresh chat should start a session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        let params = &request["params"];
        assert_eq!(params["agent_alias"], "alpha");
        assert!(params["session_id"].is_null());
        // Regression guard: a fresh local session must not send the TUI's
        // launch directory as cwd. Omitting it lets the daemon resolve the
        // selected agent's configured workspace.
        assert!(params["cwd"].is_null());

        init.abort();
    }

    #[tokio::test]
    async fn fresh_local_acp_session_sends_process_cwd() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        let expected_cwd = std::env::current_dir()
            .expect("process cwd")
            .to_str()
            .expect("utf-8 cwd")
            .to_string();

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0}
                ]
            }),
        );

        let request = next_rpc_request(&mut rx, "ACP init should list recent sessions").await;
        assert_eq!(request["method"], method::SESSION_LIST_ACP);
        respond_ok(&rpc, &request, serde_json::json!({ "sessions": [] }));

        let mut chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("init should finish")
            .unwrap();
        assert!(matches!(chat.phase, ChatPhase::PickAgent { .. }));

        let start = tokio::spawn(async move {
            chat.pick_or_start_session("alpha").await;
            chat
        });

        let request = next_rpc_request(&mut rx, "fresh ACP should start a session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        let params = &request["params"];
        assert_eq!(params["agent_alias"], "alpha");
        assert!(params["session_id"].is_null());
        assert_eq!(params["chat_mode"], "acp");
        // Code sessions pin the directory zerocode was launched from so file
        // and shell tools operate on that project, not the agent workspace.
        assert_eq!(params["cwd"], expected_cwd);

        start.abort();
    }

    #[tokio::test]
    async fn restart_local_chat_session_omits_cwd_so_agent_workspace_wins() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut state = ChatState::new(
            "sess-old".to_string(),
            "alpha".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );

        let restart = tokio::spawn(async move {
            Chat::restart_session_for_state(&client, PaneKind::Chat, &mut state).await
        });

        let request = next_rpc_request(&mut rx, "restart should start a fresh session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        let params = &request["params"];
        assert_eq!(params["agent_alias"], "alpha");
        assert!(params["session_id"].is_null());
        // Regression guard: restart must not re-point the session at the TUI's
        // launch directory either.
        assert!(params["cwd"].is_null());
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({ "session_id": "sess-fresh", "workspace_dir": "/tmp/alpha" }),
        );

        let request = next_rpc_request(&mut rx, "restart should close the old session").await;
        assert_eq!(request["method"], method::SESSION_CLOSE);
        assert_eq!(request["params"]["session_id"], "sess-old");
        respond_ok(&rpc, &request, serde_json::json!({}));

        let request = next_rpc_request(&mut rx, "restart should refresh model identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));

        let phase = tokio::time::timeout(Duration::from_secs(2), restart)
            .await
            .expect("restart should finish")
            .unwrap();
        assert!(phase.is_none());
    }

    #[tokio::test]
    async fn restart_local_acp_session_sends_process_cwd() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let expected_cwd = std::env::current_dir()
            .expect("process cwd")
            .to_str()
            .expect("utf-8 cwd")
            .to_string();
        let mut state = ChatState::new(
            "sess-old".to_string(),
            "alpha".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );

        let restart = tokio::spawn(async move {
            Chat::restart_session_for_state(&client, PaneKind::Acp, &mut state).await
        });

        let request = next_rpc_request(&mut rx, "restart should start a fresh ACP session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        let params = &request["params"];
        assert_eq!(params["agent_alias"], "alpha");
        assert!(params["session_id"].is_null());
        assert_eq!(params["chat_mode"], "acp");
        assert_eq!(params["cwd"], expected_cwd);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({ "session_id": "sess-fresh", "workspace_dir": expected_cwd }),
        );

        let request = next_rpc_request(&mut rx, "restart should close the old session").await;
        assert_eq!(request["method"], method::SESSION_CLOSE);
        assert_eq!(request["params"]["session_id"], "sess-old");
        respond_ok(&rpc, &request, serde_json::json!({}));

        let request = next_rpc_request(&mut rx, "restart should refresh model identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));

        let phase = tokio::time::timeout(Duration::from_secs(2), restart)
            .await
            .expect("restart should finish")
            .unwrap();
        assert!(phase.is_none());
    }

    #[tokio::test]
    async fn restart_close_failure_keeps_the_old_session_and_cleans_the_replacement() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut state = ChatState::new(
            "sess-old".to_string(),
            "alpha".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );

        let restart = tokio::spawn(async move {
            let phase = Chat::restart_session_for_state(&client, PaneKind::Chat, &mut state).await;
            (state, phase)
        });

        let request = next_rpc_request(&mut rx, "restart should start a replacement").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({ "session_id": "sess-fresh", "workspace_dir": "/tmp/alpha" }),
        );
        let request = next_rpc_request(&mut rx, "restart should close the old session").await;
        assert_eq!(request["method"], method::SESSION_CLOSE);
        assert_eq!(request["params"]["session_id"], "sess-old");
        respond_err(
            &rpc,
            &request,
            crate::jsonrpc::error_codes::INTERNAL_ERROR,
            "controlled old close failure",
        );
        let request =
            next_rpc_request(&mut rx, "failed restart should clean the replacement").await;
        assert_eq!(request["method"], method::SESSION_CLOSE);
        assert_eq!(request["params"]["session_id"], "sess-fresh");
        respond_ok(&rpc, &request, serde_json::Value::Null);

        let (state, phase) = restart.await.unwrap();
        assert!(phase.is_none());
        assert_eq!(state.session_id, "sess-old");
        assert!(
            state
                .info_message
                .as_ref()
                .is_some_and(|message| message.text.contains("controlled old close failure"))
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "failed restart must not refresh identity for the discarded replacement"
        );
    }

    #[tokio::test]
    async fn restart_replacement_close_failure_is_visible_on_the_retained_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut state = ChatState::new(
            "sess-old".to_string(),
            "alpha".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );

        let restart = tokio::spawn(async move {
            let phase = Chat::restart_session_for_state(&client, PaneKind::Chat, &mut state).await;
            (state, phase)
        });

        let request = next_rpc_request(&mut rx, "restart should start a replacement").await;
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({ "session_id": "sess-fresh", "workspace_dir": "/tmp/alpha" }),
        );
        let request = next_rpc_request(&mut rx, "restart should close the old session").await;
        respond_err(
            &rpc,
            &request,
            crate::jsonrpc::error_codes::INTERNAL_ERROR,
            "controlled old close failure",
        );
        let request =
            next_rpc_request(&mut rx, "failed restart should clean the replacement").await;
        respond_err(
            &rpc,
            &request,
            -32001,
            "controlled replacement cleanup failure",
        );

        let (state, phase) = restart.await.unwrap();
        assert!(phase.is_none());
        assert_eq!(state.session_id, "sess-old");
        let notice = &state.info_message.as_ref().unwrap().text;
        assert!(notice.contains("controlled old close failure"));
        assert!(notice.contains("controlled replacement cleanup failure"));
    }

    #[tokio::test]
    async fn wss_acp_restart_close_failure_keeps_the_old_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc_transport(
            Arc::clone(&rpc),
            crate::client::Transport::Wss,
        ));
        let mut state = ChatState::new(
            "sess-old".to_string(),
            "alpha".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );

        let restart = tokio::spawn(async move {
            let phase = Chat::restart_session_for_state(&client, PaneKind::Acp, &mut state).await;
            (state, phase)
        });

        let request =
            next_rpc_request(&mut rx, "WSS ACP restart should close the old session").await;
        assert_eq!(request["method"], method::SESSION_CLOSE);
        assert_eq!(request["params"]["session_id"], "sess-old");
        respond_err(
            &rpc,
            &request,
            crate::jsonrpc::error_codes::INTERNAL_ERROR,
            "controlled WSS close failure",
        );

        let (state, phase) = restart.await.unwrap();
        assert!(phase.is_none());
        assert_eq!(state.session_id, "sess-old");
        assert!(
            state
                .info_message
                .as_ref()
                .is_some_and(|message| message.text.contains("controlled WSS close failure"))
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "failed WSS ACP restart must not open a CWD picker session"
        );
    }

    #[tokio::test]
    async fn agent_picker_click_selects_row() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        chat.phase = ChatPhase::PickAgent {
            agents: vec!["alpha".into(), "beta".into(), "gamma".into()],
            list_state,
            loading: false,
        };
        // Stored rect is the draw's shifted form: list_click_index treats (y+1)
        // as the first item. With y=1, first item maps to row 2.
        chat.pick_agent_list_area = Rect::new(1, 1, 20, 6);
        // Click the third item → row 2 + 2 = 4.
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 4,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(click, Rect::new(0, 0, 40, 10)).await;
        if let ChatPhase::PickAgent { list_state, .. } = &chat.phase {
            assert_eq!(
                list_state.selected(),
                Some(2),
                "click selects the clicked row"
            );
        } else {
            panic!("expected PickAgent phase");
        }
    }

    #[tokio::test]
    async fn session_picker_double_click_resumes_selected_session() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        let area = Rect::new(0, 0, 35, 30);
        let overlay_area = session_list_overlay_area(area);
        let mut state = ChatState::new(
            "sess-old".to_string(),
            "alpha".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        state.session_overlay = SessionOverlay::List {
            sessions: vec![crate::client::SessionEntry {
                session_id: "sess-new".to_string(),
                session_key: "sess-new".to_string(),
                created_at: "2026-07-07T00:00:00Z".to_string(),
                last_activity: "2026-07-07T00:01:00Z".to_string(),
                message_count: 1,
                agent_alias: Some("beta".to_string()),
                channel_id: None,
                name: Some("Beta work".to_string()),
            }],
            list_state,
        };
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: overlay_area.x + 2,
            row: overlay_area.y + 1,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(click, area).await;

        let switch = tokio::spawn(async move {
            chat.handle_mouse(click, area).await;
            chat
        });

        let request =
            next_rpc_request(&mut rx, "double-click should resume selected session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["agent_alias"], "beta");
        assert_eq!(request["params"]["session_id"], "sess-new");
        assert_eq!(request["params"]["chat_mode"], "acp");
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "session_id": "sess-new",
                "workspace_dir": "/tmp/new"
            }),
        );

        let request = next_rpc_request(&mut rx, "double-click should refresh model identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));
        let request = next_rpc_request(&mut rx, "double-click should load history").await;
        assert_eq!(request["method"], method::SESSION_MESSAGES);
        assert_eq!(request["params"]["session_id"], "sess-new");
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "messages": [
                    {"role": "agent", "content": "restored"}
                ],
                "total": 1,
                "start": 0
            }),
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), switch)
            .await
            .expect("double-click switch should finish")
            .unwrap();
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("double-click should leave the chat active");
        };
        assert_eq!(state.session_id, "sess-new");
        assert_eq!(state.agent_alias, "beta");
        assert_eq!(state.cwd.as_deref(), Some("/tmp/new"));
        assert!(matches!(state.session_overlay, SessionOverlay::None));
        // The previous session is NOT closed: it stays tracked in the
        // sidebar so the user can switch back instantly.
        assert!(
            chat.background.iter().any(|s| s.session_id == "sess-old"),
            "old session stays live in the background"
        );
    }

    #[tokio::test]
    async fn session_picker_double_click_restore_error_keeps_old_session() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        let area = Rect::new(0, 0, 100, 30);
        let overlay_area = session_list_overlay_area(area);
        let mut state = ChatState::new(
            "sess-old".to_string(),
            "alpha".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        state.session_overlay = SessionOverlay::List {
            sessions: vec![crate::client::SessionEntry {
                session_id: "sess-dead".to_string(),
                session_key: "sess-dead".to_string(),
                created_at: "2026-07-07T00:00:00Z".to_string(),
                last_activity: "2026-07-07T00:01:00Z".to_string(),
                message_count: 1,
                agent_alias: Some("beta".to_string()),
                channel_id: None,
                name: Some("Dead work".to_string()),
            }],
            list_state,
        };
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: overlay_area.x + 2,
            row: overlay_area.y + 1,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(click, area).await;

        let switch = tokio::spawn(async move {
            chat.handle_mouse(click, area).await;
            chat
        });

        let request = next_rpc_request(&mut rx, "double-click should try selected session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["agent_alias"], "beta");
        assert_eq!(request["params"]["session_id"], "sess-dead");
        respond_err(
            &rpc,
            &request,
            crate::jsonrpc::error_codes::SESSION_NOT_FOUND,
            "Session not found",
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), switch)
            .await
            .expect("failed switch should finish")
            .unwrap();
        let ChatPhase::Active(state) = chat.phase else {
            panic!("failed switch should keep the chat active");
        };
        assert_eq!(state.session_id, "sess-old");
        assert_eq!(state.agent_alias, "alpha");
        assert!(matches!(state.session_overlay, SessionOverlay::None));
        let info = state
            .info_message
            .as_ref()
            .expect("failed switch should surface an info-bar error");
        assert!(info.text.contains("Failed to switch session"));
        assert!(info.text.contains("Session not found"));
    }

    #[tokio::test]
    async fn session_picker_history_error_keeps_old_session() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        let area = Rect::new(0, 0, 100, 30);
        let overlay_area = session_list_overlay_area(area);
        let mut state = ChatState::new(
            "sess-old".to_string(),
            "alpha".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        state.session_overlay = SessionOverlay::List {
            sessions: vec![crate::client::SessionEntry {
                session_id: "sess-broken".to_string(),
                session_key: "sess-broken".to_string(),
                created_at: "2026-07-07T00:00:00Z".to_string(),
                last_activity: "2026-07-07T00:01:00Z".to_string(),
                message_count: 1,
                agent_alias: Some("beta".to_string()),
                channel_id: None,
                name: Some("Broken work".to_string()),
            }],
            list_state,
        };
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: overlay_area.x + 2,
            row: overlay_area.y + 1,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(click, area).await;

        let switch = tokio::spawn(async move {
            chat.handle_mouse(click, area).await;
            chat
        });

        let request = next_rpc_request(&mut rx, "switch should resume selected session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "session_id": "sess-broken",
                "workspace_dir": "/tmp/broken"
            }),
        );

        let request = next_rpc_request(
            &mut rx,
            "switch should refresh model identity before history",
        )
        .await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));

        let request =
            next_rpc_request(&mut rx, "switch should load history before replacing state").await;
        assert_eq!(request["method"], method::SESSION_MESSAGES);
        respond_err(
            &rpc,
            &request,
            crate::jsonrpc::error_codes::INTERNAL_ERROR,
            "malformed ACP history",
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), switch)
            .await
            .expect("failed history restore should finish")
            .unwrap();
        assert!(
            rx.try_recv().is_err(),
            "failed history replay must not close a session that may be canonical"
        );
        let ChatPhase::Active(state) = chat.phase else {
            panic!("failed history restore should keep the old session active");
        };
        assert_eq!(state.session_id, "sess-old");
        assert_eq!(state.agent_alias, "alpha");
        assert!(matches!(state.session_overlay, SessionOverlay::None));
        let info = state
            .info_message
            .as_ref()
            .expect("failed history restore should surface an error");
        assert!(info.text.contains("Failed to switch session"));
        assert!(info.text.contains("malformed ACP history"));
    }

    #[tokio::test]
    async fn active_agent_title_click_opens_agent_picker() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let area = Rect::new(10, 4, 80, 20);
        let mut state = ChatState::new(
            "abcdef1234".to_string(),
            "beta".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        state.refresh_title_hit_rects(area);
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 4,
            modifiers: KeyModifiers::NONE,
        };
        let switch = tokio::spawn(async move {
            chat.handle_mouse(click, area).await;
            chat
        });

        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("agent title click should request the agent list")
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], method::AGENTS_STATUS);
        let id = request["id"].as_str().unwrap().to_string();
        rpc.dispatch_response(
            &id,
            Some(serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0},
                    {"alias": "beta", "enabled": true, "live_sessions": 1},
                    {"alias": "disabled", "enabled": false, "live_sessions": 0}
                ]
            })),
            None,
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), switch)
            .await
            .expect("agent picker should open after agents/status response")
            .unwrap();
        let ChatPhase::PickAgent {
            agents, list_state, ..
        } = chat.phase
        else {
            panic!("expected PickAgent phase");
        };
        assert_eq!(agents, vec!["alpha".to_string(), "beta".to_string()]);
        assert_eq!(list_state.selected(), Some(1));
    }

    #[tokio::test]
    async fn active_agent_title_click_preserves_running_session_in_picker() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let area = Rect::new(10, 4, 80, 20);
        let mut state = ChatState::new(
            "abcdef1234".to_string(),
            "beta".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        state.turn_in_flight = true;
        state.refresh_title_hit_rects(area);
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 4,
            modifiers: KeyModifiers::NONE,
        };

        let open = tokio::spawn(async move {
            chat.handle_mouse(click, area).await;
            chat
        });
        let request = next_rpc_request(&mut rx, "running session may open agent picker").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({"agents": [
                {"alias": "alpha", "enabled": true}, {"alias": "beta", "enabled": true}
            ]}),
        );
        let mut chat = open.await.unwrap();
        assert!(matches!(chat.phase, ChatPhase::PickAgent { .. }));
        assert!(chat.background[0].turn_in_flight);
        assert!(chat.restore_last_focused().await);
        assert_eq!(chat.current_session_id(), Some("abcdef1234"));
        assert!(chat.state_for_session("abcdef1234").unwrap().turn_in_flight);
        assert!(
            rx.try_recv().is_err(),
            "navigation must not cancel or close the turn"
        );
    }

    #[tokio::test]
    async fn sidebar_add_creates_same_agent_sibling_while_turn_runs() {
        for pane in [PaneKind::Chat, PaneKind::Acp] {
            let (tx, mut rx) = mpsc::channel::<String>(16);
            let rpc = Arc::new(RpcOutbound::new(tx));
            let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
            let mut chat = Chat::new(client, pane);
            let mut old = state_for("sess-old", "alpha");
            old.turn_in_flight = true;
            old.input_bar.insert_text("unsent draft");
            chat.phase = ChatPhase::Active(Box::new(old));
            chat.session_order.push("sess-old".into());
            let add = tokio::spawn(async move {
                chat.add_agent_session("alpha").await;
                chat
            });
            let request = next_rpc_request(&mut rx, "add must create another session").await;
            assert_eq!(request["method"], method::SESSION_NEW);
            assert_eq!(request["params"]["agent_alias"], "alpha");
            assert!(request["params"]["session_id"].is_null());
            assert_eq!(request["params"]["keep_siblings"], true);
            respond_ok(
                &rpc,
                &request,
                serde_json::json!({"session_id": "sess-new", "workspace_dir": "/w"}),
            );
            let request = next_rpc_request(&mut rx, "new session refreshes identity").await;
            assert_eq!(request["method"], method::CONFIG_LIST);
            respond_ok(&rpc, &request, serde_json::json!([]));
            let mut chat = add.await.unwrap();
            assert_eq!(chat.current_session_id(), Some("sess-new"));
            assert_eq!(chat.session_summaries().len(), 2);
            let old = chat.state_for_session("sess-old").unwrap();
            assert!(old.turn_in_flight);
            assert_eq!(old.input_bar.input(), "unsent draft");
            assert!(chat.focus_session("sess-old").await);
            assert!(chat.focus_session("sess-new").await);
            assert!(
                rx.try_recv().is_err(),
                "focus must not send cancel/close/new"
            );
            chat.rpc.push_notification_for_test("session/update", serde_json::json!({
                "type": "turn_complete", "session_id": "sess-old", "outcome": "completed", "content": "done"
            }));
            chat.drain_notifications();
            assert!(!chat.state_for_session("sess-old").unwrap().turn_in_flight);
            assert_eq!(chat.current_session_id(), Some("sess-new"));
        }
    }

    #[tokio::test]
    async fn sidebar_add_does_not_consume_failed_resume_identity_or_queue() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut retained = resume_entry("sess-failed", "beta", true);
        retained.queue.messages.push_back(QueuedMessage {
            id: 0,
            text: "retained queue".into(),
            attachments: Vec::new(),
            status: QueueItemStatus::Pending,
        });
        chat.set_resume_sessions(vec![retained]);
        let add = tokio::spawn(async move {
            chat.add_agent_session("alpha").await;
            chat
        });
        let request = next_rpc_request(&mut rx, "fresh add must not reuse failed resume ID").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["agent_alias"], "alpha");
        assert!(request["params"]["session_id"].is_null());
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({"session_id": "sess-new", "workspace_dir": "/w"}),
        );
        let request = next_rpc_request(&mut rx, "new identity refresh").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));
        let request = next_rpc_request(&mut rx, "failed resume remains a separate owner").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["agent_alias"], "beta");
        assert_eq!(request["params"]["session_id"], "sess-failed");
        respond_err(&rpc, &request, -32000, "still unavailable");
        let chat = add.await.unwrap();
        assert_eq!(chat.current_session_id(), Some("sess-new"));
        assert_eq!(chat.resume_backgrounds[0].session_id, "sess-failed");
        assert_eq!(
            chat.resume_backgrounds[0].queue.messages[0].text,
            "retained queue"
        );
        assert_eq!(chat.tracked_session_count(), 2);
    }

    #[tokio::test]
    async fn switch_session_shortcut_preserves_in_flight_turn() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);
        let ChatPhase::Active(state) = &mut chat.phase else {
            unreachable!()
        };
        state.turn_in_flight = true;
        let switch = tokio::spawn(async move {
            let mut term: crate::config_manager::Term = ratatui::Terminal::with_options(
                crate::terminal_backend::WideCellCleanupBackend::new(std::io::stdout()),
                ratatui::TerminalOptions {
                    viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, 100, 30)),
                },
            )
            .unwrap();
            chat.handle_key(
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
                &mut term,
            )
            .await;
            let ChatPhase::Active(state) = &chat.phase else {
                unreachable!()
            };
            assert!(matches!(state.session_overlay, SessionOverlay::List { .. }));
            chat.handle_key(KeyEvent::from(KeyCode::Enter), &mut term)
                .await;
            chat
        });
        let request = next_rpc_request(&mut rx, "Ctrl+S must work during a turn").await;
        assert_eq!(request["method"], method::SESSION_LIST);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({"sessions": [{
                "session_id": "sess-b", "session_key": "sess-b", "agent_alias": "beta",
                "created_at": "2026-09-05T00:00:00Z", "last_activity": "2026-09-05T00:00:00Z", "message_count": 0
            }]}),
        );
        let chat = switch.await.unwrap();
        assert_eq!(chat.current_session_id(), Some("sess-b"));
        assert!(chat.state_for_session("sess-a").unwrap().turn_in_flight);
        assert!(
            rx.try_recv().is_err(),
            "switching must not cancel/close the old session"
        );
    }

    // This test intentionally holds the process-global keymap test guard while
    // async dispatch runs so override-mutating tests cannot race it.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn rtg_9739_composer_enter_and_modifier_enter_dispatch_without_approval() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let _guard = crate::keymap::overrides::TEST_GUARD
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        crate::keymap::overrides::reset();

        for kind in [PaneKind::Chat, PaneKind::Acp] {
            let mut cases = vec![
                (KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), "submit"),
                (
                    KeyEvent::new(
                        KeyCode::Enter,
                        crate::keymap::Chord::with_primary(KeyCode::Enter, KeyModifiers::NONE)
                            .effective_modifiers(),
                    ),
                    "inject",
                ),
            ];
            if cfg!(target_os = "macos") {
                cases.push((
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
                    "control inject",
                ));
            }

            for (key, prompt) in cases {
                let (tx, mut rx) = mpsc::channel::<String>(16);
                let outbound = Arc::new(RpcOutbound::new(tx));
                let client = Arc::new(RpcClient::with_rpc(Arc::clone(&outbound)));
                let mut chat = Chat::new(client, kind);
                let mut active = state();
                active.input_bar.insert_text(prompt);
                chat.phase = ChatPhase::Active(Box::new(active));
                let mut term: crate::config_manager::Term = ratatui::Terminal::with_options(
                    crate::terminal_backend::WideCellCleanupBackend::new(std::io::stdout()),
                    ratatui::TerminalOptions {
                        viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, 100, 30)),
                    },
                )
                .unwrap();

                chat.handle_key(key, &mut term).await;

                let request =
                    next_rpc_request(&mut rx, "composer key must dispatch a prompt").await;
                assert_eq!(request["method"], method::SESSION_PROMPT);
                assert_eq!(request["params"]["prompt"], prompt);
                assert!(active_state(&mut chat).input_bar.input().is_empty());
            }
        }
    }

    #[tokio::test]
    async fn rtg_9739_approval_enter_approves_without_submitting_composer() {
        use crossterm::event::KeyCode;

        let (tx, mut rx) = mpsc::channel::<String>(16);
        let outbound = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&outbound)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        let mut active = state();
        active.input_bar.insert_text("keep draft");
        active.pending_approval = Some(PendingApproval {
            request_id: "approval-enter".to_string(),
            tool_name: "shell".to_string(),
            arguments_summary: "pwd".to_string(),
            timeout_secs: 30,
        });
        chat.phase = ChatPhase::Active(Box::new(active));
        let approve = tokio::spawn(async move {
            let mut term: crate::config_manager::Term = ratatui::Terminal::with_options(
                crate::terminal_backend::WideCellCleanupBackend::new(std::io::stdout()),
                ratatui::TerminalOptions {
                    viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, 100, 30)),
                },
            )
            .unwrap();
            chat.handle_key(KeyEvent::from(KeyCode::Enter), &mut term)
                .await;
            chat
        });
        let request = next_rpc_request(&mut rx, "approval Enter must answer the modal").await;
        assert_eq!(request["method"], method::SESSION_APPROVE);
        assert_eq!(request["params"]["request_id"], "approval-enter");
        respond_ok(&outbound, &request, serde_json::json!({}));
        let mut chat = approve.await.unwrap();

        assert_eq!(active_state(&mut chat).input_bar.input(), "keep draft");
        assert!(active_state(&mut chat).pending_approval.is_none());
        assert!(
            rx.try_recv().is_err(),
            "approval Enter must not submit a prompt"
        );
    }

    #[tokio::test]
    async fn input_bar_click_clears_transcript_mouse_highlight() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.mouse_down_entry = Some(0);
        state.mark_dirty_full();

        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut state, area, PaneKind::Chat);
            })
            .expect("draw chat");

        state.transcript_selection = Some(TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 1, row: 0 },
            dragged: true,
        });

        state.dirty = LinesDirty::Clean;
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: area.height.saturating_sub(2),
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(click, area).await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.transcript_selection, None);
        assert_eq!(state.mouse_down_entry, None);
        assert_eq!(
            state.dirty,
            LinesDirty::Clean,
            "clearing an overlay-only selection must preserve cached transcript lines"
        );
    }

    #[tokio::test]
    async fn model_picker_blocks_attachment_remove_click() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut active = state();
        active.input_bar.add_attachment(PendingAttachment {
            path: std::path::PathBuf::from("one.png"),
            mime_type: "image/png".into(),
            filename: "one.png".into(),
            size_bytes: 1,
            source: crate::attachment::AttachmentSource::File,
        });
        active.model_picker =
            ModelPickerOverlay::Model(crate::widgets::PickerState::new(vec!["a".into()], None));

        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, &mut active, area, PaneKind::Chat))
            .expect("draw chat");
        let attachment_area = active
            .input_bar
            .attachment_area()
            .expect("attachment row rendered");
        chat.phase = ChatPhase::Active(Box::new(active));

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: attachment_area.x + attachment_area.width - 2,
                row: attachment_area.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
        .await;

        let ChatPhase::Active(active) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(active.input_bar.pending_attachments().len(), 1);
    }

    #[tokio::test]
    async fn blank_side_click_clears_transcript_mouse_highlight() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hi")));
        state.mouse_down_entry = Some(0);
        state.mark_dirty_full();

        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut state, area, PaneKind::Chat);
            })
            .expect("draw chat");

        state.transcript_selection = Some(TranscriptSelection {
            anchor: CellPoint { column: 0, row: 0 },
            head: CellPoint { column: 1, row: 0 },
            dragged: true,
        });

        // The rendered entry rect must hug the text, not span the panel, so
        // there is blank space beside the short message to click in.
        let (_, rect) = state
            .entry_rects
            .iter()
            .find(|(idx, _)| *idx == 0)
            .copied()
            .expect("entry 0 has a screen rect");
        assert!(
            rect.width < area.width - 2,
            "short message rect must not span the full panel width: {rect:?}"
        );
        // A column just past the text but well within the panel — the blank
        // margin beside the message.
        let blank_col = rect.x + rect.width + 1;
        let blank_row = rect.y;
        assert!(
            blank_col < area.width - 1,
            "blank column stays in the panel"
        );

        state.dirty = LinesDirty::Clean;
        chat.phase = ChatPhase::Active(Box::new(state));

        let mouse_down = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: blank_col,
            row: blank_row,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(mouse_down, area).await;
        let mouse_up = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: blank_col,
            row: blank_row,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(mouse_up, area).await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.transcript_selection, None);
        assert_eq!(state.mouse_down_entry, None);
        assert_eq!(
            state.dirty,
            LinesDirty::Clean,
            "clearing an overlay-only selection must preserve cached transcript lines"
        );
    }

    #[tokio::test]
    async fn plain_message_copy_action_stays_in_browse_mode() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.mark_dirty_full();

        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut state, area, PaneKind::Chat);
            })
            .expect("draw chat");

        let entry_rect = state
            .entry_rects
            .first()
            .expect("entry region should be rendered")
            .1;
        let rows_before = state.cached_total_rows;
        state.dirty = LinesDirty::Clean;
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: entry_rect.x + 1,
            row: entry_rect.y,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(click, area).await;

        let ChatPhase::Active(state) = &mut chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(
            state.browse_cursor, None,
            "normal-mode click must not enter browse mode"
        );
        assert!(
            state.info_message.is_none(),
            "normal-mode click must not copy or show copied feedback"
        );
        assert!(
            state.copy_hit_regions.is_empty(),
            "normal-mode click must not reveal a copy action"
        );

        state.enter_browse_mode();

        terminal
            .draw(|frame| {
                render(frame, state, area, PaneKind::Chat);
            })
            .expect("redraw browse-mode chat");
        let selected_entry_rect = state
            .entry_rects
            .first()
            .expect("selected entry region should still be rendered")
            .1;
        assert_eq!(
            state.cached_total_rows, rows_before,
            "message copy affordance must overlay the transcript without adding rows"
        );
        assert_eq!(
            selected_entry_rect.y, entry_rect.y,
            "revealing message copy must not push earlier transcript rows"
        );

        let copy_rect = state
            .copy_hit_regions
            .iter()
            .find(|region| region.text.as_ref() == "hello")
            .expect("browse-mode selected message copy action should be rendered")
            .rect;
        assert_eq!(
            copy_rect.y, selected_entry_rect.y,
            "message copy action should overlay the selected row"
        );
        let body_x = area.x + 1;
        let body_width = area.width.saturating_sub(2);
        let expected_x = body_x + body_width.saturating_sub(copy_rect.width) / 2;
        assert_eq!(
            copy_rect.x, expected_x,
            "message copy action should be horizontally centered in the chat body"
        );
        let copy_click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: copy_rect.x,
            row: copy_rect.y,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(copy_click, area).await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(
            state.info_message.as_ref().map(|m| m.text.as_str()),
            Some(crate::i18n::t("zc-chat-copied-clipboard").as_str()),
            "explicit message copy action should copy"
        );
        assert_eq!(
            state.browse_cursor, None,
            "copy action should dismiss selection"
        );
        assert!(
            matches!(
                state.copy_feedback,
                Some(CopyFeedback {
                    target: CopyFeedbackTarget::Overlay(_),
                    ..
                })
            ),
            "message copy should leave a transient copied-state cue"
        );
    }

    #[tokio::test]
    async fn control_click_uses_platform_secondary_click_behavior() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state.entries.push(ChatEntry::AgentMessage(Arc::<str>::from(
            "select just this word",
        )));
        state.mark_dirty_full();

        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut state, area, PaneKind::Chat);
            })
            .expect("draw chat");
        let entry_rect = state
            .entry_rects
            .first()
            .expect("entry region should be rendered")
            .1;
        state.dirty = LinesDirty::Clean;
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: entry_rect.x + 1,
            row: entry_rect.y,
            modifiers: KeyModifiers::CONTROL,
        };
        chat.handle_mouse(click, area).await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert!(
            state.info_message.is_none(),
            "modifier-click outside browse mode must not app-copy the whole message"
        );
        assert_eq!(
            state.browse_cursor, None,
            "modifier-click outside browse mode should not select the whole message"
        );
        assert_eq!(
            state.context_menu.is_some(),
            cfg!(target_os = "macos"),
            "Control+click should open the context menu only on macOS"
        );
    }

    #[tokio::test]
    async fn right_click_context_menu_activation_runs_through_mouse_boundary() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.mark_dirty_full();
        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, &mut state, area, PaneKind::Chat))
            .expect("draw chat");
        let entry_rect = state.entry_rects[0].1;
        chat.phase = ChatPhase::Active(Box::new(state));

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: entry_rect.x + 1,
                row: entry_rect.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
        .await;

        let menu_action = {
            let ChatPhase::Active(state) = &chat.phase else {
                panic!("expected active chat");
            };
            let menu = state.context_menu.as_ref().expect("menu opens");
            assert!(state.info_message.is_none());
            (menu.rect.x + 1, menu.rect.y + 1)
        };

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: menu_action.0,
                row: menu_action.1,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
        .await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert!(state.context_menu.is_none());
        assert!(state.info_message.is_some());
        assert!(matches!(
            state.copy_feedback,
            Some(CopyFeedback {
                target: CopyFeedbackTarget::Overlay(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn outside_click_dismisses_context_menu_without_copying() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("hello")));
        state.mark_dirty_full();
        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, &mut state, area, PaneKind::Chat))
            .expect("draw chat");
        let entry_rect = state.entry_rects[0].1;
        chat.phase = ChatPhase::Active(Box::new(state));

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: entry_rect.x + 1,
                row: entry_rect.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
        .await;
        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert!(state.context_menu.is_some());
        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
        .await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert!(state.context_menu.is_none());
        assert!(state.info_message.is_none());
        assert!(state.copy_feedback.is_none());
    }

    #[tokio::test]
    async fn mouse_up_after_browse_drag_does_not_copy_selection() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("first")));
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("second")));
        state.browse_cursor = Some(1);
        state.browse_anchor = Some(0);
        state.mouse_down_entry = Some(0);
        state.mark_dirty_full();
        chat.phase = ChatPhase::Active(Box::new(state));

        let up = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(up, Rect::new(0, 0, 80, 20)).await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.mouse_down_entry, None);
        assert!(
            state.info_message.is_none(),
            "ending a mouse drag must not app-copy the selected messages"
        );
    }

    #[tokio::test]
    async fn code_copy_shows_shared_feedback() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from("previous")));
        state.entries.push(ChatEntry::AgentMessage(Arc::<str>::from(
            "```bash\necho hello\n```",
        )));
        state.mark_dirty_full();
        assert!(!state.in_browse_mode());

        let area = Rect::new(0, 0, 80, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut state, area, PaneKind::Chat);
            })
            .expect("draw chat");

        let code_regions: Vec<CopyHitRegion> = state
            .copy_hit_regions
            .iter()
            .filter(|region| region.text.as_ref() == "echo hello")
            .cloned()
            .collect();
        assert_eq!(
            code_regions.len(),
            2,
            "top and bottom fence labels should both be copy targets"
        );
        assert_eq!(
            code_regions[0].group, code_regions[1].group,
            "top and bottom copy targets for one fence should share feedback"
        );
        let copy_rect = code_regions[0].rect;
        let copy_group = code_regions[0].group;
        state.dirty = LinesDirty::Clean;
        chat.phase = ChatPhase::Active(Box::new(state));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: copy_rect.x,
            row: copy_rect.y,
            modifiers: KeyModifiers::NONE,
        };
        chat.handle_mouse(click, area).await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.mouse_down_entry, None);
        assert_eq!(
            state.info_message.as_ref().map(|m| m.text.as_str()),
            Some(crate::i18n::t("zc-chat-copied-clipboard").as_str())
        );
        assert!(matches!(
            state.copy_feedback,
            Some(CopyFeedback {
                target: CopyFeedbackTarget::Code(group),
                ..
            }) if group == copy_group
        ));
    }

    #[tokio::test]
    async fn browse_mode_code_copy_clears_selection() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        state.entries.push(ChatEntry::AgentMessage(Arc::<str>::from(
            "```sh\necho hi\n```",
        )));
        state.browse_cursor = Some(0);
        state.copy_hit_regions.push(CopyHitRegion {
            rect: Rect::new(2, 2, 6, 1),
            text: Arc::<str>::from("echo hi"),
            kind: CopyHitKind::Code,
            group: 0,
        });
        chat.phase = ChatPhase::Active(Box::new(state));

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 2,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 80, 20),
        )
        .await;

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(state.browse_cursor, None);
        assert!(state.info_message.is_some());
        assert!(matches!(
            state.copy_feedback,
            Some(CopyFeedback {
                target: CopyFeedbackTarget::Code(0),
                ..
            })
        ));
    }

    fn authoritative_rows(s: &ChatState, width: u16) -> u16 {
        Paragraph::new(s.cached_lines.iter().map(borrow_line).collect::<Vec<_>>())
            .wrap(Wrap { trim: false })
            .line_count(width) as u16
    }

    #[test]
    fn copy_cached_total_rows_and_breaks_match_full_line_count() {
        let width: u16 = 40;
        let mut s = state();

        for i in 0..50 {
            s.push_user_message(Some(format!("message number {i} with enough text to wrap across the forty column width budget")), Vec::new());
        }
        s.rebuild_lines(width);
        assert_eq!(
            s.cached_total_rows,
            authoritative_rows(&s, width),
            "full-rebuild row total must match line_count"
        );
        assert_eq!(
            s.cached_row_breaks.len(),
            usize::from(s.cached_total_rows),
            "full rebuild must cache one separator per rendered row"
        );

        for i in 50..60 {
            s.push_user_message(
                Some(format!(
                    "appended message {i} also long enough to wrap somewhere in the middle of a row"
                )),
                Vec::new(),
            );
        }
        s.rebuild_lines(width);
        assert_eq!(
            s.cached_total_rows,
            authoritative_rows(&s, width),
            "incremental-append row total must match line_count"
        );
        assert_eq!(
            s.cached_row_breaks.len(),
            usize::from(s.cached_total_rows),
            "incremental append must preserve separator alignment"
        );

        let narrower: u16 = 20;
        s.rebuild_lines(narrower);
        assert_eq!(
            s.cached_total_rows,
            authoritative_rows(&s, narrower),
            "width change must force a recompute that still matches line_count"
        );
        assert_eq!(
            s.cached_row_breaks.len(),
            usize::from(s.cached_total_rows),
            "width rebuild must realign cached separators"
        );
    }

    #[tokio::test]
    async fn chat_entry_refresh_reloads_agents_from_error_phase() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("No enabled agents yet.".to_string());

        let refresh = tokio::spawn(async move {
            chat.refresh_if_inactive().await;
            chat
        });

        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("refresh should request the agent list")
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], method::AGENTS_STATUS);

        let id = request["id"].as_str().unwrap().to_string();
        rpc.dispatch_response(
            &id,
            Some(serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0},
                    {"alias": "beta", "enabled": true, "live_sessions": 0, "persisted_sessions": 0}
                ]
            })),
            None,
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), refresh)
            .await
            .expect("refresh should finish after agents/status response")
            .unwrap();
        let ChatPhase::PickAgent {
            agents, loading, ..
        } = chat.phase
        else {
            panic!("refresh should leave stale error state");
        };
        assert_eq!(agents, vec!["alpha".to_string(), "beta".to_string()]);
        assert!(!loading);
    }

    #[tokio::test]
    async fn chat_entry_refresh_reloads_agents_from_pick_phase() {
        // Re-entering the pane while parked on the picker must re-fetch the
        // agent list so an agent created elsewhere (Quickstart / Config) shows
        // up — and the existing highlight must survive the refresh. Regression
        // for "new agent missing from Code/Chat tab when agents already exist".
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut list_state = ListState::default();
        list_state.select(Some(1)); // user has "beta" highlighted
        chat.phase = ChatPhase::PickAgent {
            agents: vec!["alpha".to_string(), "beta".to_string()],
            list_state,
            loading: false,
        };

        let refresh = tokio::spawn(async move {
            chat.refresh_if_inactive().await;
            chat
        });

        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("refresh should request the agent list")
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], method::AGENTS_STATUS);

        let id = request["id"].as_str().unwrap().to_string();
        rpc.dispatch_response(
            &id,
            Some(serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0},
                    {"alias": "beta", "enabled": true, "live_sessions": 0},
                    {"alias": "gamma", "enabled": true, "live_sessions": 0}
                ]
            })),
            None,
        );

        let chat = tokio::time::timeout(Duration::from_secs(2), refresh)
            .await
            .expect("refresh should finish after agents/status response")
            .unwrap();
        let ChatPhase::PickAgent {
            agents, list_state, ..
        } = chat.phase
        else {
            panic!("refresh should keep the agent picker");
        };
        // The newly-created agent is now present...
        assert_eq!(
            agents,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
        // ...and the prior highlight ("beta", row 1) is preserved.
        assert_eq!(list_state.selected(), Some(1));
    }

    #[test]
    fn entry_retry_phase_claims_exclusive_boundary_ownership() {
        let phase = AtomicU8::new(ENTRY_RETRY_PRE_SESSION);
        assert!(claim_entry_retry_session_creation(&phase));
        assert!(!claim_entry_retry_cancellation(&phase));
        assert_eq!(
            phase.load(Ordering::Acquire),
            ENTRY_RETRY_SESSION_CREATION,
            "worker ownership must prevent cancellation from aborting session/new"
        );

        let phase = AtomicU8::new(ENTRY_RETRY_PRE_SESSION);
        assert!(claim_entry_retry_cancellation(&phase));
        assert!(!claim_entry_retry_session_creation(&phase));
        assert_eq!(
            phase.load(Ordering::Acquire),
            ENTRY_RETRY_CANCELLED,
            "cancellation ownership must prevent session/new from starting"
        );
    }

    #[tokio::test]
    async fn chat_entry_retry_starts_once_until_result_is_drained() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("try again".to_string());

        chat.start_entry_retry();
        chat.start_entry_retry();

        let request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "repeated Chat entry must not start a second request"
        );
    }

    #[tokio::test]
    async fn dropping_chat_aborts_unresolved_pre_session_entry_retry() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("try again".to_string());

        chat.start_entry_retry();
        let _request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        assert_eq!(rpc.pending_count(), 1);

        drop(chat);
        for _ in 0..16 {
            if rpc.pending_count() == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            rpc.pending_count(),
            0,
            "dropping Chat must drop an unresolved pre-session retry RPC"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "cancelling pre-session retry must not continue into session creation"
        );
    }

    #[tokio::test]
    async fn chat_entry_retry_applies_picker_and_preserves_selection() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut list_state = ListState::default();
        list_state.select(Some(1));
        chat.phase = ChatPhase::PickAgent {
            agents: vec!["alpha".to_string(), "beta".to_string()],
            list_state,
            loading: false,
        };

        chat.start_entry_retry();
        let request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true},
                    {"alias": "beta", "enabled": true},
                    {"alias": "gamma", "enabled": true}
                ]
            }),
        );

        for _ in 0..32 {
            chat.drain_entry_retry_results();
            if matches!(
                &chat.phase,
                ChatPhase::PickAgent {
                    agents,
                    loading: false,
                    list_state,
                } if agents.len() == 3 && list_state.selected() == Some(1)
            ) {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("Chat entry retry result was not applied");
    }

    #[tokio::test]
    async fn chat_entry_retry_replaces_picker_when_no_enabled_agents_remain() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut list_state = ListState::default();
        list_state.select(Some(1));
        chat.phase = ChatPhase::PickAgent {
            agents: vec!["alpha".to_string(), "beta".to_string()],
            list_state,
            loading: false,
        };

        chat.start_entry_retry();
        let request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": false},
                    {"alias": "beta", "enabled": false}
                ]
            }),
        );

        for _ in 0..32 {
            chat.drain_entry_retry_results();
            if matches!(
                &chat.phase,
                ChatPhase::Error(message) if message == &crate::i18n::t("zc-chat-no-agents")
            ) {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("authoritative empty-agent result did not replace the stale picker");
    }

    #[tokio::test]
    async fn chat_entry_retry_reattaches_focused_and_background_resumes() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("retry reconnect".to_string());
        chat.set_resume_sessions(vec![
            resume_entry("sess-f", "beta", true),
            resume_entry("sess-bg", "alpha", false),
        ]);

        chat.start_entry_retry();
        let request = next_rpc_request(&mut rx, "retry requests enabled agents").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true},
                    {"alias": "beta", "enabled": true}
                ]
            }),
        );

        let request = next_rpc_request(&mut rx, "retry resumes the focused session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["session_id"], "sess-f");
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({"session_id": "sess-f", "workspace_dir": "/w"}),
        );
        let request = next_rpc_request(&mut rx, "retry refreshes focused identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));
        let request = next_rpc_request(&mut rx, "retry reloads focused history").await;
        assert_eq!(request["method"], method::SESSION_MESSAGES);
        respond_ok(&rpc, &request, serde_json::json!({"messages": []}));

        let request = next_rpc_request(&mut rx, "retry resumes the background session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["session_id"], "sess-bg");
        assert_eq!(request["params"]["keep_siblings"], true);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({"session_id": "sess-bg", "workspace_dir": "/w"}),
        );
        let request = next_rpc_request(&mut rx, "retry refreshes background identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));
        let request = next_rpc_request(&mut rx, "retry reloads background history").await;
        assert_eq!(request["method"], method::SESSION_MESSAGES);
        respond_ok(&rpc, &request, serde_json::json!({"messages": []}));

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                chat.drain_entry_retry_results();
                // Sidebar summaries also include sessions still waiting to resume.
                if matches!(chat.phase, ChatPhase::Active(_)) && chat.background.len() == 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the complete retry pane must be adopted");

        assert_eq!(
            chat.session_summaries()
                .into_iter()
                .map(|summary| (summary.session_id, summary.focused))
                .collect::<Vec<_>>(),
            vec![("sess-f".to_string(), true), ("sess-bg".to_string(), false)]
        );
        assert!(chat.resume_backgrounds.is_empty());
    }

    #[tokio::test]
    async fn chat_entry_retry_keeps_empty_receiver_after_worker_finishes() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(Arc::clone(&client), PaneKind::Chat);
        chat.phase = ChatPhase::Error("stale error".to_string());

        let (result_tx, result_rx) = oneshot::channel();
        let worker = tokio::spawn(async {});
        while !worker.is_finished() {
            tokio::task::yield_now().await;
        }
        chat.entry_retry_attempt = Some(EntryRetryAttempt {
            result_rx,
            worker,
            cancelled: Arc::new(AtomicBool::new(false)),
            phase: Arc::new(AtomicU8::new(ENTRY_RETRY_PRE_SESSION)),
            rpc: Arc::clone(&client),
        });

        chat.drain_entry_retry_results();
        assert!(
            chat.entry_retry_attempt.is_some(),
            "worker completion cannot prove an empty receiver is terminal"
        );

        assert!(
            result_tx
                .send(ChatEntryRetryResult {
                    chat: Box::new({
                        let mut result = Chat::new(client, PaneKind::Chat);
                        result.phase = ChatPhase::Error("authoritative result".to_string());
                        result
                    }),
                    init_outcome: ChatInitOutcome::NoEnabledAgents,
                })
                .is_ok(),
            "retained receiver should accept the completed result"
        );
        chat.drain_entry_retry_results();

        assert!(chat.entry_retry_attempt.is_none());
        assert!(matches!(
            chat.phase,
            ChatPhase::Error(ref message) if message == "authoritative result"
        ));
    }

    #[tokio::test]
    async fn completed_entry_retry_blur_preserves_queue_until_next_adoption() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("retry reconnect".into());
        let mut queued = state_for("sess-retained", "alpha");
        queued
            .enqueue_message("send once".into(), Vec::new())
            .unwrap();
        let mut entry = resume_entry("sess-retained", "alpha", true);
        entry.queue = queued.reconnect_queue_state();
        chat.set_resume_sessions(vec![entry]);

        for abandon in [true, false] {
            chat.start_entry_retry();
            let request = next_rpc_request(&mut rx, "retry lists agents").await;
            assert_eq!(request["method"], method::AGENTS_STATUS);
            respond_ok(
                &rpc,
                &request,
                serde_json::json!({
                    "agents": [{"alias": "alpha", "enabled": true}]
                }),
            );
            let request = next_rpc_request(&mut rx, "retry resumes retained ID").await;
            assert_eq!(request["method"], method::SESSION_NEW);
            assert_eq!(request["params"]["session_id"], "sess-retained");
            respond_ok(
                &rpc,
                &request,
                serde_json::json!({
                    "session_id": "sess-retained", "workspace_dir": "/w"
                }),
            );
            let request = next_rpc_request(&mut rx, "retry refreshes identity").await;
            assert_eq!(request["method"], method::CONFIG_LIST);
            respond_ok(&rpc, &request, serde_json::json!([]));
            let request = next_rpc_request(&mut rx, "retry reloads history").await;
            assert_eq!(request["method"], method::SESSION_MESSAGES);
            respond_ok(&rpc, &request, serde_json::json!({"messages": []}));

            tokio::time::timeout(Duration::from_secs(2), async {
                while !chat
                    .entry_retry_attempt
                    .as_ref()
                    .unwrap()
                    .worker
                    .is_finished()
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("retry result should be ready without adoption");
            assert!(
                tokio::time::timeout(Duration::from_millis(50), rx.recv())
                    .await
                    .is_err(),
                "a completed but unadopted retry must not send the borrowed queue"
            );
            if abandon {
                chat.on_pane_blur();
                assert_eq!(
                    chat.resume_focused.as_ref().unwrap().queue.messages.len(),
                    1
                );
            } else {
                chat.drain_entry_retry_results();
            }
        }

        let prompt = next_rpc_request(&mut rx, "adopted pane sends the queue once").await;
        assert_eq!(prompt["method"], method::SESSION_PROMPT);
        assert_eq!(prompt["params"]["session_id"], "sess-retained");
        assert_eq!(prompt["params"]["prompt"], "send once");
        respond_ok(&rpc, &prompt, serde_json::Value::Null);
        assert_eq!(
            chat.state_for_session("sess-retained").unwrap().queue_len(),
            0
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "blur and retry must not duplicate the prompt or close the retained session"
        );
    }

    #[tokio::test]
    async fn entry_retry_defers_recovery_until_adoption() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut worker = Chat::new(Arc::clone(&client), PaneKind::Chat);
        worker.entry_retry_preparing = true;
        let mut queued = state();
        queued
            .enqueue_message("after recovery".into(), Vec::new())
            .unwrap();
        worker.phase = ChatPhase::Active(Box::new(queued));
        worker.begin_session_resync("sess-1".into());
        worker.pump_all_queues();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err()
        );

        let mut pane = Chat::new(client, PaneKind::Chat);
        pane.phase = ChatPhase::Error("retry".into());
        let (result_tx, result_rx) = oneshot::channel();
        assert!(
            result_tx
                .send(ChatEntryRetryResult {
                    chat: Box::new(worker),
                    init_outcome: ChatInitOutcome::Other,
                })
                .is_ok()
        );
        pane.entry_retry_attempt = Some(EntryRetryAttempt {
            result_rx,
            worker: tokio::spawn(async {}),
            cancelled: Arc::new(AtomicBool::new(false)),
            phase: Arc::new(AtomicU8::new(ENTRY_RETRY_SESSION_CREATION)),
            rpc: Arc::clone(&pane.rpc),
        });
        pane.drain_entry_retry_results();
        assert!(!pane.entry_retry_preparing);
        assert!(pane.session_resync_in_flight.contains("sess-1"));
        assert_eq!(pane.state_for_session("sess-1").unwrap().queue_len(), 1);
        let request = next_rpc_request(&mut rx, "adoption starts deferred recovery").await;
        assert_eq!(request["method"], method::SESSION_CANCEL);
    }

    #[tokio::test]
    async fn cancelling_entry_retry_during_session_creation_closes_new_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("try again".to_string());

        chat.start_entry_retry();
        let request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [{"alias": "alpha", "enabled": true}]
            }),
        );
        let request = next_rpc_request(&mut rx, "entry retry should create a session").await;
        assert_eq!(request["method"], method::SESSION_NEW);

        chat.cancel_entry_retry();
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "session_id": "sess-stale",
                "workspace_dir": "/tmp/stale"
            }),
        );
        let close = next_rpc_request(&mut rx, "cancelled retry should close its session").await;
        assert_eq!(close["method"], method::SESSION_CLOSE);
        assert_eq!(close["params"]["session_id"], "sess-stale");
        respond_err(
            &rpc,
            &close,
            crate::jsonrpc::error_codes::INTERNAL_ERROR,
            "close unavailable",
        );

        let retry = next_rpc_request(&mut rx, "failed stale close should be retried").await;
        assert_eq!(retry["method"], method::SESSION_CLOSE);
        assert_eq!(retry["params"]["session_id"], "sess-stale");
        respond_ok(&rpc, &retry, serde_json::json!({}));

        for _ in 0..16 {
            if rpc.pending_count() == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(rpc.pending_count(), 0);
        assert!(matches!(chat.phase, ChatPhase::Error(_)));
    }

    #[tokio::test]
    async fn cancelling_resumed_entry_retry_does_not_close_retained_session() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("try resume again".to_string());
        chat.set_resume_sessions(vec![resume_entry("sess-retained", "alpha", true)]);

        chat.start_entry_retry();
        let request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [{"alias": "alpha", "enabled": true}]
            }),
        );
        let request = next_rpc_request(&mut rx, "entry retry should resume the session").await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["session_id"], "sess-retained");

        chat.cancel_entry_retry();
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "session_id": "sess-retained",
                "workspace_dir": "/tmp/retained"
            }),
        );

        for _ in 0..16 {
            if rpc.pending_count() == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(rpc.pending_count(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "cancelling a resume must not close the retained session"
        );
        assert!(matches!(chat.phase, ChatPhase::Error(_)));
    }

    #[tokio::test]
    async fn stale_session_close_stops_after_attempt_limit() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));

        let close_task = tokio::spawn(async move {
            close_stale_session(&client, "sess-stale").await;
        });

        for _ in 0..STALE_SESSION_CLOSE_ATTEMPTS {
            let request = next_rpc_request(&mut rx, "stale close should be attempted").await;
            assert_eq!(request["method"], method::SESSION_CLOSE);
            assert_eq!(request["params"]["session_id"], "sess-stale");
            respond_err(
                &rpc,
                &request,
                crate::jsonrpc::error_codes::INTERNAL_ERROR,
                "close unavailable",
            );
        }

        close_task.await.expect("stale close task should finish");
        assert!(
            rx.try_recv().is_err(),
            "cleanup must stop after the configured attempt limit"
        );
        assert_eq!(rpc.pending_count(), 0);
    }

    #[tokio::test]
    async fn failed_resumed_entry_retry_preserves_live_resume_state() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("resume failed".to_string());
        chat.set_resume_sessions(vec![resume_entry("sess-resume", "alpha", true)]);

        chat.start_entry_retry();
        let request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [{"alias": "alpha", "enabled": true}]
            }),
        );
        let request = next_rpc_request(&mut rx, "entry retry should resume a session").await;
        assert_eq!(request["params"]["session_id"], "sess-resume");
        respond_err(&rpc, &request, -32000, "resume rejected");

        for _ in 0..32 {
            chat.drain_entry_retry_results();
            if chat.entry_retry_attempt.is_none() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(matches!(chat.phase, ChatPhase::Error(_)));
        assert_eq!(
            chat.resume_focused
                .as_ref()
                .map(|entry| entry.session_id.as_str()),
            Some("sess-resume")
        );
        assert_eq!(
            chat.resume_focused
                .as_ref()
                .map(|entry| entry.agent_alias.as_str()),
            Some("alpha")
        );
    }

    #[tokio::test]
    async fn agents_status_failure_keeps_picker_selection() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut list_state = ListState::default();
        list_state.select(Some(1));
        chat.phase = ChatPhase::PickAgent {
            agents: vec!["alpha".to_string(), "beta".to_string()],
            list_state,
            loading: false,
        };

        chat.start_entry_retry();
        let request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        respond_err(&rpc, &request, -32000, "agents unavailable");

        for _ in 0..32 {
            chat.drain_entry_retry_results();
            if chat.entry_retry_attempt.is_none() {
                break;
            }
            tokio::task::yield_now().await;
        }
        let ChatPhase::PickAgent {
            agents, list_state, ..
        } = &chat.phase
        else {
            panic!("agent failure should keep the picker visible");
        };
        assert_eq!(agents, &["alpha".to_string(), "beta".to_string()]);
        assert_eq!(list_state.selected(), Some(1));
    }

    #[tokio::test]
    async fn active_chat_wins_against_queued_entry_retry_result() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::Error("try again".to_string());

        chat.start_entry_retry();
        let request = next_rpc_request(&mut rx, "entry retry should request agents").await;
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [{"alias": "alpha", "enabled": true}]
            }),
        );
        let request = next_rpc_request(&mut rx, "entry retry should create a session").await;
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "session_id": "sess-stale",
                "workspace_dir": "/tmp/stale"
            }),
        );
        let request = next_rpc_request(&mut rx, "entry retry should refresh model identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        chat.phase = ChatPhase::Active(Box::new(state()));
        respond_ok(&rpc, &request, serde_json::json!([]));

        let mut close = None;
        for _ in 0..32 {
            chat.drain_entry_retry_results();
            if let Ok(request) = rx.try_recv() {
                close = Some(serde_json::from_str::<serde_json::Value>(&request).unwrap());
                break;
            }
            tokio::task::yield_now().await;
        }
        let close = close.expect("stale Active retry result should be closed");
        assert_eq!(close["method"], method::SESSION_CLOSE);
        assert_eq!(close["params"]["session_id"], "sess-stale");
        respond_ok(&rpc, &close, serde_json::json!({}));
        assert!(matches!(chat.phase, ChatPhase::Active(_)));
    }

    #[tokio::test]
    async fn apply_update_during_turn_in_flight() {
        let mut s = state();
        s.turn_in_flight = true;
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "streaming...".to_string(),
        });
        assert_eq!(s.current_agent_text(), "streaming...");
    }

    #[test]
    fn input_append_and_clear() {
        let mut s = state();
        s.input_bar.push_input_char('h');
        s.input_bar.push_input_char('i');
        assert_eq!(s.input_bar.input(), "hi");
        let taken = s.input_bar.take_input();
        assert_eq!(taken, "hi");
        assert_eq!(s.input_bar.input(), "");
    }

    #[test]
    fn text_chunk_accumulates() {
        let mut s = state();
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "Hello".to_string(),
        });
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: " world".to_string(),
        });
        assert_eq!(s.current_agent_text(), "Hello world");
    }

    #[test]
    fn history_trimmed_update_adds_visible_system_notice() {
        let mut s = state();
        s.apply_update(SessionUpdate::HistoryTrimmed {
            session_id: "sess-1".to_string(),
            dropped_messages: 12,
            kept_turns: 3,
            reason: "history message limit exceeded".to_string(),
            token_budget: None,
            tokens_before: None,
            tokens_after: None,
            tokens_before_source: None,
            tokens_after_source: None,
            unsatisfiable_floor: None,
        });

        assert!(matches!(
            s.entries().last(),
            Some(ChatEntry::SystemMessage(text))
                if text.contains("history message limit exceeded")
                    && text.contains("12")
                    && text.contains("3")
        ));
    }

    #[test]
    fn history_trimmed_token_accounting_renders_in_notice() {
        let mut s = state();
        s.apply_update(SessionUpdate::HistoryTrimmed {
            session_id: "sess-1".to_string(),
            dropped_messages: 12,
            kept_turns: 33,
            reason: "context token budget exceeded".to_string(),
            token_budget: Some(500_000),
            tokens_before: Some(612_000),
            tokens_after: Some(117_000),
            tokens_before_source: Some("provider".to_string()),
            tokens_after_source: Some("calibrated".to_string()),
            unsatisfiable_floor: None,
        });

        assert!(matches!(
            s.entries().last(),
            Some(ChatEntry::SystemMessage(text))
                if text.contains("612000")
                    && text.contains("117000")
                    && text.contains("configured token budget: 500000")
                    && text.contains("context token budget exceeded")
                    && text.contains("12")
                    && text.contains("33")
                    && text.contains("provider")
                    && text.contains("estimate")
                    && text.contains("before")
                    && text.contains("after")
                    && !text.contains("against a")
        ));
    }

    #[test]
    fn history_trimmed_recovery_below_configured_budget_does_not_claim_budget_governed() {
        let mut s = state();
        s.apply_update(SessionUpdate::HistoryTrimmed {
            session_id: "sess-1".to_string(),
            dropped_messages: 4,
            kept_turns: 2,
            reason: "context window overflow recovery".to_string(),
            token_budget: Some(500_000),
            tokens_before: Some(612_000),
            tokens_after: Some(117_000),
            tokens_before_source: Some("provider".to_string()),
            tokens_after_source: Some("calibrated".to_string()),
            unsatisfiable_floor: None,
        });

        assert!(matches!(
            s.entries().last(),
            Some(ChatEntry::SystemMessage(text))
                if text.contains("612000")
                    && text.contains("117000")
                    && text.contains("context window overflow recovery")
                    && text.contains("configured token budget: 500000")
                    && !text.contains("against a")
        ));
    }

    #[test]
    fn history_trimmed_recovery_with_enforcement_disabled_renders_valid_counts() {
        let mut s = state();
        s.apply_update(SessionUpdate::HistoryTrimmed {
            session_id: "sess-1".to_string(),
            dropped_messages: 4,
            kept_turns: 2,
            reason: "context window overflow recovery".to_string(),
            token_budget: None,
            tokens_before: Some(612_000),
            tokens_after: Some(117_000),
            tokens_before_source: Some("provider".to_string()),
            tokens_after_source: Some("calibrated".to_string()),
            unsatisfiable_floor: None,
        });

        assert!(matches!(
            s.entries().last(),
            Some(ChatEntry::SystemMessage(text))
                if text.contains("612000")
                    && text.contains("117000")
                    && text.contains("context window overflow recovery")
                    && !text.contains("budget")
        ));
    }

    #[test]
    fn history_trimmed_untrimmable_floor_does_not_claim_history_changed() {
        // The unsatisfiable newest-turn/schema floor carries the explicit
        // `unsatisfiable_floor` flag while the projected `tokens_after`
        // still exceeds the configured budget. The notice must not claim
        // history was trimmed.
        let mut s = state();
        s.apply_update(SessionUpdate::HistoryTrimmed {
            session_id: "sess-1".to_string(),
            dropped_messages: 0,
            kept_turns: 1,
            reason: "context token budget exceeded".to_string(),
            token_budget: Some(100_000),
            tokens_before: Some(117_000),
            tokens_after: Some(117_000),
            tokens_before_source: Some("calibrated".to_string()),
            tokens_after_source: Some("calibrated".to_string()),
            unsatisfiable_floor: Some(true),
        });

        assert!(matches!(
            s.entries().last(),
            Some(ChatEntry::SystemMessage(text))
                if text.contains("could not be trimmed below the configured token budget")
                    && text.contains("117000")
                    && text.contains("configured budget: 100000")
                    && !text.contains("was trimmed:")
                    && !text.contains("messages dropped")
        ));
    }

    #[test]
    fn history_trimmed_floor_with_real_drops_reports_both_facts() {
        // A breadcrumb-induced floor after real turns were removed carries
        // BOTH the honest drop count and the unsatisfiable flag; the notice
        // must use the floor wording, not claim an ordinary successful trim.
        let mut s = state();
        s.apply_update(SessionUpdate::HistoryTrimmed {
            session_id: "sess-1".to_string(),
            dropped_messages: 2,
            kept_turns: 1,
            reason: "context token budget exceeded".to_string(),
            token_budget: Some(100_000),
            tokens_before: Some(200_000),
            tokens_after: Some(117_000),
            tokens_before_source: Some("provider".to_string()),
            tokens_after_source: Some("calibrated".to_string()),
            unsatisfiable_floor: Some(true),
        });

        assert!(matches!(
            s.entries().last(),
            Some(ChatEntry::SystemMessage(text))
                if text.contains("could not be trimmed below the configured token budget")
                    && text.contains("configured budget: 100000")
                    && !text.contains("Earlier conversation history was trimmed")
        ));
    }

    #[test]
    fn history_trimmed_without_flag_keeps_trimmed_wording_when_over_budget() {
        // Older daemons never emit the flag; their events must keep rendering
        // through the ordinary wording paths even when counts exceed the
        // budget, so the flag alone drives the floor discriminator.
        let mut s = state();
        s.apply_update(SessionUpdate::HistoryTrimmed {
            session_id: "sess-1".to_string(),
            dropped_messages: 1,
            kept_turns: 1,
            reason: "context token budget exceeded".to_string(),
            token_budget: Some(100_000),
            tokens_before: Some(200_000),
            tokens_after: Some(117_000),
            tokens_before_source: Some("provider".to_string()),
            tokens_after_source: Some("calibrated".to_string()),
            unsatisfiable_floor: None,
        });

        assert!(matches!(
            s.entries().last(),
            Some(ChatEntry::SystemMessage(text))
                if text.contains("Earlier conversation history was trimmed")
                    && !text.contains("could not be trimmed")
        ));
    }

    #[test]
    fn history_trimmed_estimated_sources_render_estimate_label() {
        let mut s = state();
        s.apply_update(SessionUpdate::HistoryTrimmed {
            session_id: "sess-1".to_string(),
            dropped_messages: 2,
            kept_turns: 1,
            reason: "context token budget exceeded".to_string(),
            token_budget: Some(10_000),
            tokens_before: Some(12_000),
            tokens_after: Some(6_000),
            tokens_before_source: Some("estimate".to_string()),
            tokens_after_source: Some("estimate".to_string()),
            unsatisfiable_floor: None,
        });

        assert!(matches!(
            s.entries().last(),
            Some(ChatEntry::SystemMessage(text))
                if text.contains("estimated")
                    && text.contains("estimated before")
                    && text.contains("estimated after")
        ));
    }

    #[test]
    fn tool_call_followed_by_result_is_one_entry() {
        let mut s = state();
        s.apply_update(SessionUpdate::ToolCall {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            name: "shell".to_string(),
            raw_input: serde_json::json!({"command":"ls"}),
        });
        s.apply_update(SessionUpdate::ToolResult {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            raw_output: "file.txt\n".to_string(),
        });
        let entries = s.entries();
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            &entries[0],
            ChatEntry::Tool {
                result: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn tool_result_retention_truncates_utf8_safely_and_keeps_marker() {
        let mut s = state();
        s.apply_update(SessionUpdate::ToolCall {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc-long".to_string(),
            name: "shell".to_string(),
            raw_input: serde_json::json!({"command": "long-output"}),
        });
        let raw_output = format!("{}éé", "a".repeat(16 * 1024 - 1));
        s.apply_update(SessionUpdate::ToolResult {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc-long".to_string(),
            raw_output,
        });

        let ChatEntry::Tool {
            result: Some(result),
            ..
        } = &s.entries()[0]
        else {
            panic!("expected retained tool result");
        };
        assert!(result.ends_with("…[truncated]"));
        assert!(result.is_char_boundary(result.len()));
    }

    fn rendered_text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn tool_entry_disclosure_shows_full_retained_content_only_when_expanded() {
        let input = format!(r#"{{"command":"{}"}}"#, "x".repeat(180));
        let result = format!(
            "first line\n{}\n\u{1b}]52;c;payload\u{7}\n…[truncated]",
            "y".repeat(240)
        );

        let mut collapsed = Vec::new();
        render_tool_entry(
            &mut collapsed,
            "shell",
            &input,
            Some(&result),
            false,
            ToolDisclosure::Collapsed,
        );
        let collapsed_text = rendered_text(&collapsed);
        assert!(collapsed_text.starts_with("▶ [tool: shell]"));
        assert!(collapsed_text.contains("input:"));
        assert!(collapsed_text.contains("result:"));
        assert!(!collapsed_text.contains(&input));
        assert!(!collapsed_text.contains("→"));

        let mut expanded = Vec::new();
        render_tool_entry(
            &mut expanded,
            "shell",
            &input,
            Some(&result),
            false,
            ToolDisclosure::Full,
        );
        let expanded_text = rendered_text(&expanded);
        assert!(expanded_text.starts_with("▼ [tool: shell]"));
        assert!(expanded_text.contains(&input));
        assert!(expanded_text.contains(&"y".repeat(240)));
        assert!(!expanded_text.contains('\u{1b}'));
        assert!(!expanded_text.contains('\u{7}'));
        assert!(expanded_text.contains("\\u{1b}]52;c;payload\\u{7}"));
        assert!(expanded_text.contains("…[truncated]"));
    }

    #[test]
    fn expanded_tool_display_is_bounded_while_copy_retains_full_content() {
        let input_tail = "input-tail-must-remain-copyable";
        let result_tail = "result-tail-must-remain-copyable";
        let input = format!("{}\n{input_tail}", "input line\n".repeat(500));
        let result = format!("{}\n{result_tail}", "result line\n".repeat(500));
        let mut lines = Vec::new();

        render_tool_entry(
            &mut lines,
            "shell",
            &input,
            Some(&result),
            false,
            ToolDisclosure::Full,
        );

        let text = rendered_text(&lines);
        assert!(lines.len() <= 2 * TOOL_EXPANDED_MAX_LINES + 2);
        assert!(text.contains("Display limited; copy for full content"));
        assert!(!text.contains(input_tail));
        assert!(!text.contains(result_tail));

        let entry = ChatEntry::Tool {
            tool_call_id: Arc::from("tc-oversized"),
            name: Arc::from("shell"),
            input_json: Arc::from(input),
            result: Some(Arc::from(result)),
        };
        let copied = clipboard_text(&entry);
        assert!(copied.contains(input_tail));
        assert!(copied.contains(result_tail));
    }

    #[test]
    fn file_tool_preview_is_six_lines_and_full_view_avoids_raw_content_duplication() {
        let edit_input = serde_json::json!({
            "path": "/tmp/example.rs",
            "old_string": "fn old() {}",
            "new_string": "fn new() {}",
        })
        .to_string();
        let mut collapsed_edit_lines = Vec::new();
        render_tool_entry(
            &mut collapsed_edit_lines,
            "file_edit",
            &edit_input,
            Some("done"),
            false,
            ToolDisclosure::Collapsed,
        );
        let collapsed_edit_text = rendered_text(&collapsed_edit_lines);
        assert!(collapsed_edit_text.starts_with("▶ [tool: file_edit]"));
        assert!(!collapsed_edit_text.contains("fn old() {}"));
        assert!(!collapsed_edit_text.contains("fn new() {}"));
        assert!(!collapsed_edit_text.contains(&edit_input));
        assert!(collapsed_edit_text.contains("result: done"));

        let content = (0..10)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let write_input = serde_json::json!({
            "path": "/tmp/example.txt",
            "content": content,
        })
        .to_string();
        let result = format!("first\n{}", "result".repeat(60));
        let mut preview_lines = Vec::new();
        let footer_line = render_tool_entry(
            &mut preview_lines,
            "file_write",
            &write_input,
            Some(&result),
            false,
            ToolDisclosure::Preview,
        );
        let preview_text = rendered_text(&preview_lines);
        let footer_line = footer_line.expect("long preview has a disclosure footer");
        assert!(preview_text.starts_with("▼ [tool: file_write]"));
        assert!(preview_text.contains(r#"input: {"path":"/tmp/example.txt"}"#));
        assert!(preview_text.contains("line 0"));
        assert!(preview_text.contains("line 5"));
        assert!(!preview_text.contains("line 6"));
        assert!(preview_text.contains("4 more lines"));
        assert!(!preview_text.contains(&write_input));
        assert!(!preview_text.contains(&result));
        assert!(
            preview_lines[footer_line]
                .to_string()
                .contains("4 more lines")
        );
        assert!(preview_text.find("line 5").unwrap() < preview_text.find("4 more lines").unwrap());
        assert!(
            preview_text.find("4 more lines").unwrap()
                < preview_text.find("result: first").unwrap()
        );

        let mut full_lines = Vec::new();
        render_tool_entry(
            &mut full_lines,
            "file_write",
            &write_input,
            Some(&result),
            false,
            ToolDisclosure::Full,
        );
        let full_text = rendered_text(&full_lines);
        assert!(full_text.contains("line 9"));
        assert!(full_text.contains("first"));
        assert!(full_text.contains(&"result".repeat(60)));
        assert!(full_text.contains("[Show less]"));
        assert!(!full_text.contains(&write_input));
        assert!(full_text.find("line 9").unwrap() < full_text.find("[Show less]").unwrap());
        assert!(full_text.find("[Show less]").unwrap() < full_text.find("result: first").unwrap());
    }

    #[test]
    fn file_write_base64_and_malformed_inputs_have_safe_fallbacks() {
        let base64_input = serde_json::json!({
            "path": "/tmp/example.bin",
            "content": "A".repeat(4_000),
            "encoding": "base64",
        })
        .to_string();
        let mut base64_lines = Vec::new();
        let footer_line = render_tool_entry(
            &mut base64_lines,
            "file_write",
            &base64_input,
            None,
            false,
            ToolDisclosure::Preview,
        );
        let base64_text = rendered_text(&base64_lines);
        assert!(footer_line.is_none());
        assert!(base64_text.contains(r#""encoding":"base64""#));
        assert!(base64_text.contains("content: 4000 encoded characters"));
        assert!(!base64_text.contains(&"A".repeat(200)));

        let malformed = r#"{"path":"/tmp/example.txt""#;
        let mut malformed_lines = Vec::new();
        render_tool_entry(
            &mut malformed_lines,
            "file_write",
            malformed,
            None,
            false,
            ToolDisclosure::Preview,
        );
        assert!(rendered_text(&malformed_lines).contains(malformed));

        for invalid in [
            serde_json::json!({"content": "text"}),
            serde_json::json!({"path": "/tmp/a.txt", "content": "text", "encoding": 3}),
            serde_json::json!({"path": "/tmp/a.txt", "content": "text", "encoding": "hex"}),
        ] {
            let invalid = invalid.to_string();
            assert_eq!(
                default_tool_disclosure("file_write", &invalid),
                ToolDisclosure::Collapsed
            );
        }
        assert_eq!(
            default_tool_disclosure("file_edit", r#"{"old_string":"a","new_string":"b"}"#),
            ToolDisclosure::Collapsed
        );
    }

    #[test]
    fn wrapped_tool_hit_regions_exclude_blank_cells_and_respect_scroll() {
        let label = crate::i18n::t_args("zc-chat-tool-show-all", &[("count", "123")]);
        let line = Line::from(format!("  {label}"));
        let body = Rect::new(5, 7, 10, 2);
        let mut regions = Vec::new();
        append_wrapped_hit_rects(&mut regions, 4, &line, 4, 5, body);

        assert_eq!(regions.len(), 2, "first wrapped row is scrolled out");
        assert!(
            regions
                .iter()
                .all(|(entry, rect)| *entry == 4 && rect.height == 1)
        );
        assert_eq!(regions[0].1.y, body.y);
        assert!(regions[0].1.width <= body.width);
        assert!(regions[1].1.width < body.width);
        assert!(!mouse::in_rect(
            body.x + body.width - 1,
            regions[1].1.y,
            regions[1].1
        ));
    }

    #[test]
    fn tool_clipboard_keeps_raw_input_and_result() {
        let entry = ChatEntry::Tool {
            tool_call_id: Arc::<str>::from("tc-copy"),
            name: Arc::<str>::from("file_write"),
            input_json: Arc::<str>::from(r#"{"path":"a.txt","content":"raw"}"#),
            result: Some(Arc::<str>::from("written")),
        };
        let copied = clipboard_text(&entry);
        assert!(copied.contains(r#""content":"raw""#));
        assert!(copied.contains("written"));
    }

    #[tokio::test]
    async fn file_tool_header_and_footer_clicks_keep_cards_independent() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};

        let (mut chat, _rx) = test_chat();
        let mut state = state();
        let content = (0..10)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        for (id, path) in [
            ("tc-offscreen", "older.txt"),
            ("tc-1", "first.txt"),
            ("tc-2", "second.txt"),
        ] {
            state.entries.push(ChatEntry::Tool {
                tool_call_id: Arc::<str>::from(id),
                name: Arc::<str>::from("file_write"),
                input_json: Arc::<str>::from(
                    serde_json::json!({"path": path, "content": content.clone()}).to_string(),
                ),
                result: Some(Arc::<str>::from("written")),
            });
        }
        state.mark_dirty_full();

        let area = Rect::new(0, 0, 80, 24);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, &mut state, area, PaneKind::Chat))
            .expect("draw chat");
        state.pinned_to_bottom = false;
        state.scroll_offset = state.cached_screen_ranges[1].1;
        terminal
            .draw(|frame| render(frame, &mut state, area, PaneKind::Chat))
            .expect("draw scrolled chat");
        assert_eq!(
            state
                .visible_cached_entry_range(state.scroll_offset, state.last_inner_height)
                .start,
            1,
            "the click must use an absolute cached-entry index after scrolling"
        );
        assert!(state.entry_rects.iter().all(|(idx, _)| *idx != 0));
        assert!(state.tool_header_rects.iter().all(|(idx, _)| *idx != 0));
        assert!(state.tool_footer_rects.iter().all(|(idx, _)| *idx != 0));
        assert_eq!(state.tool_footer_rects[0].0, 1);
        let first_footer = state.tool_footer_rects[0].1;
        chat.phase = ChatPhase::Active(Box::new(state));

        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: first_footer.x + 1,
                row: first_footer.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
        .await;
        let ChatPhase::Active(state) = &mut chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(
            state.tool_disclosures.get("tc-1"),
            Some(&ToolDisclosure::Full)
        );
        assert!(!state.tool_disclosures.contains_key("tc-2"));
        assert!(!state.tool_disclosures.contains_key("tc-offscreen"));
        assert_eq!(state.dirty, LinesDirty::Full);

        terminal
            .draw(|frame| render(frame, state, area, PaneKind::Chat))
            .expect("redraw expanded chat");
        let first_header = state.tool_header_rects[0].1;
        chat.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: first_header.x + 1,
                row: first_header.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
        .await;
        let ChatPhase::Active(state) = &mut chat.phase else {
            panic!("expected active chat");
        };
        assert_eq!(
            state.tool_disclosures.get("tc-1"),
            Some(&ToolDisclosure::Collapsed)
        );
        assert!(!state.tool_disclosures.contains_key("tc-2"));
        assert!(!state.tool_disclosures.contains_key("tc-offscreen"));

        state.reset_for_session(
            "sess-2".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        assert!(state.tool_disclosures.is_empty());
        assert!(state.tool_header_rects.is_empty());
        assert!(state.tool_footer_rects.is_empty());
    }

    #[test]
    fn approval_request_sets_pending_approval() {
        let mut s = state();
        s.apply_update(SessionUpdate::ApprovalRequest {
            session_id: "sess-1".to_string(),
            request_id: "req-1".to_string(),
            tool_name: "shell".to_string(),
            arguments_summary: "rm -rf /".to_string(),
            timeout_secs: 30,
        });
        assert!(s.pending_approval().is_some());
        let pa = s.pending_approval().unwrap();
        assert_eq!(pa.request_id, "req-1");
        assert_eq!(pa.tool_name, "shell");
    }

    #[test]
    fn approval_overlay_uses_theme_background_after_clear() {
        use ratatui::{Terminal, backend::TestBackend};

        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let expected_bg = theme::background();
        assert_ne!(
            expected_bg,
            ratatui::style::Color::Reset,
            "default ZeroCode theme should provide a concrete modal background"
        );

        let mut s = state();
        s.apply_update(SessionUpdate::ApprovalRequest {
            session_id: "sess-1".to_string(),
            request_id: "req-1".to_string(),
            tool_name: "shell".to_string(),
            arguments_summary: "command: pwd".to_string(),
            timeout_secs: 120,
        });

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_approval_overlay(frame, &s, area);
            })
            .expect("draw approval overlay");

        let cell = &terminal.backend().buffer()[(10, 28)];
        assert_eq!(
            cell.style().bg,
            Some(expected_bg),
            "approval overlay interior must use the active ZeroCode theme background"
        );
    }

    #[test]
    fn queue_sidebar_uses_theme_background_after_clear() {
        use ratatui::{Terminal, backend::TestBackend};

        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let expected_bg = theme::background();
        assert_ne!(
            expected_bg,
            ratatui::style::Color::Reset,
            "default ZeroCode theme should provide a concrete sidebar background"
        );

        let mut s = state();
        s.enqueue_message("what's happening".to_string(), Vec::new())
            .expect("queue message");
        s.enqueue_message("second queued message".to_string(), Vec::new())
            .expect("queue message");
        s.ensure_queue_selection();

        let area = Rect::new(0, 0, 36, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_queue_sidebar(frame, &mut s, area);
            })
            .expect("draw queue sidebar");

        let cell = &terminal.backend().buffer()[(4, 6)];
        assert_eq!(
            cell.style().bg,
            Some(expected_bg),
            "queue sidebar interior must use the active ZeroCode theme background"
        );

        let unselected_text_cell = &terminal.backend().buffer()[(6, 2)];
        assert_eq!(
            unselected_text_cell.style().fg,
            Some(theme::active().body),
            "unselected queue text must use the active ZeroCode body foreground"
        );
        assert_eq!(
            unselected_text_cell.style().bg,
            Some(expected_bg),
            "unselected queue text must keep the themed fill background"
        );
    }

    #[test]
    fn session_list_overlay_uses_theme_background_after_clear() {
        use ratatui::{Terminal, backend::TestBackend};

        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let expected_bg = theme::background();
        assert_ne!(
            expected_bg,
            ratatui::style::Color::Reset,
            "default ZeroCode theme should provide a concrete modal background"
        );

        let sessions = vec![SessionEntry {
            session_id: "session-1".to_string(),
            session_key: "session-1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            last_activity: "2026-01-01T00:00:00Z".to_string(),
            agent_alias: Some("agent".to_string()),
            channel_id: None,
            name: Some("first prompt".to_string()),
            message_count: 1,
        }];
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let area = Rect::new(0, 0, 100, 30);
        let overlay_area = session_list_overlay_area(area);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_session_list_overlay(
                    frame,
                    area,
                    &sessions,
                    &mut list_state,
                    crate::i18n::t("zc-chat-session-list-switch-title"),
                    None,
                );
            })
            .expect("draw session list overlay");

        let cell = &terminal.backend().buffer()[(overlay_area.x + 4, overlay_area.y + 6)];
        assert_eq!(
            cell.style().bg,
            Some(expected_bg),
            "session list overlay interior must use the active ZeroCode theme background"
        );

        let selected_text_cell =
            &terminal.backend().buffer()[(overlay_area.x + 4, overlay_area.y + 1)];
        assert_eq!(
            selected_text_cell.style().fg,
            Some(theme::active().heading),
            "selected session row must keep the overlay highlight foreground"
        );
        assert_eq!(
            selected_text_cell.style().bg,
            Some(expected_bg),
            "selected session row must keep the themed fill background"
        );
    }

    /// Collects every row of `area` in `terminal`'s buffer into a single
    /// newline-joined string, for substring assertions on rendered text.
    fn overlay_text(
        terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
        area: Rect,
    ) -> String {
        let buf = terminal.backend().buffer();
        (area.y..area.y + area.height)
            .map(|y| {
                (area.x..area.x + area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn session_list_overlay_renders_memory_isolation_note() {
        use ratatui::{Terminal, backend::TestBackend};

        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let sessions = vec![SessionEntry {
            session_id: "session-1".to_string(),
            session_key: "session-1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            last_activity: "2026-01-01T00:00:00Z".to_string(),
            agent_alias: Some("agent".to_string()),
            channel_id: None,
            name: Some("first prompt".to_string()),
            message_count: 1,
        }];
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let area = Rect::new(0, 0, 100, 30);
        let overlay_area = session_list_overlay_area(area);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                // Mirrors the Code (ACP) pre-session picker call site: the
                // resume title plus the memory-isolation note.
                render_session_list_overlay(
                    frame,
                    area,
                    &sessions,
                    &mut list_state,
                    crate::i18n::t("zc-chat-session-list-resume-title"),
                    Some(crate::i18n::t("zc-chat-session-list-resume-note")),
                );
            })
            .expect("draw session list overlay with note");

        let text = overlay_text(&terminal, overlay_area);
        assert!(
            text.contains("resumable"),
            "Code session picker must state history is saved & resumable: {text:?}"
        );
        assert!(
            text.contains("isolated"),
            "Code session picker must state persistent memory is isolated: {text:?}"
        );
    }

    #[test]
    fn session_list_overlay_renders_full_memory_note_on_narrow_terminal() {
        use ratatui::{Terminal, backend::TestBackend};

        // Regression guard: at the ubiquitous 80-column default the centered
        // overlay is narrow enough that the note wraps to a second line. A
        // single reserved row would drop the "isolated" half; the reservation
        // must grow to keep the full disclosure visible.
        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let sessions = vec![SessionEntry {
            session_id: "session-1".to_string(),
            session_key: "session-1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            last_activity: "2026-01-01T00:00:00Z".to_string(),
            agent_alias: Some("agent".to_string()),
            channel_id: None,
            name: Some("first prompt".to_string()),
            message_count: 1,
        }];
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let area = Rect::new(0, 0, 80, 24);
        let overlay_area = session_list_overlay_area(area);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_session_list_overlay(
                    frame,
                    area,
                    &sessions,
                    &mut list_state,
                    crate::i18n::t("zc-chat-session-list-resume-title"),
                    Some(crate::i18n::t("zc-chat-session-list-resume-note")),
                );
            })
            .expect("draw session list overlay with note at 80 cols");

        let text = overlay_text(&terminal, overlay_area);
        assert!(
            text.contains("resumable"),
            "80-col Code session picker must state history is saved & resumable: {text:?}"
        );
        assert!(
            text.contains("isolated"),
            "80-col Code session picker must show the full note incl. persistent \
             memory isolation (second wrapped line must not be dropped): {text:?}"
        );
    }

    #[test]
    fn session_switch_overlay_omits_memory_isolation_note() {
        use ratatui::{Terminal, backend::TestBackend};

        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let sessions = vec![SessionEntry {
            session_id: "session-1".to_string(),
            session_key: "session-1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            last_activity: "2026-01-01T00:00:00Z".to_string(),
            agent_alias: Some("agent".to_string()),
            channel_id: None,
            name: Some("first prompt".to_string()),
            message_count: 1,
        }];
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let area = Rect::new(0, 0, 100, 30);
        let overlay_area = session_list_overlay_area(area);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                // Mirrors the in-session switch overlay call site (shared by
                // both panes): no `note`, so the Code-only copy must not
                // leak into this Chat-reachable path.
                render_session_list_overlay(
                    frame,
                    area,
                    &sessions,
                    &mut list_state,
                    crate::i18n::t("zc-chat-session-list-switch-title"),
                    None,
                );
            })
            .expect("draw session switch overlay without note");

        let text = overlay_text(&terminal, overlay_area);
        assert!(
            !text.contains("resumable") && !text.contains("isolated"),
            "in-session switch overlay must not render the Code memory-isolation note: {text:?}"
        );
    }

    #[tokio::test]
    async fn pick_session_help_context_states_memory_isolation() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        chat.phase = ChatPhase::PickSession {
            sessions: Vec::new(),
            list_state: ListState::default(),
            agents: Vec::new(),
        };

        let help = crate::widgets::HelpContext::help_context(&chat);
        let has_memory_note = help
            .entries
            .iter()
            .any(|e| e.action.contains("resumable") && e.action.contains("isolated"));
        assert!(
            has_memory_note,
            "Code session picker help must explain history is saved & resumable while \
             persistent memory is isolated: {help:?}"
        );
    }

    #[tokio::test]
    async fn pick_agent_help_context_omits_memory_isolation_note() {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        // Default phase is PickAgent — not the Code session picker — for
        // both Chat and Acp panes, so the memory-isolation entry must not
        // appear here. Force a non-loading PickAgent so this exercises the
        // real (non-loading) help branch and genuinely proves the pane gate.
        let mut chat = Chat::new(client, PaneKind::Chat);
        chat.phase = ChatPhase::PickAgent {
            agents: vec!["agent-a".to_string()],
            list_state: ListState::default(),
            loading: false,
        };

        let help = crate::widgets::HelpContext::help_context(&chat);
        let has_memory_note = help.entries.iter().any(|e| e.action.contains("isolated"));
        assert!(
            !has_memory_note,
            "non-PickSession help (e.g. Chat pane) must not surface the Code-only \
             memory-isolation note: {help:?}"
        );
    }

    #[tokio::test]
    async fn pick_agent_help_context_states_memory_isolation_on_acp_pane() {
        // No-saved-session boundary from the linked issue: a first-time Code
        // user (or any user with no resumable ACP history) lands in the *agent*
        // picker, not the resume picker, so the disclosure must be reachable
        // there too — but only on the Code (ACP) pane.
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);
        chat.phase = ChatPhase::PickAgent {
            agents: vec!["agent-a".to_string()],
            list_state: ListState::default(),
            loading: false,
        };

        let help = crate::widgets::HelpContext::help_context(&chat);
        let has_memory_note = help.entries.iter().any(|e| e.action.contains("isolated"));
        assert!(
            has_memory_note,
            "ACP agent picker (no-saved-session path) help must surface the \
             history-vs-persistent-memory disclosure: {help:?}"
        );
    }

    #[test]
    fn agent_picker_renders_memory_isolation_note_on_acp_pane() {
        use ratatui::{Terminal, backend::TestBackend};

        // The multi-agent no-saved-session path renders the agent picker; on the
        // Code (ACP) pane it must carry the memory-isolation disclosure in its
        // footer so the distinction is visible before starting Code work.
        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let agents = vec!["agent-a".to_string(), "agent-b".to_string()];
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                draw_agent_picker(
                    frame,
                    area,
                    &agents,
                    &mut list_state,
                    false,
                    &PaneKind::Acp.name(),
                    Some(crate::i18n::t("zc-chat-agent-picker-acp-memory-note")),
                );
            })
            .expect("draw agent picker with acp note");

        let text = overlay_text(&terminal, area);
        assert!(
            text.contains("resumable") && text.contains("isolated"),
            "ACP agent picker must render the full memory-isolation note: {text:?}"
        );
    }

    #[test]
    fn agent_picker_keeps_full_memory_note_and_footer_non_clickable_when_narrow() {
        use ratatui::{Terminal, backend::TestBackend};

        // Regression guard for a 22-cell pane (20-cell bordered inner width):
        // the catalogue copy needs more than three wrapped rows, so a fixed
        // three-row footer clips the word "isolated" and makes the disclosure
        // materially false at this width.
        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let agents = vec!["agent-a".to_string(), "agent-b".to_string()];
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let area = Rect::new(0, 0, 22, 16);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut list_area = Rect::default();
        terminal
            .draw(|frame| {
                list_area = draw_agent_picker(
                    frame,
                    area,
                    &agents,
                    &mut list_state,
                    false,
                    &PaneKind::Acp.name(),
                    Some(crate::i18n::t("zc-chat-agent-picker-acp-memory-note")),
                );
            })
            .expect("draw narrow agent picker with acp note");

        let text = overlay_text(&terminal, area);
        assert!(
            text.contains("resumable") && text.contains("isolated"),
            "narrow ACP picker must render the complete disclosure: {text:?}"
        );

        let footer_row = area.y + area.height - 2;
        assert!(
            crate::mouse::list_click_index(footer_row, list_area, 0, agents.len()).is_none(),
            "the disclosure footer must remain outside agent-list hit testing"
        );
    }

    #[test]
    fn agent_picker_omits_memory_isolation_note_on_chat_pane() {
        use ratatui::{Terminal, backend::TestBackend};

        // The Chat pane reaches the same PickAgent phase but must NOT surface the
        // Code-only disclosure — the render call site passes `None`.
        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let agents = vec!["agent-a".to_string(), "agent-b".to_string()];
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                draw_agent_picker(
                    frame,
                    area,
                    &agents,
                    &mut list_state,
                    false,
                    &PaneKind::Chat.name(),
                    None,
                );
            })
            .expect("draw agent picker without note");

        let text = overlay_text(&terminal, area);
        assert!(
            !text.contains("isolated"),
            "Chat pane agent picker must not render the Code memory-isolation note: {text:?}"
        );
    }

    #[tokio::test]
    async fn acp_init_single_agent_no_history_shows_disclosure_before_session_start() {
        use ratatui::{Terminal, backend::TestBackend};

        // The single-agent counterpart of the no-saved-session boundary above:
        // with exactly one enabled agent, `init` skips `show_agent_picker` and
        // `try_show_recent_acp_session_picker` finds nothing to resume, so the
        // old fall-through called `pick_or_start_session()` directly and never
        // showed the disclosure. It must now land on the same
        // disclosure-bearing agent picker as the multi-agent path, and it must
        // do so *before* any `session/new` request goes out.
        let _theme_guard = theme::set_active_for_test(theme::default_theme());
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Acp);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0}
                ]
            }),
        );

        let request =
            next_rpc_request(&mut rx, "single-agent init should check for saved sessions").await;
        assert_eq!(request["method"], method::SESSION_LIST_ACP);
        respond_ok(&rpc, &request, serde_json::json!({ "sessions": [] }));

        // `init` must finish here without a `config/list` or `session/new`
        // request ever going out: neither response was supplied above, so if
        // `init` tried to start a session first this join would time out.
        let mut chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect(
                "single-agent no-history ACP init must land on the disclosure surface \
                 without starting a session first",
            )
            .unwrap();
        let ChatPhase::PickAgent {
            agents, loading, ..
        } = &chat.phase
        else {
            panic!("single-agent no-history ACP start must land in the agent picker");
        };
        assert_eq!(agents, &vec!["alpha".to_string()]);
        assert!(!loading);

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| chat.draw(frame, area))
            .expect("draw single-agent no-history agent picker");

        let text = overlay_text(&terminal, area);
        assert!(
            text.contains("resumable") && text.contains("isolated"),
            "single-agent Code start with no saved session must render the \
             history-vs-persistent-memory disclosure before any session starts: {text:?}"
        );
    }

    #[tokio::test]
    async fn chat_init_single_agent_no_history_skips_disclosure_and_autostarts() {
        // Companion to the ACP case above: the Chat pane must keep the
        // original no-saved-session behavior unchanged — straight into the
        // session, no agent-picker detour, and no Code-only disclosure ever
        // in the picture, since Chat has no session history to disclose.
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);

        let init = tokio::spawn(async move {
            let _ = chat.init().await;
            chat
        });

        let request = next_rpc_request(&mut rx, "init should request agents/status").await;
        assert_eq!(request["method"], method::AGENTS_STATUS);
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "agents": [
                    {"alias": "alpha", "enabled": true, "live_sessions": 0, "persisted_sessions": 0}
                ]
            }),
        );

        // Chat never checks for ACP session history, so the very next request
        // must mint the session directly — never a `session/list_acp` request
        // and never a detour through the agent picker. TodoTracker settings are
        // resolved from the local ZeroCode config before this RPC boundary.
        let request = next_rpc_request(
            &mut rx,
            "Chat single-agent start should mint a fresh session",
        )
        .await;
        assert_eq!(request["method"], method::SESSION_NEW);
        assert_eq!(request["params"]["agent_alias"], "alpha");
        respond_ok(
            &rpc,
            &request,
            serde_json::json!({
                "session_id": "sess-chat",
                "workspace_dir": "/tmp/chat"
            }),
        );

        let request =
            next_rpc_request(&mut rx, "fresh Chat session should refresh model identity").await;
        assert_eq!(request["method"], method::CONFIG_LIST);
        respond_ok(&rpc, &request, serde_json::json!([]));

        let chat = tokio::time::timeout(Duration::from_secs(2), init)
            .await
            .expect("init should finish")
            .unwrap();
        let ChatPhase::Active(state) = chat.phase else {
            panic!("Chat single-agent no-history start must go straight to an active session");
        };
        assert_eq!(state.session_id, "sess-chat");
        assert_eq!(state.agent_alias, "alpha");
    }

    #[test]
    fn note_reserved_rows_accounts_for_word_boundary_wrapping() {
        let note = crate::i18n::t("zc-chat-agent-picker-acp-memory-note");
        assert!(
            unicode_width::UnicodeWidthStr::width(note.as_str()) > 31,
            "test copy must exceed the narrow inner width to exercise wrapping"
        );
        assert!(
            note_reserved_rows(&note, 31) >= 3,
            "31-cell inner width must reserve 3 rows for the word-wrapped note, \
             not the 2 a naive ceil would give"
        );
        // Wide terminal: fits on one line.
        assert_eq!(note_reserved_rows(&note, 200), 1);
        // The full disclosure remains reserved even at very narrow widths.
        assert!(note_reserved_rows(&note, 20) > 3);
    }

    #[test]
    fn note_reserved_rows_uses_paragraph_hard_wrapping_for_long_words() {
        assert_eq!(note_reserved_rows("abcdefghijkl", 5), 3);
        assert_eq!(note_reserved_rows("", 10), 1);
        assert_eq!(note_reserved_rows("word", 10), 1);
    }

    #[test]
    fn session_overlay_retains_scroll_offset_for_mouse_hit_test() {
        use ratatui::{Terminal, backend::TestBackend};
        // Regression for the picker selecting the wrong row after scrolling:
        // the renderer must persist the offset it computes so mouse hit-testing
        // reads the same geometry that was drawn.
        let sessions: Vec<SessionEntry> = (0..30)
            .map(|i| SessionEntry {
                session_id: format!("session-{i}"),
                session_key: format!("session-{i}"),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                last_activity: "2026-01-01T00:00:00Z".to_string(),
                agent_alias: Some("agent".to_string()),
                channel_id: None,
                name: Some(format!("prompt {i}")),
                message_count: 1,
            })
            .collect();

        let mut list_state = ListState::default();
        // Select a row far enough down that the list must scroll to show it.
        list_state.select(Some(25));

        let area = Rect::new(0, 0, 100, 30);
        let overlay_area = session_list_overlay_area(area);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_session_list_overlay(
                    frame,
                    area,
                    &sessions,
                    &mut list_state,
                    crate::i18n::t("zc-chat-session-list-switch-title"),
                    None,
                );
            })
            .expect("draw session list overlay");

        // The renderer scrolled the list to reveal the selection; that offset
        // must survive the draw. Before the fix it was computed on a discarded
        // copy and stayed 0.
        let offset = list_state.offset();
        assert!(
            offset > 0,
            "rendering a scrolled selection must retain a nonzero offset, got {offset}"
        );

        // A click on the third visible row must resolve through the retained
        // offset, not the pre-scroll top of the list.
        let clicked_row = overlay_area.y + 1 + 2;
        let idx = crate::mouse::list_click_index(clicked_row, overlay_area, offset, sessions.len())
            .expect("click inside the visible list resolves to a row");
        assert_eq!(
            idx,
            offset + 2,
            "clicked row must map to offset + visible row, not the unscrolled index"
        );
    }

    #[test]
    fn resume_picker_footer_clicks_do_not_select_hidden_sessions() {
        use ratatui::{Terminal, backend::TestBackend};

        let _theme_guard = theme::set_active_for_test(theme::default_theme());

        // Enough saved sessions to overflow the visible list, so the rows
        // hidden behind the footer note correspond to real (off-screen)
        // session indices — the exact shape where a footer click used to
        // move the selection to a hidden session.
        let sessions: Vec<SessionEntry> = (0..40)
            .map(|i| SessionEntry {
                session_id: format!("sess-{i}"),
                session_key: format!("sess-{i}"),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                last_activity: "2026-01-01T00:00:00Z".to_string(),
                agent_alias: Some("agent".to_string()),
                channel_id: None,
                name: Some(format!("prompt {i}")),
                message_count: 1,
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(0));

        // 80x24 default terminal: narrow enough that the resume note wraps.
        let area = Rect::new(0, 0, 80, 24);
        let overlay_area = session_list_overlay_area(area);
        let note = crate::i18n::t("zc-chat-session-list-resume-note");
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_session_list_overlay(
                    frame,
                    area,
                    &sessions,
                    &mut list_state,
                    crate::i18n::t("zc-chat-session-list-resume-title"),
                    Some(note.clone()),
                );
            })
            .expect("draw resume overlay");

        let click_area = session_list_click_area(overlay_area, Some(&note));
        let reserved = note_reserved_rows(&note, overlay_area.width.saturating_sub(2));
        assert_eq!(
            click_area.height,
            overlay_area.height - reserved,
            "click area must exclude exactly the reserved note rows"
        );

        // Every reserved footer row (the note area sits directly above the
        // bottom border) must be dead for list hit-testing, while the same
        // rows against the full overlay rect would have resolved to a session.
        let offset = list_state.offset();
        for row_from_bottom in 0..reserved {
            let note_row = overlay_area.y + overlay_area.height - 2 - row_from_bottom;
            assert!(
                crate::mouse::list_click_index(note_row, click_area, offset, sessions.len())
                    .is_none(),
                "footer note row {note_row} must not resolve to a session index"
            );
            assert!(
                crate::mouse::list_click_index(note_row, overlay_area, offset, sessions.len())
                    .is_some(),
                "regression precondition: the full overlay rect maps row {note_row} to a session"
            );
        }

        // The last true list row must still be clickable through the shrunken rect.
        let last_list_row = overlay_area.y + click_area.height - 2;
        assert!(
            crate::mouse::list_click_index(last_list_row, click_area, offset, sessions.len())
                .is_some(),
            "the final visible list row must remain clickable"
        );
    }

    #[test]
    fn thought_chunk_visible_before_commit() {
        let mut s = state();
        s.turn_in_flight = true;
        s.apply_update(SessionUpdate::AgentThoughtChunk {
            session_id: "sess-1".to_string(),
            text: "reasoning...".to_string(),
        });
        assert_eq!(s.current_thought_text(), "reasoning...");
        assert!(
            s.entries().is_empty(),
            "thought must not become an entry mid-turn"
        );
    }

    #[test]
    fn thought_flushed_as_entry_before_tool_call() {
        let mut s = state();
        s.turn_in_flight = true;
        s.apply_update(SessionUpdate::AgentThoughtChunk {
            session_id: "sess-1".to_string(),
            text: "plan: run ls".to_string(),
        });
        s.apply_update(SessionUpdate::ToolCall {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            name: "shell".to_string(),
            raw_input: serde_json::json!({"command": "ls"}),
        });
        // Thought must be committed as an entry before the tool entry.
        assert_eq!(s.entries().len(), 2);
        assert!(
            matches!(&s.entries()[0], ChatEntry::AgentThought(t) if t.as_ref() == "plan: run ls")
        );
        assert!(matches!(&s.entries()[1], ChatEntry::Tool { .. }));
        // streaming_thought is now clear.
        assert!(s.current_thought_text().is_empty());
    }

    #[test]
    fn thought_flushed_as_entry_before_first_response_chunk() {
        let mut s = state();
        s.turn_in_flight = true;
        s.apply_update(SessionUpdate::AgentThoughtChunk {
            session_id: "sess-1".to_string(),
            text: "thinking".to_string(),
        });
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "Here is".to_string(),
        });
        // Thought entry committed before streaming text starts.
        assert_eq!(s.entries().len(), 1);
        assert!(matches!(&s.entries()[0], ChatEntry::AgentThought(t) if t.as_ref() == "thinking"));
        assert_eq!(s.current_agent_text(), "Here is");
        assert!(s.current_thought_text().is_empty());
    }

    #[test]
    fn subsequent_message_chunks_do_not_re_flush_thought() {
        let mut s = state();
        s.turn_in_flight = true;
        s.apply_update(SessionUpdate::AgentThoughtChunk {
            session_id: "sess-1".to_string(),
            text: "thinking".to_string(),
        });
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "Hello".to_string(),
        });
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: " world".to_string(),
        });
        // Only one AgentThought entry, not two.
        assert_eq!(s.entries().len(), 1);
        assert_eq!(s.current_agent_text(), "Hello world");
    }

    // ── Interleaving regression tests ────────────────────────────

    #[test]
    fn text_before_tool_call_is_flushed_as_separate_agent_message() {
        let mut s = state();
        s.turn_in_flight = true;

        // Pre-tool text chunk.
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "I will run ls.".to_string(),
        });

        // Tool call interrupts the text stream.
        s.apply_update(SessionUpdate::ToolCall {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            name: "shell".to_string(),
            raw_input: serde_json::json!({"command": "ls"}),
        });

        // At this point the pre-tool text must be committed as its own entry.
        assert_eq!(
            s.entries().len(),
            2,
            "expected AgentMessage + Tool entries, got {:?}",
            s.entries()
        );
        assert!(
            matches!(&s.entries()[0], ChatEntry::AgentMessage(t) if t.as_ref() == "I will run ls."),
            "first entry must be AgentMessage with pre-tool text"
        );
        assert!(
            matches!(&s.entries()[1], ChatEntry::Tool { .. }),
            "second entry must be Tool"
        );
        // streaming_text must be cleared after the flush.
        assert!(
            s.current_agent_text().is_empty(),
            "streaming_text must be empty after tool-call flush"
        );
    }

    #[test]
    fn text_after_tool_call_commits_separately() {
        let mut s = state();
        s.turn_in_flight = true;

        // Pre-tool text.
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "Running ls.".to_string(),
        });
        // Tool call flushes pre-tool text.
        s.apply_update(SessionUpdate::ToolCall {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            name: "shell".to_string(),
            raw_input: serde_json::json!({"command": "ls"}),
        });
        // Tool result.
        s.apply_update(SessionUpdate::ToolResult {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            raw_output: "file.txt\n".to_string(),
        });
        // Post-tool text.
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "Done.".to_string(),
        });
        assert_eq!(s.current_agent_text(), "Done.");

        // commit_turn: only the post-tool text should become a new AgentMessage.
        s.commit_turn("Done.".to_string(), true);

        // Final order: AgentMessage("Running ls.") | Tool | AgentMessage("Done.")
        assert_eq!(
            s.entries().len(),
            3,
            "expected 3 entries: pre-tool AgentMessage, Tool, post-tool AgentMessage"
        );
        assert!(
            matches!(&s.entries()[0], ChatEntry::AgentMessage(t) if t.as_ref() == "Running ls."),
            "first entry must be pre-tool AgentMessage"
        );
        assert!(
            matches!(
                &s.entries()[1],
                ChatEntry::Tool {
                    result: Some(_),
                    ..
                }
            ),
            "second entry must be Tool with result"
        );
        assert!(
            matches!(&s.entries()[2], ChatEntry::AgentMessage(t) if t.as_ref() == "Done."),
            "third entry must be post-tool AgentMessage"
        );
    }

    #[test]
    fn no_spurious_agent_message_when_no_pre_tool_text() {
        let mut s = state();
        s.turn_in_flight = true;

        // Tool call with no preceding text chunk.
        s.apply_update(SessionUpdate::ToolCall {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            name: "shell".to_string(),
            raw_input: serde_json::json!({"command": "ls"}),
        });

        // Only the Tool entry should exist — no empty AgentMessage.
        assert_eq!(s.entries().len(), 1);
        assert!(matches!(&s.entries()[0], ChatEntry::Tool { .. }));
    }

    #[test]
    fn commit_turn_does_not_duplicate_already_flushed_text() {
        let mut s = state();
        s.turn_in_flight = true;

        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "Before tool.".to_string(),
        });
        s.apply_update(SessionUpdate::ToolCall {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            name: "shell".to_string(),
            raw_input: serde_json::json!({"command": "ls"}),
        });
        // No post-tool text; commit_turn receives the full text but streaming_text is empty.
        s.commit_turn("Before tool.".to_string(), true);

        // Must be exactly: AgentMessage("Before tool.") | Tool
        // NOT: AgentMessage | Tool | AgentMessage (duplicate)
        assert_eq!(
            s.entries().len(),
            2,
            "commit_turn must not add a duplicate AgentMessage for already-flushed text"
        );
        assert!(
            matches!(&s.entries()[0], ChatEntry::AgentMessage(t) if t.as_ref() == "Before tool.")
        );
        assert!(matches!(&s.entries()[1], ChatEntry::Tool { .. }));
    }

    /// When no streaming text was accumulated, commit_turn must use the
    /// daemon-provided final text as a fallback — rendered exactly once.
    #[test]
    fn commit_turn_renders_nonempty_fallback_when_no_streaming() {
        let mut s = state();
        s.turn_in_flight = true;

        // No streaming chunks; commit_turn receives non-empty final text.
        s.commit_turn("Hello from daemon.".to_string(), true);

        assert_eq!(s.entries().len(), 1);
        assert!(
            matches!(&s.entries()[0], ChatEntry::AgentMessage(t) if t.as_ref() == "Hello from daemon.")
        );
    }

    /// When a turn completes with no streamed text, no tool calls, and no
    /// final content, commit_turn must render a diagnostic system message
    /// so the user knows the turn finished.
    #[test]
    fn commit_turn_shows_diagnostic_when_no_output_at_all() {
        let mut s = state();
        s.turn_in_flight = true;

        // Empty everything: no streaming, no tools, empty final text.
        s.commit_turn(String::new(), true);

        assert_eq!(s.entries().len(), 1);
        assert!(
            matches!(&s.entries()[0], ChatEntry::SystemMessage(t) if t.as_ref() == "Turn completed with no output."),
            "expected diagnostic SystemMessage for empty completion, got {:?}",
            s.entries()[0]
        );
    }

    /// When a cancelled or failed turn has no output, commit_turn must NOT
    /// append the "Turn completed with no output" diagnostic — cancelled/
    /// failed turns are not clean completions and should not claim otherwise.
    #[test]
    fn commit_turn_no_diagnostic_when_not_clean() {
        let mut s = state();
        s.turn_in_flight = true;

        // Clean=false (cancelled/failed), empty everything.
        s.commit_turn(String::new(), false);

        assert!(
            s.entries().is_empty(),
            "cancelled turn should not emit completion diagnostic, got {:?}",
            s.entries()
        );
    }

    /// When tool calls were made during a turn but no text was streamed and
    /// final text is empty, commit_turn must NOT add a diagnostic — the tool
    /// entries are the visible record of work.
    #[test]
    fn commit_turn_no_diagnostic_when_tool_calls_present() {
        let mut s = state();
        s.turn_in_flight = true;

        s.apply_update(SessionUpdate::ToolCall {
            session_id: "sess-1".to_string(),
            tool_call_id: "tc1".to_string(),
            name: "shell".to_string(),
            raw_input: serde_json::json!({"command": "ls"}),
        });
        s.commit_turn(String::new(), true);

        // Only the Tool entry — no diagnostic needed.
        assert_eq!(s.entries().len(), 1);
        assert!(matches!(&s.entries()[0], ChatEntry::Tool { .. }));
    }

    #[test]
    fn turn_commit_flushes_streaming_buffer() {
        let mut s = state();
        s.apply_update(SessionUpdate::AgentMessageChunk {
            session_id: "sess-1".to_string(),
            text: "Done".to_string(),
        });
        s.commit_turn("Done".to_string(), true);
        assert_eq!(s.current_agent_text(), "");
        assert!(
            s.entries()
                .iter()
                .any(|e| matches!(e, ChatEntry::AgentMessage(t) if t.as_ref() == "Done"))
        );
    }

    // ── markdown_to_lines ──────────────────────────────────────────

    fn rendered(input: &str, width: u16) -> String {
        markdown_to_lines(input, width)
            .into_iter()
            .map(|l| {
                l.spans
                    .into_iter()
                    .map(|s| s.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn md_code_block_bars_span_full_width() {
        let width: u16 = 50;
        let out = rendered("```rust\nlet x = 1;\n```\n", width);
        let header = out.lines().find(|l| l.starts_with('\u{250c}')).unwrap();
        let footer = out.lines().find(|l| l.starts_with('\u{2514}')).unwrap();
        assert_eq!(header.chars().count(), width as usize, "header: {header:?}");
        assert_eq!(
            header.chars().count(),
            footer.chars().count(),
            "header and footer must match width"
        );
        let copy_col = |l: &str| l.chars().take_while(|c| *c != '[').count();
        assert_eq!(
            copy_col(header),
            copy_col(footer),
            "[Copy] must start at the same column on header and footer\nheader: {header:?}\nfooter: {footer:?}"
        );
    }

    #[test]
    fn md_code_block_header_shows_language() {
        let out = rendered("```python\nx = 1\n```\n", 50);
        let header = out.lines().find(|l| l.starts_with('\u{250c}')).unwrap();
        assert!(
            header.contains(" python "),
            "header must show the fence language: {header:?}"
        );
        assert!(
            !header.contains(" code "),
            "labeled fence must not fall back to ` code `: {header:?}"
        );
    }

    #[test]
    fn md_code_block_header_strips_info_extras() {
        let out = rendered("```python title=\"x\"\nx = 1\n```\n", 50);
        let header = out.lines().find(|l| l.starts_with('\u{250c}')).unwrap();
        assert!(
            header.contains(" python "),
            "only the language token is used as the label: {header:?}"
        );
        assert!(
            !header.contains("title"),
            "info-string extras must not leak into the label: {header:?}"
        );
    }

    #[test]
    fn md_code_block_unlabeled_fence_falls_back() {
        let out = rendered("```\nx = 1\n```\n", 50);
        let header = out.lines().find(|l| l.starts_with('\u{250c}')).unwrap();
        assert!(
            header.contains(" code "),
            "unlabeled fence keeps the ` code ` fallback: {header:?}"
        );
    }

    #[test]
    fn md_code_block_body_has_no_left_gutter() {
        let out = rendered("```rust\nlet x = 1;\n```\n", 50);
        let body = out
            .lines()
            .find(|l| l.contains("let x = 1;"))
            .expect("code body line");
        assert!(
            !body.starts_with('\u{2502}'),
            "code body must not start with a vertical gutter: {body:?}"
        );
        assert_eq!(
            body.strip_prefix("  ").map(str::trim_end),
            Some("let x = 1;"),
            "body line is two-space indented code: {body:?}"
        );
    }

    #[test]
    fn md_code_block_body_is_syntax_highlighted() {
        let _g = theme::set_active_for_test(
            theme::theme_by_name("icy_blue").expect("icy_blue registered"),
        );
        let lines = markdown_to_lines("```rust\nfn main() {}\n```\n", 60);
        let body = lines
            .iter()
            .find(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .contains("fn")
            })
            .expect("code body line");
        assert!(
            body.spans.len() > 2,
            "highlighted body should split into multiple token spans, got {}",
            body.spans.len()
        );
        let keyword_fg = theme::SyntaxScope::Keyword.color();
        assert!(
            body.spans.iter().any(|s| s.style.fg == Some(keyword_fg)),
            "the `fn` keyword should carry the themed keyword colour"
        );
    }

    #[test]
    fn md_code_block_unknown_language_stays_plain() {
        let lines = markdown_to_lines("```nonexistent_lang_xyz\nfoo bar\n```\n", 60);
        let body = lines
            .iter()
            .find(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .contains("foo bar")
            })
            .expect("code body line");
        let text: String = body.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(
            text.strip_prefix("  ").map(str::trim_end),
            Some("foo bar"),
            "unknown language keeps the flat two-space-indented body: {text:?}"
        );
    }

    #[test]
    fn copy_label_cells_locate_copy_on_header_bar() {
        let lines = markdown_to_lines("```rust\nlet x = 1;\n```\n", 50);
        let header = lines
            .iter()
            .find(|l| {
                l.spans
                    .first()
                    .map(|s| s.content.starts_with('\u{250c}'))
                    .unwrap_or(false)
            })
            .expect("header bar");
        let (col, cells) = label_cells(header, " [Copy] ").expect("copy label present");
        assert_eq!(cells, "[Copy]".chars().count() as u16);
        // The cell at `col` on the rendered header must be the '[' of [Copy].
        let rendered: String = header
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert_eq!(
            rendered.chars().nth(col as usize),
            Some('['),
            "label_cells column must point at '[' of [Copy]: {rendered:?}"
        );
    }

    #[test]
    fn copy_region_recovers_full_highlighted_body() {
        let _g = theme::set_active_for_test(
            theme::theme_by_name("icy_blue").expect("icy_blue registered"),
        );
        let mut state = ChatState::new(
            "sess".to_string(),
            "agent".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        state.entries.push(ChatEntry::AgentMessage(Arc::<str>::from(
            "```rust\nfn main() {}\nlet y = 2;\n```\n",
        )));
        state.mark_dirty_full();
        state.rebuild_lines(60);
        let body = Rect::new(0, 0, 60, 20);
        state.rebuild_copy_regions(0, body);
        assert!(
            !state.copy_hit_regions.is_empty(),
            "a highlighted fence must still register copy regions"
        );
        assert_eq!(
            state.copy_hit_regions[0].text.as_ref(),
            "fn main() {}\nlet y = 2;",
            "copy text contains only the code body without markdown fences"
        );
    }

    #[test]
    fn copy_region_unlabeled_fence_omits_language() {
        let mut state = ChatState::new(
            "sess".to_string(),
            "agent".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        state.entries.push(ChatEntry::AgentMessage(Arc::<str>::from(
            "```\nplain text\n```\n",
        )));
        state.mark_dirty_full();
        state.rebuild_lines(60);
        let body = Rect::new(0, 0, 60, 20);
        state.rebuild_copy_regions(0, body);
        assert_eq!(
            state.copy_hit_regions[0].text.as_ref(),
            "plain text",
            "copy text contains only the code body without fences"
        );
    }

    #[test]
    fn copy_regions_track_scroll_with_history_above_viewport() {
        let _g = theme::set_active_for_test(
            theme::theme_by_name("icy_blue").expect("icy_blue registered"),
        );
        let mut state = ChatState::new(
            "sess".to_string(),
            "agent".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        let pad = "filler line\n".repeat(200);
        state
            .entries
            .push(ChatEntry::AgentMessage(Arc::<str>::from(pad.as_str())));
        state.entries.push(ChatEntry::AgentMessage(Arc::<str>::from(
            "```rust\nfn main() {}\n```\n",
        )));
        state.dirty = LinesDirty::Full;
        state.rebuild_lines(60);

        let fence_entry = state.cached_screen_ranges.last().copied().expect("fence");
        let body = Rect::new(0, 0, 60, 20);

        state.rebuild_copy_regions(fence_entry.1, body);
        assert_eq!(
            state.copy_hit_regions[0].text.as_ref(),
            "fn main() {}",
            "scrolled-to fence registers a copy region with body only"
        );

        state.rebuild_copy_regions(0, body);
        assert!(
            state.copy_hit_regions.is_empty(),
            "fence far below the viewport registers nothing"
        );
    }

    #[test]
    fn fenced_text_returns_body_without_markdown_fences() {
        assert_eq!(fenced_text(Some("python"), "x = 1"), "x = 1");
        assert_eq!(fenced_text(None, "x = 1"), "x = 1");
    }

    #[test]
    fn md_table_renders_box_drawing_borders() {
        let out = rendered("| A | B |\n|---|---|\n| 1 | 2 |\n", 40);
        assert!(out.contains('\u{250C}'), "missing top-left corner: {out}");
        assert!(
            out.contains('\u{2514}'),
            "missing bottom-left corner: {out}"
        );
        assert!(out.contains('\u{2502}'), "missing vertical: {out}");
        assert!(out.contains('A'));
        assert!(out.contains('1'));
    }

    #[test]
    fn md_table_truncates_when_width_is_tight() {
        let out = rendered(
            "| col |\n|-----|\n| this cell is far too long for a tiny width |\n",
            20,
        );
        assert!(out.contains('\u{2026}'), "expected ellipsis: {out}");
    }

    #[test]
    fn md_table_pads_emoji_presentation_to_two_cells() {
        // 🏔️ is U+1F3D4 + U+FE0F. Natural column width must be 2 (not 1), so a
        // wider sibling cell still leaves a full cell of space after the glyph.
        let out = rendered("| A | B |\n|---|---|\n| \u{1F3D4}\u{FE0F} | xx |\n", 40);
        let data = out
            .lines()
            .find(|l| l.contains('\u{1F3D4}'))
            .expect("emoji data row");
        let emoji = "\u{1F3D4}\u{FE0F}";
        let idx = data.find(emoji).expect("emoji in row");
        let after = &data[idx + emoji.len()..];
        // Column budget for A is max(width("A"), width(emoji)) = 2, so after
        // the emoji there is no content pad — only the trailing cell space
        // before the border.
        assert!(
            after.starts_with(" \u{2502}"),
            "emoji column natural width is 2 cells: {data:?}"
        );
        // And the header cell for A is padded to that same 2-cell budget.
        let header = out
            .lines()
            .find(|l| l.contains('A') && l.contains('B'))
            .expect("header row");
        assert!(
            header.contains(" A  "),
            "header A cell must pad to emoji's 2-cell width: {header:?}"
        );
    }

    #[test]
    fn md_heading_emits_gutter_for_h1() {
        let out = rendered("# Title\n", 80);
        assert!(out.contains('\u{258C}'), "expected H1 gutter: {out}");
        assert!(out.contains("Title"));
    }

    #[test]
    fn md_plain_text_uses_theme_body_style() {
        let out = markdown_to_lines("plain assistant text\n", 80);
        assert_eq!(out[0].spans[0].style, theme::body_style());
    }

    #[test]
    fn md_blockquote_prefixes_each_line() {
        let out = rendered("> quoted text\n", 80);
        assert!(
            out.contains('\u{2502}'),
            "expected blockquote gutter: {out}"
        );
        assert!(out.contains("quoted text"));
    }

    #[test]
    fn md_link_appends_url_inline() {
        let out = rendered("[click](https://example.com)\n", 80);
        assert!(out.contains("click"));
        assert!(out.contains("https://example.com"));
    }

    #[test]
    fn md_strikethrough_passes_text_through() {
        // Style flag isn't visible in plain text join, but the text must
        // still render — proves the parser option is enabled.
        let out = rendered("~~gone~~\n", 80);
        assert!(out.contains("gone"));
    }

    #[test]
    fn md_task_list_renders_checkbox_glyphs() {
        let out = rendered("- [x] done\n- [ ] todo\n", 80);
        assert!(out.contains('\u{2611}'), "expected checked glyph: {out}");
        assert!(out.contains('\u{2610}'), "expected unchecked glyph: {out}");
    }

    #[test]
    fn md_ordered_list_renders_numbers_not_bullets() {
        let out = rendered("1. first\n2. second\n3. third\n", 80);
        assert!(out.contains("1. first"), "expected ordinal 1: {out}");
        assert!(out.contains("2. second"), "expected ordinal 2: {out}");
        assert!(out.contains("3. third"), "expected ordinal 3: {out}");
        assert!(
            !out.contains('\u{2022}'),
            "ordered list must not render bullets: {out}"
        );
    }

    #[test]
    fn md_ordered_list_honors_start_offset() {
        let out = rendered("5. five\n6. six\n", 80);
        assert!(out.contains("5. five"), "expected start at 5: {out}");
        assert!(out.contains("6. six"), "expected continuation 6: {out}");
    }

    #[test]
    fn md_unordered_list_still_renders_bullets() {
        let out = rendered("- one\n- two\n", 80);
        assert!(out.contains('\u{2022}'), "expected bullet glyph: {out}");
    }

    #[test]
    fn md_table_with_no_width_still_emits_lines() {
        // Defensive: zero width must not panic and must not emit infinite
        // padding. The truncation rule collapses every column to `…`.
        let out = markdown_to_lines("| A |\n|---|\n| 1 |\n", 0);
        assert!(!out.is_empty());
    }

    fn att(name: &str) -> PendingAttachment {
        PendingAttachment {
            path: std::path::PathBuf::from(format!("/tmp/{name}")),
            mime_type: "text/plain".to_string(),
            filename: name.to_string(),
            size_bytes: 1,
            source: crate::attachment::AttachmentSource::File,
        }
    }

    fn clipboard_att(path: &std::path::Path, filename: &str) -> PendingAttachment {
        PendingAttachment {
            path: path.to_path_buf(),
            mime_type: "image/png".to_string(),
            filename: filename.to_string(),
            size_bytes: 1,
            source: crate::attachment::AttachmentSource::Clipboard,
        }
    }

    #[tokio::test]
    async fn explicit_close_orders_after_any_started_entry_retry() {
        for session_new_started in [false, true] {
            let (tx, mut rx) = mpsc::channel::<String>(16);
            let rpc = Arc::new(RpcOutbound::new(tx));
            let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
            let mut chat = Chat::new(client, PaneKind::Chat);
            chat.phase = ChatPhase::Error("retry reconnect".into());
            // Keep a resident survivor so close does not intentionally create a
            // fresh session through the empty-pane initialization path.
            chat.background.push(state_for("survivor", "beta"));
            chat.resume_focused = Some(resume_entry("target", "alpha", true));
            chat.session_order = vec!["target".into(), "survivor".into()];
            chat.start_entry_retry();
            let agents = next_rpc_request(&mut rx, "retry queries agents").await;
            assert_eq!(agents["method"], method::AGENTS_STATUS);
            let pending_new = if session_new_started {
                respond_ok(
                    &rpc,
                    &agents,
                    serde_json::json!({
                        "agents": [{"alias": "alpha", "enabled": true}]
                    }),
                );
                let request = next_rpc_request(&mut rx, "retry starts session/new").await;
                assert_eq!(request["method"], method::SESSION_NEW);
                Some(request)
            } else {
                None
            };

            let close = tokio::spawn(async move {
                assert!(chat.close_session("target").await);
                chat
            });
            if let Some(request) = pending_new {
                assert!(
                    tokio::time::timeout(Duration::from_millis(50), rx.recv())
                        .await
                        .is_err(),
                    "close must wait for a started session/new to settle"
                );
                respond_ok(
                    &rpc,
                    &request,
                    serde_json::json!({
                        "session_id": "target", "workspace_dir": "/w"
                    }),
                );
            }
            let request = next_rpc_request(&mut rx, "close follows cancelled retry").await;
            assert_eq!(request["method"], method::SESSION_CLOSE);
            assert_eq!(request["params"]["session_id"], "target");
            respond_ok(&rpc, &request, serde_json::Value::Null);
            let chat = close.await.unwrap();
            assert!(
                !chat
                    .session_summaries()
                    .iter()
                    .any(|s| s.session_id == "target")
            );
            assert_eq!(chat.current_session_id(), Some("survivor"));
            assert_eq!(rpc.pending_count(), 0);
            assert!(
                tokio::time::timeout(Duration::from_millis(50), rx.recv())
                    .await
                    .is_err(),
                "no retry may recreate the ID after the close acknowledgement"
            );
        }
    }

    #[tokio::test]
    async fn explicit_close_cleans_only_acknowledged_session_clipboard_owners() {
        for retained in [false, true] {
            for focused in [false, true] {
                for succeeds in [false, true] {
                    let dir = tempfile::tempdir().unwrap();
                    let queue_path = dir.path().join("queue.png");
                    let user_path = dir.path().join("user.png");
                    let survivor_path = dir.path().join("survivor.png");
                    for path in [&queue_path, &user_path, &survivor_path] {
                        std::fs::write(path, b"fixture").unwrap();
                    }
                    let mut target = state_for("target", "alpha");
                    let mut user_attachment = clipboard_att(&user_path, "user.png");
                    user_attachment.source = crate::attachment::AttachmentSource::File;
                    target
                        .enqueue_message(
                            "queued".into(),
                            vec![clipboard_att(&queue_path, "queue.png"), user_attachment],
                        )
                        .unwrap();
                    let mut owned_paths = vec![queue_path];
                    if !retained {
                        let active_path = dir.path().join("active.png");
                        let composer_path = dir.path().join("composer.png");
                        std::fs::write(&active_path, b"fixture").unwrap();
                        std::fs::write(&composer_path, b"fixture").unwrap();
                        target.active_turn_attachments =
                            vec![clipboard_att(&active_path, "active.png")];
                        target.input_bar.load_for_edit(
                            String::new(),
                            vec![clipboard_att(&composer_path, "composer.png")],
                        );
                        owned_paths.extend([active_path, composer_path]);
                    }
                    let mut survivor = state_for("survivor", "beta");
                    survivor.active_turn_attachments =
                        vec![clipboard_att(&survivor_path, "survivor.png")];
                    let (tx, mut rx) = mpsc::channel::<String>(16);
                    let rpc = Arc::new(RpcOutbound::new(tx));
                    let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
                    let mut chat = Chat::new(client, PaneKind::Chat);
                    chat.session_order = vec!["target".into(), "survivor".into()];
                    if retained {
                        let mut entry = resume_entry("target", "alpha", focused);
                        entry.queue = target.reconnect_queue_state();
                        if focused {
                            chat.phase = ChatPhase::Error("resume failed".into());
                            chat.resume_focused = Some(entry);
                            chat.background.push(survivor);
                        } else {
                            chat.phase = ChatPhase::Active(Box::new(survivor));
                            chat.resume_backgrounds.push(entry);
                        }
                    } else if focused {
                        chat.phase = ChatPhase::Active(Box::new(target));
                        chat.background.push(survivor);
                    } else {
                        chat.phase = ChatPhase::Active(Box::new(survivor));
                        chat.background.push(target);
                    }

                    let close = tokio::spawn(async move {
                        let result = chat.close_session("target").await;
                        (chat, result)
                    });
                    let request =
                        next_rpc_request(&mut rx, "close waits for daemon acknowledgement").await;
                    assert_eq!(request["method"], method::SESSION_CLOSE);
                    assert!(owned_paths.iter().all(|path| path.exists()));
                    if succeeds {
                        respond_ok(&rpc, &request, serde_json::Value::Null);
                    } else {
                        respond_err(
                            &rpc,
                            &request,
                            crate::jsonrpc::error_codes::INTERNAL_ERROR,
                            "close failed",
                        );
                    }
                    let (chat, result) = close.await.unwrap();
                    assert_eq!(result, succeeds);
                    assert!(
                        owned_paths.iter().all(|path| path.exists() != succeeds),
                        "cleanup must follow acknowledgement for retained={retained}, focused={focused}"
                    );
                    assert!(
                        user_path.exists(),
                        "user-selected files are never owned temps"
                    );
                    assert!(
                        survivor_path.exists(),
                        "another session retains its attachments"
                    );
                    assert_eq!(
                        chat.session_summaries()
                            .iter()
                            .any(|s| s.session_id == "target"),
                        !succeeds
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn explicit_close_reports_aggregate_cleanup_failures_on_surviving_session() {
        let dir = tempfile::tempdir().unwrap();
        let active_path = dir.path().join("active-unremovable");
        let queued_path = dir.path().join("queued-unremovable");
        std::fs::create_dir(&active_path).unwrap();
        std::fs::create_dir(&queued_path).unwrap();
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&rpc);
        let target = chat.state_for_session_mut("sess-a").unwrap();
        target.active_turn_attachments = vec![clipboard_att(&active_path, "active.png")];
        target
            .enqueue_message(
                "queued".into(),
                vec![clipboard_att(&queued_path, "queued.png")],
            )
            .unwrap();
        let close = tokio::spawn(async move {
            assert!(chat.close_session("sess-a").await);
            chat
        });
        let request = next_rpc_request(&mut rx, "close acknowledges before cleanup").await;
        respond_ok(&rpc, &request, serde_json::Value::Null);
        let chat = close.await.unwrap();
        let notice = &chat
            .state_for_session("sess-b")
            .unwrap()
            .info_message
            .as_ref()
            .unwrap()
            .text;
        assert_eq!(
            notice,
            &crate::i18n::t_args("zc-input-clipboard-cleanup-error", &[("count", "2")])
        );
        assert!(!notice.contains(dir.path().to_string_lossy().as_ref()));
        assert!(active_path.exists() && queued_path.exists());
    }

    #[tokio::test]
    async fn dispatched_attachments_are_cleaned_on_completion_without_touching_user_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clipboard_path = dir.path().join("clipboard.png");
        std::fs::write(&clipboard_path, b"clipboard").expect("write clipboard temp");
        let user_path = dir.path().join("user.png");
        std::fs::write(&user_path, b"user").expect("write user file");

        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc_transport(
            rpc,
            crate::client::Transport::Wss,
        ));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active
            .enqueue_message(
                "send attachments".to_string(),
                vec![
                    clipboard_att(&clipboard_path, "clipboard.png"),
                    PendingAttachment {
                        path: user_path.clone(),
                        mime_type: "image/png".to_string(),
                        filename: "user.png".to_string(),
                        size_bytes: 4,
                        source: crate::attachment::AttachmentSource::File,
                    },
                ],
            )
            .unwrap();
        chat.phase = ChatPhase::Active(Box::new(active));

        chat.pump_all_queues();

        let ChatPhase::Active(state) = &mut chat.phase else {
            panic!("expected active chat");
        };
        assert!(state.turn_in_flight);
        assert_eq!(state.active_turn_attachments.len(), 2);
        assert!(clipboard_path.exists());
        state.commit_turn(String::new(), true);

        assert!(
            !clipboard_path.exists(),
            "active clipboard temp must be removed"
        );
        assert!(user_path.exists(), "user-selected file must be preserved");
        assert!(state.active_turn_attachments.is_empty());
    }

    #[tokio::test]
    async fn serialization_failure_cleans_dispatched_clipboard_temp_and_reports_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clipboard_path = dir.path().join("clipboard-temp");
        std::fs::create_dir(&clipboard_path).expect("create forced-failure path");
        let user_path = dir.path().join("user.png");
        std::fs::write(&user_path, b"user").expect("write user file");

        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc_transport(
            rpc,
            crate::client::Transport::Wss,
        ));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active
            .enqueue_message(
                "cannot serialize".to_string(),
                vec![
                    clipboard_att(&clipboard_path, "clipboard.png"),
                    PendingAttachment {
                        path: user_path.clone(),
                        mime_type: "image/png".to_string(),
                        filename: "user.png".to_string(),
                        size_bytes: 4,
                        source: crate::attachment::AttachmentSource::File,
                    },
                ],
            )
            .unwrap();
        chat.phase = ChatPhase::Active(Box::new(active));

        chat.pump_all_queues();

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert!(!state.turn_in_flight);
        assert!(state.active_turn_attachments.is_empty());
        assert!(
            clipboard_path.exists(),
            "failed cleanup must leave the path"
        );
        assert!(user_path.exists(), "user-selected file must be preserved");
        assert!(
            state
                .info_message
                .as_ref()
                .is_some_and(|message| message.text.contains("1 temporary file"))
        );
        let clipboard_path = clipboard_path.to_string_lossy();
        assert!(state.entries.iter().all(|entry| match entry {
            ChatEntry::SystemMessage(text) => !text.contains(clipboard_path.as_ref()),
            _ => true,
        }));
    }

    #[test]
    fn notification_recovery_cleans_active_clipboard_attachment() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clipboard_path = dir.path().join("notification-recovery.png");
        std::fs::write(&clipboard_path, b"clipboard").expect("write clipboard temp");
        let mut active = state();
        active.active_turn_attachments =
            vec![clipboard_att(&clipboard_path, "notification-recovery.png")];

        active.prepare_for_notification_resync();

        assert!(active.active_turn_attachments.is_empty());
        assert!(
            !clipboard_path.exists(),
            "notification recovery must reclaim the abandoned turn's clipboard temp"
        );
    }

    #[tokio::test]
    async fn reconnect_handoff_cleans_active_clipboard_attachment() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clipboard_path = dir.path().join("reconnect-handoff.png");
        std::fs::write(&clipboard_path, b"clipboard").expect("write clipboard temp");
        let (tx, _rx) = mpsc::channel::<String>(16);
        let outbound = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(outbound));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active.active_turn_attachments =
            vec![clipboard_att(&clipboard_path, "reconnect-handoff.png")];
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.session_order = vec!["sess-1".to_string()];

        chat.commit_reconnect_handoff();

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("handoff snapshot remains readable");
        };
        assert!(state.active_turn_attachments.is_empty());
        assert!(
            !clipboard_path.exists(),
            "reconnect handoff must reclaim the abandoned turn's clipboard temp"
        );
    }

    #[test]
    fn completion_does_not_delete_an_unsent_composer_attachment() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clipboard_path = dir.path().join("composer.png");
        std::fs::write(&clipboard_path, b"clipboard").expect("write clipboard temp");

        let mut active = state();
        active.input_bar.load_for_edit(
            String::new(),
            vec![clipboard_att(&clipboard_path, "composer.png")],
        );
        active.turn_in_flight = true;

        active.commit_turn(String::new(), false);

        assert!(clipboard_path.exists());
        assert_eq!(active.input_bar.pending_attachments().len(), 1);
    }

    #[test]
    fn enqueue_dispatches_immediately_when_idle() {
        let mut s = state();
        s.enqueue_message("hello".to_string(), Vec::new()).unwrap();
        assert_eq!(s.queue_len(), 1);
        let msg = s
            .take_next_dispatchable()
            .expect("idle queue must dispatch");
        assert_eq!(msg.text, "hello");
        assert_eq!(s.queue_len(), 0);
    }

    #[test]
    fn select_queued_by_id_sets_selection() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("a".to_string(), Vec::new()).unwrap();
        s.enqueue_message("b".to_string(), Vec::new()).unwrap();
        let second = s.message_queue[1].id;
        assert!(s.select_queued_by_id(second));
        assert_eq!(s.queue_sel, Some(second));
        // Re-selecting the same id reports no change.
        assert!(!s.select_queued_by_id(second));
        // Unknown id is ignored.
        assert!(!s.select_queued_by_id(9999));
        assert_eq!(s.queue_sel, Some(second));
    }

    #[test]
    fn queue_scroll_by_clamps_at_zero() {
        let mut s = state();
        s.queue_scroll_by(-5);
        assert_eq!(s.queue_scroll, 0);
        s.queue_scroll_by(4);
        assert_eq!(s.queue_scroll, 4);
        s.queue_scroll_by(-10);
        assert_eq!(s.queue_scroll, 0);
    }

    #[test]
    fn no_dispatch_while_turn_in_flight() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("a".to_string(), Vec::new()).unwrap();
        s.enqueue_message("b".to_string(), Vec::new()).unwrap();
        assert!(s.take_next_dispatchable().is_none());
        assert_eq!(s.queue_len(), 2);
    }

    #[test]
    fn fifo_order_preserved() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("first".to_string(), Vec::new()).unwrap();
        s.enqueue_message("second".to_string(), Vec::new()).unwrap();
        s.turn_in_flight = false;
        assert_eq!(s.take_next_dispatchable().unwrap().text, "first");
        assert_eq!(s.take_next_dispatchable().unwrap().text, "second");
    }

    #[test]
    fn injection_jumps_ahead_of_pending() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("pending1".to_string(), Vec::new())
            .unwrap();
        s.enqueue_message("pending2".to_string(), Vec::new())
            .unwrap();
        s.inject_message("urgent".to_string(), Vec::new()).unwrap();
        s.turn_in_flight = false;
        assert_eq!(s.take_next_dispatchable().unwrap().text, "urgent");
        assert_eq!(s.take_next_dispatchable().unwrap().text, "pending1");
    }

    #[test]
    fn queue_action_send_now_preserves_payload_and_injected_fifo() {
        let mut s = state();
        s.turn_in_flight = true;
        s.inject_message("already urgent".to_string(), Vec::new())
            .unwrap();
        s.enqueue_message("ordinary one".to_string(), Vec::new())
            .unwrap();
        s.enqueue_message("promote me".to_string(), vec![att("keep.txt")])
            .unwrap();
        s.enqueue_message("ordinary two".to_string(), Vec::new())
            .unwrap();
        let promoted_id = s.message_queue[2].id;
        s.queue_paused = true;

        assert!(s.promote_queued_by_id(promoted_id));
        assert!(!s.queue_paused());
        assert!(s.resume_override);
        assert_eq!(
            s.message_queue
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "already urgent",
                "promote me",
                "ordinary one",
                "ordinary two"
            ]
        );
        let promoted = &s.message_queue[1];
        assert_eq!(promoted.id, promoted_id);
        assert_eq!(promoted.status, QueueItemStatus::Injected);
        assert_eq!(promoted.attachments.len(), 1);
        assert_eq!(promoted.attachments[0].filename, "keep.txt");
    }

    #[test]
    fn queue_action_send_now_is_idempotent_for_injected_items() {
        let mut s = state();
        s.turn_in_flight = true;
        s.inject_message("first".to_string(), Vec::new()).unwrap();
        s.inject_message("second".to_string(), Vec::new()).unwrap();
        let second_id = s.message_queue[1].id;

        assert!(s.promote_queued_by_id(second_id));
        assert!(s.promote_queued_by_id(second_id));
        assert_eq!(
            s.message_queue
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[test]
    fn queue_action_menu_targets_clicked_id_and_orders_actions() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("first".to_string(), Vec::new()).unwrap();
        s.enqueue_message("second".to_string(), Vec::new()).unwrap();
        let second_id = s.message_queue[1].id;
        s.queue_sidebar_rect = Some(Rect::new(40, 2, 30, 12));
        s.queue_item_rects = vec![
            (s.message_queue[0].id, Rect::new(41, 3, 28, 2)),
            (second_id, Rect::new(41, 5, 28, 2)),
        ];

        assert!(s.open_queue_context_menu(45, 5));
        assert_eq!(s.queue_sel, Some(second_id));
        let menu = s.context_menu.as_ref().expect("queue menu opens");
        assert_eq!(menu.target.actions(), QUEUE_CONTEXT_ACTIONS);
        assert!(matches!(menu.target, ChatContextMenuTarget::Queue(id) if id == second_id));

        s.context_menu_select_step(1);
        assert_eq!(
            s.take_context_menu_request(),
            Some(ChatContextMenuRequest::Queue {
                id: second_id,
                action: ChatContextMenuAction::Copy,
            })
        );
    }

    #[test]
    fn queue_action_menu_navigation_clamps_at_boundaries() {
        let target = ChatContextMenuTarget::Queue(1);
        let mut menu = ChatContextMenu {
            rect: Rect::new(0, 0, 16, 6),
            target,
            selected: 0,
        };

        menu.select_step(-1);
        assert_eq!(menu.selected_action(), Some(ChatContextMenuAction::SendNow));
        menu.selected = QUEUE_CONTEXT_ACTIONS.len() - 1;
        menu.select_step(1);
        assert_eq!(menu.selected_action(), Some(ChatContextMenuAction::Delete));
    }

    #[test]
    fn queue_action_copy_lookup_does_not_mutate_queue() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("copy me".to_string(), vec![att("keep.txt")])
            .unwrap();
        s.ensure_queue_selection();
        let id = s.selected_queue_id().unwrap();
        let before_selection = s.queue_sel;

        assert_eq!(s.queued_text(id).as_deref(), Some("copy me"));
        assert_eq!(s.queue_len(), 1);
        assert_eq!(s.queue_sel, before_selection);
        assert_eq!(s.message_queue[0].id, id);
        assert_eq!(s.message_queue[0].status, QueueItemStatus::Pending);
        assert_eq!(s.message_queue[0].attachments[0].filename, "keep.txt");
    }

    #[tokio::test]
    async fn queue_action_send_now_requests_cancel_only_once() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active.turn_in_flight = true;
        active
            .enqueue_message("send now".to_string(), Vec::new())
            .unwrap();
        let id = active.message_queue[0].id;
        chat.phase = ChatPhase::Active(Box::new(active));

        let first = tokio::spawn(async move {
            chat.execute_context_menu_request(ChatContextMenuRequest::Queue {
                id,
                action: ChatContextMenuAction::SendNow,
            })
            .await;
            chat
        });
        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("send now should request cancellation")
            .expect("writer channel open");
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], method::SESSION_CANCEL);
        let request_id = request["id"].as_str().unwrap().to_string();
        rpc.dispatch_response(
            &request_id,
            Some(serde_json::json!({"session_id":"sess-1","cancelled":true})),
            None,
        );
        let mut chat = tokio::time::timeout(Duration::from_secs(2), first)
            .await
            .expect("send now should finish after cancel response")
            .unwrap();

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert!(matches!(state.turn_status, TurnStatus::Cancelling));
        assert_eq!(state.message_queue[0].status, QueueItemStatus::Injected);

        chat.execute_context_menu_request(ChatContextMenuRequest::Queue {
            id,
            action: ChatContextMenuAction::SendNow,
        })
        .await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "an already-cancelling turn must not emit another cancel request"
        );
    }

    #[tokio::test]
    async fn queue_action_send_now_dispatches_after_cancel_failure() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(Arc::clone(&rpc)));
        let mut chat = Chat::new(client, PaneKind::Chat);
        let mut active = state();
        active.turn_in_flight = true;
        active
            .enqueue_message("recover me".to_string(), Vec::new())
            .unwrap();
        let id = active.message_queue[0].id;
        chat.phase = ChatPhase::Active(Box::new(active));

        let action = tokio::spawn(async move {
            chat.execute_context_menu_request(ChatContextMenuRequest::Queue {
                id,
                action: ChatContextMenuAction::SendNow,
            })
            .await;
            chat
        });
        let cancel_line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("send now should request cancellation")
            .expect("writer channel open");
        let cancel: serde_json::Value = serde_json::from_str(&cancel_line).unwrap();
        let request_id = cancel["id"].as_str().unwrap().to_string();
        rpc.dispatch_response(
            &request_id,
            None,
            Some(crate::jsonrpc::JsonRpcError {
                code: -32000,
                message: "cancel failed".to_string(),
                data: None,
            }),
        );
        let chat = tokio::time::timeout(Duration::from_secs(2), action)
            .await
            .expect("failed cancellation should settle locally")
            .unwrap();
        let prompt_line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("promoted item should dispatch after cancel failure")
            .expect("writer channel open");
        let prompt: serde_json::Value = serde_json::from_str(&prompt_line).unwrap();
        assert_eq!(prompt["method"], method::SESSION_PROMPT);
        assert_eq!(prompt["params"]["prompt"], "recover me");

        let ChatPhase::Active(state) = &chat.phase else {
            panic!("expected active chat");
        };
        assert!(state.turn_in_flight);
        assert!(!state.queue_paused());
        assert!(state.message_queue.is_empty());
    }

    #[test]
    fn cancel_pauses_pending_but_injection_resumes() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("queued".to_string(), Vec::new()).unwrap();
        s.commit_turn(String::new(), false);
        assert!(s.queue_paused());
        assert!(
            s.take_next_dispatchable().is_none(),
            "paused queue must not dispatch pending items"
        );
        s.inject_message("override".to_string(), Vec::new())
            .unwrap();
        assert!(
            !s.queue_paused(),
            "an explicit inject (Ctrl+Enter) resumes the whole queue"
        );
        assert_eq!(
            s.take_next_dispatchable().unwrap().text,
            "override",
            "injected item dispatches first"
        );
        assert_eq!(
            s.take_next_dispatchable().unwrap().text,
            "queued",
            "pending then flows because the inject unpaused the queue"
        );
    }

    #[test]
    fn clean_completion_does_not_pause() {
        let mut s = state();
        s.turn_in_flight = true;
        s.commit_turn(String::new(), true);
        assert!(!s.queue_paused());
    }

    #[test]
    fn empty_enqueue_rejected() {
        let mut s = state();
        assert!(s.enqueue_message("   ".to_string(), Vec::new()).is_err());
        assert!(s.inject_message(String::new(), Vec::new()).is_err());
        assert_eq!(s.queue_len(), 0);
    }

    #[test]
    fn attachment_only_enqueue_accepted() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message(String::new(), vec![att("a.txt")])
            .unwrap();
        assert_eq!(s.queue_len(), 1);
    }

    #[test]
    fn queue_sidebar_open_tracks_contents() {
        let mut s = state();
        s.turn_in_flight = true;
        assert!(!s.queue_sidebar_open(), "empty queue → sidebar closed");
        s.enqueue_message("a".to_string(), Vec::new()).unwrap();
        assert!(s.queue_sidebar_open(), "non-empty queue → sidebar open");
        s.ensure_queue_selection();
        assert!(s.queue_sel.is_some(), "first enqueue seeds a selection");
        s.delete_selected_queued();
        assert!(
            !s.queue_sidebar_open(),
            "draining the queue closes the sidebar"
        );
    }

    #[test]
    fn delete_selected_removes_item() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("a".to_string(), Vec::new()).unwrap();
        s.enqueue_message("b".to_string(), Vec::new()).unwrap();
        s.ensure_queue_selection();
        s.delete_selected_queued();
        assert_eq!(s.queue_len(), 1);
    }

    #[test]
    fn edit_pull_removes_from_queue_and_returns_content() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("draft".to_string(), vec![att("x.txt")])
            .unwrap();
        s.ensure_queue_selection();
        let (text, atts) = s.take_selected_for_edit().expect("selected item");
        assert_eq!(text, "draft");
        assert_eq!(atts.len(), 1);
        assert_eq!(s.queue_len(), 0);
    }

    #[test]
    fn clear_queue_cmd_removes_one_by_index() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("a".to_string(), Vec::new()).unwrap();
        s.enqueue_message("b".to_string(), Vec::new()).unwrap();
        s.enqueue_message("c".to_string(), Vec::new()).unwrap();
        // 1-based: remove the second item ("b").
        s.clear_queue_cmd(Some(2));
        assert_eq!(s.queue_len(), 2);
        s.turn_in_flight = false;
        assert_eq!(s.take_next_dispatchable().unwrap().text, "a");
        assert_eq!(s.take_next_dispatchable().unwrap().text, "c");
    }

    #[test]
    fn clear_queue_cmd_none_clears_all() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("a".to_string(), Vec::new()).unwrap();
        s.enqueue_message("b".to_string(), Vec::new()).unwrap();
        s.clear_queue_cmd(None);
        assert_eq!(s.queue_len(), 0);
    }

    #[test]
    fn clear_queue_cmd_invalid_index_is_a_noop() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("a".to_string(), Vec::new()).unwrap();
        // Out of range and the Some(0) sentinel must not remove anything.
        s.clear_queue_cmd(Some(9));
        s.clear_queue_cmd(Some(0));
        assert_eq!(s.queue_len(), 1);
    }

    #[test]
    fn non_clean_commit_with_empty_queue_does_not_pause() {
        let mut s = state();
        s.turn_in_flight = true;
        s.commit_turn(String::new(), false);
        assert!(
            !s.queue_paused(),
            "cancel/fail with no queued backlog must not show queue-paused state"
        );
    }

    #[test]
    fn resume_queue_unpauses_and_reports_prior_state() {
        let mut s = state();
        s.enqueue_message("queued".to_string(), Vec::new()).unwrap();
        s.commit_turn(String::new(), false);
        assert!(s.queue_paused(), "non-clean turn end must pause");
        assert!(
            s.resume_queue(),
            "resume_queue returns true when it was paused"
        );
        assert!(!s.queue_paused());
        assert!(
            !s.resume_queue(),
            "resume_queue returns false when already running"
        );
    }

    #[test]
    fn resume_then_dispatch_after_auto_pause() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("queued".to_string(), Vec::new()).unwrap();
        // Turn cancelled/failed mid-flight -> auto-pause.
        s.commit_turn(String::new(), false);
        assert!(s.take_next_dispatchable().is_none(), "paused: no dispatch");
        s.resume_queue();
        assert_eq!(s.take_next_dispatchable().unwrap().text, "queued");
    }

    #[test]
    fn enter_during_cancel_pauses_queue() {
        let mut s = state();
        s.turn_in_flight = true;
        s.resume_queue();
        s.enqueue_message("hello".to_string(), Vec::new()).unwrap();
        s.commit_turn(String::new(), false);
        assert!(
            s.queue_paused(),
            "a plain-Enter submission mid-turn must not bypass the cancel auto-pause"
        );
        assert!(
            s.take_next_dispatchable().is_none(),
            "the cancelled turn pauses the queue; the backlog waits for a deliberate resume"
        );
    }

    #[test]
    fn inject_survives_cancel_auto_pause() {
        let mut s = state();
        s.turn_in_flight = true;
        s.inject_message("now".to_string(), Vec::new()).unwrap();
        s.commit_turn(String::new(), false);
        assert_eq!(
            s.take_next_dispatchable().unwrap().text,
            "now",
            "an inject is the only intent that survives a cancel"
        );
    }

    #[test]
    fn inject_resume_override_is_one_shot() {
        let mut s = state();
        s.turn_in_flight = true;
        s.inject_message("a".to_string(), Vec::new()).unwrap();
        s.commit_turn(String::new(), false);
        assert_eq!(s.take_next_dispatchable().unwrap().text, "a");
        s.turn_in_flight = true;
        s.enqueue_message("b".to_string(), Vec::new()).unwrap();
        s.commit_turn(String::new(), false);
        assert!(
            s.queue_paused(),
            "a stale inject override must not leak into the next cancelled turn"
        );
    }

    #[test]
    fn enter_cancelling_arms_watchdog_and_commit_disarms() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enter_cancelling();
        assert!(matches!(s.turn_status, TurnStatus::Cancelling));
        assert!(s.cancel_started_at.is_some());
        s.commit_turn(String::new(), false);
        assert!(matches!(s.turn_status, TurnStatus::Idle));
        assert!(
            s.cancel_started_at.is_none(),
            "commit must disarm the cancel watchdog"
        );
        assert!(!s.cancel_watchdog_expired());
    }

    #[test]
    fn cancel_watchdog_expires_after_bound() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enter_cancelling();
        assert!(!s.cancel_watchdog_expired(), "fresh cancel is not expired");
        s.cancel_started_at = Some(Instant::now() - CANCEL_WATCHDOG);
        assert!(
            s.cancel_watchdog_expired(),
            "a cancel with no TurnComplete past the bound must be reported stuck"
        );
    }

    #[test]
    fn idle_session_never_reports_stuck_cancel() {
        let mut s = state();
        s.cancel_started_at = Some(Instant::now() - CANCEL_WATCHDOG);
        assert!(
            !s.cancel_watchdog_expired(),
            "watchdog only fires while status is Cancelling"
        );
    }

    #[test]
    fn info_notice_set_and_cleared_without_touching_entries() {
        let mut s = state();
        let before = s.entries.len();
        s.set_info_notice("Detached: clipboard_123.png".to_string());
        assert_eq!(
            s.info_message.as_ref().map(|m| m.text.as_str()),
            Some("Detached: clipboard_123.png")
        );
        assert_eq!(
            s.entries.len(),
            before,
            "info notice must not enter history"
        );
        s.clear_info_notice();
        assert!(s.info_message.is_none());
        assert_eq!(s.entries.len(), before);
    }

    #[test]
    fn reset_clears_queue() {
        let mut s = state();
        s.turn_in_flight = true;
        s.enqueue_message("a".to_string(), Vec::new()).unwrap();
        s.queue_paused = true;
        s.copy_hit_regions.push(CopyHitRegion {
            rect: Rect::new(1, 1, 6, 1),
            text: Arc::<str>::from("stale"),
            kind: CopyHitKind::Message,
            group: 0,
        });
        s.copy_feedback = Some(CopyFeedback {
            target: CopyFeedbackTarget::Overlay(Rect::new(1, 1, 8, 1)),
            shown_at: Instant::now(),
        });
        s.reset_for_session(
            "sess-2".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        assert_eq!(s.queue_len(), 0);
        assert!(!s.queue_paused());
        assert!(
            s.copy_hit_regions.is_empty(),
            "session reset must clear stale copy hit regions"
        );
        assert!(
            s.copy_feedback.is_none(),
            "session reset must clear stale copy feedback"
        );
    }

    #[test]
    fn toggle_queue_pause_flips_state() {
        let mut s = state();
        assert!(!s.queue_paused());
        assert!(s.toggle_queue_pause());
        assert!(s.queue_paused());
        assert!(!s.toggle_queue_pause());
        assert!(!s.queue_paused());
    }

    #[test]
    fn queue_cap_enforced() {
        let mut s = state();
        s.turn_in_flight = true;
        for i in 0..ChatState::QUEUE_CAP {
            s.enqueue_message(format!("m{i}"), Vec::new()).unwrap();
        }
        assert!(
            s.enqueue_message("overflow".to_string(), Vec::new())
                .is_err()
        );
    }

    #[test]
    fn page_and_jump_scroll_move_the_viewport() {
        let mut s = state();
        s.last_total_rows = 100;
        s.last_inner_height = 10;
        s.scroll_to_bottom();
        let bottom = s.scroll_offset;
        assert_eq!(bottom, 90);
        assert!(s.pinned_to_bottom);

        s.page_up();
        assert_eq!(s.scroll_offset, 80);
        assert!(!s.pinned_to_bottom);

        s.scroll_to_top();
        assert_eq!(s.scroll_offset, 0);
        assert!(!s.pinned_to_bottom);

        s.page_down();
        assert_eq!(s.scroll_offset, 10);

        s.scroll_to_bottom();
        assert_eq!(s.scroll_offset, bottom);
        assert!(s.pinned_to_bottom);
    }

    #[test]
    fn queue_sidebar_resize_clamps_to_bounds() {
        let mut s = state();
        for _ in 0..40 {
            s.widen_queue_sidebar();
        }
        assert_eq!(s.queue_sidebar_cols, ChatState::QUEUE_SIDEBAR_COLS_MAX);
        for _ in 0..40 {
            s.narrow_queue_sidebar();
        }
        assert_eq!(s.queue_sidebar_cols, ChatState::QUEUE_SIDEBAR_COLS_MIN);
    }

    #[test]
    fn queue_sidebar_narrow_then_widen_responds_immediately() {
        let mut s = state();
        s.narrow_queue_sidebar();
        s.narrow_queue_sidebar();
        let narrowed = s.queue_sidebar_width(200);
        s.widen_queue_sidebar();
        assert!(
            s.queue_sidebar_width(200) > narrowed,
            "one widen after narrowing must increase width, not burn a banked deficit"
        );
    }

    #[test]
    fn queue_sidebar_width_respects_absolute_clamps() {
        let s = state();
        let wide = s.queue_sidebar_width(400);
        assert!(
            wide <= ChatState::QUEUE_SIDEBAR_COLS_MAX,
            "sidebar exceeded absolute column cap"
        );
        // Narrow terminal: chat column keeps its minimum, sidebar shrinks.
        let tight = s.queue_sidebar_width(40);
        assert!(
            tight <= 40u16.saturating_sub(ChatState::QUEUE_CHAT_COLS_MIN),
            "sidebar starved the chat column on a narrow terminal"
        );
    }

    #[test]
    fn title_includes_short_session_hash() {
        let s = ChatState::new(
            "40be7731122334455".to_string(),
            "personal_code".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        assert_eq!(s.title(), "personal_code  40be773");
    }

    #[test]
    fn title_with_session_name_keeps_hash() {
        let mut s = ChatState::new(
            "40be7731122334455".to_string(),
            "personal_code".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        s.session_name = Some("my work".to_string());
        assert_eq!(s.title(), "personal_code  — my work  40be773");
    }

    fn pinned_preview_buffer(message: &str, width: u16) -> ratatui::buffer::Buffer {
        use ratatui::widgets::Widget;

        let area = Rect::new(0, 0, width, 1);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        Paragraph::new(Line::from(Span::styled(message, theme::dim_style())))
            .wrap(Wrap { trim: true })
            .render(area, &mut buffer);
        buffer
    }

    #[test]
    fn pinned_preview_preserves_wrapped_first_row() {
        let messages = [
            "",
            "   ",
            "one two three four five",
            "  original ask with leading space",
            "a verylongunbrokenwordandmore",
            "word       another word",
            "line one\nline two\tand more",
            "\u{754c}\u{754c} a \u{754c}bc",
            "e\u{301} \u{26a0}\u{fe0f} \u{1f469}\u{200d}\u{1f4bb} next word",
            "\u{200b} a\u{a0}b \u{200b}c",
        ];
        for message in messages {
            for width in 0..=24 {
                assert_eq!(
                    pinned_preview_buffer(pinned_preview_source(message, width), width),
                    pinned_preview_buffer(message, width),
                    "message {message:?}, width {width}",
                );
            }
        }
    }

    #[test]
    fn pinned_preview_long_message_only_borrows_visible_prefix() {
        for message in ["word ".repeat(100_000), "x".repeat(500_000)] {
            let preview = pinned_preview_source(&message, 80);
            assert_eq!(preview.len(), 81);
            assert_eq!(preview.as_ptr(), message.as_ptr());
            assert_eq!(
                pinned_preview_buffer(preview, 80),
                pinned_preview_buffer(&message, 80),
            );
        }
    }

    #[test]
    fn pinned_preview_keeps_grapheme_clusters_and_recomputes_for_width() {
        let message = "\u{1f469}\u{200d}\u{1f4bb}".repeat(1000);
        for width in [2, 3, 8, 20] {
            let preview = pinned_preview_source(&message, width);
            assert_eq!(preview.chars().count() % 3, 0);
            assert!(preview.len() <= (usize::from(width) / 2 + 2) * 11);
            assert_eq!(
                pinned_preview_buffer(preview, width),
                pinned_preview_buffer(&message, width),
            );
        }
    }

    #[test]
    fn first_message_captures_first_user_message_only() {
        let mut s = state();
        assert!(s.first_message.is_none());
        s.push_user_message(Some("the original ask".to_string()), Vec::new());
        s.push_user_message(Some("a follow up".to_string()), Vec::new());
        assert_eq!(s.first_message.as_deref(), Some("the original ask"));
    }

    #[test]
    fn first_message_ignores_empty_text() {
        let mut s = state();
        s.push_user_message(Some("   ".to_string()), Vec::new());
        assert!(s.first_message.is_none());
        s.push_user_message(Some("real".to_string()), Vec::new());
        assert_eq!(s.first_message.as_deref(), Some("real"));
    }

    #[test]
    fn reset_for_session_clears_first_message() {
        let mut s = state();
        s.push_user_message(Some("ask".to_string()), Vec::new());
        s.reset_for_session(
            "sess-2".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        assert!(s.first_message.is_none());
    }

    #[test]
    fn reset_for_session_clears_todo_plan() {
        // A TodoWrite plan belongs to the session that produced it, so
        // switching sessions must not leave the previous session's tasks
        // rendered in the pane.
        use crate::wire::{PlanEntry, PlanPriority, PlanStatus};
        let mut s = state();
        s.todo_tracker.set_plan(vec![
            PlanEntry {
                content: "task one".to_string(),
                status: PlanStatus::InProgress,
                priority: PlanPriority::Medium,
                active_form: None,
            },
            PlanEntry {
                content: "task two".to_string(),
                status: PlanStatus::Pending,
                priority: PlanPriority::Medium,
                active_form: None,
            },
        ]);
        assert_eq!(s.todo_tracker.total(), 2);

        s.reset_for_session(
            "sess-2".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );

        assert_eq!(
            s.todo_tracker.total(),
            0,
            "a session switch must drop the previous session's TodoWrite plan"
        );
        assert!(!s.todo_tracker.is_visible());
    }

    #[test]
    fn load_history_replays_transcript_and_seeds_first_message() {
        use crate::client::MessageEntry;
        let mut s = state();
        s.reset_for_session(
            "sess-resume".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        let before = s.entries.len();
        s.load_history(
            vec![
                MessageEntry {
                    role: "user".to_string(),
                    content: "first ask".to_string(),
                    ..Default::default()
                },
                MessageEntry {
                    role: "assistant".to_string(),
                    content: "reply".to_string(),
                    ..Default::default()
                },
                MessageEntry {
                    role: "system".to_string(),
                    content: "ignored".to_string(),
                    ..Default::default()
                },
                MessageEntry {
                    role: "user".to_string(),
                    content: "second ask".to_string(),
                    ..Default::default()
                },
            ],
            false,
        );
        // User + assistant + user replayed; system dropped.
        assert_eq!(s.entries.len(), before + 3);
        // First user message seeds the pinned recovery row.
        assert_eq!(s.first_message.as_deref(), Some("first ask"));
    }

    #[test]
    fn load_history_strips_enrichment_prefix_from_first_message() {
        use crate::client::MessageEntry;
        let mut s = state();
        s.reset_for_session(
            "sess-resume".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        s.load_history(
            vec![MessageEntry {
                role: "user".to_string(),
                content: "[CURRENT DATE & TIME: 2026-03-14 09:30:00 UTC]\n\nfirst ask".to_string(),
                ..Default::default()
            }],
            true,
        );
        // The pinned row renders a single line; it must show the message
        // text, not the runtime's timestamp prefix.
        assert_eq!(s.first_message.as_deref(), Some("first ask"));
        // The transcript entry keeps the persisted content untouched.
        assert!(matches!(
            &s.entries[s.entries.len() - 1],
            ChatEntry::UserMessage { text: Some(t), .. }
                if t.starts_with("[CURRENT DATE & TIME:") && t.ends_with("first ask")
        ));
    }

    #[test]
    fn load_history_skips_prefix_only_content_when_seeding_first_message() {
        use crate::client::MessageEntry;
        let mut s = state();
        s.reset_for_session(
            "sess-resume".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        s.load_history(
            vec![MessageEntry {
                role: "user".to_string(),
                content: "[CURRENT DATE & TIME: 2026-03-14 09:30:00 UTC]\n\n".to_string(),
                ..Default::default()
            }],
            true,
        );
        // A message that strips to nothing must not claim the pinned row —
        // Some("") would block a later real message from ever seeding it.
        assert!(s.first_message.is_none());
        s.load_history(
            vec![MessageEntry {
                role: "user".to_string(),
                content: "[CURRENT DATE & TIME: 2026-03-14 09:31:00 UTC]\n\nreal ask".to_string(),
                ..Default::default()
            }],
            true,
        );
        assert_eq!(s.first_message.as_deref(), Some("real ask"));
    }

    #[test]
    fn load_history_preserves_literal_timestamp_example_for_chat_sessions() {
        use crate::client::MessageEntry;
        let mut s = state();
        let literal = "[CURRENT DATE & TIME: 2026-03-14 09:30:00 UTC]\n\nthis is user-authored";

        s.load_history(
            vec![MessageEntry {
                role: "user".to_string(),
                content: literal.to_string(),
                ..Default::default()
            }],
            false,
        );

        assert_eq!(s.first_message.as_deref(), Some(literal));
    }

    #[test]
    fn load_history_matches_tool_result_to_call_id() {
        use crate::client::{MessageEntry, MessageEntryKind};

        let mut s = state();
        s.load_history(
            vec![
                MessageEntry {
                    role: "assistant".to_string(),
                    content: "Tool call: shell\n{}".to_string(),
                    kind: MessageEntryKind::ToolCall,
                    tool_call_id: Some("call-1".to_string()),
                    tool_name: Some("shell".to_string()),
                    tool_input: Some(serde_json::json!({"command": "pwd"})),
                    tool_output: None,
                },
                MessageEntry {
                    role: "tool".to_string(),
                    content: "Tool result: shell\n/tmp".to_string(),
                    kind: MessageEntryKind::ToolResult,
                    tool_call_id: Some("call-1".to_string()),
                    tool_name: Some("shell".to_string()),
                    tool_input: None,
                    tool_output: Some("/tmp".to_string()),
                },
            ],
            false,
        );

        assert!(matches!(
            s.entries.as_slice(),
            [ChatEntry::Tool {
                tool_call_id,
                name,
                result: Some(result),
                ..
            }] if tool_call_id.as_ref() == "call-1"
                && name.as_ref() == "shell"
                && result.as_ref() == "/tmp"
        ));
    }

    #[test]
    fn load_history_bounds_restored_tool_output_like_live_updates() {
        use crate::client::{MessageEntry, MessageEntryKind};

        let mut s = state();
        s.load_history(
            vec![MessageEntry {
                role: "assistant".to_string(),
                content: "tool call".to_string(),
                kind: MessageEntryKind::ToolCall,
                tool_call_id: Some("call-1".to_string()),
                tool_name: Some("shell".to_string()),
                tool_input: Some(serde_json::json!({})),
                tool_output: Some("λ".repeat(9_000)),
            }],
            false,
        );

        assert!(matches!(
            s.entries.as_slice(),
            [ChatEntry::Tool {
                result: Some(result),
                ..
            }] if result.ends_with("…[truncated]")
                && result.len() <= 16 * 1024
        ));
    }

    // ── Elicitation modal ────────────────────────────────────────

    fn single_elicitation() -> PendingElicitation {
        PendingElicitation {
            request_id: serde_json::json!("elicit-1"),
            session_id: "sess-1".to_string(),
            message: "Pick a fruit".to_string(),
            choices: vec![
                "Apple".to_string(),
                "Banana".to_string(),
                "Cherry".to_string(),
            ],
            multi: false,
            min_items: 1,
            max_items: 1,
            cursor: 0,
            selected: Vec::new(),
        }
    }

    fn multi_elicitation() -> PendingElicitation {
        PendingElicitation {
            request_id: serde_json::json!(42),
            session_id: "sess-1".to_string(),
            message: "Pick toppings".to_string(),
            choices: vec![
                "Cheese".to_string(),
                "Olives".to_string(),
                "Ham".to_string(),
            ],
            multi: true,
            min_items: 1,
            max_items: 2,
            cursor: 0,
            selected: vec![false, false, false],
        }
    }

    #[test]
    fn single_select_accept_content_uses_cursor_index() {
        let mut e = single_elicitation();
        e.cursor = 2;
        let content = e.accept_content().expect("single select always valid");
        assert_eq!(content, serde_json::json!({ "choice": "choice-2" }));
    }

    #[test]
    fn single_select_is_always_valid_when_choices_present() {
        let e = single_elicitation();
        assert!(e.selection_valid());
    }

    #[test]
    fn single_select_with_no_choices_is_invalid() {
        let mut e = single_elicitation();
        e.choices.clear();
        assert!(!e.selection_valid());
        assert!(e.accept_content().is_none());
    }

    #[test]
    fn multi_select_requires_min_items() {
        let e = multi_elicitation(); // min 1, nothing selected
        assert!(!e.selection_valid());
        assert!(e.accept_content().is_none());
    }

    #[test]
    fn multi_select_rejects_over_max_items() {
        let mut e = multi_elicitation(); // max 2
        e.selected = vec![true, true, true]; // 3 selected
        assert_eq!(e.selected_count(), 3);
        assert!(!e.selection_valid());
        assert!(e.accept_content().is_none());
    }

    #[test]
    fn multi_select_accept_content_lists_checked_indices() {
        let mut e = multi_elicitation();
        e.selected = vec![true, false, true]; // indices 0 and 2
        assert!(e.selection_valid());
        let content = e.accept_content().expect("2 within 1..=2");
        assert_eq!(
            content,
            serde_json::json!({ "choices": ["choice-0", "choice-2"] })
        );
    }

    #[test]
    fn elicitation_numeric_request_id_is_preserved() {
        let e = multi_elicitation();
        // Numeric ids must round-trip as numbers, not strings, so the
        // daemon can match the response to its outbound request.
        assert_eq!(e.request_id, serde_json::json!(42));
    }

    #[test]
    fn set_and_take_pending_elicitation_round_trip() {
        let mut s = state();
        assert!(s.pending_elicitation().is_none());
        s.set_pending_elicitation(single_elicitation());
        assert!(s.pending_elicitation().is_some());
        let taken = s.take_pending_elicitation().expect("was set");
        assert_eq!(taken.message, "Pick a fruit");
        assert!(s.pending_elicitation().is_none());
    }

    #[test]
    fn reset_for_session_clears_pending_elicitation() {
        let mut s = state();
        s.set_pending_elicitation(single_elicitation());
        s.reset_for_session(
            "sess-2".to_string(),
            None,
            crate::todo_tracker::TodoTrackerSettings::default(),
        );
        assert!(
            s.pending_elicitation().is_none(),
            "a session switch must drop any stale elicitation modal"
        );
    }

    // ── Inbound elicitation routing (ask_user intermittent-failure fix) ──

    /// Build an inbound `elicitation/create` request for `session_id` with a
    /// canonical single-select schema (the shape the daemon emits).
    fn inbound_single_elicitation(id: &str, session_id: &str) -> crate::client::RpcInboundRequest {
        crate::client::RpcInboundRequest {
            id: serde_json::json!(id),
            method: "elicitation/create".to_string(),
            params: serde_json::json!({
                "sessionId": session_id,
                "mode": "form",
                "message": "Pick one",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "choice": {
                            "type": "string",
                            "oneOf": [
                                { "const": "choice-0", "title": "Yes" },
                                { "const": "choice-1", "title": "No" }
                            ]
                        }
                    }
                }
            }),
        }
    }

    fn test_chat() -> (Chat, mpsc::Receiver<String>) {
        test_chat_with_transport(crate::client::Transport::Local)
    }

    fn test_chat_with_transport(
        transport: crate::client::Transport,
    ) -> (Chat, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc_transport(rpc, transport));
        (Chat::new(client, PaneKind::Chat), rx)
    }

    #[tokio::test]
    async fn prompt_completion_settles_turn_when_terminal_update_is_missing() {
        let (mut chat, mut writer_rx) = test_chat();
        let mut active = state();
        active
            .enqueue_message("hello".to_string(), Vec::new())
            .unwrap();
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.pump_all_queues();
        let request = next_rpc_request(&mut writer_rx, "prompt request should be sent").await;
        assert_eq!(request["method"], method::SESSION_PROMPT);

        assert!(
            active_state(&mut chat).turn_in_flight,
            "without a terminal frame the local turn remains in flight"
        );
        active_state(&mut chat)
            .enqueue_message("wait for explicit resume".to_string(), Vec::new())
            .unwrap();
        respond_ok(&chat.rpc_out, &request, serde_json::json!({}));
        tokio::task::yield_now().await;
        chat.drain_prompt_completions();

        let active = active_state(&mut chat);
        assert!(!active.turn_in_flight);
        assert!(matches!(active.turn_status, TurnStatus::Idle));
        assert!(active.queue_paused());
        assert_eq!(active.queue_len(), 1);
        assert!(
            active
                .entries()
                .iter()
                .all(|entry| !matches!(entry, ChatEntry::AgentMessage(_))),
            "the lifecycle fence must not invent the dropped final transcript content"
        );
    }

    #[tokio::test]
    async fn prompt_completion_before_turn_complete_does_not_duplicate_streamed_text() {
        let (mut chat, mut writer_rx) = test_chat();
        let mut active = state();
        active
            .enqueue_message("hello".to_string(), Vec::new())
            .unwrap();
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.pump_all_queues();
        let request = next_rpc_request(&mut writer_rx, "prompt request should be sent").await;

        let (notif_tx, notif_rx) = broadcast::channel(4);
        chat.notif_rx = notif_rx;
        notif_tx
            .send(RpcNotification {
                method: "session/update".to_string(),
                params: serde_json::json!({
                    "type": "agent_message_chunk",
                    "session_id": "sess-1",
                    "text": "streamed reply"
                }),
            })
            .unwrap();
        chat.drain_notifications();
        respond_ok(&chat.rpc_out, &request, serde_json::json!({}));
        tokio::task::yield_now().await;
        chat.drain_prompt_completions();

        notif_tx
            .send(RpcNotification {
                method: "session/update".to_string(),
                params: serde_json::json!({
                    "type": "turn_complete",
                    "session_id": "sess-1",
                    "outcome": "completed",
                    "content": "streamed reply"
                }),
            })
            .unwrap();
        chat.drain_notifications();

        let replies = active_state(&mut chat)
            .entries()
            .iter()
            .filter(|entry| {
                matches!(entry, ChatEntry::AgentMessage(text) if text.as_ref() == "streamed reply")
            })
            .count();
        assert_eq!(
            replies, 1,
            "a delayed terminal frame must not duplicate text committed by response settlement"
        );
    }

    #[tokio::test]
    async fn prompt_completion_keeps_late_stream_chunk_in_one_agent_entry() {
        use crossterm::event::{KeyCode, KeyModifiers};

        for interposed_error in [false, true] {
            let (mut chat, mut writer_rx) = test_chat();
            let mut active = state();
            active
                .enqueue_message("hello".to_string(), Vec::new())
                .unwrap();
            chat.phase = ChatPhase::Active(Box::new(active));
            chat.pump_all_queues();
            let request = next_rpc_request(&mut writer_rx, "prompt request should be sent").await;

            let (notif_tx, notif_rx) = broadcast::channel(4);
            chat.notif_rx = notif_rx;
            notif_tx
                .send(RpcNotification {
                    method: "session/update".to_string(),
                    params: serde_json::json!({
                        "type": "agent_message_chunk",
                        "session_id": "sess-1",
                        "text": "```rust\nlet daemon = 1;"
                    }),
                })
                .unwrap();
            chat.drain_notifications();
            respond_ok(&chat.rpc_out, &request, serde_json::json!({}));
            tokio::task::yield_now().await;
            chat.drain_prompt_completions();
            let state = active_state(&mut chat);
            state.rebuild_lines(80);
            assert_eq!(state.dirty, LinesDirty::Clean);
            assert!(state.prompt_settled_stream_entry.is_some());
            let continuation_index = state.prompt_settled_stream_entry.unwrap().1;

            if interposed_error {
                state.input_bar.insert_text("   ");
                let mut term: crate::config_manager::Term = ratatui::Terminal::with_options(
                    crate::terminal_backend::WideCellCleanupBackend::new(std::io::stdout()),
                    ratatui::TerminalOptions {
                        viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, 80, 24)),
                    },
                )
                .unwrap();
                chat.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut term)
                    .await;
                let state = active_state(&mut chat);
                assert_eq!(state.dirty, LinesDirty::Appended);
                assert!(matches!(
                    state.entries().last(),
                    Some(ChatEntry::SystemMessage(_))
                ));
                assert!(state.prompt_settled_stream_entry.is_some());
                assert!(
                    writer_rx.try_recv().is_err(),
                    "whitespace must not dispatch a prompt"
                );
            }

            notif_tx
                .send(RpcNotification {
                    method: "session/update".to_string(),
                    params: serde_json::json!({
                        "type": "agent_message_chunk",
                        "session_id": "sess-1",
                        "text": "\nlet late = 2;\n```"
                    }),
                })
                .unwrap();
            chat.drain_notifications();
            let state = active_state(&mut chat);
            assert_eq!(
                state.dirty,
                if interposed_error {
                    LinesDirty::Full
                } else {
                    LinesDirty::TailChanged(continuation_index)
                }
            );
            assert!(matches!(
                state.entries().get(continuation_index),
                Some(ChatEntry::AgentMessageContinuation(text))
                    if text == "```rust\nlet daemon = 1;\nlet late = 2;\n```"
            ));
            state.rebuild_lines(80);
            assert_eq!(state.dirty, LinesDirty::Clean);
            assert!(rendered_text(&state.cached_lines).contains("let late = 2;"));
            assert_eq!(
                state.cached_line_screen_ranges.len(),
                state.cached_lines.len()
            );
            assert_eq!(state.cached_code_blocks.len(), 1);
            assert!(state.cached_code_blocks[0].text.contains("let late = 2;"));
            notif_tx
                .send(RpcNotification {
                    method: "session/update".to_string(),
                    params: serde_json::json!({
                        "type": "turn_complete",
                        "session_id": "sess-1",
                        "outcome": "completed",
                        "content": "```rust\nlet daemon = 1;\nlet late = 2;\n```"
                    }),
                })
                .unwrap();
            chat.drain_notifications();
            assert_eq!(active_state(&mut chat).dirty, LinesDirty::Clean);

            let replies = active_state(&mut chat)
                .entries()
                .iter()
                .filter_map(|entry| match entry {
                    ChatEntry::AgentMessage(text) => Some(text.as_ref()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(replies, ["```rust\nlet daemon = 1;\nlet late = 2;\n```"]);
        }
    }

    #[tokio::test]
    async fn prompt_completion_settles_only_the_matching_background_session() {
        let (tx, mut writer_rx) = mpsc::channel::<String>(16);
        let outbound = Arc::new(RpcOutbound::new(tx));
        let mut chat = two_session_chat(&outbound);
        let focused = chat
            .state_for_session_mut("sess-a")
            .expect("focused session");
        focused.turn_in_flight = true;
        focused.turn_generation = 41;
        focused.turn_status = TurnStatus::Working;
        chat.state_for_session_mut("sess-b")
            .expect("background session")
            .enqueue_message("background prompt".to_string(), Vec::new())
            .unwrap();

        chat.pump_all_queues();
        let request = next_rpc_request(&mut writer_rx, "background prompt should be sent").await;
        assert_eq!(request["params"]["session_id"], "sess-b");
        assert!(
            chat.state_for_session("sess-b")
                .expect("background session")
                .turn_in_flight
        );

        respond_ok(&chat.rpc_out, &request, serde_json::json!({}));
        tokio::task::yield_now().await;
        chat.drain_prompt_completions();

        assert!(
            !chat
                .state_for_session("sess-b")
                .expect("background session")
                .turn_in_flight,
            "the matching background turn must settle"
        );
        let focused = chat.state_for_session("sess-a").expect("focused session");
        assert!(focused.turn_in_flight, "the focused turn must not settle");
        assert!(matches!(focused.turn_status, TurnStatus::Working));
    }

    #[tokio::test]
    async fn prior_prompt_completion_does_not_settle_next_queued_turn() {
        let (mut chat, mut writer_rx) = test_chat();
        let mut active = state();
        active
            .enqueue_message("first".to_string(), Vec::new())
            .unwrap();
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.pump_all_queues();
        let first_generation = active_state(&mut chat).turn_generation;
        let first_request =
            next_rpc_request(&mut writer_rx, "first prompt request should be sent").await;
        active_state(&mut chat)
            .enqueue_message("second".to_string(), Vec::new())
            .unwrap();

        let (notif_tx, notif_rx) = broadcast::channel(4);
        chat.notif_rx = notif_rx;
        notif_tx
            .send(RpcNotification {
                method: "session/update".to_string(),
                params: serde_json::json!({
                    "type": "turn_complete",
                    "session_id": "sess-1",
                    "outcome": "completed",
                    "content": "done"
                }),
            })
            .unwrap();
        chat.drain_notifications();

        let second_generation = active_state(&mut chat).turn_generation;
        assert_ne!(second_generation, first_generation);
        assert!(active_state(&mut chat).turn_in_flight);
        let second_request =
            next_rpc_request(&mut writer_rx, "second prompt request should be sent").await;
        assert_ne!(second_request["id"], first_request["id"]);

        respond_ok(&chat.rpc_out, &first_request, serde_json::json!({}));
        tokio::task::yield_now().await;
        chat.drain_prompt_completions();

        let active = active_state(&mut chat);
        assert!(
            active.turn_in_flight,
            "the prior response fence must not settle the newly dispatched turn"
        );
        assert_eq!(
            active
                .entries()
                .iter()
                .filter(|entry| matches!(entry, ChatEntry::AgentMessage(_)))
                .count(),
            1,
            "the surviving terminal notification must commit exactly once"
        );
    }

    #[tokio::test]
    async fn stale_correlated_terminal_does_not_settle_promoted_turn() {
        let (mut chat, mut writer_rx) = test_chat();
        let mut active = state();
        active
            .enqueue_message("first".to_string(), Vec::new())
            .unwrap();
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.pump_all_queues();
        let first_generation = active_state(&mut chat).turn_generation;
        let first_request =
            next_rpc_request(&mut writer_rx, "first prompt request should be sent").await;
        assert_eq!(
            first_request["params"]["client_turn_generation"],
            serde_json::json!(first_generation)
        );
        active_state(&mut chat).enter_cancelling();
        active_state(&mut chat)
            .inject_message("second".to_string(), Vec::new())
            .unwrap();

        let (notif_tx, notif_rx) = broadcast::channel(4);
        chat.notif_rx = notif_rx;
        notif_tx
            .send(RpcNotification {
                method: "session/update".to_string(),
                params: serde_json::json!({
                    "type": "turn_complete",
                    "session_id": "sess-1",
                    "outcome": "cancelled",
                    "content": "old turn cancelled",
                    "client_turn_generation": first_generation,
                }),
            })
            .unwrap();
        chat.drain_notifications();

        let second_generation = active_state(&mut chat).turn_generation;
        assert_ne!(second_generation, first_generation);
        let second_request =
            next_rpc_request(&mut writer_rx, "promoted prompt request should be sent").await;
        assert_eq!(
            second_request["params"]["client_turn_generation"],
            serde_json::json!(second_generation)
        );
        assert!(active_state(&mut chat).turn_in_flight);
        let current_count = active_state(&mut chat).message_count;

        // A delayed terminal from the cancelled first request carries its old
        // generation and must not settle the newly promoted second request.
        notif_tx
            .send(RpcNotification {
                method: "session/update".to_string(),
                params: serde_json::json!({
                    "type": "turn_complete",
                    "session_id": "sess-1",
                    "outcome": "cancelled",
                    "content": "stale old terminal",
                    "client_turn_generation": first_generation,
                    "message_count": 999,
                }),
            })
            .unwrap();
        chat.drain_notifications();

        assert!(
            active_state(&mut chat).turn_in_flight,
            "the new turn must remain in flight after a stale old terminal"
        );
        assert_eq!(
            active_state(&mut chat).message_count,
            current_count,
            "a stale terminal must not replace the current turn's count"
        );

        notif_tx
            .send(RpcNotification {
                method: "session/update".to_string(),
                params: serde_json::json!({
                    "type": "turn_complete",
                    "session_id": "sess-1",
                    "outcome": "completed",
                    "content": "new turn completed",
                    "client_turn_generation": second_generation,
                    "message_count": 7,
                }),
            })
            .unwrap();
        chat.drain_notifications();

        assert!(!active_state(&mut chat).turn_in_flight);
        assert_eq!(active_state(&mut chat).message_count, 7);
    }

    #[tokio::test]
    async fn prompt_session_busy_error_removes_only_optimistic_user_row() {
        let (mut chat, mut writer_rx) = test_chat();
        let mut active = state();
        active
            .enqueue_message("busy prompt".to_string(), Vec::new())
            .unwrap();
        chat.phase = ChatPhase::Active(Box::new(active));
        chat.pump_all_queues();
        let request = next_rpc_request(&mut writer_rx, "busy prompt should be sent").await;
        respond_err(
            &chat.rpc_out,
            &request,
            crate::jsonrpc::error_codes::SESSION_BUSY,
            "Session busy",
        );
        tokio::task::yield_now().await;
        chat.drain_prompt_completions();

        let active = active_state(&mut chat);
        assert!(!active.turn_in_flight);
        assert!(
            active.first_message.is_none(),
            "a failed first prompt must not remain pinned above an empty transcript"
        );
        assert!(
            active
                .entries()
                .iter()
                .all(|entry| !matches!(entry, ChatEntry::UserMessage { .. })),
            "a connected prompt error must not leave a duplicate optimistic user row"
        );
        assert!(
            active.info_message.is_some(),
            "the dispatch error must be surfaced"
        );
    }

    #[test]
    fn restored_session_state_is_idle() {
        let mut active = state();
        active.push_user_message(Some("old prompt".to_string()), Vec::new());
        active.reset_for_session(
            "sess-restored".to_string(),
            Some("restored".to_string()),
            crate::todo_tracker::TodoTrackerSettings::default(),
        );

        assert!(!active.turn_in_flight);
        assert!(matches!(active.turn_status, TurnStatus::Idle));
    }

    fn chat_with_active_input(kind: PaneKind) -> Chat {
        let (tx, _rx) = mpsc::channel::<String>(16);
        let rpc = Arc::new(RpcOutbound::new(tx));
        let client = Arc::new(RpcClient::with_rpc(rpc));
        let mut chat = Chat::new(client, kind);
        let mut active = state();
        active.input_bar.insert_text("alpha beta");
        chat.phase = ChatPhase::Active(Box::new(active));
        chat
    }

    fn active_state(chat: &mut Chat) -> &mut ChatState {
        let ChatPhase::Active(active) = &mut chat.phase else {
            unreachable!();
        };
        active
    }

    #[tokio::test]
    async fn active_turn_paste_populates_composer_and_queues_on_submit() {
        let mut chat = chat_with_active_input(PaneKind::Chat);
        let state = active_state(&mut chat);
        state.input_bar.clear_input();
        state.turn_in_flight = true;

        chat.handle_paste("pasted while active");

        let state = active_state(&mut chat);
        assert_eq!(state.input_bar.input(), "pasted while active");
        assert!(state.turn_in_flight);

        let InputBarAction::Submit { text, attachments } =
            state.input_bar.submit_current_input_for_test()
        else {
            panic!("pasted input must submit normally");
        };
        state
            .enqueue_message(text.unwrap_or_default(), attachments)
            .expect("pasted input queues during an active turn");

        assert_eq!(state.queue_len(), 1);
        assert!(
            state.take_next_dispatchable().is_none(),
            "an active turn must not dispatch the queued pasted input"
        );
    }

    #[tokio::test]
    async fn paste_does_not_mutate_composer_while_approval_is_pending() {
        let mut chat = chat_with_active_input(PaneKind::Chat);
        let state = active_state(&mut chat);
        state.turn_in_flight = true;
        state.pending_approval = Some(PendingApproval {
            request_id: "request-1".to_string(),
            tool_name: "shell".to_string(),
            arguments_summary: "pwd".to_string(),
            timeout_secs: 30,
        });

        chat.handle_paste(" must not reach the composer");

        assert_eq!(active_state(&mut chat).input_bar.input(), "alpha beta");
    }

    #[tokio::test]
    async fn paste_does_not_mutate_hidden_composer_when_another_surface_owns_input() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut cases = Vec::new();

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).model_picker = ModelPickerOverlay::Loading;
        cases.push(("model picker", chat));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        let state = active_state(&mut chat);
        state.turn_in_flight = true;
        state.pending_elicitation = Some(single_elicitation());
        cases.push(("elicitation", chat));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).session_overlay = SessionOverlay::List {
            sessions: Vec::new(),
            list_state: ListState::default(),
        };
        cases.push(("session picker", chat));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).context_menu = Some(ChatContextMenu {
            rect: Rect::new(0, 0, 20, 5),
            target: ChatContextMenuTarget::Queue(1),
            selected: 0,
        });
        cases.push(("context menu", chat));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        let state = active_state(&mut chat);
        state.input_bar.add_attachment(PendingAttachment {
            path: std::path::PathBuf::from("already-attached.png"),
            mime_type: "image/png".into(),
            filename: "already-attached.png".into(),
            size_bytes: 1,
            source: crate::attachment::AttachmentSource::File,
        });
        state.input_bar.clear_input();
        state.input_bar.insert_text("/attachments");
        assert!(matches!(
            state
                .input_bar
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            InputBarAction::Consumed
        ));
        cases.push(("attachment manager", chat));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        assert!(matches!(
            active_state(&mut chat).input_bar.handle_key(KeyEvent::new(
                KeyCode::Char('a'),
                crate::keymap::Chord::primary('a').effective_modifiers(),
            )),
            InputBarAction::Consumed
        ));
        cases.push(("file explorer", chat));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).browse_cursor = Some(0);
        cases.push(("browse mode", chat));

        let attachment_path = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
        for (surface, mut chat) in cases {
            let state = active_state(&mut chat);
            let original_input = state.input_bar.input().to_string();
            let original_attachment_count = state.input_bar.pending_attachments().len();

            chat.handle_paste(" hidden text");
            chat.handle_paste(&attachment_path);

            let state = active_state(&mut chat);
            assert_eq!(
                state.input_bar.input(),
                original_input,
                "{surface} must keep pasted text out of the hidden composer"
            );
            assert_eq!(
                state.input_bar.pending_attachments().len(),
                original_attachment_count,
                "{surface} must not create hidden attachments from pasted paths"
            );
        }
    }

    #[test]
    fn file_explorer_attachment_error_reaches_info_notice() {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Keep the explorer listing deterministic and let the guard clean up
        // the oversized fixture even when an assertion fails.
        let temp_dir = tempfile::tempdir().expect("create attachment fixture directory");
        let oversized_path = temp_dir.path().join("oversized.bin");
        let file = std::fs::File::create(&oversized_path).expect("create oversized attachment");
        file.set_len(10 * 1024 * 1024 + 1)
            .expect("make attachment exceed the 10 MiB limit");

        let mut state = state();
        state
            .input_bar
            .open_file_explorer_for_test(oversized_path.clone());

        assert!(
            state.handle_input_bar_overlay_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE,))
        );
        assert!(
            state
                .info_message
                .as_ref()
                .is_some_and(|notice| !notice.text.is_empty()),
            "a rejected file-explorer attachment must produce visible feedback regardless of locale"
        );
        assert!(!state.input_bar.has_file_explorer());
    }

    #[tokio::test]
    async fn pane_navigation_claims_only_unobstructed_active_input() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let word_left = KeyEvent::new(KeyCode::Left, KeyModifiers::ALT);

        for kind in [PaneKind::Chat, PaneKind::Acp] {
            let chat = chat_with_active_input(kind);
            assert!(chat.claims_pane_navigation(&word_left));
        }

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).input_bar.clear_input();
        assert!(!chat.claims_pane_navigation(&word_left));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).model_picker = ModelPickerOverlay::Loading;
        assert!(!chat.claims_pane_navigation(&word_left));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).pending_elicitation = Some(single_elicitation());
        assert!(!chat.claims_pane_navigation(&word_left));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).pending_approval = Some(PendingApproval {
            request_id: "request-1".to_string(),
            tool_name: "shell".to_string(),
            arguments_summary: "pwd".to_string(),
            timeout_secs: 30,
        });
        assert!(!chat.claims_pane_navigation(&word_left));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).session_overlay = SessionOverlay::List {
            sessions: Vec::new(),
            list_state: ListState::default(),
        };
        assert!(!chat.claims_pane_navigation(&word_left));

        let mut chat = chat_with_active_input(PaneKind::Chat);
        active_state(&mut chat).browse_cursor = Some(0);
        assert!(!chat.claims_pane_navigation(&word_left));

        let (mut chat, _rx) = test_chat();
        chat.phase = ChatPhase::PickSession {
            sessions: Vec::new(),
            list_state: ListState::default(),
            agents: Vec::new(),
        };
        assert!(!chat.claims_pane_navigation(&word_left));
    }

    #[tokio::test]
    async fn elicitation_matching_active_session_installs_modal() {
        let (mut chat, mut rx) = test_chat();
        chat.phase = ChatPhase::Active(Box::new(state())); // session_id = "sess-1"

        let result = chat.try_install_elicitation(inbound_single_elicitation("e1", "sess-1"));
        assert!(matches!(result, ElicitationRouting::Installed));

        // Modal installed.
        match &chat.phase {
            ChatPhase::Active(s) => assert!(
                s.pending_elicitation().is_some(),
                "matching-session elicitation must install a modal"
            ),
            _ => panic!("expected Active phase"),
        }
        // The pane only installs; the app-level router owns all responses.
        assert!(
            rx.try_recv().is_err(),
            "an installed elicitation must not be auto-answered"
        );
    }

    #[tokio::test]
    async fn elicitation_for_other_session_is_deferred_not_cancelled() {
        let (mut chat, mut rx) = test_chat();
        chat.phase = ChatPhase::Active(Box::new(state())); // active = "sess-1"

        let result = chat.try_install_elicitation(inbound_single_elicitation("e1", "sess-OTHER"));
        assert!(matches!(result, ElicitationRouting::Defer(_)));
        // Give the (non-)spawned responder a chance — nothing must be sent yet.
        tokio::task::yield_now().await;
        assert!(
            rx.try_recv().is_err(),
            "a deferred elicitation must not be answered during its grace window"
        );
    }

    #[tokio::test]
    async fn unparseable_elicitation_is_returned_to_router_for_cancellation() {
        let (mut chat, mut rx) = test_chat();
        chat.phase = ChatPhase::Active(Box::new(state()));

        let mut req = inbound_single_elicitation("e1", "sess-1");
        // Corrupt the schema so `ElicitationShape::from_schema` returns None.
        req.params["requestedSchema"] = serde_json::json!({ "type": "object" });

        let result = chat.try_install_elicitation(req);
        assert!(matches!(result, ElicitationRouting::Unparseable(id) if id == "e1"));
        tokio::task::yield_now().await;
        assert!(
            rx.try_recv().is_err(),
            "the pane must not answer requests behind the app router"
        );
    }

    use super::*;
    use clap::{CommandFactory, Parser};
    use std::net::TcpListener;

    #[cfg(feature = "agent-runtime")]
    struct SelectorTestTerminal {
        size: Option<(u16, u16)>,
        keys: std::collections::VecDeque<std::io::Result<QuickstartSelectorKey>>,
        actions: Vec<&'static str>,
        fail_action: Option<&'static str>,
    }

    #[cfg(feature = "agent-runtime")]
    impl SelectorTestTerminal {
        fn new(
            size: Option<(u16, u16)>,
            keys: impl IntoIterator<Item = std::io::Result<QuickstartSelectorKey>>,
        ) -> Self {
            Self {
                size,
                keys: keys.into_iter().collect(),
                actions: Vec::new(),
                fail_action: None,
            }
        }

        fn perform(&mut self, action: &'static str) -> std::io::Result<()> {
            self.actions.push(action);
            if self.fail_action == Some(action) {
                return Err(std::io::Error::other(format!("injected {action} failure")));
            }
            Ok(())
        }
    }

    #[cfg(feature = "agent-runtime")]
    impl QuickstartSelectorTerminal for SelectorTestTerminal {
        fn size_checked(&mut self) -> Option<(u16, u16)> {
            self.size
        }

        fn enter_alternate_screen(&mut self) -> std::io::Result<()> {
            self.perform("enter_alternate_screen")
        }

        fn clear_screen(&mut self) -> std::io::Result<()> {
            self.perform("clear_screen")
        }

        fn move_cursor_to_origin(&mut self) -> std::io::Result<()> {
            self.perform("move_cursor_to_origin")
        }

        fn hide_cursor(&mut self) -> std::io::Result<()> {
            self.perform("hide_cursor")
        }

        fn show_cursor(&mut self) -> std::io::Result<()> {
            self.perform("show_cursor")
        }

        fn leave_alternate_screen(&mut self) -> std::io::Result<()> {
            self.perform("leave_alternate_screen")
        }

        fn write_line(&mut self, _line: &str) -> std::io::Result<()> {
            self.perform("write_line")
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.perform("flush")
        }

        fn read_key(&mut self) -> std::io::Result<QuickstartSelectorKey> {
            self.actions.push("read_key");
            self.keys.pop_front().unwrap_or_else(|| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "no injected selector key",
                ))
            })
        }
    }

    /// One step of a deterministic PTY interaction: a key press, or a resize
    /// of the output terminal applied between key presses the way a terminal
    /// emulator changes a window while the selector waits for input.
    #[cfg(all(feature = "agent-runtime", unix))]
    enum PtyStep {
        Key(QuickstartSelectorKey),
        ResizeOutput { rows: u16, columns: u16 },
    }

    /// Injected input for the production Crossterm adapter.
    ///
    /// Keys are queued rather than read from the process-global event source
    /// so the regression runs under a test harness without racing a
    /// controlling terminal. Resizes are applied to the PTY master exactly as
    /// a terminal emulator would, so the adapter's own geometry query must
    /// observe them.
    #[cfg(all(feature = "agent-runtime", unix))]
    struct PtyQuickstartInput {
        master: std::fs::File,
        steps: std::collections::VecDeque<PtyStep>,
    }

    #[cfg(all(feature = "agent-runtime", unix))]
    impl QuickstartSelectorInput for PtyQuickstartInput {
        fn read_key(&mut self) -> std::io::Result<QuickstartSelectorKey> {
            loop {
                match self.steps.pop_front() {
                    Some(PtyStep::Key(key)) => return Ok(key),
                    Some(PtyStep::ResizeOutput { rows, columns }) => {
                        set_pty_size(&self.master, rows, columns);
                    }
                    None => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "no injected selector key",
                        ));
                    }
                }
            }
        }
    }

    /// Open a PTY pair sized `rows` by `columns`, returned as `(master, slave)`.
    #[cfg(all(feature = "agent-runtime", unix))]
    fn open_pty(rows: u16, columns: u16) -> (std::fs::File, std::fs::File) {
        use std::os::fd::FromRawFd;

        let mut master_fd = -1;
        let mut slave_fd = -1;
        let mut dimensions = libc::winsize {
            ws_row: rows,
            ws_col: columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: both descriptor pointers refer to live `c_int` storage. The
        // optional name and termios inputs are null, and `dimensions` remains
        // live for the duration of the call.
        let openpty_result = unsafe {
            libc::openpty(
                &raw mut master_fd,
                &raw mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut dimensions,
            )
        };
        assert_eq!(openpty_result, 0, "openpty failed");

        // SAFETY: `openpty` returned two distinct, live descriptors. Each is
        // transferred to exactly one `File`, which closes it exactly once.
        unsafe {
            (
                std::fs::File::from_raw_fd(master_fd),
                std::fs::File::from_raw_fd(slave_fd),
            )
        }
    }

    #[cfg(all(feature = "agent-runtime", unix))]
    fn set_pty_size(pty: &std::fs::File, rows: u16, columns: u16) {
        use std::os::fd::AsRawFd;

        let dimensions = libc::winsize {
            ws_row: rows,
            ws_col: columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: `pty` owns a live PTY descriptor and `dimensions` is a fully
        // initialized `winsize` that outlives the call.
        let result =
            unsafe { libc::ioctl(pty.as_raw_fd(), libc::TIOCSWINSZ, &raw const dimensions) };
        assert_eq!(result, 0, "TIOCSWINSZ failed");
    }

    /// Build the production Crossterm adapter over a PTY slave with injected
    /// input, so the exact production escape sequences and geometry query run.
    #[cfg(all(feature = "agent-runtime", unix))]
    fn pty_quickstart_terminal(
        master: &std::fs::File,
        slave: std::fs::File,
        steps: impl IntoIterator<Item = PtyStep>,
    ) -> CrosstermQuickstartTerminal<std::fs::File, PtyQuickstartInput> {
        CrosstermQuickstartTerminal {
            output: slave,
            input: PtyQuickstartInput {
                master: master.try_clone().expect("PTY master should be clonable"),
                steps: steps.into_iter().collect(),
            },
        }
    }

    /// Read everything written to the PTY, returning once the output is idle.
    #[cfg(all(feature = "agent-runtime", unix))]
    fn drain_pty_output(master: &mut std::fs::File) -> String {
        use std::os::fd::AsRawFd;

        // SAFETY: the PTY master descriptor is live; preserving its current
        // flags and adding O_NONBLOCK prevents a spurious poll wakeup from
        // hanging the test.
        let master_flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(master_flags >= 0, "reading PTY master flags failed");
        assert_eq!(
            unsafe {
                libc::fcntl(
                    master.as_raw_fd(),
                    libc::F_SETFL,
                    master_flags | libc::O_NONBLOCK,
                )
            },
            0,
            "setting PTY master nonblocking mode failed"
        );

        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let mut poll_fd = libc::pollfd {
                fd: master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `poll_fd` points to one initialized poll descriptor.
            let ready = unsafe { libc::poll(&raw mut poll_fd, 1, 100) };
            assert!(ready >= 0, "polling PTY output failed");
            if ready == 0 || poll_fd.revents & libc::POLLIN == 0 {
                break;
            }
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("failed to read PTY output: {error}"),
            }
        }
        String::from_utf8(output).expect("selector output should be UTF-8")
    }

    #[cfg(all(feature = "agent-runtime", unix))]
    const PTY_CLEAR_AND_HOME: &str = "\u{1b}[2J\u{1b}[1;1H";

    #[cfg(all(feature = "agent-runtime", unix))]
    const PTY_SHOW_CURSOR_AND_LEAVE_SCREEN: &str = "\u{1b}[?25h\u{1b}[?1049l";

    #[cfg(all(feature = "agent-runtime", unix))]
    #[test]
    fn quickstart_selector_repeated_navigation_redraws_at_pty_origin() {
        let (mut master, slave) = open_pty(20, 80);
        let mut term = pty_quickstart_terminal(
            &master,
            slave,
            [
                PtyStep::Key(QuickstartSelectorKey::Down),
                PtyStep::Key(QuickstartSelectorKey::Down),
                PtyStep::Key(QuickstartSelectorKey::Up),
                PtyStep::Key(QuickstartSelectorKey::Cancel),
            ],
        );

        let outcome = interact_quickstart_selector(
            &mut term,
            &["first".to_string(), "second".to_string()],
            "Choose",
            (20, 80),
        )
        .expect("repeated PTY navigation should succeed");
        assert_eq!(outcome, QuickstartSelectorOutcome::Pick(None));

        let output = drain_pty_output(&mut master);
        drop(term);

        assert_eq!(
            output.matches(PTY_CLEAR_AND_HOME).count(),
            4,
            "the initial frame and all three navigation redraws must begin at the PTY origin; \
             output: {output:?}"
        );
    }

    /// Quickstart accepts distinct input and output terminals. The frame must
    /// be fitted to the terminal that receives it: a process-global query can
    /// describe the controlling terminal while stderr is a narrower one.
    #[cfg(all(feature = "agent-runtime", unix))]
    #[test]
    fn quickstart_selector_measures_the_terminal_that_receives_the_frame() {
        let (controlling_master, controlling_slave) = open_pty(20, 80);
        let (mut output_master, output_slave) = open_pty(20, 40);

        let mut controlling = pty_quickstart_terminal(&controlling_master, controlling_slave, []);
        assert_eq!(
            controlling.size_checked(),
            Some((20, 80)),
            "the adapter over the controlling PTY reports that PTY's geometry"
        );

        let mut term = pty_quickstart_terminal(
            &output_master,
            output_slave,
            [
                PtyStep::Key(QuickstartSelectorKey::Down),
                PtyStep::Key(QuickstartSelectorKey::Cancel),
            ],
        );
        let output_size = quickstart_selector_terminal_size(&mut term)
            .expect("the output PTY reports its geometry");
        assert_eq!(
            output_size,
            (20, 40),
            "the adapter over the output PTY must report the output PTY, not the controlling one"
        );

        // Fit exactly as the Quickstart caller does, from the sampled output
        // geometry, with content that only fits the wider terminal unfitted.
        let row_budget = quickstart_selector_row_budget(usize::from(output_size.1))
            .expect("40 columns is a supported width");
        let prompt = "Open a selector (Enter), or pick Create. Esc to quit.";
        let fitted_prompt = fit_quickstart_selector_row(prompt, row_budget);
        assert_ne!(
            fitted_prompt, prompt,
            "the prompt needs fitting at 40 columns"
        );
        let label = "[ ] Model provider — not yet chosen (pick one to continue)";
        let fitted_label = fit_quickstart_selector_row(label, row_budget);
        assert_ne!(fitted_label, label, "the row needs fitting at 40 columns");

        let outcome = interact_quickstart_selector(
            &mut term,
            std::slice::from_ref(&fitted_label),
            &fitted_prompt,
            output_size,
        )
        .expect("navigation on the output PTY should succeed");
        assert_eq!(outcome, QuickstartSelectorOutcome::Pick(None));

        let output = drain_pty_output(&mut output_master);
        drop(term);
        drop(controlling);

        assert!(
            output.contains(&format!("? {fitted_prompt}")) && output.contains(&fitted_label),
            "the fitted prompt and row must reach the output terminal; output: {output:?}"
        );
        assert!(
            !output.contains(prompt) && !output.contains(label),
            "unfitted text must never reach the 40-column output terminal; output: {output:?}"
        );
        for line in output.split("\r\n") {
            assert!(
                console::measure_text_width(line) <= 40,
                "{line:?} exceeds the 40-column output terminal"
            );
        }
    }

    /// A resize of the output terminal alone raises no Crossterm resize event,
    /// so the recheck on the next key must read the output terminal itself.
    #[cfg(all(feature = "agent-runtime", unix))]
    #[test]
    fn quickstart_selector_fails_closed_when_only_the_output_terminal_resizes() {
        let (mut master, slave) = open_pty(20, 40);
        let mut term = pty_quickstart_terminal(
            &master,
            slave,
            [
                PtyStep::Key(QuickstartSelectorKey::Down),
                PtyStep::ResizeOutput {
                    rows: 20,
                    columns: 30,
                },
                PtyStep::Key(QuickstartSelectorKey::Down),
                PtyStep::Key(QuickstartSelectorKey::Cancel),
            ],
        );
        let initial_size = quickstart_selector_terminal_size(&mut term)
            .expect("the output PTY reports its geometry");
        assert_eq!(initial_size, (20, 40));

        let error = interact_quickstart_selector(
            &mut term,
            &["first".to_string(), "second".to_string()],
            "Choose",
            initial_size,
        )
        .expect_err("an output-only resize must stop the selector");
        assert_eq!(
            error.to_string(),
            quickstart_selector_resize_error((20, 40), (20, 30)).to_string(),
            "the recheck must report the output terminal's new geometry"
        );

        let output = drain_pty_output(&mut master);
        drop(term);

        assert_eq!(
            output.matches(PTY_CLEAR_AND_HOME).count(),
            2,
            "only the initial frame and the pre-resize redraw may be drawn; output: {output:?}"
        );
        assert!(
            output.ends_with(PTY_SHOW_CURSOR_AND_LEAVE_SCREEN),
            "the cursor and main screen must be restored after the resize; output: {output:?}"
        );
    }

    #[cfg(all(feature = "agent-runtime", unix))]
    #[test]
    fn quickstart_output_terminal_size_is_unknown_without_reported_geometry() {
        let not_a_terminal = tempfile::tempfile().expect("temporary file");
        assert_eq!(quickstart_output_terminal_size(&not_a_terminal), None);

        let (_unset_master, unset_slave) = open_pty(0, 0);
        assert_eq!(quickstart_output_terminal_size(&unset_slave), None);

        let (_master, slave) = open_pty(9, 20);
        assert_eq!(quickstart_output_terminal_size(&slave), Some((9, 20)));
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn fit_quickstart_selector_row_respects_byte_and_display_budgets() {
        let short = "[ ] Memory — not yet chosen";
        assert_eq!(fit_quickstart_selector_row(short, 80), short);

        let rows = [
            "[✓] Model provider — Anthropic (alias: main, model: claude-sonnet-4-5)",
            "[✓] モデルプロバイダー — Anthropic（モデル：長い名前）",
            "[✓] 模型提供方 — 提供商与模型摘要",
            "emoji 👩‍💻 and combining e\u{301} text",
            "line one\nline two\twith controls",
        ];
        for row in rows {
            for budget in 0..=64 {
                let fitted = fit_quickstart_selector_row(row, budget);
                assert!(
                    fitted.len() <= budget,
                    "{fitted:?} uses {} bytes with budget {budget}",
                    fitted.len()
                );
                assert!(
                    console::measure_text_width(&fitted) <= budget,
                    "{fitted:?} uses {} columns with budget {budget}",
                    console::measure_text_width(&fitted)
                );
                assert!(
                    fitted.chars().all(|ch| !ch.is_control()),
                    "{fitted:?} contains a terminal control character"
                );
            }
        }

        let long = rows[0];
        assert_eq!(fit_quickstart_selector_row(long, 0), "");
        assert_eq!(fit_quickstart_selector_row(long, 1), ".");
        assert_eq!(fit_quickstart_selector_row(long, 2), "[.");
        assert!(fit_quickstart_selector_row(long, 40).ends_with('…'));
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_budget_rejects_unsafe_terminal_widths() {
        assert!(
            (0..QUICKSTART_SELECTOR_MIN_WIDTH)
                .all(|width| quickstart_selector_row_budget(width).is_none())
        );
        assert_eq!(quickstart_selector_row_budget(20), Some(17));
        assert_eq!(quickstart_selector_row_budget(21), Some(18));
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_minimum_width_keeps_actions_identifiable() {
        let budget = quickstart_selector_row_budget(QUICKSTART_SELECTOR_MIN_WIDTH).unwrap();
        let rows = [
            ("[ ] Model provider — not yet chosen", "[ ] Model"),
            ("[ ] Risk profile — not yet chosen", "[ ] Risk"),
            ("[ ] Memory — not yet chosen", "[ ] Memory"),
            ("[ ] Channels (0) — not yet chosen", "[ ] Channels"),
            ("[ ] Peer groups — not yet chosen", "[ ] Peer"),
            ("[ ] Agent identity — not yet chosen", "[ ] Agent"),
            ("── Create agent", "── Create"),
        ];

        for (row, identifiable_prefix) in rows {
            let fitted = fit_quickstart_selector_row(row, budget);
            assert!(
                fitted.starts_with(identifiable_prefix),
                "{fitted:?} does not identify {row:?}"
            );
        }
    }

    /// The checklist rows exactly as a committed locale ships them.
    ///
    /// The identifiability guarantee is about the strings users actually see,
    /// so these are read from the committed catalogues rather than retyped:
    /// a hand-written approximation can stay distinguishable at a width where
    /// the real, longer, column-padded row has already collapsed.
    #[cfg(feature = "agent-runtime")]
    fn quickstart_checklist_rows_for_locale(cli_ftl: &str) -> Vec<String> {
        const ROW_KEYS: [&str; 6] = [
            "cli-quickstart-row-model-provider",
            "cli-quickstart-row-risk-profile",
            "cli-quickstart-row-memory",
            "cli-quickstart-row-channels",
            "cli-quickstart-row-peer-groups",
            "cli-quickstart-row-agent-identity",
        ];

        let value_for = |key: &str| -> String {
            cli_ftl
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key} = ")))
                .unwrap_or_else(|| panic!("{key} should be defined in the catalogue"))
                .to_string()
        };

        let mut rows: Vec<String> = ROW_KEYS
            .iter()
            .map(|key| {
                value_for(key)
                    .replace("{$glyph}", "[ ]")
                    .replace("{$summary}", "not yet chosen")
            })
            .collect();
        rows.push(value_for("cli-quickstart-create-agent"));
        rows
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_accepted_widths_keep_every_action_distinguishable() {
        // The blocker this guards: a width floor chosen only for arithmetic
        // safety left widths 3 and 4 "supported" while every fitted row
        // collapsed to "" or ".", producing an interactive menu in which the
        // user could not tell Provider from Risk from Create — and could
        // commit real config chosen blind. Accepting a width must therefore
        // mean the rows stay individually readable, in every locale we ship,
        // not merely that the budget subtraction did not underflow.
        let locales: [(&str, &str); 5] = [
            (
                "en",
                include_str!("../crates/clawcrew-runtime/locales/en/cli.ftl"),
            ),
            (
                "es",
                include_str!("../crates/clawcrew-runtime/locales/es/cli.ftl"),
            ),
            (
                "fr",
                include_str!("../crates/clawcrew-runtime/locales/fr/cli.ftl"),
            ),
            (
                "ja",
                include_str!("../crates/clawcrew-runtime/locales/ja/cli.ftl"),
            ),
            (
                "zh-CN",
                include_str!("../crates/clawcrew-runtime/locales/zh-CN/cli.ftl"),
            ),
        ];

        for (locale, cli_ftl) in locales {
            let rows = quickstart_checklist_rows_for_locale(cli_ftl);
            assert_eq!(rows.len(), 7, "{locale}: expected seven checklist rows");

            for width in 0..=120usize {
                let Some(budget) = quickstart_selector_row_budget(width) else {
                    continue;
                };

                let fitted: Vec<String> = rows
                    .iter()
                    .map(|row| fit_quickstart_selector_row(row, budget))
                    .collect();

                for (row, label) in rows.iter().zip(&fitted) {
                    assert!(
                        !label.is_empty(),
                        "{locale}: width {width} accepted but {row:?} fits to an empty label"
                    );
                    assert!(
                        label.chars().any(|ch| ch.is_alphanumeric()),
                        "{locale}: width {width} accepted but {row:?} fits to {label:?}, \
                         which carries no readable text"
                    );
                }

                let distinct: std::collections::HashSet<&str> =
                    fitted.iter().map(String::as_str).collect();
                assert_eq!(
                    distinct.len(),
                    fitted.len(),
                    "{locale}: width {width} accepted but the fitted rows are not all \
                     distinguishable: {fitted:?}"
                );
            }
        }
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_rejects_widths_that_erase_action_labels() {
        // The specific widths the previous floor blessed. At width 3 the row
        // budget was 0 and every label fitted to ""; at width 4 the budget was
        // 1 and every label fitted to ".". Both must now be rejected before
        // any interaction can start.
        let rows = quickstart_checklist_rows_for_locale(include_str!(
            "../crates/clawcrew-runtime/locales/en/cli.ftl"
        ));

        for width in [0usize, 1, 2, 3, 4, 5, 10, 19] {
            assert_eq!(
                quickstart_selector_row_budget(width),
                None,
                "width {width} must be rejected, not fitted"
            );
        }

        // Demonstrate what acceptance at those widths would have meant, so the
        // rejection above is anchored to the user-visible failure rather than
        // to an arbitrary constant.
        for (collapsed_budget, expected) in [(0usize, ""), (1, ".")] {
            let fitted: std::collections::HashSet<String> = rows
                .iter()
                .map(|row| fit_quickstart_selector_row(row, collapsed_budget))
                .collect();
            assert_eq!(
                fitted,
                std::collections::HashSet::from([expected.to_string()]),
                "budget {collapsed_budget} collapses every action to {expected:?}"
            );
        }

        assert!(
            quickstart_selector_row_budget(QUICKSTART_SELECTOR_MIN_WIDTH).is_some(),
            "the floor itself must remain usable"
        );
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_height_prevents_paging_suffixes() {
        let item_count = 7;
        let min_height = quickstart_selector_min_height(item_count);

        assert_eq!(min_height, 9);
        assert!((0..min_height).all(|height| !quickstart_selector_fits_height(height, item_count)));
        assert!(quickstart_selector_fits_height(min_height, item_count));
        assert!(quickstart_selector_fits_height(min_height + 1, item_count));
        assert_eq!(
            quickstart_selector_min_height(usize::MAX),
            usize::MAX,
            "the terminal guard must not wrap on an unexpected item count"
        );
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_prompt_stays_within_final_terminal_budget() {
        let prompts = [
            "Open a selector (Enter), or pick Create. Esc to quit.",
            "選択肢を開くには Enter、終了するには Esc を押してください。",
            "Open a selector\nwithout adding a physical terminal row.",
        ];

        for terminal_width in [20, 40, 80] {
            let budget = quickstart_selector_row_budget(terminal_width).unwrap();
            for prompt in prompts {
                let fitted = fit_quickstart_selector_row(prompt, budget);
                assert!(
                    fitted.len() <= budget,
                    "{fitted:?} uses {} bytes with budget {budget}",
                    fitted.len()
                );
                assert!(
                    console::measure_text_width(&fitted) <= budget,
                    "{fitted:?} uses {} columns with budget {budget}",
                    console::measure_text_width(&fitted)
                );
                assert!(
                    fitted.chars().all(|ch| !ch.is_control()),
                    "{fitted:?} contains a terminal control character"
                );
            }
        }
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_unknown_terminal_size_fails_closed() {
        // A narrow terminal with an unavailable size must not get rows fitted
        // against a guessed geometry.
        assert!(
            !quickstart_selector_size_is_usable(None),
            "an unknown terminal size must not be accepted for fitting"
        );
        assert!(
            quickstart_selector_size_is_usable(Some((24, 80))),
            "a reported size must still be accepted"
        );

        let mut term = SelectorTestTerminal::new(None, []);
        assert_eq!(
            quickstart_selector_terminal_size(&mut term),
            None,
            "the selector must preserve a failed terminal size query"
        );
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_recheck_rejects_resize_and_unknown_size() {
        let initial = (24u16, 80u16);

        assert!(
            quickstart_selector_recheck_size(initial, Some(initial)).is_ok(),
            "an unchanged size must allow the interaction to continue"
        );

        let resized = quickstart_selector_recheck_size(initial, Some((24, 40)))
            .expect_err("a changed size must abort the interaction");
        assert!(
            resized.to_string().contains("40"),
            "the resize error should name the new width; got {resized}"
        );

        // The important half: unknown is not evidence the geometry still
        // matches. Without the checked query this branch would compare the
        // fabricated (24, 80) against the initial sample, find them equal, and
        // keep redrawing rows fitted for a terminal it can no longer see.
        let unknown = quickstart_selector_recheck_size(initial, None)
            .expect_err("an unavailable size must abort the interaction");
        assert_eq!(
            unknown.to_string(),
            qta("cli-quickstart-terminal-size-unknown", &[]),
            "unknown size must surface the localized size-unknown error"
        );
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_ctrl_c_restores_screen_even_when_cursor_restore_fails() {
        let mut term =
            SelectorTestTerminal::new(Some((20, 80)), [Ok(QuickstartSelectorKey::Interrupt)]);
        term.fail_action = Some("show_cursor");

        let outcome = interact_quickstart_selector(
            &mut term,
            &["first".to_string(), "second".to_string()],
            "Choose",
            (20, 80),
        )
        .expect("cleanup failure must not replace Ctrl+C interrupt semantics");

        assert_eq!(outcome, QuickstartSelectorOutcome::Interrupt);
        let show = term
            .actions
            .iter()
            .position(|action| *action == "show_cursor")
            .expect("cursor restoration must be attempted");
        let leave = term
            .actions
            .iter()
            .position(|action| *action == "leave_alternate_screen")
            .expect("alternate-screen restoration must be attempted");
        assert!(
            show < leave,
            "cleanup attempts should retain their safe order"
        );
        assert_eq!(term.actions.last(), Some(&"flush"));
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_partial_entry_failure_still_restores_screen() {
        let mut term = SelectorTestTerminal::new(Some((20, 80)), []);
        term.fail_action = Some("clear_screen");

        let error = match QuickstartSelectorScreen::enter(&mut term) {
            Ok(_) => panic!("injected clear failure should abort entry"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("clear_screen"));
        assert_eq!(
            term.actions,
            [
                "enter_alternate_screen",
                "clear_screen",
                "show_cursor",
                "leave_alternate_screen",
                "flush",
            ]
        );
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selector_read_error_still_restores_screen() {
        let mut term = SelectorTestTerminal::new(
            Some((20, 80)),
            [Err(std::io::Error::other("injected read failure"))],
        );

        let error =
            interact_quickstart_selector(&mut term, &["first".to_string()], "Choose", (20, 80))
                .expect_err("injected read failure should surface");
        assert!(error.to_string().contains("injected read failure"));
        assert!(term.actions.contains(&"show_cursor"));
        assert!(term.actions.contains(&"leave_alternate_screen"));
        assert_eq!(term.actions.last(), Some(&"flush"));
    }

    #[cfg(feature = "agent-runtime")]
    #[test]
    fn quickstart_selection_maps_by_index_when_fitted_labels_are_identical() {
        let actions = [
            QuickstartChecklistAction::Provider,
            QuickstartChecklistAction::Risk,
            QuickstartChecklistAction::Memory,
            QuickstartChecklistAction::Channels,
            QuickstartChecklistAction::PeerGroups,
            QuickstartChecklistAction::Agent,
            QuickstartChecklistAction::Create,
        ];
        let choices: Vec<(QuickstartChecklistAction, String)> = actions
            .iter()
            .copied()
            .map(|action| (action, "same row".to_string()))
            .collect();
        let fitted: Vec<String> = choices
            .iter()
            .map(|(_, label)| fit_quickstart_selector_row(label, 0))
            .collect();
        assert!(fitted.windows(2).all(|pair| pair[0] == pair[1]));

        for (index, expected) in actions.into_iter().enumerate() {
            assert_eq!(quickstart_action_for_pick(&choices, Some(index)), expected);
        }
        assert_eq!(
            quickstart_action_for_pick(&choices, None),
            QuickstartChecklistAction::Quit
        );
        assert_eq!(
            quickstart_action_for_pick(&choices, Some(choices.len())),
            QuickstartChecklistAction::Quit
        );
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_reads_appimage_from_entry() {
        let entry = "[Desktop Entry]\n\
             Name=ClawCrew\n\
             Exec=/home/user/Applications/ClawCrew-x86_64.AppImage %U\n\
             Icon=clawcrew\n\
             Type=Application\n";
        assert_eq!(
            clawcrew_desktop_exec(entry).as_deref(),
            Some("/home/user/Applications/ClawCrew-x86_64.AppImage")
        );
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_ignores_unrelated_entry() {
        let entry = "[Desktop Entry]\n\
             Name=Some Other App\n\
             Exec=/usr/bin/other %F\n\
             Type=Application\n";
        assert_eq!(clawcrew_desktop_exec(entry), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_rejects_substring_lookalike() {
        // Identity is the visible Name, not any field containing "clawcrew":
        // an unrelated entry whose Exec merely mentions the substring must not
        // qualify, otherwise it could preempt the real companion app.
        let entry = "[Desktop Entry]\n\
             Name=Unrelated App\n\
             Exec=/tmp/not-clawcrew-helper %U\n\
             Type=Application\n";
        assert_eq!(clawcrew_desktop_exec(entry), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_keeps_quoted_path_with_spaces() {
        let entry = "[Desktop Entry]\n\
             Name=ClawCrew\n\
             Exec=\"/home/user/My Applications/ClawCrew-x86_64.AppImage\" %U\n\
             Type=Application\n";
        assert_eq!(
            clawcrew_desktop_exec(entry).as_deref(),
            Some("/home/user/My Applications/ClawCrew-x86_64.AppImage")
        );
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_rejects_unquoted_reserved_and_escaped_space() {
        // Per the Desktop Entry spec a space (a reserved character) must be
        // quoted; a backslash-escaped space outside quotes is malformed. The
        // parser fails closed rather than launching a partially interpreted path.
        let escaped_space = "[Desktop Entry]\n\
             Name=ClawCrew\n\
             Exec=/home/user/My\\ Apps/clawcrew-desktop %U\n\
             Type=Application\n";
        assert_eq!(clawcrew_desktop_exec(escaped_space), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_decodes_quoted_literal_dollar_and_backslash() {
        // A literal `$` in a quoted path is written `\\$` (general unescape
        // `\\`->`\`, then the Exec layer unescapes `\$`->`$`); a literal
        // backslash is written `\\\\`.
        let dollar = "[Desktop Entry]\n\
             Name=ClawCrew\n\
             Exec=\"/opt/\\\\$dir/clawcrew-desktop\" %U\n\
             Type=Application\n";
        assert_eq!(
            clawcrew_desktop_exec(dollar).as_deref(),
            Some("/opt/$dir/clawcrew-desktop")
        );
        let backslash = "[Desktop Entry]\n\
             Name=ClawCrew\n\
             Exec=\"/opt/a\\\\\\\\b/clawcrew-desktop\"\n\
             Type=Application\n";
        assert_eq!(
            clawcrew_desktop_exec(backslash).as_deref(),
            Some("/opt/a\\b/clawcrew-desktop")
        );
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn parse_exec_program_fails_closed_on_malformed_input() {
        // Unterminated quote.
        assert_eq!(parse_exec_program("\"/opt/clawcrew-desktop"), None);
        // Dangling escape inside a quote.
        assert_eq!(parse_exec_program("\"/opt/clawcrew\\"), None);
        // Dangling escape outside quotes (invalid general escape).
        assert_eq!(parse_exec_program("/opt/clawcrew\\"), None);
        // A forbidden `=` in the executable token.
        assert_eq!(parse_exec_program("/opt/a=b/clawcrew-desktop"), None);
        // Unquoted reserved character.
        assert_eq!(parse_exec_program("/opt/$HOME/clawcrew-desktop"), None);
        // A valid bare token still parses.
        assert_eq!(
            parse_exec_program("clawcrew-desktop %U").as_deref(),
            Some("clawcrew-desktop")
        );
        // The WHOLE line is validated, not just the first token:
        // an unknown field code invalidates it.
        assert_eq!(parse_exec_program("clawcrew-desktop %Z"), None);
        // Text directly adjacent to a closing quote is malformed.
        assert_eq!(parse_exec_program("\"/opt/clawcrew-desktop\"junk"), None);
        // A raw (unescaped) reserved character inside quotes is malformed.
        assert_eq!(parse_exec_program("\"/opt/$HOME/clawcrew-desktop\""), None);
        assert_eq!(parse_exec_program("\"/opt/`x`/clawcrew-desktop\""), None);
        // Known field codes and extra plain args are accepted.
        assert_eq!(
            parse_exec_program("clawcrew-desktop %U --flag").as_deref(),
            Some("clawcrew-desktop")
        );
        assert_eq!(
            parse_exec_program("clawcrew-desktop %%").as_deref(),
            Some("clawcrew-desktop")
        );
        // A field code embedded in the PROGRAM token (not just a leading `%`)
        // invalidates it, even though the basename would pass the AppImage-name
        // check — both an unknown (`%Z`) and a known (`%U`) code are rejected.
        assert_eq!(parse_exec_program("/tmp/ClawCrew-%Z.AppImage"), None);
        assert_eq!(parse_exec_program("/tmp/ClawCrew-%U.AppImage"), None);
        // A field code embedded in an ARGUMENT token (must stand alone) is
        // rejected for both unknown and known codes.
        assert_eq!(parse_exec_program("clawcrew-desktop --flag=%Z"), None);
        assert_eq!(parse_exec_program("clawcrew-desktop --flag=%U"), None);
        // A field code inside a quoted argument is rejected — the quote context
        // is retained so `"%U"` cannot masquerade as a standalone field code.
        assert_eq!(parse_exec_program("clawcrew-desktop \"%U\""), None);
        // An escaped literal percent embedded in a path stays valid.
        assert_eq!(
            parse_exec_program("/opt/clawcrew-desktop 100%%done").as_deref(),
            Some("/opt/clawcrew-desktop")
        );
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_strips_field_codes_and_quotes() {
        let entry = "[Desktop Entry]\n\
             Name=ClawCrew Companion\n\
             Exec=\"/opt/clawcrew/clawcrew-desktop\" %u\n\
             Type=Application\n";
        assert_eq!(
            clawcrew_desktop_exec(entry).as_deref(),
            Some("/opt/clawcrew/clawcrew-desktop")
        );
        // A bare field code with no real command must not resolve.
        let bad = "[Desktop Entry]\nName=ClawCrew\nExec=%U\nType=Application\n";
        assert_eq!(clawcrew_desktop_exec(bad), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_honours_hidden_and_group_scope() {
        // Otherwise a fully valid ClawCrew Application entry — it resolves only
        // because `Hidden=true` masks it, so the fixture actually exercises the
        // Hidden rule rather than passing on some other missing field.
        let masked = "[Desktop Entry]\n\
             Type=Application\n\
             Name=ClawCrew\n\
             Exec=/opt/clawcrew/clawcrew-desktop\n\
             Hidden=true\n";
        assert_eq!(clawcrew_desktop_exec(masked), None);

        // Only the [Desktop Entry] group is consulted. The main group is an
        // otherwise valid ClawCrew Application with no Name of its own, so it
        // resolves iff a `Name=ClawCrew` from the Desktop Action group leaks in.
        // It must not.
        let action_only = "[Desktop Entry]\n\
             Type=Application\n\
             Exec=/opt/clawcrew/clawcrew-desktop\n\
             [Desktop Action foo]\n\
             Name=ClawCrew\n\
             Exec=/tmp/evil\n";
        assert_eq!(clawcrew_desktop_exec(action_only), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn is_clawcrew_name_matches_deliberate_identity() {
        assert!(is_clawcrew_name("ClawCrew"));
        assert!(is_clawcrew_name("clawcrew"));
        assert!(is_clawcrew_name("ClawCrew Companion"));
        assert!(is_clawcrew_name("ClawCrew-desktop"));
        assert!(!is_clawcrew_name("ClawCrewesome"));
        assert!(!is_clawcrew_name("Not ClawCrew"));
        assert!(!is_clawcrew_name("Some Other App"));
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn discover_desktop_app_honours_precedence_masking_and_executability() {
        use std::os::unix::fs::PermissionsExt;

        fn write_exec(path: &Path) {
            std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
        fn write_entry(dir: &Path, id: &str, exec: &Path) {
            let apps = dir.join("applications");
            std::fs::create_dir_all(&apps).unwrap();
            std::fs::write(
                apps.join(id),
                format!(
                    "[Desktop Entry]\nName=ClawCrew\nExec={}\nType=Application\n",
                    exec.display()
                ),
            )
            .unwrap();
        }

        let high = tempfile::tempdir().unwrap();
        let low = tempfile::tempdir().unwrap();

        // Both are the supported `clawcrew-desktop` binary, in separate dirs.
        let high_bin = high.path().join("clawcrew-desktop");
        let low_bin = low.path().join("clawcrew-desktop");
        write_exec(&high_bin);
        write_exec(&low_bin);

        // Same desktop-file ID in both dirs: the higher-precedence one wins.
        write_entry(high.path(), "ClawCrew.desktop", &high_bin);
        write_entry(low.path(), "ClawCrew.desktop", &low_bin);

        let dirs = [high.path().to_path_buf(), low.path().to_path_buf()];
        assert_eq!(
            discover_desktop_app(&dirs).as_deref(),
            Some(high_bin.as_path())
        );

        // A non-executable Exec target is skipped rather than returned.
        let broken = tempfile::tempdir().unwrap();
        let non_exec = broken.path().join("clawcrew-desktop");
        std::fs::write(&non_exec, "not executable").unwrap();
        write_entry(broken.path(), "ClawCrew.desktop", &non_exec);
        assert_eq!(discover_desktop_app(&[broken.path().to_path_buf()]), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn discover_desktop_app_skips_lookalike_ordered_before_real_app() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join("applications");
        std::fs::create_dir_all(&apps).unwrap();

        fn write_exec(path: &Path) {
            std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }

        // A lexically earlier entry (`000...`) with a ClawCrew Name but a
        // lookalike executable must not preempt the real companion app.
        let lookalike = dir.path().join("clawcrew-helper");
        let real = dir.path().join("clawcrew-desktop");
        write_exec(&lookalike);
        write_exec(&real);
        std::fs::write(
            apps.join("000-lookalike.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=ClawCrew\nExec={}\n",
                lookalike.display()
            ),
        )
        .unwrap();
        std::fs::write(
            apps.join("zzz-real.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=ClawCrew\nExec={}\n",
                real.display()
            ),
        )
        .unwrap();

        assert_eq!(
            discover_desktop_app(&[dir.path().to_path_buf()]).as_deref(),
            Some(real.as_path())
        );
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn discover_desktop_app_higher_precedence_hidden_masks_lower_valid() {
        use std::os::unix::fs::PermissionsExt;

        fn write_exec(path: &Path) {
            std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
        fn write_entry(dir: &Path, body: &str) {
            let apps = dir.join("applications");
            std::fs::create_dir_all(&apps).unwrap();
            std::fs::write(apps.join("ClawCrew.desktop"), body).unwrap();
        }

        let high = tempfile::tempdir().unwrap();
        let low = tempfile::tempdir().unwrap();
        let low_bin = low.path().join("clawcrew-desktop");
        write_exec(&low_bin);

        // A higher-precedence Hidden=true entry masks the same desktop-file ID in
        // the lower directory, so the lower (valid) entry must not be launched.
        write_entry(
            high.path(),
            "[Desktop Entry]\nType=Application\nName=ClawCrew\nExec=/opt/clawcrew/clawcrew-desktop\nHidden=true\n",
        );
        write_entry(
            low.path(),
            &format!(
                "[Desktop Entry]\nType=Application\nName=ClawCrew\nExec={}\n",
                low_bin.display()
            ),
        );

        let dirs = [high.path().to_path_buf(), low.path().to_path_buf()];
        assert_eq!(discover_desktop_app(&dirs), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn resolve_executable_rejects_relative_path_with_separator() {
        // A relative Exec value with a separator would be resolved by `which` against the
        // current working directory, so it must be rejected rather than launched.
        assert_eq!(resolve_executable("./clawcrew-helper"), None);
        assert_eq!(resolve_executable("../bin/clawcrew-helper"), None);
        assert_eq!(resolve_executable("sub/dir/clawcrew-helper"), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn collect_desktop_entries_does_not_follow_directory_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("ClawCrew.desktop"),
            "[Desktop Entry]\nName=ClawCrew\nExec=/usr/bin/clawcrew\nType=Application\n",
        )
        .unwrap();
        // A directory symlink pointing back at its own parent would recurse forever if
        // followed. The scan must treat it as a non-directory and terminate.
        std::os::unix::fs::symlink(&apps, apps.join("loop")).unwrap();

        let mut out: Vec<(String, PathBuf)> = Vec::new();
        let mut visited: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        collect_desktop_entries(&apps, &apps, &mut out, &mut visited);

        // Terminates (no infinite loop) and collects only the real entry.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "ClawCrew.desktop");
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn discover_desktop_app_skips_special_symlink_and_oversized_entries() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join("applications");
        std::fs::create_dir_all(&apps).unwrap();

        let fifo = apps.join("000-fifo.desktop");
        let fifo_name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `fifo_name` is a live, NUL-terminated pathname and the mode is
        // a valid permission bitmask. The return value is checked immediately.
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
        assert_eq!(read_desktop_entry(&fifo), None);

        let fifo_link = apps.join("001-fifo-link.desktop");
        std::os::unix::fs::symlink(&fifo, &fifo_link).unwrap();
        assert_eq!(read_desktop_entry(&fifo_link), None);

        let oversized = apps.join("002-oversized.desktop");
        let oversized_len = usize::try_from(DESKTOP_ENTRY_MAX_BYTES).unwrap() + 1;
        std::fs::write(&oversized, vec![b'x'; oversized_len]).unwrap();
        assert_eq!(read_desktop_entry(&oversized), None);

        let real = dir.path().join("clawcrew-desktop");
        std::fs::write(&real, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = std::fs::metadata(&real).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&real, permissions).unwrap();
        std::fs::write(
            apps.join("zzz-real.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=ClawCrew\nExec={}\n",
                real.display()
            ),
        )
        .unwrap();

        assert_eq!(
            discover_desktop_app(&[dir.path().to_path_buf()]).as_deref(),
            Some(real.as_path())
        );
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_rejects_clawcrew_name_with_unrelated_exec() {
        // A ClawCrew display name paired with an unrelated executable must not
        // resolve: identity is Type + Name + a ClawCrew-shaped Exec target, not
        // the display name alone. A lexically earlier entry like this must not
        // preempt the real app.
        let entry = "[Desktop Entry]\n\
             Name=ClawCrew Helper\n\
             Exec=/tmp/unrelated %U\n\
             Type=Application\n";
        assert_eq!(clawcrew_desktop_exec(entry), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn clawcrew_desktop_exec_requires_application_type() {
        // A non-Application entry never resolves, even with a ClawCrew Name and
        // a ClawCrew executable.
        let link = "[Desktop Entry]\n\
             Name=ClawCrew\n\
             Exec=/opt/clawcrew/clawcrew-desktop\n\
             Type=Link\n";
        assert_eq!(clawcrew_desktop_exec(link), None);

        // Missing Type is also rejected (the published entry always sets it).
        let no_type = "[Desktop Entry]\n\
             Name=ClawCrew\n\
             Exec=/opt/clawcrew/clawcrew-desktop\n";
        assert_eq!(clawcrew_desktop_exec(no_type), None);
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn is_clawcrew_appimage_name_anchors_identity() {
        // The published `ClawCrew-*.AppImage` form (separator required).
        assert!(is_clawcrew_appimage_name("ClawCrew-x86_64.AppImage"));
        assert!(is_clawcrew_appimage_name("clawcrew-aarch64.appimage"));
        // A no-boundary lookalike must not qualify.
        assert!(!is_clawcrew_appimage_name("ClawCrewevil.AppImage"));
        // Missing the separator (not a published form).
        assert!(!is_clawcrew_appimage_name("clawcrew.appimage"));
        // A lookalike whose name merely contains the substring must not qualify.
        assert!(!is_clawcrew_appimage_name("not-clawcrew-helper.AppImage"));
        assert!(!is_clawcrew_appimage_name("ClawCrew.txt"));
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn is_clawcrew_program_binds_to_supported_names() {
        // Exact published binary, or a published-form AppImage.
        assert!(is_clawcrew_program("/usr/bin/clawcrew-desktop"));
        assert!(is_clawcrew_program(
            "/home/user/Applications/ClawCrew-x86_64.AppImage"
        ));
        // Lookalikes sharing the prefix are rejected.
        assert!(!is_clawcrew_program("/tmp/clawcrew-helper"));
        assert!(!is_clawcrew_program("/tmp/clawcrew-evil"));
        assert!(!is_clawcrew_program("/usr/bin/clawcrew"));
    }

    #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
    #[test]
    fn discover_desktop_app_masks_nested_desktop_file_ids() {
        use std::os::unix::fs::PermissionsExt;

        fn write_exec(path: &Path) {
            std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
        fn write_nested_entry(dir: &Path, rel_id: &str, exec: &Path) {
            let full = dir.join("applications").join(rel_id);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(
                full,
                format!(
                    "[Desktop Entry]\nName=ClawCrew\nExec={}\nType=Application\n",
                    exec.display()
                ),
            )
            .unwrap();
        }

        let high = tempfile::tempdir().unwrap();
        let low = tempfile::tempdir().unwrap();
        let high_bin = high.path().join("clawcrew-desktop");
        let low_bin = low.path().join("clawcrew-desktop");
        write_exec(&high_bin);
        write_exec(&low_bin);

        // Same nested desktop-file ID (`vendor/ClawCrew.desktop` -> ID
        // `vendor-ClawCrew.desktop`) in both dirs: the higher-precedence entry
        // must mask the lower one, which only works if IDs are derived
        // recursively rather than from top-level basenames.
        write_nested_entry(high.path(), "vendor/ClawCrew.desktop", &high_bin);
        write_nested_entry(low.path(), "vendor/ClawCrew.desktop", &low_bin);

        let dirs = [high.path().to_path_buf(), low.path().to_path_buf()];
        assert_eq!(
            discover_desktop_app(&dirs).as_deref(),
            Some(high_bin.as_path())
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn openrc_log_writer_cli_maps_only_known_streams() {
        for (value, expected) in [
            ("stdout", ServiceLogStream::Stdout),
            ("stderr", ServiceLogStream::Stderr),
        ] {
            let cli = Cli::try_parse_from(["clawcrew", "service", "run-openrc-log-writer", value])
                .expect("internal OpenRC logger should parse");
            assert!(matches!(
                cli.command,
                Commands::Service {
                    service_command: ServiceCommands::RunOpenrcLogWriter { stream },
                    ..
                } if stream == expected
            ));
        }
        assert!(
            Cli::try_parse_from([
                "clawcrew",
                "service",
                "run-openrc-log-writer",
                "/tmp/arbitrary.log"
            ])
            .is_err()
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn desktop_daemon_cli_parses_hidden_command() {
        let cli = Cli::try_parse_from([
            "clawcrew",
            "service",
            "run-desktop-daemon",
            "--port",
            "42617",
        ])
        .expect("internal desktop daemon should parse");
        assert!(matches!(
            cli.command,
            Commands::Service {
                service_command: ServiceCommands::RunDesktopDaemon { port },
                ..
            } if port == 42617
        ));

        let help = Cli::command().render_help().to_string();
        assert!(!help.contains("run-desktop-daemon"));
    }

    #[test]
    fn probe_config_dir_extracts_global_flag_in_all_forms() {
        fn argv(parts: &[&str]) -> std::vec::IntoIter<std::ffi::OsString> {
            parts
                .iter()
                .map(|s| std::ffi::OsString::from(*s))
                .collect::<Vec<_>>()
                .into_iter()
        }

        let command = Cli::command();

        // argv[0] is consumed by clap as the binary name.
        // Space form.
        assert_eq!(
            probe_config_dir(&command, argv(&["clawcrew", "--config-dir", "/x"])),
            Some("/x".to_string())
        );
        // Equals form.
        assert_eq!(
            probe_config_dir(&command, argv(&["clawcrew", "--config-dir=/y"])),
            Some("/y".to_string())
        );
        // Global arg: may appear *after* a subcommand.
        assert_eq!(
            probe_config_dir(
                &command,
                argv(&["clawcrew", "status", "--config-dir", "/z"])
            ),
            Some("/z".to_string())
        );
        // Absent.
        assert_eq!(
            probe_config_dir(&command, argv(&["clawcrew", "status"])),
            None
        );
        // `--` ends option parsing; later values must never redirect config.
        assert_eq!(
            probe_config_dir(
                &command,
                argv(&[
                    "clawcrew",
                    "config",
                    "set",
                    "locale",
                    "--",
                    "--config-dir=/ignored",
                ])
            ),
            None
        );
        // Present but empty — returned verbatim for clap's validation path.
        assert_eq!(
            probe_config_dir(&command, argv(&["clawcrew", "--config-dir", ""])),
            Some(String::new())
        );
    }

    #[test]
    fn probe_config_dir_follows_clap_token_ownership() {
        fn argv(parts: &[&str]) -> std::vec::IntoIter<std::ffi::OsString> {
            parts
                .iter()
                .map(|s| std::ffi::OsString::from(*s))
                .collect::<Vec<_>>()
                .into_iter()
        }

        let command = Cli::command();
        let external_payload = [
            "clawcrew",
            "props",
            "legacy-command",
            "--config-dir=/unintended",
        ];

        // The external subcommand owns every remaining token, including one
        // that looks like a global option.
        let cli = Cli::try_parse_from(external_payload)
            .expect("the deprecated external-subcommand path is valid clap input");
        assert!(cli.config_dir.is_none());
        assert_eq!(probe_config_dir(&command, argv(&external_payload)), None);

        // Option-looking and terminating tokens cannot satisfy the spaced
        // form's required value.
        assert!(Cli::try_parse_from(["clawcrew", "--config-dir", "--help"]).is_err());
        assert_eq!(
            probe_config_dir(&command, argv(&["clawcrew", "--config-dir", "--help"])),
            None
        );
        assert_eq!(
            probe_config_dir(&command, argv(&["clawcrew", "--config-dir", "--"])),
            None
        );
    }

    #[test]
    fn acp_cli_accepts_process_default_agent() {
        let cli = Cli::try_parse_from(["clawcrew", "acp", "--agent", "fable"])
            .expect("standalone ACP should accept a process default agent");

        match cli.command {
            Commands::Acp { agent, .. } => {
                assert_eq!(agent.as_deref(), Some("fable"));
            }
            other => panic!("expected ACP command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn cli_quickstart_uses_advertised_local_provider_runtime_default() {
        let providers = vec![clawcrew_runtime::quickstart::QuickstartTypeOption {
            kind: "lmstudio".into(),
            display_name: "LM Studio".into(),
            local: true,
            default_runtime_profile: Some("local_small".into()),
        }];

        assert_eq!(
            quickstart_runtime_profile_for_provider("lmstudio", &providers, "unbounded"),
            "local_small"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn cli_quickstart_uses_advertised_remote_provider_runtime_default() {
        let providers = vec![clawcrew_runtime::quickstart::QuickstartTypeOption {
            kind: "anthropic".into(),
            display_name: "Anthropic".into(),
            local: false,
            default_runtime_profile: Some("unbounded".into()),
        }];

        assert_eq!(
            quickstart_runtime_profile_for_provider("anthropic", &providers, "unbounded"),
            "unbounded"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn cli_quickstart_uses_state_fallback_when_provider_has_no_override() {
        let providers = vec![clawcrew_runtime::quickstart::QuickstartTypeOption {
            kind: "ollama".into(),
            display_name: "Ollama".into(),
            local: true,
            default_runtime_profile: None,
        }];

        assert_eq!(
            quickstart_runtime_profile_for_provider("ollama", &providers, "unbounded"),
            "unbounded"
        );
    }

    #[test]
    fn cap_line_utf8_safe_no_panic_on_multibyte_boundary() {
        // Neutral multi-byte placeholder text; each CJK char is 3 bytes, so a
        // byte cap can land inside a character. Pre-fix this panicked via the
        // raw `String::truncate(cap)`.
        let mut line = "语言".repeat(64); // 128 chars, 384 bytes, all 3-byte
        let cap = 10; // byte index 10 is mid-character (10 % 3 != 0)
        assert!(
            !line.is_char_boundary(cap),
            "precondition: cap splits a char"
        );
        cap_line_utf8_safe(&mut line, cap);
        assert!(line.len() <= cap, "must not exceed the byte cap");
        assert!(
            line.is_char_boundary(line.len()),
            "result must end on a valid UTF-8 char boundary"
        );
        // cap 10 floors to byte 9 = three whole 3-byte chars.
        assert_eq!(
            line, "语言语",
            "should keep whole chars up to the floored cap"
        );
    }

    #[test]
    fn cap_line_utf8_safe_is_noop_when_within_cap() {
        let mut line = String::from("héllo"); // 6 bytes
        cap_line_utf8_safe(&mut line, 1024);
        assert_eq!(line, "héllo");
    }

    #[test]
    fn cap_line_utf8_safe_ascii_exact_cap() {
        let mut line = String::from("abcdefgh");
        cap_line_utf8_safe(&mut line, 4);
        assert_eq!(line, "abcd");
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn cli_definition_has_no_flag_conflicts() {
        Cli::command().debug_assert();
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn quickstart_inline_auth_uses_auth_mode_field() {
        let fields =
            std::collections::HashMap::from([("auth_mode".to_string(), " codex ".to_string())]);
        assert_eq!(
            quickstart_inline_auth("openai", "codex", &fields),
            Some(InlineProviderAuth::Codex)
        );

        let fields =
            std::collections::HashMap::from([("auth_mode".to_string(), "setup_token".to_string())]);
        assert_eq!(
            quickstart_inline_auth("anthropic", "max", &fields),
            Some(InlineProviderAuth::AnthropicSetupToken {
                alias: "max".to_string()
            })
        );

        assert_eq!(quickstart_inline_auth("openai", "api", &fields), None);
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn ensure_map_key_materializes_typed_provider_entries() {
        use crate::config::schema::Config;
        for (path, value) in [
            ("providers.models.openai.default.model", "gpt-4o"),
            ("providers.tts.openai.default.voice", "alloy"),
            ("providers.transcription.openai.default.model", "whisper-1"),
            ("channels.telegram.default.bot_token", "tok"),
        ] {
            let mut config = Config::default();
            assert!(
                config.set_prop(path, value).is_err(),
                "precondition: {path} should be unknown on a fresh config"
            );
            config.ensure_map_key_for_path(path);
            assert!(
                config.set_prop(path, value).is_ok(),
                "{path} must be settable after map-key materialization"
            );
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn ensure_map_key_ignores_non_map_paths() {
        use crate::config::schema::Config;
        let mut config = Config::default();
        config.ensure_map_key_for_path("gateway.port");
        config.ensure_map_key_for_path("locale");
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn onboard_help_includes_model_flag() {
        let cmd = Cli::command();
        let onboard = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "onboard")
            .expect("onboard subcommand must exist");

        let has_model_flag = onboard
            .get_arguments()
            .any(|arg| arg.get_id().as_str() == "model" && arg.get_long() == Some("model"));

        assert!(
            has_model_flag,
            "onboard help should include --model for quick setup overrides"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn gateway_admin_url_uses_unprefixed_admin_path_by_default() {
        assert_eq!(
            gateway_admin_url("127.0.0.1", 42617, None, "/admin/paircode"),
            "http://127.0.0.1:42617/admin/paircode"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn gateway_admin_url_prepends_configured_path_prefix() {
        assert_eq!(
            gateway_admin_url("localhost", 42617, Some("/clawcrew"), "/admin/paircode/new"),
            "http://localhost:42617/clawcrew/admin/paircode/new"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn onboard_cli_accepts_model_provider_and_api_key_in_quick_mode() {
        let cli = Cli::try_parse_from([
            "clawcrew",
            "onboard",
            "--model-provider",
            "openrouter",
            "--model",
            "custom-model-946",
            "--api-key",
            "sk-issue946",
        ])
        .expect("quick onboard invocation should parse");

        match cli.command {
            Commands::Onboard {
                force,
                channels_only,
                api_key,
                model_provider,
                model,
                ..
            } => {
                assert!(!force);
                assert!(!channels_only);
                assert_eq!(model_provider.as_deref(), Some("openrouter"));
                assert_eq!(model.as_deref(), Some("custom-model-946"));
                assert_eq!(api_key.as_deref(), Some("sk-issue946"));
            }
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn completions_cli_parses_supported_shells() {
        for shell in ["bash", "fish", "zsh", "powershell", "elvish"] {
            let cli = Cli::try_parse_from(["clawcrew", "completions", shell])
                .expect("completions invocation should parse");
            match cli.command {
                Commands::Completions { .. } => {}
                other => panic!("expected completions command, got {other:?}"),
            }
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn completion_generation_mentions_binary_name() {
        let mut output = Vec::new();
        write_shell_completion(CompletionShell::Bash, &mut output)
            .expect("completion generation should succeed");
        let script = String::from_utf8(output).expect("completion output should be valid utf-8");
        assert!(
            script.contains("clawcrew"),
            "completion script should reference binary name"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn bash_completion_avoids_infinite_recursion() {
        let mut output = Vec::new();
        write_shell_completion(CompletionShell::Bash, &mut output)
            .expect("completion generation should succeed");
        let script = String::from_utf8(output).expect("completion output should be valid utf-8");
        // The wrapper must capture the original clap-generated function body
        // (via declare -f) rather than calling _clawcrew by name, which would
        // create an infinite recursion loop after _clawcrew is redefined.
        assert!(
            script.contains("declare -f _clawcrew"),
            "bash completion should use declare -f to capture the original _clawcrew function body"
        );
        assert!(
            !script.contains("_clawcrew_clap_orig() { _clawcrew \"$@\"; }"),
            "bash completion must not define _clawcrew_clap_orig as a simple forwarder to _clawcrew"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn onboard_cli_accepts_force_flag() {
        let cli = Cli::try_parse_from(["clawcrew", "onboard", "--force"])
            .expect("onboard --force should parse");

        match cli.command {
            Commands::Onboard { force, .. } => assert!(force),
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn onboard_cli_rejects_removed_interactive_flag() {
        // --interactive was removed; onboard auto-detects TTY instead.
        assert!(Cli::try_parse_from(["clawcrew", "onboard", "--interactive"]).is_err());
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn onboard_cli_parses_quick_flag() {
        let cli = Cli::try_parse_from(["clawcrew", "onboard", "--quick"])
            .expect("onboard --quick should parse");

        match cli.command {
            Commands::Onboard { quick, .. } => assert!(quick),
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn gateway_get_paircode_cli_accepts_port_and_host_overrides() {
        let cli = Cli::try_parse_from([
            "clawcrew",
            "gateway",
            "get-paircode",
            "--new",
            "--port",
            "3001",
            "--host",
            "192.168.1.20",
        ])
        .expect("gateway get-paircode overrides should parse");

        match cli.command {
            Commands::Gateway {
                gateway_command:
                    Some(clawcrew::GatewayCommands::GetPaircode {
                        new,
                        rotate,
                        rotate_device,
                        port,
                        host,
                    }),
            } => {
                assert!(new);
                assert!(!rotate);
                assert_eq!(rotate_device, None);
                assert_eq!(port, Some(3001));
                assert_eq!(host.as_deref(), Some("192.168.1.20"));
            }
            other => panic!("expected gateway get-paircode command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn security_status_cli_requires_agent_and_parses_json_form() {
        let err = Cli::try_parse_from(["clawcrew", "security", "status"])
            .expect_err("security status requires --agent");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);

        let cli =
            Cli::try_parse_from(["clawcrew", "security", "status", "--agent", "ops", "--json"])
                .expect("security status --agent --json should parse");
        match cli.command {
            Commands::Security {
                security_command: SecurityCommands::Status { agent, json },
            } => {
                assert_eq!(agent, "ops");
                assert!(json);
            }
            other => panic!("expected security status command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn issue_client_cert_cleans_staged_material_when_ledger_record_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = tempfile::tempdir().expect("out tempdir");
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let tls_dir = config.data_dir.join("tls");
        clawcrew_tls::ensure_server_materials(&tls_dir, &[]).expect("daemon TLS materials");
        std::fs::create_dir(tls_dir.join("ledger.db")).expect("poison ledger path");

        let err = issue_wss_client_cert(
            &config,
            "dev_under_test",
            Some(out.path().to_path_buf()),
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("ledger"), "got: {err}");
        assert!(!out.path().join("client-dev_under_test.crt").exists());
        assert!(!out.path().join("client-dev_under_test.key").exists());
        assert!(!out.path().join(".client-dev_under_test.crt.tmp").exists());
        assert!(!out.path().join(".client-dev_under_test.key.tmp").exists());
    }

    /// `delivered_at` for a fingerprint, straight from the ledger table. The
    /// ledger exposes no reader for it (nothing in production asks), so the
    /// operator-CLI test reads SQLite directly.
    #[cfg(feature = "agent-runtime")]
    fn cert_delivered_at(data_dir: &std::path::Path, fingerprint: &str) -> Option<i64> {
        let conn = rusqlite::Connection::open(data_dir.join("tls").join("ledger.db")).unwrap();
        conn.query_row(
            "SELECT delivered_at FROM issued_certs WHERE fingerprint = ?1",
            rusqlite::params![fingerprint],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// The operator CLI's publication boundary, which is the most direct of the
    /// three: `issue-client-cert` records the issuance and only then renames
    /// the staged key and certificate into place. A rename that fails leaves an
    /// ACTIVE ledger row for a credential that was never published, and a retry
    /// used to add a SECOND active row for the same device rather than
    /// replacing the first.
    /// The drop-in copies into --out-dir are operator-facing credentials, not
    /// cosmetic output: a failure there must fail the command rather than
    /// report a successful issuance over a missing or stale ca.crt.
    #[test]
    #[cfg(feature = "agent-runtime")]
    fn issue_client_cert_out_dir_drop_in_failure_fails_the_command() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = tempfile::tempdir().expect("out tempdir");
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        clawcrew_tls::ensure_server_materials(&config.data_dir.join("tls"), &[])
            .expect("daemon TLS materials");

        // Obstruct the drop-in ca.crt with a non-empty directory so the copy
        // fails after the primary named files were published.
        let ca_dest = out.path().join("ca.crt");
        std::fs::create_dir(&ca_dest).expect("obstruct ca.crt");
        std::fs::write(ca_dest.join("occupied"), b"x").expect("occupy it");

        let err = issue_wss_client_cert(
            &config,
            "dev_dropin_test",
            Some(out.path().to_path_buf()),
            true,
        )
        .expect_err("an incomplete drop-in directory must fail the command")
        .to_string();
        assert!(
            err.contains("ca.crt") && err.contains("drop-in"),
            "the error must name the drop-in file and directory: {err}"
        );
        assert!(
            err.contains("issued"),
            "the error must say the primary credentials were still issued: {err}"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn issue_client_cert_rename_failure_leaves_an_undelivered_row_that_reconciles_away() {
        use clawcrew_runtime::security::cert_ledger::{CertLedger, CertStatus, revoked_list_path};
        let dir = tempfile::tempdir().expect("tempdir");
        let out = tempfile::tempdir().expect("out tempdir");
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        clawcrew_tls::ensure_server_materials(&config.data_dir.join("tls"), &[])
            .expect("daemon TLS materials");

        // Make the publication rename fail the way a real filesystem does:
        // the destination is a non-empty directory, so renaming a file onto it
        // cannot succeed. `--force` gets past the "already exists" guard, which
        // is exactly how an operator re-issuing over a broken layout arrives
        // here.
        let key_dest = out.path().join("client-dev_under_test.key");
        std::fs::create_dir(&key_dest).expect("obstruct the key destination");
        std::fs::write(key_dest.join("occupied"), b"x").expect("occupy it");

        let err = issue_wss_client_cert(
            &config,
            "dev_under_test",
            Some(out.path().to_path_buf()),
            true,
        )
        .expect_err("an unpublishable certificate must fail the command")
        .to_string();
        assert!(
            err.contains("publish private key"),
            "the error must say publication failed: {err}"
        );
        assert!(
            err.contains(".client-dev_under_test.key.tmp"),
            "the error must name the STAGED file, not only the destination: {err}"
        );
        // Staged material is not left lying around as a stray private key.
        assert!(!out.path().join(".client-dev_under_test.key.tmp").exists());
        assert!(!out.path().join(".client-dev_under_test.crt.tmp").exists());

        // The row is active - promotion happens before publication by design -
        // but undelivered, because the rename never succeeded.
        let ghost = {
            let ledger = CertLedger::open(&config.data_dir, None).expect("open ledger");
            let active = ledger.list_active().expect("list active");
            assert_eq!(
                active.len(),
                1,
                "the issuance was recorded before publishing"
            );
            active[0].fingerprint.clone()
        };
        assert_eq!(
            cert_delivered_at(&config.data_dir, &ghost),
            None,
            "a failed rename must not mark the certificate delivered"
        );

        // Once the delivery deadline passes, the next ledger open revokes it.
        {
            let conn =
                rusqlite::Connection::open(config.data_dir.join("tls").join("ledger.db")).unwrap();
            conn.execute(
                "UPDATE issued_certs SET issued_at = issued_at - 7200 WHERE fingerprint = ?1",
                rusqlite::params![ghost],
            )
            .unwrap();
        }
        {
            let ledger = CertLedger::open(&config.data_dir, None).expect("reopen ledger");
            assert_eq!(
                ledger.status_of(&ghost).expect("status"),
                Some(CertStatus::Revoked),
                "an unpublished certificate must be reconciled to revoked"
            );
        }
        let crl = std::fs::read_to_string(revoked_list_path(&config.data_dir)).expect("read crl");
        assert!(
            crl.lines().any(|l| l == ghost),
            "the reconciled revocation must reach the verifier's file, got: {crl:?}"
        );

        // The retry - the operator clears the obstruction and re-issues - must
        // end with exactly ONE usable credential, not two.
        std::fs::remove_dir_all(&key_dest).expect("clear the obstruction");
        issue_wss_client_cert(
            &config,
            "dev_under_test",
            Some(out.path().to_path_buf()),
            true,
        )
        .expect("the retry must publish");

        let ledger = CertLedger::open(&config.data_dir, None).expect("reopen ledger");
        let active = ledger.list_active().expect("list active");
        assert_eq!(
            active.len(),
            1,
            "the retry must not leave a second active row for the same device, got: {:?}",
            active.iter().map(|e| &e.fingerprint).collect::<Vec<_>>()
        );
        let published = active[0].fingerprint.clone();
        assert_ne!(published, ghost, "the retry mints a fresh certificate");
        assert!(
            cert_delivered_at(&config.data_dir, &published).is_some(),
            "a published certificate must be recorded as delivered"
        );
        assert_eq!(
            ledger.status_of(&ghost).expect("status"),
            Some(CertStatus::Revoked),
            "the first attempt stays revoked - not duplicated into a second active row"
        );
        // And the files the operator asked for are actually there.
        assert!(out.path().join("client-dev_under_test.crt").is_file());
        assert!(out.path().join("client-dev_under_test.key").is_file());
    }

    /// Operator revocation through the `revoke-client-cert` handler writes the
    /// fingerprint into `<data_dir>/tls/revoked` - the exact file the WSS
    /// verifier reads - so a revoked cert is refused at the next handshake (A5).
    /// Guards the production trigger for revocation (the path the ledger revoke
    /// API exposes but nothing operator-facing reached before this command).
    #[test]
    #[cfg(feature = "agent-runtime")]
    fn revoke_client_cert_handler_materializes_the_revoked_file() {
        use clawcrew_runtime::security::cert_ledger::{
            CertLedger, CertStatus, IssuanceActor, LedgerEntry, revoked_list_path,
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let fp = "ab".repeat(32); // 64-hex fingerprint
        {
            let ledger = CertLedger::open(&config.data_dir, None).expect("open ledger");
            ledger
                .record_issued(
                    &LedgerEntry {
                        device_id: "dev_under_test".to_string(),
                        fingerprint: fp.clone(),
                        not_before: 0,
                        not_after: i64::MAX,
                        status: CertStatus::Active,
                        token_hash: String::new(),
                        actor: IssuanceActor::Operator.label(),
                        issued_at: 0,
                    },
                    false,
                )
                .expect("record issued");
        }

        // The operator command revokes by fingerprint.
        revoke_wss_client_cert(&config, Some(fp.clone()), None).expect("revoke");

        // The verifier's input file now lists the fingerprint, and the ledger
        // reflects the revocation.
        let revoked = std::fs::read_to_string(revoked_list_path(&config.data_dir))
            .expect("read revoked file");
        assert!(
            revoked.lines().any(|l| l == fp),
            "revoked file must list the revoked fingerprint, got: {revoked:?}"
        );
        let ledger = CertLedger::open(&config.data_dir, None).expect("reopen ledger");
        assert_eq!(
            ledger.status_of(&fp).expect("status"),
            Some(CertStatus::Revoked)
        );
    }

    /// `revoke-client-cert` requires exactly one of --fingerprint / --device,
    /// and `list-client-certs` parses.
    #[test]
    #[cfg(feature = "agent-runtime")]
    fn revoke_and_list_client_cert_cli_parsing() {
        // Neither selector -> rejected.
        assert!(Cli::try_parse_from(["clawcrew", "security", "revoke-client-cert"]).is_err());
        // Both selectors -> rejected (mutually exclusive).
        assert!(
            Cli::try_parse_from([
                "clawcrew",
                "security",
                "revoke-client-cert",
                "--fingerprint",
                "ab",
                "--device",
                "d",
            ])
            .is_err()
        );
        // Exactly one selector -> parses.
        let cli = Cli::try_parse_from([
            "clawcrew",
            "security",
            "revoke-client-cert",
            "--fingerprint",
            "abcd",
        ])
        .expect("single selector parses");
        match cli.command {
            Commands::Security {
                security_command:
                    SecurityCommands::RevokeClientCert {
                        fingerprint,
                        device,
                    },
            } => {
                assert_eq!(fingerprint.as_deref(), Some("abcd"));
                assert!(device.is_none());
            }
            other => panic!("expected revoke-client-cert, got {other:?}"),
        }
        // list-client-certs parses with --json.
        let cli = Cli::try_parse_from(["clawcrew", "security", "list-client-certs", "--json"])
            .expect("list parses");
        assert!(matches!(
            cli.command,
            Commands::Security {
                security_command: SecurityCommands::ListClientCerts { json: true }
            }
        ));
    }

    /// `--rotate` parses and is mutually exclusive with `--new` and
    /// `--rotate-device` so the destructive path cannot be silently combined
    /// with "add another client".
    #[test]
    #[cfg(feature = "agent-runtime")]
    fn gateway_get_paircode_rotate_flags_parse_and_conflict() {
        let cli = Cli::try_parse_from(["clawcrew", "gateway", "get-paircode", "--rotate"])
            .expect("gateway get-paircode --rotate should parse");
        match cli.command {
            Commands::Gateway {
                gateway_command: Some(clawcrew::GatewayCommands::GetPaircode { rotate, .. }),
            } => assert!(rotate),
            other => panic!("expected gateway get-paircode command, got {other:?}"),
        }

        let cli = Cli::try_parse_from([
            "clawcrew",
            "gateway",
            "get-paircode",
            "--rotate-device",
            "dash-1",
        ])
        .expect("gateway get-paircode --rotate-device should parse");
        match cli.command {
            Commands::Gateway {
                gateway_command: Some(clawcrew::GatewayCommands::GetPaircode { rotate_device, .. }),
            } => assert_eq!(rotate_device.as_deref(), Some("dash-1")),
            other => panic!("expected gateway get-paircode command, got {other:?}"),
        }

        assert!(
            Cli::try_parse_from(["clawcrew", "gateway", "get-paircode", "--new", "--rotate"])
                .is_err(),
            "--new and --rotate must conflict"
        );
        assert!(
            Cli::try_parse_from([
                "clawcrew",
                "gateway",
                "get-paircode",
                "--rotate",
                "--rotate-device",
                "dash-1"
            ])
            .is_err(),
            "--rotate and --rotate-device must conflict"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn paircode_url_combines_host_port_override_with_configured_path_prefix() {
        assert_eq!(
            gateway_admin_url(
                "127.0.0.1",
                9001,
                Some("/agents/myagent"),
                "/admin/paircode/new"
            ),
            "http://127.0.0.1:9001/agents/myagent/admin/paircode/new",
        );
        assert_eq!(
            gateway_admin_url("192.168.1.20", 42617, Some("/gw"), "/admin/paircode"),
            "http://192.168.1.20:42617/gw/admin/paircode",
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn paircode_no_code_message_explains_bare_command_does_not_mint() {
        let default = config::GatewayConfig::default();
        let msg = paircode_no_code_message(
            "127.0.0.1",
            42617,
            &default.host,
            default.port,
            &PaircodeAction::Show,
            true,
            Some("Pairing is active but no new code available (already paired or code expired)"),
        );

        assert!(msg.contains(&t(
            "cli-pairing-show-only",
            "`clawcrew gateway get-paircode` only displays an existing active code; it does not mint a new one.",
        )));
        assert!(msg.contains("clawcrew gateway get-paircode --new"));
        assert!(msg.contains("clawcrew gateway get-paircode --rotate"));
        assert!(msg.contains("open http://127.0.0.1:42617"));
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn paircode_no_code_message_preserves_host_port_on_suggestions() {
        let default = config::GatewayConfig::default();
        let msg = paircode_no_code_message(
            "192.168.1.20",
            9001,
            &default.host,
            default.port,
            &PaircodeAction::Show,
            true,
            None,
        );

        assert!(
            msg.contains("clawcrew gateway get-paircode --new --port 9001 --host 192.168.1.20")
        );
        assert!(
            msg.contains("clawcrew gateway get-paircode --rotate --port 9001 --host 192.168.1.20")
        );
        assert!(msg.contains("open http://192.168.1.20:9001"));
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn paircode_no_code_message_uses_loopback_browser_hint_for_wildcard_hosts() {
        let default = config::GatewayConfig::default();

        for (host, browser_host) in [("0.0.0.0", "127.0.0.1"), ("::", "[::1]"), ("[::]", "[::1]")] {
            let msg = paircode_no_code_message(
                host,
                9001,
                &default.host,
                default.port,
                &PaircodeAction::Show,
                true,
                None,
            );

            assert!(
                msg.contains(&format!("open http://{browser_host}:9001")),
                "{msg}"
            );
            assert!(msg.contains(&format!("--port 9001 --host {host}")), "{msg}");
            assert!(!msg.contains(&format!("open http://{host}:9001")), "{msg}");
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn paircode_no_code_message_omits_configured_default_host_port() {
        let msg = paircode_no_code_message(
            "192.168.1.20",
            9001,
            "192.168.1.20",
            9001,
            &PaircodeAction::Show,
            true,
            None,
        );

        assert!(msg.contains("clawcrew gateway get-paircode --new\n"));
        assert!(msg.contains("clawcrew gateway get-paircode --rotate\n"));
        assert!(!msg.contains("--port 9001"));
        assert!(!msg.contains("--host 192.168.1.20"));
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn paircode_no_code_message_for_new_suggests_rotate() {
        let default = config::GatewayConfig::default();
        let msg = paircode_no_code_message(
            "127.0.0.1",
            42617,
            &default.host,
            default.port,
            &PaircodeAction::AddClient,
            true,
            Some("Pairing is active but no new code available (already paired or code expired)"),
        );

        assert!(msg.contains(&t(
            "cli-pairing-new-code-unavailable",
            "The gateway did not mint a new pairing code. A code may already be pending, or pairing may need a reset.",
        )));
        assert!(msg.contains("clawcrew gateway get-paircode --rotate"));
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn paircode_no_code_message_preserves_localized_output_for_show_and_disabled_branches() {
        let default = config::GatewayConfig::default();
        let show = paircode_no_code_message(
            &default.host,
            default.port,
            &default.host,
            default.port,
            &PaircodeAction::Show,
            true,
            None,
        );
        let expected_show = indent_paircode_lines(vec![
            t(
                "cli-pairing-no-code",
                "🔐 Gateway pairing is enabled, but no active pairing code is available.",
            ),
            String::new(),
            t(
                "cli-pairing-show-only",
                "`clawcrew gateway get-paircode` only displays an existing active code; it does not mint a new one.",
            ),
            t("cli-pairing-pair-another", "To pair another device, run:"),
            "    clawcrew gateway get-paircode --new".into(),
            String::new(),
            t(
                "cli-pairing-revoke-replace",
                "To revoke existing pairings and mint a replacement code, run:",
            ),
            "    clawcrew gateway get-paircode --rotate".into(),
            String::new(),
            t("cli-pairing-inspect", "To inspect the running gateway:"),
            "    open http://127.0.0.1:42617".into(),
        ]);
        assert_eq!(show, expected_show);

        let disabled = paircode_no_code_message(
            &default.host,
            default.port,
            &default.host,
            default.port,
            &PaircodeAction::Show,
            false,
            None,
        );
        let expected_disabled = indent_paircode_lines(vec![
            t(
                "cli-pairing-disabled",
                "⚠️  Gateway pairing is disabled in config.",
            ),
            t(
                "cli-pairing-requests-accepted",
                "All requests will be accepted without authentication.",
            ),
            t(
                "cli-pairing-enable-config",
                "To enable pairing, set [gateway] require_pairing = true.",
            ),
        ]);
        assert_eq!(disabled, expected_disabled);
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn paircode_no_code_message_preserves_localized_action_recovery_branches() {
        let default = config::GatewayConfig::default();
        let add_client = paircode_no_code_message(
            &default.host,
            default.port,
            &default.host,
            default.port,
            &PaircodeAction::AddClient,
            true,
            None,
        );
        assert!(add_client.contains(&t(
            "cli-pairing-new-code-unavailable",
            "The gateway did not mint a new pairing code. A code may already be pending, or pairing may need a reset.",
        )));
        assert!(add_client.contains(&t(
            "cli-pairing-retry-or-rotate",
            "Try again shortly, or revoke existing pairings and mint a replacement code:",
        )));

        let rotate = paircode_no_code_message(
            &default.host,
            default.port,
            &default.host,
            default.port,
            &PaircodeAction::RotateAll,
            true,
            None,
        );
        assert!(rotate.contains(&t(
            "cli-pairing-rotate-no-code",
            "The rotate request completed without returning a replacement code.",
        )));
        assert!(rotate.contains(&t(
            "cli-pairing-check-enabled",
            "Check whether pairing is enabled, then request a new device code:",
        )));
    }

    #[test]
    fn gateway_addr_in_use_message_guides_default_gateway_recovery() {
        let default = config::GatewayConfig::default();
        let msg = gateway_addr_in_use_message(
            "127.0.0.1",
            42617,
            &default.host,
            default.port,
            Some(42618),
        );

        assert!(msg.contains("Port 42617 is already in use"));
        assert!(msg.contains("open http://127.0.0.1:42617"));
        assert!(msg.contains("clawcrew gateway get-paircode\n"));
        assert!(msg.contains("clawcrew gateway start --port 42618"));
        assert!(msg.contains("lsof -nP -iTCP:42617 -sTCP:LISTEN"));
    }

    #[test]
    fn gateway_addr_in_use_message_keeps_non_default_host_context() {
        let default = config::GatewayConfig::default();
        let msg =
            gateway_addr_in_use_message("0.0.0.0", 9001, &default.host, default.port, Some(9002));

        assert!(!msg.contains("open http://127.0.0.1:42617"));
        assert!(msg.contains("clawcrew gateway get-paircode --port 9001 --host 0.0.0.0"));
        assert!(msg.contains("clawcrew gateway start --port 9002 --host 0.0.0.0"));
        assert!(msg.contains("lsof -nP -iTCP:9001 -sTCP:LISTEN"));
    }

    #[test]
    fn gateway_addr_in_use_message_uses_loopback_browser_hint_for_wildcard_default() {
        for (host, browser_host) in [("0.0.0.0", "127.0.0.1"), ("::", "[::1]"), ("[::]", "[::1]")] {
            let msg = gateway_addr_in_use_message(host, 9001, host, 9001, None);

            assert!(
                msg.contains(&format!("open http://{browser_host}:9001")),
                "{msg}"
            );
            assert!(!msg.contains(&format!("open http://{host}:9001")), "{msg}");
        }
    }

    #[test]
    fn gateway_addr_in_use_message_omits_restart_when_no_available_port() {
        let default = config::GatewayConfig::default();
        let msg =
            gateway_addr_in_use_message("127.0.0.1", 42617, &default.host, default.port, None);

        assert!(msg.contains("clawcrew gateway get-paircode\n"));
        assert!(!msg.contains("clawcrew gateway start --port"));
        assert!(msg.contains("lsof -nP -iTCP:42617 -sTCP:LISTEN"));
    }

    #[test]
    fn gateway_addr_in_use_message_skips_occupied_restart_hint_port() {
        let default = config::GatewayConfig::default();
        let (port, mut listeners) = reserve_consecutive_local_ports(3);
        let available_port = port + 2;
        drop(listeners.pop());

        let restart_port = available_gateway_restart_hint_port("127.0.0.1", port);
        let msg = gateway_addr_in_use_message(
            "127.0.0.1",
            port,
            &default.host,
            default.port,
            restart_port,
        );

        assert!(
            !msg.contains(&format!("clawcrew gateway start --port {}", port + 1)),
            "{msg}"
        );
        assert!(
            msg.contains(&format!("clawcrew gateway start --port {available_port}")),
            "{msg}"
        );
    }

    #[test]
    fn gateway_addr_in_use_message_uses_configured_default_gateway_recovery() {
        let msg = gateway_addr_in_use_message("192.168.1.20", 9001, "192.168.1.20", 9001, None);

        assert!(msg.contains("open http://192.168.1.20:9001"));
        assert!(msg.contains("clawcrew gateway get-paircode\n"));
        assert!(!msg.contains("get-paircode --port 9001"));
    }

    #[test]
    fn gateway_restart_hint_uses_gateway_bind_fallback_for_hostnames() {
        let (port, mut listeners) = reserve_consecutive_local_ports(3);
        let available_port = port + 2;
        drop(listeners.pop());

        assert_eq!(
            available_gateway_restart_hint_port("localhost", port),
            Some(available_port)
        );
    }

    #[test]
    fn gateway_bind_addr_resolver_accepts_bracketed_ipv6_hosts() {
        let addr = clawcrew_infra::effective_gateway_bind_socket_addr("[::1]", 9001);

        assert_eq!(addr.port(), 9001);
        assert!(addr.is_ipv6());
    }

    #[test]
    fn gateway_addr_in_use_detector_recognizes_nested_io_error() {
        let err = std::io::Error::from(ErrorKind::AddrInUse);
        let err = anyhow::Error::new(err).context("gateway bind failed");

        assert!(is_addr_in_use_error(&err));
    }

    fn reserve_consecutive_local_ports(count: u16) -> (u16, Vec<TcpListener>) {
        for _ in 0..100 {
            let Ok(first) = TcpListener::bind(("127.0.0.1", 0)) else {
                continue;
            };
            let port = first.local_addr().expect("listener has local addr").port();
            if port > u16::MAX - count {
                continue;
            }

            let mut listeners = vec![first];
            let mut reserved_all = true;
            for offset in 1..count {
                match TcpListener::bind(("127.0.0.1", port + offset)) {
                    Ok(listener) => listeners.push(listener),
                    Err(_) => {
                        reserved_all = false;
                        break;
                    }
                }
            }

            if reserved_all {
                return (port, listeners);
            }
        }

        panic!("could not reserve {count} consecutive local ports");
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn onboard_cli_quick_and_channels_only_conflict() {
        // --quick and --channels-only should both parse at the CLI level
        // (the conflict is checked at runtime), but we verify both flags parse.
        let cli = Cli::try_parse_from(["clawcrew", "onboard", "--quick", "--channels-only"]);
        assert!(
            cli.is_ok(),
            "--quick --channels-only should parse at CLI level"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn onboard_cli_bare_parses() {
        let cli = Cli::try_parse_from(["clawcrew", "onboard"]).expect("bare onboard should parse");

        match cli.command {
            Commands::Onboard { section, .. } => assert!(section.is_none()),
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn onboard_cli_positional_sections_parse() {
        for w in clawcrew_config::sections::QUICKSTART_SECTIONS {
            let cli = Cli::try_parse_from(["clawcrew", "onboard", w.as_str()])
                .unwrap_or_else(|_| panic!("onboard {} should parse", w.as_str()));
            match cli.command {
                Commands::Onboard { section, .. } => assert_eq!(section, Some(*w)),
                other => panic!("expected onboard command, got {other:?}"),
            }
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn cli_parses_estop_default_engage() {
        let cli = Cli::try_parse_from(["clawcrew", "estop"]).expect("estop command should parse");

        match cli.command {
            Commands::Estop {
                estop_command,
                level,
                domains,
                tools,
            } => {
                assert!(estop_command.is_none());
                assert!(level.is_none());
                assert!(domains.is_empty());
                assert!(tools.is_empty());
            }
            other => panic!("expected estop command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn cli_parses_estop_resume_domain() {
        let cli = Cli::try_parse_from(["clawcrew", "estop", "resume", "--domain", "*.chase.com"])
            .expect("estop resume command should parse");

        match cli.command {
            Commands::Estop {
                estop_command: Some(EstopSubcommands::Resume { domains, .. }),
                ..
            } => assert_eq!(domains, vec!["*.chase.com".to_string()]),
            other => panic!("expected estop resume command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn agent_command_parses_with_temperature() {
        let cli = Cli::try_parse_from([
            "clawcrew",
            "agent",
            "--agent",
            "morning-shift",
            "--temperature",
            "0.5",
        ])
        .expect("agent command with temperature should parse");

        match cli.command {
            Commands::Agent { temperature, .. } => {
                assert_eq!(temperature, Some(0.5));
            }
            other => panic!("expected agent command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn agent_command_parses_without_temperature() {
        let cli = Cli::try_parse_from([
            "clawcrew",
            "agent",
            "--agent",
            "morning-shift",
            "--message",
            "hello",
        ])
        .expect("agent command without temperature should parse");

        match cli.command {
            Commands::Agent { temperature, .. } => {
                assert_eq!(temperature, None);
            }
            other => panic!("expected agent command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn agent_command_parses_session_state_file() {
        let cli = Cli::try_parse_from([
            "clawcrew",
            "agent",
            "--agent",
            "morning-shift",
            "--session-state-file",
            "session.json",
        ])
        .expect("agent command with session state file should parse");

        match cli.command {
            Commands::Agent {
                session_state_file, ..
            } => {
                assert_eq!(session_state_file, Some(PathBuf::from("session.json")));
            }
            other => panic!("expected agent command, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn agent_uses_provider_temperature_when_unset() {
        // When the user doesn't pass --temperature, the agent CLI
        // resolves from the agent's model_provider entry's temperature,
        // bottoming out at 0.7.
        let mut config = Config::default();
        config
            .providers
            .models
            .ensure("openai", "default")
            .expect("known family")
            .temperature = Some(1.5);

        let user_temperature: Option<f64> = std::hint::black_box(None);
        let final_temperature = user_temperature.unwrap_or_else(|| {
            config
                .providers
                .models
                .find("openai", "default")
                .and_then(|e| e.temperature)
                .unwrap_or(0.7)
        });

        assert!((final_temperature - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn config_set_materializes_missing_typed_provider_alias() {
        let mut config = Config::default();
        let path = "providers.models.deepseek.default.model";

        assert!(
            config
                .providers
                .models
                .find("deepseek", "default")
                .is_none(),
            "fresh config should not already contain the requested provider alias"
        );

        let created = ensure_map_key_for_prop_path(&mut config, path)
            .expect("known typed provider path should be materialized");

        assert!(created, "missing provider alias should be created");
        config
            .set_prop_persistent(path, "deepseek-chat")
            .expect("materialized path should be writable");
        assert_eq!(
            config
                .providers
                .models
                .find("deepseek", "default")
                .and_then(|provider| provider.model.as_deref()),
            Some("deepseek-chat")
        );

        let known_paths: Vec<String> = config.prop_fields().into_iter().map(|f| f.name).collect();
        let api_key_path = clawcrew_config::helpers::resolve_field_path(
            &known_paths,
            "providers.models.deepseek.default.api-key",
        );
        config
            .set_prop_persistent(&api_key_path, "sk-test-placeholder")
            .expect(
                "kebab-case secret path should resolve to the materialized typed provider field",
            );
        assert_eq!(
            config
                .providers
                .models
                .find("deepseek", "default")
                .and_then(|provider| provider.api_key.as_deref()),
            Some("sk-test-placeholder")
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn config_set_materializes_missing_tts_provider_alias() {
        let mut config = Config::default();
        let path = "providers.tts.openai.alloy.voice";

        assert!(
            config
                .providers
                .tts
                .iter_entries()
                .all(|(family, alias, _)| !(family == "openai" && alias == "alloy")),
            "fresh config should not already contain the requested tts alias"
        );

        let created = ensure_map_key_for_prop_path(&mut config, path)
            .expect("known typed tts provider path should be materialized");

        assert!(created, "missing tts alias should be created");
        config
            .set_prop_persistent(path, "alloy")
            .expect("materialized tts path should be writable");
        assert!(
            config
                .providers
                .tts
                .iter_entries()
                .any(|(family, alias, _)| family == "openai" && alias == "alloy"),
            "tts alias should resolve after materialization"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn config_set_materializes_first_dynamic_secret_map_entry() {
        let mut config = Config::default();
        let path = "providers.models.openai.fresh.extra_headers.X-Foo";

        let created = ensure_map_key_for_prop_path(&mut config, path)
            .expect("known dynamic secret-map path should materialize");

        assert!(created, "missing provider alias should be created");
        config
            .set_prop_persistent(path, "bar")
            .expect("first dynamic secret-map entry should be writable");
        assert_eq!(
            config
                .providers
                .models
                .openai
                .get("fresh")
                .and_then(|provider| provider.base.extra_headers.get("X-Foo"))
                .map(String::as_str),
            Some("bar")
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn config_set_materializes_missing_transcription_provider_alias() {
        let mut config = Config::default();
        let raw = "providers.transcription.groq.fast.model";

        assert!(
            config
                .providers
                .transcription
                .iter_aliases()
                .all(|(family, alias)| !(family == "groq" && alias == "fast")),
            "fresh config should not already contain the requested transcription alias"
        );

        // Mirror the CLI `config set` path exactly: resolve, materialize the
        // map key, then re-resolve so the now-present alias field is found.
        let known: Vec<String> = config.prop_fields().into_iter().map(|f| f.name).collect();
        let mut path = clawcrew_config::helpers::resolve_field_path(&known, raw);
        let created = ensure_map_key_for_prop_path(&mut config, &path)
            .expect("known typed transcription provider path should be materialized");
        assert!(created, "missing transcription alias should be created");
        let known: Vec<String> = config.prop_fields().into_iter().map(|f| f.name).collect();
        path = clawcrew_config::helpers::resolve_field_path(&known, &path);

        config
            .set_prop_persistent(&path, "whisper-large-v3")
            .expect("materialized transcription path should be writable");
        assert!(
            config
                .providers
                .transcription
                .iter_aliases()
                .any(|(family, alias)| family == "groq" && alias == "fast"),
            "transcription alias should resolve after materialization"
        );
    }

    #[test]
    fn config_set_does_not_materialize_non_provider_map_keys() {
        let mut config = Config::default();
        let created = ensure_map_key_for_prop_path(
            &mut config,
            "cost.rates.providers.models.openai.gpt-4.1.input_per_mtok",
        )
        .expect("resource-key map paths should be ignored, not rejected");

        assert!(
            !created,
            "auto-materialization must stay scoped to alias-keyed sections, excluding #[resource_key] sections"
        );
        assert!(
            config.cost.rates.providers.models.openai.is_empty(),
            "no bogus model-id key should have been materialized under cost.rates",
        );
    }

    #[test]
    fn ensure_map_key_materializes_non_provider_alias_sections() {
        for (path, value) in [
            ("risk_profiles.newprofile.level", "supervised"),
            ("channels.telegram.main.enabled", "true"),
            ("channels.telegram.main.bot_token", "tok"),
            ("peer_groups.pi400_owner.channel", "telegram.main"),
        ] {
            let mut config = Config::default();
            assert!(
                config.set_prop(path, value).is_err(),
                "precondition: {path} should be unknown on a fresh config"
            );
            let created = ensure_map_key_for_prop_path(&mut config, path)
                .expect("newly-widened alias sections should materialize");
            assert!(created, "{path}'s alias should be created");
            assert!(
                config.set_prop(path, value).is_ok(),
                "{path} must be settable after map-key materialization"
            );
        }
    }

    #[test]
    fn ensure_map_key_for_prop_path_refuses_reserved_default_agent() {
        let mut config = Config::default();

        let created = ensure_map_key_for_prop_path(&mut config, "agents.default.enabled")
            .expect("agents is a known map-keyed section; refusal is not an error");
        assert!(
            !created,
            "must refuse to auto-create the reserved `default` agent alias"
        );
        assert!(
            config.agents.is_empty(),
            "no `agents.default` entry should have been left behind by the refused create"
        );

        let created = ensure_map_key_for_prop_path(&mut config, "agents.researcher.enabled")
            .expect("non-reserved agent aliases should still materialize");
        assert!(
            created,
            "agents.<non-default> must still auto-materialize like every other widened section"
        );
        assert!(
            config.agents.contains_key("researcher"),
            "researcher alias should have been created"
        );
    }

    #[test]
    fn config_set_materializes_agent_workspace_path() {
        let mut config = Config::default();
        let raw = "agents.assistant.workspace.path";

        let known: Vec<String> = config.prop_fields().into_iter().map(|f| f.name).collect();
        let mut path = clawcrew_config::helpers::resolve_field_path(&known, raw);
        let created = ensure_map_key_for_prop_path(&mut config, &path)
            .expect("agent alias and workspace path should materialize");
        assert!(created, "missing agent alias should be created");

        let known: Vec<String> = config.prop_fields().into_iter().map(|f| f.name).collect();
        path = clawcrew_config::helpers::resolve_field_path(&known, &path);
        config
            .set_prop_persistent(&path, "/srv/clawcrew/assistant")
            .expect("agent workspace path should be writable");

        assert_eq!(path, raw);
        assert_eq!(
            config
                .agents
                .get("assistant")
                .and_then(|agent| agent.workspace.path.as_deref()),
            Some(std::path::Path::new("/srv/clawcrew/assistant"))
        );
    }

    #[test]
    fn ensure_map_key_rolls_back_alias_on_unknown_tail_field() {
        let mut config = Config::default();
        let path = "risk_profiles.newprofile.not_a_real_field";

        let created = ensure_map_key_for_prop_path(&mut config, path)
            .expect("section resolves; only the tail field is bogus");
        assert!(
            !created,
            "must not report success when the tail field doesn't resolve"
        );
        assert!(
            config
                .get_map_keys("risk_profiles")
                .unwrap_or_default()
                .is_empty(),
            "the tentatively-created alias must be rolled back, not left dangling",
        );
    }

    #[test]
    fn ensure_map_key_for_prop_path_leaves_existing_hyphenated_alias_alone() {
        let mut config = Config::default();
        config.cron.insert(
            "morning-brief".to_string(),
            clawcrew_config::schema::CronJobDecl::default(),
        );

        let created = ensure_map_key_for_prop_path(&mut config, "cron.morning-brief.name")
            .expect("an existing loaded alias must never be rejected by the create grammar");
        assert!(
            !created,
            "the existing `morning-brief` alias must not be reported as newly created"
        );
        assert!(
            config
                .set_prop("cron.morning-brief.name", "Morning brief")
                .is_ok(),
            "setting a field on an existing hyphenated cron alias must succeed"
        );
        assert_eq!(
            config.get_prop("cron.morning-brief.name").ok(),
            Some("Morning brief".to_string())
        );

        let err = ensure_map_key_for_prop_path(&mut config, "cron.bad-alias.name")
            .expect_err("creating a NEW hyphenated alias must still be rejected");
        assert!(
            err.to_string().contains("invalid character"),
            "new-alias grammar must be preserved: {err}"
        );
    }

    // `config init` alias tests. Every test in this module builds a bare
    // `Config::default()`, whose `config_path` points at the developer's real
    // `~/.clawcrew/config.toml`, and no gate catches a write from `src/`. These
    // stay safe only by calling `init_map_alias` and in-memory readers such as
    // `get_map_keys` — never `save()`, `save_dirty()`, a persisting `set_prop`,
    // `ensure_disk_at_current_version`, or the real `ConfigCommands::Init` arm.
    // End-to-end coverage of the handler lives in `tests/component/`.

    #[test]
    fn config_init_materializes_new_map_alias() {
        for (arg, section) in [
            ("risk_profiles.strict", "risk_profiles"),
            ("peer_groups.pi400_owner", "peer_groups"),
        ] {
            let mut config = Config::default();
            let created = init_map_alias(&mut config, arg)
                .expect("alias-shaped section arguments should materialize");
            assert_eq!(created.as_deref(), Some(arg));
            let alias = arg.rsplit('.').next().expect("alias segment");
            assert!(
                config
                    .get_map_keys(section)
                    .unwrap_or_default()
                    .iter()
                    .any(|k| k == alias),
                "{arg} should be present under {section}"
            );
        }
    }

    #[test]
    fn config_init_alias_is_idempotent() {
        let mut config = Config::default();
        init_map_alias(&mut config, "risk_profiles.strict").expect("first create");
        let again = init_map_alias(&mut config, "risk_profiles.strict").expect("second create");
        assert!(again.is_none(), "an existing alias is not re-reported");
        assert_eq!(
            config.get_map_keys("risk_profiles").unwrap_or_default(),
            vec!["strict".to_string()],
        );
    }

    #[test]
    fn config_init_ignores_plain_section_prefixes() {
        let mut config = Config::default();
        for arg in ["channels.telegram", "gateway"] {
            assert!(
                init_map_alias(&mut config, arg)
                    .expect("plain prefixes are not an error")
                    .is_none(),
                "{arg} has no trailing alias segment; init_defaults keeps ownership"
            );
        }
    }

    #[test]
    fn config_init_ignores_resource_keyed_sections() {
        let mut config = Config::default();
        assert!(
            init_map_alias(&mut config, "cost.rates.providers.models.openai.gpt-5")
                .expect("resource-keyed sections are ignored, not rejected")
                .is_none()
        );
        assert!(config.cost.rates.providers.models.openai.is_empty());
    }

    #[test]
    fn config_init_refuses_reserved_default_agent() {
        let mut config = Config::default();
        let err = init_map_alias(&mut config, "agents.default")
            .expect_err("the reserved agent guard must surface, not exit 0");
        assert!(
            err.to_string().contains("reserved"),
            "message should name the reserved alias: {err}"
        );
        assert!(config.agents.is_empty());

        assert_eq!(
            init_map_alias(&mut config, "agents.researcher")
                .expect("non-reserved agent aliases still materialize")
                .as_deref(),
            Some("agents.researcher"),
        );
    }

    #[test]
    fn config_init_rejects_invalid_alias_key() {
        let mut config = Config::default();
        assert!(
            init_map_alias(&mut config, "risk_profiles.Bad-Name").is_err(),
            "validate_alias_key's refusal must propagate"
        );
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn config_set_materializes_missing_channel_alias() {
        let mut config = Config::default();
        let path = "channels.telegram.default.bot_token";

        assert!(
            !config.channels.telegram.contains_key("default"),
            "fresh config should not already contain the default channel alias"
        );

        let created = ensure_map_key_for_prop_path(&mut config, path)
            .expect("known channel path should be materialized");

        assert!(created, "missing channel alias should be created");
        config
            .set_prop_persistent(path, "test-token")
            .expect("materialized channel path should be writable");
        assert_eq!(
            config
                .channels
                .telegram
                .get("default")
                .unwrap()
                .bot_token
                .as_str(),
            "test-token"
        );
    }

    #[tokio::test]
    #[cfg(feature = "agent-runtime")]
    async fn sop_maintenance_tick_dispatches_cached_cron_triggers() {
        use std::sync::{Arc, Mutex};
        use clawcrew_config::schema::{MemoryConfig, SopConfig};
        use clawcrew_memory::traits::Memory;
        use clawcrew_runtime::sop::{
            Sop, SopEngine, SopExecutionMode, SopPriority, SopStep, SopStepKind, SopTrigger,
        };

        let mut engine = SopEngine::new(SopConfig::default());
        engine.set_sops_for_test(vec![Sop {
            name: "cron-sop".into(),
            description: "cron regression".into(),
            version: "0.1.0".into(),
            execution_mode: SopExecutionMode::Supervised,
            priority: SopPriority::Normal,
            triggers: vec![SopTrigger::Cron {
                expression: "* * * * *".into(),
            }],
            steps: vec![SopStep {
                number: 1,
                title: "Step one".into(),
                body: "Do step one".into(),
                suggested_tools: vec![],
                requires_confirmation: false,
                kind: SopStepKind::default(),
                schema: None,
                ..SopStep::default()
            }],
            cooldown_secs: 0,
            max_concurrent: 2,
            location: None,
            deterministic: false,
            admission_policy: clawcrew_runtime::sop::types::SopAdmissionPolicy::Parallel,
            max_pending_approvals: 0,
            agent: None,
        }]);
        let engine = Arc::new(Mutex::new(engine));

        let tmp = tempfile::tempdir().expect("temp dir");
        let mem_cfg = MemoryConfig {
            backend: "sqlite".into(),
            ..MemoryConfig::default()
        };
        let memory: Arc<dyn Memory> =
            Arc::from(clawcrew_memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());
        let audit = Arc::new(clawcrew_runtime::sop::SopAuditLogger::new(memory));
        let cache = clawcrew_runtime::sop::dispatch::SopCronCache::from_engine(&engine);

        let mut last_cron_check = chrono::Utc::now() - chrono::Duration::minutes(2);
        let report =
            run_sop_maintenance_tick(&engine, Some(&audit), Some(&cache), &mut last_cron_check)
                .await
                .expect("maintenance tick should complete");

        assert_eq!(report.cron_started, 1);
        assert_eq!(engine.lock().unwrap().active_runs().len(), 1);
    }

    #[test]
    #[cfg(feature = "agent-runtime")]
    fn agent_fallback_uses_hardcoded_when_config_uses_default() {
        // Test that when config uses default value (0.7), fallback still works
        let config = Config::default();

        // Simulate None temperature (user didn't provide --temperature)
        let user_temperature: Option<f64> = std::hint::black_box(None);
        let final_temperature = user_temperature.unwrap_or_else(|| {
            config
                .providers
                .models
                .iter_entries()
                .next()
                .and_then(|(_, _, e)| e.temperature)
                .unwrap_or(0.7)
        });

        assert!((final_temperature - 0.7).abs() < f64::EPSILON);
    }

    #[tokio::test]
    #[cfg(feature = "agent-runtime")]
    async fn gate_security_posture_fails_closed_unless_allowed() {
        use crate::config::schema::Config;

        // Clean posture: no gate, no nag.
        let clean = Config::default();
        assert!(clean.degraded_security.is_empty());
        let handle = gate_security_posture(&clean, false).expect("clean posture must pass");
        assert!(handle.is_none(), "clean posture must not spawn a nag");

        // Degraded posture, not allowed: must refuse to serve.
        let mut degraded = Config::default();
        degraded.degraded_security = vec!["security".to_string()];
        assert!(
            gate_security_posture(&degraded, false).is_err(),
            "degraded posture must fail closed when not explicitly allowed"
        );

        // Degraded posture, explicitly allowed: boots and returns a nag handle.
        let nag = gate_security_posture(&degraded, true)
            .expect("degraded posture must boot when allowed")
            .expect("allowed degraded posture must spawn a nag task");
        nag.abort();

        // Whole-config loss (sentinel marker) is degraded too: same fail-closed
        // behavior so a defaulted security posture cannot serve silently.
        let mut whole = Config::default();
        whole.degraded_security = vec![crate::config::migration::WHOLE_CONFIG_SENTINEL.to_string()];
        assert!(
            gate_security_posture(&whole, false).is_err(),
            "whole-config loss must fail closed when not explicitly allowed"
        );
    }

    #[tokio::test]
    #[cfg(feature = "agent-runtime")]
    async fn models_set_persists_model_and_preserves_slash_bearing_ids() {
        use crate::config::schema::{AnthropicModelProviderConfig, Config, ModelProviderConfig};

        let tmp = tempfile::TempDir::new().expect("temp dir");
        let config_path = tmp.path().join("config.toml");

        std::fs::write(
            &config_path,
            format!(
                "schema_version = {}\n\n[providers.models.anthropic.default]\nmodel = \"claude-opus-4-7\"\n",
                crate::config::migration::CURRENT_SCHEMA_VERSION,
            ),
        )
        .unwrap();

        let mut config = Config {
            config_path: config_path.clone(),
            data_dir: tmp.path().join("workspace"),
            schema_version: crate::config::migration::CURRENT_SCHEMA_VERSION,
            ..Config::default()
        };
        config.providers.models.anthropic.insert(
            "default".to_string(),
            AnthropicModelProviderConfig {
                base: ModelProviderConfig {
                    model: Some("claude-opus-4-7".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );

        // ── Test 1: Normal model ID persists via the dispatch boundary ──
        dispatch_models_command(
            ModelCommands::Set {
                model: "claude-sonnet-4-6".to_string(),
            },
            &mut config,
        )
        .await
        .expect("normal model ID must persist");

        let contents = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            contents.contains("claude-sonnet-4-6"),
            "normal model ID must be persisted to config.toml; got:\n{contents}"
        );

        // ── Test 2: Slash-bearing model ID preserved as-is ──
        dispatch_models_command(
            ModelCommands::Set {
                model: "anthropic/claude-sonnet-4-20250514".to_string(),
            },
            &mut config,
        )
        .await
        .expect("slash-bearing model ID must persist");

        let contents = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            contents.contains("anthropic/claude-sonnet-4-20250514"),
            "slash-bearing model ID must be stored as-is; got:\n{contents}"
        );

        // ── Test 3: No configured provider → error surfaced by dispatch ──
        let mut empty_config = Config {
            config_path: tmp.path().join("empty.toml"),
            data_dir: tmp.path().join("empty_workspace"),
            schema_version: crate::config::migration::CURRENT_SCHEMA_VERSION,
            ..Config::default()
        };
        let err = dispatch_models_command(
            ModelCommands::Set {
                model: "any-model".to_string(),
            },
            &mut empty_config,
        )
        .await
        .expect_err("empty config must fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("No model provider configured"),
            "error must mention missing provider; got: {msg}"
        );
    }

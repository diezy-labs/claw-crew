#![recursion_limit = "256"]
#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::assigning_clones,
    clippy::bool_to_int_with_if,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::cast_possible_wrap,
    clippy::doc_markdown,
    clippy::field_reassign_with_default,
    clippy::float_cmp,
    clippy::implicit_clone,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::manual_let_else,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::needless_pass_by_value,
    clippy::needless_raw_string_hashes,
    clippy::redundant_closure_for_method_calls,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unused_self,
    clippy::cast_precision_loss,
    clippy::unnecessary_cast,
    clippy::unnecessary_lazy_evaluations,
    clippy::unnecessary_literal_bound,
    clippy::unnecessary_map_or,
    clippy::unnecessary_wraps,
    unused_variables,
    unused_imports
)]

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use dialoguer::Select;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::io::{BufRead, ErrorKind, Read, Write};

#[cfg(feature = "agent-runtime")]
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

#[cfg(any(not(feature = "agent-runtime"), windows))]
const STDIN_LINE_CAP: usize = 1024 * 1024;

/// Result of [`read_capped_line`].
#[cfg(not(feature = "agent-runtime"))]
#[derive(Debug)]
enum CappedLine {
    /// A full line under the cap, with the trailing `\n` stripped.
    Line(String),
    /// The physical line exceeded `cap`. The remainder has been drained
    /// and must not be used as a prompt.
    Truncated,
    /// EOF with no bytes read.
    Eof,
}

#[cfg(not(feature = "agent-runtime"))]
fn read_capped_line<R: std::io::BufRead>(reader: R, cap: usize) -> std::io::Result<CappedLine> {
    let mut raw = Vec::new();
    let mut limited = reader.take((cap + 1) as u64);
    std::io::BufRead::read_until(&mut limited, b'\n', &mut raw)?;
    let truncated = raw.len() > cap;
    if truncated {
        let mut inner = limited.into_inner();
        discard_until_newline(&mut inner)?;
        return Ok(CappedLine::Truncated);
    } else if raw.last() == Some(&b'\n') {
        raw.pop();
    }
    if raw.is_empty() {
        return Ok(CappedLine::Eof);
    }
    Ok(CappedLine::Line(String::from_utf8_lossy(&raw).into_owned()))
}

/// Truncate `line` in place to at most `cap` bytes, rounding the cut down to a
/// UTF-8 char boundary. `String::truncate` panics when the byte index lands
/// inside a multi-byte character, so a raw `line.truncate(cap)` on piped input
/// is a latent panic. No-op when the string already fits.
#[cfg(any(windows, test))]
fn cap_line_utf8_safe(line: &mut String, cap: usize) {
    if line.len() > cap {
        line.truncate(line.floor_char_boundary(cap));
    }
}

/// Discard bytes from `reader` until the next `\n` or EOF, using only
/// `BufRead::fill_buf` / `consume`. This avoids the unbounded allocation
/// that `read_until(..., &mut Vec::new())` would incur on an oversized
/// physical line, and it stops exactly at the newline so the next line
/// is not consumed.
#[cfg(not(feature = "agent-runtime"))]
fn discard_until_newline<R: std::io::BufRead>(reader: &mut R) -> std::io::Result<()> {
    loop {
        let buf = reader.fill_buf()?;
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            reader.consume(pos + 1);
            return Ok(());
        }
        let len = buf.len();
        if len == 0 {
            return Ok(());
        }
        reader.consume(len);
    }
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(feature = "agent-runtime")]
use clawcrew_config::api_error::{ConfigApiCode, ConfigApiError};

/// Resolve a `cli-*` Fluent key for CLI output. Routes through the runtime
/// i18n catalogue under `agent-runtime` (default + CI/release); without that
/// feature the runtime crate is absent, so the English `fallback` is used.
#[allow(unused_variables)]
fn t(key: &str, fallback: &str) -> String {
    #[cfg(feature = "agent-runtime")]
    {
        clawcrew_runtime::i18n::get_required_cli_string(key)
    }
    #[cfg(not(feature = "agent-runtime"))]
    {
        fallback.to_string() // i18n-exempt: English fallback when Fluent (agent-runtime) is disabled
    }
}

/// `t` with `{$name}` arguments.
#[allow(unused_variables)]
fn ta(key: &str, args: &[(&str, &str)], fallback: impl Into<String>) -> String {
    #[cfg(feature = "agent-runtime")]
    {
        clawcrew_runtime::i18n::get_required_cli_string_with_args(key, args)
    }
    #[cfg(not(feature = "agent-runtime"))]
    {
        fallback.into() // i18n-exempt: English fallback when Fluent (agent-runtime) is disabled
    }
}

/// Interactive secret prompt with pre-submit feedback.
///
/// The value stays hidden, but the prompt shows a bounded mask once the input
/// buffer becomes non-empty.
#[cfg(feature = "agent-runtime")]
fn secret_prompt(prompt_text: &str, allow_empty: bool) -> Result<String> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        bail!(ta(
            "cli-secret-needs-tty",
            &[],
            "Secret input requires a terminal on stdin and stderr."
        ));
    }

    let value = cli_input::SecretInput::new()
        .with_prompt(prompt_text)
        .interact()?;
    if allow_empty || !value.trim().is_empty() {
        Ok(value)
    } else {
        bail!(ta("cli-secret-empty", &[], "Value cannot be empty."))
    }
}

#[cfg(feature = "agent-runtime")]
fn qta(key: &str, args: &[(&str, &str)]) -> String {
    clawcrew_runtime::i18n::get_required_cli_string_with_args(key, args)
}

#[cfg(feature = "agent-runtime")]
fn quickstart_row(key: &str, glyph: &str, summary: &str) -> String {
    qta(key, &[("glyph", glyph), ("summary", summary)])
}

#[cfg(feature = "agent-runtime")]
const QUICKSTART_SELECTOR_MIN_WIDTH: usize = 20;

#[cfg(feature = "agent-runtime")]
const QUICKSTART_SELECTOR_ROW_OVERHEAD: usize = 3;

#[cfg(feature = "agent-runtime")]
const QUICKSTART_SELECTOR_VERTICAL_OVERHEAD: usize = 2;

#[cfg(feature = "agent-runtime")]
fn quickstart_selector_row_budget(terminal_width: usize) -> Option<usize> {
    if terminal_width < QUICKSTART_SELECTOR_MIN_WIDTH {
        return None;
    }
    terminal_width.checked_sub(QUICKSTART_SELECTOR_ROW_OVERHEAD)
}

/// Resolve the terminal dimensions the Quickstart checklist will be fitted to.
///
/// A narrow terminal whose size is unavailable must not get rows fitted against
/// a guessed geometry — that would reintroduce the exact overflow class this
/// change exists to prevent. Unknown dimensions therefore take the same
/// fail-closed path as a too-narrow terminal.
#[cfg(feature = "agent-runtime")]
fn quickstart_selector_terminal_size<T: QuickstartSelectorTerminal>(
    term: &mut T,
) -> Option<(u16, u16)> {
    term.size_checked()
}

/// Whether a sampled terminal size is usable for fitting the checklist.
#[cfg(all(feature = "agent-runtime", test))]
fn quickstart_selector_size_is_usable(size: Option<(u16, u16)>) -> bool {
    size.is_some()
}

#[cfg(feature = "agent-runtime")]
fn quickstart_selector_min_height(item_count: usize) -> usize {
    item_count.saturating_add(QUICKSTART_SELECTOR_VERTICAL_OVERHEAD)
}

#[cfg(feature = "agent-runtime")]
fn quickstart_selector_fits_height(terminal_height: usize, item_count: usize) -> bool {
    terminal_height >= quickstart_selector_min_height(item_count)
}

#[cfg(feature = "agent-runtime")]
fn fit_quickstart_selector_row(row: &str, budget: usize) -> String {
    let normalized: String = row
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if normalized.len() <= budget && console::measure_text_width(&normalized) <= budget {
        return normalized;
    }
    if budget == 0 {
        return String::new();
    }

    let marker = if budget >= "…".len() { "…" } else { "." };
    let byte_budget = budget - marker.len();
    let width_budget = budget - console::measure_text_width(marker);
    let mut fitted = String::with_capacity(budget);
    for ch in normalized.chars() {
        fitted.push(ch);
        if fitted.len() > byte_budget || console::measure_text_width(&fitted) > width_budget {
            fitted.pop();
            break;
        }
    }
    fitted.push_str(marker);
    fitted
}

#[cfg(feature = "agent-runtime")]
fn quickstart_selector_resize_error(
    initial_size: (u16, u16),
    current_size: (u16, u16),
) -> anyhow::Error {
    let (initial_height, initial_width) = initial_size;
    let (current_height, current_width) = current_size;
    anyhow::Error::msg(qta(
        "cli-quickstart-terminal-resized",
        &[
            ("initial_width", &initial_width.to_string()),
            ("initial_height", &initial_height.to_string()),
            ("current_width", &current_width.to_string()),
            ("current_height", &current_height.to_string()),
        ],
    ))
}

/// Decide whether an interaction may continue at the size sampled now.
///
/// Returns `Err` both when the terminal changed size and when its size became
/// unavailable: an unknown size is not evidence that the geometry still
/// matches, and `Term::size()`'s fabricated `(24, 80)` fallback could even
/// compare *equal* to the initial sample on an 80x24 terminal that has since
/// lost its size query. Unknown therefore fails closed, like a resize.
#[cfg(feature = "agent-runtime")]
fn quickstart_selector_recheck_size(
    initial_size: (u16, u16),
    current_size: Option<(u16, u16)>,
) -> Result<()> {
    match current_size {
        Some(current) if current == initial_size => Ok(()),
        Some(current) => Err(quickstart_selector_resize_error(initial_size, current)),
        None => Err(anyhow::Error::msg(qta(
            "cli-quickstart-terminal-size-unknown",
            &[],
        ))),
    }
}

#[cfg(feature = "agent-runtime")]
fn quickstart_selector_frame_lines(
    labels: &[String],
    prompt: &str,
    selected: usize,
) -> Vec<String> {
    std::iter::once(format!("? {prompt}"))
        .chain(labels.iter().enumerate().map(|(index, label)| {
            let marker = if index == selected { ">" } else { " " };
            format!("{marker} {label}")
        }))
        .collect()
}

#[cfg(feature = "agent-runtime")]
fn render_quickstart_selector<T: QuickstartSelectorTerminal>(
    term: &mut T,
    lines: &[String],
) -> std::io::Result<()> {
    for line in lines {
        term.write_line(line)?;
    }
    term.flush()
}

#[cfg(feature = "agent-runtime")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickstartSelectorKey {
    Down,
    Up,
    Select,
    Cancel,
    Interrupt,
    Other,
}

#[cfg(feature = "agent-runtime")]
trait QuickstartSelectorTerminal {
    /// Geometry of the terminal that receives `write_line` output, as
    /// `(rows, columns)`, or `None` when it cannot be determined.
    fn size_checked(&mut self) -> Option<(u16, u16)>;
    fn enter_alternate_screen(&mut self) -> std::io::Result<()>;
    fn clear_screen(&mut self) -> std::io::Result<()>;
    fn move_cursor_to_origin(&mut self) -> std::io::Result<()>;
    fn hide_cursor(&mut self) -> std::io::Result<()>;
    fn show_cursor(&mut self) -> std::io::Result<()>;
    fn leave_alternate_screen(&mut self) -> std::io::Result<()>;
    fn write_line(&mut self, line: &str) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
    fn read_key(&mut self) -> std::io::Result<QuickstartSelectorKey>;
}

/// The input half of the Crossterm selector: raw-mode ownership plus key
/// decoding. It is separate from the output half so a regression can drive the
/// production output adapter with injected keys.
#[cfg(feature = "agent-runtime")]
trait QuickstartSelectorInput {
    fn read_key(&mut self) -> std::io::Result<QuickstartSelectorKey>;
}

#[cfg(feature = "agent-runtime")]
struct CrosstermQuickstartInput {
    restore_cooked_mode: bool,
}

#[cfg(feature = "agent-runtime")]
impl CrosstermQuickstartInput {
    fn new() -> std::io::Result<Self> {
        let raw_mode_was_enabled = terminal::is_raw_mode_enabled()?;
        if !raw_mode_was_enabled {
            terminal::enable_raw_mode()?;
        }
        Ok(Self {
            restore_cooked_mode: !raw_mode_was_enabled,
        })
    }
}

#[cfg(feature = "agent-runtime")]
impl Drop for CrosstermQuickstartInput {
    fn drop(&mut self) {
        if self.restore_cooked_mode {
            let _ = terminal::disable_raw_mode();
        }
    }
}

#[cfg(feature = "agent-runtime")]
impl QuickstartSelectorInput for CrosstermQuickstartInput {
    fn read_key(&mut self) -> std::io::Result<QuickstartSelectorKey> {
        loop {
            match event::read()? {
                Event::Key(key)
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                {
                    let control = key.modifiers.contains(KeyModifiers::CONTROL);
                    let modified = key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::META);
                    return Ok(match key.code {
                        KeyCode::Char('c') if control => QuickstartSelectorKey::Interrupt,
                        KeyCode::Down | KeyCode::Tab => QuickstartSelectorKey::Down,
                        KeyCode::Char('j') if !modified => QuickstartSelectorKey::Down,
                        KeyCode::Up | KeyCode::BackTab => QuickstartSelectorKey::Up,
                        KeyCode::Char('k') if !modified => QuickstartSelectorKey::Up,
                        KeyCode::Enter => QuickstartSelectorKey::Select,
                        KeyCode::Char(' ') if !modified => QuickstartSelectorKey::Select,
                        KeyCode::Esc => QuickstartSelectorKey::Cancel,
                        KeyCode::Char('q') if !modified => QuickstartSelectorKey::Cancel,
                        _ => QuickstartSelectorKey::Other,
                    });
                }
                // A resize is returned to the loop so the checked geometry is
                // sampled immediately rather than waiting for another key.
                Event::Resize(_, _) => return Ok(QuickstartSelectorKey::Other),
                _ => {}
            }
        }
    }
}

/// A frame destination whose own terminal geometry can be measured.
///
/// Quickstart requires stdin and stderr to be terminals, not the same
/// terminal. The frame is therefore fitted to the descriptor it is written to
/// rather than to whichever terminal a process-global query describes.
#[cfg(all(feature = "agent-runtime", unix))]
trait QuickstartSelectorOutput: Write + std::os::fd::AsFd {}

#[cfg(all(feature = "agent-runtime", unix))]
impl<W: Write + std::os::fd::AsFd> QuickstartSelectorOutput for W {}

#[cfg(all(feature = "agent-runtime", not(unix)))]
trait QuickstartSelectorOutput: Write {}

#[cfg(all(feature = "agent-runtime", not(unix)))]
impl<W: Write> QuickstartSelectorOutput for W {}

/// Measure the terminal behind `output` as `(rows, columns)`.
///
/// A zero dimension means the driver holds no geometry for that terminal. It
/// is reported as unknown so the caller fails closed instead of fitting rows
/// to a zero-width frame.
#[cfg(all(feature = "agent-runtime", unix))]
fn quickstart_output_terminal_size<W: QuickstartSelectorOutput>(output: &W) -> Option<(u16, u16)> {
    use std::os::fd::AsRawFd;

    let mut size = std::mem::MaybeUninit::<libc::winsize>::uninit();
    // SAFETY: `size` points to writable `winsize` storage and the borrowed
    // descriptor stays open for the duration of the call.
    let result = unsafe {
        libc::ioctl(
            output.as_fd().as_raw_fd(),
            libc::TIOCGWINSZ,
            size.as_mut_ptr(),
        )
    };
    if result != 0 {
        return None;
    }
    // SAFETY: a successful `TIOCGWINSZ` initialized `size`.
    let size = unsafe { size.assume_init() };
    (size.ws_row > 0 && size.ws_col > 0).then_some((size.ws_row, size.ws_col))
}

/// Measure the active console screen buffer as `(rows, columns)`.
///
/// Crossterm offers no per-handle geometry query here. A native console
/// shares one screen buffer between stdout and stderr, so the measured
/// surface is the one that receives the frame. Native-console rendering is
/// not exercised by hosted checks and remains a documented verification gap.
#[cfg(all(feature = "agent-runtime", not(unix)))]
fn quickstart_output_terminal_size<W: QuickstartSelectorOutput>(_output: &W) -> Option<(u16, u16)> {
    terminal::size().ok().map(|(columns, rows)| (rows, columns))
}

/// Crossterm-backed selector terminal: frames go to `output`, keys come from
/// `input`, and geometry is always read from `output`.
#[cfg(feature = "agent-runtime")]
struct CrosstermQuickstartTerminal<W: QuickstartSelectorOutput, K: QuickstartSelectorInput> {
    output: W,
    input: K,
}

#[cfg(feature = "agent-runtime")]
impl CrosstermQuickstartTerminal<std::io::Stderr, CrosstermQuickstartInput> {
    fn stderr() -> std::io::Result<Self> {
        Ok(Self {
            output: std::io::stderr(),
            input: CrosstermQuickstartInput::new()?,
        })
    }
}

#[cfg(feature = "agent-runtime")]
impl<W: QuickstartSelectorOutput, K: QuickstartSelectorInput> QuickstartSelectorTerminal
    for CrosstermQuickstartTerminal<W, K>
{
    fn size_checked(&mut self) -> Option<(u16, u16)> {
        quickstart_output_terminal_size(&self.output)
    }

    fn enter_alternate_screen(&mut self) -> std::io::Result<()> {
        execute!(self.output, EnterAlternateScreen)
    }

    fn clear_screen(&mut self) -> std::io::Result<()> {
        execute!(self.output, Clear(ClearType::All))
    }

    fn move_cursor_to_origin(&mut self) -> std::io::Result<()> {
        execute!(self.output, MoveTo(0, 0))
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        execute!(self.output, Hide)
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        execute!(self.output, Show)
    }

    fn leave_alternate_screen(&mut self) -> std::io::Result<()> {
        // Crossterm uses the native screen-buffer API on legacy Windows
        // consoles and the ANSI sequence on terminals that support it.
        execute!(self.output, LeaveAlternateScreen)
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        // Raw mode disables the Unix terminal driver's LF-to-CRLF mapping.
        // Emit both controls explicitly so every row begins in column zero.
        write!(self.output, "{line}\r\n")
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }

    fn read_key(&mut self) -> std::io::Result<QuickstartSelectorKey> {
        self.input.read_key()
    }
}

/// Own the alternate screen from before its first fallible operation.
///
/// Claiming ownership before `enter_alternate_screen` means a partial write or
/// flush failure still triggers a best-effort restore. Cleanup attempts are
/// independent: a cursor error must never strand the alternate screen.
#[cfg(feature = "agent-runtime")]
struct QuickstartSelectorScreen<'a, T: QuickstartSelectorTerminal> {
    term: &'a mut T,
    restore_needed: bool,
}

#[cfg(feature = "agent-runtime")]
impl<'a, T: QuickstartSelectorTerminal> QuickstartSelectorScreen<'a, T> {
    fn enter(term: &'a mut T) -> std::io::Result<Self> {
        let screen = Self {
            term,
            restore_needed: true,
        };
        screen.term.enter_alternate_screen()?;
        screen.term.clear_screen()?;
        screen.term.move_cursor_to_origin()?;
        screen.term.hide_cursor()?;
        screen.term.flush()?;
        Ok(screen)
    }

    fn restore(&mut self) -> std::io::Result<()> {
        if !self.restore_needed {
            return Ok(());
        }
        self.restore_needed = false;

        let mut first_error = None;
        for result in [
            self.term.show_cursor(),
            self.term.leave_alternate_screen(),
            self.term.flush(),
        ] {
            if first_error.is_none() {
                first_error = result.err();
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(feature = "agent-runtime")]
impl<T: QuickstartSelectorTerminal> Drop for QuickstartSelectorScreen<'_, T> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(feature = "agent-runtime")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickstartSelectorOutcome {
    Pick(Option<usize>),
    Interrupt,
}

/// Render the fixed-size Quickstart checklist without dialoguer paging.
///
/// The terminal dimensions sampled for fitting are part of this interaction's
/// contract. They describe the terminal that receives the frame, and every
/// input event rechecks them before navigation or selection; a resize exits
/// the selector-owned alternate screen instead of trying to erase a
/// main-screen frame whose physical rows the terminal may have reflowed. A
/// resize of the output terminal alone raises no input event, so it is caught
/// at the next key. Leaving the alternate screen atomically restores
/// unrelated output.
#[cfg(feature = "agent-runtime")]
fn interact_quickstart_selector<T: QuickstartSelectorTerminal>(
    term: &mut T,
    labels: &[String],
    prompt: &str,
    initial_size: (u16, u16),
) -> Result<QuickstartSelectorOutcome> {
    if labels.is_empty() {
        bail!(qta("cli-quickstart-empty-checklist", &[]));
    }
    let current_size = quickstart_selector_terminal_size(term);
    quickstart_selector_recheck_size(initial_size, current_size)?;

    let mut screen = QuickstartSelectorScreen::enter(term)?;
    let interaction = (|| -> Result<QuickstartSelectorOutcome> {
        let mut selected = 0;
        let mut frame = quickstart_selector_frame_lines(labels, prompt, selected);
        render_quickstart_selector(screen.term, &frame)?;

        loop {
            let key = screen.term.read_key()?;
            let current_size = quickstart_selector_terminal_size(screen.term);
            quickstart_selector_recheck_size(initial_size, current_size)?;

            match key {
                QuickstartSelectorKey::Down => {
                    selected = (selected + 1) % labels.len();
                    frame = quickstart_selector_frame_lines(labels, prompt, selected);
                    screen.term.clear_screen()?;
                    screen.term.move_cursor_to_origin()?;
                    render_quickstart_selector(screen.term, &frame)?;
                }
                QuickstartSelectorKey::Up => {
                    selected = selected.checked_sub(1).unwrap_or(labels.len() - 1);
                    frame = quickstart_selector_frame_lines(labels, prompt, selected);
                    screen.term.clear_screen()?;
                    screen.term.move_cursor_to_origin()?;
                    render_quickstart_selector(screen.term, &frame)?;
                }
                QuickstartSelectorKey::Select => {
                    return Ok(QuickstartSelectorOutcome::Pick(Some(selected)));
                }
                QuickstartSelectorKey::Cancel => {
                    return Ok(QuickstartSelectorOutcome::Pick(None));
                }
                QuickstartSelectorKey::Interrupt => {
                    return Ok(QuickstartSelectorOutcome::Interrupt);
                }
                QuickstartSelectorKey::Other => {}
            }
        }
    })();
    let cleanup = screen.restore();
    match (interaction, cleanup) {
        (Ok(QuickstartSelectorOutcome::Interrupt), _) => Ok(QuickstartSelectorOutcome::Interrupt),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Ok(selection), Ok(())) => Ok(selection),
    }
}

#[cfg(feature = "agent-runtime")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickstartChecklistAction {
    Provider,
    Risk,
    Memory,
    Channels,
    PeerGroups,
    Agent,
    Create,
    Quit,
}

#[cfg(feature = "agent-runtime")]
fn quickstart_action_for_pick(
    choices: &[(QuickstartChecklistAction, String)],
    pick: Option<usize>,
) -> QuickstartChecklistAction {
    pick.and_then(|index| choices.get(index).map(|(action, _)| *action))
        .unwrap_or(QuickstartChecklistAction::Quit)
}

#[cfg(feature = "agent-runtime")]
fn quickstart_step_label(step: clawcrew_runtime::quickstart::QuickstartStep) -> String {
    t(step.label_key(), step.label())
}

/// Decorate the value at `path` in `config.toml` with a leading `# {comment}`
/// line, preserving any non-comment whitespace. Mirrors the gateway's
/// `apply_comments`. Best-effort — silently bails on parse errors so a
/// successful set isn't downgraded to a failure for a metadata problem.
#[cfg(feature = "agent-runtime")]
async fn apply_comment_inline(
    config_path: &std::path::Path,
    path: &str,
    comment: &str,
) -> Result<()> {
    clawcrew_config::comment_writer::apply_comments(
        config_path,
        &[(path.to_string(), comment.to_string())],
    )
    .await
    .context("failed to write comment annotation")
}

#[cfg(feature = "agent-runtime")]
fn config_patch_prop_kind(config: &Config, path: &str) -> Option<crate::config::PropKind> {
    config
        .prop_fields()
        .into_iter()
        .find(|f| f.name == path)
        .map(|f| f.kind)
}

#[cfg(feature = "agent-runtime")]
fn json_value_to_setprop_string(
    value: &serde_json::Value,
    config: &Config,
    path: &str,
    op_index: usize,
    json: bool,
) -> Result<String> {
    let kind = config_patch_prop_kind(config, path);
    match clawcrew_config::typed_value::coerce_for_set_prop(value, kind) {
        Ok(value_str) => Ok(value_str),
        Err(err) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"path": path, "error": err.message.clone()})),
                "config patch coercion rejected JSON value"
            );
            let err = err.with_path(path).with_op_index(op_index);
            let human = err.message.clone();
            config_patch_fail_json_or_human(json, err, human)
        }
    }
}

#[cfg(feature = "agent-runtime")]
fn config_patch_map_prop_error(err: anyhow::Error, path: &str, op_index: usize) -> ConfigApiError {
    let msg = err.to_string();
    if msg.starts_with("Unknown property") {
        ConfigApiError::path_not_found(path).with_op_index(op_index)
    } else {
        ConfigApiError::from_validation(err)
            .with_path(path)
            .with_op_index(op_index)
    }
}

#[cfg(feature = "agent-runtime")]
fn config_patch_json_error(err: &ConfigApiError) -> Result<()> {
    eprintln!("{}", serde_json::to_string_pretty(err)?);
    std::process::exit(1);
}

#[cfg(feature = "agent-runtime")]
fn config_patch_json_value_type_error(
    message: impl Into<String>,
    path: Option<String>,
    op_index: Option<usize>,
) -> ConfigApiError {
    let mut err = ConfigApiError::new(ConfigApiCode::ValueTypeMismatch, message.into());
    if let Some(path) = path {
        err = err.with_path(path);
    }
    if let Some(op_index) = op_index {
        err = err.with_op_index(op_index);
    }
    err
}

#[cfg(feature = "agent-runtime")]
fn config_patch_fail_json_or_human<T>(
    json: bool,
    err: ConfigApiError,
    human: impl Into<String>,
) -> Result<T>
where
    T: Sized,
{
    if json {
        config_patch_json_error(&err)?;
    }
    anyhow::bail!("{}", human.into())
}

fn parse_temperature(s: &str) -> std::result::Result<f64, String> {
    let t: f64 = s
        .parse()
        .map_err(|e| format!("invalid temperature '{s}': {e}"))?;
    config::schema::validate_temperature(t)
}

fn print_no_command_help(cmd: clap::Command) -> Result<()> {
    #[cfg(feature = "agent-runtime")]
    {
        println!(
            "{}",
            crate::i18n::get_cli_string("cli-no-command-provided")
                .as_deref()
                .unwrap_or("No command provided.")
        );
        println!(
            "{}",
            crate::i18n::get_cli_string("cli-try-quickstart")
                .as_deref()
                .unwrap_or("Try `clawcrew quickstart` to create your first agent.")
        );
    }
    #[cfg(not(feature = "agent-runtime"))]
    {
        println!("{}", t("cli-no-command", "No command provided."));
        println!(
            "{}",
            t(
                "cli-try-quickstart",
                "Try `clawcrew quickstart` to create your first agent."
            )
        );
    }
    println!();

    let mut cmd = cmd;
    cmd.print_help()?;
    println!();

    #[cfg(windows)]
    pause_after_no_command_help();

    Ok(())
}

#[cfg(windows)]
fn pause_after_no_command_help() {
    println!();
    print!("{}", t("cli-press-enter", "Press Enter to exit..."));
    let _ = std::io::stdout().flush();
    // Cap the read so a piped-in flood (e.g. `dir | clawcrew` with no
    // command) cannot blow up RSS in this trivial one-Enter prompt.
    // See module-level `STDIN_LINE_CAP` for rationale.
    let mut line = String::new();
    let _ = std::io::stdin()
        .lock()
        .take((STDIN_LINE_CAP + 1) as u64)
        .read_line(&mut line);
    if line.len() > STDIN_LINE_CAP {
        // Round down to a UTF-8 char boundary before truncating: a piped
        // multi-byte payload can land the byte cap inside a character, and
        // `String::truncate` panics on a non-boundary index.
        cap_line_utf8_safe(&mut line, STDIN_LINE_CAP);
    }
}

// RF-A6 pure-move: src/main.rs sibling modules (bin crate root cannot become a
// folder; follows the same #[path] pattern as the main_tests wiring below).
#[path = "main_cli_args.rs"]
mod main_cli_args;
pub(crate) use main_cli_args::*;

#[cfg(feature = "agent-runtime")]
mod agent;
#[cfg(feature = "agent-runtime")]
mod alias_cli;
#[cfg(feature = "agent-runtime")]
mod approval;
#[cfg(feature = "agent-runtime")]
mod auth;
#[cfg(feature = "agent-runtime")]
mod channels;
#[cfg(feature = "agent-runtime")]
mod cli_input;
mod commands;
#[cfg(feature = "agent-runtime")]
mod rag {
    pub use clawcrew::rag::*;
}
#[cfg(feature = "agent-runtime")]
mod browse;
mod config;
#[cfg(feature = "agent-runtime")]
mod cost;
#[cfg(feature = "agent-runtime")]
mod cron;
#[cfg(feature = "agent-runtime")]
mod daemon;
#[cfg(feature = "agent-runtime")]
mod doctor;
#[cfg(feature = "gateway")]
mod gateway;
#[cfg(feature = "agent-runtime")]
mod hardware;
#[cfg(feature = "agent-runtime")]
mod health;
#[cfg(feature = "agent-runtime")]
mod heartbeat;
#[cfg(feature = "agent-runtime")]
mod hooks;
#[cfg(feature = "agent-runtime")]
mod i18n;
#[cfg(feature = "agent-runtime")]
mod identity;
#[cfg(feature = "agent-runtime")]
mod integrations;
#[cfg(feature = "agent-runtime")]
mod memory;
#[cfg(feature = "agent-runtime")]
mod migration;
#[cfg(feature = "agent-runtime")]
mod multimodal;
#[cfg(feature = "agent-runtime")]
mod observability;
#[cfg(feature = "agent-runtime")]
mod peripherals;
#[cfg(feature = "agent-runtime")]
mod platform;
#[cfg(feature = "plugins-wasm")]
mod plugin_catalog;
#[cfg(feature = "plugins-wasm")]
mod plugin_registry;
#[cfg(feature = "plugins-wasm")]
mod plugins;
mod providers;
#[cfg(feature = "agent-runtime")]
mod security;
#[cfg(feature = "agent-runtime")]
mod security_status;
#[cfg(feature = "agent-runtime")]
mod service;
#[cfg(feature = "agent-runtime")]
mod skills;
#[cfg(feature = "agent-runtime")]
mod sop;
#[cfg(feature = "agent-runtime")]
mod tools;
#[cfg(feature = "agent-runtime")]
mod trust;
#[cfg(feature = "agent-runtime")]
mod tunnel;
#[cfg(feature = "agent-runtime")]
mod util;
#[cfg(feature = "agent-runtime")]
mod verifiable_intent;

use config::Config;

// Re-export so binary modules can use crate::<CommandEnum> while keeping a single source of truth.
pub use clawcrew::{
    AgentsCommands, ChannelCommands, ChannelsCommands, CronCommands, CronDeliveryArgs,
    GatewayCommands, HardwareCommands, IntegrationCommands, MigrateCommands, PeripheralCommands,
    ProvidersCommands, ServiceCommands, ServiceLogStream, SkillBundleCommands, SkillCommands,
    SopCommands, SopGraphFormat,
};


#[cfg(feature = "agent-runtime")]
fn quickstart_runtime_profile_for_provider(
    provider_type: &str,
    providers: &[clawcrew_runtime::quickstart::QuickstartTypeOption],
    default_runtime_profile: &str,
) -> String {
    providers
        .iter()
        .find(|provider| provider.kind == provider_type)
        .and_then(|provider| provider.default_runtime_profile.as_deref())
        .unwrap_or(default_runtime_profile)
        .to_string()
}

/// `clawcrew quickstart` CLI entry — checklist UX, not a wizard.
///
/// Mirrors the TUI Quickstart pane's structure: a single screen
/// listing all six selectors with `[ ]` / `[✓]` status and a one-line
/// summary, the user picks which selector to fill (any order), each
/// selector opens its own picker / field-form / channel-list sub-flow,
/// and `c` creates the agent once every selector is `[✓]`. There are
/// no pre-checked defaults anywhere — every selector starts `[ ]` and
/// is only satisfied by an explicit user choice (either a "Use
/// existing" pick of an already-configured alias, or a fully-filled
/// "Create new" entry).
///
/// All option lists, field shapes, presets, and the apply path come
/// directly from `clawcrew_runtime::quickstart` — the same module the
/// gateway and TUI surfaces consume. No RPC, no daemon: the CLI is
/// compiled in-process with `clawcrew-runtime` and calls
/// `snapshot_state` / `field_shape` / `apply_with_surface` as plain
/// functions.
///
/// Flag pre-fills (`--model-provider`, `--model`, `--api-key`,
/// `--agent`) silently seed the relevant selector's value and mark it
/// `[✓]` if the seed is enough to satisfy the selector; the user can
/// still open that selector and overwrite it.
#[cfg(feature = "agent-runtime")]
async fn run_quickstart_cli(
    model_provider: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    agent: Option<String>,
) -> anyhow::Result<()> {
    use dialoguer::{Confirm, Editor, FuzzySelect, Input};
    use clawcrew_config::presets::{
        AgentIdentity, BuilderSubmission, ChannelQuickStart, MemoryChoice, ModelProviderChoice,
        RISK_PRESETS, SelectorChoice,
    };
    use clawcrew_runtime::quickstart::{
        FieldSection, QuickstartTypeOption, Surface, apply_with_surface, field_shape,
        snapshot_state,
    };

    if !std::io::IsTerminal::is_terminal(&std::io::stdin())
        || !std::io::IsTerminal::is_terminal(&std::io::stderr())
    {
        anyhow::bail!(
            "{}",
            t(
                "cli-quickstart-needs-tty",
                "Quickstart is interactive and needs a terminal on stdin and stderr. \
                 Run it from an interactive shell, or use \
                 `clawcrew config set <path> <value>` for headless configuration."
            )
        );
    }

    #[derive(Default)]
    struct Form {
        provider: Option<ProviderChoice>,
        risk: Option<PresetChoice>,
        memory: Option<MemoryChoice>,
        channels: Vec<ChannelChoice>,
        // Tracks whether the user explicitly visited Channels and
        // confirmed "no channels". An empty `channels` Vec with
        // `channels_visited == false` is *not* satisfied — the
        // selector still shows `[ ]`.
        channels_visited: bool,
        peer_groups: Vec<clawcrew_config::presets::QuickstartPeerGroup>,
        // Mirrors `channels_visited`: peer groups are optional, so an
        // empty `peer_groups` Vec only counts as satisfied once the
        // user has actually opened the selector and left it. Until
        // then the row stays `[ ]` rather than a pre-checked default.
        peer_groups_visited: bool,
        agent: Option<AgentChoice>,
    }
    enum ProviderChoice {
        Fresh {
            kind: String,
            display_name: String,
            alias: String,
            model: String,
            /// Round-trip of every non-`model` descriptor value the
            /// daemon's `field_shape()` emitted, keyed by descriptor
            /// key. The CLI doesn't know what these mean — the daemon
            /// authored them and consumes them on the way back.
            fields: std::collections::HashMap<String, String>,
        },
        Existing {
            alias_ref: String,
        },
    }
    enum PresetChoice {
        Fresh(&'static str),
        Existing(String),
    }
    enum ChannelChoice {
        Fresh {
            kind: String,
            alias: String,
            extras: std::collections::BTreeMap<String, String>,
        },
        Existing {
            alias_ref: String,
        },
    }
    struct AgentChoice {
        name: String,
        system_prompt: String,
        personality_files: Vec<clawcrew_config::presets::QuickstartPersonalityFile>,
    }

    impl Form {
        fn provider_done(&self) -> bool {
            self.provider.is_some()
        }
        fn risk_done(&self) -> bool {
            self.risk.is_some()
        }
        fn memory_done(&self) -> bool {
            self.memory.is_some()
        }
        fn channels_done(&self) -> bool {
            self.channels_visited
        }
        fn peer_groups_done(&self) -> bool {
            self.peer_groups_visited
        }
        fn agent_done(&self) -> bool {
            self.agent
                .as_ref()
                .is_some_and(|a| !a.name.trim().is_empty())
        }
        fn all_done(&self) -> bool {
            self.provider_done()
                && self.risk_done()
                && self.memory_done()
                && self.channels_done()
                && self.agent_done()
        }
    }

    // ── Load config + canonical registries ──────────────────────
    let _dirs = crate::config::schema::resolve_runtime_dirs().await?;
    let mut cfg = Box::pin(crate::config::schema::Config::load_or_init()).await?;
    let state = snapshot_state(&cfg);
    let providers: &[QuickstartTypeOption] = &state.model_provider_types;
    let channel_types: &[QuickstartTypeOption] = &state.channel_types;
    if providers.is_empty() {
        anyhow::bail!(
            "Quickstart could not enumerate model providers — \
             clawcrew_providers::list_model_providers() returned no entries."
        );
    }

    let mut form = Form::default();

    if let (Some(mp), Some(m)) = (model_provider.as_deref(), model.as_deref())
        && let Some((canonical_provider, codex_auth)) =
            clawcrew_runtime::quickstart::resolve_model_provider_type(mp)
        && let Some(found) = providers
            .iter()
            .find(|p| p.kind.eq_ignore_ascii_case(canonical_provider))
    {
        let needs_key = !found.local && api_key.is_none() && !codex_auth;
        if !needs_key {
            let mut fields: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            if codex_auth {
                fields.insert("auth_mode".to_string(), "codex".to_string());
            }
            if let Some(key) = api_key.as_deref().filter(|s| !s.is_empty()) {
                // Submission field keys are snake_case (`api_key`) — the apply
                // path round-trips them verbatim into `set_prop_persistent`,
                // which rejects kebab-case with "Unknown property".
                fields.insert("api_key".to_string(), key.to_string());
            }
            form.provider = Some(ProviderChoice::Fresh {
                kind: found.kind.clone(),
                display_name: found.display_name.clone(),
                alias: "default".to_string(),
                model: m.to_string(),
                fields,
            });
        }
    }
    if let Some(a) = agent.as_deref() {
        let trimmed = a.trim();
        if !trimmed.is_empty() {
            form.agent = Some(AgentChoice {
                name: trimmed.to_string(),
                system_prompt: String::new(),
                personality_files: Vec::new(),
            });
        }
    }

    println!();
    println!(
        "{}",
        t(
            "cli-quickstart-title",
            "Quickstart — create one working agent end-to-end."
        )
    );
    println!();

    loop {
        // Render selector list with current status / summary.
        let glyph = |ok: bool| if ok { "[✓]" } else { "[ ]" };
        let provider_summary = match &form.provider {
            None => t("cli-quickstart-summary-not-yet-chosen", "not yet chosen"),
            Some(ProviderChoice::Fresh {
                display_name,
                alias,
                model,
                ..
            }) => qta(
                "cli-quickstart-summary-provider-fresh",
                &[("name", display_name), ("alias", alias), ("model", model)],
            ),
            Some(ProviderChoice::Existing { alias_ref }) => qta(
                "cli-quickstart-summary-use-existing",
                &[("reference", alias_ref)],
            ),
        };
        let preset_summary = |p: &Option<PresetChoice>| -> String {
            match p {
                None => t("cli-quickstart-summary-not-yet-chosen", "not yet chosen"),
                Some(PresetChoice::Fresh(name)) => {
                    qta("cli-quickstart-summary-preset-fresh", &[("name", name)])
                }
                Some(PresetChoice::Existing(a)) => {
                    qta("cli-quickstart-summary-use-existing", &[("reference", a)])
                }
            }
        };
        let memory_summary = match &form.memory {
            None => t("cli-quickstart-summary-not-yet-chosen", "not yet chosen"),
            Some(kind) => serde_json::to_value(kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{kind:?}").to_lowercase()),
        };
        let channels_summary = if !form.channels_visited {
            t("cli-quickstart-summary-not-yet-visited", "not yet visited")
        } else if form.channels.is_empty() {
            t(
                "cli-quickstart-summary-channels-none",
                "none (chat via `clawcrew agent` only)",
            )
        } else {
            form.channels
                .iter()
                .map(|c| match c {
                    ChannelChoice::Fresh { kind, alias, .. } => format!("{kind}.{alias}"),
                    ChannelChoice::Existing { alias_ref } => alias_ref.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let agent_summary = match &form.agent {
            None => t("cli-quickstart-summary-not-yet-named", "not yet named"),
            Some(a) => qta(
                "cli-quickstart-summary-agent",
                &[
                    ("alias", &a.name),
                    ("chars", &a.system_prompt.len().to_string()),
                    ("files", &a.personality_files.len().to_string()),
                ],
            ),
        };
        let peer_groups_summary = if form.peer_groups.is_empty() {
            t(
                "cli-quickstart-summary-peer-groups-none",
                "none — channels accept no peers",
            )
        } else {
            form.peer_groups
                .iter()
                .map(|pg| format!("{} → {}", pg.channel, pg.name))
                .collect::<Vec<_>>()
                .join(", ")
        };

        let risk_summary = preset_summary(&form.risk);
        let mut choices: Vec<(QuickstartChecklistAction, String)> = vec![
            (
                QuickstartChecklistAction::Provider,
                quickstart_row(
                    "cli-quickstart-row-model-provider",
                    glyph(form.provider_done()),
                    &provider_summary,
                ),
            ),
            (
                QuickstartChecklistAction::Risk,
                quickstart_row(
                    "cli-quickstart-row-risk-profile",
                    glyph(form.risk_done()),
                    &risk_summary,
                ),
            ),
            (
                QuickstartChecklistAction::Memory,
                quickstart_row(
                    "cli-quickstart-row-memory",
                    glyph(form.memory_done()),
                    &memory_summary,
                ),
            ),
            (
                QuickstartChecklistAction::Channels,
                quickstart_row(
                    "cli-quickstart-row-channels",
                    glyph(form.channels_done()),
                    &channels_summary,
                ),
            ),
            (
                QuickstartChecklistAction::PeerGroups,
                quickstart_row(
                    "cli-quickstart-row-peer-groups",
                    glyph(form.peer_groups_done()),
                    &peer_groups_summary,
                ),
            ),
            (
                QuickstartChecklistAction::Agent,
                quickstart_row(
                    "cli-quickstart-row-agent-identity",
                    glyph(form.agent_done()),
                    &agent_summary,
                ),
            ),
        ];
        let create_enabled = form.all_done();
        choices.push((
            QuickstartChecklistAction::Create,
            if create_enabled {
                t("cli-quickstart-create-agent", "── Create agent")
            } else {
                t(
                    "cli-quickstart-create-agent-locked",
                    "── Create agent (locked — fill every selector first)",
                )
            },
        ));

        let mut term = CrosstermQuickstartTerminal::stderr()?;
        // Fail closed when the terminal API cannot report its dimensions;
        // fitting against a guessed size would reintroduce row overflow.
        let Some(terminal_size) = quickstart_selector_terminal_size(&mut term) else {
            anyhow::bail!("{}", qta("cli-quickstart-terminal-size-unknown", &[]));
        };
        let (terminal_height, terminal_width) = terminal_size;
        let terminal_height = usize::from(terminal_height);
        let terminal_width = usize::from(terminal_width);
        let Some(row_budget) = quickstart_selector_row_budget(terminal_width) else {
            let terminal_width = terminal_width.to_string();
            let min_width = QUICKSTART_SELECTOR_MIN_WIDTH.to_string();
            anyhow::bail!(
                "{}",
                qta(
                    "cli-quickstart-terminal-too-narrow",
                    &[("width", &terminal_width), ("min_width", &min_width)],
                )
            );
        };
        let labels: Vec<String> = choices
            .iter()
            .map(|(_, label)| fit_quickstart_selector_row(label, row_budget))
            .collect();
        let min_height = quickstart_selector_min_height(labels.len());
        if !quickstart_selector_fits_height(terminal_height, labels.len()) {
            let terminal_height = terminal_height.to_string();
            let min_height = min_height.to_string();
            anyhow::bail!(
                "{}",
                qta(
                    "cli-quickstart-terminal-too-short",
                    &[("height", &terminal_height), ("min_height", &min_height)],
                )
            );
        }

        let prompt = fit_quickstart_selector_row(
            &t(
                "cli-quickstart-open-selector-prompt",
                "Open a selector (Enter), or pick Create. Esc to quit.",
            ),
            row_budget,
        );
        // Keep this checklist non-searchable and non-paged, and fail closed if
        // its fitted terminal dimensions change while it is active.
        let outcome = interact_quickstart_selector(&mut term, &labels, &prompt, terminal_size)?;
        // `process::exit` does not run destructors. Restore cooked mode before
        // preserving the selector's historical Ctrl+C exit semantics.
        drop(term);
        let pick = match outcome {
            QuickstartSelectorOutcome::Pick(pick) => pick,
            QuickstartSelectorOutcome::Interrupt => std::process::exit(130),
        };
        let action = quickstart_action_for_pick(&choices, pick);

        match action {
            QuickstartChecklistAction::Quit => {
                println!(
                    "{}",
                    t(
                        "cli-quickstart-cancelled",
                        "Quickstart cancelled. No config written."
                    )
                );
                return Ok(());
            }
            QuickstartChecklistAction::Create => {
                if !create_enabled {
                    println!(
                        "{}",
                        t(
                            "cli-quickstart-incomplete",
                            "  Not all selectors are filled yet."
                        )
                    );
                    continue;
                }
                break;
            }
            QuickstartChecklistAction::Provider => {
                // Step 1: pick Existing or Fresh, when there are
                // existing providers to choose from.
                let mut mode_labels: Vec<String> = Vec::new();
                let mut mode_kinds: Vec<&str> = Vec::new();
                if !state.model_providers.is_empty() {
                    mode_labels.push(t("cli-quickstart-use-existing", "Use existing"));
                    mode_kinds.push("existing");
                }
                mode_labels.push(t("cli-quickstart-create-new", "Create new"));
                mode_kinds.push("fresh");
                let mode = if mode_labels.len() == 1 {
                    Some(0)
                } else {
                    FuzzySelect::new()
                        .with_prompt(t("cli-quickstart-model-provider-prompt", "Model provider"))
                        .items(&mode_labels)
                        .default(0)
                        .max_length(mode_labels.len())
                        .interact_opt()?
                };
                let Some(mi) = mode else { continue };
                if mode_kinds[mi] == "existing" {
                    let labels: Vec<String> = state.model_providers.clone();
                    let Some(i) = FuzzySelect::new()
                        .with_prompt(t(
                            "cli-quickstart-pick-configured-provider",
                            "Pick a configured provider",
                        ))
                        .items(&labels)
                        .default(0)
                        .max_length(labels.len().max(1))
                        .interact_opt()?
                    else {
                        continue;
                    };
                    form.provider = Some(ProviderChoice::Existing {
                        alias_ref: labels[i].clone(),
                    });
                    continue;
                }
                // Fresh: type → alias → field form.
                let prov_labels: Vec<String> = providers
                    .iter()
                    .map(|p| {
                        if p.local {
                            qta(
                                "cli-quickstart-provider-local-label",
                                &[("name", &p.display_name)],
                            )
                        } else {
                            p.display_name.clone()
                        }
                    })
                    .collect();
                let Some(pi) = FuzzySelect::new()
                    .with_prompt(t("cli-quickstart-provider-type-prompt", "Provider type"))
                    .items(&prov_labels)
                    .default(0)
                    .max_length(prov_labels.len().max(1))
                    .interact_opt()?
                else {
                    continue;
                };
                let chosen = &providers[pi];
                let Ok(alias) = Input::<String>::new()
                    .with_prompt(qta(
                        "cli-quickstart-alias-for",
                        &[("name", &chosen.display_name)],
                    ))
                    .default("default".to_string())
                    .allow_empty(false)
                    .validate_with(|input: &String| {
                        clawcrew_config::helpers::validate_alias_key(input)
                    })
                    .interact_text()
                else {
                    continue;
                };
                // Field shape from the canonical schema.
                let descriptors = field_shape(FieldSection::ModelProvider, &chosen.kind);
                let mut model = String::new();
                let mut field_buf: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                let mut aborted = false;
                for d in &descriptors {
                    if d.key == "api_key" {
                        let skips_api_key =
                            quickstart_field_value_eq(&field_buf, "auth_mode", "codex")
                                || (chosen.kind == "anthropic"
                                    && quickstart_field_value_eq(
                                        &field_buf,
                                        "auth_mode",
                                        "setup_token",
                                    ));
                        if skips_api_key {
                            continue;
                        }
                    }
                    // For the model field, upgrade the descriptor with a
                    // live catalog so `prompt_for_field` renders a picker
                    // instead of a free-text input. Empty catalog (live=false)
                    // leaves the descriptor unchanged → free-text fallback.
                    let upgraded;
                    let d_used = if d.key.eq_ignore_ascii_case("model") {
                        let (models, _pricing, live) =
                            clawcrew_runtime::quickstart::model_catalog(&chosen.kind).await;
                        if live && !models.is_empty() {
                            upgraded = clawcrew_runtime::quickstart::FieldDescriptor {
                                kind: clawcrew_config::traits::PropKind::Enum,
                                enum_variants: Some(models),
                                ..d.clone()
                            };
                            &upgraded
                        } else {
                            d
                        }
                    } else {
                        d
                    };
                    let collected = prompt_for_field(d_used, None)?;
                    let Some(value) = collected else {
                        aborted = true;
                        break;
                    };
                    // `model` is hoisted to a top-level field on
                    // ProviderChoice for the summary line. Every other
                    // descriptor flows through `field_buf` keyed by
                    // its schema identifier — no cherry-picking.
                    if d.key.eq_ignore_ascii_case("model") {
                        model = value;
                    } else if !value.is_empty() && value != clawcrew_config::traits::UNSET_DISPLAY {
                        field_buf.insert(d.key.clone(), value);
                    }
                }
                if aborted {
                    continue;
                }
                if model.is_empty() {
                    eprintln!(
                        "{}",
                        qta(
                            "cli-quickstart-model-field-missing-warning",
                            &[("provider", &chosen.kind)],
                        )
                    );
                    let Ok(m) = Input::<String>::new()
                        .with_prompt(qta(
                            "cli-quickstart-model-id-for",
                            &[("name", &chosen.display_name)],
                        ))
                        .allow_empty(false)
                        .interact_text()
                    else {
                        continue;
                    };
                    model = m;
                }
                form.provider = Some(ProviderChoice::Fresh {
                    kind: chosen.kind.clone(),
                    display_name: chosen.display_name.clone(),
                    alias,
                    model,
                    fields: field_buf,
                });
            }
            QuickstartChecklistAction::Risk => {
                let chosen = pick_preset(
                    &t("cli-quickstart-risk-profile-prompt", "Risk profile"),
                    RISK_PRESETS
                        .iter()
                        .map(|p| (p.preset_name, p.label, p.help))
                        .collect(),
                    &state.risk_profiles,
                )?;
                if let Some(c) = chosen {
                    form.risk = Some(match c {
                        Ok(name) => PresetChoice::Fresh(name),
                        Err(alias) => PresetChoice::Existing(alias),
                    });
                }
            }
            QuickstartChecklistAction::Memory => {
                let kinds: [MemoryChoice; 6] = [
                    MemoryChoice::Sqlite,
                    MemoryChoice::Markdown,
                    MemoryChoice::Postgres,
                    MemoryChoice::Qdrant,
                    MemoryChoice::Lucid,
                    MemoryChoice::None,
                ];
                #[allow(clippy::no_effect_underscore_binding)]
                let _exhaustive = |k: MemoryChoice| match k {
                    MemoryChoice::Sqlite
                    | MemoryChoice::Markdown
                    | MemoryChoice::Postgres
                    | MemoryChoice::Qdrant
                    | MemoryChoice::Lucid
                    | MemoryChoice::None => (),
                };
                let labels: Vec<String> = kinds
                    .iter()
                    .map(|k| {
                        serde_json::to_value(k)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_string))
                            .unwrap_or_else(|| format!("{k:?}").to_lowercase())
                    })
                    .collect();
                let Some(i) = FuzzySelect::new()
                    .with_prompt(t("cli-quickstart-memory-backend-prompt", "Memory backend"))
                    .items(&labels)
                    .default(0)
                    .max_length(labels.len().max(1))
                    .interact_opt()?
                else {
                    continue;
                };
                form.memory = Some(kinds[i]);
            }
            QuickstartChecklistAction::Channels => {
                // Channels sub-flow: list current drafts + Add / Done.
                loop {
                    let mut items: Vec<String> = form
                        .channels
                        .iter()
                        .map(|c| match c {
                            ChannelChoice::Fresh { kind, alias, .. } => qta(
                                "cli-quickstart-channel-remove-row",
                                &[("reference", &format!("{kind}.{alias}"))],
                            ),
                            ChannelChoice::Existing { alias_ref } => qta(
                                "cli-quickstart-channel-remove-row",
                                &[("reference", alias_ref)],
                            ),
                        })
                        .collect();
                    items.push(t("cli-quickstart-add-channel", "+ Add a channel"));
                    items.push(t(
                        "cli-quickstart-channels-done",
                        "Done (channels selector counts as visited)",
                    ));
                    let Some(i) = FuzzySelect::new()
                        .with_prompt(t(
                            "cli-quickstart-channels-prompt",
                            "Channels (optional, 0..N)",
                        ))
                        .items(&items)
                        .default(items.len().saturating_sub(2))
                        .max_length(items.len())
                        .interact_opt()?
                    else {
                        break;
                    };
                    if i < form.channels.len() {
                        form.channels.remove(i);
                        continue;
                    }
                    if i == form.channels.len() {
                        // Add — pick Existing or Fresh.
                        let mut mode_labels: Vec<String> = Vec::new();
                        let mut mode_kinds: Vec<&str> = Vec::new();
                        if !state.unassigned_channels.is_empty() {
                            mode_labels.push(t("cli-quickstart-use-existing", "Use existing"));
                            mode_kinds.push("existing");
                        }
                        mode_labels.push(t("cli-quickstart-create-new", "Create new"));
                        mode_kinds.push("fresh");
                        let mode = if mode_labels.len() == 1 {
                            Some(0)
                        } else {
                            FuzzySelect::new()
                                .with_prompt(t(
                                    "cli-quickstart-channel-source-prompt",
                                    "Channel source",
                                ))
                                .items(&mode_labels)
                                .default(0)
                                .max_length(mode_labels.len())
                                .interact_opt()?
                        };
                        let Some(mi) = mode else { continue };
                        if mode_kinds[mi] == "existing" {
                            let labels: Vec<String> = state.unassigned_channels.clone();
                            if labels.is_empty() {
                                println!(
                                    "{}",
                                    t(
                                        "cli-quickstart-all-channels-bound",
                                        "  Every configured channel is already bound to an agent. Free one with `clawcrew config set agents.<alias>.channels ...` before reusing it here.",
                                    )
                                );
                                continue;
                            }
                            let Some(ei) = FuzzySelect::new()
                                .with_prompt(t(
                                    "cli-quickstart-pick-configured-channel",
                                    "Pick a configured channel",
                                ))
                                .items(&labels)
                                .default(0)
                                .max_length(labels.len().max(1))
                                .interact_opt()?
                            else {
                                continue;
                            };
                            form.channels.push(ChannelChoice::Existing {
                                alias_ref: labels[ei].clone(),
                            });
                            continue;
                        }
                        if channel_types.is_empty() {
                            println!(
                                "{}",
                                t(
                                    "cli-no-channels-compiled",
                                    "  No channel types are compiled into this binary."
                                )
                            );
                            continue;
                        }
                        let labels: Vec<String> = channel_types
                            .iter()
                            .map(|c| c.display_name.clone())
                            .collect();
                        let Some(ci) = FuzzySelect::new()
                            .with_prompt(t("cli-quickstart-channel-type-prompt", "Channel type"))
                            .items(&labels)
                            .default(0)
                            .max_length(labels.len().max(1))
                            .interact_opt()?
                        else {
                            continue;
                        };
                        let chosen = &channel_types[ci];
                        let Ok(alias) = Input::<String>::new()
                            .with_prompt(qta(
                                "cli-quickstart-alias-for",
                                &[("name", &chosen.display_name)],
                            ))
                            .default(chosen.kind.clone())
                            .allow_empty(false)
                            .interact_text()
                        else {
                            continue;
                        };
                        let descriptors = field_shape(FieldSection::Channel, &chosen.kind);
                        let mut extras: std::collections::BTreeMap<String, String> =
                            std::collections::BTreeMap::new();
                        let mut aborted = false;
                        for d in &descriptors {
                            let Some(value) = prompt_for_field(d, None)? else {
                                aborted = true;
                                break;
                            };
                            if !value.is_empty() && value != clawcrew_config::traits::UNSET_DISPLAY
                            {
                                extras.insert(d.key.clone(), value);
                            }
                        }
                        if aborted {
                            continue;
                        }
                        form.channels.push(ChannelChoice::Fresh {
                            kind: chosen.kind.clone(),
                            alias,
                            extras,
                        });
                        continue;
                    }
                    // Done.
                    form.channels_visited = true;
                    break;
                }
            }
            QuickstartChecklistAction::PeerGroups => {
                // Available channel refs: staged channels (this run) +
                // unassigned channels already in config. Refs already
                // covered by a staged peer-group are filtered out.
                let staged_refs: Vec<String> = form
                    .channels
                    .iter()
                    .map(|c| match c {
                        ChannelChoice::Fresh { kind, alias, .. } => format!("{kind}.{alias}"),
                        ChannelChoice::Existing { alias_ref } => alias_ref.clone(),
                    })
                    .collect();
                let claimed: std::collections::HashSet<String> = form
                    .peer_groups
                    .iter()
                    .map(|pg| pg.channel.clone())
                    .collect();
                let mut available: Vec<String> = staged_refs
                    .iter()
                    .chain(state.unassigned_channels.iter())
                    .filter(|r| !claimed.contains(r.as_str()))
                    .cloned()
                    .collect();
                available.dedup();
                loop {
                    let mut items: Vec<String> = form
                        .peer_groups
                        .iter()
                        .map(|pg| {
                            qta(
                                "cli-quickstart-peer-group-row",
                                &[
                                    ("channel", &pg.channel),
                                    ("name", &pg.name),
                                    ("count", &pg.external_peers.len().to_string()),
                                ],
                            )
                        })
                        .collect();
                    let drafts = items.len();
                    if !available.is_empty() {
                        items.push(t("cli-quickstart-add-peer-group", "+ Add peer group"));
                    }
                    items.push(t("cli-quickstart-done", "Done"));
                    let Some(pick) = FuzzySelect::new()
                        .with_prompt(t(
                            "cli-quickstart-peer-groups-prompt",
                            "Peer groups (Enter on a row to remove, + Add to create)",
                        ))
                        .items(&items)
                        .default(items.len() - 1)
                        .max_length(items.len())
                        .interact_opt()?
                    else {
                        break;
                    };
                    if pick < drafts {
                        form.peer_groups.remove(pick);
                        continue;
                    }
                    if pick == drafts && !available.is_empty() {
                        let Some(ch_idx) = FuzzySelect::new()
                            .with_prompt(t(
                                "cli-quickstart-channel-to-authorize-prompt",
                                "Channel to authorize",
                            ))
                            .items(&available)
                            .default(0)
                            .max_length(available.len())
                            .interact_opt()?
                        else {
                            continue;
                        };
                        let channel = available[ch_idx].clone();
                        let (ch_type, ch_alias) = match channel.split_once('.') {
                            Some(parts) => parts,
                            None => continue,
                        };
                        let name = format!("{ch_type}_{ch_alias}_default");
                        let Ok(peers_raw) = Input::<String>::new()
                            .with_prompt(t(
                                "cli-quickstart-external-peers-prompt",
                                "External peers (comma- or newline-separated, blank for none)",
                            ))
                            .allow_empty(true)
                            .interact_text()
                        else {
                            continue;
                        };
                        let external_peers: Vec<String> = peers_raw
                            .split([',', '\n'])
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        form.peer_groups
                            .push(clawcrew_config::presets::QuickstartPeerGroup {
                                name,
                                channel,
                                external_peers,
                                ignore: Vec::new(),
                            });
                        // The channel just got claimed; refresh the available list.
                        available = staged_refs
                            .iter()
                            .chain(state.unassigned_channels.iter())
                            .filter(|r| !form.peer_groups.iter().any(|pg| &pg.channel == *r))
                            .cloned()
                            .collect();
                        available.dedup();
                        continue;
                    }
                    // Done.
                    form.peer_groups_visited = true;
                    break;
                }
            }
            QuickstartChecklistAction::Agent => {
                let default_name = form
                    .agent
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_default();
                let mut input = Input::<String>::new()
                    .with_prompt(t("cli-quickstart-agent-alias-prompt", "Agent alias"))
                    .allow_empty(false)
                    .validate_with(|input: &String| {
                        clawcrew_config::helpers::validate_alias_key(input)
                    });
                if !default_name.is_empty() {
                    input = input.default(default_name);
                }
                let Ok(name) = input.interact_text() else {
                    continue;
                };
                let mut system_prompt = form
                    .agent
                    .as_ref()
                    .map(|a| a.system_prompt.clone())
                    .unwrap_or_default();
                let edit = Confirm::new()
                    .with_prompt(t(
                        "cli-quickstart-edit-system-prompt",
                        "Edit system prompt in $EDITOR? (blank if you skip)",
                    ))
                    .default(false)
                    .interact_opt()?;
                if let Some(true) = edit
                    && let Some(edited) = Editor::new().edit(&system_prompt)?
                {
                    system_prompt = edited;
                }
                // Personality files. The canonical list comes from the
                // snapshot — no hardcoded filenames. Pre-seed buffers
                // from any previously-staged content so re-entering
                // Agent doesn't drop the user's edits.
                let prior_files: std::collections::HashMap<String, String> = form
                    .agent
                    .as_ref()
                    .map(|a| {
                        a.personality_files
                            .iter()
                            .map(|f| (f.filename.clone(), f.content.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                // Pre-render the default template set once; the per-file
                // [t] Use template option seeds the editor from this map.
                let template_ctx =
                    clawcrew_runtime::agent::personality_templates::TemplateContext {
                        agent: trimmed_agent_name_for_templates(
                            form.agent.as_ref().map(|a| a.name.as_str()),
                        ),
                        ..Default::default()
                    };
                let templates: std::collections::HashMap<String, String> =
                    clawcrew_runtime::agent::personality_templates::render_preset_default(
                        &template_ctx,
                    )
                    .into_iter()
                    .map(|(filename, content)| (filename.to_string(), content))
                    .collect();
                let mut personality_results: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();

                #[derive(Clone, Copy)]
                enum PersonalityAction {
                    StartWithTemplate,
                    StartFromScratch,
                    Skip,
                }
                impl PersonalityAction {
                    fn label(self, has_staged: bool) -> String {
                        match self {
                            Self::StartWithTemplate => t(
                                "cli-quickstart-personality-start-template",
                                "Start with template (open in $EDITOR)",
                            ),
                            Self::StartFromScratch => {
                                if has_staged {
                                    t(
                                        "cli-quickstart-personality-start-current",
                                        "Start from current content (open in $EDITOR)",
                                    )
                                } else {
                                    t(
                                        "cli-quickstart-personality-start-scratch",
                                        "Start from scratch (open in $EDITOR)",
                                    )
                                }
                            }
                            Self::Skip => t("cli-quickstart-personality-skip", "Skip"),
                        }
                    }
                }

                let files = state.personality_files;
                let mut idx: usize = 0;
                let mut back_to_checklist = false;
                while idx < files.len() {
                    let filename = files[idx];
                    // Prefer a decision made earlier in this loop (e.g. after
                    // stepping back), else fall back to any pre-staged content.
                    let staged = personality_results
                        .get(filename)
                        .or_else(|| prior_files.get(filename))
                        .cloned()
                        .unwrap_or_default();
                    let template_available = templates.contains_key(filename);

                    let mut actions: Vec<PersonalityAction> = Vec::with_capacity(3);
                    if template_available {
                        actions.push(PersonalityAction::StartWithTemplate);
                    }
                    actions.push(PersonalityAction::StartFromScratch);
                    actions.push(PersonalityAction::Skip);
                    let has_staged = !staged.is_empty();
                    let choices: Vec<String> =
                        actions.iter().map(|a| a.label(has_staged)).collect();
                    let position = if files.len() > 1 {
                        format!(" [{}/{}]", idx + 1, files.len())
                    } else {
                        String::new()
                    };
                    let back_hint = if idx > 0 {
                        t("cli-quickstart-esc-go-back", " (Esc to go back)")
                    } else {
                        t(
                            "cli-quickstart-esc-return-checklist",
                            " (Esc to return to checklist)",
                        )
                    };
                    let label = qta(
                        "cli-quickstart-personality-file-prompt",
                        &[
                            ("filename", filename),
                            ("position", &position),
                            ("back_hint", &back_hint),
                        ],
                    );
                    let Some(pick) = FuzzySelect::new()
                        .with_prompt(label)
                        .items(&choices)
                        .default(0)
                        .max_length(choices.len())
                        .interact_opt()?
                    else {
                        // Esc steps back one file in the stack. On the first
                        // file there's nowhere earlier to go, so it returns to
                        // the base checklist.
                        if idx == 0 {
                            back_to_checklist = true;
                            break;
                        }
                        idx -= 1;
                        continue;
                    };
                    match actions[pick] {
                        PersonalityAction::StartWithTemplate => {
                            let seed = templates
                                .get(filename)
                                .cloned()
                                .unwrap_or_else(|| staged.clone());
                            if let Some(edited) = Editor::new().edit(&seed)?
                                && !edited.trim().is_empty()
                            {
                                personality_results.insert(filename.to_string(), edited);
                            }
                        }
                        PersonalityAction::StartFromScratch => {
                            if let Some(edited) = Editor::new().edit(&staged)?
                                && !edited.trim().is_empty()
                            {
                                personality_results.insert(filename.to_string(), edited);
                            }
                        }
                        PersonalityAction::Skip => {
                            // Keep any previously-staged content rather than
                            // dropping it silently.
                            if has_staged {
                                personality_results.insert(filename.to_string(), staged);
                            }
                        }
                    }
                    idx += 1;
                }
                if back_to_checklist {
                    continue;
                }
                // Materialize in canonical file order; only files with content.
                let personality_files: Vec<clawcrew_config::presets::QuickstartPersonalityFile> =
                    files
                        .iter()
                        .filter_map(|filename| {
                            personality_results.get(*filename).map(|content| {
                                clawcrew_config::presets::QuickstartPersonalityFile {
                                    filename: (*filename).to_string(),
                                    content: content.clone(),
                                }
                            })
                        })
                        .collect();
                form.agent = Some(AgentChoice {
                    name,
                    system_prompt,
                    personality_files,
                });
            }
        }
    }

    // ── Assemble submission ─────────────────────────────────────
    let inline_auth = match form.provider.as_ref() {
        Some(ProviderChoice::Fresh {
            kind,
            alias,
            fields,
            ..
        }) => quickstart_inline_auth(kind, alias, fields),
        _ => None,
    };

    let provider = form.provider.expect("provider satisfied");
    let provider_type = match &provider {
        ProviderChoice::Fresh { kind, .. } => kind.as_str(),
        ProviderChoice::Existing { alias_ref } => alias_ref
            .split_once('.')
            .map(|(provider_type, _)| provider_type)
            .unwrap_or(alias_ref),
    };
    let runtime_profile = SelectorChoice::Fresh(quickstart_runtime_profile_for_provider(
        provider_type,
        providers,
        &state.default_runtime_profile,
    ));
    let model_provider = match provider {
        ProviderChoice::Fresh {
            kind,
            alias,
            model,
            fields,
            ..
        } => SelectorChoice::Fresh(ModelProviderChoice {
            provider_type: kind,
            alias,
            model,
            fields,
        }),
        ProviderChoice::Existing { alias_ref } => SelectorChoice::Existing(alias_ref),
    };
    let risk_profile = match form.risk.expect("risk satisfied") {
        PresetChoice::Fresh(n) => SelectorChoice::Fresh(n.to_string()),
        PresetChoice::Existing(a) => SelectorChoice::Existing(a),
    };
    let memory = SelectorChoice::Fresh(form.memory.expect("memory satisfied"));
    let channels = form
        .channels
        .into_iter()
        .map(|c| match c {
            ChannelChoice::Fresh {
                kind,
                alias,
                extras,
                ..
            } => SelectorChoice::Fresh(ChannelQuickStart {
                channel_type: kind,
                alias,
                fields: extras.into_iter().collect(),
            }),
            ChannelChoice::Existing { alias_ref } => SelectorChoice::Existing(alias_ref),
        })
        .collect();
    let agent_choice = form.agent.expect("agent satisfied");
    let submission = BuilderSubmission {
        model_provider,
        risk_profile,
        runtime_profile,
        memory,
        channels,
        peer_groups: form.peer_groups,
        agent: AgentIdentity {
            name: agent_choice.name.clone(),
            system_prompt: agent_choice.system_prompt,
            personality_file: None,
            personality_files: agent_choice.personality_files,
        },
    };

    match Box::pin(apply_with_surface(submission, &mut cfg, Surface::Cli)).await {
        Ok(applied) => {
            println!();
            println!(
                "{}",
                ta(
                    "cli-quickstart-complete",
                    &[("alias", &applied.alias)],
                    "Quickstart complete."
                )
            );
            if let Some(auth) = inline_auth {
                Box::pin(run_inline_provider_auth(auth, &mut cfg)).await;
            }
            println!();
            println!("{}", t("cli-next-steps", "Next steps:"));
            println!(
                "{}",
                qta(
                    "cli-quickstart-next-agent-command",
                    &[("alias", &applied.alias)]
                )
            );
            if which_zerocode_on_path() {
                println!("  zerocode                   # launch the TUI"); // i18n-exempt: literal command/identifier example
            }
            Ok(())
        }
        Err(errs) => {
            eprintln!();
            eprintln!(
                "{}",
                t(
                    "cli-agent-not-created",
                    "Your agent was not created — and nothing on disk was changed."
                )
            );
            eprintln!(
                "{}",
                t(
                    "cli-quickstart-fix-and-rerun",
                    "Your existing config is untouched. Fix the following and run quickstart again:",
                )
            );
            eprintln!();
            for e in &errs {
                eprintln!("  • {}: {}", quickstart_step_label(e.step), e.message);
            }
            eprintln!();
            anyhow::bail!(
                "{}",
                qta(
                    "cli-quickstart-could-not-finish",
                    &[("count", &errs.len().to_string())],
                )
            )
        }
    }
}

#[cfg(feature = "agent-runtime")]
fn model_path_provider_type(path: &str) -> Option<&'static str> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() != 5 || parts[0] != "providers" || parts[1] != "models" || parts[4] != "model" {
        return None;
    }
    let family = parts[2];
    clawcrew_providers::list_model_providers()
        .iter()
        .find(|p| p.name == family)
        .map(|p| p.name)
}

#[cfg(any(feature = "agent-runtime", test))]
fn map_key_for_prop_path<'a>(section_path: &str, prop_path: &'a str) -> Option<&'a str> {
    let tail = prop_path.strip_prefix(section_path)?.strip_prefix('.')?;
    let mut parts = tail.split('.');
    let key = parts.next().filter(|key| !key.is_empty())?;
    parts.next()?;
    Some(key)
}

/// Split `section_arg` into the map key under `section_path` with NOTHING after
/// it, the `config init <section>.<alias>` shape.
#[cfg(any(feature = "agent-runtime", test))]
fn map_key_for_section_arg<'a>(section_path: &str, section_arg: &'a str) -> Option<&'a str> {
    let tail = section_arg.strip_prefix(section_path)?.strip_prefix('.')?;
    (!tail.is_empty() && !tail.contains('.')).then_some(tail)
}

/// Longest alias-materializable section whose path prefixes `path`, plus the
/// alias `split` extracts. `#[resource_key]` sections are excluded: their keys
/// are values from another domain (model id, voice, tool name) and may
/// themselves contain dots, so a dot split would yield a bogus alias.
#[cfg(any(feature = "agent-runtime", test))]
fn alias_target_for_path<'a>(
    path: &'a str,
    split: impl Fn(&str, &'a str) -> Option<&'a str>,
) -> Option<(&'static str, &'a str)> {
    Config::map_key_sections()
        .into_iter()
        .filter(|section| section.kind == clawcrew_config::traits::MapKeyKind::Map)
        .filter(|section| !section.resource_key)
        .filter_map(|section| split(section.path, path).map(|key| (section.path, key)))
        .max_by_key(|(section_path, _)| section_path.len())
}

/// `config init <section>.<alias>`: materialize a dynamic-map alias with schema
/// defaults. Returns the created `"<section>.<alias>"` path, or `None` when
/// `section_arg` is not a `<map-section>.<new-alias>` shape (the alias already
/// exists, the section is resource-keyed or a natural-key list, or the argument
/// is a plain nested prefix that `init_defaults` already handles). A reserved
/// alias is an error, not a silent no-op.
#[cfg(any(feature = "agent-runtime", test))]
fn init_map_alias(config: &mut Config, section_arg: &str) -> Result<Option<String>> {
    let Some((section_path, alias)) = alias_target_for_path(section_arg, map_key_for_section_arg)
    else {
        return Ok(None);
    };
    match clawcrew_config::alias_refs::create_map_key_checked(config, section_path, alias) {
        Ok(true) => Ok(Some(format!("{section_path}.{alias}"))),
        Ok(false) => Ok(None),
        Err(e) => Err(anyhow::Error::msg(e.to_string())),
    }
}

/// Dirty every generated leaf under a newly created map alias so required
/// default-valued fields survive the incremental writer's empty-leaf pruning.
#[cfg(feature = "agent-runtime")]
fn mark_new_map_alias_dirty(config: &mut Config, alias_path: &str) {
    let prefix = format!("{alias_path}.");
    let leaf_paths: Vec<String> = config
        .prop_fields()
        .into_iter()
        .filter_map(|field| field.name.starts_with(&prefix).then_some(field.name))
        .collect();

    if leaf_paths.is_empty() {
        config.mark_dirty(alias_path);
    } else {
        for path in leaf_paths {
            config.mark_dirty(&path);
        }
    }
}

#[cfg(any(feature = "agent-runtime", test))]
fn ensure_map_key_for_prop_path(config: &mut Config, prop_path: &str) -> Result<bool> {
    let Some((section_path, key)) = alias_target_for_path(prop_path, map_key_for_prop_path) else {
        return Ok(false);
    };

    // The alias already exists in the loaded config (e.g. a hyphenated cron
    // alias the TOML loader accepts and `config get`/`config list` resolve):
    // leave it alone. `create_map_key` applies the strict new-alias grammar,
    // which would reject a valid loaded key. Mirror `Config::ensure_map_key_for_path`,
    // which also skips creation for existing keys so alias validation runs only
    // when auto-materializing a brand-new alias.
    if config
        .get_map_keys(section_path)
        .is_some_and(|keys| keys.iter().any(|k| k == key))
    {
        return Ok(false);
    }

    // Route through the shared `create_map_key_checked` (not raw
    // `create_map_key`) so this CLI path inherits the reserved `default`
    // agent guard from the one place it's defined, rather than re-deriving
    // `section == "agents" && is_reserved_agent_alias(key)` here too. Without
    // this, widening past `providers.*` would let `config set
    // agents.default.enabled ...` auto-create the reserved runtime-fallback
    // agent alias, which the rename guard then refuses to ever rename.
    let created =
        match clawcrew_config::alias_refs::create_map_key_checked(config, section_path, key) {
            Ok(created) => created,
            Err(clawcrew_config::alias_refs::CreateError::Reserved(_)) => return Ok(false),
            Err(e) => return Err(anyhow::Error::msg(e.to_string())),
        };
    if created {
        // The section matched and the alias was newly materialized, but the
        // requested prop path might still not resolve (typo'd trailing field
        // name, or belt-and-suspenders against a resource-key path that
        // slipped past the `!resource_key` filter above). Roll back rather
        // than leave a phantom alias, falling through to the normal
        // "Unknown property" error exactly as before this alias existed.
        //
        // IMPORTANT: this probe/rollback must stay strictly inside the
        // `if created` branch. `create_map_key` returns `Ok(false)` when the
        // alias already existed (idempotent case) — never run this rollback
        // when `created == false`, or a bogus tail-field on an
        // ALREADY-EXISTING alias would delete a legitimate, pre-existing
        // config entry that has nothing to do with this call.
        if config.get_prop(prop_path).is_err() && !Config::prop_is_secret(prop_path) {
            let _ = config.delete_map_key(section_path, key);
            return Ok(false);
        }
        config.mark_dirty(&format!("{section_path}.{key}"));
    }
    Ok(created)
}

#[cfg(feature = "agent-runtime")]
fn trimmed_agent_name_for_templates(prior_name: Option<&str>) -> String {
    prior_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            clawcrew_runtime::agent::personality_templates::TemplateContext::default().agent
        })
}

#[cfg(feature = "agent-runtime")]
fn prompt_for_field(
    desc: &clawcrew_runtime::quickstart::FieldDescriptor,
    seed: Option<&str>,
) -> anyhow::Result<Option<String>> {
    use dialoguer::{FuzzySelect, Input};
    use clawcrew_config::traits::PropKind;
    if !desc.help.is_empty() {
        println!("  {}", desc.help);
    }
    let prompt = desc.label.clone();
    if desc.is_secret {
        match secret_prompt(&prompt, true) {
            Ok(pw) => {
                if !pw.is_empty() {
                    eprintln!("{}", ta("cli-secret-received", &[], "  ✓ Secret received"));
                }
                return Ok(Some(pw));
            }
            Err(e) => {
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::Interrupted)
                {
                    return Ok(None);
                }
                return Err(e);
            }
        }
    }
    if let (PropKind::Enum, Some(variants)) = (&desc.kind, &desc.enum_variants) {
        let Some(i) = FuzzySelect::new()
            .with_prompt(prompt)
            .items(variants)
            .default(0)
            .max_length(variants.len().max(1))
            .interact_opt()?
        else {
            return Ok(None);
        };
        return Ok(Some(variants[i].clone()));
    }
    let mut input = Input::<String>::new()
        .with_prompt(prompt)
        .allow_empty(!desc.required);
    if let Some(s) = seed {
        input = input.default(s.to_string());
    } else if let Some(d) = desc.default.as_deref()
        && !d.is_empty()
        && d != clawcrew_config::traits::UNSET_DISPLAY
    {
        // `<unset>` is a display placeholder for an unset Option, not a
        // real default. Seeding it pre-fills the prompt so a bare Enter
        // submits `<unset>`, which the daemon then validates against the
        // field's true type (e.g. a bool) and rejects.
        input = input.default(d.to_string());
    }
    // Same Ctrl+C-as-cancel mapping as the secret prompt branch above.
    match input.interact_text() {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            let io: std::io::Error = e.into();
            if io.kind() == std::io::ErrorKind::Interrupted {
                Ok(None)
            } else {
                Err(io.into())
            }
        }
    }
}

#[cfg(feature = "agent-runtime")]
fn pick_preset(
    prompt: &str,
    presets: Vec<(&'static str, &'static str, &'static str)>,
    existing: &[String],
) -> anyhow::Result<Option<Result<&'static str, String>>> {
    use dialoguer::FuzzySelect;
    let mut mode_labels: Vec<String> = Vec::new();
    let mut mode_kinds: Vec<&str> = Vec::new();
    if !existing.is_empty() {
        mode_labels.push(t("cli-quickstart-use-existing", "Use existing"));
        mode_kinds.push("existing");
    }
    mode_labels.push(t("cli-quickstart-pick-preset", "Pick a preset"));
    mode_kinds.push("preset");
    let mode = if mode_labels.len() == 1 {
        Some(0)
    } else {
        FuzzySelect::new()
            .with_prompt(prompt)
            .items(&mode_labels)
            .default(0)
            .max_length(mode_labels.len())
            .interact_opt()?
    };
    let Some(mi) = mode else { return Ok(None) };
    if mode_kinds[mi] == "existing" {
        let Some(i) = FuzzySelect::new()
            .with_prompt(qta(
                "cli-quickstart-pick-existing-prompt",
                &[("prompt", prompt)],
            ))
            .items(existing)
            .default(0)
            .max_length(existing.len().max(1))
            .interact_opt()?
        else {
            return Ok(None);
        };
        return Ok(Some(Err(existing[i].clone())));
    }
    let labels: Vec<String> = presets
        .iter()
        .map(|(_, label, help)| format!("{label}  —  {help}"))
        .collect();
    let Some(i) = FuzzySelect::new()
        .with_prompt(qta(
            "cli-quickstart-pick-preset-prompt",
            &[("prompt", prompt)],
        ))
        .items(&labels)
        .default(0)
        .max_length(labels.len().max(1))
        .interact_opt()?
    else {
        return Ok(None);
    };
    Ok(Some(Ok(presets[i].0)))
}

#[cfg(feature = "agent-runtime")]
fn which_zerocode_on_path() -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|p| p.join("zerocode").is_file()))
        .unwrap_or(false)
}

#[cfg(feature = "plugins-wasm")]
#[derive(Subcommand, Debug)]
enum PluginCommands {
    /// List installed and cached-registry plugins
    List,
    /// Search an installable plugin registry
    Search {
        /// Query to match against plugin names and descriptions
        query: String,
        /// Registry JSON URL to search
        #[arg(long)]
        registry: Option<String>,
    },
    /// Install a plugin from a local directory/manifest or registry name
    Install {
        /// Path to plugin directory/manifest, or registry name/version
        source: String,
        /// Registry JSON URL used for install-by-name
        #[arg(long)]
        registry: Option<String>,
    },
    /// Remove an installed plugin
    Remove {
        /// Plugin name
        name: String,
    },
    /// Show information about a plugin
    Info {
        /// Plugin name
        name: String,
    },
    /// Move plugins from legacy install directories into the configured one
    Migrate,
}

#[cfg(feature = "plugins-wasm")]
fn plugin_host_with_configured_security(
    config: &crate::config::schema::Config,
) -> Result<clawcrew::plugins::host::PluginHost> {
    let mode = clawcrew::plugins::host::PluginHost::resolve_signature_mode(
        &config.plugins.security.signature_mode,
    );
    let trusted = config.plugins.security.trusted_publisher_keys.clone();
    Ok(
        clawcrew::plugins::host::PluginHost::from_plugins_dir_with_security(
            &config.plugins.resolved_plugins_dir(),
            mode,
            trusted,
        )?,
    )
}

#[cfg(feature = "plugins-wasm")]
fn installed_plugin_config_entries(
    host: &clawcrew::plugins::host::PluginHost,
    plugin_name: &str,
) -> Result<Vec<(clawcrew::plugins::PluginCapability, String)>> {
    let manifest = host
        .manifest(plugin_name)
        .ok_or_else(|| anyhow::Error::msg("installed plugin manifest is unavailable"))?;
    if manifest.config_schema.is_none()
        || !manifest
            .capabilities
            .contains(&clawcrew::plugins::PluginCapability::Tool)
    {
        return Ok(Vec::new());
    }

    // Tool registration currently owns the only package-name runtime binding.
    // Alias-owned channel bindings must seed their actual instance key when
    // their production construction path lands; install must not invent one.
    let scope = clawcrew::plugins::instance::PluginInstanceScope::for_package_binding(
        manifest,
        clawcrew::plugins::PluginCapability::Tool,
        std::iter::empty(),
    )?;
    Ok(vec![(
        clawcrew::plugins::PluginCapability::Tool,
        scope.id().config_entry_key()?,
    )])
}

/// Seed empty `[[plugins.entries]]` blocks for a freshly installed plugin's
/// canonical default instance keys. `config set
/// plugins.entries.<instance-key>.config.<key>` routes through natural-key path
/// resolution, which only matches entries already present in live config.
/// Idempotent: existing entries and operator values remain untouched.
#[cfg(feature = "plugins-wasm")]
async fn seed_plugin_config_entries(
    config: &mut crate::config::schema::Config,
    entries: &[(clawcrew::plugins::PluginCapability, String)],
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let whole_config_degraded = config
        .degraded_security
        .iter()
        .any(|s| s == crate::config::migration::WHOLE_CONFIG_SENTINEL);
    if whole_config_degraded || config.degraded_sections.iter().any(|s| s == "plugins") {
        for (_, instance_key) in entries {
            eprintln!(
                "{}",
                ta(
                    "cli-plugin-config-entry-seed-skipped",
                    &[("name", instance_key)],
                    "warning: skipped seeding the plugin config entry: the \
                     [plugins] section on disk is malformed. Repair it, add \
                     `[[plugins.entries]]` with the instance key, then set values \
                     with `clawcrew config set plugins.entries.<instance-key>.config.<key>`."
                )
            );
        }
        return Ok(());
    }

    let mut created = Vec::new();
    for (_, instance_key) in entries {
        if config
            .create_map_key("plugins.entries", instance_key)
            .map_err(anyhow::Error::msg)?
        {
            config.mark_dirty(&format!("plugins.entries.{instance_key}"));
            created.push(instance_key);
        }
    }
    if created.is_empty() {
        return Ok(());
    }
    Box::pin(config.save_dirty()).await?;
    for instance_key in created {
        println!(
            "{}",
            ta(
                "cli-plugin-config-entry-seeded",
                &[("name", instance_key)],
                "Seeded config entry. Set plugin config values with \
                 `clawcrew config set plugins.entries.<instance-key>.config.<key>`."
            )
        );
    }
    Ok(())
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Dump the full configuration JSON Schema to stdout. With `--path`, returns
    /// the schema fragment for that property only — same payload `OPTIONS
    /// /api/config/prop?path=...` returns over HTTP.
    Schema {
        /// Property path to scope the schema dump (e.g.
        /// `agents.researcher.model_provider`). Without it, dumps the
        /// whole-config schema.
        #[arg(long)]
        path: Option<String>,
    },
    /// List all config properties with current values
    List {
        /// Filter by path prefix (e.g. "channels.telegram")
        #[arg(short, long)]
        filter: Option<String>,
        /// Show only secret (encrypted) fields
        #[arg(long)]
        secrets: bool,
    },
    /// Get a config property value
    Get {
        /// Property path (e.g. channels.telegram.mention-only)
        path: String,
        /// Emit a structured JSON envelope ({path, value} or {path, populated}) instead of plain text.
        #[arg(long)]
        json: bool,
    },
    /// Set a config property (secret fields auto-prompt for masked input)
    Set {
        /// Property path
        path: String,
        /// New value (omit for secret fields to get masked input)
        value: Option<String>,
        /// Skip interactive prompts — require value on command line, accept raw strings for enums
        #[arg(long)]
        no_interactive: bool,
        /// Optional comment to write alongside the value in TOML (preserves through future edits).
        #[arg(long)]
        comment: Option<String>,
        /// Emit a structured JSON envelope on success.
        #[arg(long)]
        json: bool,
    },
    /// Initialize unconfigured sections with defaults (enabled=false)
    Init {
        /// Section prefix (e.g. channels.matrix), or <section>.<alias> to create a new dynamic-map alias (e.g. risk_profiles.strict). Omit to init all.
        section: Option<String>,
        /// Emit a structured JSON envelope ({initialized: [...]}) instead of plain text.
        #[arg(long)]
        json: bool,
    },
    /// Migrate the on-disk config to the current schema version (preserves comments)
    Migrate {
        /// Emit a structured JSON envelope ({migrated, backup_path?, schema_version, valid?, error?}) instead of plain text.
        #[arg(long)]
        json: bool,
    },
    /// Apply a JSON Patch (RFC 6902) document atomically. Mirrors `PATCH /api/config`.
    /// Reads operations from the given file, or from stdin when path is `-` or omitted.
    /// Supported ops: `add`, `replace`, `remove`, `test`. `move` and `copy` are rejected.
    Patch {
        /// Path to a JSON Patch document, or `-` for stdin (default).
        input: Option<String>,
        /// Print results as JSON (one object per applied op) instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
    /// Print the API explorer URL (plus a hint if the daemon isn't running).
    Docs,
    Generate {
        /// Target schema version (e.g. 1, 2, 3). Defaults to current.
        version: Option<u32>,
        /// Encrypt secret-bearing string values in the output (api_key,
        /// bot_token, access_token, password, refresh_token, etc.). Works
        /// at every schema version via a key-name-based walker. Uses the
        /// resolved config-dir's `.secret_key` (creates one if missing).
        #[arg(long)]
        encrypt: bool,
    },
    /// Print matching property paths for shell completion (hidden)
    #[command(hide = true)]
    Complete {
        /// Partial path to complete
        partial: Option<String>,
    },
}

#[cfg(feature = "agent-runtime")]
#[derive(Subcommand, Debug)]
enum SecurityCommands {
    /// Show security posture for the default or selected agent risk profile
    Status {
        /// Agent alias whose effective runtime security posture should be inspected.
        #[arg(long)]
        agent: String,

        /// Emit machine-readable JSON instead of human text.
        #[arg(long)]
        json: bool,
    },

    /// Issue a client certificate from the daemon's mTLS CA for connecting over WSS.
    ///
    /// Reads the per-daemon CA at `<data_dir>/tls/ca.{crt,key}` (auto-generated on
    /// first run when `[wss]` is enabled) and writes a `clientAuth` certificate +
    /// key that zerocode (or any client) can present to the mutually-authenticated
    /// WSS plane.
    IssueClientCert {
        /// Subject/device identity stamped into the certificate (CN).
        #[arg(long, default_value = "zerocode")]
        name: String,

        /// Directory to write the certificate / key. Defaults to `<data_dir>/tls`.
        #[arg(long)]
        out_dir: Option<PathBuf>,

        /// Overwrite an existing certificate/key for this name.
        #[arg(long)]
        force: bool,
    },

    /// Revoke an issued client certificate so the daemon refuses it at the next
    /// WSS handshake (threat A5). The revoke is written to the issued-cert ledger,
    /// which materializes `<data_dir>/tls/revoked` for the verifier - no daemon
    /// restart needed. Identify the cert by `--fingerprint` (its SHA-256 hex) or
    /// `--device` (revokes every active cert that device holds).
    RevokeClientCert {
        /// SHA-256 fingerprint (hex) of the certificate to revoke.
        #[arg(long, conflicts_with = "device", required_unless_present = "device")]
        fingerprint: Option<String>,

        /// Device id whose active certificates should ALL be revoked.
        #[arg(
            long,
            conflicts_with = "fingerprint",
            required_unless_present = "fingerprint"
        )]
        device: Option<String>,
    },

    /// List the still-active client certificates issued by this daemon's CA
    /// (device id, fingerprint, validity) by reading the issued-cert ledger.
    ListClientCerts {
        /// Emit machine-readable JSON instead of a text table.
        #[arg(long)]
        json: bool,
    },

    /// Ask the running daemon to mint another enrollment pairing code.
    ///
    /// This adds another browser/zerocode device without restarting the daemon.
    /// It is a local operator command: the request is exchanged through the
    /// daemon's data dir, not through the public enrollment route.
    EnrollPaircode {
        /// Mint a new one-time enrollment code.
        #[arg(long)]
        new: bool,

        /// Seconds to wait for the running daemon to answer.
        #[arg(long, default_value_t = 5)]
        timeout_secs: u64,
    },

    /// Request an on-demand relay node-id rotation. The running daemon mints a
    /// fresh id, registers it alongside the old one for a grace window, then
    /// retires the old id; the new id reaches clients in-band on their next
    /// certificate renewal. Only applies when `[relay].node_id` is auto-minted.
    RelayRotateNodeId,
}

/// Issue a WSS client certificate signed by the daemon's per-daemon mTLS CA.
/// CA private-key at-rest protection sourced from the environment (decision:
/// opt-in passphrase, 0600 floor; threat A4). `CLAWCREW_CA_PASSPHRASE` (or a file
/// referenced by `CLAWCREW_CA_PASSPHRASE_FILE`) enables scrypt + XChaCha20-Poly1305
/// encryption of the CA key at rest; unset keeps the plaintext-0600 default so
/// zero-config and headless bring-up are unaffected. The daemon sources it
/// identically at CA generation (the WSS path) and at every CA read (enrollment
/// + this CLI), so the on-disk form always matches.
#[cfg(feature = "agent-runtime")]
fn ca_key_protection_from_env() -> clawcrew_tls::CaKeyProtection {
    clawcrew_tls::CaKeyProtection::from_env()
}

/// Resolve the WSS mTLS policy without conflating the auto-CA and BYO-CA modes.
///
/// The WSS plane is always mTLS. `enabled` controls only whether the configured
/// CA replaces the daemon-generated CA; certificate pins apply in either mode.
#[cfg(feature = "agent-runtime")]
fn resolve_wss_client_auth(
    client_auth: Option<&clawcrew_config::schema::WssClientAuthConfig>,
) -> Result<(Option<String>, Vec<String>)> {
    if let Some(config) = client_auth
        && !config.ca_cert_path.is_empty()
        && !config.enabled
    {
        anyhow::bail!(
            "[wss.client_auth].ca_cert_path is set but [wss.client_auth].enabled is false. \
             Set enabled = true to use your CA, or clear ca_cert_path to auto-generate one."
        );
    }

    let pinned = client_auth
        .map(|config| config.pinned_certs.clone())
        .unwrap_or_default();
    let byo_ca = client_auth
        .filter(|config| config.enabled && !config.ca_cert_path.is_empty())
        .map(|config| config.ca_cert_path.clone());
    Ok((byo_ca, pinned))
}

#[cfg(feature = "agent-runtime")]
fn wss_server_sans(wss_cfg: &clawcrew_config::schema::WssConfig) -> Vec<String> {
    if wss_cfg.sans.is_empty() {
        return Vec::new();
    }

    let mut sans = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    sans.extend(
        wss_cfg
            .sans
            .iter()
            .filter(|value| !value.trim().is_empty())
            .cloned(),
    );
    sans
}

#[cfg(all(test, feature = "agent-runtime"))]
mod wss_client_auth_tests {
    use super::*;

    #[test]
    fn auto_ca_honors_configured_client_certificate_pins() {
        let auth = clawcrew_config::schema::WssClientAuthConfig {
            pinned_certs: vec!["a".repeat(64)],
            ..Default::default()
        };

        let (byo_ca, pinned) = resolve_wss_client_auth(Some(&auth)).expect("valid auto-CA policy");
        assert!(byo_ca.is_none(), "the daemon CA remains selected");
        assert_eq!(
            pinned, auth.pinned_certs,
            "pins must reach the mTLS acceptor"
        );
    }

    #[test]
    fn disabled_byo_ca_is_rejected_before_listener_startup() {
        let auth = clawcrew_config::schema::WssClientAuthConfig {
            ca_cert_path: "/etc/clawcrew/client-ca.pem".into(),
            ..Default::default()
        };

        let err =
            resolve_wss_client_auth(Some(&auth)).expect_err("disabled BYO CA must fail closed");
        assert!(err.to_string().contains("enabled is false"));
    }

    #[test]
    fn wss_server_sans_adds_local_and_configured_sans() {
        let cfg = clawcrew_config::schema::WssConfig {
            sans: vec!["relay.example.test".into(), " ".into()],
            ..Default::default()
        };

        assert_eq!(
            wss_server_sans(&cfg),
            vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "relay.example.test".to_string(),
            ]
        );
        assert!(wss_server_sans(&clawcrew_config::schema::WssConfig::default()).is_empty());
    }
}

#[cfg(feature = "agent-runtime")]
fn issue_wss_client_cert(
    config: &Config,
    name: &str,
    out_dir: Option<PathBuf>,
    force: bool,
) -> Result<()> {
    let tls_dir = config.data_dir.join("tls");
    let ca_cert = tls_dir.join("ca.crt");
    let ca_key = tls_dir.join("ca.key");
    if !ca_cert.exists() || !ca_key.exists() {
        anyhow::bail!(
            "no daemon mTLS CA found at {}. Start the daemon once with [wss] enabled to \
             auto-generate it, or configure a bring-your-own CA.",
            tls_dir.display()
        );
    }

    // Per-device file names so issuing certs for multiple devices does not clobber.
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let has_out_dir = out_dir.is_some();
    let dest = out_dir.unwrap_or(tls_dir);
    let cert_path = dest.join(format!("client-{slug}.crt"));
    let key_path = dest.join(format!("client-{slug}.key"));
    let cert_tmp_path = dest.join(format!(".client-{slug}.crt.tmp"));
    let key_tmp_path = dest.join(format!(".client-{slug}.key.tmp"));
    if !force && (cert_path.exists() || key_path.exists()) {
        anyhow::bail!(
            "{} already exists. Pass --force to overwrite, or --out-dir / --name for a new one.",
            key_path.display()
        );
    }

    let ca_cert_pem = std::fs::read_to_string(&ca_cert)?;
    // Read the CA key honoring any at-rest passphrase, so an encrypted CA still
    // signs from the CLI (the key never leaves this process).
    let ca_key_pem = clawcrew_tls::load_ca_key_pem(&ca_key, &ca_key_protection_from_env())?;
    let issued = clawcrew_tls::issue_client_cert(&ca_cert_pem, &ca_key_pem, name)?;

    // Directory 0700, private key written 0600 atomically (no world-readable window).
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).ok();
        }
    }
    std::fs::write(&cert_tmp_path, issued.cert_pem.as_bytes())
        .with_context(|| format!("write staged certificate {}", cert_tmp_path.display()))?;
    if let Err(e) = clawcrew_tls::certgen::write_private_pem(&key_tmp_path, &issued.key_pem) {
        let _ = std::fs::remove_file(&cert_tmp_path);
        return Err(e)
            .with_context(|| format!("write staged private key {}", key_tmp_path.display()));
    }

    // Record the issuance in the daemon-owned ledger so this cert is revocable and
    // appears in the canonical "who holds which cert" record (actor = operator).
    //
    // Deliberately BEFORE the staged files are published: a ledger this command
    // could not write must not leave certificate material on disk, and an
    // over-recorded credential is recoverable where an unrecorded one is not
    // (see CertLedger::record_issued). The row is therefore active-but-
    // undelivered until the renames below succeed.
    use clawcrew_runtime::security::cert_ledger::{
        CertLedger, CertStatus, IssuanceActor, LedgerEntry,
    };
    let ledger_result = (|| -> Result<(CertLedger, String)> {
        let fingerprint = clawcrew_tls::single_cert_pem_sha256_fingerprint(&issued.cert_pem)
            .context("parse staged issued certificate")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let ledger = CertLedger::open_at(&config.data_dir, None, effective_crl_path(config))?;
        ledger.record_issued(
            &LedgerEntry {
                device_id: name.to_string(),
                fingerprint: fingerprint.clone(),
                not_before: now - 300,
                not_after: now + 30 * 86_400,
                status: CertStatus::Active,
                token_hash: String::new(),
                actor: IssuanceActor::Operator.label(),
                issued_at: now,
            },
            false,
        )?;
        Ok((ledger, fingerprint))
    })();
    let (ledger, fingerprint) = match ledger_result {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_file(&cert_tmp_path);
            let _ = std::fs::remove_file(&key_tmp_path);
            return Err(e);
        }
    };

    // Publication. A rename that fails leaves the ledger row undelivered, which
    // is exactly what the undelivered sweep needs to see: the operator never
    // got a usable pair, so the certificate is revoked at the next ledger open
    // rather than sitting active forever for a credential nobody holds.
    //
    // Both failure paths name the STAGED path as well as the destination - the
    // destination alone does not tell an operator which half of the operation
    // got where - and clear the staged material, so a private key never
    // survives a failed publish as a stray dotfile.
    if let Err(e) = std::fs::rename(&key_tmp_path, &key_path).with_context(|| {
        format!(
            "publish private key {} from staged {}",
            key_path.display(),
            key_tmp_path.display()
        )
    }) {
        let _ = std::fs::remove_file(&cert_tmp_path);
        let _ = std::fs::remove_file(&key_tmp_path);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&cert_tmp_path, &cert_path).with_context(|| {
        format!(
            "publish certificate {} from staged {}",
            cert_path.display(),
            cert_tmp_path.display()
        )
    }) {
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_file(&cert_tmp_path);
        return Err(e);
    }

    // Published: the operator now holds both halves, so the credential is
    // delivered. Marking BEFORE this point would have recorded a delivery the
    // filesystem never made.
    ledger.mark_delivered(&fingerprint).with_context(|| {
        format!(
            "record delivery of certificate {fingerprint}; the files were published but the \
             ledger could not record it, so this certificate will be revoked as undelivered - \
             re-issue it with --force"
        )
    })?;

    // When issuing into a separate out-dir, also lay it out as a drop-in client
    // `tls/` directory (ca.crt + client.crt + client.key). zerocode looks for
    // exactly these names under its <config-dir>/tls, so a client that copies this
    // directory needs no --tls-* flags at all.
    if has_out_dir {
        // The primary publish above already succeeded; a failure here must
        // still fail the command loudly - reporting success while the drop-in
        // directory is missing or stale hands the operator dead credentials.
        std::fs::copy(&ca_cert, dest.join("ca.crt")).with_context(|| {
            format!(
                "copy ca.crt into {}; the primary credentials were issued but this \
                 drop-in directory is incomplete - fix the directory and re-run with \
                 --force, or copy the published files by hand",
                dest.display()
            )
        })?;
        std::fs::write(dest.join("client.crt"), issued.cert_pem.as_bytes()).with_context(|| {
            format!(
                "write client.crt into {}; the primary credentials were issued but \
                 this drop-in directory is incomplete",
                dest.display()
            )
        })?;
        clawcrew_tls::certgen::write_private_pem(&dest.join("client.key"), &issued.key_pem)
            .with_context(|| {
                format!(
                    "write client.key into {}; the primary credentials were issued but \
                     this drop-in directory is incomplete",
                    dest.display()
                )
            })?;
    }

    let cert_path_display = cert_path.display().to_string();
    let key_path_display = key_path.display().to_string();
    let ca_cert_display = ca_cert.display().to_string();
    println!(
        "{}",
        ta("cli-mtls-issued-client-cert", &[("name", name)], "issued")
    );
    println!(
        "{}",
        ta(
            "cli-mtls-issued-cert-path",
            &[("path", &cert_path_display)],
            "cert"
        )
    );
    println!(
        "{}",
        ta(
            "cli-mtls-issued-key-path",
            &[("path", &key_path_display)],
            "key"
        )
    );
    println!(
        "{}",
        ta(
            "cli-mtls-issued-ca-path",
            &[("path", &ca_cert_display)],
            "CA"
        )
    );

    let relay = &config.relay;
    // node_id is auto-minted when unset, so resolve the real one (persisted) for
    // the guidance rather than requiring the operator to have pinned it.
    let relay_ready = relay.enabled && !relay.url.is_empty();
    let relay_node = if relay_ready {
        clawcrew_runtime::relay::ensure_node_id(&config.data_dir, &relay.node_id)
            .unwrap_or_else(|_| relay.node_id.clone())
    } else {
        relay.node_id.clone()
    };
    if has_out_dir {
        println!();
        println!("{}", t("cli-mtls-dropin-line-1", "drop-in TLS dir"));
        println!("{}", t("cli-mtls-dropin-line-2", "client key"));
        println!("{}", t("cli-mtls-dropin-line-3", "automatic TLS material"));
    }
    println!();
    if relay_ready {
        // The relay tunnels to the daemon's loopback listener, so the client does
        // not name a host: --connect defaults to wss://127.0.0.1 in relay mode.
        // The OUTER hop to the relay needs the relay's OWN ca (--relay-ca), which
        // is a different trust root from the daemon CA (--tls-ca-cert).
        let mut relay_flags = String::new();
        if !relay.relay_host.is_empty() {
            let _ = write!(relay_flags, " --relay-host {}", relay.relay_host);
        }
        if relay.relay_insecure {
            relay_flags.push_str(" --relay-insecure");
        } else if !relay.relay_ca_path.is_empty() {
            let _ = write!(relay_flags, " --relay-ca {}", relay.relay_ca_path);
        } else {
            relay_flags.push_str(" --relay-ca <relay-ca.crt>");
        }
        println!("{}", t("cli-mtls-relay-connect-header", "relay connect"));
        if has_out_dir {
            // i18n-exempt: literal zerocode command line; the flags are not translatable
            println!(
                "  zerocode --config-dir <dir-with-the-tls-folder> --relay {} --relay-node {}{}",
                relay.url, relay_node, relay_flags
            );
        } else {
            // i18n-exempt: literal zerocode command line; the flags are not translatable
            println!(
                "  zerocode --relay {} --relay-node {}{} --tls-ca-cert {} --tls-client-cert {} --tls-client-key {}",
                relay.url,
                relay_node,
                relay_flags,
                ca_cert.display(),
                cert_path.display(),
                key_path.display()
            );
        }
        println!("{}", t("cli-mtls-relay-ca-note-1", "relay CA note"));
        println!("{}", t("cli-mtls-relay-ca-note-2", "daemon CA note"));
    } else {
        println!("{}", t("cli-mtls-direct-connect-header", "direct connect"));
        // i18n-exempt: literal zerocode command line; the flags are not translatable
        println!(
            "  zerocode --connect wss://<host>:<port> --tls-ca-cert {} --tls-client-cert {} --tls-client-key {}",
            ca_cert.display(),
            cert_path.display(),
            key_path.display()
        );
    }
    Ok(())
}

/// Revoke an issued client certificate (or every active cert a device holds) in
/// the daemon ledger, which materializes `<data_dir>/tls/revoked` so the WSS
/// verifier refuses it at the next handshake (threat A5). The operator-driven
/// counterpart to `issue-client-cert`.
#[cfg(feature = "agent-runtime")]
fn revoke_wss_client_cert(
    config: &Config,
    fingerprint: Option<String>,
    device: Option<String>,
) -> Result<()> {
    use clawcrew_runtime::security::cert_ledger::CertLedger;
    // `operator` matches the issuance actor `issue-client-cert` records.
    const ACTOR: &str = "operator";
    let ledger = CertLedger::open_at(&config.data_dir, None, effective_crl_path(config))?;
    let changed = if let Some(fp) = fingerprint {
        let fp = fp.trim().to_ascii_lowercase();
        if ledger.mark_revoked(&fp, ACTOR)? {
            println!(
                "{}",
                ta(
                    "cli-mtls-revoked-certificate",
                    &[("fingerprint", &fp)],
                    "revoked"
                )
            );
            true
        } else {
            println!(
                "{}",
                ta(
                    "cli-mtls-revoke-no-active-fingerprint",
                    &[("fingerprint", &fp)],
                    "not found"
                )
            );
            false
        }
    } else if let Some(device_id) = device {
        let n = ledger.revoke_device(&device_id, ACTOR)?;
        let n_s = n.to_string();
        println!(
            "{}",
            ta(
                "cli-mtls-revoked-device-certs",
                &[("count", &n_s), ("device", &device_id)],
                "revoked"
            )
        );
        n > 0
    } else {
        // clap requires exactly one of --fingerprint / --device; defensive only.
        anyhow::bail!("provide --fingerprint <hex> or --device <id>");
    };
    if changed {
        // Report the path the verifier ACTUALLY reads - the same one the ledger
        // materialized to above. Printing the ledger default here would name a
        // file the verifier never consults whenever `[wss.client_auth].crl_path`
        // is set, which is exactly the moment (incident response) the operator
        // needs the real path.
        let revoked_path = effective_crl_path(config).display().to_string();
        println!(
            "{}",
            ta(
                "cli-mtls-revoked-list-updated",
                &[("path", &revoked_path)],
                "updated"
            )
        );
    }
    Ok(())
}

/// The revoked-fingerprint list this daemon's WSS verifier actually reads:
/// `[wss.client_auth].crl_path` when set, else the ledger default. Operator
/// commands must materialize to this path or a revocation is reported but never
/// enforced.
#[cfg(feature = "agent-runtime")]
fn effective_crl_path(config: &Config) -> std::path::PathBuf {
    clawcrew_runtime::security::cert_ledger::effective_revoked_list_path(
        &config.data_dir,
        config.wss.client_auth.as_ref().map(|c| c.crl_path.as_str()),
    )
}

/// List the still-active client certificates this daemon's CA has issued, read
/// from the issued-cert ledger. Read-only operator visibility into who holds a
/// live certificate.
#[cfg(feature = "agent-runtime")]
fn list_wss_client_certs(config: &Config, json: bool) -> Result<()> {
    use clawcrew_runtime::security::cert_ledger::CertLedger;
    let ledger = CertLedger::open_at(&config.data_dir, None, effective_crl_path(config))?;
    let active = ledger.list_active()?;
    if json {
        let rows: Vec<serde_json::Value> = active
            .iter()
            .map(|e| {
                serde_json::json!({
                    "device_id": e.device_id,
                    "fingerprint": e.fingerprint,
                    "not_before": e.not_before,
                    "not_after": e.not_after,
                    "issued_at": e.issued_at,
                    "actor": e.actor,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if active.is_empty() {
        println!("{}", t("cli-mtls-list-no-active-certs", "no active certs"));
        return Ok(());
    }
    let active_len = active.len().to_string();
    println!(
        "{}",
        ta(
            "cli-mtls-list-active-header",
            &[("count", &active_len)],
            "active certs"
        )
    );
    for e in &active {
        // i18n-exempt: structured cert row; device/not_after/actor are field identifiers
        println!(
            "  {}  device={}  not_after={}  actor={}",
            e.fingerprint, e.device_id, e.not_after, e.actor
        );
    }
    Ok(())
}

#[derive(Subcommand, Debug)]
enum EstopSubcommands {
    /// Print current estop status.
    Status,
    /// Resume from an engaged estop level.
    Resume {
        /// Resume only network kill.
        #[arg(long)]
        network: bool,
        /// Resume one or more blocked domain patterns.
        #[arg(long = "domain")]
        domains: Vec<String>,
        /// Resume one or more frozen tools.
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// OTP code. If omitted and OTP is required, a prompt is shown.
        #[arg(long)]
        otp: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum AuthCommands {
    /// Login with OAuth (OpenAI Codex, Gemini, or xAI)
    Login {
        /// ModelProvider (`openai-codex`, `gemini`, or `xai`)
        #[arg(long)]
        model_provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Use OAuth device-code flow
        #[arg(long)]
        device_code: bool,
        /// Import an existing auth.json file instead of starting a new login flow.
        /// Supports `openai-codex` (`~/.codex/auth.json`) and `xai` (`~/.grok/auth.json`).
        #[arg(long, value_name = "PATH", conflicts_with = "device_code")]
        import: Option<PathBuf>,
    },
    /// Complete OAuth by pasting redirect URL or auth code
    PasteRedirect {
        /// ModelProvider (`openai-codex`, `gemini`, or `xai`)
        #[arg(long)]
        model_provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Full redirect URL or raw OAuth code
        #[arg(long)]
        input: Option<String>,
    },
    /// Paste setup token / auth token (for Anthropic subscription auth)
    PasteToken {
        /// ModelProvider (`anthropic`)
        #[arg(long)]
        model_provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Token value (if omitted, read interactively)
        #[arg(long)]
        token: Option<String>,
        /// Auth kind override (`authorization` or `api-key`)
        #[arg(long)]
        auth_kind: Option<String>,
    },
    /// Alias for `paste-token` (interactive by default)
    SetupToken {
        /// ModelProvider (`anthropic`)
        #[arg(long)]
        model_provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Refresh OAuth access token using refresh token
    Refresh {
        /// ModelProvider (`openai-codex`, `gemini`, or `xai`)
        #[arg(long)]
        model_provider: String,
        /// Profile name or profile id
        #[arg(long)]
        profile: Option<String>,
    },
    /// Remove auth profile
    Logout {
        /// ModelProvider
        #[arg(long)]
        model_provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Set active profile for a model_provider
    Use {
        /// ModelProvider
        #[arg(long)]
        model_provider: String,
        /// Profile name or full profile id
        #[arg(long)]
        profile: String,
    },
    /// List auth profiles
    List,
    /// Show auth status with active profile and token expiry info
    Status,
    /// Authenticate an email channel via OAuth2 device-code flow
    EmailLogin {
        /// Email channel alias from [channels.email.<alias>] (e.g. 'hotmail')
        #[arg(long)]
        channel: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
}

#[derive(Subcommand, Debug)]
enum ModelCommands {
    /// Refresh and cache model_provider models
    Refresh {
        /// ModelProvider name (defaults to configured default model_provider)
        #[arg(long)]
        model_provider: Option<String>,

        /// Refresh all model_providers that support live model discovery
        #[arg(long)]
        all: bool,

        /// Force live refresh and ignore fresh cache
        #[arg(long)]
        force: bool,
    },
    /// List the models configured in config.toml
    List {
        /// ModelProvider name (defaults to all configured entries)
        #[arg(long)]
        model_provider: Option<String>,

        /// Verify each configured model against the provider's live catalog
        #[arg(long)]
        check: bool,
    },
    /// Set the default model in config
    Set {
        /// Model name to set as default
        model: String,
    },
    /// Show current model configuration and cache status
    Status,
}

#[derive(Subcommand, Debug)]
enum DoctorCommands {
    /// Probe model catalogs across model_providers and report availability
    Models {
        /// Probe a specific model_provider only (default: all known model_providers)
        #[arg(long)]
        model_provider: Option<String>,

        /// Prefer cached catalogs when available (skip forced live refresh)
        #[arg(long)]
        use_cache: bool,
    },
    /// Query runtime trace events (tool diagnostics and model replies)
    Traces {
        /// Show a specific trace event by id
        #[arg(long)]
        id: Option<String>,
        /// Filter list output by event type
        #[arg(long)]
        event: Option<String>,
        /// Case-insensitive text match across message/payload
        #[arg(long)]
        contains: Option<String>,
        /// Maximum number of events to display
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Update context_window in config.toml from provider /models endpoints
    UpdateContextWindows {
        /// Update a specific model_provider only (default: all known model_providers)
        #[arg(long)]
        model_provider: Option<String>,

        /// Show what would be updated without writing to config
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
enum BackupCommands {
    Create {
        #[arg(long, default_value = ".clawcrew/backups/latest.tar.gz")]
        dest: String,
    },
    Restore {
        #[arg(long)]
        source: String,
    },
}

#[derive(Subcommand, Debug)]
enum AppCommands {
    Install { url: String },
    Remove { name: String },
}

#[derive(Subcommand, Debug)]
enum MemoryCommands {
    /// List memory entries with optional filters
    List {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value = "50")]
        limit: usize,
        #[arg(long, default_value = "0")]
        offset: usize,
    },
    /// Get a specific memory entry by key
    Get {
        key: String,
    },
    /// Show memory backend statistics and health
    Stats,
    /// Clear memories by category, by key, or clear all
    Clear {
        /// Delete a single entry by key (supports prefix match)
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        category: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    Reindex,
}

/// Bootstrap the value of the global `--config-dir` flag before clap renders
/// localized help. The command comes from [`Cli::command`], so clap remains
/// responsible for option ownership, external-subcommand payloads, value
/// parsing, and the option terminator.
fn probe_config_dir(
    command: &clap::Command,
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Option<String> {
    // Help and version normally return display errors before exposing matches.
    // In this bootstrap view, make them ordinary parse boundaries and retain
    // the matches clap accumulated before the boundary.
    let matches = command
        .clone()
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .disable_version_flag(true)
        .ignore_errors(true)
        .try_get_matches_from(args)
        .ok()?;

    matches
        .try_get_one::<String>("config_dir")
        .ok()
        .flatten()
        .cloned()
}

fn apply_i18n_to_command(cmd: clap::Command) -> clap::Command {
    #[cfg(feature = "agent-runtime")]
    {
        apply_cmd_translations(cmd, "cli")
    }
    #[cfg(not(feature = "agent-runtime"))]
    cmd
}

#[cfg(feature = "agent-runtime")]
fn apply_cmd_translations(cmd: clap::Command, prefix: &str) -> clap::Command {
    let sub_names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();

    let about_key = format!("{prefix}-about");
    let cmd = match crate::i18n::get_cli_string(&about_key) {
        Some(about) => cmd.about(about),
        None => cmd,
    };

    let long_about_key = format!("{prefix}-long-about");
    let cmd = match crate::i18n::get_cli_string(&long_about_key) {
        Some(long_about) => cmd.long_about(long_about),
        None => cmd,
    };

    let mut cmd = cmd;
    for name in &sub_names {
        let child_prefix = format!("{prefix}-{name}");
        cmd = cmd.mut_subcommand(name, |sub| apply_cmd_translations(sub, &child_prefix));
    }
    cmd
}

#[cfg(feature = "agent-runtime")]
fn validated_locale(locale: &str) -> Result<String> {
    let ok_shape = !locale.is_empty()
        && locale.len() <= 16
        && locale
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !ok_shape {
        bail!("invalid locale code '{locale}'");
    }
    let known = clawcrew_runtime::i18n::available_locales();
    if !known.iter().any(|o| o.code == locale) {
        let codes: Vec<&str> = known.iter().map(|o| o.code.as_str()).collect();
        bail!(
            "locale '{locale}' is not in the locales.toml registry; known: {}",
            codes.join(", ")
        );
    }
    Ok(locale.to_string())
}

#[cfg(feature = "agent-runtime")]
async fn fetch_locales(locale: &str, catalog: Option<&str>) -> Result<()> {
    let locale = validated_locale(locale)?;

    let selected: Vec<&(&str, &str, &str)> = match catalog {
        None => clawcrew_config::schema::FTL_CATALOGS.iter().collect(),
        Some(list) => {
            let names: Vec<&str> = list
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let mut out = Vec::new();
            for name in &names {
                match clawcrew_config::schema::FTL_CATALOGS
                    .iter()
                    .find(|(n, _, _)| n == name)
                {
                    Some(entry) => out.push(entry),
                    None => {
                        let valid = clawcrew_config::schema::FTL_CATALOGS
                            .iter()
                            .map(|(n, _, _)| *n)
                            .collect::<Vec<_>>()
                            .join(", ");
                        bail!("unknown catalog '{name}'; valid: {valid}");
                    }
                }
            }
            out
        }
    };

    let dest = clawcrew_config::schema::ftl_locale_dir(&locale)?;
    std::fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;
    // Confinement check: the resolved dest must live under the data-dir FTL root.
    let ftl_root = dest
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dest.clone());
    let canon_dest = std::fs::canonicalize(&dest).unwrap_or_else(|_| dest.clone());
    let canon_root = std::fs::canonicalize(&ftl_root).unwrap_or(ftl_root);
    if !canon_dest.starts_with(&canon_root) {
        bail!("refusing to write outside the FTL data directory");
    }

    // Prefer the tag matching this binary; fall back to master.
    let version = env!("CARGO_PKG_VERSION");
    let refs = [format!("v{version}"), "master".to_string()];
    let client = reqwest::Client::new();
    let mut fetched = 0u32;

    for (name, path_tmpl, out_name) in selected {
        let repo_path = path_tmpl.replace("{locale}", &locale);
        let mut body: Option<String> = None;
        for git_ref in &refs {
            let url = format!(
                "https://raw.githubusercontent.com/clawcrew-labs/clawcrew/{git_ref}/{repo_path}"
            );
            let resp = client.get(&url).send().await?;
            if resp.status().is_success() {
                body = Some(resp.text().await?);
                break;
            }
        }
        match body {
            Some(content) => {
                let out_path = dest.join(out_name);
                std::fs::write(&out_path, content)
                    .with_context(|| format!("writing {}", out_path.display()))?;
                println!(
                    "{}",
                    ta(
                        "cli-locales-fetched",
                        &[("name", name), ("path", &out_path.display().to_string())],
                        "fetched catalogue",
                    )
                );
                fetched += 1;
            }
            None => {
                eprintln!(
                    "{}",
                    ta(
                        "cli-locales-skipped",
                        &[
                            ("name", name),
                            ("path", &repo_path),
                            ("refs", &refs.join(", "))
                        ],
                        "skipped: not on upstream",
                    )
                );
            }
        }
    }

    if fetched == 0 {
        bail!("no catalogues fetched for locale '{locale}'");
    }
    println!(
        "{}",
        ta(
            "cli-locales-installed",
            &[
                ("count", &fetched.to_string()),
                ("locale", &locale),
                ("dir", &dest.display().to_string())
            ],
            "Installed catalogues",
        )
    );
    Ok(())
}

fn main() -> Result<()> {
    let command = Cli::command();

    // Locale detection runs while clap builds localized help, so expose the CLI
    // override through the bootstrap env before either i18n or Tokio starts.
    // Empty values remain for clap's canonical parse/validation path below.
    if let Some(config_dir) = probe_config_dir(&command, std::env::args_os())
        && !config_dir.trim().is_empty()
    {
        // SAFETY: this synchronous bootstrap runs before the Tokio runtime (and
        // therefore its worker threads) is constructed.
        unsafe { std::env::set_var("CLAWCREW_CONFIG_DIR", config_dir) };
    }

    async_main(command)
}

/// Explicit runtime construction instead of `#[tokio::main]` so worker
/// threads get an 8 MiB stack. Debug builds of the deepest inline RPC
/// handlers (quickstart apply walks the whole config tree with several
/// `Config`-sized temporaries) overflow tokio's 2 MiB worker default and
/// abort the daemon. The size matches the 8 MiB main-thread stacks the
/// workspace already requests via linker args on other targets.
fn async_main(command: clap::Command) -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()?
        .block_on(async_main_inner(command))
}

/// True when a desktop entry's `Name` deliberately identifies ClawCrew: it is
/// exactly "ClawCrew" or "ClawCrew" followed by a separator (e.g. "ClawCrew
/// Companion"), case-insensitively. Matching the visible application name — not
/// any field that merely contains the substring "clawcrew" — is what stops an
/// unrelated entry (or a lookalike like `not-clawcrew-helper`) from qualifying.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn is_clawcrew_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    match lower.strip_prefix("clawcrew") {
        Some("") => true,
        Some(rest) => rest.starts_with([' ', '-', '_']),
        None => false,
    }
}

/// Reserved characters that the Desktop Entry Specification requires to be
/// double-quoted in an `Exec` value. Encountering one outside quotes means the
/// value is malformed, so parsing fails closed rather than launching a partially
/// interpreted path.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
const EXEC_RESERVED_CHARS: &[char] = &[
    '"', '`', '$', '\\', '>', '<', '~', '|', '&', ';', '*', '?', '#', '(', ')', '\'',
];

/// Apply the Desktop Entry Specification's general string-value unescape rules
/// (`\s \n \t \r \\`) to the raw `Exec` value. The spec applies this layer
/// *before* the `Exec` quoting rules, so e.g. a literal `$` in a quoted path is
/// written `\\$`: the general layer turns `\\` into `\`, leaving `\$` for the
/// quoting layer. Any other escape, or a dangling backslash, is malformed and
/// fails closed (`None`).
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn unescape_desktop_value(raw: &str) -> Option<String> {
    let mut out = String::new();
    // An escape consumes the following char too; the `while let` body advances
    // the same iterator, so it can't be a `for` loop.
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            _ => return None,
        }
    }
    Some(out)
}

/// A `%X` token is a known desktop-entry field code (or `%%`, a literal percent).
/// An unknown field code invalidates the whole `Exec` command line per the spec.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn is_known_field_code(token: &str) -> bool {
    token == "%%"
        || matches!(
            token,
            "%f" | "%F"
                | "%u"
                | "%U"
                | "%i"
                | "%c"
                | "%k"
                | "%d"
                | "%D"
                | "%n"
                | "%N"
                | "%v"
                | "%m"
        )
}

/// Tokenize a (general-unescaped) desktop-entry `Exec` value into its whitespace-
/// separated arguments, applying the `Exec` quoting rules to each. Each token is
/// returned with a flag recording whether it was quoted, so field-code
/// validation can reject a field code that appears inside a quoted argument (the
/// Desktop Entry Specification forbids that). Fails closed (`None`) on any
/// malformed token: an unterminated quote, a dangling or invalid escape, a raw
/// reserved character (`"`, `` ` ``, `$`, `\` unquoted or an unescaped `$`/`` ` ``
/// inside quotes), or text directly adjacent to a closing quote (e.g. `"…"junk`).
/// Validating the whole line — not just the first token — is what keeps a
/// malformed entry from launching its first argument.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn tokenize_exec_line(line: &str) -> Option<Vec<(String, bool)>> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut token = String::new();
        let quoted = chars.peek() == Some(&'"');
        if quoted {
            chars.next(); // opening quote
            loop {
                match chars.next() {
                    Some('"') => break, // closing quote
                    Some('\\') => match chars.next() {
                        Some(esc @ ('"' | '`' | '$' | '\\')) => token.push(esc),
                        _ => return None, // invalid or dangling escape inside quotes
                    },
                    // An unterminated quote, or an unescaped reserved character
                    // (`$`/`` ` ``) inside quotes: fail closed.
                    None | Some('$' | '`') => return None,
                    Some(c) => token.push(c),
                }
            }
            // A closing quote must end the token; adjacent text is malformed.
            if matches!(chars.peek(), Some(c) if !c.is_whitespace()) {
                return None;
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                if EXEC_RESERVED_CHARS.contains(&c) {
                    return None; // a reserved character must be quoted
                }
                token.push(c);
                chars.next();
            }
        }
        tokens.push((token, quoted));
    }
    Some(tokens)
}

/// Validate the field codes carried by a single tokenized `Exec` argument per the
/// Desktop Entry Specification. Inside a token, the only permitted `%` is the
/// escaped literal `%%`; a bare, embedded, or unknown field code (`%U`, `%Z`,
/// `ClawCrew-%Z.AppImage`, `--flag=%U`) invalidates the command line. The one
/// exception is that an *argument* (never the program) that was *not* quoted may
/// be exactly one known standalone field code such as `%U`. A field code inside a
/// quoted argument is always rejected.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn exec_token_field_codes_ok(token: &str, quoted: bool, is_program: bool) -> bool {
    // A lone, unquoted, standalone known field code is a valid argument — but the
    // program (executable) can never be a field code, so it has no exception.
    if !is_program && !quoted && is_known_field_code(token) {
        return true;
    }
    // Otherwise every `%` must be the escaped literal `%%`.
    let mut chars = token.chars();
    while let Some(c) = chars.next() {
        if c == '%' && chars.next() != Some('%') {
            return false;
        }
    }
    true
}

/// Parse the program token (first argument) from a desktop-entry `Exec=` value,
/// per the Desktop Entry Specification. The general string-unescape layer is
/// applied first (see [`unescape_desktop_value`]), then the whole command line is
/// tokenized with the `Exec` quoting rules (see [`tokenize_exec_line`]). Parsing
/// fails closed (`None`) on malformed input anywhere on the line — an unterminated
/// quote, a dangling/invalid escape, an unquoted reserved character, text adjacent
/// to a closing quote, an unknown field code (e.g. `%Z`), an `=` in the program
/// token, or a program token that is empty or itself a field code — rather than
/// launching a partially interpreted path.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn parse_exec_program(exec: &str) -> Option<String> {
    let unescaped = unescape_desktop_value(exec)?;
    let mut tokens = tokenize_exec_line(&unescaped)?.into_iter();
    let (program, program_quoted) = tokens.next()?;
    // The executable may not be empty, carry an `=`, or contain any field code
    // (bare or embedded — only an escaped `%%` literal is allowed). This rejects
    // a program like `ClawCrew-%Z.AppImage` whose basename would otherwise pass
    // the AppImage-name check.
    if program.is_empty()
        || program.contains('=')
        || !exec_token_field_codes_ok(&program, program_quoted, true)
    {
        return None;
    }
    // Every argument token must likewise carry no field code, except a single
    // unquoted standalone known field code. An unknown, embedded, or quoted field
    // code anywhere on the line invalidates it.
    for (token, quoted) in tokens {
        if !exec_token_field_codes_ok(&token, quoted, false) {
            return None;
        }
    }
    Some(program)
}

/// The published companion-app binary name (the `Exec` of `ClawCrew.desktop` in
/// the v0.8.3 Debian package). This is the single source of truth for the
/// supported non-AppImage executable, so discovery cannot select a lookalike
/// such as `clawcrew-helper` or `clawcrew-evil`.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
const CLAWCREW_DESKTOP_BIN: &str = "clawcrew-desktop";

/// True when a desktop entry's resolved `Exec` program is a supported ClawCrew
/// executable: either the exact published binary `clawcrew-desktop`, or a
/// ClawCrew AppImage in the published `ClawCrew-*.AppImage` form. It is bound to
/// those forms — not to any `clawcrew*` basename — so a deliberate ClawCrew
/// `Name` cannot be paired with a lookalike (`clawcrew-helper`, `clawcrew-evil`)
/// to preempt the real app.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn is_clawcrew_program(program: &str) -> bool {
    let Some(name) = Path::new(program).file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower == CLAWCREW_DESKTOP_BIN || is_clawcrew_appimage_name(name)
}

/// True when a bare file name is a supported ClawCrew AppImage in the published
/// `ClawCrew-*.AppImage` form: it begins with "clawcrew-" (the separator is
/// required) and ends with ".appimage", case-insensitively. Requiring the
/// separator rejects lookalikes with no boundary such as `ClawCrewevil.AppImage`
/// as well as `not-clawcrew-helper.AppImage`.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn is_clawcrew_appimage_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.starts_with("clawcrew-") && lower.ends_with(".appimage")
}

/// Read the `Exec` target from a desktop entry, but only when the entry is a
/// ClawCrew application, so an unrelated `.desktop` file is never launched.
/// Identity is a bounded combination, not a display name alone: the entry must
/// be `Type=Application`, its `Name` must deliberately identify ClawCrew (see
/// [`is_clawcrew_name`]), and its resolved `Exec` program must be a ClawCrew
/// executable (see [`is_clawcrew_program`]). Only the `[Desktop Entry]` group is
/// consulted, a `Hidden=true` ("masked") entry is ignored, and the `Exec` value
/// is parsed with the desktop-entry quoting grammar (see [`parse_exec_program`]).
///
/// Gated with the `desktop` command's `which` dependency (`agent-runtime`) on
/// Linux, matching its sole caller and the desktop-entry tests.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn clawcrew_desktop_exec(contents: &str) -> Option<String> {
    let mut in_entry = false;
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut entry_type: Option<String> = None;
    let mut hidden = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        // Only the default (non-localized) key matters; first occurrence wins.
        match key.trim() {
            "Name" if name.is_none() => name = Some(value.trim().to_string()),
            "Exec" if exec.is_none() => exec = Some(value.trim().to_string()),
            "Type" if entry_type.is_none() => entry_type = Some(value.trim().to_string()),
            "Hidden" if value.trim().eq_ignore_ascii_case("true") => hidden = true,
            _ => {}
        }
    }
    if hidden {
        return None;
    }
    // A launchable app entry only: `Type` must be `Application`, per the
    // published `ClawCrew.desktop` contract. A non-`Application` entry (e.g.
    // `Link`/`Directory`) never resolves.
    if !entry_type
        .as_deref()
        .is_some_and(|t| t.eq_ignore_ascii_case("Application"))
    {
        return None;
    }
    if !is_clawcrew_name(&name?) {
        return None;
    }
    let program = parse_exec_program(&exec?)?;
    if !is_clawcrew_program(&program) {
        return None;
    }
    Some(program)
}

/// True when `path` is a regular file with an execute bit set.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Resolve a desktop-entry command to an executable file: an absolute path is
/// taken as-is (and must be executable), a bare command name is resolved through
/// `PATH`. Non-executable candidates are rejected so a broken entry is skipped.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn resolve_executable(command: &str) -> Option<PathBuf> {
    let candidate = Path::new(command);
    if candidate.is_absolute() {
        return is_executable(candidate).then(|| candidate.to_path_buf());
    }
    // A relative value containing a path separator (e.g. `./clawcrew-helper`) would be
    // resolved by `which` against the current working directory, letting a desktop entry
    // launch a binary from wherever `clawcrew desktop` happened to run. Per the Desktop
    // Entry spec `Exec` must be an absolute path or a bare executable name resolved on
    // `PATH`, so reject any relative value that carries a separator.
    if command.contains('/') {
        return None;
    }
    which::which(command).ok()
}

/// Maximum accepted size of one XDG desktop entry. Desktop files are small
/// metadata documents; bounding ambient entries prevents one unrelated file
/// from consuming unbounded memory before a valid ClawCrew entry is reached.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
const DESKTOP_ENTRY_MAX_BYTES: u64 = 256 * 1024;

/// Open and read a desktop entry without following its final symlink, blocking
/// on a FIFO, or trusting pathname metadata that can change before the open.
/// Classification and the byte limit are both applied to the opened handle.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn read_desktop_entry(path: &Path) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > DESKTOP_ENTRY_MAX_BYTES {
        return None;
    }

    let mut bytes = Vec::new();
    let mut limited = file.take(DESKTOP_ENTRY_MAX_BYTES + 1);
    limited.read_to_end(&mut bytes).ok()?;
    if u64::try_from(bytes.len()).ok()? > DESKTOP_ENTRY_MAX_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Recursively collect `.desktop` entries under `root` (an `applications`
/// directory) as `(desktop-file-id, path)` pairs. Per the Desktop Entry
/// Specification the ID is the path relative to `root` with directory
/// separators replaced by `-`, so a nested `kde/foo.desktop` has ID
/// `kde-foo.desktop`. Deriving IDs recursively (rather than from top-level
/// basenames only) is what lets a nested higher-precedence entry correctly
/// mask the same ID in a lower-precedence directory.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn collect_desktop_entries(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
    visited: &mut std::collections::HashSet<PathBuf>,
) {
    // Guard against directory cycles (e.g. a bind mount pointing back up the tree) by
    // tracking canonical paths already scanned.
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `entry.file_type()` does not follow symlinks, so a directory symlink such as
        // `applications/loop -> .` is not treated as a directory and is never recursed
        // into — preventing an unbounded traversal.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_desktop_entries(root, &path, out, visited);
        } else if file_type.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("desktop")
            && let Ok(rel) = path.strip_prefix(root)
        {
            let id = rel.to_string_lossy().replace('/', "-");
            out.push((id, path));
        }
    }
}

/// Scan `applications` subdirectories of the given XDG base dirs (already in
/// precedence order) for a ClawCrew desktop entry and return its executable
/// `Exec` target. The first occurrence of a desktop-file ID wins and shadows the
/// same ID in later (lower-precedence) directories, matching XDG masking.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn discover_desktop_app(data_dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for base in data_dirs {
        let root = base.join("applications");
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        let mut visited: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        collect_desktop_entries(&root, &root, &mut files, &mut visited);
        files.sort(); // deterministic order by desktop-file ID within a directory
        for (id, path) in files {
            if !seen_ids.insert(id) {
                continue; // shadowed by a higher-precedence entry with the same ID
            }
            let Some(contents) = read_desktop_entry(&path) else {
                continue;
            };
            if let Some(target) =
                clawcrew_desktop_exec(&contents).and_then(|cmd| resolve_executable(&cmd))
            {
                return Some(target);
            }
        }
    }
    None
}

/// Discover an installed companion app on Linux that is not on `PATH`, such as
/// an AppImage registered in the application menu. Reads the `Exec` target from
/// a ClawCrew XDG desktop entry (honouring `$XDG_DATA_HOME`/`$XDG_DATA_DIRS`
/// precedence), then falls back to scanning common AppImage install locations.
/// Returns the launchable binary/AppImage path.
#[cfg(all(feature = "agent-runtime", target_os = "linux"))]
fn find_linux_desktop_app() -> Option<PathBuf> {
    let home = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf());

    // XDG application dirs in precedence order: $XDG_DATA_HOME first, then each
    // $XDG_DATA_DIRS entry. Unset or empty falls back to the spec defaults. Per
    // the Base Directory Specification a relative value is invalid and must be
    // ignored, so it is never searched from the process working directory.
    let mut data_dirs: Vec<PathBuf> = Vec::new();
    match std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        Some(v) => data_dirs.push(v),
        None => {
            if let Some(home) = &home {
                data_dirs.push(home.join(".local/share"));
            }
        }
    }
    let extra = std::env::var_os("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    for dir in extra.split(':').filter(|s| !s.is_empty()) {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            data_dirs.push(path);
        }
    }

    if let Some(target) = discover_desktop_app(&data_dirs) {
        return Some(target);
    }

    // Fall back to scanning common AppImage locations for a ClawCrew image that
    // was made executable but never registered on PATH. `read_dir` order is
    // unspecified, so collect every match and pick deterministically: within
    // a directory the lexicographically greatest file name (so a higher version
    // like `ClawCrew-2...` is preferred over `ClawCrew-1...`); earlier
    // directories in the list keep priority.
    if let Some(home) = &home {
        for dir in [home.join("Applications"), home.join(".local/bin")] {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut matches: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    is_clawcrew_appimage_name(name) && is_executable(path)
                })
                .collect();
            if !matches.is_empty() {
                matches.sort();
                return matches.pop();
            }
        }
    }

    None
}

#[allow(clippy::too_many_lines)]
async fn async_main_inner(command: clap::Command) -> Result<()> {
    // Install default crypto model_provider for Rustls TLS.
    // This prevents the error: "could not automatically determine the process-level CryptoProvider"
    // when both aws-lc-rs and ring features are available (or neither is explicitly selected).
    #[cfg(feature = "agent-runtime")]
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!(
            "{}",
            ta(
                "cli-warn-crypto-provider",
                &[("err", &format!("{e:?}"))],
                "Warning: Failed to install default crypto provider"
            )
        );
    }

    let cmd = apply_i18n_to_command(command);

    if std::env::args_os().len() <= 1 {
        return print_no_command_help(cmd);
    }

    let cli = Cli::from_arg_matches(&cmd.get_matches()).map_err(|e| e.exit())?;

    if let Some(config_dir) = &cli.config_dir
        && config_dir.trim().is_empty()
    {
        bail!("--config-dir cannot be empty");
    }

    #[cfg(feature = "agent-runtime")]
    crate::i18n::init(&crate::i18n::detect_locale());

    // Completions must remain stdout-only and should not load config or initialize logging.
    // This avoids warnings/log lines corrupting sourced completion scripts.
    if let Commands::Completions { shell } = &cli.command {
        let mut stdout = std::io::stdout().lock();
        write_shell_completion(*shell, &mut stdout)?;
        return Ok(());
    }

    // Docs-pipeline subcommands: stdout-only, no config load, no logging init.
    match &cli.command {
        Commands::MarkdownHelp => {
            clap_markdown::print_help_markdown::<Cli>();
            return Ok(());
        }
        Commands::MarkdownSchema => {
            #[cfg(feature = "schema-export")]
            {
                let schema = schemars::schema_for!(config::Config);
                print!(
                    "{}",
                    clawcrew_config::schema_markdown::generate(&schema.to_value())
                );
                return Ok(());
            }
            #[cfg(not(feature = "schema-export"))]
            anyhow::bail!("clawcrew was built without the 'schema-export' feature");
        }
        _ => {}
    }

    let default_floor = match &cli.command {
        Commands::Daemon {
            ephemeral: true, ..
        } => "debug",
        Commands::Acp { .. } | Commands::Agent { message: None, .. } => "warn",
        _ => "info",
    };

    // The explicit flag wins over RUST_LOG; without a flag the
    // subscriber honours RUST_LOG and falls back to this default.
    // matrix suppression is appended in both flag and default paths.
    let recording_filter = cli.log_level.map(|level| {
        format!(
            "{},matrix_sdk=warn,matrix_sdk_base=warn,matrix_sdk_crypto=warn",
            level.as_directive()
        )
    });
    let default_filter =
        format!("{default_floor},matrix_sdk=warn,matrix_sdk_base=warn,matrix_sdk_crypto=warn");

    clawcrew_log::install_global_subscriber(
        recording_filter.as_deref(),
        &default_filter,
        cli.verbose,
    );

    #[cfg(feature = "agent-runtime")]
    if let Commands::Onboard {
        section,
        quick,
        cli: use_cli,
        tui: _,
        force,
        reinit,
        api_key,
        model_provider,
        model,
        memory,
        channels_only,
        providers_only,
        memory_only,
        hardware_only,
        tunnel_only,
    } = &cli.command
    {
        let any_legacy_flag = section.is_some()
            || *quick
            || *use_cli
            || *force
            || *reinit
            || api_key.is_some()
            || model_provider.is_some()
            || model.is_some()
            || memory.is_some()
            || *channels_only
            || *providers_only
            || *memory_only
            || *hardware_only
            || *tunnel_only;
        if any_legacy_flag {
            eprintln!(
                "error: `clawcrew onboard` is deprecated and its flags no longer apply. \
                 Use `clawcrew quickstart` to create a new agent, or `clawcrew config set <path>=<value>` \
                 for headless updates."
            );
            std::process::exit(2);
        }
        eprintln!(
            "{}",
            t(
                "cli-onboard-deprecated",
                "`clawcrew onboard` is deprecated — use `clawcrew quickstart`."
            )
        );
        return Ok(());
    }

    #[cfg(feature = "agent-runtime")]
    if let Commands::Service {
        service_command: ServiceCommands::RunLaunchdDaemon,
        ..
    } = &cli.command
    {
        let config_dir = cli
            .config_dir
            .as_deref()
            .map(std::path::Path::new)
            .context("launchd runner requires --config-dir")?;
        return service::run_launchd_daemon(config_dir).await;
    }

    #[cfg(feature = "agent-runtime")]
    if let Commands::Service {
        service_command: ServiceCommands::RunDesktopDaemon { port },
        ..
    } = &cli.command
    {
        return service::run_desktop_daemon(*port).await;
    }

    #[cfg(feature = "agent-runtime")]
    if let Commands::Service {
        service_command: ServiceCommands::RunOpenrcLogWriter { stream },
        ..
    } = &cli.command
    {
        return service::run_openrc_log_writer(matches!(stream, ServiceLogStream::Stderr));
    }

    // All other commands need config loaded first
    let mut config = Box::pin(Config::load_or_init()).await?;
    let running_executable =
        running_executable_for_remediation().map(|path| path.display().to_string());
    for section in config
        .degraded_sections
        .iter()
        .chain(config.degraded_security.iter())
    {
        let path = config.config_path.display().to_string();
        let warning = if let Some(executable) = running_executable.as_deref() {
            let fallback = format!(
                "warning: config section `{section}` in {path} is malformed and was reset to \
                 defaults for this run. Values in that section are NOT in effect. Use the \
                 running executable at `{executable}` with `config migrate` to see the parse \
                 error, then repair the file."
            );
            ta(
                "cli-config-section-degraded-executable",
                &[
                    ("section", section),
                    ("path", &path),
                    ("executable", executable),
                ],
                &fallback,
            )
        } else {
            format!(
                "warning: config section `{section}` in {path} is malformed and was reset to \
                 defaults for this run. Values in that section are NOT in effect. The running \
                 executable path could not be resolved; repair the file through a daemon-owned \
                 config surface instead of an unqualified PATH command."
            )
        };
        eprintln!("{warning}");
    }
    for section in &config.retired_wati_config_sections {
        let fallback = format!(
            "warning: retired WATI channel config section '{section}' is ignored because WATI support was removed. Migrate to '[channels.whatsapp.<alias>]' using the Cloud API or WhatsApp Web, then revoke the unused WATI API token."
        );
        eprintln!(
            "{}",
            ta(
                "cli-config-section-retired-wati",
                &[("section", section)],
                &fallback,
            )
        );
    }
    if config.retired_node_transport_config {
        eprintln!(
            "{}",
            t(
                "cli-config-section-retired-node-transport",
                "warning: retired `[node_transport]` config is ignored because the legacy HMAC node transport was removed. Delete the section from config.toml."
            )
        );
    }
    #[cfg(feature = "agent-runtime")]
    observability::runtime_trace::init_from_config(&config.observability, &config.data_dir);
    // Must follow the trace sink init above, or the record has no destination.
    // The daemon reload arm calls the same helper against its reloaded config.
    #[cfg(feature = "agent-runtime")]
    warn_verifiable_intent_withheld(&config);
    #[cfg(feature = "agent-runtime")]
    if config.security.otp.enabled {
        let config_dir = config
            .config_path
            .parent()
            .context("Config path must have a parent directory")?;
        let store = security::SecretStore::new(config_dir, config.secrets.encrypt);
        let (_validator, enrollment_uri) =
            security::OtpValidator::from_config(&config.security.otp, config_dir, &store)?;
        if let Some(uri) = enrollment_uri {
            println!(
                "{}",
                t(
                    "cli-otp-initialized",
                    "Initialized OTP secret for ClawCrew."
                )
            );
            println!(
                "{}",
                ta("cli-otp-enrollment-uri", &[("uri", &uri)], "Enrollment URI")
            );
        }
    }

    #[cfg(not(feature = "agent-runtime"))]
    {
        // Kernel-only mode: minimal CLI agent without channels/tools/gateway
        match cli.command {
            Commands::Agent {
                agent: agent_alias,
                message,
                model_provider,
                model,
                temperature,
                ..
            } => {
                if config.agent(&agent_alias).is_none() {
                    anyhow::bail!(
                        "`clawcrew agent --agent {agent_alias}` is not configured (no [agents.{agent_alias}] entry)"
                    );
                }
                let agent_entry = config.model_provider_for_agent(&agent_alias);
                let final_temperature = temperature
                    .unwrap_or_else(|| agent_entry.and_then(|e| e.temperature).unwrap_or(0.7));
                if let Some(p) = &model_provider {
                    // Parse --model-provider as "type.alias" or bare "type" (use agent alias as alias name).
                    let (type_key, alias_key) =
                        p.split_once('.').unwrap_or((p.as_str(), &agent_alias));
                    let entry = config
                        .providers
                        .models
                        .ensure(type_key, alias_key)
                        .ok_or_else(|| {
                            ::clawcrew_log::record!(
                                WARN,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Reject
                                )
                                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                                .with_attrs(::serde_json::json!({"family": type_key})),
                                "ask CLI refused: --model-provider names an unknown family"
                            );
                            anyhow::Error::msg(format!(
                                "Unknown model_provider family: {type_key}. \
                             Configure a provider via `clawcrew quickstart` or the /config editor."
                            ))
                        })?;
                    if let Some(m) = &model {
                        entry.model = Some(m.clone());
                    }
                    entry.temperature = Some(final_temperature);
                    // Update the agent's model_provider to point to the override
                    if let Some(agent_cfg) = config.agents.get_mut(&agent_alias) {
                        agent_cfg.model_provider = format!("{type_key}.{alias_key}").into();
                    }
                } else if config.model_provider_for_agent(&agent_alias).is_none() {
                    anyhow::bail!(
                        "No model model_provider configured for agent {agent_alias}. \
                         Pass --model-provider <type> or run `clawcrew quickstart` to configure one."
                    );
                }

                let (provider_name, resolved_entry) = config
                    .resolved_model_provider_for_agent(&agent_alias)
                    .map(|(ty, _alias, entry)| (ty, Some(entry)))
                    .unwrap_or(("openai", None));
                let model_provider = clawcrew::providers::create_model_provider(
                    provider_name,
                    resolved_entry.and_then(|e| e.api_key.as_deref()),
                )?;
                let model_name = resolved_entry
                    .and_then(|e| e.model.as_deref())
                    .unwrap_or("default");
                match message {
                    Some(msg) => {
                        let response =
                            clawcrew_providers::ProviderDispatch::from_ref(&*model_provider)
                                .simple_chat(&msg, model_name, Some(final_temperature))
                                .await?;
                        println!("{response}");
                    }
                    None => {
                        loop {
                            eprint!("> ");
                            let line = {
                                let stdin = std::io::stdin().lock();
                                match read_capped_line(stdin, STDIN_LINE_CAP) {
                                    Ok(CappedLine::Eof) => break,
                                    Ok(CappedLine::Line(s)) => s,
                                    Ok(CappedLine::Truncated) => {
                                        // i18n-exempt: no-runtime fallback lacks the Fluent catalogue.
                                        eprintln!(
                                            "\nWarning: input line exceeds {} bytes and was discarded.",
                                            STDIN_LINE_CAP
                                        );
                                        continue;
                                    }
                                    Err(e) => {
                                        // i18n-exempt: no-runtime fallback lacks the Fluent catalogue.
                                        eprintln!("\nError reading input: {e}\n");
                                        break;
                                    }
                                }
                            };
                            let response =
                                clawcrew_providers::ProviderDispatch::from_ref(&*model_provider)
                                    .simple_chat(line.trim(), model_name, Some(final_temperature))
                                    .await?;
                            println!("{response}");
                        }
                    }
                }
                return Ok(());
            }
            Commands::Completions { .. } | Commands::MarkdownHelp | Commands::MarkdownSchema => {
                anyhow::bail!("documentation command was not handled before runtime dispatch")
            }
            Commands::Props { props_command } => {
                let DeprecatedPropsCommands::Any(args) = props_command;
                drop(args);
                anyhow::bail!(
                    "`clawcrew props` has been renamed to `clawcrew config`. \
                     Replace `props` with `config` in your command and try again."
                );
            }
            _ => {
                anyhow::bail!(
                    "This command requires the full runtime. Rebuild with default features:\n  cargo build --release"
                );
            }
        }
    }

    #[cfg(feature = "agent-runtime")]
    {
        clawcrew_runtime::cron::scheduler::register_delivery_fn(Box::new(
            |config, channel, target, thread_id, output| {
                Box::pin(async move {
                    clawcrew_channels::orchestrator::deliver_announcement(
                        &config, &channel, &target, thread_id, &output,
                    )
                    .await
                })
            },
        ));
    }

    #[cfg(feature = "agent-runtime")]
    match cli.command {
        Commands::Onboard { .. }
        | Commands::Completions { .. }
        | Commands::MarkdownHelp
        | Commands::MarkdownSchema => {
            anyhow::bail!("pre-runtime command was not handled before runtime dispatch")
        }

        Commands::Quickstart {
            model_provider,
            model,
            api_key,
            agent,
        } => {
            Box::pin(run_quickstart_cli(model_provider, model, api_key, agent)).await?;
            Ok(())
        }

        Commands::Agent {
            agent: agent_alias,
            message,
            session_state_file,
            model_provider,
            model,
            temperature,
            peripheral,
        } => {
            let final_temperature: Option<f64> = temperature.or_else(|| {
                config
                    .model_provider_for_agent(&agent_alias)
                    .and_then(|e| e.temperature)
            });

            // Validate up-front: bail with a clear message if the alias
            // isn't configured. The runtime would error too, but this
            // catches typos before any subsystem spins up.
            if config.agent(&agent_alias).is_none() {
                anyhow::bail!(
                    "`clawcrew agent --agent {agent_alias}` is not configured (no [agents.{agent_alias}] entry)"
                );
            }

            // Wire CLI channel for interactive mode
            clawcrew_runtime::agent::loop_::register_cli_channel_fn(Box::new(|| {
                Box::new(clawcrew_channels::cli::CliChannel::new("cli"))
            }));

            // Wire peripheral tools (gpio_read/gpio_write etc.) for `clawcrew agent`.
            // Mirrors the registration done for the daemon command.
            #[cfg(feature = "hardware")]
            clawcrew_runtime::agent::loop_::register_peripheral_tools_fn(Box::new(|config| {
                Box::pin(async move {
                    clawcrew_hardware::peripherals::create_peripheral_tools(&config).await
                })
            }));

            // Register channel map factory for late-bound tool handle population.
            clawcrew_runtime::agent::loop_::register_channel_map_fn(Box::new({
                let config_clone = config.clone();
                move || clawcrew_channels::orchestrator::build_channel_map(&config_clone)
            }));

            Box::pin(agent::run(
                config,
                &agent_alias,
                message,
                model_provider,
                model,
                final_temperature,
                peripheral,
                true,
                session_state_file,
                None,
                clawcrew_api::ingress::TurnOrigin::Interactive,
                clawcrew_runtime::agent::loop_::AgentRunOverrides::default(),
            ))
            .await
            .map(|_| ())
        }

        Commands::Acp {
            agent,
            max_sessions,
            session_timeout,
        } => {
            #[cfg(feature = "channel-acp-server")]
            {
                let mut acp_config = channels::acp_server::AcpServerConfig {
                    max_sessions: config.acp.max_sessions,
                    session_timeout_secs: config.acp.session_timeout_secs,
                };
                if let Some(max) = max_sessions {
                    acp_config.max_sessions = max;
                }
                if let Some(timeout) = session_timeout {
                    acp_config.session_timeout_secs = timeout;
                }
                let store =
                    clawcrew_infra::acp_session_store::AcpSessionStore::new(&config.data_dir)
                        .map(std::sync::Arc::new)
                        .inspect_err(|e| {
                            ::clawcrew_log::record!(
                                WARN,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                .with_attrs(::serde_json::json!({"error": e.to_string()})),
                                "Failed to open ACP session store"
                            );
                        })
                        .ok();
                let server = if let Some(store) = store {
                    channels::acp_server::AcpServer::new_with_store(config, acp_config, store)
                } else {
                    channels::acp_server::AcpServer::new(config, acp_config)
                }
                .with_connection_default_agent(agent);
                std::sync::Arc::new(server).run().await
            }
            #[cfg(not(feature = "channel-acp-server"))]
            {
                let _ = (agent, max_sessions, session_timeout);
                anyhow::bail!("ACP server requires the `channel-acp-server` feature")
            }
        }

        Commands::Gateway { gateway_command } => {
            match gateway_command {
                Some(clawcrew::GatewayCommands::Restart {
                    port,
                    host,
                    allow_degraded_security,
                }) => {
                    let _nag = gate_security_posture(&config, allow_degraded_security)?;
                    let (port, host) = resolve_gateway_addr(&config, port, host);
                    let addr = format!("{host}:{port}");
                    ::clawcrew_log::record!(
                        INFO,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_attrs(::serde_json::json!({"addr": addr})),
                        "🔄 Restarting ClawCrew Gateway on"
                    );

                    // Try to gracefully shutdown existing gateway via admin endpoint
                    match shutdown_gateway(&host, port, config.gateway.path_prefix.as_deref()).await
                    {
                        Ok(()) => {
                            ::clawcrew_log::record!(
                                INFO,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_attrs(::serde_json::json!({"addr": addr})),
                                "✓ Existing gateway on shut down gracefully"
                            );
                            // Poll until the port is free (connection refused) or timeout
                            let deadline =
                                tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
                            loop {
                                match tokio::net::TcpStream::connect(&addr).await {
                                    Err(_) => break, // port is free
                                    Ok(_) if tokio::time::Instant::now() >= deadline => {
                                        ::clawcrew_log::record!(
                                            WARN,
                                            ::clawcrew_log::Event::new(
                                                module_path!(),
                                                ::clawcrew_log::Action::Note
                                            )
                                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                            .with_attrs(::serde_json::json!({"port": port})),
                                            "Timed out waiting for port to be released"
                                        );
                                        break;
                                    }
                                    Ok(_) => {
                                        tokio::time::sleep(tokio::time::Duration::from_millis(50))
                                            .await;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            ::clawcrew_log::record!(
                                INFO,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                                "   No existing gateway to shut down"
                            );
                        }
                    }

                    log_gateway_start(&host, port);
                    Box::pin(run_gateway_if_enabled(&host, port, config, None)).await
                }
                Some(clawcrew::GatewayCommands::GetPaircode {
                    new,
                    rotate,
                    rotate_device,
                    port,
                    host,
                }) => {
                    let (port, host) = resolve_gateway_addr(&config, port, host);
                    let endpoint = format!("{host}:{port}");

                    let action = if rotate {
                        PaircodeAction::RotateAll
                    } else if let Some(id) = rotate_device {
                        PaircodeAction::RotateDevice(id)
                    } else if new {
                        PaircodeAction::AddClient
                    } else {
                        PaircodeAction::Show
                    };
                    let rotating = action.is_rotation();

                    match fetch_paircode(
                        &host,
                        port,
                        config.gateway.path_prefix.as_deref(),
                        &action,
                    )
                    .await
                    {
                        Ok(PaircodeResult::Code { code, message }) => {
                            println!(
                                "{}",
                                t("cli-pairing-enabled", "🔐 Gateway pairing is enabled.")
                            );
                            println!();
                            if let Some(message) = message.as_deref()
                                && rotating
                            {
                                println!("  ✅ {message}");
                                println!();
                            }
                            println!("  ┌──────────────┐");
                            println!("  │  {code}  │");
                            println!("  └──────────────┘");
                            println!();
                            println!(
                                "{}",
                                t(
                                    "cli-pairing-use-code",
                                    "  Use this one-time code to pair a new device:"
                                )
                            );
                            println!(
                                "{}",
                                ta(
                                    "cli-pairing-post",
                                    &[("code", &code)],
                                    "POST /pair with header X-Pairing-Code"
                                )
                            );
                        }
                        Ok(PaircodeResult::NoCode { message }) => {
                            println!(
                                "{}",
                                paircode_no_code_message(
                                    &host,
                                    port,
                                    &config.gateway.host,
                                    config.gateway.port,
                                    &action,
                                    config.gateway.require_pairing,
                                    message.as_deref(),
                                )
                            );
                        }
                        Err(e) => {
                            println!(
                                "{}",
                                ta(
                                    "cli-pairing-fetch-failed",
                                    &[("endpoint", &endpoint)],
                                    format!(
                                        "❌ Failed to fetch pairing code from gateway at {endpoint}"
                                    ),
                                )
                            );
                            println!(
                                "{}",
                                ta("cli-error-label", &[("err", &e.to_string())], "Error")
                            );
                            println!();
                            println!(
                                "{}",
                                t(
                                    "cli-gateway-running-q",
                                    "   Is the gateway running? Start it with:"
                                )
                            );
                            println!("     clawcrew gateway start"); // i18n-exempt: literal command/identifier example
                        }
                    }
                    Ok(())
                }
                Some(clawcrew::GatewayCommands::Start {
                    port,
                    host,
                    allow_degraded_security,
                }) => {
                    let _nag = gate_security_posture(&config, allow_degraded_security)?;
                    let (port, host) = resolve_gateway_addr(&config, port, host);
                    log_gateway_start(&host, port);
                    Box::pin(run_gateway_if_enabled(&host, port, config, None)).await
                }
                None => {
                    // Bare `clawcrew gateway` has no flag, so degraded security
                    // is never auto-allowed here — fail closed.
                    let _nag = gate_security_posture(&config, false)?;
                    let port = config.gateway.port;
                    let host = config.gateway.host.clone();
                    log_gateway_start(&host, port);
                    Box::pin(run_gateway_if_enabled(&host, port, config, None)).await
                }
            }
        }

        Commands::Daemon {
            port,
            host,
            ephemeral,
            allow_degraded_security,
        } => {
            // Fail closed before any setup work: refuse to serve with a
            // degraded security posture unless explicitly allowed. This branch
            // never spawns the nag (the `!allow` path only bails); the nag is
            // managed per reload-iteration in the loop below.
            if !config.degraded_security.is_empty() && !allow_degraded_security {
                gate_security_posture(&config, allow_degraded_security)?;
            }
            if let Ok(exe) = std::env::current_exe() {
                let under_home = directories::UserDirs::new()
                    .map(|u| u.home_dir().to_path_buf())
                    .is_some_and(|home| exe.starts_with(&home));
                if under_home {
                    let install_hint = if cfg!(windows) {
                        "Consider installing to a system-wide location (e.g. C:\\Program Files\\ClawCrew) for service use."
                    } else if cfg!(target_os = "macos") {
                        "Consider installing to /usr/local/bin or /opt/homebrew/bin for system-wide service."
                    } else {
                        "Consider installing to /usr/local/bin for system-wide service."
                    };
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                        &format!(
                            "Daemon running from user home directory: {}. {install_hint}",
                            exe.display()
                        )
                    );
                }
            }
            let port = port.unwrap_or(config.gateway.port);
            let host = host.unwrap_or_else(|| config.gateway.host.clone());
            if port == 0 {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({"host": host})),
                    "🧠 Starting ClawCrew Daemon on (random port)"
                );
            } else {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({"host": host, "port": port})),
                    "🧠 Starting ClawCrew Daemon on"
                );
            }

            #[cfg(target_os = "linux")]
            {
                use clawcrew_config::schema::SandboxBackend;
                // Any enabled agent whose risk_profile uses the docker
                // sandbox triggers the warning — we just need to know
                // *some* agent is using it.
                let sandbox_docker = config
                    .agents
                    .iter()
                    .filter(|(_, a)| a.enabled)
                    .filter_map(|(alias, _)| config.risk_profile_for_agent(alias))
                    .any(|p| matches!(p.sandbox_config().backend, SandboxBackend::Docker));
                let runtime_docker_mem = config.runtime.kind
                    == clawcrew_config::schema::RuntimeKind::Docker
                    && config
                        .runtime
                        .docker
                        .memory_limit_mb
                        .is_some_and(|mb| mb > 0);
                if (sandbox_docker || runtime_docker_mem)
                    && !clawcrew_runtime::security::linux_memcg_available()
                {
                    let which = match (sandbox_docker, runtime_docker_mem) {
                        (true, true) => {
                            "security.sandbox.backend = \"docker\" and runtime.kind = \"docker\""
                        }
                        (true, false) => "security.sandbox.backend = \"docker\"",
                        _ => "runtime.kind = \"docker\"",
                    };
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"which": which})),
                        "Docker memory limits are configured but the Linux kernel has no memcg support. Affected config: . Consequence: --memory limits are silently ignored; agents can OOM the host. Fix: add 'cgroup_memory=1 cgroup_enable=memory' to /boot/firmware/cmdline.txt (Raspberry Pi) or enable CONFIG_MEMCG in your kernel, then reboot."
                    );
                }
            }

            // Wire CLI channel for interactive mode
            #[cfg(feature = "agent-runtime")]
            clawcrew_runtime::agent::loop_::register_cli_channel_fn(Box::new(|| {
                Box::new(clawcrew_channels::cli::CliChannel::new("cli"))
            }));

            // Wire peripheral tools from clawcrew-hardware
            #[cfg(feature = "hardware")]
            clawcrew_runtime::agent::loop_::register_peripheral_tools_fn(Box::new(|config| {
                Box::pin(async move {
                    clawcrew_hardware::peripherals::create_peripheral_tools(&config).await
                })
            }));

            // Cron delivery is registered earlier (before the command match)
            // so it works for both `daemon` and `gateway start`.

            let canvas_store = clawcrew_runtime::tools::CanvasStore::new();
            let canvas_store_for_gateway = canvas_store.clone();
            let canvas_store_for_channels = canvas_store.clone();

            // Capture the launch command now, before any in-app upgrade can
            // swap the binary on disk (after which `current_exe()` resolves to a
            // "(deleted)" path on Linux). Used by the post-loop self-respawn.
            clawcrew_runtime::restart::record_launch();

            // Reload loop. `daemon::run` returns DaemonExit::Shutdown on
            // SIGINT/SIGTERM (loop ends) or DaemonExit::Reload after a
            // `POST /admin/reload` request (loop re-reads config from disk and
            // re-runs). The PID stays the same across reloads — only the
            // in-process subsystems tear down + re-instantiate.
            let mut current_config = config;
            // Nag task for the degraded-security warning, scoped to the
            // current config. Re-evaluated each reload iteration so a repaired
            // config stops the warning and a freshly-degraded one starts it.
            let mut degraded_nag: Option<tokio::task::JoinHandle<()>> =
                gate_security_posture(&current_config, allow_degraded_security)?;
            let startup_feedback_enabled = !cli.verbose;
            loop {
                if startup_feedback_enabled && daemon::stderr_is_interactive_foreground() {
                    let mut stderr = std::io::stderr().lock();
                    let _ = daemon::echo_daemon_starting_to_terminal(&mut stderr);
                }

                // Per-iteration clones so the subsystem closures (which
                // `move`-capture) don't consume the outer bindings on the
                // first iteration; reload would otherwise see a moved value.
                let canvas_store_for_gateway = canvas_store_for_gateway.clone();
                let canvas_store_for_channels = canvas_store_for_channels.clone();
                let mut registry = daemon::DaemonRegistry::new();
                #[cfg(feature = "gateway")]
                let plugin_webhooks = Arc::new(clawcrew_api::webhook::PluginWebhookRegistry::new());
                #[cfg(feature = "gateway")]
                let channel_plugin_webhooks = Some(Arc::clone(&plugin_webhooks));
                #[cfg(not(feature = "gateway"))]
                let channel_plugin_webhooks: Option<
                    Arc<clawcrew_api::webhook::PluginWebhookRegistry>,
                > = None;

                // SOP loading is gated on `runtime_enabled()`: `sops_dir` is unset
                // (or empty) by default, so SOP runtime behavior is off until an
                // operator opts in by setting a directory.
                let (sop_engine, sop_audit) = if current_config.sop.runtime_enabled() {
                    let mem: Arc<dyn clawcrew_memory::Memory> = Arc::from(
                        clawcrew_memory::create_memory_from_config(&current_config, None)?,
                    );
                    let sop_adapters = build_sop_adapters(&current_config);
                    let (engine, audit) = clawcrew_runtime::sop::build_sop_engine(
                        current_config.sop.clone(),
                        &current_config.data_dir,
                        &current_config.install_root_dir(),
                        mem,
                        sop_adapters,
                    );
                    (Some(engine), Some(audit))
                } else {
                    (None, None)
                };

                // EPIC A1 + SOP cron: drive periodic maintenance and cron
                // triggers against the shared engine for this daemon iteration.
                let sop_maintenance = spawn_sop_maintenance(
                    sop_engine.as_ref(),
                    sop_audit.as_ref(),
                    current_config.sop.maintenance_interval_secs,
                );

                #[cfg(feature = "gateway")]
                registry.register_gateway(Box::new({
                    let sop_e = sop_engine.clone();
                    let sop_a = sop_audit.clone();
                    let plugin_webhooks = Arc::clone(&plugin_webhooks);
                    move |host, port, config, tx, reload_controls, tui_registry, ready_tx| {
                        let canvas_store = canvas_store_for_gateway.clone();
                        let sop_engine = sop_e.clone();
                        let sop_audit = sop_a.clone();
                        let plugin_webhooks = Arc::clone(&plugin_webhooks);
                        Box::pin(async move {
                            Box::pin(clawcrew_gateway::run_gateway_with_plugin_webhooks(
                                &host,
                                port,
                                config,
                                tx,
                                reload_controls,
                                tui_registry,
                                Some(canvas_store),
                                sop_engine,
                                sop_audit,
                                clawcrew_gateway::GatewaySupervision::new(
                                    ready_tx,
                                    plugin_webhooks,
                                ),
                            ))
                            .await
                        })
                    }
                }));

                registry.register_channels(Box::new({
                    let sop_e = sop_engine.clone();
                    let sop_a = sop_audit.clone();
                    let plugin_webhooks = channel_plugin_webhooks.clone();
                    move |config, cancel| {
                        let canvas_store = canvas_store_for_channels.clone();
                        let sop_engine = sop_e.clone();
                        let sop_audit = sop_a.clone();
                        let plugin_webhooks = plugin_webhooks.clone();
                        Box::pin(async move {
                            let channels = clawcrew_channels::orchestrator::start_channels_with_plugin_webhooks(
                                config,
                                Some(canvas_store),
                                cancel,
                                sop_engine,
                                sop_audit,
                                plugin_webhooks,
                            );
                            Box::pin(channels).await
                        })
                    }
                }));

                #[cfg(feature = "channel-mqtt")]
                registry.register_mqtt(Box::new({
                    let engine = sop_engine.clone();
                    let audit = sop_audit.clone();
                    move |mqtt_config| {
                        let engine = engine.clone();
                        let audit = audit.clone();
                        Box::pin(async move {
                            if let (Some(engine), Some(audit)) = (engine, audit) {
                                clawcrew_channels::orchestrator::mqtt::run_mqtt_sop_listener(
                                    &mqtt_config,
                                    engine,
                                    audit,
                                )
                                .await
                            } else {
                                // No SOPs directory configured — this is a valid
                                // user state, not a misconfiguration. Skip the
                                // listener gracefully.
                                ::clawcrew_log::record!(
                                    INFO,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Skip
                                    ),
                                    "MQTT SOP listener skipped — no SOPs directory configured"
                                );
                                Ok(())
                            }
                        })
                    }
                }));

                registry.register_socket(Box::new(|ctx, cancel, client_count, ready_tx| {
                    Box::pin(async move {
                        clawcrew_runtime::rpc::local::run_local_listener(
                            ctx,
                            cancel,
                            client_count,
                            ready_tx,
                        )
                        .await
                    })
                }));

                registry.register_wss(Box::new(|ctx, cancel, client_count| {
                    Box::pin(async move {
                        let (wss_cfg, data_dir) = {
                            let cfg = ctx.config.read();
                            (cfg.wss.clone(), cfg.data_dir.clone())
                        };
                        if !wss_cfg.enabled {
                            // WSS disabled — park until cancelled.
                            cancel.cancelled().await;
                            return Ok(());
                        }
                        // The remote WSS plane is ALWAYS mutually authenticated; there
                        // is no server-only / plaintext fallback. In auto-CA mode the
                        // same generated CA verifies client certificates, and any
                        // configured pin allowlist remains enforced.
                        let (byo_ca, pinned) =
                            resolve_wss_client_auth(wss_cfg.client_auth.as_ref())?;
                        // Bring-your-own mTLS when an operator CA is configured;
                        // otherwise auto-generate a per-daemon CA + server certificate
                        // under the data dir (secure by default, zero config).
                        let (cert_path, key_path, ca_cert_path) = match byo_ca {
                            Some(ca_cert_path) => {
                                if wss_cfg.cert_path.is_empty() || wss_cfg.key_path.is_empty() {
                                    anyhow::bail!(
                                        "[wss.client_auth].ca_cert_path is set (bring-your-own mTLS) \
                                         but [wss].cert_path/key_path are not. Provide the server \
                                         certificate and key, or clear ca_cert_path to auto-generate \
                                         the CA and server certificate."
                                    );
                                }
                                (wss_cfg.cert_path.clone(), wss_cfg.key_path.clone(), ca_cert_path)
                            }
                            None => {
                                // Generate (or reuse) the per-daemon CA + server
                                // cert. The CA key is encrypted at rest when a
                                // passphrase is configured (same source the
                                // enrollment + CLI read paths use), else 0600.
                                // [wss].sans adds the hostnames/IPs a remote client
                                // uses to reach the daemon to the server cert. The
                                // enrollment endpoint uses the same resolver so both
                                // TLS surfaces present matching daemon identities.
                                let server_sans = wss_server_sans(&wss_cfg);
                                let mats = clawcrew_tls::ensure_server_materials_protected(
                                    &data_dir.join("tls"),
                                    &server_sans,
                                    &ca_key_protection_from_env(),
                                )?;
                                (
                                    mats.server_cert_path.to_string_lossy().into_owned(),
                                    mats.server_key_path.to_string_lossy().into_owned(),
                                    mats.ca_cert_path.to_string_lossy().into_owned(),
                                )
                            }
                        };
                        // Connect-time revocation refusal (A5): default to the
                        // ledger-materialized list under <data_dir>/tls/revoked
                        // (the daemon rewrites it on every revoke), overridable by
                        // [wss.client_auth].crl_path.
                        // Resolve the effective CRL path exactly once, with the
                        // SAME normalization the ledger and operator CLI use
                        // (trim; blank means unset), and hand that one value to
                        // both the ledger and the TLS acceptor below. Selecting
                        // the raw string here let a whitespace spelling install
                        // no revocation verifier while the ledger materialized
                        // the default file - revocation must never be split or
                        // disabled by an accepted configuration spelling.
                        let crl_path =
                            clawcrew_runtime::security::cert_ledger::effective_revoked_list_path(
                                &data_dir,
                                wss_cfg.client_auth.as_ref().map(|c| c.crl_path.as_str()),
                            )
                            .to_string_lossy()
                            .into_owned();
                        // Materialize to the path the verifier will read,
                        // including a configured override. Skipping this when an
                        // override is set left `revoke-client-cert` writing to
                        // the default file while the handshake honoured a stale
                        // one, so a revoked cert kept authenticating.
                        {
                            let ledger =
                                clawcrew_runtime::security::cert_ledger::CertLedger::open_at(
                                    &data_dir,
                                    None,
                                    std::path::PathBuf::from(&crl_path),
                                )
                                .context(
                                    "open cert ledger before starting WSS revocation checks",
                                )?;
                            ledger.materialize_revocations().context(
                                "materialize cert revocations before starting WSS listener",
                            )?;
                        }
                        let tls_acceptor = clawcrew_runtime::rpc::wss::build_tls_acceptor(
                            &cert_path,
                            &key_path,
                            &ca_cert_path,
                            &pinned,
                            &crl_path,
                        )?;
                        let bind_addr: std::net::SocketAddr =
                            format!("{}:{}", wss_cfg.bind, wss_cfg.port).parse()?;
                        let wss_limits = clawcrew_runtime::rpc::wss::WssLimits {
                            max_pending_handshakes: wss_cfg.max_pending_handshakes,
                            handshake_timeout: std::time::Duration::from_secs(
                                wss_cfg.handshake_timeout_secs,
                            ),
                            max_sessions: wss_cfg.max_sessions,
                            max_sessions_per_client: wss_cfg.max_sessions_per_client,
                            incomplete_message_timeout: std::time::Duration::from_secs(
                                wss_cfg.incomplete_message_timeout_secs,
                            ),
                        };
                        clawcrew_runtime::rpc::wss::run_wss_listener(
                            ctx,
                            cancel,
                            client_count,
                            tls_acceptor,
                            bind_addr,
                            wss_limits,
                        )
                        .await
                    })
                }));

                // Shared between the relay bridge and the enrollment endpoint:
                // the bridge registers its enroll-dial source ports here so the
                // endpoint can classify those loopback connections as
                // relay-routed rather than direct (finding: relay enrollment
                // collapsed every client to the bridge's loopback identity, so
                // one hostile client's failures locked out all relay enrollees).
                let enroll_bridge_ports: clawcrew_runtime::enroll::BridgePortSet =
                    std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
                let enroll_bridge_ports_for_bridge = enroll_bridge_ports.clone();
                let enroll_bridge_ports_for_endpoint = enroll_bridge_ports.clone();
                // Relay bridge: keep an outbound connection to a nominated relay
                // so clients behind NAT can reach this daemon through it. The
                // relay forwards to the local WSS listener (loopback), where the
                // inner mTLS terminates; it never decrypts anything.
                registry.register_relay(Box::new(move |ctx, cancel, _client_count| {
                    let enroll_bridge_ports_for_bridge = enroll_bridge_ports_for_bridge.clone();
                    Box::pin(async move {
                        let (relay_cfg, wss_cfg, enroll_cfg, data_dir) = {
                            let cfg = ctx.config.read();
                            (
                                cfg.relay.clone(),
                                cfg.wss.clone(),
                                cfg.enroll.clone(),
                                cfg.data_dir.clone(),
                            )
                        };
                        if !relay_cfg.enabled {
                            cancel.cancelled().await;
                            return Ok(());
                        }
                        if !wss_cfg.enabled {
                            return Err(anyhow::Error::msg(
                                "[relay] is enabled but [wss] is not. The relay forwards clients to \
                                 the local WSS listener, so enable [wss] (it provides the mutually \
                                 authenticated plane the relay tunnels).",
                            ));
                        }
                        if relay_cfg.url.is_empty() {
                            return Err(anyhow::Error::msg(
                                "[relay] is enabled but relay.url is required.",
                            ));
                        }
                        // Persistent Ed25519 identity the relay binds the node-id to.
                        let signing_key_pkcs8 =
                            clawcrew_runtime::relay::ensure_signing_key(&data_dir)?;
                        // node_id is an unguessable 128-bit capability: auto-minted +
                        // persisted unless the operator pinned one in [relay].node_id.
                        let node_id = clawcrew_runtime::relay::ensure_node_id(
                            &data_dir,
                            &relay_cfg.node_id,
                        )?;
                        ::clawcrew_log::record!(
                            INFO,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note,
                            )
                            .with_attrs(::serde_json::json!({
                                "node_id": node_id,
                                "relay": relay_cfg.url,
                            })),
                            "relay bridge: node_id (give clients this as --relay-node)"
                        );
                        // Default the relay's expected cert name to its host:port host.
                        let relay_host = if relay_cfg.relay_host.is_empty() {
                            relay_cfg
                                .url
                                .rsplit_once(':')
                                .map(|(h, _)| h.to_string())
                                .unwrap_or_else(|| relay_cfg.url.clone())
                        } else {
                            relay_cfg.relay_host.clone()
                        };
                        // Rotation is permitted only for an auto-minted id (a
                        // pinned [relay].node_id is fixed).
                        let rotation_allowed = relay_cfg.node_id.trim().is_empty();
                        let node_id_rotation_days = relay_cfg.node_id_rotation_days;
                        let bridge_cfg = clawcrew_runtime::relay::RelayBridgeConfig {
                            relay_addr: relay_cfg.url,
                            relay_host,
                            node_id,
                            relay_token: Some(relay_cfg.token).filter(|t| !t.is_empty()),
                            local_wss_addr: format!("127.0.0.1:{}", wss_cfg.port),
                            local_enroll_addr: enroll_cfg
                                .enabled
                                .then(|| format!("127.0.0.1:{}", enroll_cfg.port)),
                            enroll_bridge_ports: Some(enroll_bridge_ports_for_bridge.clone()),
                            signing_key_pkcs8,
                            relay_ca_path: Some(relay_cfg.relay_ca_path)
                                .filter(|p| !p.is_empty()),
                            relay_insecure: relay_cfg.relay_insecure,
                            relay_tofu: relay_cfg.tofu,
                            outer_client_cert: Some(relay_cfg.outer_client_cert)
                                .filter(|p| !p.is_empty()),
                            outer_client_key: Some(relay_cfg.outer_client_key)
                                .filter(|p| !p.is_empty()),
                            max_conns: 256,
                            // Bridge-side OPEN-flood cap (A6): fast-reject beyond
                            // ~20 new conns/sec (burst 60) so an OPEN flood cannot
                            // force unbounded loopback mTLS handshakes.
                            open_burst: 60,
                            open_rate_per_sec: 20.0,
                            data_dir: data_dir.clone(),
                            node_id_rotation_days,
                            rotation_allowed,
                        };
                        clawcrew_runtime::relay::run_relay_bridge(bridge_cfg, cancel).await
                    })
                }));

                // Certificate enrollment endpoint: the bootstrap surface a
                // certless client reaches for its FIRST cert (server-auth TLS +
                // one-time pairing code, CSR-only). The daemon owns the CA, so
                // this works with no gateway. It is NOT the mTLS RPC plane.
                registry.register_enroll(Box::new(move |ctx, cancel, _client_count| {
                    let enroll_bridge_ports = enroll_bridge_ports_for_endpoint.clone();
                    Box::pin(async move {
                        let (
                            enroll_cfg,
                            wss_cfg,
                            relay_cfg,
                            data_dir,
                            startup_pairing_code_policy,
                        ) = {
                            let cfg = ctx.config.read();
                            (
                                cfg.enroll.clone(),
                                cfg.wss.clone(),
                                cfg.relay.clone(),
                                cfg.data_dir.clone(),
                                cfg.gateway.pairing_code,
                            )
                        };
                        if !enroll_cfg.enabled {
                            cancel.cancelled().await;
                            return Ok(());
                        }
                        if !wss_cfg.enabled {
                            return Err(anyhow::Error::msg(
                                "[enroll] is enabled but [wss] is not. Enrollment issues client \
                                 certificates for the mutually authenticated WSS plane; enable [wss].",
                            ));
                        }
                        // Issuance needs the daemon CA *private key*. Two
                        // bring-your-own forms exist:
                        //   1. BYO-CA with key (in-band): the operator drops
                        //      ca.crt + ca.key into <data_dir>/tls; the issuer
                        //      loads and signs against them (handled below by
                        //      ensure_server_materials_protected's load path).
                        //   2. BYO-CA without key (external CA): the WSS verifier
                        //      trusts an external CA cert whose key the daemon does
                        //      not hold. It cannot sign - fail closed: do not open
                        //      the endpoint (provision client certs out of band).
                        let byo_ca = wss_cfg
                            .client_auth
                            .as_ref()
                            .filter(|c| c.enabled)
                            .map(|c| !c.ca_cert_path.is_empty())
                            .unwrap_or(false);
                        if byo_ca {
                            ::clawcrew_log::record!(
                                WARN,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note,
                                ),
                                "enrollment endpoint disabled: a bring-your-own CA has no signing \
                                 key; provision client certs out of band"
                            );
                            cancel.cancelled().await;
                            return Ok(());
                        }
                        // Per-daemon CA + server cert. Loaded when the operator has
                        // provisioned their own ca.{crt,key} (BYO-CA with key),
                        // otherwise auto-generated (secure by default). Same
                        // passphrase source as the WSS gen + CLI read paths, so the
                        // on-disk CA-key form always matches.
                        let tls_dir = data_dir.join("tls");
                        let ca_provided =
                            tls_dir.join("ca.crt").exists() && tls_dir.join("ca.key").exists();
                        let protection = ca_key_protection_from_env();
                        let server_sans = wss_server_sans(&wss_cfg);
                        let mats = clawcrew_tls::ensure_server_materials_protected(
                            &tls_dir,
                            &server_sans,
                            &protection,
                        )?;
                        ::clawcrew_log::record!(
                            INFO,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note,
                            ),
                            if ca_provided {
                                "enrollment signing against an operator-provided CA \
                                 (<data_dir>/tls/ca.*)"
                            } else {
                                "enrollment signing against the auto-generated per-daemon CA"
                            }
                        );
                        let ca_cert_pem = std::fs::read_to_string(&mats.ca_cert_path)?;
                        let ca_key_pem =
                            clawcrew_tls::load_ca_key_pem(&mats.ca_key_path, &protection)?;
                        let ca_fingerprint = {
                            let ders =
                                clawcrew_tls::load_certs(&mats.ca_cert_path.to_string_lossy())?;
                            clawcrew_tls::cert_sha256_fingerprint(ders[0].as_ref())
                        };

                        // Server-authentication-only TLS (no client cert; this is
                        // the bootstrap surface, explicitly not the mTLS plane).
                        let acceptor =
                            clawcrew_tls::build_tls_acceptor(&clawcrew_tls::ServerConfigParams {
                                cert_path: mats.server_cert_path.to_string_lossy().into_owned(),
                                key_path: mats.server_key_path.to_string_lossy().into_owned(),
                                client_auth: None,
                            })?;

                        // Relay coordinates handed to the enrolled client (shared
                        // with the renew path). The pin (relay LEAF sha256) is
                        // sourced from the relay bridge's pin store when present.
                        let relay_profile =
                            clawcrew_runtime::enroll::relay_profile(&data_dir, &relay_cfg);

                        // One-time pairing code gates enrollment. Print it AND the
                        // CA-bound short-auth-string so the operator reads both to
                        // the client out of band (no blind trust-on-first-use).
                        let pairing = std::sync::Arc::new(clawcrew_config::pairing::PairingGuard::new(
                            true,
                            &[],
                            startup_pairing_code_policy,
                        ));
                        if let Some(code) = pairing.pairing_code() {
                            let sas = clawcrew_tls::enrollment_sas(&code, &ca_fingerprint);
                            let enroll_bind = enroll_cfg.bind.to_string();
                            let enroll_port = enroll_cfg.port.to_string();
                            println!();
                            println!(
                                "{}",
                                ta(
                                    "cli-enroll-endpoint-ready",
                                    &[("bind", &enroll_bind), ("port", &enroll_port)],
                                    "enrollment ready"
                                )
                            );
                            println!(
                                "{}",
                                t("cli-enroll-confirm-sas-line-1", "confirm SAS")
                            );
                            println!("{}", t("cli-enroll-confirm-sas-line-2", "match SAS"));
                            println!(
                                "{}",
                                ta("cli-enroll-pairing-code", &[("code", &code)], "code")
                            );
                            println!("{}", ta("cli-enroll-sas", &[("sas", &sas)], "SAS"));
                            println!();
                        }

                        // Reserved migration knob. Code-less enrollment needs a
                        // separate client trust anchor before certs can be cached.
                        let allow_unpaired_until = {
                            let s = enroll_cfg.allow_unpaired_enrollment.trim();
                            if !s.is_empty() {
                                anyhow::bail!(
                                    "[enroll].allow_unpaired_enrollment is reserved for a future \
                                     no-code enrollment flow and is not supported in this release. \
                                     Clear it and use the printed pairing code."
                                );
                            }
                            None
                        };

                        // The daemon's shared certificate audit logger, built
                        // once in `daemon::run` and handed to every certificate
                        // path through the RPC context. Enrollment must not
                        // build its own: a second logger over the same file
                        // recovers the same Merkle-chain tip as the renewal
                        // path and races it into duplicate sequence numbers,
                        // which makes `verify_chain` reject the trail.
                        let audit = ctx
                            .cert_audit
                            .clone()
                            .context(
                                "the enrollment endpoint requires the daemon's certificate \
                                 audit logger; it failed to initialize at startup (see the \
                                 startup error) and enrollment will not issue certificates \
                                 without an audit trail",
                            )?;
                        // Materialize revocations to the file the WSS verifier
                        // ACTUALLY reads - the same `[wss.client_auth].crl_path`
                        // resolution the acceptor above performs. Opening on the
                        // ledger default instead meant an enrollment-path
                        // revocation (including the undelivered sweep, which
                        // runs on this long-lived handle) rewrote
                        // `<data_dir>/tls/revoked` while the verifier kept
                        // reading an unchanged operator-managed file: revoked in
                        // SQLite, still accepted at the handshake.
                        let ledger = std::sync::Arc::new(
                            clawcrew_runtime::security::cert_ledger::CertLedger::open_at(
                                &data_dir,
                                Some(audit),
                                clawcrew_runtime::security::cert_ledger::effective_revoked_list_path(
                                    &data_dir,
                                    wss_cfg.client_auth.as_ref().map(|c| c.crl_path.as_str()),
                                ),
                            )?,
                        );

                        let bind_addr: std::net::SocketAddr =
                            format!("{}:{}", enroll_cfg.bind, enroll_cfg.port).parse()?;
                        let server = std::sync::Arc::new(clawcrew_runtime::enroll::EnrollServer {
                            bind_addr,
                            acceptor,
                            ca_cert_pem,
                            ca_key_pem,
                            ledger,
                            pairing,
                            pairing_code_policy: {
                                let config = ctx.config.clone();
                                std::sync::Arc::new(move || config.read().gateway.pairing_code)
                            },
                            static_client_pins_configured: wss_cfg
                                .client_auth
                                .as_ref()
                                .map(|auth| !auth.pinned_certs.is_empty())
                                .unwrap_or(false),
                            allow_unpaired_until,
                            relay_profile,
                            bridge_ports: Some(enroll_bridge_ports.clone()),
                            relay_attempt_bucket:
                                clawcrew_runtime::enroll::RelayAttemptBucket::default(),
                            paircode_admin_data_dir: Some(data_dir.clone()),
                        });
                        clawcrew_runtime::enroll::serve(server, cancel).await
                    })
                }));

                // Pass the shared SOP engine through the registry so
                // RpcContext (RPC/TUI agent sessions) can share it.
                registry.set_sop_engine(sop_engine, sop_audit);

                let exit = Box::pin(daemon::run(
                    current_config.clone(),
                    host.clone(),
                    port,
                    registry,
                    ephemeral,
                    startup_feedback_enabled,
                ))
                .await;
                if let Some(handle) = sop_maintenance {
                    handle.abort();
                }
                let exit = exit?;
                match exit {
                    daemon::DaemonExit::Shutdown => break,
                    daemon::DaemonExit::Reload => {
                        ::clawcrew_log::record!(
                            INFO,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            ),
                            "🔄 Daemon reload — re-reading config from disk"
                        );
                        current_config = Box::pin(Config::load_or_init()).await?;
                        #[cfg(feature = "agent-runtime")]
                        observability::runtime_trace::init_from_config(
                            &current_config.observability,
                            &current_config.data_dir,
                        );
                        // A reload applies config the process has not seen, so an
                        // operator who just enabled the section learns why the tool
                        // is still absent without having to restart.
                        #[cfg(feature = "agent-runtime")]
                        warn_verifiable_intent_withheld(&current_config);
                        if let Some(handle) = degraded_nag.take() {
                            handle.abort();
                        }
                        degraded_nag =
                            gate_security_posture(&current_config, allow_degraded_security)?;
                        // Continue loop: fresh subsystems with the new config.
                    }
                }
            }
            if let Some(handle) = degraded_nag.take() {
                handle.abort();
            }
            if clawcrew_runtime::restart::desktop_restart_requested() {
                std::process::exit(clawcrew_runtime::restart::DESKTOP_RESTART_EXIT_CODE);
            }
            // Bare-process auto-restart: the daemon has now torn down (the
            // gateway listener is released), so launch the upgraded binary as a
            // detached child before we exit. No-op unless an in-app upgrade
            // requested a self-respawn.
            clawcrew_runtime::restart::respawn_if_requested();
            Ok(())
        }

        Commands::Status { format } => {
            if format.as_deref() == Some("exit-code") {
                // Lightweight health probe for Docker HEALTHCHECK
                let port = config.gateway.port;
                let host = if config.gateway.host == "[::]" || config.gateway.host == "0.0.0.0" {
                    "127.0.0.1"
                } else {
                    &config.gateway.host
                };
                let url = format!("http://{}:{}/health", host, port);
                match reqwest::Client::new()
                    .get(&url)
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        std::process::exit(0);
                    }
                    _ => {
                        std::process::exit(1);
                    }
                }
            }
            println!("{}", t("cli-status-title", "🦀 ClawCrew Status"));
            println!();
            println!(
                "{}",
                ta(
                    "cli-status-version",
                    &[("v", env!("CARGO_PKG_VERSION"))],
                    "Version"
                )
            );
            println!(
                "{}",
                ta(
                    "cli-status-workspace",
                    &[("v", &config.data_dir.display().to_string())],
                    "Workspace"
                )
            );
            println!(
                "{}",
                ta(
                    "cli-status-config",
                    &[("v", &config.config_path.display().to_string())],
                    "Config"
                )
            );
            println!();
            let mut shown_provider = false;
            for (family, alias, entry) in config.providers.models.iter_entries() {
                let model = entry.model.as_deref().unwrap_or("(none)");
                if shown_provider {
                    println!(
                        "{}",
                        ta(
                            "cli-status-provider-indent",
                            &[("family", family), ("alias", alias)],
                            "ModelProvider"
                        )
                    );
                    println!("{}", ta("cli-status-model", &[("model", model)], "Model"));
                } else {
                    println!(
                        "{}",
                        ta(
                            "cli-status-provider",
                            &[("family", family), ("alias", alias)],
                            "ModelProvider"
                        )
                    );
                    println!("{}", ta("cli-status-model", &[("model", model)], "Model"));
                    shown_provider = true;
                }
            }
            if !shown_provider {
                println!(
                    "{}",
                    t(
                        "cli-status-provider-none",
                        "🤖 ModelProvider:      (none configured)"
                    )
                );
            }
            println!(
                "{}",
                ta(
                    "cli-status-observability",
                    &[("v", config.observability.backend.as_wire())],
                    "Observability"
                )
            );
            let trace_storage_mode = config.observability.log_persistence.as_wire().to_string();
            let trace_storage_path = config.observability.log_persistence_path.to_string();
            let trace_storage_fallback = format!(
                "🧾 Trace storage:  {} ({})",
                trace_storage_mode, trace_storage_path
            );
            println!(
                "{}",
                ta(
                    "cli-status-trace-storage",
                    &[("mode", &trace_storage_mode), ("path", &trace_storage_path),],
                    &trace_storage_fallback
                )
            );
            // Per-agent autonomy: each enabled agent picks its own
            // risk_profile, so list them rather than collapsing to one.
            let mut agent_aliases: Vec<&String> = config
                .agents
                .iter()
                .filter(|(_, a)| a.enabled)
                .map(|(alias, _)| alias)
                .collect();
            agent_aliases.sort();
            if agent_aliases.is_empty() {
                println!(
                    "{}",
                    t(
                        "cli-status-agents-none",
                        "🛡️  Agents:        (none configured)"
                    )
                );
            } else {
                let summary: Vec<String> = agent_aliases
                    .iter()
                    .map(|alias| match config.risk_profile_for_agent(alias) {
                        Some(p) => {
                            let level = format!("{:?}", p.level);
                            let fallback = format!("{alias}={level}");
                            ta(
                                "cli-status-agent-risk-profile",
                                &[("alias", alias), ("level", &level)],
                                &fallback,
                            )
                        }
                        None => {
                            let fallback = format!("{alias}=<no risk_profile>");
                            ta(
                                "cli-status-agent-no-risk-profile-summary",
                                &[("alias", alias)],
                                &fallback,
                            )
                        }
                    })
                    .collect();
                println!(
                    "{}",
                    ta("cli-status-agents", &[("v", &summary.join(", "))], "Agents")
                );
            }
            println!(
                "{}",
                ta(
                    "cli-status-runtime",
                    &[("v", config.runtime.kind.as_wire())],
                    "Runtime"
                )
            );
            if service::is_running(&config) {
                println!(
                    "{}",
                    t("cli-status-service-running", "🟢 Service:       running")
                );
            } else {
                println!(
                    "{}",
                    t("cli-status-service-stopped", "🔴 Service:       stopped")
                );
            }
            #[cfg(feature = "gateway")]
            {
                match clawcrew_gateway::resolve_web_dashboard_availability(&config) {
                    Some(clawcrew_gateway::WebDashboardAvailability::Embedded) => {
                        let path = "embedded";
                        let fallback = format!("🌐 Web UI:        FOUND ({path})");
                        println!(
                            "{}",
                            ta("cli-status-web-ui-found", &[("path", path)], &fallback)
                        );
                    }
                    Some(clawcrew_gateway::WebDashboardAvailability::Filesystem(web_dist_dir)) => {
                        let path = web_dist_dir.display().to_string();
                        let fallback = format!("🌐 Web UI:        FOUND ({path})");
                        println!(
                            "{}",
                            ta("cli-status-web-ui-found", &[("path", &path)], &fallback)
                        );
                    }
                    None => {
                        println!(
                            "{}",
                            t("cli-status-web-ui-missing", "🌐 Web UI:        MISSING")
                        );
                    }
                }
            }
            let effective_memory_backend = config.resolve_active_storage().kind();
            let heartbeat_value = if config.heartbeat.enabled {
                let interval_minutes = config.heartbeat.interval_minutes.to_string();
                let heartbeat_every_fallback = format!("every {}min", interval_minutes);
                ta(
                    "cli-status-heartbeat-every-minutes",
                    &[("minutes", &interval_minutes)],
                    &heartbeat_every_fallback,
                )
            } else {
                t("cli-status-word-disabled", "disabled")
            };
            let heartbeat_fallback = format!("💓 Heartbeat:      {}", heartbeat_value);
            println!(
                "{}",
                ta(
                    "cli-status-heartbeat",
                    &[("v", &heartbeat_value)],
                    &heartbeat_fallback
                )
            );
            let memory_backend = effective_memory_backend.to_string();
            let memory_auto_save = if config.memory.auto_save {
                t("cli-status-word-on", "on")
            } else {
                t("cli-status-word-off", "off")
            };
            let memory_fallback = format!(
                "🧠 Memory:         {} (auto-save: {})",
                memory_backend, memory_auto_save
            );
            println!(
                "{}",
                ta(
                    "cli-status-memory",
                    &[
                        ("backend", &memory_backend),
                        ("auto_save", &memory_auto_save),
                    ],
                    &memory_fallback
                )
            );

            println!();
            // Per-agent security: each enabled agent's risk profile.
            for alias in &agent_aliases {
                let Some(profile) = config.risk_profile_for_agent(alias) else {
                    println!(
                        "{}",
                        ta(
                            "cli-status-security-noprofile",
                            &[("alias", alias)],
                            "Security: no risk_profile"
                        )
                    );
                    continue;
                };
                println!(
                    "{}",
                    ta("cli-status-security", &[("alias", alias)], "Security")
                );
                println!(
                    "{}",
                    ta(
                        "cli-status-workspace-only",
                        &[("v", &profile.workspace_only.to_string())],
                        "Workspace only"
                    )
                );
                let allowed_roots = if profile.allowed_roots.is_empty() {
                    t("cli-status-word-none", "(none)")
                } else {
                    profile.allowed_roots.join(", ")
                };
                let allowed_roots_fallback = format!("  Allowed roots:     {}", allowed_roots);
                println!(
                    "{}",
                    ta(
                        "cli-status-allowed-roots",
                        &[("v", &allowed_roots)],
                        &allowed_roots_fallback
                    )
                );
                let allowed_commands = profile.allowed_commands.join(", ");
                let allowed_commands_fallback =
                    format!("  Allowed commands:  {}", allowed_commands);
                println!(
                    "{}",
                    ta(
                        "cli-status-allowed-commands",
                        &[("v", &allowed_commands)],
                        &allowed_commands_fallback
                    )
                );
                let actions_cap = config
                    .runtime_profile_for_agent(alias)
                    .map_or(0, |r| r.max_actions_per_hour);
                println!(
                    "{}",
                    ta(
                        "cli-status-max-actions",
                        &[("v", &actions_cap.to_string())],
                        "Max actions/hour"
                    )
                );
            }
            let cost_tracking = if config.cost.enabled {
                t("cli-status-word-enabled", "enabled")
            } else {
                t("cli-status-word-disabled", "disabled")
            };
            let cost_tracking_fallback = format!("  Cost tracking:     {}", cost_tracking);
            println!(
                "{}",
                ta(
                    "cli-status-cost-tracking",
                    &[("v", &cost_tracking)],
                    &cost_tracking_fallback
                )
            );
            println!(
                "{}",
                ta(
                    "cli-status-max-cost-day",
                    &[("v", &format!("{:.2}", config.cost.daily_limit_usd))],
                    "Max cost/day"
                )
            );
            println!(
                "{}",
                ta(
                    "cli-status-max-cost-month",
                    &[("v", &format!("{:.2}", config.cost.monthly_limit_usd))],
                    "Max cost/month"
                )
            );
            if config.cost.enabled {
                match cost::CostTracker::new(config.cost.clone(), &config.data_dir) {
                    Ok(tracker) => match tracker.get_summary() {
                        Ok(summary) => {
                            let spent_today = format!("{:.4}", summary.daily_cost_usd);
                            let daily_limit = format!("{:.2}", config.cost.daily_limit_usd);
                            let spent_today_fallback =
                                format!("  Spent today:       ${spent_today} / ${daily_limit}");
                            println!(
                                "{}",
                                ta(
                                    "cli-status-spent-today",
                                    &[("spent", &spent_today), ("limit", &daily_limit)],
                                    &spent_today_fallback
                                )
                            );
                            let spent_month = format!("{:.4}", summary.monthly_cost_usd);
                            let monthly_limit = format!("{:.2}", config.cost.monthly_limit_usd);
                            let spent_month_fallback =
                                format!("  Spent this month:  ${spent_month} / ${monthly_limit}");
                            println!(
                                "{}",
                                ta(
                                    "cli-status-spent-month",
                                    &[("spent", &spent_month), ("limit", &monthly_limit)],
                                    &spent_month_fallback
                                )
                            );
                            // Pricing provenance is recorded per usage row.
                            // The warning qualifies the monthly spend line,
                            // so it reads the current-UTC-month model rollup
                            // rather than `summary.by_model`, which stays
                            // daily-scoped for other consumers; unpriced usage
                            // from an earlier day this month must not vanish
                            // at day rollover. Surface any explicitly unpriced
                            // subset loudly rather than let an understated
                            // dollar total reassure the operator. Configured
                            // zero rates and legacy rows without provenance
                            // remain compatible and do not trigger this
                            // warning.
                            let month_by_model = match tracker.get_current_month_model_stats() {
                                Ok(by_model) => by_model,
                                Err(e) => {
                                    eprintln!(
                                        "{}",
                                        ta(
                                            "cli-warn-cost-usage",
                                            &[("err", &e.to_string())],
                                            "Could not load cost usage"
                                        )
                                    );
                                    std::collections::HashMap::new()
                                }
                            };
                            let unpriced =
                                clawcrew_runtime::agent::cost::unpriced_models_in_summary(
                                    &month_by_model,
                                );
                            if !unpriced.is_empty() {
                                let uncosted_tokens: u64 =
                                    unpriced.iter().map(|m| m.unpriced_tokens).sum();
                                let count = unpriced.len().to_string();
                                let tokens = uncosted_tokens.to_string();
                                let models = unpriced
                                    .iter()
                                    .map(|m| m.model.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let warn_fallback = format!(
                                    "  ⚠ Pricing unavailable for {count} model(s) ({tokens} tokens uncosted): {models}. \
Recorded spend is understated and daily/monthly caps CANNOT be enforced for these. \
Add pricing to the active provider profile or supply a catalog entry."
                                );
                                eprintln!(
                                    "{}",
                                    ta(
                                        "cli-status-pricing-unavailable",
                                        &[
                                            ("count", &count),
                                            ("tokens", &tokens),
                                            ("models", &models),
                                        ],
                                        &warn_fallback
                                    )
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "{}",
                                ta(
                                    "cli-warn-cost-usage",
                                    &[("err", &e.to_string())],
                                    "Could not load cost usage"
                                )
                            );
                        }
                    },
                    Err(e) => {
                        eprintln!(
                            "{}",
                            ta(
                                "cli-warn-cost-tracker",
                                &[("err", &e.to_string())],
                                "Could not init cost tracker"
                            )
                        );
                    }
                }
            }
            println!(
                "{}",
                ta(
                    "cli-status-otp",
                    &[("v", &config.security.otp.enabled.to_string())],
                    "OTP enabled"
                )
            );
            println!(
                "{}",
                ta(
                    "cli-status-estop",
                    &[("v", &config.security.estop.enabled.to_string())],
                    "E-stop enabled"
                )
            );
            println!();
            println!("{}", t("cli-status-channels", "Channels:"));
            println!("{}", t("cli-status-cli-always", "  CLI:      ✅ always"));
            for entry in clawcrew_channels::listing::compiled_channels(&config.channels) {
                let channel_status = if entry.configured {
                    t("cli-status-word-configured", "configured")
                } else {
                    t("cli-status-word-not-configured", "not configured")
                };
                let status = if entry.configured {
                    ta(
                        "cli-status-channel-configured",
                        &[("status", &channel_status)],
                        format!("✅ {channel_status}"),
                    )
                } else {
                    ta(
                        "cli-status-channel-not-configured",
                        &[("status", &channel_status)],
                        format!("❌ {channel_status}"),
                    )
                };
                println!("  {:9} {}", entry.name, status);
            }
            let uncompiled =
                clawcrew_channels::listing::configured_uncompiled_channels(&config.channels);
            if !uncompiled.is_empty() {
                println!(
                    "{}",
                    t(
                        "cli-channels-not-compiled-header",
                        "  Configured but not compiled in this binary:"
                    )
                );
                for entry in &uncompiled {
                    let status = t(
                        "cli-status-channel-not-compiled",
                        "🚫 configured, not compiled",
                    );
                    println!("  {:9} {}", entry.name, status);
                }
                println!(
                    "{}",
                    t(
                        "cli-channels-build-hint",
                        "  Build from source with `./install.sh --source --preset full`, `--features channels-full`, or the specific `channel-*` feature."
                    )
                );
            }
            println!();
            println!("{}", t("cli-status-peripherals", "Peripherals:"));
            let peripherals_enabled = if config.peripherals.enabled {
                t("cli-status-word-yes", "yes")
            } else {
                t("cli-status-word-no", "no")
            };
            let peripherals_enabled_fallback = format!("  Enabled:   {}", peripherals_enabled);
            println!(
                "{}",
                ta(
                    "cli-status-peripherals-enabled",
                    &[("v", &peripherals_enabled)],
                    &peripherals_enabled_fallback
                )
            );
            println!(
                "{}",
                ta(
                    "cli-status-boards",
                    &[("v", &config.peripherals.boards.len().to_string())],
                    "Boards"
                )
            );

            Ok(())
        }

        #[cfg(feature = "agent-runtime")]
        Commands::Security { security_command } => match security_command {
            SecurityCommands::Status { agent, json } => {
                let report = security_status::build_report(&config, &agent)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    security_status::print_report(&report);
                }
                Ok(())
            }
            SecurityCommands::IssueClientCert {
                name,
                out_dir,
                force,
            } => issue_wss_client_cert(&config, &name, out_dir, force),
            SecurityCommands::RevokeClientCert {
                fingerprint,
                device,
            } => revoke_wss_client_cert(&config, fingerprint, device),
            SecurityCommands::ListClientCerts { json } => list_wss_client_certs(&config, json),
            SecurityCommands::EnrollPaircode { new, timeout_secs } => {
                if !new {
                    anyhow::bail!("pass --new to mint a fresh enrollment pairing code");
                }
                let generated = clawcrew_runtime::enroll::request_new_paircode(
                    &config.data_dir,
                    std::time::Duration::from_secs(timeout_secs),
                )
                .await?;
                println!(
                    "{}",
                    ta(
                        "cli-enroll-pairing-code",
                        &[("code", &generated.pairing_code)],
                        "pairing code"
                    )
                );
                println!(
                    "{}",
                    ta("cli-enroll-sas", &[("sas", &generated.sas)], "SAS")
                );
                Ok(())
            }
            SecurityCommands::RelayRotateNodeId => {
                if !config.relay.node_id.trim().is_empty() {
                    anyhow::bail!(
                        "[relay].node_id is pinned, so the node-id is fixed and not rotatable. \
                         Clear it to auto-mint (and enable rotation)."
                    );
                }
                clawcrew_runtime::relay::request_node_id_rotation(&config.data_dir)?;
                let rotate_secs = 15.to_string();
                println!(
                    "{}",
                    ta(
                        "cli-relay-rotation-requested",
                        &[("secs", &rotate_secs)],
                        "relay node-id rotation requested"
                    )
                );
                Ok(())
            }
        },

        Commands::Estop {
            estop_command,
            level,
            domains,
            tools,
        } => handle_estop_command(&config, estop_command, level, domains, tools),

        Commands::Cron { cron_command } => cron::handle_command(cron_command, &config),

        Commands::Models { model_command } => {
            #[cfg(feature = "agent-runtime")]
            {
                dispatch_models_command(model_command, &mut config).await
            }
            #[cfg(not(feature = "agent-runtime"))]
            {
                match model_command {
                    ModelCommands::List {
                        model_provider,
                        check,
                    } => {
                        doctor::run_configured_models(&config, model_provider.as_deref(), check)
                            .await
                    }
                    ModelCommands::Refresh { model_provider, .. } => {
                        doctor::run_models(&config, model_provider.as_deref(), false, false).await
                    }
                    _ => doctor::run_models(&config, None, false, false).await,
                }
            }
        }

        Commands::Providers {
            providers_command: None,
        } => {
            let model_providers = clawcrew_providers::list_model_providers();
            let configured_types: std::collections::HashSet<&str> = config
                .providers
                .models
                .iter_entries()
                .map(|(ty, _, _)| ty)
                .collect();
            println!(
                "Supported model model_providers ({} total):\n",
                model_providers.len()
            );
            println!("  ID (use in config)  DESCRIPTION"); // i18n-exempt: literal command/identifier example
            println!("  ─────────────────── ───────────");
            for category in clawcrew_providers::ModelProviderCategory::all() {
                let in_category: Vec<_> = model_providers
                    .iter()
                    .filter(|p| p.category == *category)
                    .collect();
                if in_category.is_empty() {
                    continue;
                }
                println!("\n  {}:", category.as_str());
                for p in in_category {
                    let is_configured = configured_types.contains(p.name);
                    let marker = if is_configured { " (configured)" } else { "" };
                    let local_tag = if p.local { " [local]" } else { "" };
                    println!("  {:<19} {}{}{}", p.name, p.display_name, local_tag, marker);
                }
            }
            println!(
                "\n  Set [providers.models.custom.<alias>] uri = \"<URL>\" for any \
                 OpenAI-compatible endpoint, or [providers.models.anthropic.<alias>] \
                 uri = \"<URL>\" for an Anthropic-compatible endpoint."
            );
            Ok(())
        }

        Commands::Providers {
            providers_command: Some(providers_command),
        } => Box::pin(alias_cli::handle_providers(providers_command, &mut config)).await,

        Commands::Service {
            service_command,
            service_init,
        } => {
            let init_system = service_init.parse()?;
            service::handle_command(&service_command, &config, init_system)
        }

        Commands::Doctor { doctor_command } => match doctor_command {
            Some(DoctorCommands::Models {
                model_provider,
                use_cache: _,
            }) => doctor::run_configured_models(&config, model_provider.as_deref(), true).await,
            Some(DoctorCommands::Traces {
                id,
                event,
                contains,
                limit,
            }) => doctor::run_traces(
                &config,
                id.as_deref(),
                event.as_deref(),
                contains.as_deref(),
                limit,
            ),
            Some(DoctorCommands::UpdateContextWindows {
                model_provider,
                dry_run,
            }) => {
                Box::pin(doctor::update_context_windows(
                    &mut config,
                    model_provider.as_deref(),
                    dry_run,
                    None,
                ))
                .await?;
                Ok(())
            }
            None => doctor::run(&config).await,
        },

        Commands::Channel { channel_command } => match channel_command {
            ChannelCommands::Start => {
                #[cfg(feature = "hardware")]
                clawcrew_runtime::agent::loop_::register_peripheral_tools_fn(Box::new(|config| {
                    Box::pin(async move {
                        clawcrew_hardware::peripherals::create_peripheral_tools(&config).await
                    })
                }));

                let cancel = tokio_util::sync::CancellationToken::new();
                let (sop_engine, sop_audit) = if config.sop.runtime_enabled() {
                    let mem: Arc<dyn clawcrew_memory::Memory> =
                        Arc::from(clawcrew_memory::create_memory_from_config(&config, None)?);
                    let sop_adapters = build_sop_adapters(&config);
                    let (engine, audit) = clawcrew_runtime::sop::build_sop_engine(
                        config.sop.clone(),
                        &config.data_dir,
                        &config.install_root_dir(),
                        mem,
                        sop_adapters,
                    );
                    (Some(engine), Some(audit))
                } else {
                    (None, None)
                };
                // EPIC A1 + SOP cron: same tick as the full daemon path.
                let sop_maintenance = spawn_sop_maintenance(
                    sop_engine.as_ref(),
                    sop_audit.as_ref(),
                    config.sop.maintenance_interval_secs,
                );
                let result = Box::pin(channels::start_channels(
                    config, None, cancel, sop_engine, sop_audit,
                ))
                .await;
                if let Some(handle) = sop_maintenance {
                    handle.abort();
                }
                result
            }
            ChannelCommands::Doctor => Box::pin(channels::doctor_channels(config)).await,
            other => Box::pin(channels::handle_command(other, &config)).await,
        },

        Commands::Agents { agents_command } => {
            Box::pin(alias_cli::handle_agents(agents_command, &mut config)).await
        }
        Commands::Channels { channels_command } => {
            Box::pin(alias_cli::handle_channels(channels_command, &mut config)).await
        }

        Commands::Integrations {
            integration_command,
        } => integrations::handle_command(integration_command, &config),

        Commands::Skills { skill_command } => skills::handle_command(skill_command, &config).await,

        Commands::Browse { path } => browse::handle_browse(path, &config),

        Commands::Sop { sop_command } => match sop_command {
            // Out-of-band approval verbs talk to the running daemon over the
            // gateway (they must see the daemon's runs, not a throwaway local
            // engine). List/Validate/Show stay local + synchronous.
            cmd @ (SopCommands::Approve { .. }
            | SopCommands::Deny { .. }
            | SopCommands::Pending) => sop_admin_dispatch(cmd, &config).await,
            other => sop::handle_command(other, &config),
        },

        Commands::Migrate { migrate_command } => {
            migration::handle_command(migrate_command, &config).await
        }

        Commands::Memory { memory_command } => {
            memory::cli::handle_command(memory_command, &config).await
        }
        Commands::Backup { backup_command } => {
            match backup_command {
                BackupCommands::Create { dest } => println!("Mock: Backup created at {}", dest),
                BackupCommands::Restore { source } => {
                    println!("Mock: Backup restored from {}", source)
                }
            };
            Ok(())
        }
        Commands::App { app_command } => {
            match app_command {
                AppCommands::Install { url } => println!("Mock: App installed from {}", url),
                AppCommands::Remove { name } => println!("Mock: App removed {}", name),
            };
            Ok(())
        }

        Commands::Auth { auth_command } => handle_auth_command(auth_command, &config).await,

        Commands::Hardware { hardware_command } => {
            hardware::handle_command(hardware_command.clone(), &config)
        }

        Commands::Peripheral { peripheral_command } => {
            Box::pin(peripherals::handle_command(
                peripheral_command.clone(),
                &config,
            ))
            .await
        }

        Commands::Desktop {
            install: do_install,
        } => {
            // The marketing download page is not live; point at the GitHub
            // releases page, which hosts the desktop download assets (.deb /
            // .AppImage / .dmg) for the latest release.
            let download_url = "https://github.com/clawcrew-labs/clawcrew/releases/latest";

            if do_install {
                println!(
                    "{}",
                    t(
                        "cli-desktop-download",
                        "Opening the ClawCrew companion app download page:"
                    )
                );
                println!();
                #[cfg(target_os = "macos")]
                {
                    println!("  macOS:  {download_url}"); // i18n-exempt: literal command/identifier example
                    println!();
                    println!(
                        "{}",
                        t(
                            "cli-desktop-homebrew",
                            "Or install via Homebrew (coming soon):"
                        )
                    );
                    println!("  brew install --cask clawcrew"); // i18n-exempt: literal command/identifier example
                }
                #[cfg(target_os = "linux")]
                {
                    println!("  Linux:  {download_url}"); // i18n-exempt: literal command/identifier example
                    println!();
                    println!(
                        "{}",
                        t(
                            "cli-desktop-linux-pkg",
                            "  The page provides .deb and .AppImage downloads by architecture."
                        )
                    );
                }
                #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                {
                    println!("  {download_url}");
                }
                println!();

                // On macOS, open the download page in the browser
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("open").arg(download_url).spawn();
                }
                #[cfg(target_os = "linux")]
                {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(download_url)
                        .spawn();
                }
                return Ok(());
            }

            // Locate the companion app
            let desktop_bin = {
                let mut found = None;

                // 1. macOS: check /Applications/ClawCrew.app
                #[cfg(target_os = "macos")]
                {
                    let app_paths = [
                        PathBuf::from("/Applications/ClawCrew.app/Contents/MacOS/ClawCrew"),
                        PathBuf::from(std::env::var("HOME").unwrap_or_default())
                            .join("Applications/ClawCrew.app/Contents/MacOS/ClawCrew"),
                    ];
                    for app in &app_paths {
                        if app.is_file() {
                            found = Some(app.clone());
                            break;
                        }
                    }
                }

                // 2. Same directory as the current executable
                if found.is_none()
                    && let Ok(exe) = std::env::current_exe()
                {
                    let sibling = exe.with_file_name("clawcrew-desktop");
                    if sibling.is_file() {
                        found = Some(sibling);
                    }
                }

                // 3. Common cargo/local install locations under the user's home directory.
                //    Uses directories::UserDirs so HOME (Unix) and USERPROFILE (Windows)
                //    are both resolved correctly. On Windows the binary is .exe — try
                //    both names since which::which (step 4) only catches PATH entries.
                if found.is_none()
                    && let Some(home) =
                        directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
                {
                    let bin_names: &[&str] = if cfg!(windows) {
                        &["clawcrew-desktop.exe", "clawcrew-desktop"]
                    } else {
                        &["clawcrew-desktop"]
                    };
                    // .cargo/bin works the same on Windows; .local/bin is XDG (Unix only).
                    let dirs: &[&str] = if cfg!(windows) {
                        &[".cargo/bin"]
                    } else {
                        &[".cargo/bin", ".local/bin"]
                    };
                    'outer: for dir in dirs {
                        for name in bin_names {
                            let candidate = home.join(dir).join(name);
                            if candidate.is_file() {
                                found = Some(candidate);
                                break 'outer;
                            }
                        }
                    }
                }

                // 4. Fallback to PATH lookup
                if found.is_none()
                    && let Ok(path) = which::which("clawcrew-desktop")
                {
                    found = Some(path);
                }

                // 5. Linux: an AppImage registered in the application menu is
                //    not on PATH and has no fixed binary name, so discover it
                //    from its desktop entry or common AppImage locations.
                #[cfg(all(feature = "agent-runtime", target_os = "linux"))]
                if found.is_none() {
                    found = find_linux_desktop_app();
                }

                found
            };

            match desktop_bin {
                Some(bin) => {
                    println!(
                        "{}",
                        t(
                            "cli-desktop-launching",
                            "Launching ClawCrew companion app..."
                        )
                    );
                    let mut command = std::process::Command::new(&bin);
                    command.stdin(std::process::Stdio::null());
                    command.stdout(std::process::Stdio::null());
                    command.stderr(std::process::Stdio::null());
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW.
                        command.creation_flags(0x0000_0200 | 0x0800_0000);
                    }
                    let _child = command
                        .spawn()
                        .with_context(|| format!("Failed to launch {}", bin.display()))?;
                    Ok(())
                }
                None => {
                    println!(
                        "{}",
                        t(
                            "cli-desktop-not-installed",
                            "ClawCrew companion app is not installed."
                        )
                    );
                    println!();
                    println!(
                        "{}",
                        ta(
                            "cli-desktop-download-at",
                            &[("url", download_url)],
                            "Download it at"
                        )
                    );
                    println!("  Or run: clawcrew desktop --install"); // i18n-exempt: literal command
                    println!();
                    println!(
                        "{}",
                        t(
                            "cli-desktop-blurb1",
                            "The companion app is a lightweight menu bar app that"
                        )
                    );
                    println!(
                        "{}",
                        t(
                            "cli-desktop-blurb2",
                            "connects to the same gateway as the CLI."
                        )
                    );
                    std::process::exit(1);
                }
            }
        }

        Commands::Locales { locales_command } => {
            let LocalesCommands::Fetch { locale, catalog } = locales_command;
            fetch_locales(&locale, catalog.as_deref()).await?;
            Ok(())
        }

        Commands::Update {
            check,
            force,
            version,
            json,
        } => {
            if check {
                let info = commands::update::check(version.as_deref()).await?;
                if json {
                    // Machine-readable shape consumed by the gateway's
                    // `GET /api/version/check`. Keep field names stable.
                    println!(
                        "{}",
                        serde_json::to_string(&serde_json::json!({
                            "current_version": info.current_version,
                            "latest_version": info.latest_version,
                            "is_newer": info.is_newer,
                            "release_url": info.release_url,
                            "release_notes": info.release_notes,
                            "published_at": info.published_at,
                        }))?
                    );
                } else if info.is_newer {
                    println!(
                        "{}",
                        ta(
                            "cli-update-available",
                            &[
                                ("current", &info.current_version),
                                ("latest", &info.latest_version)
                            ],
                            "Update available"
                        )
                    );
                } else {
                    println!(
                        "{}",
                        ta(
                            "cli-update-already-current",
                            &[("version", &info.current_version)],
                            "Already up to date"
                        )
                    );
                }
                Ok(())
            } else {
                commands::update::run(version.as_deref(), force).await
            }
        }

        Commands::SelfTest { quick } => {
            let results = if quick {
                commands::self_test::run_quick(&config).await?
            } else {
                commands::self_test::run_full(&config).await?
            };
            commands::self_test::print_results(&results);
            let failed = results.iter().filter(|r| !r.passed).count();
            if failed > 0 {
                std::process::exit(1);
            }
            Ok(())
        }

        Commands::Eval { eval_command } => match eval_command {
            EvalCommands::Run {
                suite,
                mode,
                format,
            } => {
                let suite_dir = suite.unwrap_or_else(|| config.eval.suite_dir.clone());
                let mode: clawcrew_eval::Mode =
                    mode.unwrap_or_else(|| config.eval.mode.clone()).parse()?;
                let report = commands::eval::run(std::path::PathBuf::from(suite_dir), mode).await?;
                commands::eval::print_report(&report, format);
                // Only a failing suite needs the hard exit to carry a non-zero
                // status; a passing run returns normally so shutdown runs.
                match report.exit_code() {
                    0 => Ok(()),
                    code => std::process::exit(code),
                }
            }
        },

        Commands::Config { config_command } => match config_command {
            ConfigCommands::Schema { path } => {
                #[cfg(feature = "schema-export")]
                {
                    let schema = schemars::schema_for!(config::Config);
                    let value = match path.as_deref() {
                        None => serde_json::to_value(&schema)
                            .context("failed to serialize JSON Schema")?,
                        Some(prop_path) => {
                            let full = serde_json::to_value(&schema)
                                .context("failed to serialize JSON Schema")?;
                            let mut out = full;
                            if let serde_json::Value::Object(ref mut map) = out {
                                map.insert(
                                    "x-clawcrew-requested-path".into(),
                                    serde_json::Value::String(prop_path.into()),
                                );
                            }
                            out
                        }
                    };
                    println!("{}", serde_json::to_string_pretty(&value)?);
                    Ok(())
                }
                #[cfg(not(feature = "schema-export"))]
                {
                    let _ = path;
                    anyhow::bail!("clawcrew was built without the 'schema-export' feature")
                }
            }
            ConfigCommands::List { filter, secrets } => {
                let entries = config.prop_fields();
                println!(
                    "{}",
                    t(
                        "cli-config-legend",
                        "Legend: \u{1f489} env-overridden  \u{1f512} secret"
                    )
                );
                println!();
                let mut current_category = "";
                for entry in &entries {
                    if secrets && !entry.is_secret {
                        continue;
                    }
                    if let Some(ref f) = filter
                        && !entry.name.starts_with(f.as_str())
                    {
                        continue;
                    }
                    if entry.category != current_category {
                        if !current_category.is_empty() {
                            println!();
                        }
                        println!("{}:", entry.category);
                        current_category = entry.category;
                    }
                    let env = if config.prop_is_env_overridden(&entry.name) {
                        "\u{1f489} "
                    } else {
                        "  "
                    };
                    let lock = if entry.is_secret { " \u{1f512}" } else { "" };
                    println!(
                        "{env}{:<45} = {:<20} ({}){lock}",
                        entry.name, entry.display_value, entry.type_hint
                    );
                }
                Ok(())
            }
            ConfigCommands::Get { path, json } => {
                let known_paths: Vec<String> =
                    config.prop_fields().into_iter().map(|f| f.name).collect();
                let path = clawcrew_config::helpers::resolve_field_path(&known_paths, &path);
                if Config::prop_is_secret(&path) {
                    let entries = config.prop_fields();
                    let populated = entries
                        .iter()
                        .find(|e| e.name == path)
                        .map(|e| e.display_value != "<unset>")
                        .unwrap_or(false);
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "path": path,
                                "populated": populated,
                            }))?
                        );
                    } else if populated {
                        println!(
                            "{}",
                            ta(
                                "cli-config-secret-set",
                                &[("path", &path)],
                                "is set (encrypted secret, value not displayed)"
                            )
                        );
                    } else {
                        println!(
                            "{}",
                            ta(
                                "cli-config-secret-unset",
                                &[("path", &path)],
                                "is not set (encrypted secret)"
                            )
                        );
                    }
                } else {
                    match config.get_prop(&path) {
                        Ok(value) => {
                            if json {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&serde_json::json!({
                                        "path": path,
                                        "value": value,
                                    }))?
                                );
                            } else {
                                println!("{value}");
                            }
                        }
                        Err(e) => {
                            // Classify the anyhow string into a stable code so
                            // the CLI's --json envelope matches the HTTP shape.
                            // Same single-source-of-truth helper the gateway
                            // uses; never hardcode a code at the call site.
                            let api_err =
                                clawcrew_config::api_error::ConfigApiError::from_validation(
                                    anyhow::Error::msg(e.to_string()),
                                )
                                .with_path(&path);
                            if json {
                                eprintln!("{}", serde_json::to_string_pretty(&api_err)?);
                                std::process::exit(1);
                            }
                            anyhow::bail!("{e}");
                        }
                    }
                }
                Ok(())
            }
            ConfigCommands::Set {
                path,
                value,
                no_interactive,
                comment,
                json,
            } => {
                crate::config::migration::ensure_disk_at_current_version(&config.config_path)?;
                let known_paths: Vec<String> =
                    config.prop_fields().into_iter().map(|f| f.name).collect();
                let mut path = clawcrew_config::helpers::resolve_field_path(&known_paths, &path);
                if ensure_map_key_for_prop_path(&mut config, &path)? {
                    let known_paths: Vec<String> =
                        config.prop_fields().into_iter().map(|f| f.name).collect();
                    path = clawcrew_config::helpers::resolve_field_path(&known_paths, &path);
                }
                if no_interactive {
                    let val = value.ok_or_else(|| {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                                .with_attrs(::serde_json::json!({"path": path})),
                            "config set --no-interactive refused: positional value missing"
                        );
                        anyhow::Error::msg(format!(
                            "Value required in --no-interactive mode. Usage: clawcrew config set --no-interactive {path} <value>"
                        ))
                    })?;
                    config.set_prop_persistent(&path, &val)?;
                } else if Config::prop_is_secret(&path) {
                    if value.is_some() {
                        eprintln!(
                            "  \u{26a0} {path} is an encrypted secret \u{2014} using masked input."
                        );
                    }
                    let secret_value = secret_prompt(&format!("Enter value for {path}"), false)?
                        .trim()
                        .to_string();
                    if !secret_value.is_empty() {
                        eprintln!("{}", ta("cli-secret-received", &[], "  ✓ Secret received"));
                    }
                    if secret_value.is_empty() {
                        anyhow::bail!("Value cannot be empty.");
                    }
                    config.set_prop_persistent(&path, &secret_value)?;
                } else if let Some(val) = value {
                    config.set_prop_persistent(&path, &val)?;
                } else if let Some(provider_type) = model_path_provider_type(&path) {
                    use dialoguer::{FuzzySelect, Input};
                    let provider_ref = path
                        .split('.')
                        .nth(3)
                        .map(|alias| format!("{provider_type}.{alias}"));
                    let catalog_selector = provider_ref.as_deref().unwrap_or(provider_type);
                    let (models, _pricing, live) =
                        clawcrew_runtime::quickstart::model_catalog_with_config(
                            Some(&config),
                            catalog_selector,
                        )
                        .await;
                    if live && !models.is_empty() {
                        let current = config.get_prop(&path).unwrap_or_default();
                        let default = models.iter().position(|m| m == &current).unwrap_or(0);
                        let Some(idx) = FuzzySelect::new()
                            .with_prompt(format!("Model id for {provider_type}"))
                            .items(&models)
                            .default(default)
                            .max_length(models.len().max(1))
                            .interact_opt()?
                        else {
                            anyhow::bail!("cancelled");
                        };
                        config.set_prop_persistent(&path, &models[idx])?;
                    } else {
                        eprintln!(
                            "  no live catalog for `{provider_type}` — \
                             enter the model id manually."
                        );
                        let m = Input::<String>::new()
                            .with_prompt(format!("Model id for {provider_type}"))
                            .allow_empty(false)
                            .interact_text()?;
                        config.set_prop_persistent(&path, &m)?;
                    }
                } else {
                    let field_info = config.prop_fields().into_iter().find(|f| f.name == path);
                    let variants = field_info.as_ref().and_then(|info| {
                        let get_variants = info.enum_variants?;
                        let variants = get_variants();
                        let current_index = variants
                            .iter()
                            .position(|v| v == &info.display_value)
                            .unwrap_or(0);
                        Some((variants, current_index))
                    });
                    if let Some((variants, current_index)) = variants {
                        let selected = Select::new()
                            .with_prompt(format!("Select value for {path}"))
                            .items(&variants)
                            .default(current_index)
                            .interact()?;
                        config.set_prop_persistent(&path, &variants[selected])?;
                    } else if field_info
                        .as_ref()
                        .is_some_and(|f| f.kind == crate::config::PropKind::StringArray)
                    {
                        let current_items: Vec<String> = field_info
                            .as_ref()
                            .and_then(|f| {
                                let raw = toml::from_str::<toml::Value>(&format!(
                                    "v = {}",
                                    if f.display_value == "<unset>" {
                                        "[]".to_string()
                                    } else {
                                        f.display_value.clone()
                                    }
                                ))
                                .ok();
                                raw.and_then(|v| v.get("v").cloned())
                                    .and_then(|v| v.as_array().cloned())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                            .collect()
                                    })
                            })
                            .unwrap_or_default();
                        let editor_content = current_items.join("\n");
                        let edited = dialoguer::Editor::new()
                            .edit(&editor_content)?
                            .unwrap_or(editor_content);
                        let val = edited
                            .lines()
                            .map(|l| l.trim())
                            .filter(|l| !l.is_empty())
                            .collect::<Vec<_>>()
                            .join(", ");
                        config.set_prop_persistent(&path, &val)?;
                    } else {
                        anyhow::bail!("Value required. Usage: clawcrew config set {path} <value>");
                    }
                }
                Box::pin(config.save_dirty()).await?;
                if let Some(c) = comment.as_ref()
                    && !c.is_empty()
                {
                    apply_comment_inline(&config.config_path, &path, c).await?;
                }
                if json {
                    let envelope = if Config::prop_is_secret(&path) {
                        serde_json::json!({"path": path, "populated": true})
                    } else {
                        let value_str = config.get_prop(&path).unwrap_or_default();
                        serde_json::json!({"path": path, "value": value_str})
                    };
                    println!("{}", serde_json::to_string_pretty(&envelope)?);
                } else {
                    println!(
                        "{}",
                        ta("cli-config-updated", &[("path", &path)], "updated")
                    );
                }
                Ok(())
            }
            ConfigCommands::Init { section, json } => {
                crate::config::migration::ensure_disk_at_current_version(&config.config_path)?;
                let mut initialized: Vec<String> = config
                    .init_defaults(section.as_deref())
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                for section in &initialized {
                    config.mark_dirty(section);
                }
                // `init_defaults` only instantiates nested struct sections. A
                // `<section>.<alias>` argument names a dynamic-map entry, which
                // has to be materialized through `create_map_key` instead.
                if let Some(arg) = section.as_deref()
                    && let Some(created) = init_map_alias(&mut config, arg)?
                {
                    mark_new_map_alias_dirty(&mut config, &created);
                    initialized.push(created);
                }
                if !initialized.is_empty() {
                    Box::pin(config.save_dirty()).await?;
                }
                if json {
                    let envelope = serde_json::json!({"initialized": initialized});
                    println!("{}", serde_json::to_string_pretty(&envelope)?);
                } else if initialized.is_empty() {
                    println!(
                        "{}",
                        t(
                            "cli-config-all-configured",
                            "All sections already configured."
                        )
                    );
                } else {
                    println!(
                        "Initialized {} section(s) with defaults:",
                        initialized.len()
                    );
                    for name in &initialized {
                        println!("  {name}");
                    }
                    println!(
                        "\n{}",
                        t(
                            "cli-config-review-hint",
                            "Run `clawcrew config list` to review, then set required fields."
                        )
                    );
                }
                Ok(())
            }
            ConfigCommands::Migrate { json } => {
                match crate::config::migration::migrate_file_in_place(&config.config_path)? {
                    Some(report) => {
                        let to = report.to_version;
                        if json {
                            let envelope = serde_json::json!({
                                "migrated": true,
                                "backup_path": report.backup_path.display().to_string(),
                                "schema_version": to,
                            });
                            println!("{}", serde_json::to_string_pretty(&envelope)?);
                        } else {
                            println!(
                                "{}",
                                ta(
                                    "cli-config-backed-up",
                                    &[("path", &report.backup_path.display().to_string())],
                                    "Backed up to"
                                )
                            );
                            println!(
                                "Migrated {} to schema version {to}.",
                                config.config_path.display()
                            );
                        }
                    }
                    None => {
                        let strict_error = std::fs::read_to_string(&config.config_path)
                            .ok()
                            .and_then(|raw| {
                                crate::config::migration::migrate_to_current(&raw)
                                    .err()
                                    .map(|e| format!("{e:#}"))
                            });
                        if json {
                            let envelope = serde_json::json!({
                                "migrated": false,
                                "schema_version": crate::config::migration::CURRENT_SCHEMA_VERSION,
                                "valid": strict_error.is_none(),
                                "error": strict_error,
                            });
                            println!("{}", serde_json::to_string_pretty(&envelope)?);
                            if strict_error.is_some() {
                                std::process::exit(1);
                            }
                        } else {
                            println!(
                                "{}",
                                t(
                                    "cli-config-schema-current",
                                    "Config already at current schema version."
                                )
                            );
                            if let Some(error) = strict_error {
                                anyhow::bail!(
                                    "config at {} does not deserialize strictly; the resilient \
                                     loader is substituting defaults for the failing section. \
                                     Parse error: {error}",
                                    config.config_path.display()
                                );
                            }
                        }
                    }
                }
                Ok(())
            }
            ConfigCommands::Patch { input, json } => {
                crate::config::migration::ensure_disk_at_current_version(&config.config_path)?;
                let body = match input.as_deref() {
                    None | Some("-") => {
                        use std::io::Read;
                        let mut buf = String::new();
                        if let Err(err) = std::io::stdin().read_to_string(&mut buf) {
                            let api_err = ConfigApiError::new(
                                ConfigApiCode::InternalError,
                                format!("failed to read JSON Patch from stdin: {err}"),
                            );
                            config_patch_fail_json_or_human(
                                json,
                                api_err,
                                format!("Failed to read JSON Patch from stdin: {err}"),
                            )?;
                        }
                        buf
                    }
                    Some(path) => match tokio::fs::read_to_string(path).await {
                        Ok(body) => body,
                        Err(err) => {
                            let api_err = ConfigApiError::new(
                                ConfigApiCode::InternalError,
                                format!("failed to read JSON Patch from {path}: {err}"),
                            );
                            config_patch_fail_json_or_human(
                                json,
                                api_err,
                                format!("Failed to read JSON Patch from {path}: {err}"),
                            )?
                        }
                    },
                };

                let parsed: serde_json::Value = match serde_json::from_str(body.trim()) {
                    Ok(parsed) => parsed,
                    Err(err) => {
                        let api_err = config_patch_json_value_type_error(
                            format!("JSON Patch body must be valid JSON: {err}"),
                            None,
                            None,
                        );
                        config_patch_fail_json_or_human(
                            json,
                            api_err,
                            format!("JSON Patch body must be valid JSON: {err}"),
                        )?
                    }
                };
                let ops = match parsed.as_array() {
                    Some(ops) => ops,
                    None => {
                        let api_err = config_patch_json_value_type_error(
                            "JSON Patch body must be a JSON array of operations",
                            None,
                            None,
                        );
                        config_patch_fail_json_or_human(
                            json,
                            api_err,
                            "JSON Patch body must be a JSON array of operations",
                        )?
                    }
                };

                // The withheld-capability notice is recorded once per config
                // application, and the record written during startup describes
                // the config as it was loaded. A patch that turns the section on
                // is a new application of that setting, so the state before the
                // ops run is captured here to tell that transition apart from a
                // patch that leaves an already-enabled section alone.
                #[cfg(feature = "agent-runtime")]
                let verifiable_intent_was_enabled = config.verifiable_intent.enabled;

                let mut results: Vec<serde_json::Value> = Vec::with_capacity(ops.len());

                for (idx, op) in ops.iter().enumerate() {
                    let object = match op.as_object() {
                        Some(object) => object,
                        None => {
                            let message = format!("JSON Patch op[{idx}] must be an object");
                            let api_err = config_patch_json_value_type_error(
                                message.clone(),
                                None,
                                Some(idx),
                            );
                            config_patch_fail_json_or_human(json, api_err, message)?
                        }
                    };
                    let op_name = match object.get("op").and_then(|v| v.as_str()) {
                        Some(op_name) => op_name,
                        None => {
                            let message =
                                format!("JSON Patch op[{idx}] requires string `op` field");
                            let api_err = config_patch_json_value_type_error(
                                message.clone(),
                                None,
                                Some(idx),
                            );
                            config_patch_fail_json_or_human(json, api_err, message)?
                        }
                    };
                    let raw_path = match object.get("path").and_then(|v| v.as_str()) {
                        Some(raw_path) => raw_path,
                        None => {
                            let message =
                                format!("JSON Patch op[{idx}] requires string `path` field");
                            let api_err = config_patch_json_value_type_error(
                                message.clone(),
                                None,
                                Some(idx),
                            );
                            config_patch_fail_json_or_human(json, api_err, message)?
                        }
                    };
                    let path = if let Some(stripped) = raw_path.strip_prefix('/') {
                        stripped.replace('/', ".")
                    } else {
                        raw_path.to_string()
                    };
                    if matches!(op_name, "add" | "replace") && config.ensure_map_key_for_path(&path)
                    {
                        let err = ConfigApiError::new(
                            ConfigApiCode::ValidationFailed,
                            "alias `default` is reserved and cannot be created",
                        )
                        .with_path(&path)
                        .with_op_index(idx);
                        let human = format!(
                            "op[{idx}] `{op_name}` on `{path}`: alias `default` is reserved and cannot be created"
                        );
                        config_patch_fail_json_or_human(json, err, human)?;
                    }
                    let comment = match object.get("comment") {
                        Some(value) => match value.as_str() {
                            Some(comment) => Some(comment),
                            None => {
                                let message = format!(
                                    "JSON Patch op[{idx}] `comment` field must be a string"
                                );
                                let api_err = config_patch_json_value_type_error(
                                    message.clone(),
                                    Some(path.clone()),
                                    Some(idx),
                                );
                                config_patch_fail_json_or_human(json, api_err, message)?
                            }
                        },
                        None => None,
                    };
                    let is_secret = Config::prop_is_secret(&path);

                    let result_entry: serde_json::Value = match op_name {
                        "add" | "replace" => {
                            let value = match op.get("value") {
                                Some(value) => value,
                                None => {
                                    ::clawcrew_log::record!(
                                        WARN,
                                        ::clawcrew_log::Event::new(
                                            module_path!(),
                                            ::clawcrew_log::Action::Reject
                                        )
                                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                                        .with_attrs(
                                            ::serde_json::json!({
                                                "op": op_name,
                                                "op_index": idx,
                                                "path": path,
                                            })
                                        ),
                                        "config patch op rejected: missing `value` field"
                                    );
                                    let message = format!(
                                        "op[{idx}] `{op_name}` on `{path}`: missing `value` field"
                                    );
                                    let api_err = config_patch_json_value_type_error(
                                        message.clone(),
                                        Some(path.clone()),
                                        Some(idx),
                                    );
                                    config_patch_fail_json_or_human(json, api_err, message)?
                                }
                            };
                            let value_str =
                                json_value_to_setprop_string(value, &config, &path, idx, json)?;
                            match config.set_prop_persistent(&path, &value_str) {
                                Ok(()) => {}
                                Err(err) => {
                                    let api_err = config_patch_map_prop_error(err, &path, idx);
                                    let human = format!(
                                        "op[{idx}] `{op_name}` on `{path}` failed: {}",
                                        api_err.message
                                    );
                                    config_patch_fail_json_or_human(json, api_err, human)?;
                                }
                            }
                            if is_secret {
                                serde_json::json!({
                                    "op": op_name,
                                    "path": path,
                                    "populated": !value_str.is_empty(),
                                })
                            } else {
                                serde_json::json!({
                                    "op": op_name,
                                    "path": path,
                                    "value": value_str,
                                })
                            }
                        }
                        "remove" => {
                            match config.set_prop_persistent(&path, "") {
                                Ok(()) => {}
                                Err(err) => {
                                    let api_err = config_patch_map_prop_error(err, &path, idx);
                                    let human = format!(
                                        "op[{idx}] `remove` on `{path}` failed: {}",
                                        api_err.message
                                    );
                                    config_patch_fail_json_or_human(json, api_err, human)?;
                                }
                            }
                            if is_secret {
                                serde_json::json!({
                                    "op": "remove",
                                    "path": path,
                                    "populated": false,
                                })
                            } else {
                                serde_json::json!({
                                    "op": "remove",
                                    "path": path,
                                    "value": serde_json::Value::Null,
                                })
                            }
                        }
                        "test" => {
                            if is_secret {
                                let err =
                                    ConfigApiError::secret_test_forbidden(&path).with_op_index(idx);
                                let human = format!(
                                    "op[{idx}] `test` on `{path}`: secret_test_forbidden \
                                     \u{2014} test ops are not allowed against secret paths"
                                );
                                config_patch_fail_json_or_human(json, err, human)?;
                            }
                            let want = match op.get("value") {
                                Some(value) => value,
                                None => {
                                    let err = ConfigApiError::new(
                                        ConfigApiCode::ValueTypeMismatch,
                                        "JSON Patch `test` op requires `value` field",
                                    )
                                    .with_path(&path)
                                    .with_op_index(idx);
                                    let human = format!(
                                        "op[{idx}] `test` on `{path}`: missing `value` field"
                                    );
                                    config_patch_fail_json_or_human(json, err, human)?
                                }
                            };
                            let actual = match config.get_prop(&path) {
                                Ok(actual) => actual,
                                Err(err) => {
                                    let human = format!(
                                        "op[{idx}] `test` on `{path}` failed to read current value: {err}"
                                    );
                                    let api_err = config_patch_map_prop_error(err, &path, idx);
                                    config_patch_fail_json_or_human(json, api_err, human)?
                                }
                            };
                            let want_str = match clawcrew_config::typed_value::coerce_for_set_prop(
                                want,
                                config_patch_prop_kind(&config, &path),
                            ) {
                                Ok(want_str) => want_str,
                                Err(err) => {
                                    let err = err.with_path(&path).with_op_index(idx);
                                    config_patch_fail_json_or_human(
                                        json,
                                        err.clone(),
                                        err.message.clone(),
                                    )?
                                }
                            };
                            if actual != want_str {
                                let err = ConfigApiError::new(
                                    ConfigApiCode::ValidationFailed,
                                    format!(
                                        "`test` op failed: expected {want_str:?}, got {actual:?}"
                                    ),
                                )
                                .with_path(&path)
                                .with_op_index(idx);
                                let human = format!(
                                    "op[{idx}] `test` on `{path}` failed: expected {want_str}, got {actual}"
                                );
                                config_patch_fail_json_or_human(json, err, human)?;
                            }
                            serde_json::json!({
                                "op": "test",
                                "path": path,
                                "value": actual,
                            })
                        }
                        "move" | "copy" => {
                            let err = ConfigApiError::op_not_supported(op_name)
                                .with_path(&path)
                                .with_op_index(idx);
                            let human = format!(
                                "op[{idx}] `{op_name}` on `{path}`: op_not_supported \
                                 \u{2014} move/copy require a reference graph that is not built yet"
                            );
                            config_patch_fail_json_or_human(json, err, human)?
                        }
                        other => {
                            let err = ConfigApiError::new(
                                ConfigApiCode::OpNotSupported,
                                format!("unknown JSON Patch operation `{other}`"),
                            )
                            .with_path(&path)
                            .with_op_index(idx);
                            let human = format!("op[{idx}] unknown JSON Patch operation `{other}`");
                            config_patch_fail_json_or_human(json, err, human)?
                        }
                    };
                    results.push(result_entry);
                }

                if let Err(err) = config.validate() {
                    let api_err = ConfigApiError::from_validation(err);
                    let human = format!(
                        "validation failed after applying patch \u{2014} no changes saved: {}",
                        api_err.message
                    );
                    config_patch_fail_json_or_human(json, api_err, human)?;
                }
                Box::pin(config.save_dirty()).await?;

                // Report the withheld tool when this patch is what enabled the
                // section. The helper returns early while it stays disabled, so
                // the guard is only about the already-enabled case: the startup
                // call has recorded that one for this process, and recording it
                // again here would restore the second copy this command used to
                // write. The trace sink was installed before the command
                // dispatched, so the record has somewhere to go.
                #[cfg(feature = "agent-runtime")]
                if !verifiable_intent_was_enabled {
                    warn_verifiable_intent_withheld(&config);
                }

                if json {
                    let body = serde_json::json!({"saved": true, "results": results});
                    println!("{}", serde_json::to_string_pretty(&body)?);
                } else {
                    println!(
                        "{}",
                        ta(
                            "cli-config-applied-ops",
                            &[("count", &results.len().to_string())],
                            "Applied operations"
                        )
                    );
                    for entry in &results {
                        let op = entry.get("op").and_then(|v| v.as_str()).unwrap_or("?");
                        let path = entry.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                        if let Some(populated) = entry.get("populated").and_then(|v| v.as_bool()) {
                            let lock = "\u{1f512}";
                            let label = if populated { "set" } else { "unset" };
                            println!("  {op:<8} {path}  {lock} ({label})");
                        } else {
                            let value = entry
                                .get("value")
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "null".to_string());
                            println!("  {op:<8} {path} = {value}");
                        }
                    }
                }
                Ok(())
            }
            ConfigCommands::Docs => {
                let port = config.gateway.port;
                let host = if config.gateway.host == "[::]" || config.gateway.host == "0.0.0.0" {
                    "127.0.0.1".to_string()
                } else {
                    config.gateway.host.clone()
                };
                let url = format!("http://{host}:{port}/api/docs");

                let health = format!("http://{host}:{port}/health");
                let daemon_running = reqwest::Client::new()
                    .get(&health)
                    .timeout(std::time::Duration::from_secs(2))
                    .send()
                    .await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false);

                println!("{url}");
                if !daemon_running {
                    eprintln!(
                        "Note: gateway does not appear to be running at {host}:{port}. \
                         Start it with `clawcrew service start` (background) or `clawcrew daemon` (foreground) to load the explorer."
                    );
                }
                Ok(())
            }
            ConfigCommands::Complete { partial } => {
                let prefix = partial.as_deref().unwrap_or("");
                for entry in config.prop_fields() {
                    if entry.name.starts_with(prefix) {
                        println!("{}", entry.name);
                    }
                }
                Ok(())
            }
            ConfigCommands::Generate { version, encrypt } => {
                let target = version.unwrap_or(crate::config::migration::CURRENT_SCHEMA_VERSION);
                let clawcrew_dir = config
                    .config_path
                    .parent()
                    .map(std::path::Path::to_path_buf);
                let opts = crate::config::migration::GenerateOptions {
                    encrypt_secrets: encrypt,
                    secret_store_dir: clawcrew_dir.as_deref(),
                };
                let toml_out = crate::config::migration::generate(target, &opts)?;
                print!("{toml_out}");
                Ok(())
            }
        },

        Commands::Props { props_command } => {
            let DeprecatedPropsCommands::Any(args) = props_command;
            drop(args);
            anyhow::bail!(
                "`clawcrew props` has been renamed to `clawcrew config`. \
                 Replace `props` with `config` in your command and try again."
            );
        }

        #[cfg(feature = "plugins-wasm")]
        Commands::Plugin { plugin_command } => match plugin_command {
            PluginCommands::List => {
                let host = plugin_host_with_configured_security(&config)?;
                plugin_catalog::print(&config, &host);
                let target = config.plugins.resolved_plugins_dir().display().to_string();
                for legacy in crate::config::schema::legacy_plugin_dirs_with_entries(&config) {
                    eprintln!(
                        "{}",
                        ta(
                            "cli-plugin-legacy-detected",
                            &[("path", &legacy.display().to_string()), ("target", &target)],
                            "Note: plugins in a legacy location are not loaded by the agent — \
                             run `clawcrew plugin migrate` to move them.",
                        )
                    );
                }
                Ok(())
            }
            PluginCommands::Search { query, registry } => {
                let registry_url = plugin_registry::registry_url(registry.as_deref());
                let index = plugin_registry::fetch_registry_index(&registry_url).await?;
                clawcrew::plugins::registry::write_cached_registry_index(
                    &config.data_dir,
                    &registry_url,
                    &index,
                )?;
                let matches = plugin_registry::search_entries(&index, &query);
                if matches.is_empty() {
                    println!(
                        "{}",
                        ta(
                            "cli-plugin-search-none",
                            &[("query", &query)],
                            "No matching plugins."
                        )
                    );
                } else {
                    println!(
                        "{}",
                        ta(
                            "cli-plugin-search-results",
                            &[("query", &query), ("count", &matches.len().to_string())],
                            "Plugins matching query:"
                        )
                    );
                    for plugin in &matches {
                        let missing_description;
                        let description = if let Some(description) = plugin.description.as_deref() {
                            description
                        } else {
                            missing_description =
                                t("cli-plugin-no-description", "(no description)");
                            &missing_description
                        };
                        println!(
                            "{}",
                            ta(
                                "cli-plugin-search-result",
                                &[
                                    ("name", &plugin.name),
                                    ("version", &plugin.version),
                                    ("description", description),
                                ],
                                "Plugin search result"
                            )
                        );
                    }
                }
                Ok(())
            }
            PluginCommands::Install { source, registry } => {
                if plugin_registry::looks_like_url(&source) {
                    bail!(
                        "`clawcrew plugin install <url>` is not supported; use `--registry <url>` with a plugin name, or install a local plugin path"
                    );
                }
                let mut host = plugin_host_with_configured_security(&config)?;
                if plugin_registry::is_local_plugin_source(&source) {
                    let name = host.install(&source)?;
                    let config_entries = installed_plugin_config_entries(&host, &name)?;
                    println!(
                        "{}",
                        ta(
                            "cli-plugin-installed-from",
                            &[("source", &source)],
                            "Plugin installed"
                        )
                    );
                    Box::pin(seed_plugin_config_entries(&mut config, &config_entries)).await?;
                } else {
                    let registry_url = plugin_registry::registry_url(registry.as_deref());
                    println!(
                        "{}",
                        ta(
                            "cli-plugin-install-resolving",
                            &[("source", &source)],
                            "Resolving plugin from registry..."
                        )
                    );
                    let downloaded = plugin_registry::download_registry_plugin(
                        &registry_url,
                        &source,
                        Some(&config.data_dir),
                    )
                    .await?;
                    let plugin_dir = downloaded.plugin_dir().display().to_string();
                    let name = host.install(&plugin_dir)?;
                    let config_entries = installed_plugin_config_entries(&host, &name)?;
                    println!(
                        "{}",
                        ta(
                            "cli-plugin-installed-name-version",
                            &[
                                ("name", &downloaded.manifest().name),
                                ("version", &downloaded.manifest().version),
                            ],
                            "Plugin installed"
                        )
                    );
                    Box::pin(seed_plugin_config_entries(&mut config, &config_entries)).await?;
                }
                Ok(())
            }
            PluginCommands::Remove { name } => {
                let mut host = plugin_host_with_configured_security(&config)?;
                host.remove(&name)?;
                println!(
                    "{}",
                    ta("cli-plugin-removed", &[("name", &name)], "Plugin removed")
                );
                Ok(())
            }
            PluginCommands::Info { name } => {
                let host = plugin_host_with_configured_security(&config)?;
                match host.get_plugin(&name) {
                    Some(info) => {
                        println!(
                            "{}",
                            ta(
                                "cli-plugin-name-version",
                                &[("name", &info.name), ("version", &info.version)],
                                "Plugin"
                            )
                        );
                        if let Some(desc) = &info.description {
                            println!(
                                "{}",
                                ta("cli-plugin-description", &[("desc", desc)], "Description")
                            );
                        }
                        println!(
                            "{}",
                            ta(
                                "cli-plugin-capabilities",
                                &[("v", &format!("{:?}", info.capabilities))],
                                "Capabilities"
                            )
                        );
                        println!(
                            "{}",
                            ta(
                                "cli-plugin-permissions",
                                &[("v", &format!("{:?}", info.permissions))],
                                "Permissions"
                            )
                        );
                        for (capability, key) in installed_plugin_config_entries(&host, &info.name)?
                        {
                            println!(
                                "{}",
                                ta(
                                    "cli-plugin-config-entry-key",
                                    &[("capability", &format!("{capability:?}")), ("key", &key),],
                                    "Config entry key"
                                )
                            );
                        }
                        match &info.wasm_path {
                            Some(path) => println!(
                                "{}",
                                ta(
                                    "cli-plugin-wasm",
                                    &[("path", &path.display().to_string())],
                                    "WASM"
                                )
                            ),
                            None => println!(
                                "{}",
                                t("cli-plugin-wasm-none", "WASM: (skill-only plugin)")
                            ),
                        }
                    }
                    None => println!(
                        "{}",
                        ta(
                            "cli-plugin-not-found",
                            &[("name", &name)],
                            "Plugin not found"
                        )
                    ),
                }
                Ok(())
            }
            PluginCommands::Migrate => {
                let target = config.plugins.resolved_plugins_dir();
                let target_str = target.display().to_string();
                let legacy_dirs = crate::config::schema::legacy_plugin_dirs_with_entries(&config);
                let mut total = 0usize;
                for legacy in &legacy_dirs {
                    let moved = clawcrew::plugins::host::migrate_plugins_dir(legacy, &target)?;
                    if moved > 0 {
                        println!(
                            "{}",
                            ta(
                                "cli-plugin-migrated",
                                &[
                                    ("count", &moved.to_string()),
                                    ("path", &legacy.display().to_string()),
                                    ("target", &target_str),
                                ],
                                "Migrated plugins from a legacy location.",
                            )
                        );
                    }
                    total += moved;
                }
                if total == 0 {
                    println!("{}", t("cli-plugin-migrate-none", "Nothing to migrate."));
                }
                Ok(())
            }
        },
    }
}

#[cfg(feature = "agent-runtime")]
fn handle_estop_command(
    config: &Config,
    estop_command: Option<EstopSubcommands>,
    level: Option<EstopLevelArg>,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<()> {
    if !config.security.estop.enabled {
        bail!("Emergency stop is disabled. Enable [security.estop].enabled = true in config.toml");
    }

    let config_dir = config
        .config_path
        .parent()
        .context("Config path must have a parent directory")?;
    let mut manager = security::EstopManager::load(&config.security.estop, config_dir)?;

    match estop_command {
        Some(EstopSubcommands::Status) => {
            print_estop_status(&manager.status());
            Ok(())
        }
        Some(EstopSubcommands::Resume {
            network,
            domains,
            tools,
            otp,
        }) => {
            let selector = build_resume_selector(network, domains, tools)?;
            let mut otp_code = otp;
            let otp_validator = if config.security.estop.require_otp_to_resume {
                if !config.security.otp.enabled {
                    bail!(
                        "security.estop.require_otp_to_resume=true but security.otp.enabled=false"
                    );
                }
                if otp_code.is_none() {
                    let entered = secret_prompt("Enter OTP code", false)?;
                    if !entered.is_empty() {
                        eprintln!("{}", ta("cli-otp-received", &[], "  ✓ OTP received"));
                    }
                    otp_code = Some(entered);
                }

                let store = security::SecretStore::new(config_dir, config.secrets.encrypt);
                let (validator, enrollment_uri) =
                    security::OtpValidator::from_config(&config.security.otp, config_dir, &store)?;
                if let Some(uri) = enrollment_uri {
                    println!(
                        "{}",
                        t(
                            "cli-otp-initialized",
                            "Initialized OTP secret for ClawCrew."
                        )
                    );
                    println!(
                        "{}",
                        ta("cli-otp-enrollment-uri", &[("uri", &uri)], "Enrollment URI")
                    );
                }
                Some(validator)
            } else {
                None
            };

            manager.resume(selector, otp_code.as_deref(), otp_validator.as_ref())?;
            println!("{}", t("cli-estop-resume-done", "Estop resume completed."));
            print_estop_status(&manager.status());
            Ok(())
        }
        None => {
            let engage_level = build_engage_level(level, domains, tools)?;
            manager.engage(engage_level)?;
            println!("{}", t("cli-estop-engaged", "Estop engaged."));
            print_estop_status(&manager.status());
            Ok(())
        }
    }
}

#[cfg(feature = "agent-runtime")]
fn build_engage_level(
    level: Option<EstopLevelArg>,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<security::EstopLevel> {
    let requested = level.unwrap_or(EstopLevelArg::KillAll);
    match requested {
        EstopLevelArg::KillAll => {
            if !domains.is_empty() || !tools.is_empty() {
                bail!("--domain/--tool are only valid with --level domain-block/tool-freeze");
            }
            Ok(security::EstopLevel::KillAll)
        }
        EstopLevelArg::NetworkKill => {
            if !domains.is_empty() || !tools.is_empty() {
                bail!("--domain/--tool are not valid with --level network-kill");
            }
            Ok(security::EstopLevel::NetworkKill)
        }
        EstopLevelArg::DomainBlock => {
            if domains.is_empty() {
                bail!("--level domain-block requires at least one --domain");
            }
            if !tools.is_empty() {
                bail!("--tool is not valid with --level domain-block");
            }
            Ok(security::EstopLevel::DomainBlock(domains))
        }
        EstopLevelArg::ToolFreeze => {
            if tools.is_empty() {
                bail!("--level tool-freeze requires at least one --tool");
            }
            if !domains.is_empty() {
                bail!("--domain is not valid with --level tool-freeze");
            }
            Ok(security::EstopLevel::ToolFreeze(tools))
        }
    }
}

#[cfg(feature = "agent-runtime")]
fn build_resume_selector(
    network: bool,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<security::ResumeSelector> {
    let selected =
        usize::from(network) + usize::from(!domains.is_empty()) + usize::from(!tools.is_empty());
    if selected > 1 {
        bail!("Use only one of --network, --domain, or --tool for estop resume");
    }
    if network {
        return Ok(security::ResumeSelector::Network);
    }
    if !domains.is_empty() {
        return Ok(security::ResumeSelector::Domains(domains));
    }
    if !tools.is_empty() {
        return Ok(security::ResumeSelector::Tools(tools));
    }
    Ok(security::ResumeSelector::KillAll)
}

#[cfg(feature = "agent-runtime")]
fn print_estop_status(state: &security::EstopState) {
    println!("{}", t("cli-estop-status", "Estop status:"));
    println!(
        "  engaged:        {}",
        if state.is_engaged() { "yes" } else { "no" }
    );
    println!(
        "  kill_all:       {}",
        if state.kill_all { "active" } else { "inactive" }
    );
    println!(
        "  network_kill:   {}",
        if state.network_kill {
            "active"
        } else {
            "inactive"
        }
    );
    if state.blocked_domains.is_empty() {
        println!(
            "{}",
            t("cli-estop-domains-none", "  domain_blocks:  (none)")
        );
    } else {
        println!(
            "{}",
            ta(
                "cli-estop-domains",
                &[("v", &state.blocked_domains.join(", "))],
                "domain_blocks"
            )
        );
    }
    if state.frozen_tools.is_empty() {
        println!("{}", t("cli-estop-tools-none", "  tool_freeze:    (none)"));
    } else {
        println!(
            "{}",
            ta(
                "cli-estop-tools",
                &[("v", &state.frozen_tools.join(", "))],
                "tool_freeze"
            )
        );
    }
    if let Some(updated_at) = &state.updated_at {
        println!(
            "{}",
            ta(
                "cli-estop-updated-at",
                &[("v", &updated_at.to_string())],
                "updated_at"
            )
        );
    }
}

fn write_shell_completion<W: Write>(shell: CompletionShell, writer: &mut W) -> Result<()> {
    use clap_complete::generate;
    use clap_complete::shells;

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();

    match shell {
        CompletionShell::Bash => {
            generate(shells::Bash, &mut cmd, bin_name.clone(), writer);
            // Wrap clap's _clawcrew to inject dynamic config path completion
            writeln!(
                writer,
                r#"
# Dynamic completion for clawcrew config get/set paths
if type _clawcrew &>/dev/null; then
    # Capture the original clap-generated function body so the wrapper
    # can fall back to it without entering an infinite recursion loop.
    eval "$(declare -f _clawcrew | sed '1s/_clawcrew/_clawcrew_clap_orig/')"
    _clawcrew() {{
        local cur="${{COMP_WORDS[COMP_CWORD]}}"
        if [[ "${{COMP_WORDS[*]}}" =~ "config "(get|set)" " ]]; then
            COMPREPLY=($(compgen -W "$(clawcrew config complete "$cur" 2>/dev/null)" -- "$cur"))
            return
        fi
        _clawcrew_clap_orig "$@"
    }}
fi"#
            )?;
        }
        CompletionShell::Fish => {
            generate(shells::Fish, &mut cmd, bin_name.clone(), writer);
            writeln!(
                writer,
                r#"
# Dynamic completion for clawcrew config get/set paths
complete -c clawcrew -n '__fish_seen_subcommand_from config; and __fish_seen_subcommand_from get set' \
    -a '(clawcrew config complete (commandline -ct) 2>/dev/null)' -f"#
            )?;
        }
        CompletionShell::Zsh => {
            generate(shells::Zsh, &mut cmd, bin_name.clone(), writer);
            // Wrap clap's _clawcrew to inject dynamic config path completion
            writeln!(
                writer,
                r#"
# Dynamic completion for clawcrew config get/set paths
if (( $+functions[_clawcrew] )); then
    functions[_clawcrew_clap_orig]=$functions[_clawcrew]
    _clawcrew() {{
        if [[ "${{words[*]}}" == *"config "(get|set)* ]] && (( CURRENT > 3 )); then
            local -a props
            props=(${{(f)"$(clawcrew config complete "$words[CURRENT]" 2>/dev/null)"}})
            compadd -a props
            return
        fi
        _clawcrew_clap_orig "$@"
    }}
fi"#
            )?;
        }
        CompletionShell::PowerShell => {
            generate(shells::PowerShell, &mut cmd, bin_name.clone(), writer);
        }
        CompletionShell::Elvish => generate(shells::Elvish, &mut cmd, bin_name, writer),
    }

    writer.flush()?;
    Ok(())
}

// ─── Gateway helper functions ───────────────────────────────────────────────

/// Resolve gateway host and port from CLI args or config.
#[cfg(feature = "agent-runtime")]
fn resolve_gateway_addr(config: &Config, port: Option<u16>, host: Option<String>) -> (u16, String) {
    let port = port.unwrap_or(config.gateway.port);
    let host = host.unwrap_or_else(|| config.gateway.host.clone());
    (port, host)
}

/// Log gateway startup message.
#[cfg(feature = "agent-runtime")]
fn log_gateway_start(host: &str, port: u16) {
    if port == 0 {
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"host": host})),
            "🚀 Starting ClawCrew Gateway on (random port)"
        );
    } else {
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"host": host, "port": port})),
            "🚀 Starting ClawCrew Gateway on"
        );
    }
}

/// Gracefully shutdown a running gateway via the admin endpoint.
#[cfg(feature = "agent-runtime")]
async fn shutdown_gateway(host: &str, port: u16, path_prefix: Option<&str>) -> Result<()> {
    let url = gateway_admin_url(host, port, path_prefix, "/admin/shutdown");
    let client = reqwest::Client::new();

    match client
        .post(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => Ok(()),
        Ok(response) => {
            let status = response.status();
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"endpoint": url, "status": status.as_u16()})),
                "gateway admin shutdown returned non-success status"
            );
            Err(anyhow::Error::msg(format!(
                "Gateway responded with status: {status}"
            )))
        }
        Err(e) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"endpoint": url, "error": format!("{}", e)})),
                "gateway admin shutdown: connect failed"
            );
            Err(anyhow::Error::msg(format!(
                "Failed to connect to gateway: {e}"
            )))
        }
    }
}

/// Dispatch the gateway-backed SOP verbs. Requires the `agent-runtime` build (the
/// gateway HTTP client + `gateway_admin_url` live behind it, like `shutdown_gateway`);
/// without it these verbs cannot reach the daemon, so they error clearly.
#[cfg(feature = "agent-runtime")]
async fn sop_admin_dispatch(cmd: SopCommands, config: &crate::config::Config) -> Result<()> {
    sop_admin_request(cmd, config).await
}

/// CLI -> daemon dispatch for the out-of-band SOP approval verbs (EPIC C, C8).
/// Posts to `/admin/sop/*` on the running gateway (mirrors `shutdown_gateway`);
/// never builds a throwaway local engine, which cannot see the daemon's runs.
#[cfg(feature = "agent-runtime")]
async fn sop_admin_request(cmd: SopCommands, config: &crate::config::Config) -> Result<()> {
    let host = config.gateway.host.clone();
    let port = config.gateway.port;
    let prefix = config.gateway.path_prefix.as_deref();
    let client = reqwest::Client::new();
    match cmd {
        SopCommands::Pending => {
            let url = gateway_admin_url(&host, port, prefix, "/admin/sop/pending");
            let resp = client
                .get(&url)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
                .map_err(|e| anyhow::Error::msg(format!("Failed to connect to gateway: {e}")))?;
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                let err = body
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("request failed");
                anyhow::bail!("Gateway responded {status}: {err}");
            }
            let pending = body
                .get("pending")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            if pending.is_empty() {
                println!(
                    "{}",
                    t("cli-sop-pending-none", "No SOP runs waiting for approval.")
                );
            } else {
                println!(
                    "{}",
                    t("cli-sop-pending-header", "SOP runs waiting for approval:")
                );
                for r in pending {
                    let run_id = r.get("run_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let sop_name = r.get("sop_name").and_then(|v| v.as_str()).unwrap_or("?");
                    let step = r
                        .get("step")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                        .to_string();
                    let total = r
                        .get("total_steps")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                        .to_string();
                    println!(
                        "{}",
                        ta(
                            "cli-sop-pending-row",
                            &[
                                ("run_id", run_id),
                                ("sop_name", sop_name),
                                ("step", &step),
                                ("total", &total),
                            ],
                            "  (sop run)",
                        )
                    );
                }
            }
            Ok(())
        }
        SopCommands::Approve { run_id } => {
            let url = gateway_admin_url(&host, port, prefix, "/admin/sop/approve");
            sop_admin_post(&client, &url, serde_json::json!({ "run_id": run_id })).await
        }
        SopCommands::Deny { run_id, reason } => {
            let url = gateway_admin_url(&host, port, prefix, "/admin/sop/deny");
            sop_admin_post(
                &client,
                &url,
                serde_json::json!({ "run_id": run_id, "reason": reason }),
            )
            .await
        }
        // List/Validate/Show are dispatched on the local synchronous path.
        _ => anyhow::bail!("local SOP verb reached the gateway dispatch path"),
    }
}

/// POST a JSON body to a gateway SOP admin endpoint and report the outcome.
#[cfg(feature = "agent-runtime")]
async fn sop_admin_post(
    client: &reqwest::Client,
    url: &str,
    body: serde_json::Value,
) -> Result<()> {
    let resp = client
        .post(url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow::Error::msg(format!("Failed to connect to gateway: {e}")))?;
    let status = resp.status();
    let out: serde_json::Value = resp.json().await.unwrap_or_default();
    if status.is_success() {
        println!(
            "{}",
            out.get("outcome").and_then(|v| v.as_str()).unwrap_or("ok")
        );
        Ok(())
    } else {
        // Non-2xx bodies from the SOP routes carry the typed `outcome` label
        // (e.g. not_waiting -> 404, rejected_self_approval -> 403), not `error`;
        // prefer it so the operator sees why, falling back to `error`.
        let detail = out
            .get("outcome")
            .and_then(|v| v.as_str())
            .or_else(|| out.get("error").and_then(|v| v.as_str()))
            .unwrap_or("request failed");
        anyhow::bail!("Gateway responded {status}: {detail}");
    }
}

#[cfg(feature = "agent-runtime")]
enum PaircodeAction {
    /// GET the current code; do not mint or revoke anything.
    Show,
    /// Issue a fresh code for an additional client; revoke nothing.
    AddClient,
    /// Revoke every paired token + clear the registry, then issue a code.
    RotateAll,
    /// Revoke a single device's token, then issue a code.
    RotateDevice(String),
}

#[cfg(feature = "agent-runtime")]
impl PaircodeAction {
    /// True when the action mints a new code (POST), false for `Show` (GET).
    fn mints_code(&self) -> bool {
        !matches!(self, PaircodeAction::Show)
    }

    /// True when the action revokes existing tokens.
    fn is_rotation(&self) -> bool {
        matches!(
            self,
            PaircodeAction::RotateAll | PaircodeAction::RotateDevice(_)
        )
    }

    /// The `rotate` query value to send, if any.
    fn rotate_query(&self) -> Option<String> {
        match self {
            PaircodeAction::RotateAll => Some("all".to_string()),
            PaircodeAction::RotateDevice(id) => Some(id.clone()),
            PaircodeAction::Show | PaircodeAction::AddClient => None,
        }
    }
}

/// Outcome of a `get-paircode` request.
#[cfg(feature = "agent-runtime")]
enum PaircodeResult {
    /// A code was returned (with an optional human-readable message).
    Code {
        code: String,
        message: Option<String>,
    },
    /// No code is available (with an optional explanatory message from the
    /// gateway, e.g. a revoke that succeeded but could not issue a code).
    NoCode { message: Option<String> },
}

#[cfg(feature = "agent-runtime")]
async fn fetch_paircode(
    host: &str,
    port: u16,
    path_prefix: Option<&str>,
    action: &PaircodeAction,
) -> Result<PaircodeResult> {
    let client = reqwest::Client::new();

    let response = if action.mints_code() {
        let mut url = gateway_admin_url(host, port, path_prefix, "/admin/paircode/new");
        if let Some(rotate) = action.rotate_query() {
            url.push_str("?rotate=");
            url.push_str(&urlencoding::encode(&rotate));
        }
        client
            .post(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
    } else {
        let url = gateway_admin_url(host, port, path_prefix, "/admin/paircode");
        client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
    };

    let response = response.map_err(|e| {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
            "gateway paircode fetch: connect failed"
        );
        anyhow::Error::msg(format!("Failed to connect to gateway: {e}"))
    })?;

    let status = response.status();
    let json: serde_json::Value = response.json().await.map_err(|e| {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                .with_attrs(
                    ::serde_json::json!({"error": format!("{}", e), "status": status.as_u16()})
                ),
            "gateway paircode response: JSON parse failed"
        );
        anyhow::Error::msg(format!("Gateway responded with status {status}: {e}"))
    })?;

    let message = json
        .get("message")
        .and_then(|v| v.as_str())
        .map(String::from);

    if json.get("success").and_then(|v| v.as_bool()) != Some(true) {
        if !status.is_success() {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"status": status.as_u16()})),
                "gateway paircode fetch returned non-success status"
            );
        }
        return Ok(PaircodeResult::NoCode { message });
    }

    match json.get("pairing_code").and_then(|v| v.as_str()) {
        Some(code) => Ok(PaircodeResult::Code {
            code: code.to_string(),
            message,
        }),
        None => Ok(PaircodeResult::NoCode { message }),
    }
}

#[cfg(feature = "agent-runtime")]
fn gateway_admin_url(host: &str, port: u16, path_prefix: Option<&str>, admin_path: &str) -> String {
    let prefix = path_prefix.unwrap_or("");
    format!("http://{host}:{port}{prefix}{admin_path}")
}

#[cfg(feature = "agent-runtime")]
fn paircode_no_code_message(
    host: &str,
    port: u16,
    default_host: &str,
    default_port: u16,
    action: &PaircodeAction,
    require_pairing: bool,
    gateway_message: Option<&str>,
) -> String {
    let mut lines = Vec::new();

    if let Some(message) = gateway_message.filter(|m| !m.trim().is_empty()) {
        lines.push(format!("⚠️  {message}"));
    } else if require_pairing {
        lines.push(t(
            "cli-pairing-no-code",
            "🔐 Gateway pairing is enabled, but no active pairing code is available.",
        ));
    } else {
        lines.push(t(
            "cli-pairing-disabled",
            "⚠️  Gateway pairing is disabled in config.",
        ));
        lines.push(t(
            "cli-pairing-requests-accepted",
            "All requests will be accepted without authentication.",
        ));
        lines.push(t(
            "cli-pairing-enable-config",
            "To enable pairing, set [gateway] require_pairing = true.",
        ));
        return indent_paircode_lines(lines);
    }

    lines.push(String::new());
    match action {
        PaircodeAction::Show => {
            lines.push(t(
                "cli-pairing-show-only",
                "`clawcrew gateway get-paircode` only displays an existing active code; it does not mint a new one.",
            ));
            lines.push(t(
                "cli-pairing-pair-another",
                "To pair another device, run:",
            ));
            lines.push(paircode_command(
                host,
                port,
                default_host,
                default_port,
                Some("--new"),
            ));
            lines.push(String::new());
            lines.push(t(
                "cli-pairing-revoke-replace",
                "To revoke existing pairings and mint a replacement code, run:",
            ));
            lines.push(paircode_command(
                host,
                port,
                default_host,
                default_port,
                Some("--rotate"),
            ));
        }
        PaircodeAction::AddClient => {
            lines.push(t(
                "cli-pairing-new-code-unavailable",
                "The gateway did not mint a new pairing code. A code may already be pending, or pairing may need a reset.",
            ));
            lines.push(t(
                "cli-pairing-retry-or-rotate",
                "Try again shortly, or revoke existing pairings and mint a replacement code:",
            ));
            lines.push(paircode_command(
                host,
                port,
                default_host,
                default_port,
                Some("--rotate"),
            ));
        }
        PaircodeAction::RotateAll | PaircodeAction::RotateDevice(_) => {
            lines.push(t(
                "cli-pairing-rotate-no-code",
                "The rotate request completed without returning a replacement code.",
            ));
            lines.push(t(
                "cli-pairing-check-enabled",
                "Check whether pairing is enabled, then request a new device code:",
            ));
            lines.push(paircode_command(
                host,
                port,
                default_host,
                default_port,
                Some("--new"),
            ));
        }
    }

    lines.push(String::new());
    lines.push(t("cli-pairing-inspect", "To inspect the running gateway:"));
    lines.push(format!(
        "    open http://{}:{port}",
        gateway_browser_host(host)
    ));
    indent_paircode_lines(lines)
}

#[cfg(feature = "agent-runtime")]
fn paircode_command(
    host: &str,
    port: u16,
    default_host: &str,
    default_port: u16,
    flag: Option<&str>,
) -> String {
    let mut command = "    clawcrew gateway get-paircode".to_string();
    if let Some(flag) = flag {
        command.push(' ');
        command.push_str(flag);
    }
    if port != default_port {
        write!(command, " --port {port}").expect("writing to String cannot fail");
    }
    if host != default_host {
        write!(command, " --host {host}").expect("writing to String cannot fail");
    }
    command
}

#[cfg(feature = "agent-runtime")]
fn indent_paircode_lines(lines: Vec<String>) -> String {
    lines
        .into_iter()
        .map(|line| {
            if line.starts_with("    ") || line.is_empty() {
                line
            } else {
                format!("  {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// Interactive CLI input helpers used by `auth paste-token` /
// `auth setup-token` / `auth paste-redirect`. The dialoguer dep belongs
// to the binary; auth/mod.rs in clawcrew-providers shouldn't pull it in,
// so reads live here and trait flows accept the resulting string.

#[cfg(feature = "agent-runtime")]
fn read_auth_input(prompt: &str) -> Result<String> {
    let input = secret_prompt(prompt, false)?;
    if !input.is_empty() {
        eprintln!("{}", ta("cli-secret-received", &[], "  ✓ Secret received"));
    }
    Ok(input.trim().to_string())
}

#[cfg(feature = "agent-runtime")]
fn read_plain_input(prompt: &str) -> Result<String> {
    let input: String = cli_input::Input::new()
        .with_prompt(prompt)
        .interact_text()?;
    Ok(input.trim().to_string())
}

#[cfg(feature = "agent-runtime")]
fn format_expiry(profile: &auth::profiles::AuthProfile) -> String {
    match profile
        .token_set
        .as_ref()
        .and_then(|token_set| token_set.expires_at)
    {
        Some(ts) => {
            let now = chrono::Utc::now();
            if ts <= now {
                format!("expired at {}", ts.to_rfc3339())
            } else {
                let mins = (ts - now).num_minutes();
                format!("expires in {mins}m ({})", ts.to_rfc3339())
            }
        }
        None => "n/a".to_string(),
    }
}

#[cfg(feature = "agent-runtime")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum InlineProviderAuth {
    Codex,
    AnthropicSetupToken { alias: String },
}

#[cfg(feature = "agent-runtime")]
fn quickstart_field_value_eq(
    fields: &std::collections::HashMap<String, String>,
    key: &str,
    expected: &str,
) -> bool {
    fields
        .get(key)
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(expected))
}

#[cfg(feature = "agent-runtime")]
fn quickstart_inline_auth(
    kind: &str,
    alias: &str,
    fields: &std::collections::HashMap<String, String>,
) -> Option<InlineProviderAuth> {
    if kind == "openai" && quickstart_field_value_eq(fields, "auth_mode", "codex") {
        return Some(InlineProviderAuth::Codex);
    }
    if kind == "anthropic" && quickstart_field_value_eq(fields, "auth_mode", "setup_token") {
        return Some(InlineProviderAuth::AnthropicSetupToken {
            alias: alias.to_string(),
        });
    }
    None
}

/// `~/.codex/auth.json` — the credential file the upstream Codex CLI writes.
/// When present, offer a direct import instead of starting a fresh browser flow.
#[cfg(feature = "agent-runtime")]
fn codex_auth_json_path() -> Option<std::path::PathBuf> {
    directories::UserDirs::new().map(|u| u.home_dir().join(".codex").join("auth.json"))
}

#[cfg(feature = "agent-runtime")]
async fn run_inline_provider_auth(auth: InlineProviderAuth, config: &mut Config) {
    use dialoguer::Confirm;

    let codex_import = match &auth {
        InlineProviderAuth::Codex => codex_auth_json_path().filter(|path| path.exists()),
        InlineProviderAuth::AnthropicSetupToken { .. } => None,
    };
    let (prompt, skip_hint) = match &auth {
        InlineProviderAuth::Codex => (
            if codex_import.is_some() {
                t(
                    "cli-quickstart-auth-codex-import-prompt",
                    "Found an existing Codex login (~/.codex/auth.json) — import it now?",
                )
            } else {
                t(
                    "cli-quickstart-auth-codex-prompt",
                    "Sign in to OpenAI Codex with your ChatGPT account now?",
                )
            },
            t(
                "cli-quickstart-auth-codex-skip-hint",
                "  Finish later with: clawcrew auth login --model-provider openai-codex",
            ),
        ),
        InlineProviderAuth::AnthropicSetupToken { alias } => (
            ta(
                "cli-quickstart-auth-anthropic-prompt",
                &[("alias", alias)],
                "Run `claude setup-token` for this Anthropic provider now?",
            ),
            ta(
                "cli-quickstart-auth-anthropic-skip-hint",
                &[("alias", alias)],
                "  Finish later with: claude setup-token",
            ),
        ),
    };
    if !Confirm::new()
        .with_prompt(prompt)
        .default(true)
        .interact()
        .unwrap_or(false)
    {
        println!("{skip_hint}");
        return;
    }

    let result = match auth {
        InlineProviderAuth::Codex => {
            let cmd = AuthCommands::Login {
                model_provider: "openai-codex".to_string(),
                profile: "default".to_string(),
                device_code: false,
                import: codex_import,
            };
            handle_auth_command(cmd, config).await
        }
        InlineProviderAuth::AnthropicSetupToken { alias } => {
            Box::pin(run_anthropic_setup_token_inline(&alias, config)).await
        }
    };
    if let Err(error) = result {
        let error = error.to_string();
        eprintln!(
            "{}",
            ta(
                "cli-quickstart-auth-failed",
                &[("error", &error)],
                "  Auth setup didn't complete.",
            )
        );
        println!("{skip_hint}");
    }
}

#[cfg(feature = "agent-runtime")]
async fn run_anthropic_setup_token_inline(alias: &str, config: &mut Config) -> Result<()> {
    let status = tokio::process::Command::new("claude")
        .arg("setup-token")
        .status()
        .await
        .context("failed to run `claude setup-token`; is the Claude CLI installed and on PATH?")?;
    if !status.success() {
        bail!("`claude setup-token` exited with status {status}");
    }

    let token = read_auth_input(&t(
        "cli-quickstart-auth-anthropic-token-prompt",
        "Paste the token from `claude setup-token`",
    ))?;
    if token.trim().is_empty() {
        bail!("Token cannot be empty");
    }

    let path = format!("providers.models.anthropic.{alias}.api_key");
    config.set_prop_persistent(&path, token.trim())?;
    Box::pin(config.save_dirty()).await?;
    println!(
        "{}",
        ta(
            "cli-quickstart-auth-anthropic-saved",
            &[("alias", alias)],
            "  Saved Claude setup token.",
        )
    );
    Ok(())
}

#[allow(clippy::too_many_lines)]
#[cfg(feature = "agent-runtime")]
async fn handle_auth_command(auth_command: AuthCommands, config: &Config) -> Result<()> {
    let auth_service = auth::AuthService::from_config(config);
    let auth_cli_formatter =
        |key: &str, args: &[(&str, &str)], fallback: &str| ta(key, args, fallback);

    match auth_command {
        AuthCommands::Login {
            model_provider,
            profile,
            device_code,
            import,
        } => {
            let provider: auth::AuthProvider = model_provider.parse()?;
            let client = reqwest::Client::new();
            let ctx = auth::AuthFlowContext {
                config,
                auth_service: &auth_service,
                client: &client,
                format_cli: &auth_cli_formatter,
            };
            provider
                .flow()
                .login(&ctx, &profile, device_code, import.as_deref())
                .await
        }

        AuthCommands::PasteRedirect {
            model_provider,
            profile,
            input,
        } => {
            let provider: auth::AuthProvider = model_provider.parse()?;
            let client = reqwest::Client::new();
            let ctx = auth::AuthFlowContext {
                config,
                auth_service: &auth_service,
                client: &client,
                format_cli: &auth_cli_formatter,
            };
            let input_str: Option<String> = match input {
                Some(value) => Some(value),
                None => Some(read_plain_input("Paste redirect URL or OAuth code")?),
            };
            provider
                .flow()
                .paste_redirect(&ctx, &profile, input_str.as_deref())
                .await
        }

        AuthCommands::PasteToken {
            model_provider,
            profile,
            token,
            auth_kind,
        } => {
            let model_provider = auth::normalize_model_provider(&model_provider)?;
            let token = match token {
                Some(token) => token.trim().to_string(),
                None => read_auth_input("Paste token")?,
            };
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, auth_kind.as_deref());
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_model_provider_token(&model_provider, &profile, &token, metadata, true)
                .await?;
            println!(
                "{}",
                ta("cli-auth-saved", &[("profile", &profile)], "Saved profile")
            );
            println!(
                "{}",
                ta(
                    "cli-auth-active-for",
                    &[("provider", &model_provider), ("profile", &profile)],
                    "Active profile"
                )
            );
            Ok(())
        }

        AuthCommands::SetupToken {
            model_provider,
            profile,
        } => {
            let model_provider = auth::normalize_model_provider(&model_provider)?;
            let token = read_auth_input("Paste token")?;
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, Some("authorization"));
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_model_provider_token(&model_provider, &profile, &token, metadata, true)
                .await?;
            println!(
                "{}",
                ta("cli-auth-saved", &[("profile", &profile)], "Saved profile")
            );
            println!(
                "{}",
                ta(
                    "cli-auth-active-for",
                    &[("provider", &model_provider), ("profile", &profile)],
                    "Active profile"
                )
            );
            Ok(())
        }

        AuthCommands::Refresh {
            model_provider,
            profile,
        } => {
            let provider: auth::AuthProvider = model_provider.parse()?;
            let client = reqwest::Client::new();
            let ctx = auth::AuthFlowContext {
                config,
                auth_service: &auth_service,
                client: &client,
                format_cli: &auth_cli_formatter,
            };
            let status = provider
                .flow()
                .refresh_status(&ctx, profile.as_deref())
                .await?;
            match status {
                auth::RefreshStatus::Refreshed { profile } => {
                    println!(
                        "{}",
                        ta(
                            "cli-auth-refresh-ok",
                            &[("profile", &profile)],
                            "Token refresh OK"
                        )
                    );
                    Ok(())
                }
                auth::RefreshStatus::NoProfile => {
                    bail!(
                        "No auth profile found. Run `clawcrew auth login --model-provider <provider>` first.",
                    )
                }
            }
        }

        AuthCommands::Logout {
            model_provider,
            profile,
        } => {
            let model_provider = auth::normalize_model_provider(&model_provider)?;
            let removed = auth_service
                .remove_profile(&model_provider, &profile)
                .await?;
            if removed {
                println!(
                    "{}",
                    ta(
                        "cli-auth-removed",
                        &[("provider", &model_provider), ("profile", &profile)],
                        "Removed auth profile"
                    )
                );
            } else {
                println!(
                    "{}",
                    ta(
                        "cli-auth-not-found",
                        &[("provider", &model_provider), ("profile", &profile)],
                        "Auth profile not found"
                    )
                );
            }
            Ok(())
        }

        AuthCommands::Use {
            model_provider,
            profile,
        } => {
            let model_provider = auth::normalize_model_provider(&model_provider)?;
            auth_service
                .set_active_profile(&model_provider, &profile)
                .await?;
            println!(
                "{}",
                ta(
                    "cli-auth-active-for",
                    &[("provider", &model_provider), ("profile", &profile)],
                    "Active profile"
                )
            );
            Ok(())
        }

        AuthCommands::List => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("{}", t("cli-auth-none", "No auth profiles configured."));
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.model_provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!("{marker} {id}");
            }

            Ok(())
        }

        AuthCommands::Status => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("{}", t("cli-auth-none", "No auth profiles configured."));
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.model_provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!(
                    "{} {} kind={:?} account={} expires={}",
                    marker,
                    id,
                    profile.kind,
                    crate::security::redact(profile.account_id.as_deref().unwrap_or("unknown")),
                    format_expiry(profile)
                );
            }

            println!();
            println!("{}", t("cli-auth-active", "Active profiles:"));
            for (model_provider, profile_id) in &data.active_profiles {
                println!("  {model_provider}: {profile_id}");
            }

            Ok(())
        }

        AuthCommands::EmailLogin { channel, profile } => {
            let email_cfg = config.channels.email.get(&channel).ok_or_else(|| {
                anyhow::Error::msg(format!(
                    "No [channels.email.{channel}] block found in config. \
                     Add the block with an [channels.email.{channel}.oauth2] section first."
                ))
            })?;

            let oauth2 = email_cfg.oauth2.as_ref().ok_or_else(|| anyhow::Error::msg(format!(
                "[channels.email.{channel}] exists but has no [channels.email.{channel}.oauth2] block."
            )))?;

            let client = reqwest::Client::new();
            let device = auth::email_oauth2::start_device_code_flow(
                &client,
                &oauth2.device_code_url,
                &oauth2.client_id,
                &oauth2.scopes,
            )
            .await?;

            println!("Email OAuth2 device-code login started."); // i18n-exempt: interactive device-code CLI prompt
            println!("Visit:  {}", device.verification_uri); // i18n-exempt: interactive device-code CLI prompt
            println!("Code:   {}", device.user_code); // i18n-exempt: interactive device-code CLI prompt
            if let Some(ref uri) = device.verification_uri_complete {
                println!("Or open directly: {uri}"); // i18n-exempt: interactive device-code CLI prompt
            }
            println!("Waiting for authorization…"); // i18n-exempt: interactive device-code CLI prompt

            let token_set = auth::email_oauth2::poll_device_code_tokens(
                &client,
                &oauth2.token_url,
                &oauth2.client_id,
                &device,
            )
            .await?;

            let channel_alias = format!("email.{channel}");
            auth_service
                .store_email_oauth2_tokens(&channel_alias, &profile, token_set)
                .await?;
            println!("Saved profile {profile} for {channel_alias}"); // i18n-exempt: interactive device-code CLI prompt
            Ok(())
        }
    }
}

/// Tell the operator that `vi_verify` is withheld from the model-visible
/// registry while no credential chain verifier exists.
///
/// Called once per config application: at process config load, and again when
/// the daemon reload arm re-reads config from disk. Registry assembly is the
/// wrong home for it, because that runs on ordinary gateway requests and on
/// nested SOP and delegation rebuilds. Each call site must sit after its
/// `runtime_trace::init_from_config`, or the record has no sink.
#[cfg(feature = "agent-runtime")]
fn warn_verifiable_intent_withheld(config: &Config) {
    if !config.verifiable_intent.enabled {
        return;
    }
    ::clawcrew_log::record!(
        WARN,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
            // Operator-facing posture notice, not runtime bookkeeping. An event
            // with no category stores as `internal`, and the dashboard Logs view
            // hides that category by default, so an uncategorised notice is
            // absent from the history an operator actually reads.
            .with_category(::clawcrew_log::EventCategory::System)
            // The config surface reports this same fact as a structured
            // warning. Carrying its code and path here is what lets an operator
            // correlate the two rather than read them as separate problems;
            // `with_attrs` persists them to the trace and serves them from the
            // logs API, which the ephemeral variant would not.
            .with_attrs(::serde_json::json!({
                "code": ::clawcrew_config::validation_warnings::VERIFIABLE_INTENT_TOOL_WITHHELD,
                "path": "verifiable_intent.enabled",
            })),
        "verifiable_intent: vi_verify is not registered as a model-callable tool because no credential chain verifier exists yet (see #9328)"
    );
}

fn running_executable_for_remediation() -> Option<std::path::PathBuf> {
    #[cfg(feature = "agent-runtime")]
    {
        if let Some(executable) = clawcrew_runtime::restart::recorded_launch_executable() {
            return Some(executable.to_path_buf());
        }
        if clawcrew_runtime::restart::launch_command_recorded() {
            return None;
        }
        std::env::current_exe().ok()
    }

    #[cfg(not(feature = "agent-runtime"))]
    {
        std::env::current_exe().ok()
    }
}

#[cfg(feature = "agent-runtime")]
fn gate_security_posture(
    config: &clawcrew::config::Config,
    allow_degraded: bool,
) -> anyhow::Result<Option<tokio::task::JoinHandle<()>>> {
    if config.degraded_security.is_empty() {
        return Ok(None);
    }
    let sections = config.degraded_security.join(", ");
    if !allow_degraded {
        let remediation_executable = running_executable_for_remediation();
        let remediation = remediation_executable.map_or_else(
            || {
                "The running executable path could not be resolved; use a daemon-owned repair \
                 surface such as the gateway config editor instead of an unqualified PATH command."
                    .to_string()
            },
            |exe| {
                format!(
                    "Running executable: {}. Use that executable with `config migrate` to see \
                     the precise error.",
                    exe.display()
                )
            },
        );
        anyhow::bail!(
            "Config contains malformed security-critical sections ({sections}); \
             they were reset to defaults, so the running posture may be weaker \
             than intended. Refusing to serve with a degraded security posture. \
             Repair these sections in {} and restart — {remediation} To boot anyway \
             (e.g. to reach the gateway config editor and repair from there), re-run with \
             `--allow-degraded-security`.",
            config.config_path.display()
        );
    }
    let config_path = config.config_path.display().to_string();
    let handle = ::clawcrew_spawn::spawn!(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            ticker.tick().await;
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({ "degraded_security": sections })),
                &format!(
                    "Running with DEGRADED security: sections ({sections}) were reset to \
                     defaults and `--allow-degraded-security` was set. The posture may be \
                     weaker than intended — repair {config_path} and restart \
                     the process as soon as possible."
                )
            );
        }
    });
    Ok(Some(handle))
}

/// Build the SOP channel-backed adapters from one shared channel map:
/// - the approval ROUTE adapter, so a SOP that parks at a policied gate (or later
///   times out) can deliver its approval request / escalation notice to a real
///   channel (Discord, Slack, ...);
/// - the FORGE-WRITE adapter, so an approved `forge.comment` capability step can
///   post its comment back to the forge by driving the git channel's normal
///   outbound path.
///
/// - the LLM adapter, so an `llm.generate` capability step can run one bounded
///   model call on the default agent's resolved provider.
///
/// Each field is `None` when not applicable (no channels at all; no git channel
/// for the forge half; no resolvable default model provider for the llm half), in
/// which case `build_sop_engine` falls back to the log-only no-op route adapter
/// and the fail-closed `forge.comment` / `llm.generate` placeholders (unchanged
/// behavior). MUST be called from within the tokio runtime: it captures
/// `Handle::current()` so the sync, under-the-engine-lock adapter calls can bridge
/// to the async channel/provider calls.
#[cfg(feature = "agent-runtime")]
fn build_sop_adapters(config: &Config) -> clawcrew_runtime::sop::SopEngineAdapters {
    // `llm.generate` runs on the DEFAULT agent's resolved model provider — the
    // daemon-level model of record. No resolvable provider = fail-closed.
    let llm: Option<std::sync::Arc<dyn clawcrew_runtime::sop::capability::LlmGenerateAdapter>> =
        config
            .resolved_model_provider_for_agent("default")
            .and_then(|(provider_type, alias, entry)| {
                // Alias-aware factory WITH the alias's runtime options: the options
                // carry clawcrew_dir (auth-profile store) and per-alias runtime
                // knobs — without them, OAuth/subscription providers (codex,
                // opencode) sit unauthenticated and never answer. This mirrors the
                // delegate tool's provider construction.
                let options = clawcrew::providers::provider_runtime_options_for_alias(
                    config,
                    provider_type,
                    alias,
                );
                let provider = match clawcrew::providers::create_model_provider_for_alias(
                    config,
                    provider_type,
                    alias,
                    entry.api_key.as_deref(),
                    &options,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({"error": e.to_string()})),
                            "SOP llm.generate adapter unavailable: default model provider failed to build"
                        );
                        return None;
                    }
                };
                let model = entry.model.clone().unwrap_or_else(|| "default".to_string());
                Some(std::sync::Arc::new(
                    clawcrew_runtime::sop::capability::ProviderLlmAdapter::new(
                        std::sync::Arc::from(provider),
                        model,
                    ),
                ) as _)
            });

    let channels = clawcrew_channels::orchestrator::build_channel_map(config);
    // Startup validation: this send-only adapter's channel map omits channels that
    // need runtime SOP handles (e.g. AMQP SOP-dispatch channels). Surface at BOOT any
    // configured approval route whose channel is absent here, so a `request_route` /
    // `escalation_route` that would silently fail to deliver at gate time is caught up
    // front rather than on the first parked gate. This runs BEFORE the empty-map return:
    // when there are no deliverable channels at all, EVERY configured route is
    // undeliverable and must still be surfaced.
    // A route target must be a channel that can actually deliver OUTBOUND; an
    // inbound-only channel (e.g. AMQP, whose `send` is a no-op) in the map cannot send
    // an approval notice, so it is not a resolvable route target.
    let deliverable_keys: std::collections::HashSet<String> = channels
        .iter()
        .filter(|(_, ch)| ch.supports_outbound_send())
        .map(|(key, _)| key.clone())
        .collect();
    for issue in clawcrew_runtime::sop::approval::unresolvable_approval_routes(
        &config.sop.approval,
        &deliverable_keys,
    ) {
        match issue {
            clawcrew_runtime::sop::approval::ApprovalRouteIssue::Malformed {
                policy,
                route_kind,
                route,
            } => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "policy": policy,
                            "route_kind": route_kind,
                            "route": route,
                        })),
                    "SOP approval route is malformed; use the required channel:recipient format"
                );
            }
            clawcrew_runtime::sop::approval::ApprovalRouteIssue::UndeliverableChannel {
                policy,
                route_kind,
                route,
                channel_key,
            } => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "policy": policy,
                            "route_kind": route_kind,
                            "route": route,
                            "channel": channel_key,
                        })),
                    "SOP approval route names a channel the route adapter cannot deliver to; \
                     its approval notices will not be sent (the channel may require runtime SOP \
                     handles this send-only adapter lacks)"
                );
            }
        }
    }
    if channels.is_empty() {
        return clawcrew_runtime::sop::SopEngineAdapters {
            llm,
            ..Default::default()
        };
    }
    let handle = tokio::runtime::Handle::current();
    let route: std::sync::Arc<dyn clawcrew_runtime::sop::approval::ApprovalRouteAdapter> =
        std::sync::Arc::new(clawcrew_runtime::sop::approval::ChannelRouteAdapter::new(
            channels.clone(),
            handle.clone(),
        ));
    // Only offer the forge adapter when a git channel actually exists, so
    // `forge.comment` stays fail-closed on daemons without a forge.
    let has_git = channels.keys().any(|k| k == "git" || k.starts_with("git."));
    let forge: Option<std::sync::Arc<dyn clawcrew_runtime::sop::capability::ForgeCommentAdapter>> =
        has_git.then(|| {
            std::sync::Arc::new(clawcrew_runtime::sop::capability::ChannelForgeAdapter::new(
                channels,
            )) as _
        });
    clawcrew_runtime::sop::SopEngineAdapters {
        route: Some(route),
        forge,
        llm,
    }
}

/// Spawn the periodic SOP maintenance tick (EPIC A1 + SOP cron): on each interval it
/// fires fail-closed approval timeouts, reaps expired concurrency-claim leases,
/// prunes terminal runs past the retention policy, and dispatches cached cron
/// SOP triggers. Returns `None` (no task) when the tick is disabled
/// (`interval_secs == 0`) or no SOP engine is configured. The caller owns the
/// returned handle and aborts it when the foreground daemon/channel run exits.
/// The tick itself self-approves nothing - timeout handling follows
/// `approval_timeout_action` (default `escalate`, fail-closed).
#[cfg(feature = "agent-runtime")]
fn spawn_sop_maintenance(
    sop_engine: Option<&std::sync::Arc<std::sync::Mutex<clawcrew_runtime::sop::SopEngine>>>,
    sop_audit: Option<&std::sync::Arc<clawcrew_runtime::sop::SopAuditLogger>>,
    interval_secs: u64,
) -> Option<tokio::task::JoinHandle<()>> {
    if interval_secs == 0 {
        return None;
    }
    let engine = sop_engine.cloned()?;
    let audit = sop_audit.cloned();
    let cron_cache = audit
        .as_ref()
        .map(|_| clawcrew_runtime::sop::dispatch::SopCronCache::from_engine(&engine));
    Some(::clawcrew_spawn::spawn!(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_cron_check = chrono::Utc::now();
        loop {
            ticker.tick().await;
            let Some(report) = run_sop_maintenance_tick(
                &engine,
                audit.as_ref(),
                cron_cache.as_ref(),
                &mut last_cron_check,
            )
            .await
            else {
                continue;
            };
            if !report.is_empty() {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({
                            "timed_out": report.maintenance.timed_out,
                            "reaped_claims": report.maintenance.reaped_claims,
                            "pruned_runs": report.maintenance.pruned_runs,
                            "cron_started": report.cron_started,
                            "cron_skipped": report.cron_skipped,
                            "cron_no_match": report.cron_no_match,
                        })),
                    "SOP maintenance tick"
                );
            }
        }
    }))
}

#[cfg(feature = "agent-runtime")]
#[derive(Default)]
struct SopMaintenanceTickReport {
    maintenance: clawcrew_runtime::sop::MaintenanceSummary,
    cron_started: usize,
    cron_skipped: usize,
    cron_blocked_unsafe: usize,
    cron_no_match: usize,
}

#[cfg(feature = "agent-runtime")]
impl SopMaintenanceTickReport {
    fn is_empty(&self) -> bool {
        self.maintenance.is_empty()
            && self.cron_started == 0
            && self.cron_skipped == 0
            && self.cron_blocked_unsafe == 0
            && self.cron_no_match == 0
    }
}

#[cfg(feature = "agent-runtime")]
async fn run_sop_maintenance_tick(
    engine: &std::sync::Arc<std::sync::Mutex<clawcrew_runtime::sop::SopEngine>>,
    audit: Option<&std::sync::Arc<clawcrew_runtime::sop::SopAuditLogger>>,
    cron_cache: Option<&clawcrew_runtime::sop::dispatch::SopCronCache>,
    last_cron_check: &mut chrono::DateTime<chrono::Utc>,
) -> Option<SopMaintenanceTickReport> {
    let maintenance = match engine.lock() {
        Ok(mut e) => e.run_maintenance_tick(),
        Err(_) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                "SOP maintenance tick: engine lock poisoned; skipping this pass"
            );
            return None;
        }
    };

    let mut report = SopMaintenanceTickReport {
        maintenance,
        ..SopMaintenanceTickReport::default()
    };

    if let (Some(audit), Some(cache)) = (audit, cron_cache) {
        let results = clawcrew_runtime::sop::dispatch::check_sop_cron_triggers(
            engine,
            audit,
            cache,
            last_cron_check,
        )
        .await;
        for result in &results {
            match result {
                clawcrew_runtime::sop::dispatch::DispatchResult::Started { .. } => {
                    report.cron_started += 1;
                }
                clawcrew_runtime::sop::dispatch::DispatchResult::Skipped { .. }
                | clawcrew_runtime::sop::dispatch::DispatchResult::Deferred { .. }
                | clawcrew_runtime::sop::dispatch::DispatchResult::Coalesced { .. } => {
                    // A2: deferred (backpressure) / coalesced triggers did not start a
                    // run this tick; the cron schedule re-fires them next pass. The
                    // precise outcome is logged by process_headless_results below.
                    report.cron_skipped += 1;
                }
                clawcrew_runtime::sop::dispatch::DispatchResult::BlockedUnsafe { .. } => {
                    report.cron_blocked_unsafe += 1;
                }
                clawcrew_runtime::sop::dispatch::DispatchResult::NoMatch => {
                    report.cron_no_match += 1;
                }
            }
        }
        clawcrew_runtime::sop::dispatch::process_headless_results(&results);
    }

    Some(report)
}

#[cfg(feature = "gateway")]
async fn run_gateway_if_enabled(
    host: &str,
    port: u16,
    config: clawcrew::config::Config,
    tx: Option<tokio::sync::broadcast::Sender<serde_json::Value>>,
) -> anyhow::Result<()> {
    let default_host = config.gateway.host.clone();
    let default_port = config.gateway.port;
    // Capture the launch command before the gateway starts so in-app upgrade
    // can self-respawn after the listener is released. Must mirror the same
    // call in the Daemon branch.
    clawcrew_runtime::restart::record_launch();
    // Standalone gateway (no daemon supervisor): pass None for reload_tx so
    // /admin/reload returns 503 with a clear "no supervisor; restart
    // manually" message, None for tui_registry (no TUI socket), and None
    // for canvas_store so the gateway falls back to its own default.
    let result = Box::pin(gateway::run_gateway(
        host, port, config, tx, None, None, None, None, None, None,
    ))
    .await;
    // Self-respawn after the listener is released, if an in-app upgrade
    // requested it. No-op when no respawn was requested or on supervised
    // restart modes.
    clawcrew_runtime::restart::respawn_if_requested();
    match result {
        Err(err) if is_addr_in_use_error(&err) => {
            let restart_port = available_gateway_restart_hint_port(host, port);
            anyhow::bail!(
                "{}",
                gateway_addr_in_use_message(host, port, &default_host, default_port, restart_port)
            );
        }
        other => other,
    }
}

#[cfg(all(feature = "agent-runtime", not(feature = "gateway")))]
#[allow(clippy::unused_async)]
async fn run_gateway_if_enabled(
    _host: &str,
    _port: u16,
    _config: clawcrew::config::Config,
    _tx: Option<tokio::sync::broadcast::Sender<serde_json::Value>>,
) -> anyhow::Result<()> {
    anyhow::bail!("Gateway feature is not enabled. Rebuild with --features gateway")
}

#[cfg(any(feature = "agent-runtime", test))]
fn is_addr_in_use_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == ErrorKind::AddrInUse)
    })
}

#[cfg(any(feature = "agent-runtime", test))]
fn is_default_gateway_addr(host: &str, port: u16, default_host: &str, default_port: u16) -> bool {
    host == default_host && port == default_port
}

#[cfg(any(feature = "agent-runtime", test))]
fn gateway_browser_host(host: &str) -> &str {
    match host {
        "0.0.0.0" => "127.0.0.1",
        "::" | "[::]" => "[::1]",
        _ => host,
    }
}

#[cfg(any(feature = "agent-runtime", test))]
fn gateway_addr_in_use_message(
    host: &str,
    port: u16,
    default_host: &str,
    default_port: u16,
    restart_port: Option<u16>,
) -> String {
    let mut lines = vec![
        format!("Port {port} is already in use, so the gateway could not start."),
        String::new(),
        "A ClawCrew daemon or another service may already be running on this port.".to_string(),
        "Try one of:".to_string(),
        String::new(),
    ];

    if is_default_gateway_addr(host, port, default_host, default_port) {
        lines.push(format!(
            "    open http://{}:{port}",
            gateway_browser_host(host)
        ));
    }

    lines.push(gateway_paircode_recovery_command(
        host,
        port,
        default_host,
        default_port,
    ));
    if let Some(restart_port) = restart_port {
        lines.push(gateway_restart_recovery_command(
            host,
            restart_port,
            default_host,
        ));
    }
    lines.extend([
        String::new(),
        "To inspect the listener:".to_string(),
        format!("    lsof -nP -iTCP:{port} -sTCP:LISTEN"),
    ]);
    lines.join("\n")
}

#[cfg(any(feature = "agent-runtime", test))]
fn gateway_restart_recovery_command(host: &str, port: u16, default_host: &str) -> String {
    let mut command = format!("    clawcrew gateway start --port {port}");
    if host != default_host {
        write!(command, " --host {host}").expect("writing to String cannot fail");
    }
    command
}

#[cfg(any(feature = "agent-runtime", test))]
fn gateway_paircode_recovery_command(
    host: &str,
    port: u16,
    default_host: &str,
    default_port: u16,
) -> String {
    if host == default_host && port == default_port {
        return "    clawcrew gateway get-paircode".to_string();
    }

    let mut command = format!("    clawcrew gateway get-paircode --port {port}");
    if host != default_host {
        write!(command, " --host {host}").expect("writing to String cannot fail");
    }
    command
}

#[cfg(any(feature = "agent-runtime", test))]
fn available_gateway_restart_hint_port(host: &str, port: u16) -> Option<u16> {
    const SCAN_LIMIT: u16 = 20;

    for offset in 1..=SCAN_LIMIT {
        let Some(candidate) = port.checked_add(offset) else {
            break;
        };
        if std::net::TcpListener::bind(clawcrew_infra::effective_gateway_bind_socket_addr(
            host, candidate,
        ))
        .is_ok()
        {
            return Some(candidate);
        }
    }

    None
}

/// Persist `model` as the default for the first configured provider.
#[cfg(feature = "agent-runtime")]
async fn handle_models_set(config: &mut Config, model: &str) -> Result<()> {
    crate::config::migration::ensure_disk_at_current_version(&config.config_path)?;
    let (type_key, alias) = {
        let entry = config
            .providers
            .models
            .iter_entries()
            .find(|(_, _, entry)| entry.model.as_ref().map_or(false, |m| !m.trim().is_empty()))
            .ok_or_else(|| {
                anyhow::Error::msg(
                    "No model provider configured. Run `clawcrew config init` first.",
                )
            })?;
        (entry.0, entry.1.to_string())
    };
    let prop_path = format!("providers.models.{type_key}.{alias}.model");
    config.set_prop_persistent(&prop_path, model)?;
    Box::pin(config.save_dirty()).await?;
    println!(
        "{}",
        crate::i18n::get_required_cli_string_with_args(
            "cli-models-set-ok",
            &[
                ("model", model),
                ("provider", &format!("{type_key}.{alias}")),
            ]
        )
    );
    Ok(())
}

#[cfg(feature = "agent-runtime")]
async fn dispatch_models_command(model_command: ModelCommands, config: &mut Config) -> Result<()> {
    match model_command {
        ModelCommands::List {
            model_provider,
            check,
        } => doctor::run_configured_models(config, model_provider.as_deref(), check).await,
        ModelCommands::Refresh { model_provider, .. } => {
            doctor::run_models(config, model_provider.as_deref(), false, false).await
        }
        ModelCommands::Set { model } => handle_models_set(config, &model).await,
        ModelCommands::Status => {
            match config
                .providers
                .models
                .iter_entries()
                .find(|(_, _, entry)| entry.model.as_ref().map_or(false, |m| !m.trim().is_empty()))
            {
                Some((ty, alias, entry)) => {
                    let model = entry.model.as_deref().unwrap_or("unknown");
                    println!(
                        "{}",
                        crate::i18n::get_required_cli_string_with_args(
                            "cli-models-status-current",
                            &[("model", model), ("provider", &format!("{ty}.{alias}")),]
                        )
                    );
                }
                None => {
                    println!(
                        "{}",
                        crate::i18n::get_required_cli_string("cli-models-status-none")
                    );
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ops::Range;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::{Duration, Instant};

use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use pulldown_cmark::{Event as MdEvent, Options as MdOptions, Parser as MdParser, Tag, TagEnd};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::attachment::{
    CleanupReport, PendingAttachment, build_attachments_json, cleanup_attachment_temps,
};
use crate::client::{
    ApprovalDecision, RpcClient, RpcNotification, SessionEntry, SessionUpdate, TurnEndOutcome,
    method, parse_session_update,
};
use crate::diff;
use crate::file_explorer::{ExplorerAction, FileExplorerState};
use crate::input_bar::{InputBarAction, InputBarState};
use crate::jsonrpc::RpcOutbound;
use crate::mouse;
#[cfg(test)]
use crate::text_selection::{CellPoint, TextCell as TranscriptCell, row_breaks_for_line};
use crate::text_selection::{
    TextRowBreak as TranscriptRowBreak, TextSelection as TranscriptSelection,
    TextSnapshot as TranscriptSnapshot, borrow_line, row_breaks_for_lines, wrapped_rows,
};
use crate::theme;
use crate::turn_status::TurnStatus;

// Height of the approval popup anchored to the bottom of the content area.
// Used both in render_approval_overlay and to pad diffs so they aren't covered.
const APPROVAL_OVERLAY_HEIGHT: u16 = 7;

/// How often the cwd line re-polls the daemon for the current git branch.
const GIT_BRANCH_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const CANCEL_WATCHDOG: Duration = Duration::from_secs(30);
const COPY_FEEDBACK_TTL: Duration = Duration::from_secs(1);
const SESSION_RECOVERY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const SESSION_RECOVERY_TERMINAL_TIMEOUT: Duration = Duration::from_secs(35);
const SESSION_RECOVERY_MAX_ATTEMPTS: u8 = 4;
const SESSION_RECOVERY_RETRY_BASE: Duration = Duration::from_millis(100);

fn append_cleanup_notice(mut message: String, cleanup: Option<String>) -> String {
    if let Some(cleanup) = cleanup {
        if !message.is_empty() {
            message.push_str("; ");
        }
        message.push_str(&cleanup);
    }
    message
}

// ── Chat pane (tab mode) ─────────────────────────────────────────

enum ChatPhase {
    /// Showing agent picker (or loading the list).
    PickAgent {
        agents: Vec<String>,
        list_state: ListState,
        loading: bool,
    },
    /// Showing saved Code sessions before any new session has been created.
    PickSession {
        sessions: Vec<SessionEntry>,
        list_state: ListState,
        agents: Vec<String>,
    },
    /// WSS only: user picks the remote working directory before session starts.
    PickCwd {
        /// The agent alias already chosen.
        agent_alias: String,
        /// Interactive directory picker.
        explorer: FileExplorerState,
    },
    /// Active chat session.
    Active(Box<ChatState>),
    /// Unrecoverable error.
    Error(String),
}

/// Distinguishes which kind of chat pane this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneKind {
    Chat,
    Acp,
}

/// Why pinning a local Code session to the launch directory failed.
///
/// A local Code session promises that file and shell tools operate on the
/// project zerocode was launched from. If that directory cannot be captured we
/// must not silently fall through to an omitted cwd: `session/new` would then
/// resolve the selected agent's workspace and the session would look healthy
/// while acting on a different project tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalCodeCwdError {
    /// `std::env::current_dir()` failed (e.g. the directory was deleted or is
    /// unreadable). Carries the OS error text.
    Unavailable(String),
    /// The launch directory is not valid UTF-8, so it cannot be represented in
    /// the JSON-RPC `cwd` string. Carries the lossy rendering for diagnosis.
    NotUtf8(String),
}

impl LocalCodeCwdError {
    /// Localized, user-facing text for this capture failure.
    fn localized(&self) -> String {
        match self {
            LocalCodeCwdError::Unavailable(error) => {
                crate::i18n::t_args("zc-chat-code-cwd-unavailable", &[("error", error.as_str())])
            }
            LocalCodeCwdError::NotUtf8(path) => {
                crate::i18n::t_args("zc-chat-code-cwd-not-utf8", &[("path", path.as_str())])
            }
        }
    }
}

/// Process cwd for a fresh local Code session. Chat and remote transports
/// return `Ok(None)` to deliberately omit cwd so the daemon uses the agent
/// workspace or an explicit picker.
///
/// Returns `Err` when a local Code session *should* pin the launch directory
/// but cannot. Callers must surface that error rather than starting a session
/// against a different project.
fn local_code_session_cwd(
    pane_kind: PaneKind,
    transport: crate::client::Transport,
) -> Result<Option<String>, LocalCodeCwdError> {
    if pane_kind == PaneKind::Acp && transport == crate::client::Transport::Local {
        resolve_local_code_cwd(std::env::current_dir()).map(Some)
    } else {
        // Deliberate omission: Chat uses the agent workspace, remote Code uses
        // the directory picker. Neither is a failure.
        Ok(None)
    }
}

/// Pure capture step, split out so tests can inject both failure modes without
/// mutating global process state.
fn resolve_local_code_cwd(
    current_dir: std::io::Result<std::path::PathBuf>,
) -> Result<String, LocalCodeCwdError> {
    let path = current_dir.map_err(|e| LocalCodeCwdError::Unavailable(e.to_string()))?;
    match path.to_str() {
        Some(s) => Ok(s.to_string()),
        None => Err(LocalCodeCwdError::NotUtf8(path.display().to_string())),
    }
}

impl PaneKind {
    /// Short name for this pane (no padding — callers format as needed).
    pub(crate) fn name(self) -> String {
        crate::i18n::t(self.fluent_key())
    }

    /// Stable Fluent key for this pane's display name.
    pub(crate) fn fluent_key(self) -> &'static str {
        match self {
            PaneKind::Chat => "zc-chat-pane-chat",
            PaneKind::Acp => "zc-chat-pane-acp",
        }
    }
}

/// Traffic-light status of one tracked session, derived per frame for the
/// agent sidebar. See [`ChatState::sidebar_status`] for the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarStatus {
    /// Idle, ready for input (green).
    Ready,
    /// Turn in flight (blue).
    Running,
    /// Blocked on an approval or elicitation (yellow).
    NeedsHuman,
    /// Failed turn or lost session (red).
    Errored,
}

/// One sidebar row: a session this pane tracks, in stable creation order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidebarSessionSummary {
    pub session_id: String,
    pub agent_alias: String,
    /// Durable projected conversation-entry count, not visible bubbles.
    pub message_count: usize,
    pub status: SidebarStatus,
    pub pane_kind: PaneKind,
    /// Whether this session is the pane's focused (rendered) session.
    pub focused: bool,
}

/// One session to re-attach after a reconnect rebuild, captured from the
/// pane's tracked set before the socket dropped.
#[derive(Debug, Clone)]
pub(crate) struct ResumeEntry {
    pub session_id: String,
    pub agent_alias: String,
    /// Durable projected conversation-entry count carried until reload.
    pub message_count: usize,
    pub was_focused: bool,
    queue: ReconnectQueueState,
    interrupted: bool,
    recovery_required: bool,
}

/// Client-owned queue and composer state that cannot be reconstructed from the
/// daemon's durable transcript. This is the live `ChatState` data carried
/// across a transport rebuild; transcript and turn state are reloaded from the
/// daemon.
#[derive(Debug, Clone, Default)]
struct ReconnectQueueState {
    messages: VecDeque<QueuedMessage>,
    next_id: u64,
    paused: bool,
    selected: Option<u64>,
    composer_text: String,
    composer_attachments: Vec<PendingAttachment>,
}

/// Why a tracked session shows the red status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionError {
    /// The last turn ended `Failed` (the transcript carries the message).
    TurnFailed,
    /// The daemon no longer knows the session; re-attach before prompting.
    SessionLost,
    /// Notification recovery exhausted its bounded retries. Queued input is
    /// retained and an explicit focus/enqueue action retries reconciliation.
    ResyncFailed,
}

/// Upper bound on sessions one chat-like pane tracks (focused + background).
/// Two panes keep the TUI comfortably under the daemon's 64-session cap.
const MAX_TRACKED_SESSIONS_PER_PANE: usize = 8;

pub(crate) struct Chat {
    rpc: Arc<RpcClient>,
    rpc_out: Arc<RpcOutbound>,
    notif_rx: broadcast::Receiver<RpcNotification>,
    /// Background-fetched git status updates: (session_id, branch, hash).
    git_branch_tx: mpsc::Sender<GitStatusUpdate>,
    git_branch_rx: mpsc::Receiver<GitStatusUpdate>,
    /// In-flight git_branch refresh; gates repeat fetches until result arrives.
    git_branch_inflight: bool,
    /// Background model-catalog fetch result, routed back so the Loading
    /// picker can swap to the populated list without blocking the draw loop.
    model_fetch_tx: mpsc::Sender<ModelFetchResult>,
    model_fetch_rx: mpsc::Receiver<ModelFetchResult>,
    /// Background lost-session re-attachments. The set is the canonical
    /// in-flight operation state and prevents duplicate `session/new` calls;
    /// queued messages remain owned by their `ChatState` until success.
    session_reattach_tx: mpsc::Sender<SessionReattachResult>,
    session_reattach_rx: mpsc::Receiver<SessionReattachResult>,
    session_reattach_in_flight: HashSet<String>,
    /// Durable transcript reloads triggered by a notification-channel lag.
    /// Membership is also the dispatch gate: queued prompts stay owned by the
    /// session but cannot leave while its live view is desynchronized.
    session_resync_tx: mpsc::Sender<SessionResyncResult>,
    session_resync_rx: mpsc::Receiver<SessionResyncResult>,
    session_resync_in_flight: HashSet<String>,
    /// Request-form `session/prompt` completions. Terminal notifications remain
    /// transcript authority; this channel only prevents a lost terminal frame
    /// from leaving the matching local turn stuck in flight.
    prompt_completion_tx: mpsc::Sender<PromptCompletion>,
    prompt_completion_rx: mpsc::Receiver<PromptCompletion>,
    phase: ChatPhase,
    pane_kind: PaneKind,
    /// Live but unfocused sessions of this pane. Each keeps its full
    /// transcript, caches, queue, and pending prompts warm; notifications
    /// route to them by session id so switching back is instant.
    background: Vec<ChatState>,
    /// Sidebar-stable ordering of tracked session ids (creation order).
    /// Reconciled against the live states by `session_summaries`.
    session_order: Vec<String>,
    /// Session to restore focus to when an add/pick flow is cancelled or
    /// fails while background sessions exist.
    last_focused_sid: Option<String>,
    /// One-shot focused session to reattach on the next session start. It also
    /// owns the client-side queue that cannot be recovered from daemon history.
    resume_focused: Option<ResumeEntry>,
    /// Remaining sessions to rehydrate into `background` once the focused
    /// session lands after a reconnect rebuild. Entries that fail to attach
    /// stay here so a later explicit retry or transport reconnect can recover
    /// them without losing their client-owned queues.
    resume_backgrounds: Vec<ResumeEntry>,
    /// List rect of the agent picker, recorded each draw so mouse clicks in the
    /// PickAgent phase can map a row to a selection. Default until first draw.
    pick_agent_list_area: Rect,
    /// Double-click tracker for the agent picker: a second click on the same row
    /// confirms (enters the session), matching the keyboard Enter.
    pick_agent_double_click: crate::mouse::DoubleClickTracker,
    /// Double-click tracker for the session picker: a second click on the same row
    /// resumes that saved session, matching the keyboard Enter.
    session_list_double_click: crate::mouse::DoubleClickTracker,
    /// One-shot app-level Help request, set by the `/help` slash command and
    /// drained immediately by `app.rs` after this pane handles the key.
    help_requested: bool,
    /// Owns the Chat-only entry retry so leaving the pane invalidates its result.
    entry_retry_attempt: Option<EntryRetryAttempt>,
    /// A temporary retry borrows retained queues until the resident pane adopts
    /// it. Neither prompts nor recovery may run while the caller still owns them.
    entry_retry_preparing: bool,
    /// Records whether an active session produced by this pane's entry retry uses
    /// an ID generated by the retry or a stable ID supplied by the caller.
    entry_retry_session_ownership: Option<EntryRetrySessionOwnership>,
}

/// Outcome of attempting to route one inbound `elicitation/create` to the
/// active session. See `Chat::try_install_elicitation`.
pub(crate) enum ElicitationRouting {
    /// Modal installed on the active session; it owns the request id.
    Installed,
    /// Schema/params could not be decoded; caller must answer `cancel`.
    Unparseable(serde_json::Value),
    /// Parsed but does not target the active session yet; retry briefly.
    Defer(crate::client::RpcInboundRequest),
}

/// Result of one background `session/git_branch` poll, routed back to the UI
/// thread over `git_branch_tx`.
struct GitStatusUpdate {
    session_id: String,
    branch: Option<String>,
    hash: Option<String>,
}

/// Result of a background model-catalog fetch, routed back so the Loading
/// picker swaps to the populated list (or surfaces an error) on the draw loop.
struct ModelFetchResult {
    session_id: String,
    model_provider_ref: String,
    models: Vec<String>,
    current: Option<String>,
}

/// Completion of a background lost-session re-attachment. The message queue
/// itself stays in `ChatState`; this carries only the operation result.
struct SessionReattachResult {
    session_id: String,
    result: Result<(), String>,
}

/// Completion of a durable transcript reload after notification loss.
struct SessionResyncResult {
    session_id: String,
    result: Result<SessionResyncSnapshot, String>,
}

/// Canonical daemon state recovered after notification loss. Transcript and
/// plan travel together so the UI cannot display a plan from the abandoned
/// turn beside a newly reloaded transcript.
struct SessionResyncSnapshot {
    messages: Vec<crate::client::MessageEntry>,
    message_count: usize,
    plan: Option<Vec<crate::wire::PlanEntry>>,
}

struct ChatEntryRetryResult {
    chat: Box<Chat>,
    init_outcome: ChatInitOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChatInitOutcome {
    Other,
    NoEnabledAgents,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryRetrySessionOwnership {
    RetryCreated,
    CallerOwned,
}

const ENTRY_RETRY_PRE_SESSION: u8 = 0;
const ENTRY_RETRY_SESSION_CREATION: u8 = 1;
const ENTRY_RETRY_CANCELLED: u8 = 2;
const STALE_SESSION_CLOSE_ATTEMPTS: usize = 3;

struct EntryRetryAttempt {
    result_rx: oneshot::Receiver<ChatEntryRetryResult>,
    worker: tokio::task::JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
    phase: Arc<AtomicU8>,
    rpc: Arc<RpcClient>,
}

impl EntryRetryAttempt {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        // Before session/new, aborting drops the in-flight RPC future and its
        // pending-map guard. Once session/new may have started, let the worker
        // finish cooperatively so a late-created session can be closed. The
        // phase CAS makes this ownership decision exclusive with the worker's
        // session/new claim.
        if claim_entry_retry_cancellation(&self.phase) {
            self.worker.abort();
        }
    }
}

impl Drop for EntryRetryAttempt {
    fn drop(&mut self) {
        self.cancel();
        if let Ok(result) = self.result_rx.try_recv()
            && let Some(session_id) = active_session_id(&result.chat.phase)
        {
            spawn_close_session_if_owned(
                Arc::clone(&self.rpc),
                session_id,
                result.chat.entry_retry_session_ownership,
            );
        }
    }
}

fn claim_entry_retry_session_creation(phase: &AtomicU8) -> bool {
    phase
        .compare_exchange(
            ENTRY_RETRY_PRE_SESSION,
            ENTRY_RETRY_SESSION_CREATION,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
}

fn claim_entry_retry_cancellation(phase: &AtomicU8) -> bool {
    phase
        .compare_exchange(
            ENTRY_RETRY_PRE_SESSION,
            ENTRY_RETRY_CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
}

fn active_session_id(phase: &ChatPhase) -> Option<String> {
    match phase {
        ChatPhase::Active(state) => Some(state.session_id.clone()),
        _ => None,
    }
}

fn spawn_close_session_if_owned(
    rpc: Arc<RpcClient>,
    session_id: String,
    ownership: Option<EntryRetrySessionOwnership>,
) {
    if ownership != Some(EntryRetrySessionOwnership::RetryCreated) {
        return;
    }
    tokio::spawn(async move {
        close_stale_session(&rpc, &session_id).await;
    });
}

async fn close_owned_active_session(
    rpc: &RpcClient,
    phase: &ChatPhase,
    ownership: Option<EntryRetrySessionOwnership>,
) {
    if let Some(session_id) = active_session_id(phase) {
        close_stale_session_if_owned(rpc, &session_id, ownership).await;
    }
}

async fn close_stale_session_if_owned(
    rpc: &RpcClient,
    session_id: &str,
    ownership: Option<EntryRetrySessionOwnership>,
) {
    if ownership == Some(EntryRetrySessionOwnership::RetryCreated) {
        close_stale_session(rpc, session_id).await;
    }
}

async fn close_stale_session(rpc: &RpcClient, session_id: &str) {
    for _ in 0..STALE_SESSION_CLOSE_ATTEMPTS {
        if rpc.session_close(session_id).await.is_ok() {
            return;
        }
    }
}

fn is_cancelled(cancellation: Option<&Arc<AtomicBool>>) -> bool {
    cancellation.is_some_and(|flag| flag.load(Ordering::Acquire))
}

struct PromptCompletion {
    session_id: String,
    turn_generation: u64,
    error: Option<String>,
    transport_closed: bool,
}

/// Map a wire `token_source` value ("provider"/"estimate"/"calibrated") to the
/// Fluent key whose localized label describes that provenance. Unknown values
/// (older or future daemons) fall back to a label-less render.
fn token_source_fluent_key(source: &str) -> String {
    match source {
        "provider" => "zc-chat-history-trimmed-token-source-provider".to_string(),
        "estimate" => "zc-chat-history-trimmed-token-source-estimate".to_string(),
        "calibrated" => "zc-chat-history-trimmed-token-source-calibrated".to_string(),
        other => format!("zc-chat-history-trimmed-token-source-{other}"),
    }
}

fn should_retry_on_entry(phase: &ChatPhase) -> bool {
    matches!(phase, ChatPhase::Error(_) | ChatPhase::PickAgent { .. })
}

impl Chat {
    pub(crate) fn new(rpc: Arc<RpcClient>, pane_kind: PaneKind) -> Self {
        let (git_branch_tx, git_branch_rx) = mpsc::channel(4);
        let (model_fetch_tx, model_fetch_rx) = mpsc::channel(4);
        let (session_reattach_tx, session_reattach_rx) =
            mpsc::channel(MAX_TRACKED_SESSIONS_PER_PANE);
        let (session_resync_tx, session_resync_rx) = mpsc::channel(MAX_TRACKED_SESSIONS_PER_PANE);
        let (prompt_completion_tx, prompt_completion_rx) =
            mpsc::channel(MAX_TRACKED_SESSIONS_PER_PANE);
        Self {
            rpc: rpc.clone(),
            rpc_out: rpc.rpc.clone(),
            notif_rx: rpc.subscribe_notifications(),
            git_branch_tx,
            git_branch_rx,
            git_branch_inflight: false,
            model_fetch_tx,
            model_fetch_rx,
            session_reattach_tx,
            session_reattach_rx,
            session_reattach_in_flight: HashSet::new(),
            session_resync_tx,
            session_resync_rx,
            session_resync_in_flight: HashSet::new(),
            prompt_completion_tx,
            prompt_completion_rx,
            phase: ChatPhase::PickAgent {
                agents: Vec::new(),
                list_state: ListState::default(),
                loading: true,
            },
            pane_kind,
            background: Vec::new(),
            session_order: Vec::new(),
            last_focused_sid: None,
            resume_focused: None,
            resume_backgrounds: Vec::new(),
            pick_agent_list_area: Rect::default(),
            pick_agent_double_click: crate::mouse::DoubleClickTracker::new(),
            session_list_double_click: crate::mouse::DoubleClickTracker::new(),
            help_requested: false,
            entry_retry_attempt: None,
            entry_retry_preparing: false,
            entry_retry_session_ownership: None,
        }
    }

    fn selected_agent_alias(&self) -> Option<String> {
        match &self.phase {
            ChatPhase::PickAgent {
                agents, list_state, ..
            } => list_state.selected().and_then(|i| agents.get(i)).cloned(),
            _ => None,
        }
    }

    /// Seed the sessions to reattach to across a reconnect rebuild. The
    /// `was_focused` entry drives the existing single-session resume path;
    /// the rest rehydrate into `background` once the focused session lands.
    /// One-shot: consumed by the first `start_session`.
    pub(crate) fn set_resume_sessions(&mut self, entries: Vec<ResumeEntry>) {
        self.resume_focused = None;
        self.resume_backgrounds = Vec::new();
        for entry in entries {
            if !self.session_order.contains(&entry.session_id) {
                self.session_order.push(entry.session_id.clone());
            }
            if entry.was_focused && self.resume_focused.is_none() {
                self.resume_focused = Some(entry);
            } else {
                self.resume_backgrounds.push(entry);
            }
        }
        // No entry was flagged focused (e.g. the pane sat in a picker when
        // the socket dropped): resume the first tracked session as focused.
        if self.resume_focused.is_none() && !self.resume_backgrounds.is_empty() {
            let entry = self.resume_backgrounds.remove(0);
            self.resume_focused = Some(entry);
        }
    }

    /// Every session this pane tracks, in sidebar order, for the app layer to
    /// carry across a reconnect rebuild. Durable transcript is intentionally
    /// not copied: the rebuilt pane reloads it from `session/messages`.
    pub(crate) fn resume_entries(&self) -> Vec<ResumeEntry> {
        let summaries = self.session_summaries();
        let mut entries = Vec::with_capacity(
            summaries.len()
                + self.resume_backgrounds.len()
                + usize::from(self.resume_focused.is_some()),
        );
        let mut seen = HashSet::new();
        for summary in summaries {
            let Some(state) = self.state_for_session(&summary.session_id) else {
                continue;
            };
            seen.insert(summary.session_id.clone());
            entries.push(ResumeEntry {
                session_id: summary.session_id,
                agent_alias: summary.agent_alias,
                message_count: state.message_count,
                was_focused: summary.focused,
                queue: state.reconnect_queue_state(),
                interrupted: state.turn_in_flight
                    || state.pending_approval.is_some()
                    || state.pending_elicitation.is_some(),
                recovery_required: state.last_error == Some(SessionError::ResyncFailed)
                    || self.session_resync_in_flight.contains(&state.session_id),
            });
        }

        // Failed resume entries are not live `ChatState`s yet, but they still
        // canonically own client-side queues. Carry them across another socket
        // rebuild as well. A currently live focused session wins focus; the
        // previously focused failed entry becomes a background candidate.
        let mut has_focused = entries.iter().any(|entry| entry.was_focused);
        for retained in self
            .resume_focused
            .iter()
            .chain(self.resume_backgrounds.iter())
        {
            if !seen.insert(retained.session_id.clone()) {
                continue;
            }
            let mut entry = retained.clone();
            if has_focused {
                entry.was_focused = false;
            } else if entry.was_focused {
                has_focused = true;
            }
            entries.push(entry);
        }
        entries
    }

    /// Finalize the old pane only after every replacement pane has built.
    /// Snapshotting resume entries is deliberately read-only, so a failed
    /// rebuild leaves queues and interaction owners intact for the next retry.
    pub(crate) fn commit_reconnect_handoff(&mut self) {
        let rpc = self.rpc.clone();
        let session_ids = self
            .session_summaries()
            .into_iter()
            .map(|summary| summary.session_id)
            .collect::<Vec<_>>();
        for session_id in session_ids {
            let Some(state) = self.state_for_session_mut(&session_id) else {
                continue;
            };
            if let Some(approval) = state.pending_approval.take() {
                Self::reject_stale_approval(&rpc, state.session_id.clone(), approval.request_id);
            }
            if let Some(elicitation) = state.pending_elicitation.take() {
                Self::answer_cancel(&rpc, elicitation.request_id);
            }
            let mut cleanup_report = state.cleanup_active_turn_attachments();
            // Composer clipboard files are transport-local temporaries. Keep
            // them alive until the replacement pane has built successfully,
            // then reclaim them without affecting user-selected files.
            state.input_bar.cleanup_temps();
            cleanup_report.merge(state.input_bar.take_cleanup_report());
            state.surface_cleanup_report(cleanup_report);
        }
    }

    /// One summary per tracked session, in stable creation order, for the
    /// agent sidebar. Cheap: derives from live state, owns nothing.
    /// Terminal status candidates for every live session this pane tracks,
    /// focused or not, paired with the owning agent alias. Background sessions
    /// keep draining transport events each tick, so their state is current.
    pub(crate) fn terminal_statuses(&self) -> Vec<(TurnStatus, String)> {
        let mut out = Vec::with_capacity(self.background.len() + 1);
        if let ChatPhase::Active(state) = &self.phase {
            out.push((state.terminal_status(), state.agent_alias.clone()));
        }
        for state in &self.background {
            out.push((state.terminal_status(), state.agent_alias.clone()));
        }
        out
    }

    pub(crate) fn session_summaries(&self) -> Vec<SidebarSessionSummary> {
        let active = match &self.phase {
            ChatPhase::Active(state) => Some(state.as_ref()),
            _ => None,
        };
        let mut out = Vec::with_capacity(
            self.background.len()
                + self.resume_backgrounds.len()
                + usize::from(self.resume_focused.is_some())
                + 1,
        );
        let summarize = |out: &mut Vec<SidebarSessionSummary>, state: &ChatState, focused: bool| {
            out.push(SidebarSessionSummary {
                session_id: state.session_id.clone(),
                agent_alias: state.agent_alias.clone(),
                message_count: state.message_count,
                status: state.sidebar_status(),
                pane_kind: self.pane_kind,
                focused,
            });
        };
        for sid in &self.session_order {
            if let Some(state) = active.filter(|s| &s.session_id == sid) {
                summarize(&mut out, state, true);
            } else if let Some(state) = self.background.iter().find(|s| &s.session_id == sid) {
                summarize(&mut out, state, false);
            } else if let Some(entry) = self
                .resume_focused
                .iter()
                .chain(self.resume_backgrounds.iter())
                .find(|entry| &entry.session_id == sid)
            {
                out.push(SidebarSessionSummary {
                    session_id: entry.session_id.clone(),
                    agent_alias: entry.agent_alias.clone(),
                    message_count: entry.message_count,
                    status: SidebarStatus::Errored,
                    pane_kind: self.pane_kind,
                    focused: active.is_none() && entry.was_focused,
                });
            }
        }
        // Defensive: surface live sessions that fell out of the order list
        // rather than hiding them.
        for state in &self.background {
            if !self.session_order.contains(&state.session_id) {
                summarize(&mut out, state, false);
            }
        }
        if let Some(state) = active
            && !self.session_order.contains(&state.session_id)
        {
            summarize(&mut out, state, true);
        }
        out
    }

    /// Count every live or retained session exposed through the sidebar.
    fn tracked_session_count(&self) -> usize {
        self.session_summaries().len()
    }

    /// Keep `session_order` current after an in-place session restart
    /// (Ctrl+N replaces the focused session's id) or a restart that left the
    /// pane without a focused session (WSS ACP goes through the CWD picker).
    fn note_session_replaced(&mut self, old_sid: &str) {
        let new_sid = self.current_session_id().map(str::to_string);
        match new_sid {
            Some(new) if new != old_sid => {
                if let Some(slot) = self.session_order.iter_mut().find(|sid| *sid == old_sid) {
                    *slot = new;
                } else if !self.session_order.contains(&new) {
                    self.session_order.push(new);
                }
            }
            // Restart failed and kept the old session: nothing changed.
            Some(_) => {}
            // No focused session (mid CWD pick): the old id is closed.
            None => self.session_order.retain(|sid| sid != old_sid),
        }
    }

    fn state_for_session_mut(&mut self, session_id: &str) -> Option<&mut ChatState> {
        if let ChatPhase::Active(ref mut state) = self.phase
            && state.session_id == session_id
        {
            return Some(state);
        }
        self.background
            .iter_mut()
            .find(|s| s.session_id == session_id)
    }

    fn state_for_session(&self, session_id: &str) -> Option<&ChatState> {
        if let ChatPhase::Active(ref state) = self.phase
            && state.session_id == session_id
        {
            return Some(state);
        }
        self.background.iter().find(|s| s.session_id == session_id)
    }

    /// Park the focused session (if any) in `background`, remembering it as
    /// the restore target. The phase is left as a loading placeholder the
    /// caller immediately replaces (no frame renders mid-handler).
    fn stash_active(&mut self) {
        if !matches!(self.phase, ChatPhase::Active(_)) {
            return;
        }
        let prior = std::mem::replace(
            &mut self.phase,
            ChatPhase::PickAgent {
                agents: Vec::new(),
                list_state: ListState::default(),
                loading: true,
            },
        );
        if let ChatPhase::Active(state) = prior {
            self.last_focused_sid = Some(state.session_id.clone());
            self.background.push(*state);
        }
    }

    /// Bring a tracked session to the front. Returns false when the id is
    /// unknown (e.g. a stale sidebar click racing a close).
    pub(crate) async fn focus_session(&mut self, session_id: &str) -> bool {
        if let ChatPhase::Active(ref state) = self.phase
            && state.session_id == session_id
        {
            self.ensure_session_alive(session_id).await;
            return true;
        }
        if self
            .resume_focused
            .as_ref()
            .is_some_and(|entry| entry.session_id == session_id)
            || self
                .resume_backgrounds
                .iter()
                .any(|entry| entry.session_id == session_id)
        {
            return self.retry_retained_session(session_id).await;
        }
        let Some(idx) = self
            .background
            .iter()
            .position(|s| s.session_id == session_id)
        else {
            return false;
        };
        let incoming = self.background.remove(idx);
        // Swap with the focused session; a non-Active phase (picker, error)
        // is simply replaced — clicking a session row leaves those flows.
        match std::mem::replace(&mut self.phase, ChatPhase::Active(Box::new(incoming))) {
            ChatPhase::Active(prev) => {
                self.last_focused_sid = Some(prev.session_id.clone());
                self.background.push(*prev);
            }
            _ => {
                self.last_focused_sid = Some(session_id.to_string());
            }
        }
        self.ensure_session_alive(session_id).await;
        true
    }

    async fn retry_retained_session(&mut self, session_id: &str) -> bool {
        let entry = if self
            .resume_focused
            .as_ref()
            .is_some_and(|entry| entry.session_id == session_id)
        {
            self.resume_focused.take()
        } else {
            self.resume_backgrounds
                .iter()
                .position(|entry| entry.session_id == session_id)
                .map(|idx| self.resume_backgrounds.remove(idx))
        };
        let Some(mut entry) = entry else {
            return false;
        };

        match self.attach_resume_entry(&entry).await {
            Ok(state) => {
                let needs_terminal_recovery = entry.interrupted || entry.recovery_required;
                let resumed_id = state.session_id.clone();
                match std::mem::replace(&mut self.phase, ChatPhase::Active(Box::new(state))) {
                    ChatPhase::Active(previous) => {
                        self.last_focused_sid = Some(previous.session_id.clone());
                        self.background.push(*previous);
                    }
                    _ => {
                        self.last_focused_sid = Some(session_id.to_string());
                    }
                }
                if needs_terminal_recovery {
                    self.begin_session_resync(resumed_id);
                }
                self.pump_all_queues();
                true
            }
            Err(error) => {
                entry.was_focused = false;
                self.resume_backgrounds.push(entry);
                if let ChatPhase::Active(state) = &mut self.phase {
                    state.set_info_notice(crate::i18n::t_args(
                        "zc-chat-error-resume-history",
                        &[("error", &error)],
                    ));
                }
                false
            }
        }
    }

    /// Close one tracked session: the daemon drops it (history persists) and
    /// the sidebar entry disappears. Focus moves to the next tracked session,
    /// or back to the agent picker when none remain. Returns false when the
    /// id is unknown.
    pub(crate) async fn close_session(&mut self, session_id: &str) -> bool {
        let was_focused = matches!(
            &self.phase,
            ChatPhase::Active(state) if state.session_id == session_id
        );
        let in_background = self
            .background
            .iter()
            .position(|s| s.session_id == session_id);
        let retained_was_focused = self
            .resume_focused
            .as_ref()
            .is_some_and(|entry| entry.session_id == session_id);
        let retained_background = self
            .resume_backgrounds
            .iter()
            .position(|entry| entry.session_id == session_id);
        if !was_focused
            && in_background.is_none()
            && !retained_was_focused
            && retained_background.is_none()
        {
            return false;
        }
        if let Some(mut attempt) = self.entry_retry_attempt.take() {
            attempt.cancel();
            // A started session/new must settle before session/close is sent;
            // otherwise the cancelled retry could recreate the closed ID.
            if let Err(error) = (&mut attempt.worker).await
                && !error.is_cancelled()
            {
                eprintln!("zerocode: entry retry task failed before session close");
            }
        }
        if let Err(error) = self.rpc.session_close(session_id).await {
            let notice = crate::i18n::t_args(
                "zc-chat-session-close-error",
                &[("error", &error.to_string())],
            );
            if let Some(state) = self.state_for_session_mut(session_id) {
                state.set_info_notice(notice);
            } else if let ChatPhase::Active(state) = &mut self.phase {
                state.set_info_notice(notice);
            } else {
                self.phase = ChatPhase::Error(notice);
            }
            return false;
        }
        let mut cleanup_report = CleanupReport::default();
        if let Some(state) = self.state_for_session_mut(session_id) {
            cleanup_report.merge(state.cleanup_active_turn_attachments());
            cleanup_report.merge(state.clear_queue());
            state.input_bar.cleanup_temps();
            cleanup_report.merge(state.input_bar.take_cleanup_report());
        }
        for entry in self
            .resume_focused
            .iter_mut()
            .chain(self.resume_backgrounds.iter_mut())
            .filter(|entry| entry.session_id == session_id)
        {
            for message in entry.queue.messages.drain(..) {
                cleanup_report.merge(cleanup_attachment_temps(&message.attachments));
            }
        }
        self.session_order.retain(|sid| sid != session_id);
        if self
            .last_focused_sid
            .as_deref()
            .is_some_and(|sid| sid == session_id)
        {
            self.last_focused_sid = None;
        }
        if let Some(idx) = in_background {
            self.background.remove(idx);
        } else if let Some(idx) = retained_background {
            self.resume_backgrounds.remove(idx);
        } else {
            if retained_was_focused {
                self.resume_focused = None;
            }
            // Closed the focused session: promote the next tracked one (sidebar
            // order), else fall back to the agent picker.
            let next_idx = self
                .session_order
                .iter()
                .find_map(|sid| self.background.iter().position(|s| &s.session_id == sid))
                .or_else(|| (!self.background.is_empty()).then_some(0));
            match next_idx {
                Some(idx) => {
                    let incoming = self.background.remove(idx);
                    self.phase = ChatPhase::Active(Box::new(incoming));
                }
                None => {
                    self.phase = ChatPhase::PickAgent {
                        agents: Vec::new(),
                        list_state: ListState::default(),
                        loading: true,
                    };
                    let _ = self.init().await;
                }
            }
        }
        if let Some(notice) = cleanup_report.notice() {
            if let ChatPhase::Active(state) = &mut self.phase {
                state.set_info_notice(notice);
            } else {
                // A picker has no session info bar. Report the bounded count,
                // never the private attachment paths, when no pane can show it.
                eprintln!("zerocode: {notice}");
            }
        }
        true
    }

    /// Restore the most recently stashed session (or any background one)
    /// after a cancelled or failed add/pick flow. Returns false when no
    /// background session exists.
    async fn restore_last_focused(&mut self) -> bool {
        let sid = self
            .last_focused_sid
            .take()
            .filter(|sid| self.background.iter().any(|s| &s.session_id == sid))
            .or_else(|| self.background.last().map(|s| s.session_id.clone()));
        match sid {
            Some(sid) => self.focus_session(&sid).await,
            None => false,
        }
    }

    /// Re-attach a session the daemon reported lost (`session_not_found`).
    /// Runs at most one `session/new` per call; success clears the red
    /// status, failure keeps it with a notice.
    async fn ensure_session_alive(&mut self, session_id: &str) {
        if self
            .state_for_session(session_id)
            .is_some_and(|state| state.last_error == Some(SessionError::ResyncFailed))
        {
            self.begin_session_resync(session_id.to_string());
            return;
        }

        let Some(agent_alias) = self
            .state_for_session_mut(session_id)
            .filter(|state| state.last_error == Some(SessionError::SessionLost))
            .map(|state| state.agent_alias.clone())
        else {
            return;
        };

        // A queue-driven re-attach may already be running while the user
        // focuses this session. Let that single canonical operation finish.
        if !self
            .session_reattach_in_flight
            .insert(session_id.to_string())
        {
            return;
        }
        let result =
            Self::reattach_session(&self.rpc, self.pane_kind, session_id, &agent_alias).await;
        self.session_reattach_in_flight.remove(session_id);

        let Some(state) = self.state_for_session_mut(session_id) else {
            return;
        };
        match result {
            Ok(_) => {
                state.last_error = None;
            }
            Err(e) => {
                state.set_info_notice(crate::i18n::t_args(
                    "zc-chat-session-switch-error",
                    &[("error", &e.to_string())],
                ));
            }
        }
        if state.last_error.is_none() {
            self.pump_all_queues();
        }
    }

    async fn reattach_session(
        rpc: &Arc<RpcClient>,
        pane_kind: PaneKind,
        session_id: &str,
        agent_alias: &str,
    ) -> Result<(), String> {
        let result = if pane_kind == PaneKind::Acp {
            rpc.session_new_acp(agent_alias, None, Some(session_id))
                .await
        } else {
            rpc.session_new_with_id(agent_alias, None, Some(session_id))
                .await
        };
        result.map(|_| ()).map_err(|error| error.to_string())
    }

    /// The active session id, if a session is live.
    pub(crate) fn current_session_id(&self) -> Option<&str> {
        match &self.phase {
            ChatPhase::Active(state) => Some(state.session_id.as_str()),
            _ => None,
        }
    }

    /// Whether this pane currently owns a focused or background session id.
    /// The app-level inbound router uses this as the single routing decision;
    /// non-owner panes never see or answer the request.
    pub(crate) fn owns_session(&self, session_id: &str) -> bool {
        matches!(
            &self.phase,
            ChatPhase::Active(state) if state.session_id == session_id
        ) || self
            .background
            .iter()
            .any(|state| state.session_id == session_id)
    }

    pub(crate) fn note_elicitation_drop(&mut self) {
        if let ChatPhase::Active(ref mut state) = self.phase {
            state
                .entries
                .push(ChatEntry::SystemMessage(Arc::<str>::from(crate::i18n::t(
                    "zc-chat-elicitation-dropped",
                ))));
            state.mark_dirty_append();
        }
    }

    #[cfg(test)]
    pub(crate) fn activate_session_for_test(&mut self, session_id: &str) {
        self.phase = ChatPhase::Active(Box::new(ChatState::new(
            session_id.to_string(),
            "test-agent".to_string(),
            crate::todo_tracker::TodoTrackerSettings::default(),
        )));
        self.session_order = vec![session_id.to_string()];
    }

    #[cfg(test)]
    pub(crate) fn has_pending_elicitation_for_test(&self) -> bool {
        matches!(
            &self.phase,
            ChatPhase::Active(state) if state.pending_elicitation.is_some()
        )
    }

    #[cfg(test)]
    pub(crate) fn begin_transcript_drag_for_test(&mut self, move_pointer: bool) {
        if let ChatPhase::Active(state) = &mut self.phase {
            state.transcript_snapshot = Some(TranscriptSnapshot {
                area: Rect::new(0, 0, 5, 1),
                cells: "hello"
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| TranscriptCell {
                        symbol: ch.to_string(),
                        span_start: column as u16,
                    })
                    .collect(),
                row_breaks: vec![TranscriptRowBreak::Hard],
            });
            assert!(state.begin_transcript_drag(0, 0));
            if move_pointer {
                assert!(state.update_transcript_drag(4, 0));
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn transcript_selected_text_for_test(&self) -> Option<String> {
        match &self.phase {
            ChatPhase::Active(state) => state.transcript_selected_text(),
            _ => None,
        }
    }

    /// Fetch agent list. If exactly one enabled agent, auto-start a session (or
    /// show the CWD picker first on WSS ACP connections) — except on the Code
    /// (ACP) pane with no resumable history, where the single-item agent
    /// picker is shown first so the memory-isolation disclosure is visible
    /// before the session starts.
    pub(crate) async fn init(&mut self) -> anyhow::Result<()> {
        self.init_with_cancel(None, None).await.map(|_| ())
    }

    async fn init_with_cancel(
        &mut self,
        cancellation: Option<&Arc<AtomicBool>>,
        phase: Option<&Arc<AtomicU8>>,
    ) -> anyhow::Result<ChatInitOutcome> {
        if is_cancelled(cancellation) {
            return Ok(ChatInitOutcome::Other);
        }
        let agents = match self.rpc.agents_status().await {
            Ok(result) => result
                .agents
                .into_iter()
                .filter(|a| a.enabled)
                .map(|a| a.alias)
                .collect::<Vec<_>>(),
            Err(e) => {
                if !is_cancelled(cancellation) {
                    self.phase = ChatPhase::Error(crate::i18n::t_args(
                        "zc-chat-error-fetch-agents",
                        &[("error", &e.to_string())],
                    ));
                }
                return Ok(ChatInitOutcome::Other);
            }
        };

        if is_cancelled(cancellation) {
            return Ok(ChatInitOutcome::Other);
        }

        if agents.is_empty() {
            self.phase = ChatPhase::Error(crate::i18n::t("zc-chat-no-agents"));
            return Ok(ChatInitOutcome::NoEnabledAgents);
        }

        // Multi-agent reconnect: if a resumed session was carried across the
        // rebuild and its agent is still present, reattach to it automatically
        // rather than forcing the user back through the picker and minting a
        // fresh session. The resume id is consumed by `start_session`.
        if let Some(prior) = self
            .resume_focused
            .as_ref()
            .map(|entry| entry.agent_alias.clone())
        {
            if agents.iter().any(|a| a == &prior) {
                self.pick_or_start_session_inner(&prior, cancellation, phase)
                    .await;
                return Ok(ChatInitOutcome::Other);
            }
            self.phase = ChatPhase::Error(crate::i18n::t_args(
                "zc-chat-resume-dropped",
                &[("count", "1")],
            ));
            return Ok(ChatInitOutcome::Other);
        }

        if agents.len() == 1 {
            if self.resume_focused.is_some() {
                self.pick_or_start_session_inner(&agents[0], cancellation, phase)
                    .await;
                return Ok(ChatInitOutcome::Other);
            }
            if self.try_show_recent_acp_session_picker(&agents).await {
                return Ok(ChatInitOutcome::Other);
            }
            if self.pane_kind == PaneKind::Acp {
                // No resumable ACP history: route through the same
                // disclosure-bearing agent picker as the multi-agent
                // no-history path (below) instead of starting straight into
                // a session, so a first-time Code user still sees the
                // history-vs-persistent-memory note before any fresh
                // `session/new` request goes out. Chat has no such note and
                // keeps auto-starting.
                self.show_agent_picker(agents);
                return Ok(ChatInitOutcome::Other);
            }
            self.pick_or_start_session_inner(&agents[0], cancellation, phase)
                .await;
            return Ok(ChatInitOutcome::Other);
        }

        if self.try_show_recent_acp_session_picker(&agents).await {
            return Ok(ChatInitOutcome::Other);
        }

        self.show_agent_picker(agents);
        Ok(ChatInitOutcome::Other)
    }

    fn show_agent_picker(&mut self, agents: Vec<String>) {
        let prior_alias = match &self.phase {
            ChatPhase::PickAgent {
                agents: prev,
                list_state,
                ..
            } => list_state.selected().and_then(|i| prev.get(i)).cloned(),
            _ => None,
        };
        let selected = prior_alias
            .and_then(|alias| agents.iter().position(|a| a == &alias))
            .unwrap_or(0);
        let mut list_state = ListState::default();
        list_state.select(Some(selected));
        // No carried session matched: a manual pick of a different agent must
        // not bleed a stale resume id into a mismatched agent's session.
        self.resume_focused = None;
        self.phase = ChatPhase::PickAgent {
            agents,
            list_state,
            loading: false,
        };
    }

    async fn try_show_recent_acp_session_picker(&mut self, agents: &[String]) -> bool {
        if self.pane_kind != PaneKind::Acp || self.resume_focused.is_some() || agents.is_empty() {
            return false;
        }

        let Ok(list) = self.rpc.acp_session_list().await else {
            return false;
        };

        let sessions = list
            .sessions
            .into_iter()
            .filter(|entry| {
                entry
                    .agent_alias
                    .as_ref()
                    .is_some_and(|alias| agents.iter().any(|enabled| enabled == alias))
            })
            .collect::<Vec<_>>();

        if sessions.is_empty() {
            return false;
        }

        let mut list_state = ListState::default();
        list_state.select(Some(0));
        self.phase = ChatPhase::PickSession {
            sessions,
            list_state,
            agents: agents.to_vec(),
        };
        true
    }

    async fn resume_session_entry(&mut self, entry: SessionEntry) {
        let Some(agent_alias) = entry.agent_alias else {
            return;
        };
        self.resume_focused = Some(ResumeEntry {
            session_id: entry.session_id,
            agent_alias: agent_alias.clone(),
            message_count: entry.message_count,
            was_focused: true,
            queue: ReconnectQueueState::default(),
            interrupted: false,
            recovery_required: false,
        });
        self.pick_or_start_session(&agent_alias).await;
    }

    async fn start_fresh_from_picker(&mut self, agents: Vec<String>) {
        self.resume_focused = None;
        if agents.len() == 1 {
            self.pick_or_start_session(&agents[0]).await;
        } else {
            self.show_agent_picker(agents);
        }
    }

    /// Decide whether to show the CWD picker (WSS ACP) or start the session
    /// immediately (Unix, or non-ACP pane).
    async fn pick_or_start_session(&mut self, agent_alias: &str) {
        self.cancel_entry_retry();
        self.pick_or_start_session_inner(agent_alias, None, None)
            .await;
    }

    async fn pick_or_start_session_inner(
        &mut self,
        agent_alias: &str,
        cancellation: Option<&Arc<AtomicBool>>,
        phase: Option<&Arc<AtomicU8>>,
    ) {
        if is_cancelled(cancellation) {
            return;
        }
        // A carried resume id means we are reattaching a daemon-retained session
        // across a reconnect: it already has a cwd, so skip the picker and
        // resume directly instead of forcing the user to re-pick a directory.
        if self.resume_focused.is_some() {
            self.start_session_with_cancel(agent_alias, None, cancellation, phase)
                .await;
            return;
        }
        if self.pane_kind == PaneKind::Acp && self.rpc.transport() == crate::client::Transport::Wss
        {
            // Remote ACP: start from the daemon root, not a local path.
            let start_dir = std::path::PathBuf::from("/");
            self.phase = ChatPhase::PickCwd {
                agent_alias: agent_alias.to_string(),
                explorer: FileExplorerState::new_dir_picker_remote(
                    start_dir,
                    Arc::clone(&self.rpc),
                ),
            };
        } else {
            self.start_session_with_cancel(agent_alias, None, cancellation, phase)
                .await;
        }
    }

    /// Public entry point for "start a session against this specific
    /// agent." Used by the Quickstart pane on Stage 2 to route the
    /// user into the freshly-created agent's chat.
    pub(crate) async fn focus_agent(&mut self, agent_alias: &str) {
        let summaries = self.session_summaries();
        let existing = summaries
            .iter()
            .find(|s| s.agent_alias == agent_alias && s.focused)
            .or_else(|| summaries.iter().find(|s| s.agent_alias == agent_alias));
        if let Some(existing) = existing {
            let sid = existing.session_id.clone();
            self.focus_session(&sid).await;
            return;
        }
        self.add_agent_session(agent_alias).await;
    }

    /// Sidebar "+" always creates a new session, preserving existing siblings.
    pub(crate) async fn add_agent_session(&mut self, agent_alias: &str) {
        if self.tracked_session_count() >= MAX_TRACKED_SESSIONS_PER_PANE {
            if let ChatPhase::Active(ref mut state) = self.phase {
                state.set_info_notice(crate::i18n::t_args(
                    "zc-chat-session-cap",
                    &[("max", &MAX_TRACKED_SESSIONS_PER_PANE.to_string())],
                ));
            }
            return;
        }
        // A fresh launch must not consume a failed reconnect's stable ID or queue.
        if let Some(mut retained) = self.resume_focused.take() {
            retained.was_focused = false;
            self.resume_backgrounds.push(retained);
        }
        self.stash_active();
        self.pick_or_start_session(agent_alias).await;
    }

    fn cancel_entry_retry(&mut self) {
        self.entry_retry_attempt.take();
    }

    pub(crate) fn on_pane_blur(&mut self) {
        self.cancel_entry_retry();
    }

    pub(crate) async fn refresh_if_inactive(&mut self) {
        if should_retry_on_entry(&self.phase) {
            let _ = self.init().await;
        } else if !self.resume_backgrounds.is_empty() {
            self.after_session_start().await;
        }
    }

    /// Start a non-blocking Chat-pane retry when entering Chat. ACP keeps using
    /// `refresh_if_inactive` because its session-list semantics are distinct.
    pub(crate) fn start_entry_retry(&mut self) {
        if self.pane_kind != PaneKind::Chat {
            return;
        }
        if self.entry_retry_attempt.is_some() || !should_retry_on_entry(&self.phase) {
            return;
        }

        let rpc = Arc::clone(&self.rpc);
        let resume_entries = self
            .resume_focused
            .iter()
            .chain(self.resume_backgrounds.iter())
            .cloned()
            .collect::<Vec<_>>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let phase = Arc::new(AtomicU8::new(ENTRY_RETRY_PRE_SESSION));
        let worker_phase = Arc::clone(&phase);
        let (result_tx, result_rx) = oneshot::channel();
        self.entry_retry_attempt = Some(EntryRetryAttempt {
            result_rx,
            worker: tokio::spawn(async move {
                let mut retry = Chat::new(rpc, PaneKind::Chat);
                retry.entry_retry_preparing = true;
                if !resume_entries.is_empty() {
                    retry.set_resume_sessions(resume_entries);
                }
                let init_outcome = retry
                    .init_with_cancel(Some(&worker_cancelled), Some(&worker_phase))
                    .await
                    .unwrap_or(ChatInitOutcome::Other);
                let result = ChatEntryRetryResult {
                    chat: Box::new(retry),
                    init_outcome,
                };
                if worker_cancelled.load(Ordering::Acquire) {
                    close_owned_active_session(
                        &result.chat.rpc,
                        &result.chat.phase,
                        result.chat.entry_retry_session_ownership,
                    )
                    .await;
                    return;
                }
                if let Err(result) = result_tx.send(result) {
                    close_owned_active_session(
                        &result.chat.rpc,
                        &result.chat.phase,
                        result.chat.entry_retry_session_ownership,
                    )
                    .await;
                }
            }),
            cancelled,
            phase,
            rpc: Arc::clone(&self.rpc),
        });
    }

    fn drain_entry_retry_results(&mut self) {
        let Some(mut attempt) = self.entry_retry_attempt.take() else {
            return;
        };
        match attempt.result_rx.try_recv() {
            Ok(result) => {
                let cancelled = attempt.cancelled.load(Ordering::Acquire);
                drop(attempt);
                if cancelled || !should_retry_on_entry(&self.phase) {
                    if let Some(session_id) = active_session_id(&result.chat.phase) {
                        spawn_close_session_if_owned(
                            Arc::clone(&self.rpc),
                            session_id,
                            result.chat.entry_retry_session_ownership,
                        );
                    }
                    return;
                }

                if matches!(result.chat.phase, ChatPhase::Error(_)) {
                    if matches!(self.phase, ChatPhase::Error(_))
                        || result.init_outcome == ChatInitOutcome::NoEnabledAgents
                    {
                        self.phase = result.chat.phase;
                    }
                    return;
                }

                let prior_alias = self.selected_agent_alias();
                let mut replacement = *result.chat;
                if let ChatPhase::PickAgent {
                    agents, list_state, ..
                } = &mut replacement.phase
                    && let Some(alias) = prior_alias
                    && let Some(index) = agents.iter().position(|agent| agent == &alias)
                {
                    list_state.select(Some(index));
                }
                replacement.entry_retry_attempt = None;
                *self = replacement;
                self.entry_retry_preparing = false;
                let pending_resync = std::mem::take(&mut self.session_resync_in_flight);
                for session_id in pending_resync {
                    self.begin_session_resync(session_id);
                }
                self.pump_all_queues();
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                self.entry_retry_attempt = Some(attempt);
            }
            Err(oneshot::error::TryRecvError::Closed) => drop(attempt),
        }
    }

    /// Resolve the local `[todotracker]` settings from `zerocode-config.toml`.
    /// Called at every session boundary (new / restart / switch) so the file
    /// stays the single source of truth and a Config-pane save takes effect on
    /// the next transition.
    ///
    /// On a load failure (e.g. an invalid `ZEROCODE_todotracker__*` override or
    /// a malformed section) the `fallback` is returned rather than hard
    /// defaults, so a transient error does not silently reset a user's tracker
    /// layout/visibility to the built-ins. The failure is logged so it can be
    /// diagnosed.
    fn resolve_todo_settings(
        fallback: crate::todo_tracker::TodoTrackerSettings,
    ) -> crate::todo_tracker::TodoTrackerSettings {
        match crate::config::resolve_todo_tracker_checked(&crate::i18n::config_dir()) {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!(
                    "zerocode: resolving [todotracker] failed ({error:#}); keeping current settings"
                );
                fallback
            }
        }
    }

    async fn start_session(&mut self, agent_alias: &str, cwd_override: Option<&str>) {
        self.start_session_with_cancel(agent_alias, cwd_override, None, None)
            .await;
    }

    async fn start_session_with_cancel(
        &mut self,
        agent_alias: &str,
        cwd_override: Option<&str>,
        cancellation: Option<&Arc<AtomicBool>>,
        phase: Option<&Arc<AtomicU8>>,
    ) {
        if is_cancelled(cancellation) {
            return;
        }
        // TodoWrite display is a ZeroCode UI concern owned by
        // `zerocode-config.toml` — the daemon holds no TodoWrite display schema
        // — so read it from the local config file (honoring `--config-dir` /
        // `CLAWCREW_CONFIG_DIR`), not over RPC. A fresh pane has no prior
        // tracker, so a load failure falls back to the built-in defaults.
        let todo_settings =
            Self::resolve_todo_settings(crate::todo_tracker::TodoTrackerSettings::default());

        // Reattach to a carried-over session on reconnect (one-shot); else a
        // fresh session. `session_new_with_id`/`_acp` with Some(id) restores
        // the daemon-retained session, its persisted history, and its cwd.
        let resume = self.resume_focused.clone();
        let resume_id = resume.as_ref().map(|entry| entry.session_id.as_str());
        // Ownership follows the request, not whether persisted history was
        // present. A caller-provided stable ID remains caller-owned even when
        // the daemon rehydrates it with an empty transcript.
        let session_ownership = if resume.is_some() {
            EntryRetrySessionOwnership::CallerOwned
        } else {
            EntryRetrySessionOwnership::RetryCreated
        };
        // A resume must not re-point the session at the TUI's launch directory:
        // pass no cwd so the daemon keeps the retained session's own cwd.
        //
        // Fresh Chat sessions omit cwd so the daemon uses the selected agent's
        // workspace. Fresh local Code sessions pin the process cwd so file and
        // shell tools operate on the project zerocode was launched from. An
        // explicit caller-supplied cwd (the remote ACP picker) still wins.
        //
        // If a local Code session cannot capture its launch directory we fail
        // the creation instead of omitting cwd: a silent fallback would start a
        // healthy-looking session rooted at the agent workspace, letting file
        // and shell tools act on a different project.
        let explicit_cwd = cwd_override
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string);
        let cwd_str: Option<String> = if resume_id.is_some() {
            None
        } else if let Some(cwd) = explicit_cwd {
            Some(cwd)
        } else {
            match local_code_session_cwd(self.pane_kind, self.rpc.transport()) {
                Ok(cwd) => cwd,
                Err(e) => {
                    self.phase = ChatPhase::Error(crate::i18n::t_args(
                        "zc-chat-error-create-session",
                        &[("error", &e.localized())],
                    ));
                    return;
                }
            }
        };
        if is_cancelled(cancellation) {
            return;
        }
        if let Some(phase) = phase
            && !claim_entry_retry_session_creation(phase)
        {
            return;
        }
        if is_cancelled(cancellation) {
            return;
        }
        let result = if self.pane_kind == PaneKind::Acp {
            self.rpc
                .session_new_acp(agent_alias, cwd_str.as_deref(), resume_id)
                .await
        } else {
            self.rpc
                .session_new_with_id(agent_alias, cwd_str.as_deref(), resume_id)
                .await
        };
        match result {
            Ok(session) => {
                let resumed_sid = resume.as_ref().map(|_| session.session_id.clone());
                let recovery_session_id = session.session_id.clone();
                let needs_terminal_recovery = resume
                    .as_ref()
                    .is_some_and(|entry| entry.interrupted || entry.recovery_required);
                if phase.is_some() {
                    self.entry_retry_session_ownership = Some(session_ownership);
                }
                if is_cancelled(cancellation) {
                    close_stale_session_if_owned(
                        &self.rpc,
                        &session.session_id,
                        Some(session_ownership),
                    )
                    .await;
                    return;
                }
                // `todo_settings` is resolved fresh at this boundary from
                // `zerocode-config.toml` (the canonical owner); the removed
                // `self.todo_settings` cache was the stale cross-session copy.
                let mut state = ChatState::with_shared_commands(
                    session.session_id,
                    agent_alias.to_string(),
                    todo_settings,
                    self.rpc.commands(),
                );
                state.message_count = session.message_count;
                state.cwd = session.workspace_dir;
                if is_cancelled(cancellation) {
                    close_stale_session_if_owned(
                        &self.rpc,
                        &state.session_id,
                        Some(session_ownership),
                    )
                    .await;
                    return;
                }
                Self::refresh_model_identity(&self.rpc, &mut state).await;
                if is_cancelled(cancellation) {
                    close_stale_session_if_owned(
                        &self.rpc,
                        &state.session_id,
                        Some(session_ownership),
                    )
                    .await;
                    return;
                }
                // On a resume, replay the daemon-retained transcript so the
                // reattached pane shows the prior conversation rather than an
                // empty history. Fresh sessions have nothing to load.
                if let Some(sid) = resumed_sid {
                    let msgs = match self.rpc.session_messages(&sid).await {
                        Ok(msgs) => msgs,
                        Err(error) => {
                            self.phase = ChatPhase::Error(crate::i18n::t_args(
                                "zc-chat-error-resume-history",
                                &[("error", &error.to_string())],
                            ));
                            self.after_session_start().await;
                            return;
                        }
                    };
                    state.message_count = msgs.total;
                    state.load_history(msgs.messages, self.pane_kind == PaneKind::Acp);
                }
                // The carried entry remains canonical until both session/new
                // and durable-history replay succeed.
                self.resume_focused = None;
                if let Some(entry) = resume {
                    state.restore_reconnect_state(
                        entry.queue,
                        entry.interrupted,
                        entry.recovery_required,
                    );
                }
                if !self.session_order.contains(&state.session_id) {
                    self.session_order.push(state.session_id.clone());
                }
                if is_cancelled(cancellation) {
                    close_stale_session_if_owned(
                        &self.rpc,
                        &state.session_id,
                        Some(session_ownership),
                    )
                    .await;
                    return;
                }
                self.phase = ChatPhase::Active(Box::new(state));
                if needs_terminal_recovery {
                    self.begin_session_resync(recovery_session_id);
                }
            }
            Err(e) => {
                self.phase = ChatPhase::Error(crate::i18n::t_args(
                    "zc-chat-error-create-session",
                    &[("error", &e.to_string())],
                ));
            }
        }
        self.after_session_start().await;
    }

    /// Post-`start_session` bookkeeping for the multi-session pane:
    /// reconnect-resume promotion and background rehydration, and the
    /// guarantee that a failed start never strands live background sessions
    /// behind an `Error` screen.
    async fn after_session_start(&mut self) {
        let start_error_notice = match &self.phase {
            ChatPhase::Error(message) => Some(message.clone()),
            _ => None,
        };
        let mut failed_this_start = HashSet::new();

        // If the focused reconnect entry cannot attach, promote the first
        // retained background that can. Failures stay queued for a later
        // reconnect instead of being retried repeatedly in this same pass.
        if start_error_notice.is_some()
            && self.background.is_empty()
            && !self.resume_backgrounds.is_empty()
        {
            let entries = std::mem::take(&mut self.resume_backgrounds);
            let mut retained = Vec::new();
            let mut entries = entries.into_iter();
            while let Some(entry) = entries.next() {
                match self.attach_resume_entry(&entry).await {
                    Ok(state) => {
                        let session_id = state.session_id.clone();
                        let needs_terminal_recovery = entry.interrupted || entry.recovery_required;
                        if !self.session_order.contains(&session_id) {
                            self.session_order.push(session_id.clone());
                        }
                        self.phase = ChatPhase::Active(Box::new(state));
                        retained.extend(entries);
                        if needs_terminal_recovery {
                            self.begin_session_resync(session_id);
                        }
                        break;
                    }
                    Err(_) => {
                        failed_this_start.insert(entry.session_id.clone());
                        retained.push(entry);
                    }
                }
            }
            self.resume_backgrounds = retained;
        }

        if matches!(self.phase, ChatPhase::Active(_)) {
            let entries = std::mem::take(&mut self.resume_backgrounds);
            let mut retained = Vec::new();
            for entry in entries {
                if failed_this_start.contains(&entry.session_id) {
                    retained.push(entry);
                    continue;
                }
                if self.tracked_session_count() >= MAX_TRACKED_SESSIONS_PER_PANE {
                    retained.push(entry);
                    continue;
                }
                if self
                    .background
                    .iter()
                    .any(|s| s.session_id == entry.session_id)
                {
                    continue;
                }
                match self.attach_resume_entry(&entry).await {
                    Ok(state) => {
                        let session_id = state.session_id.clone();
                        let needs_terminal_recovery = entry.interrupted || entry.recovery_required;
                        if !self.session_order.contains(&entry.session_id) {
                            self.session_order.push(entry.session_id.clone());
                        }
                        self.background.push(state);
                        if needs_terminal_recovery {
                            self.begin_session_resync(session_id);
                        }
                    }
                    Err(_) => retained.push(entry),
                }
            }
            let retained_count = retained.len();
            self.resume_backgrounds = retained;
            if let ChatPhase::Active(ref mut state) = self.phase {
                let mut notices = Vec::new();
                if let Some(message) = start_error_notice.clone() {
                    notices.push(message);
                }
                if retained_count > 0 {
                    notices.push(crate::i18n::t_args(
                        "zc-chat-resume-dropped",
                        &[("count", &retained_count.to_string())],
                    ));
                }
                if !notices.is_empty() {
                    state.set_info_notice(notices.join(" "));
                }
            }
        }

        if matches!(self.phase, ChatPhase::Active(_)) {
            self.pump_all_queues();
        }

        // A failed start with live background sessions: restore the last
        // focused one and surface the error as a notice instead of taking
        // over the pane with the Error screen.
        if let ChatPhase::Error(msg) = &self.phase
            && !self.background.is_empty()
        {
            let msg = msg.clone();
            if self.restore_last_focused().await
                && let ChatPhase::Active(ref mut state) = self.phase
            {
                state.set_info_notice(msg);
            }
        }
    }

    /// Rehydrate an existing daemon session into a fresh `ChatState` (history
    /// replayed), without touching the pane phase. Used for background resume
    /// entries and session-picker attaches.
    async fn attach_session(
        &self,
        agent_alias: &str,
        session_id: &str,
    ) -> Result<ChatState, String> {
        let result = if self.pane_kind == PaneKind::Acp {
            self.rpc
                .session_new_acp(agent_alias, None, Some(session_id))
                .await
        } else {
            self.rpc
                .session_new_with_id(agent_alias, None, Some(session_id))
                .await
        };
        let session = result.map_err(|e| e.to_string())?;
        let todo_fallback = match &self.phase {
            ChatPhase::Active(state) => state.todo_tracker.settings(),
            _ => self
                .background
                .first()
                .map(|state| state.todo_tracker.settings())
                .unwrap_or_default(),
        };
        let mut state = ChatState::with_shared_commands(
            session.session_id,
            agent_alias.to_string(),
            Self::resolve_todo_settings(todo_fallback),
            self.rpc.commands(),
        );
        state.message_count = session.message_count;
        state.cwd = session.workspace_dir;
        Self::refresh_model_identity(&self.rpc, &mut state).await;
        let msgs = match self.rpc.session_messages(session_id).await {
            Ok(messages) => messages,
            Err(error) => {
                return Err(error.to_string());
            }
        };
        state.message_count = msgs.total;
        state.load_history(msgs.messages, self.pane_kind == PaneKind::Acp);
        Ok(state)
    }

    /// Reattach one reconnect entry, restoring only the client-owned queue
    /// after the daemon-owned transcript has been reloaded.
    async fn attach_resume_entry(&self, entry: &ResumeEntry) -> Result<ChatState, String> {
        let mut state = self
            .attach_session(&entry.agent_alias, &entry.session_id)
            .await?;
        state.restore_reconnect_state(
            entry.queue.clone(),
            entry.interrupted,
            entry.recovery_required,
        );
        Ok(state)
    }

    async fn confirm_model_picker_selection(rpc: &Arc<RpcClient>, state: &mut ChatState) {
        // Resolve the selection, then act. The final switch needs async + `rpc`,
        // so extract owned values before replacing the overlay.
        match &state.model_picker {
            ModelPickerOverlay::Model(p) => {
                let choice = p.selected().map(str::to_string);
                state.model_picker = ModelPickerOverlay::None;
                if let Some(model) = choice {
                    Self::apply_session_override(
                        rpc,
                        state,
                        crate::client::SessionOverrides {
                            model: Some(model),
                            ..Default::default()
                        },
                    )
                    .await;
                }
            }
            ModelPickerOverlay::ConfiguredProviderStage(p) => {
                let choice = p.selected().map(str::to_string);
                state.model_picker = ModelPickerOverlay::None;
                if let Some(model_provider) = choice {
                    Self::apply_session_override(
                        rpc,
                        state,
                        crate::client::SessionOverrides {
                            model_provider: Some(model_provider),
                            ..Default::default()
                        },
                    )
                    .await;
                } else {
                    state.mark_dirty_full();
                }
            }
            ModelPickerOverlay::Loading | ModelPickerOverlay::None => {}
        }
    }

    async fn restart_session_for_state(
        rpc: &Arc<RpcClient>,
        pane_kind: PaneKind,
        state: &mut ChatState,
    ) -> Option<ChatPhase> {
        let alias = state.agent_alias.clone();
        if pane_kind == PaneKind::Acp && rpc.transport() == crate::client::Transport::Wss {
            // For WSS ACP, go through the CWD picker for new sessions too.
            if let Err(error) = rpc.session_close(&state.session_id).await {
                state.set_info_notice(crate::i18n::t_args(
                    "zc-chat-session-close-error",
                    &[("error", &error.to_string())],
                ));
                return None;
            }
            // Remote ACP picker must start from a path the daemon understands.
            let start_dir = std::path::PathBuf::from("/");
            return Some(ChatPhase::PickCwd {
                agent_alias: alias,
                explorer: FileExplorerState::new_dir_picker_remote(start_dir, Arc::clone(rpc)),
            });
        }

        // Chat restarts omit cwd so the daemon keeps the agent workspace.
        // Local Code restarts pin the process cwd. Remote ACP re-prompts via
        // the picker above.
        //
        // A capture failure aborts the restart and keeps the existing session
        // rather than minting one rooted at the agent workspace, which would
        // silently move file and shell tools to a different project.
        let cwd_str = match local_code_session_cwd(pane_kind, rpc.transport()) {
            Ok(cwd) => cwd,
            Err(e) => {
                state.set_info_notice(crate::i18n::t_args(
                    "zc-chat-session-restart-error",
                    &[("error", &e.localized())],
                ));
                return None;
            }
        };
        let new_session = if pane_kind == PaneKind::Acp {
            rpc.session_new_acp(&alias, cwd_str.as_deref(), None).await
        } else {
            rpc.session_new(&alias, cwd_str.as_deref()).await
        };
        match new_session {
            Ok(s) => {
                let old_session_id = state.session_id.clone();
                if let Err(error) = rpc.session_close(&old_session_id).await {
                    // The old session remains the local owner when the daemon
                    // cannot confirm its close. Best-effort cleanup prevents
                    // the unused replacement from accumulating server-side.
                    let close_notice = crate::i18n::t_args(
                        "zc-chat-session-close-error",
                        &[("error", &error.to_string())],
                    );
                    let cleanup_notice =
                        rpc.session_close(&s.session_id)
                            .await
                            .err()
                            .map(|cleanup_error| {
                                crate::i18n::t_args(
                                    "zc-chat-session-close-error",
                                    &[("error", &cleanup_error.to_string())],
                                )
                            });
                    state.set_info_notice(append_cleanup_notice(close_notice, cleanup_notice));
                    return None;
                }
                let current = state.todo_tracker.settings();
                state.reset_for_session(s.session_id, None, Self::resolve_todo_settings(current));
                state.cwd = s.workspace_dir;
                Self::refresh_model_identity(rpc, state).await;
                state.set_info_notice(crate::i18n::t("zc-chat-session-restarted"));
            }
            Err(e) => {
                state.set_info_notice(crate::i18n::t_args(
                    "zc-chat-session-restart-error",
                    &[("error", &e.to_string())],
                ));
            }
        }
        None
    }

    // ── Drain channels (called from draw) ────────────────────────

    fn drain_notifications(&mut self) {
        let mut applied = false;
        loop {
            match self.notif_rx.try_recv() {
                Ok(notif) if notif.method == "session/update" => {
                    if let Some(update) = parse_session_update(&notif.params) {
                        // Route by session id: background sessions receive
                        // their stream too, so transcripts and status dots
                        // stay current while unfocused.
                        let sid = update.session_id().to_string();
                        if !self.session_resync_in_flight.contains(&sid)
                            && let Some(state) = self.state_for_session_mut(&sid)
                            && state.last_error != Some(SessionError::ResyncFailed)
                        {
                            state.apply_update(update);
                            applied = true;
                        }
                    }
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    self.begin_notification_resync();
                    continue;
                }
                _ => break,
            }
        }
        if applied {
            self.pump_all_queues();
        }
    }

    /// Notification overflow means the live view is no longer authoritative:
    /// the missing frame may have been a terminal update. Gate queue dispatch,
    /// invalidate transport-bound interactions, and reload every tracked
    /// session from the daemon's durable transcript.
    fn begin_notification_resync(&mut self) {
        let session_ids = self
            .session_summaries()
            .into_iter()
            .map(|summary| summary.session_id)
            .collect::<Vec<_>>();
        for session_id in session_ids {
            self.begin_session_resync(session_id);
        }
    }

    fn drain_prompt_completions(&mut self) {
        let mut settled = false;
        while let Ok(completion) = self.prompt_completion_rx.try_recv() {
            let Some(state) = self.state_for_session_mut(&completion.session_id) else {
                continue;
            };
            if state.turn_generation != completion.turn_generation || !state.turn_in_flight {
                continue;
            }

            if completion.transport_closed {
                // A closed transport cannot prove that the daemon completed
                // this turn; retain the interrupted state for reconnect.
                continue;
            }
            if completion.error.is_some() {
                state.remove_optimistic_user_message(completion.turn_generation);
            }
            // The response proves the handler returned, but only the missing
            // terminal notification distinguishes completed from cancelled or
            // failed. Settle conservatively so queued work cannot auto-run.
            state.settle_turn_from_prompt_response();
            if let Some(error) = completion.error {
                state.set_info_notice(crate::i18n::t_args(
                    "zc-queue-dispatch-failed",
                    &[("error", &error)],
                ));
            }
            settled = true;
        }
        if settled {
            self.pump_all_queues();
        }
    }

    fn begin_session_resync(&mut self, session_id: String) {
        if !self.session_resync_in_flight.insert(session_id.clone()) {
            return;
        }
        if self.entry_retry_preparing {
            return;
        }

        let rpc = self.rpc.clone();
        let tx = self.session_resync_tx.clone();
        if let Some(state) = self.state_for_session_mut(&session_id) {
            let approval = state.pending_approval.take();
            let elicitation = state.pending_elicitation.take();
            state.prepare_for_notification_resync();
            if let Some(approval) = approval {
                Self::reject_stale_approval(&rpc, session_id.clone(), approval.request_id);
            }
            if let Some(elicitation) = elicitation {
                Self::answer_cancel(&rpc, elicitation.request_id);
            }
        }

        tokio::spawn(async move {
            let result = Self::cancel_confirm_and_reload_with_retry(&rpc, &session_id).await;
            let _ = tx.send(SessionResyncResult { session_id, result }).await;
        });
    }

    async fn cancel_confirm_and_reload_with_retry(
        rpc: &Arc<RpcClient>,
        session_id: &str,
    ) -> Result<SessionResyncSnapshot, String> {
        let mut last_error = String::new();
        for attempt in 0..SESSION_RECOVERY_MAX_ATTEMPTS {
            match Self::cancel_confirm_and_reload(rpc, session_id).await {
                Ok(messages) => return Ok(messages),
                Err(error) => last_error = error,
            }
            if attempt + 1 < SESSION_RECOVERY_MAX_ATTEMPTS {
                let multiplier = 1_u32 << u32::from(attempt);
                tokio::time::sleep(SESSION_RECOVERY_RETRY_BASE * multiplier).await;
            }
        }
        Err(last_error)
    }

    async fn cancel_confirm_and_reload(
        rpc: &Arc<RpcClient>,
        session_id: &str,
    ) -> Result<SessionResyncSnapshot, String> {
        // Cancellation is best effort: an already-idle session reports that
        // no active turn exists. The state query below is the authoritative
        // terminal confirmation in either case.
        let _ = rpc.session_cancel(session_id).await;
        let deadline = Instant::now() + SESSION_RECOVERY_TERMINAL_TIMEOUT;
        let plan = loop {
            let state = rpc
                .session_state(session_id)
                .await
                .map_err(|error| format!("terminal-state check failed: {error}"))?;
            if state.state != "running" && state.turn_id.is_none() {
                break state.plan;
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "turn did not reach a terminal state within {} seconds",
                    SESSION_RECOVERY_TERMINAL_TIMEOUT.as_secs()
                ));
            }
            tokio::time::sleep(SESSION_RECOVERY_POLL_INTERVAL).await;
        };

        let messages = rpc
            .session_messages(session_id)
            .await
            .map(|messages| SessionResyncSnapshot {
                message_count: messages.total,
                messages: messages.messages,
                plan,
            })
            .map_err(|error| format!("transcript reload failed: {error}"))?;
        Ok(messages)
    }

    fn apply_session_resync_result(&mut self, update: SessionResyncResult) {
        match update.result {
            Ok(snapshot) => {
                let strip_runtime_enrichment = self.pane_kind == PaneKind::Acp;
                let Some(state) = self.state_for_session_mut(&update.session_id) else {
                    self.session_resync_in_flight.remove(&update.session_id);
                    return;
                };
                state.replace_history_after_notification_resync(
                    snapshot.messages,
                    strip_runtime_enrichment,
                );
                state.message_count = snapshot.message_count;
                if let Some(plan) = snapshot.plan {
                    state.todo_tracker.set_plan(plan);
                }
                self.session_resync_in_flight.remove(&update.session_id);
                self.pump_all_queues();
            }
            Err(error) => {
                let Some(state) = self.state_for_session_mut(&update.session_id) else {
                    self.session_resync_in_flight.remove(&update.session_id);
                    return;
                };
                state
                    .entries
                    .push(ChatEntry::SystemMessage(Arc::<str>::from(
                        crate::i18n::t_args("zc-chat-resync-failed", &[("error", &error)]),
                    )));
                state.last_error = Some(SessionError::ResyncFailed);
                state.mark_dirty_append();
                // Release the operation slot but keep dispatch fail-closed via
                // `last_error`; focusing this session or enqueueing again starts
                // one fresh bounded reconciliation attempt.
                self.session_resync_in_flight.remove(&update.session_id);
            }
        }
    }

    fn drain_session_resync_results(&mut self) {
        while let Ok(update) = self.session_resync_rx.try_recv() {
            self.apply_session_resync_result(update);
        }
    }

    /// Answer an inbound request with `{"action":"cancel"}`, which the daemon's
    /// `RpcApprovalChannel::request_choice` collapses to `Ok(None)` so the
    /// calling tool takes its non-channel fallback path.
    pub(crate) fn answer_cancel(rpc: &Arc<RpcClient>, id: serde_json::Value) {
        rpc.respond_to_inbound_request(id, Ok(serde_json::json!({ "action": "cancel" })));
    }

    /// A reconnect rebuild invalidates approval UI tied to the old transport.
    /// Reject it best-effort so the daemon does not keep a tool call waiting on
    /// a decision the rebuilt pane can no longer safely provide.
    fn reject_stale_approval(rpc: &Arc<RpcClient>, session_id: String, request_id: String) {
        let rpc = rpc.clone();
        tokio::spawn(async move {
            let _ = rpc
                .session_approve(&session_id, &request_id, ApprovalDecision::Reject)
                .await;
        });
    }

    pub(crate) fn try_install_elicitation(
        &mut self,
        req: crate::client::RpcInboundRequest,
    ) -> ElicitationRouting {
        let params: Option<crate::wire::ElicitationRequestParams> =
            serde_json::from_value(req.params.clone()).ok();
        let shape = params
            .as_ref()
            .and_then(|p| crate::wire::ElicitationShape::from_schema(&p.requested_schema));

        // A request we can't decode (missing params or an unknown schema)
        // can never install — cancel it immediately, no retry.
        let (params, shape) = match (params, shape) {
            (Some(p), Some(s)) => (p, s),
            _ => return ElicitationRouting::Unparseable(req.id),
        };

        // Must target a session THIS pane tracks (focused or background). If
        // not, it may simply be that the pane is mid resume/reset/switch —
        // defer and retry rather than cancel a prompt this pane will shortly
        // own. Background installs render once their session is focused; the
        // sidebar shows the needs-input dot meanwhile.
        let matches_active = matches!(
            &self.phase,
            ChatPhase::Active(state) if state.session_id == params.session_id
        );
        let matches_background = self
            .background
            .iter()
            .any(|s| s.session_id == params.session_id);
        if !matches_active && !matches_background {
            return ElicitationRouting::Defer(req);
        }

        let pending = match shape {
            crate::wire::ElicitationShape::Single { choices, .. } => PendingElicitation {
                request_id: req.id,
                session_id: params.session_id,
                message: params.message,
                choices: choices.into_iter().map(|c| c.title).collect(),
                multi: false,
                min_items: 1,
                max_items: 1,
                cursor: 0,
                selected: Vec::new(),
            },
            crate::wire::ElicitationShape::Multi {
                choices,
                min_items,
                max_items,
                ..
            } => {
                let n = choices.len();
                PendingElicitation {
                    request_id: req.id,
                    session_id: params.session_id,
                    message: params.message,
                    choices: choices.into_iter().map(|c| c.title).collect(),
                    multi: true,
                    min_items,
                    max_items,
                    cursor: 0,
                    selected: vec![false; n],
                }
            }
        };

        let sid = pending.session_id.clone();
        if let Some(state) = self.state_for_session_mut(&sid) {
            state.set_pending_elicitation(pending);
        }
        ElicitationRouting::Installed
    }

    fn settle_stuck_cancel(&mut self) {
        let mut settled = false;
        let mut settle = |state: &mut ChatState| {
            if !state.cancel_watchdog_expired() {
                return;
            }
            state
                .entries
                .push(ChatEntry::SystemMessage(Arc::<str>::from(crate::i18n::t(
                    "zc-cancel-timed-out",
                ))));
            state.mark_dirty_append();
            state.commit_turn(String::new(), false);
            settled = true;
        };
        if let ChatPhase::Active(ref mut state) = self.phase {
            settle(state);
        }
        for state in &mut self.background {
            settle(state);
        }
        if settled {
            self.pump_all_queues();
        }
    }

    fn after_enqueue(&mut self, enq: Result<(), String>) {
        match enq {
            Ok(()) => {
                if let ChatPhase::Active(ref mut state) = self.phase {
                    state.ensure_queue_selection();
                }
                self.retry_failed_resyncs_with_queued_input();
                self.pump_all_queues();
            }
            Err(msg) => {
                if let ChatPhase::Active(ref mut state) = self.phase {
                    state
                        .entries
                        .push(ChatEntry::SystemMessage(Arc::<str>::from(msg)));
                    state.mark_dirty_append();
                }
            }
        }
    }

    async fn cancel_active_turn_for_injection(&mut self) {
        let session_id = match self.phase {
            ChatPhase::Active(ref state)
                if state.turn_in_flight && !matches!(state.turn_status, TurnStatus::Cancelling) =>
            {
                state.session_id.clone()
            }
            _ => return,
        };
        let result = self.rpc.session_cancel(&session_id).await;
        if let ChatPhase::Active(ref mut state) = self.phase {
            if result.is_ok() {
                state.enter_cancelling();
            } else {
                state.commit_turn(String::new(), false);
            }
        }
    }

    async fn execute_context_menu_request(&mut self, request: ChatContextMenuRequest) {
        match request {
            ChatContextMenuRequest::CopyTranscript(target) => {
                let ChatPhase::Active(ref mut state) = self.phase else {
                    return;
                };
                if !state.copy_text_and_clear_selection(&target.text) {
                    return;
                }
                match target.kind {
                    CopyHitKind::Code => {
                        state.set_copy_feedback(CopyFeedbackTarget::Code(target.group));
                    }
                    CopyHitKind::Message | CopyHitKind::Transcript => {
                        state.set_overlay_copy_feedback(target.rect);
                    }
                }
            }
            ChatContextMenuRequest::Queue { id, action } => match action {
                ChatContextMenuAction::SendNow => {
                    let promoted = match self.phase {
                        ChatPhase::Active(ref mut state) => state.promote_queued_by_id(id),
                        _ => false,
                    };
                    if promoted {
                        self.cancel_active_turn_for_injection().await;
                        self.retry_failed_resyncs_with_queued_input();
                        self.pump_all_queues();
                    }
                }
                ChatContextMenuAction::Copy => {
                    let ChatPhase::Active(ref mut state) = self.phase else {
                        return;
                    };
                    let Some(text) = state.queued_text(id).filter(|text| !text.is_empty()) else {
                        return;
                    };
                    crate::mouse::copy_osc52(&text);
                    state.set_info_notice(crate::i18n::t("zc-chat-copied-clipboard"));
                }
                ChatContextMenuAction::Edit => {
                    let ChatPhase::Active(ref mut state) = self.phase else {
                        return;
                    };
                    let composer_busy = !state.input_bar.input().trim().is_empty()
                        || state.input_bar.has_pending_attachments();
                    if composer_busy {
                        state
                            .entries
                            .push(ChatEntry::SystemMessage(Arc::<str>::from(crate::i18n::t(
                                "zc-queue-edit-busy",
                            ))));
                        state.mark_dirty_append();
                    } else if let Some((text, attachments)) = state.take_queued_for_edit(id) {
                        state.input_bar.load_for_edit(text, attachments);
                    }
                }
                ChatContextMenuAction::Delete => {
                    if let ChatPhase::Active(ref mut state) = self.phase {
                        state.delete_queued_by_id(id);
                    }
                }
            },
        }
    }

    /// Retry failed reconciliation only from explicit queue interaction.
    /// Routine notifications still call `pump_all_queues`, so keeping this
    /// separate prevents an active sibling from causing an endless retry loop.
    fn retry_failed_resyncs_with_queued_input(&mut self) {
        let retry_resyncs = self
            .session_summaries()
            .into_iter()
            .filter_map(|summary| {
                self.state_for_session(&summary.session_id)
                    .filter(|state| {
                        state.last_error == Some(SessionError::ResyncFailed)
                            && state.queue_len() > 0
                    })
                    .map(|_| summary.session_id)
            })
            .collect::<Vec<_>>();
        for session_id in retry_resyncs {
            self.begin_session_resync(session_id);
        }
    }

    fn pump_all_queues(&mut self) {
        if self.entry_retry_preparing {
            return;
        }
        let rpc_out = self.rpc_out.clone();
        let prompt_completion_tx = self.prompt_completion_tx.clone();
        let rpc = self.rpc.clone();
        let transport = self.rpc.transport();
        let pane_kind = self.pane_kind;
        let session_reattach_tx = self.session_reattach_tx.clone();
        let session_reattach_in_flight = &mut self.session_reattach_in_flight;
        let session_resync_in_flight = &self.session_resync_in_flight;
        if let ChatPhase::Active(ref mut state) = self.phase {
            Self::pump_state_queue(
                &rpc_out,
                &prompt_completion_tx,
                &rpc,
                &session_reattach_tx,
                session_reattach_in_flight,
                session_resync_in_flight,
                pane_kind,
                transport,
                state,
            );
        }
        for state in &mut self.background {
            Self::pump_state_queue(
                &rpc_out,
                &prompt_completion_tx,
                &rpc,
                &session_reattach_tx,
                session_reattach_in_flight,
                session_resync_in_flight,
                pane_kind,
                transport,
                state,
            );
        }
    }

    fn pump_state_queue(
        rpc_out: &Arc<RpcOutbound>,
        prompt_completion_tx: &mpsc::Sender<PromptCompletion>,
        rpc: &Arc<RpcClient>,
        session_reattach_tx: &mpsc::Sender<SessionReattachResult>,
        session_reattach_in_flight: &mut HashSet<String>,
        session_resync_in_flight: &HashSet<String>,
        pane_kind: PaneKind,
        transport: crate::client::Transport,
        state: &mut ChatState,
    ) {
        if session_resync_in_flight.contains(&state.session_id) {
            return;
        }
        if state.last_error == Some(SessionError::ResyncFailed) {
            return;
        }
        if state.next_dispatch_index().is_none() {
            return;
        }
        if state.last_error == Some(SessionError::SessionLost) {
            let sid = state.session_id.clone();
            if session_reattach_in_flight.insert(sid.clone()) {
                let rpc = rpc.clone();
                let tx = session_reattach_tx.clone();
                let agent_alias = state.agent_alias.clone();
                tokio::spawn(async move {
                    let result = Self::reattach_session(&rpc, pane_kind, &sid, &agent_alias).await;
                    let _ = tx
                        .send(SessionReattachResult {
                            session_id: sid,
                            result,
                        })
                        .await;
                });
            }
            return;
        }
        let Some(QueuedMessage {
            text, attachments, ..
        }) = state.take_next_dispatchable()
        else {
            return;
        };
        let sid = state.session_id.clone();

        let attachments_json = if attachments.is_empty() {
            Vec::new()
        } else {
            match build_attachments_json(&attachments, transport) {
                Ok(json) => json,
                Err(e) => {
                    let cleanup_report = cleanup_attachment_temps(&attachments);
                    state.surface_cleanup_report(cleanup_report);
                    state
                        .entries
                        .push(ChatEntry::SystemMessage(Arc::<str>::from(
                            crate::i18n::t_args(
                                "zc-queue-dispatch-failed",
                                // The anyhow context includes the local
                                // attachment path. Keep it out of the
                                // transcript; the cleanup notice already
                                // reports the bounded, actionable count.
                                &[("error", &e.root_cause().to_string())],
                            ),
                        )));
                    state.mark_dirty_append();
                    return;
                }
            }
        };

        let att_names: Vec<String> = attachments
            .iter()
            .map(|attachment| attachment.filename.clone())
            .collect();
        let prompt = if text.is_empty() {
            None
        } else {
            Some(text.clone())
        };
        state.own_active_turn_attachments(attachments);
        state.push_user_message(prompt, att_names);
        let turn_generation = state.turn_generation;
        Self::spawn_prompt_on(
            rpc_out,
            rpc,
            prompt_completion_tx,
            sid,
            turn_generation,
            text,
            attachments_json,
        );
    }

    fn spawn_prompt_on(
        rpc_out: &Arc<RpcOutbound>,
        rpc: &Arc<RpcClient>,
        completion_tx: &mpsc::Sender<PromptCompletion>,
        sid: String,
        turn_generation: u64,
        prompt: String,
        attachments_json: Vec<serde_json::Value>,
    ) {
        let rpc_arc = rpc_out.clone();
        let client = rpc.clone();
        let completion_tx = completion_tx.clone();
        tokio::spawn(async move {
            let mut params = serde_json::json!({
                "session_id": &sid,
                "prompt": prompt,
                "client_turn_generation": turn_generation,
            });
            if !attachments_json.is_empty() {
                params["attachments"] = serde_json::Value::Array(attachments_json);
            }
            let result = rpc_arc.request(method::SESSION_PROMPT, params).await;
            let transport_closed = result.is_err()
                && matches!(
                    client.connection_state(),
                    crate::client::ConnectionState::Disconnected { .. }
                );
            let error = result.err().map(|e| format!("{} ({})", e.message, e.code));
            let _ = completion_tx
                .send(PromptCompletion {
                    session_id: sid,
                    turn_generation,
                    error,
                    transport_closed,
                })
                .await;
        });
    }

    fn drain_git_branch_results(&mut self) {
        while let Ok(update) = self.git_branch_rx.try_recv() {
            self.git_branch_inflight = false;
            if let ChatPhase::Active(ref mut state) = self.phase
                && state.session_id == update.session_id
            {
                state.git_branch = update.branch;
                state.git_hash = update.hash;
                state.git_branch_last_fetch = Some(Instant::now());
            }
        }
    }

    fn drain_model_fetch_results(&mut self) {
        while let Ok(res) = self.model_fetch_rx.try_recv() {
            self.apply_model_fetch(res);
        }
    }

    fn apply_session_reattach_result(&mut self, update: SessionReattachResult) {
        self.session_reattach_in_flight.remove(&update.session_id);
        let Some(state) = self.state_for_session_mut(&update.session_id) else {
            return;
        };
        match update.result {
            Ok(()) => {
                state.last_error = None;
                self.pump_all_queues();
            }
            Err(error) => {
                // Keep `SessionLost` and the queued message intact so an
                // explicit retry can attempt the same dispatch again.
                state.set_info_notice(crate::i18n::t_args(
                    "zc-chat-session-switch-error",
                    &[("error", &error)],
                ));
            }
        }
    }

    fn drain_session_reattach_results(&mut self) {
        while let Ok(update) = self.session_reattach_rx.try_recv() {
            self.apply_session_reattach_result(update);
        }
    }

    /// Spawn a background `session/git_branch` poll when the cache is stale.
    /// Gated by `git_branch_inflight` so we never have more than one fetch
    /// outstanding per Chat — the daemon walks the filesystem each call and
    /// the user only sees one result at a time anyway.
    fn maybe_refresh_git_branch(&mut self) {
        if self.git_branch_inflight {
            return;
        }
        let ChatPhase::Active(ref state) = self.phase else {
            return;
        };
        if state.cwd.is_none() {
            return;
        }
        let due = state
            .git_branch_last_fetch
            .is_none_or(|t| t.elapsed() >= GIT_BRANCH_REFRESH_INTERVAL);
        if !due {
            return;
        }
        self.git_branch_inflight = true;
        let sid = state.session_id.clone();
        let rpc = self.rpc.clone();
        let tx = self.git_branch_tx.clone();
        tokio::spawn(async move {
            let result = rpc.session_git_branch(&sid).await.ok();
            let (branch, hash) = match result {
                Some(r) => (r.branch, r.hash),
                None => (None, None),
            };
            let _ = tx
                .send(GitStatusUpdate {
                    session_id: sid,
                    branch,
                    hash,
                })
                .await;
        });
    }

    // ── Drawing ──────────────────────────────────────────────────

    /// Drain transport-driven state independently of which pane is visible.
    /// The app calls this for both Chat and Code every frame so background
    /// sessions cannot silently fall behind while another screen is active.
    pub(crate) fn tick_transport_events(&mut self) {
        // The daemon enqueues a turn's terminal notification before its
        // runtime-owned session/state can become idle. Drain that same-stream
        // notification queue while the resync gate is still installed, then
        // apply the terminal barrier result and release queued prompts. This
        // is the correlation fence that prevents an old TurnComplete from
        // settling the first post-resync prompt.
        self.drain_entry_retry_results();
        self.drain_notifications();
        self.drain_session_resync_results();
        self.drain_prompt_completions();
        self.settle_stuck_cancel();
        self.drain_git_branch_results();
        self.drain_model_fetch_results();
        self.drain_session_reattach_results();
        self.maybe_refresh_git_branch();
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        match &mut self.phase {
            ChatPhase::PickAgent {
                agents,
                list_state,
                loading,
            } => {
                let list_area = draw_agent_picker(
                    frame,
                    area,
                    agents,
                    list_state,
                    *loading,
                    &self.pane_kind.name(),
                    (self.pane_kind == PaneKind::Acp)
                        .then(|| crate::i18n::t("zc-chat-agent-picker-acp-memory-note")),
                );
                self.pick_agent_list_area = list_area;
            }
            ChatPhase::PickSession {
                sessions,
                list_state,
                ..
            } => {
                render_session_list_overlay(
                    frame,
                    area,
                    sessions,
                    list_state,
                    crate::i18n::t("zc-chat-session-list-resume-title"),
                    Some(crate::i18n::t("zc-chat-session-list-resume-note")),
                );
            }
            ChatPhase::PickCwd { explorer, .. } => {
                explorer.render(frame, area);
            }
            ChatPhase::Active(state) => {
                render(frame, state, area, self.pane_kind);
            }
            ChatPhase::Error(msg) => {
                draw_error(frame, area, msg, &self.pane_kind.name());
            }
        }
    }

    // ── Key handling ─────────────────────────────────────────────

    pub(crate) async fn handle_key(
        &mut self,
        key: KeyEvent,
        term: &mut crate::config_manager::Term,
    ) -> bool {
        // Determine which phase we're in without holding a borrow on self.
        // For the picker, extract what we need; for active, delegate below.
        match &mut self.phase {
            ChatPhase::PickAgent {
                agents,
                list_state,
                loading,
            } => {
                if *loading {
                    return false;
                }
                use crate::keymap::{ChatTabAction, GlobalAction, ModalAction};
                // Three action types in scope here — explicit short-circuit
                // chain instead of one mixed match.
                match ModalAction::from_chord(&key) {
                    Some(ModalAction::Confirm) => {
                        if let Some(i) = list_state.selected()
                            && let Some(alias) = agents.get(i).cloned()
                        {
                            self.focus_agent(&alias).await;
                        }
                        return false;
                    }
                    Some(ModalAction::Cancel) => {
                        // With a stashed session (picker opened from a live
                        // chat), Esc returns to it; at startup it still quits.
                        return !self.restore_last_focused().await;
                    }
                    _ => {}
                }
                if GlobalAction::from_chord(&key) == Some(GlobalAction::Quit) {
                    return true;
                }
                match ChatTabAction::from_chord(&key) {
                    Some(ChatTabAction::BrowseUp) | Some(ChatTabAction::BrowseUpVim) => {
                        let i = list_state.selected().unwrap_or(0);
                        list_state.select(Some(i.saturating_sub(1)));
                    }
                    Some(ChatTabAction::BrowseDown) | Some(ChatTabAction::BrowseDownVim) => {
                        let i = list_state.selected().unwrap_or(0);
                        if i + 1 < agents.len() {
                            list_state.select(Some(i + 1));
                        }
                    }
                    _ => {}
                }
                return false;
            }
            ChatPhase::PickSession {
                sessions,
                list_state,
                agents,
            } => {
                use crate::keymap::{ChatTabAction, ModalAction};
                if ModalAction::from_chord(&key) == Some(ModalAction::Confirm) {
                    if let Some(i) = list_state.selected()
                        && let Some(entry) = sessions.get(i).cloned()
                    {
                        self.resume_session_entry(entry).await;
                    }
                    return false;
                }
                if ModalAction::from_chord(&key) == Some(ModalAction::Cancel)
                    || ChatTabAction::from_chord(&key) == Some(ChatTabAction::NewSession)
                {
                    let agents = agents.clone();
                    self.start_fresh_from_picker(agents).await;
                    return false;
                }
                match ModalAction::from_chord(&key) {
                    Some(ModalAction::Up) => {
                        let i = list_state.selected().unwrap_or(0);
                        list_state.select(Some(i.saturating_sub(1)));
                    }
                    Some(ModalAction::Down) => {
                        let i = list_state.selected().unwrap_or(0);
                        if i + 1 < sessions.len() {
                            list_state.select(Some(i + 1));
                        }
                    }
                    _ => {}
                }
                return false;
            }
            ChatPhase::PickCwd {
                agent_alias,
                explorer,
            } => {
                let action = explorer.handle_key(key);
                match action {
                    ExplorerAction::ConfirmDir(path) => {
                        let alias = agent_alias.clone();
                        let cwd_str = path.to_str().map(str::to_string);
                        self.start_session(&alias, cwd_str.as_deref()).await;
                    }
                    ExplorerAction::Cancel => {
                        // With live background sessions, a cancelled CWD pick
                        // returns to the previously focused session instead of
                        // the agent picker.
                        if !self.restore_last_focused().await {
                            self.phase = ChatPhase::PickAgent {
                                agents: Vec::new(),
                                list_state: ListState::default(),
                                loading: true,
                            };
                            // Re-fetch agents asynchronously.
                            let _ = self.init().await;
                        }
                    }
                    ExplorerAction::Confirm(_) | ExplorerAction::None => {}
                }
                return false;
            }
            ChatPhase::Error(_) => {
                use crate::keymap::{ChatTabAction, GlobalAction};
                return GlobalAction::from_chord(&key) == Some(GlobalAction::Quit)
                    || ChatTabAction::from_chord(&key) == Some(ChatTabAction::ErrorDismiss);
            }
            ChatPhase::Active(_) => { /* handled below to avoid borrow conflict */ }
        }

        // Active phase — borrow state directly to avoid double &mut self.
        let ChatPhase::Active(ref mut state) = self.phase else {
            return false;
        };

        // ── Model / model_provider picker overlay key handling ───
        // Takes priority over all other Active-phase keys while open.
        if state.model_picker.is_open() {
            use crate::keymap::ModalAction;

            let action = ModalAction::from_chord(&key);
            let up = action == Some(ModalAction::Up);
            let down = action == Some(ModalAction::Down);

            // Movement first.
            if up || down {
                match &mut state.model_picker {
                    ModelPickerOverlay::Model(p)
                    | ModelPickerOverlay::ConfiguredProviderStage(p) => {
                        if up {
                            p.move_up();
                        } else {
                            p.move_down();
                        }
                    }
                    ModelPickerOverlay::Loading | ModelPickerOverlay::None => {}
                }
                state.mark_dirty_full();
                return false;
            }

            match action {
                Some(ModalAction::Cancel) => {
                    state.model_picker = ModelPickerOverlay::None;
                    state.mark_dirty_full();
                    return false;
                }
                Some(ModalAction::Confirm) => {
                    let rpc = self.rpc.clone();
                    Self::confirm_model_picker_selection(&rpc, state).await;
                    return false;
                }
                _ => {
                    // Any other key while the picker is open is swallowed so it
                    // doesn't leak into the input bar.
                    return false;
                }
            }
        }

        if state.pending_elicitation.is_some() {
            use crate::keymap::ModalAction;
            let action = ModalAction::from_chord(&key);

            // Multi-select toggle on Space. Single-select ignores Space.
            if action == Some(ModalAction::Toggle) {
                let mut toggled = false;
                if let Some(e) = state.pending_elicitation.as_mut()
                    && e.multi
                    && let Some(slot) = e.selected.get_mut(e.cursor)
                {
                    *slot = !*slot;
                    toggled = true;
                }
                if toggled {
                    state.mark_dirty_full();
                }
                return false;
            }

            match action {
                Some(ModalAction::Up) => {
                    if let Some(e) = state.pending_elicitation.as_mut() {
                        e.cursor = e.cursor.saturating_sub(1);
                    }
                    state.mark_dirty_full();
                    return false;
                }
                Some(ModalAction::Down) => {
                    if let Some(e) = state.pending_elicitation.as_mut()
                        && e.cursor + 1 < e.choices.len()
                    {
                        e.cursor += 1;
                    }
                    state.mark_dirty_full();
                    return false;
                }
                Some(ModalAction::Confirm) => {
                    // Build the response without holding the modal borrow,
                    // then answer the daemon. For an invalid multi-select
                    // (bounds unmet) keep the modal open.
                    let payload = state
                        .pending_elicitation
                        .as_ref()
                        .and_then(|e| e.accept_content().map(|c| (e.request_id.clone(), c)));
                    if let Some((id, content)) = payload {
                        state.pending_elicitation = None;
                        state.mark_dirty_full();
                        self.rpc.respond_to_inbound_request(
                            id,
                            Ok(serde_json::json!({
                                "action": "accept",
                                "content": content
                            })),
                        );
                    }
                    // else: invalid selection — swallow, leave modal up.
                    return false;
                }
                Some(ModalAction::Cancel) => {
                    if let Some(e) = state.pending_elicitation.take() {
                        state.mark_dirty_full();
                        let id = e.request_id;
                        self.rpc.respond_to_inbound_request(
                            id,
                            Ok(serde_json::json!({ "action": "cancel" })),
                        );
                    }
                    return false;
                }
                _ => {
                    // Swallow every other key so the prompt stays modal and
                    // nothing leaks into the input bar.
                    return false;
                }
            }
        }

        // ── Session overlay key handling ─────────────────────────
        let mut handled_session_overlay = false;
        let mut confirm_session = None;
        if let SessionOverlay::List {
            sessions,
            list_state,
        } = &mut state.session_overlay
        {
            handled_session_overlay = true;
            use crate::keymap::ModalAction;
            match ModalAction::from_chord(&key) {
                Some(ModalAction::Cancel) => {
                    state.session_overlay = SessionOverlay::None;
                }
                Some(ModalAction::Confirm) => {
                    if let Some(i) = list_state.selected() {
                        confirm_session = sessions.get(i).cloned();
                    }
                }
                Some(ModalAction::Up) => {
                    let i = list_state.selected().unwrap_or(0);
                    list_state.select(Some(i.saturating_sub(1)));
                }
                Some(ModalAction::Down) => {
                    let i = list_state.selected().unwrap_or(0);
                    if i + 1 < sessions.len() {
                        list_state.select(Some(i + 1));
                    }
                }
                _ => {}
            }
        }
        if handled_session_overlay {
            if let Some(entry) = confirm_session {
                self.switch_to_session_entry(entry).await;
            }
            return false;
        }

        // The transcript or queue context menu is modal within an active chat. Handle
        // it before selection clearing or input dispatch so Esc cannot leak
        // into the editor and Enter cannot submit a prompt.
        if state.context_menu.is_some() {
            use crate::keymap::ModalAction;
            let request = match ModalAction::from_chord(&key) {
                Some(ModalAction::Up) => {
                    state.context_menu_select_step(-1);
                    None
                }
                Some(ModalAction::Down) => {
                    state.context_menu_select_step(1);
                    None
                }
                Some(ModalAction::Confirm) => state.take_context_menu_request(),
                Some(ModalAction::Cancel) => {
                    state.dismiss_context_menu();
                    None
                }
                _ => None,
            };
            if let Some(request) = request {
                self.execute_context_menu_request(request).await;
            }
            return false;
        }

        // Input-bar overlays are modal within the input surface. Higher
        // overlays above have already had first refusal; handle them before
        // queue, browse, and other pane-level shortcuts.
        if state.handle_input_bar_overlay_key(key) {
            return false;
        }

        {
            use crate::keymap::ChatTabAction as QAction;
            let qaction = QAction::from_chord(&key);
            match qaction {
                Some(QAction::PauseResumeQueue) => {
                    let paused = state.toggle_queue_pause();
                    if paused {
                        // The paused state is shown as ghost text in the empty
                        // input bar, so no info-bar notice is needed here.
                        state.clear_info_notice();
                    } else {
                        state.set_info_notice(crate::i18n::t("zc-queue-resumed"));
                        self.retry_failed_resyncs_with_queued_input();
                        self.pump_all_queues();
                    }
                    return false;
                }
                Some(QAction::QueueNavUp) if state.queue_sidebar_open() => {
                    state.queue_select_step(-1);
                    return false;
                }
                Some(QAction::QueueNavDown) if state.queue_sidebar_open() => {
                    state.queue_select_step(1);
                    return false;
                }
                Some(
                    action @ (QAction::QueueSendNow
                    | QAction::QueueCopy
                    | QAction::QueueDelete
                    | QAction::QueueEdit),
                ) if state.queue_sidebar_open() => {
                    let action = match action {
                        QAction::QueueSendNow => ChatContextMenuAction::SendNow,
                        QAction::QueueCopy => ChatContextMenuAction::Copy,
                        QAction::QueueEdit => ChatContextMenuAction::Edit,
                        QAction::QueueDelete => ChatContextMenuAction::Delete,
                        _ => unreachable!(),
                    };
                    if let Some(id) = state.selected_queue_id() {
                        let request = ChatContextMenuRequest::Queue { id, action };
                        self.execute_context_menu_request(request).await;
                    }
                    return false;
                }
                Some(QAction::QueueWiden) if state.queue_sidebar_open() => {
                    state.widen_queue_sidebar();
                    return false;
                }
                Some(QAction::QueueNarrow) if state.queue_sidebar_open() => {
                    state.narrow_queue_sidebar();
                    return false;
                }
                _ => {}
            }
        }

        // Copy must run before the general "any key clears mouse highlight"
        // path below. Otherwise Command+C / Ctrl+Shift+C would erase a
        // character-level transcript selection before extracting it.
        if should_copy_current_selection(state, &key) {
            state.copy_current_selection();
            return false;
        }

        // Any key press clears the mouse-click highlight — the user is done
        // with visual selection and is interacting via keyboard.
        state.clear_mouse_highlight();

        // ── Auto-exit browse mode on typing keys ─────────────────
        // If the user pressed a printable key that isn't a browse-mode
        // navigation key (j/k/↑/↓/Esc/Enter/Ctrl+C), exit browse mode
        // so they can type without an extra Esc press.
        if state.in_browse_mode() {
            let is_browse_key = {
                use crate::keymap::ChatTabAction;
                matches!(
                    ChatTabAction::from_chord(&key),
                    Some(
                        ChatTabAction::BrowseEnter
                            | ChatTabAction::BrowseUp
                            | ChatTabAction::BrowseDown
                            | ChatTabAction::BrowseUpVim
                            | ChatTabAction::BrowseDownVim
                            | ChatTabAction::BrowseSelectExtend
                            | ChatTabAction::BrowseSelectExtendDown
                            | ChatTabAction::BrowseExitSelection
                            | ChatTabAction::CopySelection
                            | ChatTabAction::CopyAllVisible
                    )
                )
            };
            if !is_browse_key {
                state.exit_browse_mode();
            }
        }

        if state.pending_approval().is_none() && !state.turn_in_flight {
            use crate::keymap::ChatTabAction;
            if let Some(ChatTabAction::BrowseEnter) = ChatTabAction::from_chord(&key) {
                if state.in_browse_mode() {
                    state.browse_move_up(1, false);
                } else {
                    state.enter_browse_mode();
                }
                return false;
            }
        }

        use crate::keymap::ChatTabAction;
        let chat_action = ChatTabAction::from_chord(&key);
        let chat_action_bypasses_text_input = chat_action.is_some_and(|action| {
            !matches!(action, ChatTabAction::ApprovalApprove)
                && crate::keymap::action_bypasses_text_input(action, &key)
        });

        // Enter (slash commands + submit), text input, cursor, backspace.
        // It does NOT handle approval, selection, session management, etc.
        if state.composer_owns_text_input() && !chat_action_bypasses_text_input {
            let action = state.input_bar.handle_key(key);
            match action {
                InputBarAction::Submit { text, attachments } => {
                    state.clear_info_notice();
                    state.resume_queue();
                    let prompt = text.unwrap_or_default();
                    let enq = state.enqueue_message(prompt, attachments);
                    self.after_enqueue(enq);
                    return false;
                }
                InputBarAction::Inject { text, attachments } => {
                    state.clear_info_notice();
                    let prompt = text.unwrap_or_default();
                    let enq = state.inject_message(prompt, attachments);
                    if enq.is_ok() {
                        self.cancel_active_turn_for_injection().await;
                    }
                    self.after_enqueue(enq);
                    return false;
                }
                InputBarAction::StatusMessage(msg) => {
                    let cleanup_notice = state.input_bar.take_cleanup_report().notice();
                    state.set_info_notice(append_cleanup_notice(msg, cleanup_notice));
                    return false;
                }
                InputBarAction::ToggleThinking => {
                    state.show_thoughts = !state.show_thoughts;
                    state.mark_dirty_full();
                    let status = if state.show_thoughts {
                        crate::i18n::t("zc-chat-thinking-visible")
                    } else {
                        crate::i18n::t("zc-chat-thinking-hidden")
                    };
                    state
                        .entries
                        .push(ChatEntry::SystemMessage(Arc::<str>::from(status)));
                    state.mark_dirty_append();
                    return false;
                }
                InputBarAction::EnterBrowseMode => {
                    state.enter_browse_mode();
                    return false;
                }
                InputBarAction::OpenHelp => {
                    self.help_requested = true;
                    return false;
                }
                InputBarAction::ClearQueue(idx) => {
                    let notice = append_cleanup_notice(
                        state.clear_queue_cmd(idx),
                        state.input_bar.take_cleanup_report().notice(),
                    );
                    state.set_info_notice(notice);
                    return false;
                }
                InputBarAction::RestartSession => {
                    let rpc = self.rpc.clone();
                    let pane_kind = self.pane_kind;
                    let old_sid = state.session_id.clone();
                    if let Some(next_phase) =
                        Self::restart_session_for_state(&rpc, pane_kind, state).await
                    {
                        self.phase = next_phase;
                    }
                    self.note_session_replaced(&old_sid);
                    return false;
                }
                InputBarAction::ResumeQueue => {
                    state.clear_info_notice();
                    if state.resume_queue() {
                        self.retry_failed_resyncs_with_queued_input();
                        self.pump_all_queues();
                    }
                    return false;
                }
                InputBarAction::SetModel(model) => {
                    let rpc = self.rpc.clone();
                    Self::apply_session_override(
                        &rpc,
                        state,
                        crate::client::SessionOverrides {
                            model: Some(model),
                            ..Default::default()
                        },
                    )
                    .await;
                    return false;
                }
                InputBarAction::SetModelProvider(model_provider) => {
                    let rpc = self.rpc.clone();
                    Self::apply_session_override(
                        &rpc,
                        state,
                        crate::client::SessionOverrides {
                            model_provider: Some(model_provider),
                            ..Default::default()
                        },
                    )
                    .await;
                    return false;
                }
                InputBarAction::OpenModelPicker => {
                    let rpc = self.rpc.clone();
                    let tx = self.model_fetch_tx.clone();
                    Self::open_model_picker(&rpc, &tx, state).await;
                    return false;
                }
                InputBarAction::OpenModelProviderPicker => {
                    let rpc = self.rpc.clone();
                    Self::open_provider_picker(&rpc, state).await;
                    return false;
                }
                InputBarAction::Consumed => {
                    if let Some(message) = state.input_bar.take_cleanup_report().notice() {
                        state.set_info_notice(message);
                    }
                    return false;
                }
                InputBarAction::NotHandled => { /* fall through to chat-specific keys */ }
            }
        }

        // ── Chat-specific key handling ───────────────────────────
        use crate::keymap::GlobalAction;
        // Quit chord wins (chat overrides conditionally on turn state below).
        if GlobalAction::from_chord(&key) == Some(GlobalAction::Quit) {
            if state.turn_in_flight {
                if !matches!(state.turn_status, TurnStatus::Cancelling) {
                    let res = self.rpc.session_cancel(&state.session_id).await;
                    if res.is_ok() {
                        state.enter_cancelling();
                    } else {
                        state.commit_turn(String::new(), false);
                    }
                }
            } else {
                return true;
            }
            return false;
        }
        match chat_action {
            Some(ChatTabAction::BrowseExitSelection) => {
                if state.in_browse_mode() {
                    state.exit_browse_mode();
                } else if state.turn_in_flight
                    && !matches!(state.turn_status, TurnStatus::Cancelling)
                {
                    let res = self.rpc.session_cancel(&state.session_id).await;
                    if res.is_ok() {
                        state.enter_cancelling();
                    } else {
                        state.commit_turn(String::new(), false);
                    }
                }
            }
            Some(ChatTabAction::ApprovalApprove) if state.pending_approval().is_some() => {
                if let Some(pa) = state.take_pending_approval() {
                    let _ = self
                        .rpc
                        .session_approve(
                            &state.session_id,
                            &pa.request_id,
                            ApprovalDecision::AllowOnce,
                        )
                        .await;
                }
            }
            Some(ChatTabAction::CancelTurn) if state.pending_approval().is_some() => {
                if let Some(pa) = state.take_pending_approval() {
                    let _ = self
                        .rpc
                        .session_approve(
                            &state.session_id,
                            &pa.request_id,
                            ApprovalDecision::Reject,
                        )
                        .await;
                }
            }
            Some(ChatTabAction::ApprovalApproveAll) if state.pending_approval().is_some() => {
                if let Some(pa) = state.take_pending_approval() {
                    let _ = self
                        .rpc
                        .session_approve(
                            &state.session_id,
                            &pa.request_id,
                            ApprovalDecision::AllowAlways,
                        )
                        .await;
                }
            }
            Some(ChatTabAction::ApprovalApproveEdit) if state.pending_approval().is_some() => {
                let is_edit_tool = state
                    .pending_approval()
                    .map(|pa| matches!(pa.tool_name.as_str(), "file_edit" | "file_write"))
                    .unwrap_or(false);
                if is_edit_tool && let Some(pa) = state.take_pending_approval() {
                    let initial = pa.arguments_summary.clone();
                    let edited = open_editor_for_content(&initial).await;
                    let _ = term.clear();
                    let _ = self
                        .rpc
                        .session_approve(
                            &state.session_id,
                            &pa.request_id,
                            ApprovalDecision::RejectWithEdit {
                                replacement: edited,
                            },
                        )
                        .await;
                }
            }
            Some(ChatTabAction::NewSession) if !state.turn_in_flight => {
                let rpc = self.rpc.clone();
                let pane_kind = self.pane_kind;
                let old_sid = state.session_id.clone();
                if let Some(next_phase) =
                    Self::restart_session_for_state(&rpc, pane_kind, state).await
                {
                    self.phase = next_phase;
                }
                self.note_session_replaced(&old_sid);
            }
            Some(ChatTabAction::SwitchSession) => {
                // ACP and Chat live in separate stores and must not cross-pick:
                //  • Chat → unified session_backend (filter out channel-backed
                //    sessions; those are owned by the channels pane).
                //  • ACP  → dedicated acp-sessions.db, listed by a separate RPC.
                let picker_sessions = if self.pane_kind == PaneKind::Acp {
                    self.rpc
                        .acp_session_list()
                        .await
                        .map(|list| list.sessions)
                        .unwrap_or_default()
                } else {
                    match self.rpc.session_list(None).await {
                        Ok(list) => list
                            .sessions
                            .into_iter()
                            .filter(|s| s.channel_id.is_none())
                            .collect(),
                        Err(_) => Vec::new(),
                    }
                };

                let mut ls = ListState::default();
                if !picker_sessions.is_empty() {
                    ls.select(Some(0));
                }
                state.session_overlay = SessionOverlay::List {
                    sessions: picker_sessions,
                    list_state: ls,
                };
            }
            Some(ChatTabAction::ToggleThoughts)
                if state.input_bar.input().is_empty()
                    && state.pending_approval().is_none()
                    && !state.in_browse_mode() =>
            {
                state.show_thoughts = !state.show_thoughts;
                state.mark_dirty_full();
            }
            Some(ChatTabAction::TodoToggle) => {
                state.todo_tracker.toggle();
                state.todo_close_hit_rect = None;
                state.mark_dirty_full();
            }
            Some(ChatTabAction::BrowseEnter) => {
                if state.in_browse_mode() {
                    state.browse_move_up(1, false);
                } else {
                    state.enter_browse_mode();
                }
            }
            Some(ChatTabAction::BrowseExit) if state.in_browse_mode() => {
                state.exit_browse_mode();
            }
            Some(ChatTabAction::BrowseUp) => {
                if state.in_browse_mode() {
                    state.browse_move_up(1, false);
                } else if !state.pinned_to_bottom {
                    state.scroll_up(1);
                }
            }
            Some(ChatTabAction::BrowseDown) => {
                if state.in_browse_mode() {
                    state.browse_move_down(1, false);
                } else if !state.pinned_to_bottom {
                    state.scroll_down(1);
                }
            }
            Some(ChatTabAction::BrowseSelectExtend) => {
                if state.in_browse_mode() {
                    state.browse_move_up(1, true);
                } else {
                    state.scroll_up(1);
                }
            }
            Some(ChatTabAction::BrowseSelectExtendDown) => {
                if state.in_browse_mode() {
                    state.browse_move_down(1, true);
                } else {
                    state.scroll_down(1);
                }
            }
            Some(ChatTabAction::FastScrollUp) => {
                state.scroll_up(5);
            }
            Some(ChatTabAction::FastScrollDown) => {
                state.scroll_down(5);
            }
            Some(ChatTabAction::ScrollUp) => {
                state.scroll_up(1);
            }
            Some(ChatTabAction::ScrollDown) => {
                state.scroll_down(1);
            }
            Some(ChatTabAction::PageUp) => {
                state.page_up();
            }
            Some(ChatTabAction::PageDown) => {
                state.page_down();
            }
            Some(ChatTabAction::JumpStart) => {
                state.scroll_to_top();
            }
            Some(ChatTabAction::JumpEnd) => {
                state.scroll_to_bottom();
            }
            Some(ChatTabAction::BrowseUpVim)
                if state.in_browse_mode()
                    && state.pending_approval().is_none()
                    && !state.turn_in_flight =>
            {
                state.browse_move_up(1, false);
            }
            Some(ChatTabAction::BrowseDownVim)
                if state.in_browse_mode()
                    && state.pending_approval().is_none()
                    && !state.turn_in_flight =>
            {
                state.browse_move_down(1, false);
            }
            _ => {}
        }
        false
    }

    async fn handle_model_picker_mouse(
        rpc: &Arc<RpcClient>,
        mouse: MouseEvent,
        area: Rect,
        state: &mut ChatState,
    ) {
        let Some(modal_rect) = model_picker_overlay_area(&state.model_picker, area) else {
            return;
        };

        let col = mouse.column;
        let row = mouse.row;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !mouse::in_rect(col, row, modal_rect) {
                    state.model_picker = ModelPickerOverlay::None;
                    state.mark_dirty_full();
                    return;
                }

                let item_count = state.model_picker.item_count();
                if let Some(idx) = mouse::list_click_index(row, modal_rect, 0, item_count) {
                    if let Some(picker) = state.model_picker.picker_mut() {
                        picker.cursor = idx;
                    }
                    Self::confirm_model_picker_selection(rpc, state).await;
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if mouse::in_rect(col, row, modal_rect) =>
            {
                if let Some(picker) = state.model_picker.picker_mut() {
                    if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                        picker.move_up();
                    } else {
                        picker.move_down();
                    }
                    state.mark_dirty_full();
                }
            }
            _ => {}
        }
    }

    /// Session-picker confirm: focus the picked session if this pane already
    /// tracks it, else attach it as a new tracked session. The previously
    /// focused session stays live in `background` (no `session/close`).
    async fn switch_to_session_entry(&mut self, entry: crate::client::SessionEntry) {
        let new_sid = entry.session_id;
        let new_name = entry.name;

        // Dismiss the overlay on the currently focused state first.
        let (active_sid, fallback_alias) = match &mut self.phase {
            ChatPhase::Active(state) => {
                state.session_overlay = SessionOverlay::None;
                state.mark_dirty_full();
                (Some(state.session_id.clone()), state.agent_alias.clone())
            }
            _ => (None, String::new()),
        };
        if active_sid.as_deref() == Some(new_sid.as_str()) {
            return;
        }
        let agent_alias = entry.agent_alias.unwrap_or(fallback_alias);

        if self.background.iter().any(|s| s.session_id == new_sid) {
            self.focus_session(&new_sid).await;
            return;
        }

        if self.tracked_session_count() >= MAX_TRACKED_SESSIONS_PER_PANE {
            if let ChatPhase::Active(ref mut state) = self.phase {
                state.set_info_notice(crate::i18n::t_args(
                    "zc-chat-session-cap",
                    &[("max", &MAX_TRACKED_SESSIONS_PER_PANE.to_string())],
                ));
            }
            return;
        }

        match self.attach_session(&agent_alias, &new_sid).await {
            Ok(mut state) => {
                state.session_name = new_name;
                if !self.session_order.contains(&new_sid) {
                    self.session_order.push(new_sid);
                }
                self.stash_active();
                self.phase = ChatPhase::Active(Box::new(state));
            }
            Err(e) => {
                if let ChatPhase::Active(ref mut state) = self.phase {
                    state.info_message = Some(crate::widgets::InfoMessage::error(
                        crate::i18n::t_args("zc-chat-session-switch-error", &[("error", &e)]),
                    ));
                    state.mark_dirty_full();
                }
            }
        }
    }

    /// Apply a session override (model and/or model_provider) to the active
    /// session via `session/configure`, reporting the outcome on the info bar.
    /// On a model_provider switch the daemon rebuilds the provider box live.
    async fn apply_session_override(
        rpc: &RpcClient,
        state: &mut ChatState,
        overrides: crate::client::SessionOverrides,
    ) {
        let waiting = crate::widgets::InfoMessage::info(crate::i18n::t("zc-model-switch-applying"));
        state.info_message = Some(waiting);
        state.mark_dirty_full();

        match rpc.session_configure(&state.session_id, overrides).await {
            Ok(result) => {
                let model = result.overrides.model.unwrap_or_default();
                let model_provider = result.overrides.model_provider.unwrap_or_default();
                let summary = if !model_provider.is_empty() {
                    crate::i18n::t_args(
                        "zc-model-switch-provider-ok",
                        &[("provider", &model_provider), ("model", &model)],
                    )
                } else {
                    crate::i18n::t_args("zc-model-switch-model-ok", &[("model", &model)])
                };
                state.info_message = Some(crate::widgets::InfoMessage::note(summary));
                let provider_ref = (!model_provider.is_empty()).then_some(model_provider.as_str());
                let resolved_model = if !model.is_empty() {
                    Some(model.clone())
                } else if let Some(r) = provider_ref {
                    Self::configured_model(rpc, r).await
                } else {
                    None
                };
                state.set_model_identity(provider_ref, resolved_model.as_deref());
                // A model_provider switch changes the catalog — drop the cache
                // so the next `/model` use refetches.
                if provider_ref.is_some() {
                    state.input_bar.set_model_catalog(String::new(), Vec::new());
                }
            }
            Err(e) => {
                state.info_message = Some(crate::widgets::InfoMessage::error(crate::i18n::t_args(
                    "zc-model-switch-failed",
                    &[("error", &e.to_string())],
                )));
            }
        }
        state.mark_dirty_full();
    }

    async fn refresh_model_identity(rpc: &RpcClient, state: &mut ChatState) {
        if let Some(provider_ref) = Self::resolve_model_provider_ref(rpc, &state.agent_alias).await
        {
            let model = Self::configured_model(rpc, &provider_ref).await;
            state.set_model_identity(Some(&provider_ref), model.as_deref());
        }
    }

    /// Resolve the agent's configured model_provider reference (`<type>.<alias>`)
    /// from config.
    async fn resolve_model_provider_ref(rpc: &RpcClient, agent_alias: &str) -> Option<String> {
        let prop = format!("agents.{agent_alias}.model_provider");
        let entries = rpc.config_list(Some(&prop)).await.ok()?;
        entries.into_iter().find(|e| e.path == prop).and_then(|e| {
            e.value
                .as_ref()
                .and_then(|v| v.as_str().map(str::to_string))
        })
    }

    /// Read the model configured for a dotted model_provider ref
    /// (`providers.models.<family>.<alias>.model`), used to pre-select the
    /// current model in the picker.
    async fn configured_model(rpc: &RpcClient, model_provider_ref: &str) -> Option<String> {
        let prop = format!("providers.models.{model_provider_ref}.model");
        let entries = rpc.config_list(Some(&prop)).await.ok()?;
        entries.into_iter().find(|e| e.path == prop).and_then(|e| {
            e.value
                .as_ref()
                .and_then(|v| v.as_str().map(str::to_string))
        })
    }

    /// Fetch the model catalog for the full model_provider reference. Returns an empty vec
    /// on failure; the caller surfaces the error on the info bar.
    async fn fetch_models(rpc: &RpcClient, model_provider_ref: &str) -> Vec<String> {
        match rpc.catalog_models(model_provider_ref).await {
            Ok(res) => res.models,
            Err(_) => Vec::new(),
        }
    }

    /// Open the single-stage model picker for the active agent's model_provider,
    /// pre-selecting the currently-configured model.
    async fn open_model_picker(
        rpc: &Arc<RpcClient>,
        model_fetch_tx: &mpsc::Sender<ModelFetchResult>,
        state: &mut ChatState,
    ) {
        let active_provider = match state.model_provider_ref.clone() {
            Some(r) => Some(r),
            None => Self::resolve_model_provider_ref(rpc, &state.agent_alias).await,
        };
        let Some(model_provider_ref) = active_provider else {
            state.info_message = Some(crate::widgets::InfoMessage::error(crate::i18n::t(
                "zc-model-catalog-no-provider",
            )));
            state.mark_dirty_full();
            return;
        };
        // Warm cache: open immediately, no fetch, no loading state.
        if state.input_bar.model_catalog_provider() == Some(model_provider_ref.as_str())
            && !state.input_bar.model_catalog().is_empty()
        {
            let models = state.input_bar.model_catalog().to_vec();
            let current = match state.model.clone() {
                Some(m) => Some(m),
                None => Self::configured_model(rpc, &model_provider_ref).await,
            };
            state.model_picker = ModelPickerOverlay::Model(crate::widgets::PickerState::new(
                models,
                current.as_deref(),
            ));
            state.info_message = None;
            state.mark_dirty_full();
            return;
        }

        // Cold cache: show the Loading modal now and fetch off the draw loop so
        // the waiting state actually paints. The result returns over
        // model_fetch_tx and is drained in refresh_if_inactive.
        state.model_picker = ModelPickerOverlay::Loading;
        state.info_message = Some(crate::widgets::InfoMessage::info(crate::i18n::t(
            "zc-model-catalog-loading",
        )));
        state.mark_dirty_full();

        let rpc = rpc.clone();
        let tx = model_fetch_tx.clone();
        let session_id = state.session_id.clone();
        let session_model = state.model.clone();
        tokio::spawn(async move {
            let models = Self::fetch_models(&rpc, &model_provider_ref).await;
            let current = match session_model {
                Some(m) => Some(m),
                None => Self::configured_model(&rpc, &model_provider_ref).await,
            };
            let _ = tx
                .send(ModelFetchResult {
                    session_id,
                    model_provider_ref,
                    models,
                    current,
                })
                .await;
        });
    }

    /// Apply a completed background catalog fetch: swap the Loading picker to
    /// the populated list (or surface an empty-catalog error), and warm the
    /// autocomplete cache. Ignores results for a session that has since
    /// changed or a picker the user already dismissed.
    fn apply_model_fetch(&mut self, res: ModelFetchResult) {
        let ChatPhase::Active(state) = &mut self.phase else {
            return;
        };
        if state.session_id != res.session_id {
            return;
        }
        if !matches!(state.model_picker, ModelPickerOverlay::Loading) {
            return;
        }
        if res.models.is_empty() {
            state.model_picker = ModelPickerOverlay::None;
            state.info_message = Some(crate::widgets::InfoMessage::error(crate::i18n::t(
                "zc-model-catalog-empty",
            )));
            state.mark_dirty_full();
            return;
        }
        state
            .input_bar
            .set_model_catalog(res.model_provider_ref, res.models.clone());
        state.model_picker = ModelPickerOverlay::Model(crate::widgets::PickerState::new(
            res.models,
            res.current.as_deref(),
        ));
        state.info_message = None;
        state.mark_dirty_full();
    }

    /// Open stage 1 of the two-stage model_provider picker.
    async fn open_provider_picker(rpc: &RpcClient, state: &mut ChatState) {
        match rpc.quickstart_state().await {
            Ok(snap) => {
                let providers = snap.model_providers;
                if providers.is_empty() {
                    state.info_message = Some(crate::widgets::InfoMessage::error(crate::i18n::t(
                        "zc-model-catalog-no-provider",
                    )));
                    state.mark_dirty_full();
                    return;
                }
                let current = match state.model_provider_ref.clone() {
                    Some(r) => Some(r),
                    None => Self::resolve_model_provider_ref(rpc, &state.agent_alias).await,
                };
                state.input_bar.set_provider_catalog(providers.clone());
                state.model_picker = ModelPickerOverlay::ConfiguredProviderStage(
                    crate::widgets::PickerState::new(providers, current.as_deref()),
                );
                state.mark_dirty_full();
            }
            Err(e) => {
                state.info_message = Some(crate::widgets::InfoMessage::error(crate::i18n::t_args(
                    "zc-model-provider-catalog-failed",
                    &[("error", &e.to_string())],
                )));
                state.mark_dirty_full();
            }
        }
    }

    async fn open_agent_picker(&mut self, current_alias: String) {
        let agents = match self.rpc.agents_status().await {
            Ok(result) => result
                .agents
                .into_iter()
                .filter(|agent| agent.enabled)
                .map(|agent| agent.alias)
                .collect::<Vec<_>>(),
            Err(e) => {
                if let ChatPhase::Active(state) = &mut self.phase {
                    state.info_message =
                        Some(crate::widgets::InfoMessage::error(crate::i18n::t_args(
                            "zc-chat-error-fetch-agents",
                            &[("error", &e.to_string())],
                        )));
                    state.mark_dirty_full();
                }
                return;
            }
        };

        if agents.len() <= 1 {
            return;
        }

        let selected = agents
            .iter()
            .position(|agent| agent == &current_alias)
            .unwrap_or(0);
        let mut list_state = ListState::default();
        list_state.select(Some(selected));

        self.resume_focused = None;
        // The focused session stays tracked while the picker is up: picking
        // an agent adds/focuses a session, Esc returns to it.
        self.stash_active();
        self.phase = ChatPhase::PickAgent {
            agents,
            list_state,
            loading: false,
        };
    }

    pub(crate) fn finish_transcript_drag_if_released(&mut self, mouse: &MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left))
            && let ChatPhase::Active(state) = &mut self.phase
        {
            state.finish_transcript_drag();
        }
    }

    pub(crate) async fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        self.finish_transcript_drag_if_released(&mouse);

        // Dir-picker explorer handles its own mouse events.
        if let ChatPhase::PickCwd { explorer, .. } = &mut self.phase {
            explorer.handle_mouse(mouse);
            return;
        }

        if matches!(self.phase, ChatPhase::PickSession { .. }) {
            let mut confirm_session: Option<SessionEntry> = None;
            if let ChatPhase::PickSession {
                sessions,
                list_state,
                ..
            } = &mut self.phase
            {
                let overlay_area = session_list_overlay_area(area);
                // The resume picker renders the memory-isolation note in its
                // footer; clicks there must not resolve to (possibly hidden)
                // session rows, so hit-test against the note-free list rect.
                let note = crate::i18n::t("zc-chat-session-list-resume-note");
                let click_area = session_list_click_area(overlay_area, Some(&note));
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left)
                        if mouse::in_rect(mouse.column, mouse.row, overlay_area) =>
                    {
                        if let Some(idx) = mouse::list_click_index(
                            mouse.row,
                            click_area,
                            list_state.offset(),
                            sessions.len(),
                        ) {
                            list_state.select(Some(idx));
                            if self
                                .session_list_double_click
                                .click(mouse.column, mouse.row)
                            {
                                confirm_session = sessions.get(idx).cloned();
                            }
                        }
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                        if mouse::in_rect(mouse.column, mouse.row, overlay_area) =>
                    {
                        let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                        let i = list_state.selected().unwrap_or(0);
                        list_state.select(Some(mouse::list_scroll(i, sessions.len(), up, 1)));
                    }
                    _ => {}
                }
            }
            if let Some(entry) = confirm_session {
                self.resume_session_entry(entry).await;
            }
            return;
        }

        // Agent picker: click highlights a row, double-click confirms (enters
        // the session), wheel moves the selection.
        if matches!(self.phase, ChatPhase::PickAgent { loading: false, .. }) {
            let mut confirm_alias: Option<String> = None;
            if let ChatPhase::PickAgent {
                agents, list_state, ..
            } = &mut self.phase
            {
                let list_area = self.pick_agent_list_area;
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(idx) = mouse::list_click_index(
                            mouse.row,
                            list_area,
                            list_state.offset(),
                            agents.len(),
                        ) {
                            list_state.select(Some(idx));
                            if self.pick_agent_double_click.click(mouse.column, mouse.row) {
                                confirm_alias = agents.get(idx).cloned();
                            }
                        }
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                        let i = list_state.selected().unwrap_or(0);
                        list_state.select(Some(mouse::list_scroll(i, agents.len(), up, 1)));
                    }
                    _ => {}
                }
            }
            if let Some(alias) = confirm_alias {
                self.focus_agent(&alias).await;
            }
            return;
        }

        if let ChatPhase::Active(state) = &self.phase
            && let MouseEventKind::Down(MouseButton::Left) = mouse.kind
            && !state.input_bar.has_file_explorer()
            && !state.input_bar.has_attachment_manager()
            && matches!(state.session_overlay, SessionOverlay::None)
            && !state.model_picker.is_open()
            && state.pending_approval().is_none()
            && state.pending_elicitation().is_none()
            && state.title_hit_target_at(mouse.column, mouse.row) == Some(TitleHitTarget::Agent)
        {
            let current_alias = state.agent_alias.clone();
            self.open_agent_picker(current_alias).await;
            return;
        }

        if let ChatPhase::Active(ref mut state) = self.phase {
            // The file explorer renders above every parent overlay.
            if state.input_bar.has_file_explorer() {
                let consumed = state.input_bar.handle_mouse(mouse);
                let cleanup_report = state.input_bar.take_cleanup_report();
                state.surface_cleanup_report(cleanup_report);
                if consumed {
                    state.clear_mouse_highlight();
                    return;
                }
            }

            if state.model_picker.is_open() {
                let rpc = self.rpc.clone();
                Self::handle_model_picker_mouse(&rpc, mouse, area, state).await;
                return;
            }

            // Session list overlay intercepts all mouse events when open.
            if let SessionOverlay::List {
                sessions,
                list_state,
            } = &mut state.session_overlay
            {
                let mut confirm_session: Option<crate::client::SessionEntry> = None;
                let col = mouse.column;
                let row = mouse.row;
                let overlay_area = session_list_overlay_area(area);

                match mouse.kind {
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        if !mouse::in_rect(col, row, overlay_area) {
                            // Click outside → close overlay.
                            state.session_overlay = SessionOverlay::None;
                        } else {
                            let count = sessions.len();
                            if let Some(idx) = mouse::list_click_index(
                                row,
                                overlay_area,
                                list_state.offset(),
                                count,
                            ) {
                                list_state.select(Some(idx));
                                if self.session_list_double_click.click(col, row) {
                                    confirm_session = sessions.get(idx).cloned();
                                }
                            }
                        }
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                        if mouse::in_rect(col, row, overlay_area) =>
                    {
                        let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                        let count = sessions.len();
                        let i = list_state.selected().unwrap_or(0);
                        list_state.select(Some(mouse::list_scroll(i, count, up, 1)));
                    }
                    _ => {}
                }
                if let Some(entry) = confirm_session {
                    self.switch_to_session_entry(entry).await;
                }
                return;
            }

            // Approval and elicitation overlays are keyboard-driven but still
            // block clicks from reaching controls rendered beneath them.
            if state.pending_approval().is_some() || state.pending_elicitation().is_some() {
                return;
            }

            if state.context_menu.is_some() {
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        let request = if state.context_menu_select_at(mouse.column, mouse.row) {
                            state.take_context_menu_request()
                        } else {
                            state.dismiss_context_menu();
                            None
                        };
                        if let Some(request) = request {
                            self.execute_context_menu_request(request).await;
                        }
                        return;
                    }
                    MouseEventKind::Down(MouseButton::Right)
                    | MouseEventKind::ScrollUp
                    | MouseEventKind::ScrollDown => {
                        state.dismiss_context_menu();
                    }
                    MouseEventKind::Drag(MouseButton::Left)
                    | MouseEventKind::Up(MouseButton::Left) => return,
                    _ => {}
                }
            }

            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind
                && !state.input_bar.has_attachment_manager()
                && state.todo_tracker.is_visible()
                && state
                    .todo_close_hit_rect
                    .is_some_and(|rect| mouse::in_rect(mouse.column, mouse.row, rect))
            {
                state.todo_tracker.hide();
                state.todo_close_hit_rect = None;
                state.mark_dirty_full();
                return;
            }

            let input_bar_consumed = state.input_bar.handle_mouse(mouse);
            let cleanup_report = state.input_bar.take_cleanup_report();
            state.surface_cleanup_report(cleanup_report);
            if input_bar_consumed {
                state.clear_mouse_highlight();
                return;
            }

            use crossterm::event::KeyModifiers as KM;
            let col = mouse.column;
            let row = mouse.row;

            if !state.model_picker.is_open()
                && let MouseEventKind::Down(MouseButton::Left) = mouse.kind
                && let Some(target) = state.title_hit_target_at(col, row)
            {
                match target {
                    TitleHitTarget::Agent => {}
                    TitleHitTarget::ModelProvider => {
                        let rpc = self.rpc.clone();
                        Self::open_provider_picker(&rpc, state).await;
                    }
                    TitleHitTarget::Model => {
                        let rpc = self.rpc.clone();
                        let tx = self.model_fetch_tx.clone();
                        Self::open_model_picker(&rpc, &tx, state).await;
                    }
                }
                return;
            }

            // Queue sidebar intercepts mouse events over its area before the
            // conversation handler, so clicks select queued items and the wheel
            // scrolls the queue rather than the transcript.
            if state.queue_sidebar_open() && state.point_in_queue_sidebar(col, row) {
                let opens_context_menu =
                    matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right))
                        || (cfg!(target_os = "macos")
                            && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                            && mouse.modifiers.contains(KM::CONTROL));
                if opens_context_menu {
                    state.open_queue_context_menu(col, row);
                    return;
                }
                match mouse.kind {
                    MouseEventKind::ScrollUp => state.queue_scroll_by(-3),
                    MouseEventKind::ScrollDown => state.queue_scroll_by(3),
                    MouseEventKind::Down(MouseButton::Left) => {
                        state.queue_click_at(col, row);
                    }
                    _ => {}
                }
                return;
            }

            // The scrollbar is shared by browse mode and character-level
            // transcript selection, so handle its drag lifecycle before those
            // interaction modes diverge.
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(track) = state.scrollbar_track_rect
                        && mouse::in_rect(col, row, track)
                    {
                        state.clear_transcript_selection();
                        state.scrollbar_drag = Some(ScrollbarDrag {
                            start_scroll: state.scroll_offset,
                            start_row: row,
                        });
                        let max = state
                            .last_total_rows
                            .saturating_sub(state.last_inner_height);
                        if track.height > 0 {
                            let rel = row.saturating_sub(track.y) as u32;
                            let new_off = (rel * max as u32 / track.height.max(1) as u32) as u16;
                            state.scroll_offset = new_off.min(max);
                            state.pinned_to_bottom = state.scroll_offset >= max;
                        }
                        return;
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(drag) = state.scrollbar_drag {
                        state.clear_transcript_selection();
                        let max = state
                            .last_total_rows
                            .saturating_sub(state.last_inner_height);
                        let track_h = state
                            .scrollbar_track_rect
                            .map(|r| r.height)
                            .unwrap_or(0)
                            .max(1);
                        let dy = row as i32 - drag.start_row as i32;
                        let scroll_delta = dy * max as i32 / track_h as i32;
                        let new_off =
                            (drag.start_scroll as i32 + scroll_delta).clamp(0, max as i32);
                        state.scroll_offset = new_off as u16;
                        state.pinned_to_bottom = state.scroll_offset >= max;
                        return;
                    }
                }
                MouseEventKind::Up(MouseButton::Left) if state.scrollbar_drag.is_some() => {
                    state.scrollbar_drag = None;
                    return;
                }
                _ => {}
            }

            let opens_context_menu = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right))
                || (cfg!(target_os = "macos")
                    && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                    && mouse.modifiers.contains(KM::CONTROL));
            if opens_context_menu {
                state.open_transcript_context_menu(col, row);
                return;
            }

            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && mouse.modifiers.is_empty()
                && (state.toggle_tool_footer_at(col, row) || state.toggle_tool_header_at(col, row))
            {
                return;
            }

            if !state.in_browse_mode() {
                match mouse.kind {
                    MouseEventKind::ScrollUp => state.scroll_up(3),
                    MouseEventKind::ScrollDown => state.scroll_down(3),
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(region) = state
                            .copy_hit_regions
                            .iter()
                            .find(|region| {
                                matches!(region.kind, CopyHitKind::Code | CopyHitKind::Transcript)
                                    && mouse::in_rect(col, row, region.rect)
                            })
                            .cloned()
                        {
                            if state.copy_text_and_clear_selection(&region.text) {
                                match region.kind {
                                    CopyHitKind::Code => {
                                        state.set_copy_feedback(CopyFeedbackTarget::Code(
                                            region.group,
                                        ));
                                    }
                                    CopyHitKind::Transcript => {
                                        state.set_overlay_copy_feedback(region.rect);
                                    }
                                    CopyHitKind::Message => {}
                                }
                            }
                        } else if (mouse.modifiers.contains(KM::SHIFT)
                            || mouse.modifiers.contains(KM::ALT))
                            && state.transcript_selection.is_some()
                        {
                            state.update_transcript_drag(col, row);
                        } else {
                            state.clear_mouse_highlight();
                            state.begin_transcript_drag(col, row);
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        state.update_transcript_drag(col, row);
                    }
                    _ => {}
                }
                return;
            }

            match mouse.kind {
                MouseEventKind::ScrollUp => state.scroll_up(3),
                MouseEventKind::ScrollDown => state.scroll_down(3),
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(region) = state
                        .copy_hit_regions
                        .iter()
                        .find(|r| mouse::in_rect(col, row, r.rect))
                        .cloned()
                    {
                        if state.copy_text_and_clear_selection(&region.text) {
                            match region.kind {
                                CopyHitKind::Code => {
                                    state.set_copy_feedback(CopyFeedbackTarget::Code(region.group));
                                }
                                CopyHitKind::Message => {
                                    state.set_overlay_copy_feedback(region.rect);
                                }
                                CopyHitKind::Transcript => {
                                    state.set_overlay_copy_feedback(region.rect);
                                }
                            }
                        }
                        return;
                    }
                    let hit = state
                        .entry_rects
                        .iter()
                        .find(|(_, r)| mouse::in_rect(col, row, *r))
                        .map(|(idx, _)| *idx);
                    let shift = mouse.modifiers.contains(KM::SHIFT);
                    let ctrl = mouse.modifiers.contains(KM::CONTROL);
                    if let Some(idx) = hit {
                        if ctrl {
                            if !state.browse_multi.remove(&idx) {
                                state.browse_multi.insert(idx);
                            }
                            state.mark_dirty_full();
                        } else if shift {
                            if state.browse_cursor.is_none() {
                                state.browse_cursor = Some(idx);
                            }
                            state.browse_anchor = state.browse_cursor;
                            state.browse_cursor = Some(idx);
                            state.mark_dirty_full();
                        } else {
                            // Plain click
                            state.browse_multi.clear();
                            state.browse_anchor = None;
                            // In browse mode: move cursor and prepare for
                            // optional drag-range selection. Copying still
                            // requires the explicit keyboard or button action.
                            state.browse_cursor = Some(idx);
                            state.mouse_down_entry = Some(idx);
                            state.mark_dirty_full();
                        }
                    } else {
                        state.clear_browse_selection();
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(start) = state.mouse_down_entry {
                        // Drag extends selection only in browse mode.
                        if state.in_browse_mode() {
                            let hit = state
                                .entry_rects
                                .iter()
                                .find(|(_, r)| mouse::in_rect(col, row, *r))
                                .map(|(idx, _)| *idx);
                            if let Some(end) = hit {
                                state.browse_anchor = Some(start);
                                state.browse_cursor = Some(end);
                                state.mark_dirty_full();
                            }
                        }
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    // Mouse-up ends a browse-mode drag gesture only. It must
                    // not copy implicitly: users expect dragging transcript
                    // text to be safe while selecting words/lines in the
                    // terminal, and whole-message copy now lives behind the
                    // explicit `[Copy]` affordance.
                    state.mouse_down_entry = None;
                }
                _ => {}
            }
        }
    }

    /// Handle a bracketed paste event.
    pub(crate) fn handle_paste(&mut self, text: &str) {
        let ChatPhase::Active(state) = &mut self.phase else {
            return;
        };
        // Bracketed paste bypasses the keyboard handlers that give modal and
        // browse surfaces first refusal. Consult the same composer-ownership
        // decision before routing it into the input bar so neither text nor a
        // path-like attachment can mutate hidden state.
        if !state.composer_owns_text_input() {
            return;
        }
        let action = state.input_bar.handle_paste(text);
        if let InputBarAction::StatusMessage(msg) = action {
            let cleanup_notice = state.input_bar.take_cleanup_report().notice();
            state.set_info_notice(append_cleanup_notice(msg, cleanup_notice));
        } else if let Some(message) = state.input_bar.take_cleanup_report().notice() {
            state.set_info_notice(message);
        }
    }

    /// Returns `(input_tokens, trim_budget, model_window)` for the context bar.
    pub(crate) fn ctx_tokens(&self) -> (Option<u64>, Option<u64>, Option<u64>) {
        match &self.phase {
            ChatPhase::Active(s) => (
                s.context_input_tokens,
                s.context_max_tokens,
                s.context_model_window,
            ),
            _ => (None, None, None),
        }
    }

    /// The agent alias this pane is currently focused on, if any. Used to
    /// resolve a per-agent theme override while this pane is active. Returns
    /// `None` in the agent-picker phase, where no agent is yet chosen.
    pub(crate) fn selected_agent(&self) -> Option<&str> {
        match &self.phase {
            ChatPhase::Active(s) => Some(s.agent_alias.as_str()),
            ChatPhase::PickCwd { agent_alias, .. } => Some(agent_alias.as_str()),
            _ => None,
        }
    }

    /// Working directory for the active conversation, if a session is running.
    pub(crate) fn current_cwd(&self) -> Option<&str> {
        match &self.phase {
            ChatPhase::Active(s) => s.cwd.as_deref(),
            _ => None,
        }
    }

    /// Active info-bar message for the app-level `InfoBar`, expiring it first if
    /// it has outlived [`crate::widgets::INFO_BAR_TTL`] so the bar auto-hides.
    pub(crate) fn info_message(&mut self) -> Option<&crate::widgets::InfoMessage> {
        if let ChatPhase::Active(s) = &mut self.phase {
            if s.info_message.as_ref().is_some_and(|m| m.is_expired()) {
                s.clear_info_notice();
            }
            return s.info_message.as_ref();
        }
        None
    }

    /// Whether the active chat session is in browse mode.
    pub(crate) fn in_browse_mode(&self) -> bool {
        match &self.phase {
            ChatPhase::Active(s) => s.in_browse_mode(),
            _ => false,
        }
    }

    /// Exit browse / selection mode if active. No-op otherwise.
    pub(crate) fn exit_browse_mode(&mut self) {
        if let ChatPhase::Active(s) = &mut self.phase {
            s.exit_browse_mode();
        }
    }

    /// Clear the input bar text (called when Ctrl+C arms the quit modal).
    pub(crate) fn clear_input(&mut self) {
        if let ChatPhase::Active(s) = &mut self.phase {
            s.input_bar.reset();
            let cleanup_report = s.input_bar.take_cleanup_report();
            s.surface_cleanup_report(cleanup_report);
            s.mark_dirty_full();
        }
    }

    pub(crate) fn wants_quit_chord(&self) -> bool {
        match &self.phase {
            ChatPhase::Active(s) => {
                s.turn_in_flight && !matches!(s.turn_status, TurnStatus::Cancelling)
            }
            _ => false,
        }
    }

    pub(crate) fn take_help_request(&mut self) -> bool {
        std::mem::take(&mut self.help_requested)
    }

    pub(crate) fn wants_text_input(&self) -> bool {
        match &self.phase {
            // CWD picker always captures text input.
            ChatPhase::PickCwd { .. } => true,
            ChatPhase::PickSession { .. } => false,
            ChatPhase::Active(s) => {
                // The model picker is modal: claim text-input so global keys
                // (`?`, reload) are suppressed; its own handler swallows keys.
                if s.model_picker.is_open() {
                    return true;
                }
                if s.pending_elicitation().is_some() {
                    return true;
                }
                if !matches!(s.session_overlay, SessionOverlay::None) {
                    return false;
                }
                if s.pending_approval().is_some() {
                    return false;
                }
                // Browse mode: single-char bindings active.
                if s.in_browse_mode() {
                    return false;
                }
                // Command mode when input is empty; text mode when typing.
                s.input_bar.wants_text_input()
            }
            _ => false,
        }
    }

    pub(crate) fn claims_pane_navigation(&self, key: &KeyEvent) -> bool {
        match &self.phase {
            ChatPhase::Active(state) => {
                !state.model_picker.is_open()
                    && state.pending_elicitation().is_none()
                    && state.pending_approval().is_none()
                    && matches!(state.session_overlay, SessionOverlay::None)
                    && !state.in_browse_mode()
                    && state.input_bar.claims_pane_navigation(key)
            }
            _ => false,
        }
    }
}

impl crate::widgets::HelpContext for Chat {
    fn help_context(&self) -> crate::widgets::HelpNode {
        use crate::keymap::{ChatTabAction, RebindableActions};
        use crate::widgets::{HelpEntry as E, HelpNode};
        match &self.phase {
            ChatPhase::PickAgent { loading, .. } => {
                use crate::keymap::{
                    ChatTabAction as C, GlobalAction, ModalAction, action_key_labels,
                };
                if *loading {
                    HelpNode::entries(vec![E::key("", crate::i18n::t("zc-chat-loading-agents"))])
                } else {
                    let nav = action_key_labels(C::BrowseUp)
                        .into_iter()
                        .chain(action_key_labels(C::BrowseDown))
                        .chain(action_key_labels(C::BrowseUpVim))
                        .chain(action_key_labels(C::BrowseDownVim));
                    let mut entries = vec![
                        E::new(nav, crate::i18n::t("zc-chat-help-navigate")),
                        E::new(
                            action_key_labels(ModalAction::Confirm),
                            crate::i18n::t("zc-chat-help-select-agent"),
                        ),
                        E::new(
                            action_key_labels(GlobalAction::Quit),
                            crate::i18n::t("zc-chat-help-quit"),
                        ),
                    ];
                    // On the ACP (Code) pane the agent picker is the
                    // no-saved-session entry point, so include the
                    // history-vs-persistent-memory disclosure here too. Kept out
                    // of the Chat pane's picker.
                    if self.pane_kind == PaneKind::Acp {
                        entries.push(E::desc(crate::i18n::t("zc-chat-help-acp-memory")));
                    }
                    HelpNode::entries(entries)
                }
            }
            ChatPhase::PickCwd { explorer, .. } => explorer.help_context(),
            ChatPhase::PickSession { .. } => {
                use crate::keymap::{ChatTabAction as C, ModalAction as M, action_key_labels};
                let nav = action_key_labels(M::Up)
                    .into_iter()
                    .chain(action_key_labels(M::Down));
                HelpNode::entries(vec![
                    E::new(nav, crate::i18n::t("zc-chat-help-navigate")),
                    E::new(
                        action_key_labels(M::Confirm),
                        crate::i18n::t("zc-chat-help-switch-session"),
                    ),
                    E::new(
                        action_key_labels(M::Cancel)
                            .into_iter()
                            .chain(action_key_labels(C::NewSession)),
                        crate::i18n::t("zc-chat-help-new-session"),
                    ),
                    E::desc(crate::i18n::t("zc-chat-help-acp-memory")),
                ])
            }
            ChatPhase::Error(_) => {
                use crate::keymap::{ChatTabAction as C, GlobalAction, action_key_labels};
                let keys = action_key_labels(C::ErrorDismiss)
                    .into_iter()
                    .chain(action_key_labels(GlobalAction::Quit));
                HelpNode::entries(vec![E::new(keys, crate::i18n::t("zc-chat-help-quit"))])
            }
            ChatPhase::Active(state) => {
                match &state.session_overlay {
                    SessionOverlay::List { .. } => {
                        use crate::keymap::{ModalAction as M, action_key_labels};
                        let nav = action_key_labels(M::Up)
                            .into_iter()
                            .chain(action_key_labels(M::Down));
                        return HelpNode::entries(vec![
                            E::new(nav, crate::i18n::t("zc-chat-help-navigate")),
                            E::new(
                                action_key_labels(M::Confirm),
                                crate::i18n::t("zc-chat-help-switch-session"),
                            ),
                            E::new(
                                action_key_labels(M::Cancel),
                                crate::i18n::t("zc-chat-help-close"),
                            ),
                        ]);
                    }
                    SessionOverlay::None => {}
                }
                if state.pending_elicitation().is_some() {
                    // The elicitation modal is keyboard-driven; source its
                    // hints from the ModalAction registry so they track any
                    // rebind. Multi-select adds the toggle line.
                    use crate::keymap::{ModalAction as M, action_key_labels};
                    let multi = state
                        .pending_elicitation()
                        .map(|e| e.multi)
                        .unwrap_or(false);
                    let mut entries = vec![E::new(
                        action_key_labels(M::Up)
                            .into_iter()
                            .chain(action_key_labels(M::Down)),
                        crate::i18n::t("zc-chat-help-move-up"),
                    )];
                    if multi {
                        entries.push(E::new(
                            action_key_labels(M::Toggle),
                            crate::i18n::t("zc-elicit-help-toggle"),
                        ));
                    }
                    entries.push(E::new(
                        action_key_labels(M::Confirm),
                        crate::i18n::t("zc-elicit-help-confirm"),
                    ));
                    entries.push(E::new(
                        action_key_labels(M::Cancel),
                        crate::i18n::t("zc-elicit-help-cancel"),
                    ));
                    return HelpNode::entries(entries);
                }
                if state.pending_approval().is_some() {
                    use crate::keymap::{ChatTabAction as C, action_key_labels};
                    return HelpNode::entries(vec![
                        E::new(
                            action_key_labels(C::ApprovalApprove),
                            crate::i18n::t("zc-chat-help-approve"),
                        ),
                        E::new(
                            action_key_labels(C::ApprovalApproveAll),
                            crate::i18n::t("zc-chat-help-always-approve"),
                        ),
                        E::new(
                            action_key_labels(C::CancelTurn),
                            crate::i18n::t("zc-chat-help-deny"),
                        ),
                        E::new(
                            action_key_labels(C::CancelTurn),
                            crate::i18n::t("zc-chat-help-cancel-turn"),
                        ),
                    ]);
                }
                if state.in_browse_mode() {
                    use crate::keymap::{ChatTabAction as C, action_key_labels};
                    let mut return_keys = action_key_labels(C::BrowseExit);
                    return_keys.extend(action_key_labels(C::BrowseExitSelection));
                    return HelpNode::entries(vec![
                        E::new(
                            action_key_labels(C::BrowseUp)
                                .into_iter()
                                .chain(action_key_labels(C::BrowseUpVim)),
                            crate::i18n::t("zc-chat-help-move-up"),
                        ),
                        E::new(
                            action_key_labels(C::BrowseDown)
                                .into_iter()
                                .chain(action_key_labels(C::BrowseDownVim)),
                            crate::i18n::t("zc-chat-help-move-down"),
                        ),
                        E::new(
                            action_key_labels(C::BrowseSelectExtend)
                                .into_iter()
                                .chain(action_key_labels(C::BrowseSelectExtendDown)),
                            crate::i18n::t("zc-chat-help-extend-selection"),
                        ),
                        E::new(
                            action_key_labels(C::CopySelection)
                                .into_iter()
                                .chain(action_key_labels(C::CopyAllVisible)),
                            crate::i18n::t("zc-chat-help-yank-selection"),
                        ),
                        E::new(return_keys, crate::i18n::t("zc-chat-help-return-to-input")),
                    ]);
                }
                if state.turn_in_flight {
                    use crate::keymap::{ChatTabAction as C, action_key_labels};
                    let mut cancel_keys = action_key_labels(C::CancelTurn);
                    cancel_keys.extend(action_key_labels(C::BrowseExitSelection));
                    let mut entries = vec![
                        E::new(cancel_keys, crate::i18n::t("zc-chat-help-cancel-turn")),
                        E::new(
                            action_key_labels(crate::keymap::InputBarAction::Submit),
                            crate::i18n::t("zc-queue-help-enqueue"),
                        ),
                        E::new(
                            action_key_labels(crate::keymap::InputBarAction::Inject),
                            crate::i18n::t("zc-queue-help-inject"),
                        ),
                    ];
                    // Queue-management keys are only live while the sidebar is
                    // open — surface them here too so a mid-turn open queue is
                    // not left without its own controls.
                    if state.queue_sidebar_open() {
                        entries.extend(queue_sidebar_help_entries());
                    }
                    // The input box stays editable mid-turn for queuing, so its
                    // bindings belong in help too.
                    return HelpNode::entries(entries).with_child(state.input_bar.help_context());
                }
                // Idle: compose pane-level bindings + input bar as child.
                let mut pane_entries = vec![
                    // Browse-mode bindings rendered from the registry so
                    // rebinds always stay in sync — see also the browse-mode
                    // dispatch code in `handle_key`.
                    E::new(
                        ChatTabAction::BrowseEnter
                            .resolved()
                            .iter()
                            .map(|c| c.display().to_string()),
                        crate::i18n::t("zc-chat-help-browse-mode"),
                    ),
                    E::key(
                        "Shift+↑/↓",
                        crate::i18n::t("zc-chat-help-scroll-conversation"),
                    ),
                    E::key("t", crate::i18n::t("zc-chat-help-toggle-thoughts")),
                    E::spacer(),
                    E::key(
                        chord_label(ChatTabAction::NewSession),
                        crate::i18n::t("zc-chat-help-new-session"),
                    ),
                    E::key(
                        chord_label(ChatTabAction::SwitchSession),
                        crate::i18n::t("zc-chat-help-switch-session"),
                    ),
                    E::spacer(),
                    E::key(
                        chord_label(ChatTabAction::PauseResumeQueue),
                        crate::i18n::t("zc-queue-help-resume"),
                    ),
                ];
                pane_entries.extend(queue_sidebar_help_entries());
                let pane = HelpNode::entries(pane_entries);
                pane.with_child(state.input_bar.help_context())
            }
        }
    }
}

// ── Agent picker rendering ───────────────────────────────────────

/// Build the agent-picker nav hint from the live keymap (browse up/down + the
/// modal confirm chord), never hardcoded literals.
fn picker_nav_keys() -> String {
    use crate::keymap::{ChatTabAction, Chord, ModalAction, RebindableActions};
    let mut parts: Vec<String> = Vec::new();
    let mut push = |c: &Chord| {
        let d = c.display();
        if !parts.contains(&d) {
            parts.push(d);
        }
    };
    for c in ChatTabAction::BrowseUp.resolved() {
        push(&c);
    }
    for c in ChatTabAction::BrowseDown.resolved() {
        push(&c);
    }
    for c in ModalAction::Confirm.resolved() {
        push(&c);
    }
    parts.join("/")
}

fn draw_agent_picker(
    frame: &mut Frame,
    area: Rect,
    agents: &[String],
    list_state: &mut ListState,
    loading: bool,
    tab_title: &str,
    acp_memory_note: Option<String>,
) -> Rect {
    let block = Block::default()
        .title(Span::styled(format!(" {tab_title} "), theme::title_style()))
        .borders(Borders::ALL)
        .border_style(theme::dim_style());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if loading {
        let p = Paragraph::new(crate::i18n::t("zc-chat-loading-agents-msg"))
            .alignment(Alignment::Center)
            .style(theme::dim_style());
        let vert = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .split(inner);
        frame.render_widget(p, vert[1]);
        return Rect::default();
    }

    let note_rows = acp_memory_note
        .as_deref()
        .map(|note| note_reserved_rows(note, inner.width))
        .unwrap_or(1);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(note_rows),
        ])
        .split(inner);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            format!("{} ", crate::i18n::t("zc-chat-picker-header")),
            theme::body_style(),
        ),
        Span::styled(
            crate::i18n::t_args(
                "zc-chat-picker-header-hint",
                &[("keys", &picker_nav_keys())],
            ),
            theme::dim_style(),
        ),
    ]));
    frame.render_widget(header, chunks[0]);

    let items: Vec<ListItem> = agents
        .iter()
        .map(|a| ListItem::new(Span::styled(a.as_str(), theme::body_style())))
        .collect();
    let list = List::new(items).highlight_style(theme::list_highlight_style());
    frame.render_stateful_widget(list, chunks[1], list_state);

    // On the ACP no-saved-session path (a fresh Code start with nothing to
    // resume) the resume picker never appears, so surface the same
    // history-vs-persistent-memory disclosure in the agent picker's footer
    // slot. Only rendered for the Code (ACP) pane — Chat passes `None` — so the
    // Code-specific copy stays out of the Chat picker.
    if let Some(note) = acp_memory_note {
        let note_line =
            Paragraph::new(Span::styled(note, theme::dim_style())).wrap(Wrap { trim: true });
        frame.render_widget(note_line, chunks[2]);
    }

    // The list rect is unbordered, but `mouse::list_click_index` assumes a
    // 1-cell top border. Hand back a rect shifted up one row (and one taller) so
    // the helper's border compensation lands on the true first item.
    Rect::new(
        chunks[1].x,
        chunks[1].y.saturating_sub(1),
        chunks[1].width,
        chunks[1].height + 1,
    )
}

// ── Error rendering ──────────────────────────────────────────────

fn draw_error(frame: &mut Frame, area: Rect, msg: &str, tab_title: &str) {
    let block = Block::default()
        .title(Span::styled(format!(" {tab_title} "), theme::title_style()))
        .borders(Borders::ALL)
        .border_style(theme::dim_style());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(inner);

    let p = Paragraph::new(Line::from(Span::styled(msg, theme::error_style())))
        .alignment(Alignment::Center);
    frame.render_widget(p, chunks[1]);
}

// ── Active chat rendering ────────────────────────────────────────

fn carve_todo_area(tracker: &crate::todo_tracker::TodoTracker, area: Rect) -> (Rect, Option<Rect>) {
    if !tracker.wants_space() {
        return (area, None);
    }
    match tracker.location() {
        crate::todo_tracker::TodoLocation::Right => {
            let w = tracker.width().min(area.width / 2);
            let body = Rect::new(area.x, area.y, area.width.saturating_sub(w), area.height);
            let panel = Rect::new(area.x + body.width, area.y, w, area.height);
            (body, Some(panel))
        }
        crate::todo_tracker::TodoLocation::Left => {
            let w = tracker.width().min(area.width / 2);
            let panel = Rect::new(area.x, area.y, w, area.height);
            let body = Rect::new(
                area.x + w,
                area.y,
                area.width.saturating_sub(w),
                area.height,
            );
            (body, Some(panel))
        }
        crate::todo_tracker::TodoLocation::Bottom => {
            // Grow up to the configured cap (+2 rows for the bordered
            // block), but never exceed half the pane height.
            let want = (tracker.total() as u16 + 2).min(tracker.max_height());
            let h = want.min(area.height / 2);
            let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(h));
            let panel = Rect::new(area.x, area.y + body.height, area.width, h);
            (body, Some(panel))
        }
    }
}

fn render(f: &mut Frame, state: &mut ChatState, area: Rect, pane_kind: PaneKind) {
    // Carve the TodoWrite tracker's area first (outermost split), so the
    // rest of the pane (queue sidebar, transcript, input) lays out in the
    // remaining body. When the tracker wants no space, `body == area` and
    // the existing layout is untouched.
    state.todo_close_hit_rect = None;
    let (area, todo_area) = carve_todo_area(&state.todo_tracker, area);
    if let Some(panel) = todo_area {
        state.todo_close_hit_rect = state.todo_tracker.render(f, panel);
    }

    let area = if state.queue_sidebar_open() {
        let sidebar_w = state.queue_sidebar_width(area.width);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(sidebar_w)])
            .split(area);
        render_queue_sidebar(f, state, cols[1]);
        cols[0]
    } else {
        area
    };

    let show_cursor = state.pending_approval().is_none() && state.pending_elicitation().is_none();
    let turn_status = state.turn_status.clone();
    let turn_started_at = state.turn_started_at;

    let _live_input_tokens: Option<u64> = state.context_input_tokens;

    // Transient info-bar messages (queue/attach notices, model-switch notes)
    // render at the app level via InfoBar from `state.info_message`. The paused
    // queue shows as ghost text in the empty input box below, so the chat pane
    // hands its full area to the input bar here.
    let input_area = area;

    let queue_paused_hint = if state.queue_paused() && state.queue_len() > 0 {
        Some(crate::i18n::t_args(
            "zc-queue-paused-ghost",
            &[("key", &resume_queue_chord_label())],
        ))
    } else {
        None
    };

    let conv_area = state.input_bar.render(
        f,
        input_area,
        state.turn_in_flight,
        show_cursor,
        &turn_status,
        turn_started_at,
        queue_paused_hint.as_deref(),
    );

    // Optional CWD line just above the input bar (bottom of conv_area).
    // Renders `<cwd> - (branch) (hash)`, all left-aligned; the branch and hash
    // segments are appended only when the daemon's git poll has resolved them.
    let actual_conv = if pane_kind == PaneKind::Acp
        && let Some(ref cwd) = state.cwd
    {
        if conv_area.height > 1 {
            let cwd_row = Rect::new(
                conv_area.x,
                conv_area.y + conv_area.height - 1,
                conv_area.width,
                1,
            );
            let mut line = format!(" {cwd}");
            if state.git_branch.is_some() || state.git_hash.is_some() {
                line.push_str(" -");
                if let Some(ref branch) = state.git_branch {
                    line.push_str(&format!(" ({branch})"));
                }
                if let Some(ref hash) = state.git_hash {
                    line.push_str(&format!(" ({hash})"));
                }
            }
            line.push(' ');
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(line, theme::dim_style())))
                    .alignment(Alignment::Left),
                cwd_row,
            );
            Rect::new(
                conv_area.x,
                conv_area.y,
                conv_area.width,
                conv_area.height - 1,
            )
        } else {
            conv_area
        }
    } else {
        conv_area
    };

    render_conversation(f, state, actual_conv);
    state.input_bar.render_autocomplete_popup(f);
    state.input_bar.render_attachment_manager(f, area);

    if state.pending_approval().is_some() {
        render_approval_overlay(f, state, area);
    }

    if state.pending_elicitation().is_some() {
        render_elicitation_overlay(f, state, area);
    }

    match &mut state.session_overlay {
        SessionOverlay::List {
            sessions,
            list_state,
        } => {
            render_session_list_overlay(
                f,
                area,
                sessions,
                list_state,
                crate::i18n::t("zc-chat-session-list-switch-title"),
                None,
            );
        }
        SessionOverlay::None => {}
    }

    // Model / model_provider picker overlay (drawn on top of content).
    match &state.model_picker {
        ModelPickerOverlay::Loading => {
            // The "Loading models…" status shows in the info bar; the overlay
            // exists only to block input until the catalog arrives. A modal box
            // with no rows would render nothing, so draw a titled placeholder.
            let title = crate::i18n::t("zc-model-catalog-loading");
            let placeholder = [String::new()];
            crate::widgets::PickerModal::new(&title, &placeholder, usize::MAX).render(f, area);
        }
        ModelPickerOverlay::Model(picker) => {
            crate::widgets::PickerModal::new(
                &crate::i18n::t("zc-model-picker-title"),
                &picker.items,
                picker.cursor,
            )
            .render(f, area);
        }
        ModelPickerOverlay::ConfiguredProviderStage(picker) => {
            crate::widgets::PickerModal::new(
                &crate::i18n::t("zc-model-provider-picker-title"),
                &picker.items,
                picker.cursor,
            )
            .render(f, area);
        }
        ModelPickerOverlay::None => {}
    }

    state.input_bar.render_explorer_overlay(f, area);
}

fn model_picker_overlay_area(model_picker: &ModelPickerOverlay, area: Rect) -> Option<Rect> {
    match model_picker {
        ModelPickerOverlay::Loading => {
            let title = crate::i18n::t("zc-model-catalog-loading");
            let placeholder = [String::new()];
            crate::widgets::PickerModal::area_for(&title, &placeholder, area)
        }
        ModelPickerOverlay::Model(picker) => crate::widgets::PickerModal::area_for(
            &crate::i18n::t("zc-model-picker-title"),
            &picker.items,
            area,
        ),
        ModelPickerOverlay::ConfiguredProviderStage(picker) => {
            crate::widgets::PickerModal::area_for(
                &crate::i18n::t("zc-model-provider-picker-title"),
                &picker.items,
                area,
            )
        }
        ModelPickerOverlay::None => None,
    }
}

fn resume_queue_chord_label() -> String {
    crate::keymap::ChatTabAction::PauseResumeQueue
        .default_chords()
        .first()
        .map(|c| c.display())
        .unwrap_or_else(|| "Alt+P".to_string())
}

/// Queue-management help entries shown whenever the queue sidebar is open —
/// both mid-turn and idle. Keeping this in one place stops the two call sites
/// from drifting apart. Every key label is derived from the keymap registry,
/// never hardcoded, so rebinds stay reflected in help.
fn queue_sidebar_help_entries() -> Vec<crate::widgets::HelpEntry> {
    use crate::keymap::ChatTabAction as A;
    use crate::widgets::HelpEntry as E;
    vec![
        E::key(
            chord_label_pair(A::QueueNavUp, A::QueueNavDown),
            crate::i18n::t("zc-queue-help-nav"),
        ),
        E::key(
            chord_label(A::QueueSendNow),
            crate::i18n::t("zc-queue-help-inject"),
        ),
        E::key(
            chord_label(A::QueueCopy),
            crate::i18n::t("zc-chat-context-menu-copy"),
        ),
        E::key(
            chord_label(A::QueueDelete),
            crate::i18n::t("zc-queue-help-delete"),
        ),
        E::key("/clear-queue", crate::i18n::t("zc-queue-help-clear")),
        E::key(
            chord_label(A::QueueEdit),
            crate::i18n::t("zc-queue-help-edit"),
        ),
        E::key(
            chord_label_pair(A::QueueWiden, A::QueueNarrow),
            crate::i18n::t("zc-queue-help-resize"),
        ),
    ]
}

/// Render an action's primary bound chord as a `&'static str` for help entries.
/// `HelpEntry::key` requires `'static`, and chord display is computed at
/// runtime, so the label is leaked — help is built once per popup open.
fn chord_label(action: crate::keymap::ChatTabAction) -> &'static str {
    let label = action
        .default_chords()
        .first()
        .map(|c| c.display())
        .unwrap_or_default();
    Box::leak(label.into_boxed_str())
}

/// Like `chord_label` but joins two actions' chords as `A/B` (e.g. the up/down
/// or widen/narrow pairs that share one help row).
fn chord_label_pair(
    a: crate::keymap::ChatTabAction,
    b: crate::keymap::ChatTabAction,
) -> &'static str {
    let render = |action: crate::keymap::ChatTabAction| {
        action
            .default_chords()
            .first()
            .map(|c| c.display())
            .unwrap_or_default()
    };
    Box::leak(format!("{}/{}", render(a), render(b)).into_boxed_str())
}

fn render_queue_sidebar(f: &mut Frame, state: &mut ChatState, area: Rect) {
    let title = crate::i18n::t_args(
        "zc-queue-title",
        &[("count", &state.queue_len().to_string())],
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::dim_style())
        .title(Span::styled(format!(" {title} "), theme::title_style()))
        .style(theme::fill_style());
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    state.queue_item_rects.clear();
    state.queue_sidebar_rect = None;
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    state.queue_sidebar_rect = Some(inner);

    // Build the row list, recording which rendered row index owns which queued
    // message id so a click can be mapped back to an item after scrolling.
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut row_owner: Vec<Option<u64>> = Vec::new();

    if state.message_queue.is_empty() {
        rows.push(Line::from(Span::styled(
            crate::i18n::t("zc-queue-empty-list"),
            theme::dim_style(),
        )));
        row_owner.push(None);
    } else {
        for (idx, msg) in state.message_queue.iter().enumerate() {
            let selected = state.queue_sel == Some(msg.id);
            let marker = if selected { "▶ " } else { "  " };
            let head_style = if selected {
                theme::title_style()
            } else {
                theme::body_style()
            };
            let preview = first_line_preview(&msg.text, inner.width.saturating_sub(4) as usize);
            let tag = if msg.status == QueueItemStatus::Injected {
                format!(" {}", crate::i18n::t("zc-queue-item-injected"))
            } else {
                String::new()
            };
            rows.push(Line::from(vec![
                Span::styled(format!("{marker}{}.", idx + 1), head_style),
                Span::styled(format!(" {preview}"), head_style),
                Span::styled(tag, theme::dim_style()),
            ]));
            row_owner.push(Some(msg.id));
            for att in &msg.attachments {
                rows.push(Line::from(Span::styled(
                    format!("    📎 {}", att.filename),
                    theme::dim_style(),
                )));
                row_owner.push(Some(msg.id));
            }
        }
    }

    // Clamp the scroll offset to the content that overflows the inner height,
    // then record on-screen rects for the visible item rows.
    let total = rows.len() as u16;
    let max_scroll = total.saturating_sub(inner.height);
    if state.queue_scroll > max_scroll {
        state.queue_scroll = max_scroll;
    }
    let scroll = state.queue_scroll;
    for (i, owner) in row_owner.iter().enumerate() {
        let row_i = i as u16;
        if row_i < scroll {
            continue;
        }
        let screen_y = inner.y + (row_i - scroll);
        if screen_y >= inner.y + inner.height {
            break;
        }
        if let Some(id) = owner {
            state
                .queue_item_rects
                .push((*id, Rect::new(inner.x, screen_y, inner.width, 1)));
        }
    }

    // No soft wrap: a queued message renders on a single line that the pane
    // width hard-truncates. Wrapping made long messages spill onto extra rows
    // and pushed the queue out of alignment; the preview is already clipped to
    // the inner width above, and ratatui truncates anything still too wide.
    let para = Paragraph::new(rows)
        .style(theme::fill_style())
        .scroll((scroll, 0));
    f.render_widget(para, inner);
}

fn first_line_preview(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("");
    let truncated = truncate_utf8(line, max.max(1));
    if truncated.len() < line.len() {
        format!("{truncated}…")
    } else {
        truncated.to_string()
    }
}

/// Extract the file extension from the `"path"` field of a tool's input JSON.
fn file_ext(input: &serde_json::Value) -> Option<&str> {
    let path = input.get("path")?.as_str()?;
    std::path::Path::new(path).extension()?.to_str()
}

/// Return a prefix of `s` no longer than `max_bytes`, guaranteed to end on a
/// valid UTF-8 char boundary. Never panics on multi-byte characters.
fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn terminal_safe_tool_text_limited(
    text: &str,
    max_bytes: usize,
    max_lines: usize,
) -> (String, bool) {
    let mut safe = String::with_capacity(text.len().min(max_bytes));
    let mut lines = 1usize;
    for ch in text.chars() {
        let piece = match ch {
            '\n' if lines >= max_lines => return (safe, true),
            '\n' => {
                lines += 1;
                "\n".to_string()
            }
            ch if ch.is_control() => ch.escape_default().to_string(),
            ch => ch.to_string(),
        };
        if safe.len().saturating_add(piece.len()) > max_bytes {
            return (safe, true);
        }
        safe.push_str(&piece);
    }
    (safe, false)
}

const FILE_TOOL_PREVIEW_LINES: usize = 6;
const TOOL_EXPANDED_MAX_BYTES: usize = 8 * 1024;
const TOOL_EXPANDED_MAX_LINES: usize = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolDisclosure {
    Collapsed,
    Preview,
    Full,
}

impl ToolDisclosure {
    fn is_open(self) -> bool {
        !matches!(self, Self::Collapsed)
    }
}

fn valid_specialized_file_input(name: &str, input: &serde_json::Value) -> bool {
    if input.get("path").and_then(|value| value.as_str()).is_none() {
        return false;
    }
    match name {
        "file_edit" => {
            input
                .get("old_string")
                .and_then(|value| value.as_str())
                .is_some()
                && input
                    .get("new_string")
                    .and_then(|value| value.as_str())
                    .is_some()
        }
        "file_write" => {
            let has_content = input
                .get("content")
                .and_then(|value| value.as_str())
                .is_some();
            let valid_encoding = match input.get("encoding") {
                None => true,
                Some(serde_json::Value::String(encoding)) => {
                    encoding == "utf8" || encoding == "base64"
                }
                Some(_) => false,
            };
            has_content && valid_encoding
        }
        _ => false,
    }
}

fn default_tool_disclosure(name: &str, input_json: &str) -> ToolDisclosure {
    let specialized = matches!(name, "file_edit" | "file_write")
        && serde_json::from_str::<serde_json::Value>(input_json)
            .is_ok_and(|input| valid_specialized_file_input(name, &input));
    if specialized {
        ToolDisclosure::Preview
    } else {
        ToolDisclosure::Collapsed
    }
}

fn semantic_tool_metadata(input: &serde_json::Value, bulk_fields: &[&str]) -> String {
    let Some(object) = input.as_object() else {
        return input.to_string();
    };
    let metadata = object
        .iter()
        .filter(|(key, _)| !bulk_fields.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    serde_json::Value::Object(metadata).to_string()
}

fn bounded_tool_output(raw_output: String) -> String {
    const MAX_OUTPUT: usize = 16 * 1024;
    const TRUNCATION_MARKER: &str = "…[truncated]";
    if raw_output.len() > MAX_OUTPUT {
        let content_limit = MAX_OUTPUT.saturating_sub(TRUNCATION_MARKER.len());
        format!(
            "{}{}",
            truncate_utf8(&raw_output, content_limit),
            TRUNCATION_MARKER
        )
    } else {
        raw_output
    }
}

fn render_tool_entry(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    input_json: &str,
    result: Option<&str>,
    is_selected: bool,
    disclosure: ToolDisclosure,
) -> Option<usize> {
    let sel_mod = if is_selected {
        Modifier::REVERSED
    } else {
        Modifier::empty()
    };
    let marker = if disclosure.is_open() { "▼" } else { "▶" };
    lines.push(Line::from(vec![Span::styled(
        format!("{marker} [tool: {name}] "),
        theme::tool_label_style().add_modifier(sel_mod),
    )]));

    let preview = |text: &str, max_bytes: usize| {
        let (compact, limited) = terminal_safe_tool_text_limited(text, max_bytes, 1);
        if limited {
            format!("{compact}…")
        } else {
            compact
        }
    };
    let push_text = |lines: &mut Vec<Line<'static>>, label: &str, text: &str| {
        for (line_idx, text_line) in text.split('\n').enumerate() {
            let prefix = if line_idx == 0 {
                format!("  {label}: ")
            } else {
                "    ".to_string()
            };
            lines.push(Line::from(Span::styled(
                format!("{prefix}{text_line}"),
                theme::dim_style().add_modifier(sel_mod),
            )));
        }
    };

    let body_start = lines.len();
    let mut footer = None;
    let mut display_limited = false;
    let render_generic_input = |lines: &mut Vec<Line<'static>>| {
        let (input, limited) = if matches!(disclosure, ToolDisclosure::Full) {
            terminal_safe_tool_text_limited(
                input_json,
                TOOL_EXPANDED_MAX_BYTES,
                TOOL_EXPANDED_MAX_LINES,
            )
        } else {
            (preview(input_json, 120), false)
        };
        push_text(lines, "input", &input);
        limited
    };
    match name {
        "file_edit" => {
            if disclosure.is_open() {
                let parsed = serde_json::from_str::<serde_json::Value>(input_json).ok();
                let valid = parsed
                    .as_ref()
                    .filter(|input| valid_specialized_file_input(name, input))
                    .and_then(|input| {
                        Some((
                            input.get("old_string")?.as_str()?,
                            input.get("new_string")?.as_str()?,
                        ))
                    });
                if let (Some(input), Some((old, new))) = (parsed.as_ref(), valid) {
                    let (metadata, metadata_limited) = terminal_safe_tool_text_limited(
                        &semantic_tool_metadata(input, &["old_string", "new_string"]),
                        TOOL_EXPANDED_MAX_BYTES,
                        TOOL_EXPANDED_MAX_LINES,
                    );
                    display_limited |= metadata_limited;
                    push_text(lines, "input", &metadata);
                    let (old, old_limited) = terminal_safe_tool_text_limited(
                        old,
                        TOOL_EXPANDED_MAX_BYTES,
                        TOOL_EXPANDED_MAX_LINES,
                    );
                    let (new, new_limited) = terminal_safe_tool_text_limited(
                        new,
                        TOOL_EXPANDED_MAX_BYTES,
                        TOOL_EXPANDED_MAX_LINES,
                    );
                    display_limited |= old_limited || new_limited;
                    let rendered = diff::diff_lines_limited(
                        &old,
                        &new,
                        file_ext(input),
                        None,
                        matches!(disclosure, ToolDisclosure::Preview)
                            .then_some(FILE_TOOL_PREVIEW_LINES),
                    );
                    footer = (rendered.total > FILE_TOOL_PREVIEW_LINES).then_some(rendered.omitted);
                    lines.extend(rendered.lines);
                } else {
                    display_limited |= render_generic_input(lines);
                }
            }
        }
        "file_write" => {
            if disclosure.is_open() {
                let parsed = serde_json::from_str::<serde_json::Value>(input_json).ok();
                let content = parsed
                    .as_ref()
                    .filter(|input| valid_specialized_file_input(name, input))
                    .and_then(|input| input.get("content"))
                    .and_then(|value| value.as_str());
                if let (Some(input), Some(content)) = (parsed.as_ref(), content) {
                    let (metadata, metadata_limited) = terminal_safe_tool_text_limited(
                        &semantic_tool_metadata(input, &["content"]),
                        TOOL_EXPANDED_MAX_BYTES,
                        TOOL_EXPANDED_MAX_LINES,
                    );
                    display_limited |= metadata_limited;
                    push_text(lines, "input", &metadata);
                    let encoding = input
                        .get("encoding")
                        .and_then(|value| value.as_str())
                        .unwrap_or("utf8");
                    if encoding == "base64" {
                        push_text(
                            lines,
                            "content",
                            &crate::i18n::t_args(
                                "zc-chat-tool-encoded-size",
                                &[("count", &content.len().to_string())],
                            ),
                        );
                    } else {
                        let (content, limited) = terminal_safe_tool_text_limited(
                            content,
                            TOOL_EXPANDED_MAX_BYTES,
                            TOOL_EXPANDED_MAX_LINES,
                        );
                        display_limited |= limited;
                        let rendered = diff::write_lines_limited(
                            &content,
                            file_ext(input),
                            matches!(disclosure, ToolDisclosure::Preview)
                                .then_some(FILE_TOOL_PREVIEW_LINES),
                        );
                        footer =
                            (rendered.total > FILE_TOOL_PREVIEW_LINES).then_some(rendered.omitted);
                        lines.extend(rendered.lines);
                    }
                } else {
                    display_limited |= render_generic_input(lines);
                }
            }
        }
        _ => display_limited |= render_generic_input(lines),
    }

    let mut footer_line = None;
    if let Some(omitted) = footer {
        let text = if matches!(disclosure, ToolDisclosure::Full) {
            crate::i18n::t("zc-chat-tool-show-less")
        } else {
            crate::i18n::t_args("zc-chat-tool-show-all", &[("count", &omitted.to_string())])
        };
        footer_line = Some(lines.len());
        lines.push(Line::from(Span::styled(
            format!("  {text}"),
            theme::tool_label_style().add_modifier(sel_mod),
        )));
    }

    if let Some(res) = result {
        let (result, limited) = if matches!(disclosure, ToolDisclosure::Full) {
            terminal_safe_tool_text_limited(res, TOOL_EXPANDED_MAX_BYTES, TOOL_EXPANDED_MAX_LINES)
        } else {
            (preview(res, 200), false)
        };
        push_text(lines, "result", &result);
        display_limited |= limited;
    }

    if display_limited {
        lines.push(Line::from(Span::styled(
            format!("  {}", crate::i18n::t("zc-chat-tool-display-limited")),
            theme::dim_style().add_modifier(sel_mod),
        )));
    }

    // Apply REVERSED to body lines from diff_lines/write_lines too.
    if is_selected {
        for line in &mut lines[body_start..] {
            let spans = std::mem::take(&mut line.spans);
            line.spans = spans
                .into_iter()
                .map(|s| s.patch_style(Style::default().add_modifier(Modifier::REVERSED)))
                .collect();
        }
    }
    footer_line
}

/// Render a single committed entry into `lines`.
/// Extracted so both the incremental-append and full-rebuild paths in
/// `rebuild_lines` share identical rendering logic.
fn render_entry_into(
    entry: &ChatEntry,
    is_selected: bool,
    show_thoughts: bool,
    tool_disclosure: ToolDisclosure,
    width: u16,
    lines: &mut Vec<Line<'static>>,
) -> Option<usize> {
    let sel_mod = if is_selected {
        Modifier::REVERSED
    } else {
        Modifier::empty()
    };
    match entry {
        ChatEntry::UserMessage { text, attachments } => {
            let label_span = Span::styled(
                format!("{} ", crate::i18n::t("zc-chat-label-you")),
                theme::user_label_style().add_modifier(sel_mod),
            );
            let body_style = theme::body_style().add_modifier(sel_mod);
            let mut text_lines: Vec<&str> = match text {
                Some(t) => t.split('\n').collect(),
                None => Vec::new(),
            };
            if text_lines.is_empty() {
                text_lines.push("");
            }
            for (idx, line_text) in text_lines.iter().enumerate() {
                let mut spans = Vec::new();
                if idx == 0 {
                    spans.push(label_span.clone());
                }
                spans.push(Span::styled((*line_text).to_string(), body_style));
                lines.push(Line::from(spans));
            }
            if !attachments.is_empty() {
                let label = attachments
                    .iter()
                    .map(|a| a.as_ref())
                    .collect::<Vec<&str>>()
                    .join(", ");
                lines.push(Line::from(Span::styled(
                    format!(" [{label}]"),
                    theme::warn_style().add_modifier(Modifier::ITALIC | sel_mod),
                )));
            }
        }
        ChatEntry::AgentMessage(text) => {
            render_agent_message_into(text, is_selected, width, lines);
        }
        ChatEntry::AgentMessageContinuation(text) => {
            render_agent_message_into(text, is_selected, width, lines);
        }
        ChatEntry::AgentThought(text) => {
            if show_thoughts {
                lines.push(Line::from(vec![
                    Span::styled("(thinking) ", theme::thought_style().add_modifier(sel_mod)),
                    Span::styled(text.to_string(), theme::dim_style().add_modifier(sel_mod)),
                ]));
            }
        }
        ChatEntry::SystemMessage(text) => {
            for line_text in text.lines() {
                lines.push(Line::from(Span::styled(
                    line_text.to_string(),
                    theme::warn_style().add_modifier(Modifier::ITALIC | sel_mod),
                )));
            }
        }
        ChatEntry::Tool {
            name,
            input_json,
            result,
            ..
        } => {
            return render_tool_entry(
                lines,
                name.as_ref(),
                input_json.as_ref(),
                result.as_deref().map(|s| s as &str),
                is_selected,
                tool_disclosure,
            );
        }
    }
    None
}

fn render_agent_message_into(
    text: &str,
    is_selected: bool,
    width: u16,
    lines: &mut Vec<Line<'static>>,
) {
    let sel_mod = if is_selected {
        Modifier::REVERSED
    } else {
        Modifier::empty()
    };
    lines.push(Line::from(vec![Span::styled(
        format!("{} ", crate::i18n::t("zc-chat-label-agent")),
        theme::agent_label_style().add_modifier(sel_mod),
    )]));
    let md_lines = markdown_to_lines(text, width);
    for mut line in md_lines {
        if is_selected {
            line = Line::from(
                line.spans
                    .into_iter()
                    .map(|s| s.patch_style(Style::default().add_modifier(Modifier::REVERSED)))
                    .collect::<Vec<_>>(),
            );
        }
        lines.push(line);
    }
}

/// Locate the `[Copy]` label within a code-fence bar line. Returns the label's
/// starting column (display cells from line start) and its trimmed width in
/// cells, or `None` if the line has no copy label.
fn label_cells(line: &Line<'static>, copy_lbl: &str) -> Option<(u16, u16)> {
    use unicode_width::UnicodeWidthStr;
    let mut col = 0u16;
    for span in &line.spans {
        let content = span.content.as_ref();
        if content == copy_lbl {
            let lead = copy_lbl.len() - copy_lbl.trim_start().len();
            let trimmed = copy_lbl.trim();
            return Some((col + lead as u16, UnicodeWidthStr::width(trimmed) as u16));
        }
        col += UnicodeWidthStr::width(content) as u16;
    }
    None
}

fn message_copy_label() -> String {
    crate::i18n::t("zc-chat-copy-message")
}

fn message_copied_label() -> String {
    crate::i18n::t("zc-chat-copy-message-copied")
}

#[cfg(test)]
fn context_menu_copy_label() -> String {
    crate::i18n::t("zc-chat-context-menu-copy")
}

fn context_menu_action_label(action: ChatContextMenuAction) -> String {
    let key = match action {
        ChatContextMenuAction::SendNow => "zc-chat-context-menu-send-now",
        ChatContextMenuAction::Copy => "zc-chat-context-menu-copy",
        ChatContextMenuAction::Edit => "zc-chat-context-menu-edit",
        ChatContextMenuAction::Delete => "zc-chat-context-menu-delete",
    };
    crate::i18n::t(key)
}

fn should_copy_current_selection(state: &ChatState, key: &KeyEvent) -> bool {
    use crate::keymap::ChatTabAction;

    should_copy_action(state, key, ChatTabAction::from_chord(key))
}

fn should_copy_action(
    state: &ChatState,
    key: &KeyEvent,
    action: Option<crate::keymap::ChatTabAction>,
) -> bool {
    use crate::keymap::ChatTabAction;

    match action {
        Some(action @ (ChatTabAction::CopySelection | ChatTabAction::CopyAllVisible)) => {
            state.in_browse_mode()
                || (state.transcript_selection.is_some()
                    && crate::keymap::action_bypasses_text_input(action, key))
        }
        _ => false,
    }
}

fn context_menu_rect(
    column: u16,
    row: u16,
    bounds: Rect,
    actions: &[ChatContextMenuAction],
) -> Option<Rect> {
    use unicode_width::UnicodeWidthStr;

    if bounds.width < 3 || bounds.height < 3 || actions.is_empty() {
        return None;
    }
    let label_width = actions
        .iter()
        .map(|action| UnicodeWidthStr::width(context_menu_action_label(*action).as_str()) as u16)
        .max()
        .unwrap_or(0);
    let width = (label_width + 4).min(bounds.width).max(3);
    let height = (actions.len() as u16 + 2).min(bounds.height);
    let max_x = bounds.x.saturating_add(bounds.width.saturating_sub(width));
    let max_y = bounds
        .y
        .saturating_add(bounds.height.saturating_sub(height));
    Some(Rect::new(
        column.clamp(bounds.x, max_x),
        row.clamp(bounds.y, max_y),
        width,
        height,
    ))
}

/// Recover the fence language token from a code-fence header bar line. The
/// header's first span is `┌─ lang ─────`; the ` code ` fallback label and an
/// empty info string both yield `None` so the rebuilt fence stays unlabelled.
fn header_fence_lang(line: &Line<'static>) -> Option<String> {
    let first = line.spans.first().map(|s| s.content.as_ref()).unwrap_or("");
    let token = first
        .trim_start_matches('\u{250c}')
        .trim_matches('\u{2500}')
        .trim();
    if token.is_empty() || token == "code" {
        None
    } else {
        Some(token.to_string())
    }
}

/// Return the code body for clipboard copy without markdown fences.
/// Users pasting into a terminal expect raw commands, not fenced blocks.
fn fenced_text(_lang: Option<&str>, body: &str) -> String {
    body.to_string()
}

fn append_wrapped_hit_rects(
    regions: &mut Vec<(usize, Rect)>,
    entry_idx: usize,
    line: &Line<'static>,
    screen_start: u16,
    scroll: u16,
    body: Rect,
) {
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    for (row_offset, visual_line) in crate::input_bar::wrap_visual_lines(&text, body.width)
        .iter()
        .enumerate()
    {
        let screen_row = screen_start.saturating_add(row_offset as u16);
        if screen_row < scroll || screen_row >= scroll.saturating_add(body.height) {
            continue;
        }
        let width = crate::display_width::display_width(&text[visual_line.start..visual_line.end])
            .min(usize::from(body.width));
        if width > 0 {
            regions.push((
                entry_idx,
                Rect::new(body.x, body.y + (screen_row - scroll), width as u16, 1),
            ));
        }
    }
}
/// Build a `[Copy]` region if its global wrapped row is on-screen.
fn copy_region(
    global_row: u16,
    col: u16,
    cells: u16,
    scroll: u16,
    body: Rect,
    text: &Arc<str>,
    group: usize,
) -> Option<CopyHitRegion> {
    if global_row < scroll || global_row >= scroll + body.height {
        return None;
    }
    Some(CopyHitRegion {
        rect: Rect::new(body.x + col, body.y + (global_row - scroll), cells, 1),
        text: Arc::clone(text),
        kind: CopyHitKind::Code,
        group,
    })
}

fn code_context_region(
    global_start: u16,
    global_end: u16,
    scroll: u16,
    body: Rect,
    text: &Arc<str>,
    group: usize,
) -> Option<CopyHitRegion> {
    let visible_start = global_start.max(scroll);
    let visible_end = global_end.min(scroll.saturating_add(body.height));
    if visible_end <= visible_start || text.is_empty() {
        return None;
    }
    Some(CopyHitRegion {
        rect: Rect::new(
            body.x,
            body.y + visible_start.saturating_sub(scroll),
            body.width,
            visible_end - visible_start,
        ),
        text: Arc::clone(text),
        kind: CopyHitKind::Code,
        group,
    })
}

fn centered_message_copy_rect(label: &str, anchor: Rect, body: Rect) -> Option<Rect> {
    use unicode_width::UnicodeWidthStr;

    if anchor.height == 0 || body.height == 0 || body.width == 0 {
        return None;
    }
    let cells = UnicodeWidthStr::width(label) as u16;
    if cells == 0 || cells > body.width {
        return None;
    }
    let row = anchor.y;
    if row < body.y || row >= body.y.saturating_add(body.height) {
        return None;
    }

    let x = body.x.saturating_add(body.width.saturating_sub(cells) / 2);
    Some(Rect::new(x, row, cells, 1))
}

fn centered_copy_feedback_rect(label: &str, anchor: Rect) -> Option<Rect> {
    use unicode_width::UnicodeWidthStr;

    let cells = UnicodeWidthStr::width(label) as u16;
    if cells == 0 || anchor.height == 0 {
        return None;
    }
    let center = anchor.x.saturating_add(anchor.width / 2);
    let x = center.saturating_sub(cells / 2);
    Some(Rect::new(x, anchor.y, cells, 1))
}

fn pinned_preview_source(message: &str, width: u16) -> &str {
    if width == 0 {
        return "";
    }

    let mut has_content = false;
    let mut cells = 0;
    for (offset, grapheme, grapheme_width) in crate::display_width::grapheme_widths(message) {
        // Match Span's control filtering and WordWrapper's oversized-symbol handling.
        if grapheme.contains(char::is_control) || grapheme_width > usize::from(width) {
            continue;
        }
        if !has_content {
            if ratatui::text::StyledGrapheme::new(grapheme, Style::default()).is_whitespace() {
                continue;
            }
            has_content = true;
        }
        // One positive-width lookahead lets the existing wrapper settle the first
        // row's word boundary without laying out the invisible message tail.
        if cells >= usize::from(width) && grapheme_width > 0 {
            return &message[..offset + grapheme.len()];
        }
        cells += grapheme_width;
    }
    message
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ConversationRenderWork {
    visible_cached_entries: usize,
    transcript_cached_lines: usize,
    copy_cached_blocks: usize,
    entry_rect_candidates: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VisibleCachedWindow {
    entries: Range<usize>,
    lines: Range<usize>,
    screen_lo: u16,
}

#[cfg(test)]
type ConversationRenderResult = ConversationRenderWork;
#[cfg(not(test))]
type ConversationRenderResult = ();

fn render_conversation(
    f: &mut Frame,
    state: &mut ChatState,
    area: Rect,
) -> ConversationRenderResult {
    state.refresh_title_hit_rects(area);
    state.expire_copy_feedback();

    // Width must be computed before cache rebuild — table column budgets
    // depend on it, and a width change invalidates cached layouts.
    let inner_width = area.width.saturating_sub(2);

    // ── Rebuild cached lines only when entries changed ────────
    if state.dirty != LinesDirty::Clean || state.cached_render_width != inner_width {
        state.rebuild_lines(inner_width);
    }

    // Determine transient overlays (live streaming / approval) up front from
    // cheap state reads. Both frame kinds render only a viewport slice;
    // transient frames additionally append the uncached overlay lines when
    // the window reaches past the cached history.
    let has_stream_text = !state.streaming_text.is_empty();
    let has_stream_thought = state.show_thoughts && !state.streaming_thought.is_empty();
    let has_approval = state.pending_approval().is_some();
    let transient = has_stream_text || has_stream_thought || has_approval;

    // Reserve a pinned top row inside the panel for the session's first user
    // message — a recovery reminder that stays put across scroll and reload.
    let show_first = state
        .first_message
        .as_deref()
        .is_some_and(|m| !m.is_empty());
    let first_row_h: u16 = if show_first && area.height > 2 { 1 } else { 0 };

    let inner_height = area.height.saturating_sub(2).saturating_sub(first_row_h);

    let block = theme::panel_block(&format!(" {} ", state.title()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if first_row_h == 1 {
        let first_row = Rect::new(inner.x, inner.y, inner.width, 1);
        let msg = state.first_message.as_deref().unwrap_or_default();
        let line = Line::from(Span::styled(
            pinned_preview_source(msg, first_row.width),
            theme::dim_style(),
        ));
        f.render_widget(Paragraph::new(line).wrap(Wrap { trim: true }), first_row);
    }

    // Conversation paragraph fills the inner area below the pinned row.
    let body_area = Rect::new(
        inner.x,
        inner.y + first_row_h,
        inner.width,
        inner.height.saturating_sub(first_row_h),
    );

    // Build the overlay-only line buffer (streaming text / thinking /
    // approval padding) on transient frames. History is never cloned here —
    // `visible_transient_slice` slices the cached history the same bounded
    // way idle frames do and appends this small overlay only once the
    // viewport window reaches it.
    let overlay_lines: Vec<Line<'static>> = if transient {
        state.build_overlay_lines(inner_width)
    } else {
        Vec::new()
    };
    let transient_row_breaks = if transient {
        row_breaks_for_lines(&overlay_lines, inner_width)
    } else {
        Vec::new()
    };

    let total_rows = if transient {
        let overlay_rows = Paragraph::new(overlay_lines.iter().map(borrow_line).collect::<Vec<_>>())
            .wrap(Wrap { trim: false })
            .line_count(inner_width) as u16;
        state.cached_total_rows.saturating_add(overlay_rows)
    } else {
        state.cached_total_rows
    };
    let max_scroll = total_rows.saturating_sub(inner_height);
    let scroll = if state.pinned_to_bottom {
        max_scroll
    } else {
        state.scroll_offset.min(max_scroll)
    };
    // Resolve the ordered cached window once at both entry and line
    // granularity. Overlays remain a separate on-demand segment and never
    // create a second transcript source.
    let visible_cached_window = state.visible_cached_window(scroll, inner_height);
    #[cfg(test)]
    let transcript_cached_lines = visible_cached_window.lines.len();

    // Both branches now render only the viewport slice, so cached-history
    // work stays O(log history + visible) instead of O(history), including
    // on transient frames (live streaming, approval overlay) where only the
    // small overlay buffer above is materialized in full.
    let (render_lines, render_scroll) = if transient {
        state.visible_transient_slice(scroll, inner_height, &visible_cached_window, overlay_lines)
    } else {
        state.visible_line_slice(scroll, &visible_cached_window)
    };

    let row_breaks = state
        .cached_row_breaks
        .iter()
        .chain(&transient_row_breaks)
        .copied()
        .skip(usize::from(scroll))
        .take(usize::from(body_area.height))
        .chain(std::iter::repeat(TranscriptRowBreak::Hard))
        .take(usize::from(body_area.height))
        .collect();

    let p = Paragraph::new(render_lines)
        .wrap(Wrap { trim: false })
        .scroll((render_scroll, 0));
    f.render_widget(p, body_area);
    capture_transcript_snapshot(f, state, body_area, row_breaks);
    render_transcript_selection(f, state);

    state.last_total_rows = total_rows;
    state.last_inner_height = inner_height;
    state.scroll_offset = scroll;

    // Project each entry's line range into screen coords. Off-viewport
    // ranges get no rect.
    let body_x = body_area.x;
    let body_y = body_area.y;
    let body_w = inner_width;
    let body_h = inner_height;
    state.entry_rects.clear();
    state.tool_header_rects.clear();
    state.tool_footer_rects.clear();
    for range_idx in visible_cached_window.entries.clone() {
        let (entry_idx, screen_lo, screen_hi, content_width) =
            state.cached_screen_ranges[range_idx];
        let visible_lo = screen_lo.max(scroll);
        let visible_hi = screen_hi.min(scroll.saturating_add(body_h));
        debug_assert!(visible_hi > visible_lo);
        // Width follows the entry's rendered text, not the full panel, so a
        // click in the blank margin beside a short message misses every rect
        // and clears the highlight.
        let rect = Rect::new(
            body_x,
            body_y + (visible_lo - scroll),
            content_width.min(body_w),
            visible_hi - visible_lo,
        );
        state.entry_rects.push((entry_idx, rect));

        if matches!(state.entries.get(entry_idx), Some(ChatEntry::Tool { .. })) {
            let (_, line_lo, line_hi) = state.cached_line_ranges[range_idx];
            let header_line = &state.cached_lines[line_lo];
            append_wrapped_hit_rects(
                &mut state.tool_header_rects,
                entry_idx,
                header_line,
                screen_lo,
                scroll,
                body_area,
            );

            if let Some(&footer_line) = state.cached_tool_footer_lines.get(&entry_idx)
                && (line_lo..line_hi).contains(&footer_line)
            {
                let footer_screen_lo = state.cached_line_screen_ranges[footer_line].0;
                append_wrapped_hit_rects(
                    &mut state.tool_footer_rects,
                    entry_idx,
                    &state.cached_lines[footer_line],
                    footer_screen_lo,
                    scroll,
                    body_area,
                );
            }
        }
    }

    let body_rect = Rect::new(body_x, body_y, body_w, body_h);
    let copy_cached_blocks = state.rebuild_copy_regions(scroll, body_rect);
    if state.in_browse_mode() {
        state.rebuild_message_copy_region(body_rect);
    } else {
        render_transcript_copy_overlay(f, state);
    }
    render_copy_feedback(f, state);
    render_message_copy_overlay(f, state, body_rect);
    render_context_menu(f, state);
    let mut scrollbar_state = ScrollbarState::new(total_rows as usize)
        .position(scroll as usize)
        .viewport_content_length(inner_height as usize);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None),
        area,
        &mut scrollbar_state,
    );
    // Scrollbar paints in `area.right() - 1`; mirror that.
    if area.height > 2 {
        state.scrollbar_track_rect = Some(Rect::new(
            area.x + area.width.saturating_sub(1),
            area.y + 1,
            1,
            area.height - 2,
        ));
    } else {
        state.scrollbar_track_rect = None;
    }

    #[cfg(test)]
    {
        ConversationRenderWork {
            visible_cached_entries: visible_cached_window.entries.len(),
            transcript_cached_lines,
            copy_cached_blocks,
            entry_rect_candidates: visible_cached_window.entries.len(),
        }
    }
    #[cfg(not(test))]
    {
        let _ = copy_cached_blocks;
    }
}

fn capture_transcript_snapshot(
    f: &mut Frame,
    state: &mut ChatState,
    body: Rect,
    row_breaks: Vec<TranscriptRowBreak>,
) {
    state.set_transcript_snapshot(TranscriptSnapshot::capture(f, body, row_breaks));
}

fn render_transcript_selection(f: &mut Frame, state: &ChatState) {
    let (Some(snapshot), Some(selection)) =
        (&state.transcript_snapshot, state.transcript_selection)
    else {
        return;
    };
    snapshot.render_selection(f, selection, theme::selected_bg_style());
}

fn render_transcript_copy_overlay(f: &mut Frame, state: &mut ChatState) {
    state
        .copy_hit_regions
        .retain(|region| region.kind != CopyHitKind::Transcript);

    let Some(snapshot) = &state.transcript_snapshot else {
        return;
    };
    let Some(selection) = state.transcript_selection else {
        return;
    };
    let Some(text) = snapshot.selected_text(selection) else {
        return;
    };
    let Some(anchor) = snapshot.selection_anchor_rect(selection) else {
        return;
    };
    let label = message_copy_label();
    let Some(rect) = centered_message_copy_rect(&label, anchor, snapshot.area) else {
        return;
    };

    state.copy_hit_regions.push(CopyHitRegion {
        rect,
        text: text.into(),
        kind: CopyHitKind::Transcript,
        group: 0,
    });
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label,
            theme::accent_style().add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        rect,
    );
}

fn render_message_copy_overlay(f: &mut Frame, state: &ChatState, body: Rect) {
    let Some(region) = state.message_copy_region(body) else {
        return;
    };
    f.render_widget(Clear, region.rect);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            message_copy_label(),
            theme::accent_style().add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        region.rect,
    );
}

fn render_copy_feedback(f: &mut Frame, state: &ChatState) {
    let Some(feedback) = state.copy_feedback else {
        return;
    };

    match feedback.target {
        CopyFeedbackTarget::Code(group) => {
            let label = message_copied_label();
            for region in state
                .copy_hit_regions
                .iter()
                .filter(|region| region.kind == CopyHitKind::Code && region.group == group)
            {
                if let Some(rect) = centered_copy_feedback_rect(&label, region.rect) {
                    render_copied_label(f, &label, rect);
                }
            }
        }
        CopyFeedbackTarget::Overlay(rect) => {
            render_copied_label(f, &message_copied_label(), rect);
        }
    }
}

fn render_context_menu(f: &mut Frame, state: &ChatState) {
    let Some(menu) = &state.context_menu else {
        return;
    };
    f.render_widget(Clear, menu.rect);
    let lines = menu
        .target
        .actions()
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let style = if index == menu.selected {
                theme::accent_style().add_modifier(Modifier::BOLD)
            } else {
                theme::body_style()
            };
            Line::from(Span::styled(context_menu_action_label(*action), style))
        })
        .collect::<Vec<_>>();
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::accent_style()),
        ),
        menu.rect,
    );
}

fn render_copied_label(f: &mut Frame, label: &str, rect: Rect) {
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label.to_string(),
            theme::success_style().add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        rect,
    );
}

fn render_approval_overlay(f: &mut Frame, state: &ChatState, area: Rect) {
    let pa = match state.pending_approval() {
        Some(p) => p,
        None => return,
    };

    // Anchor to the bottom of the given area.
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(APPROVAL_OVERLAY_HEIGHT),
        ])
        .split(area);
    let overlay_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(5),
            Constraint::Min(60),
            Constraint::Percentage(5),
        ])
        .split(vert[1])[1];

    f.render_widget(Clear, overlay_area);

    let is_edit_tool = matches!(pa.tool_name.as_str(), "file_edit" | "file_write");
    let allow = crate::i18n::t("zc-chat-approval-action-allow");
    let always = crate::i18n::t("zc-chat-approval-action-always");
    let reject = crate::i18n::t("zc-chat-approval-action-reject");
    let edit = crate::i18n::t("zc-chat-approval-action-edit");
    let keys = if is_edit_tool {
        format!("Enter={allow}  a={always}  Ctrl+D={reject}  e={edit}")
    } else {
        format!("Enter={allow}  a={always}  Ctrl+D={reject}")
    };

    // For file_edit/file_write, strip the bulk content fields — the diff
    // preview in the conversation already shows old/new content.
    let summary = if is_edit_tool {
        strip_content_fields(&pa.arguments_summary)
    } else {
        pa.arguments_summary.clone()
    };

    let secs = pa.timeout_secs.to_string();
    let title = crate::i18n::t_args(
        "zc-chat-approval-title",
        &[("tool", &pa.tool_name), ("secs", &secs)],
    );
    let text = if summary.is_empty() {
        format!("{title}\n\n  {keys}")
    } else {
        format!("{title}\n\n  {summary}\n\n  {keys}")
    };

    let fill = theme::fill_style();
    let p = Paragraph::new(text)
        .style(fill)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Approval Required ", theme::warn_style()))
                .border_style(theme::approval_border_style())
                .style(fill),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(p, overlay_area);
}

fn render_elicitation_overlay(f: &mut Frame, state: &ChatState, area: Rect) {
    let e = match state.pending_elicitation() {
        Some(e) => e,
        None => return,
    };

    // Body lines: message (wrapped by the List items below it is not, so
    // we keep the message in the block title area) + one row per choice +
    // a key-hint footer. Budget: 2 border + 1 message + N choices + 1
    // footer, clamped to the area height.
    let choice_rows = e.choices.len() as u16;
    let desired = choice_rows.saturating_add(5); // borders + msg + footer + pad
    let max_h = area.height.saturating_sub(2).max(3);
    let overlay_h = desired.min(max_h).max(3);

    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(overlay_h)])
        .split(area);
    let overlay_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(5),
            Constraint::Min(60),
            Constraint::Percentage(5),
        ])
        .split(vert[1])[1];

    f.render_widget(Clear, overlay_area);

    let fill = theme::fill_style();
    let title = if e.multi {
        let n = e.selected_count();
        format!(
            " Choose ({n} selected, need {}..={}) ",
            e.min_items, e.max_items
        )
    } else {
        String::from(" Choose one ")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, theme::warn_style()))
        .border_style(theme::approval_border_style())
        .style(fill);
    let inner = block.inner(overlay_area);
    f.render_widget(block, overlay_area);

    // Split inner: message line(s), choice list, footer hint.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let msg = Paragraph::new(e.message.clone())
        .style(fill)
        .wrap(Wrap { trim: true });
    f.render_widget(msg, chunks[0]);

    let items: Vec<ListItem> = e
        .choices
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let checkbox = if e.multi {
                if e.selected.get(i).copied().unwrap_or(false) {
                    "[x] "
                } else {
                    "[ ] "
                }
            } else {
                ""
            };
            let line = format!("{checkbox}{title}");
            let style = if i == e.cursor {
                theme::selected_style()
            } else {
                fill
            };
            ListItem::new(Line::from(Span::styled(line, style)))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(e.cursor.min(e.choices.len().saturating_sub(1))));
    let list = List::new(items).style(fill);
    f.render_stateful_widget(list, chunks[1], &mut list_state);

    let hint = if e.multi {
        "↑/↓ move  Space toggle  Enter confirm  Esc cancel"
    } else {
        "↑/↓ move  Enter confirm  Esc cancel"
    };
    let footer = Paragraph::new(Span::styled(hint, theme::dim_style())).style(fill);
    f.render_widget(footer, chunks[2]);
}

/// compact when a diff preview is already shown in the conversation.
fn strip_content_fields(summary: &str) -> String {
    let mut s = summary;
    for key in &["old_string", "new_string", "content"] {
        // Key appears mid-string as ", key: …"
        if let Some(i) = s.find(&format!(", {key}:")) {
            s = &s[..i];
        } else if s.starts_with(&format!("{key}:")) {
            s = "";
        }
    }
    s.trim_end_matches([',', ' ']).to_string()
}

// ── Session overlay rendering ─────────────────────────────────────

/// Compute the overlay rect for the session list picker.
/// Kept in sync with `render_session_list_overlay` so mouse hit-testing
/// can use the same geometry without storing extra state.
fn session_list_overlay_area(area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Min(8),
            Constraint::Percentage(20),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Min(40),
            Constraint::Percentage(15),
        ])
        .split(vert[1])[1]
}

/// Shrink the session-list overlay rect to the rows that actually render
/// list items when a footer `note` is present, mirroring the carve-out in
/// [`render_session_list_overlay`]. `mouse::list_click_index` treats every
/// row inside the border as list content, so hit-testing against the full
/// overlay rect would map clicks on the note rows to (possibly scrolled
/// off-screen) session indices. Keeping this next to
/// [`session_list_overlay_area`] preserves the "same geometry, no stored
/// state" contract for mouse handling.
fn session_list_click_area(overlay_area: Rect, note: Option<&str>) -> Rect {
    let Some(note) = note else {
        return overlay_area;
    };
    let inner_width = overlay_area.width.saturating_sub(2);
    let inner_height = overlay_area.height.saturating_sub(2);
    let reserved = note_reserved_rows(note, inner_width);
    if inner_height > reserved {
        Rect::new(
            overlay_area.x,
            overlay_area.y,
            overlay_area.width,
            overlay_area.height - reserved,
        )
    } else {
        // The render path keeps the full inner rect for the list when the
        // note cannot fit; mirror that here.
        overlay_area
    }
}

/// Rows to reserve for the dim footer `note` so it renders in full when
/// wrapped at `inner_width`. ratatui's `Wrap { trim: true }` breaks on word
/// boundaries, so the row count is *not* `ceil(display_width / inner_width)` —
/// a word that would overflow the current line is pushed whole to the next one,
/// which can cost an extra row. We mirror that word-boundary packing here so a
/// narrow inner width (e.g. the 80x24 default) reserves enough rows for every
/// wrapped line. The disclosure is short, fixed catalogue copy, so its full
/// wrapped height is authoritative: clipping it would hide the persistent-
/// memory isolation half of the contract on narrow Code panes.
fn note_reserved_rows(note: &str, inner_width: u16) -> u16 {
    if inner_width == 0 {
        return 1;
    }
    Paragraph::new(note)
        .wrap(Wrap { trim: true })
        .line_count(inner_width)
        .try_into()
        .unwrap_or(u16::MAX)
        .max(1)
}

fn render_session_list_overlay(
    f: &mut Frame,
    area: Rect,
    sessions: &[SessionEntry],
    list_state: &mut ListState,
    title: String,
    note: Option<String>,
) {
    let overlay_area = session_list_overlay_area(area);

    f.render_widget(Clear, overlay_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, theme::overlay_border_style()))
        .border_style(theme::overlay_border_style())
        .style(theme::fill_style());

    let inner = block.inner(overlay_area);
    f.render_widget(block, overlay_area);

    // Reserve enough dim footer rows for the note (if any) to render in full at
    // the current inner width, so narrow terminals (e.g. the 80x24 default)
    // don't silently drop the second wrapped line of the memory-isolation
    // disclosure. It is only carved out when at least one list row survives.
    let (list_area, note_area) = match &note {
        Some(text) => {
            let reserved = note_reserved_rows(text, inner.width);
            if inner.height > reserved {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(reserved)])
                    .split(inner);
                (chunks[0], Some(chunks[1]))
            } else {
                (inner, None)
            }
        }
        None => (inner, None),
    };

    let items: Vec<ListItem> = sessions
        .iter()
        .map(|s| {
            let name = s.name.as_deref().unwrap_or(&s.session_id);
            let agent = s.agent_alias.as_deref().unwrap_or("?");
            let label = format!("{name}  ({agent}, {} msgs)", s.message_count);
            ListItem::new(Span::styled(label, theme::body_style()))
        })
        .collect();

    let list = List::new(items).highlight_style(theme::list_highlight_style());
    // Render through the caller's state so the scroll offset ratatui computes
    // to keep the selection visible is retained. Mouse hit-testing later reads
    // `list_state.offset()`, so a discarded offset would make clicks after a
    // scroll resolve to the wrong row. `list_area` is `inner` minus any
    // reserved note rows, so the offset stays consistent with the rows the
    // user can actually see.
    f.render_stateful_widget(list, list_area, list_state);

    if let (Some(text), Some(note_area)) = (note, note_area) {
        let note_line =
            Paragraph::new(Span::styled(text, theme::dim_style())).wrap(Wrap { trim: true });
        f.render_widget(note_line, note_area);
    }
}

fn emit_code_block_body(lines: &mut Vec<Line<'static>>, text: &str, lang: Option<&str>) {
    let body = text.strip_suffix('\n').unwrap_or(text);
    if body.is_empty() {
        return;
    }
    let plain_fg = theme::active().body;
    let highlighted = lang.and_then(|token| crate::diff::highlight_code(body, token, plain_fg));
    match highlighted {
        Some(hl) => {
            for line in hl {
                let mut spans = vec![Span::styled("  ".to_string(), theme::code_block_style())];
                spans.extend(line.spans);
                lines.push(Line::from(spans));
            }
        }
        None => {
            for code_line in body.split('\n') {
                lines.push(Line::from(Span::styled(
                    format!("  {code_line}"),
                    theme::code_block_style(),
                )));
            }
        }
    }
}

/// Builds one full-width code-block border bar: `corner_l`, an optional left
/// label (the language), then dashes wrapping a centered `[Copy]`, then
/// `corner_r`. Header and footer share this so their geometry can never drift.
fn code_block_bar(
    width: u16,
    corner_l: char,
    corner_r: char,
    label: Option<&str>,
) -> Line<'static> {
    let label = label.unwrap_or("");
    let copy_lbl = " [Copy] ";
    let label_len = label.chars().count();
    let copy_len = copy_lbl.chars().count();
    let inner = (width as usize).saturating_sub(2);
    let left_total = inner.saturating_sub(copy_len) / 2;
    let right = inner.saturating_sub(copy_len).saturating_sub(left_total);
    let left_dashes = left_total.saturating_sub(label_len);
    Line::from(vec![
        Span::styled(
            format!("{corner_l}{label}{}", "\u{2500}".repeat(left_dashes)),
            theme::dim_style(),
        ),
        Span::styled(
            copy_lbl.to_string(),
            theme::accent_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}{corner_r}", "\u{2500}".repeat(right)),
            theme::dim_style(),
        ),
    ])
}

fn markdown_to_lines(text: &str, width: u16) -> Vec<Line<'static>> {
    use pulldown_cmark::{Alignment as MdAlign, HeadingLevel};

    let mut opts = MdOptions::empty();
    opts.insert(MdOptions::ENABLE_TABLES);
    opts.insert(MdOptions::ENABLE_STRIKETHROUGH);
    opts.insert(MdOptions::ENABLE_TASKLISTS);
    let parser = MdParser::new_ext(text, opts);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut in_bold = false;
    let mut in_italic = false;
    let mut in_strike = false;
    let mut in_code_block = false;
    let mut code_block_text: String = String::new();
    let mut code_block_lang: Option<String> = None;
    let mut heading_level: Option<HeadingLevel> = None;
    let mut blockquote_depth: u32 = 0;
    let mut link_url: Option<String> = None;

    // Stack of enclosing lists. `Some(next)` is an ordered list whose next item
    // renders `next.` and then increments; `None` is a bullet list. The stack
    // depth drives per-level indentation so nested lists step inward.
    let mut list_stack: Vec<Option<u64>> = Vec::new();

    // Table state. While non-`None`, text/inline events accumulate into the
    // current cell instead of the live `current_spans` line.
    struct TableBuf {
        alignments: Vec<MdAlign>,
        rows: Vec<Vec<String>>,
        in_header: bool,
        current_row: Vec<String>,
        current_cell: Option<String>,
    }
    let mut table: Option<TableBuf> = None;

    let push_line = |lines: &mut Vec<Line<'static>>, spans: &mut Vec<Span<'static>>| {
        if !spans.is_empty() {
            lines.push(Line::from(std::mem::take(spans)));
        }
    };

    let blockquote_gutter = |depth: u32| -> Vec<Span<'static>> {
        (0..depth)
            .map(|_| Span::styled("\u{2502} ", theme::dim_style()))
            .collect()
    };

    for event in parser {
        // While inside a table cell, route inline events into the cell
        // buffer. The table only lays out at TagEnd::Table.
        if let Some(t) = table.as_mut()
            && let Some(cell) = t.current_cell.as_mut()
        {
            match &event {
                MdEvent::Text(s) | MdEvent::Code(s) => {
                    cell.push_str(s);
                    continue;
                }
                MdEvent::SoftBreak | MdEvent::HardBreak => {
                    cell.push(' ');
                    continue;
                }
                _ => {}
            }
        }

        match event {
            MdEvent::Start(Tag::Strong) => in_bold = true,
            MdEvent::End(TagEnd::Strong) => in_bold = false,
            MdEvent::Start(Tag::Emphasis) => in_italic = true,
            MdEvent::End(TagEnd::Emphasis) => in_italic = false,
            MdEvent::Start(Tag::Strikethrough) => in_strike = true,
            MdEvent::End(TagEnd::Strikethrough) => in_strike = false,
            MdEvent::Start(Tag::Heading { level, .. }) => {
                push_line(&mut lines, &mut current_spans);
                lines.push(Line::default());
                heading_level = Some(level);
                if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) {
                    current_spans.push(Span::styled("\u{258C} ", theme::accent_style()));
                }
            }
            MdEvent::End(TagEnd::Heading(_)) => {
                push_line(&mut lines, &mut current_spans);
                lines.push(Line::default());
                heading_level = None;
            }
            MdEvent::Start(Tag::BlockQuote(_)) => {
                push_line(&mut lines, &mut current_spans);
                blockquote_depth += 1;
            }
            MdEvent::End(TagEnd::BlockQuote(_)) => {
                push_line(&mut lines, &mut current_spans);
                blockquote_depth = blockquote_depth.saturating_sub(1);
            }
            MdEvent::Start(Tag::Link { dest_url, .. }) => {
                link_url = Some(dest_url.to_string());
            }
            MdEvent::End(TagEnd::Link) => {
                if let Some(url) = link_url.take() {
                    current_spans.push(Span::styled(
                        format!(" ({url})"),
                        theme::dim_style().add_modifier(Modifier::ITALIC),
                    ));
                }
            }
            MdEvent::Start(Tag::CodeBlock(kind)) => {
                push_line(&mut lines, &mut current_spans);
                in_code_block = true;
                code_block_text.clear();
                code_block_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };

                // Header bar: ┌─ lang ──── [Copy] ────┐
                let lang_display = code_block_lang.clone().unwrap_or_default();
                let label = if lang_display.is_empty() {
                    " code ".to_string()
                } else {
                    format!(" {} ", lang_display.as_str())
                };
                lines.push(code_block_bar(
                    width,
                    '\u{250c}',
                    '\u{2510}',
                    Some(&format!("\u{2500}{label}")),
                ));
            }
            MdEvent::End(TagEnd::CodeBlock) => {
                push_line(&mut lines, &mut current_spans);
                in_code_block = false;

                emit_code_block_body(&mut lines, &code_block_text, code_block_lang.as_deref());

                // Footer bar: └──── [Copy] ────┘
                lines.push(code_block_bar(width, '\u{2514}', '\u{2518}', None));

                // Accumulated code text is ready for clipboard copy;
                // the Copy action is handled by the chat pane.
                code_block_text.clear();
                code_block_lang = None;
            }
            MdEvent::Start(Tag::List(start)) => {
                push_line(&mut lines, &mut current_spans);
                list_stack.push(start);
            }
            MdEvent::End(TagEnd::List(_)) => {
                push_line(&mut lines, &mut current_spans);
                list_stack.pop();
            }
            MdEvent::Start(Tag::Item) => {
                push_line(&mut lines, &mut current_spans);
                current_spans.extend(blockquote_gutter(blockquote_depth));
                let depth = list_stack.len().saturating_sub(1);
                current_spans.push(Span::styled("  ".repeat(depth + 1), theme::dim_style()));
                let marker = match list_stack.last_mut() {
                    Some(Some(next)) => {
                        let label = format!("{next}. ");
                        *next += 1;
                        label
                    }
                    _ => "\u{2022} ".to_string(),
                };
                current_spans.push(Span::styled(marker, theme::dim_style()));
            }
            MdEvent::End(TagEnd::Item) if !current_spans.is_empty() => {
                push_line(&mut lines, &mut current_spans);
            }
            MdEvent::Start(Tag::Paragraph) if blockquote_depth > 0 && current_spans.is_empty() => {
                current_spans.extend(blockquote_gutter(blockquote_depth));
            }
            MdEvent::Start(Tag::Paragraph) => {}
            MdEvent::End(TagEnd::Paragraph) if !current_spans.is_empty() => {
                push_line(&mut lines, &mut current_spans);
            }
            MdEvent::TaskListMarker(checked) => {
                let glyph = if checked { "\u{2611} " } else { "\u{2610} " };
                current_spans.push(Span::styled(glyph, theme::accent_style()));
            }
            // ── Tables ──────────────────────────────────────────
            MdEvent::Start(Tag::Table(alignments)) => {
                push_line(&mut lines, &mut current_spans);
                table = Some(TableBuf {
                    alignments,
                    rows: Vec::new(),
                    in_header: false,
                    current_row: Vec::new(),
                    current_cell: None,
                });
            }
            MdEvent::Start(Tag::TableHead) => {
                if let Some(t) = table.as_mut() {
                    t.in_header = true;
                    t.current_row.clear();
                }
            }
            MdEvent::End(TagEnd::TableHead) => {
                if let Some(t) = table.as_mut() {
                    let row = std::mem::take(&mut t.current_row);
                    t.rows.push(row);
                    t.in_header = false;
                }
            }
            MdEvent::Start(Tag::TableRow) => {
                if let Some(t) = table.as_mut() {
                    t.current_row.clear();
                }
            }
            MdEvent::End(TagEnd::TableRow) => {
                if let Some(t) = table.as_mut() {
                    let row = std::mem::take(&mut t.current_row);
                    t.rows.push(row);
                }
            }
            MdEvent::Start(Tag::TableCell) => {
                if let Some(t) = table.as_mut() {
                    t.current_cell = Some(String::new());
                }
            }
            MdEvent::End(TagEnd::TableCell) => {
                if let Some(t) = table.as_mut()
                    && let Some(cell) = t.current_cell.take()
                {
                    t.current_row.push(cell);
                }
            }
            MdEvent::End(TagEnd::Table) => {
                if let Some(t) = table.take() {
                    lines.extend(render_table(t.rows, t.alignments, width));
                }
            }
            MdEvent::Text(t) => {
                let owned = t.to_string();
                if in_code_block {
                    code_block_text.push_str(&owned);
                } else {
                    let mut style = theme::body_style();
                    if let Some(level) = heading_level {
                        style = match level {
                            HeadingLevel::H1 | HeadingLevel::H2 => {
                                theme::heading_style().add_modifier(Modifier::BOLD)
                            }
                            _ => theme::heading_style(),
                        };
                    }
                    if in_bold {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if in_italic {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    if in_strike {
                        style = style.add_modifier(Modifier::CROSSED_OUT);
                    }
                    if link_url.is_some() {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    current_spans.push(Span::styled(owned, style));
                }
            }
            MdEvent::Code(t) => {
                current_spans.push(Span::styled(t.to_string(), theme::code_inline_style()));
            }
            MdEvent::SoftBreak => {
                current_spans.push(Span::raw(" "));
            }
            MdEvent::HardBreak => {
                push_line(&mut lines, &mut current_spans);
                if blockquote_depth > 0 {
                    current_spans.extend(blockquote_gutter(blockquote_depth));
                }
            }
            _ => {}
        }
    }

    if !current_spans.is_empty() {
        lines.push(Line::from(current_spans));
    }

    // Fallback: if parsing produced nothing, return raw text.
    if lines.is_empty() && !text.is_empty() {
        lines.push(Line::from(Span::styled(
            text.to_string(),
            theme::body_style(),
        )));
    }

    lines
}

fn render_table(
    rows: Vec<Vec<String>>,
    alignments: Vec<pulldown_cmark::Alignment>,
    width: u16,
) -> Vec<Line<'static>> {
    use pulldown_cmark::Alignment as MdAlign;

    if rows.is_empty() {
        return Vec::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return Vec::new();
    }

    // Normalise: pad short rows so every row has `cols` cells.
    let mut grid: Vec<Vec<String>> = rows;
    for row in &mut grid {
        while row.len() < cols {
            row.push(String::new());
        }
    }

    // Natural width per column = longest cell.
    let mut natural: Vec<usize> = vec![0; cols];
    for row in &grid {
        for (i, cell) in row.iter().enumerate() {
            natural[i] = natural[i].max(crate::display_width::display_width(cell.as_str()));
        }
    }

    // Frame budget: `│` borders (cols+1) + one-cell padding either side
    // of each cell (cols * 2).
    let frame = (cols + 1) + cols * 2;
    let avail = (width as usize).saturating_sub(frame);
    let total_natural: usize = natural.iter().sum();

    let widths: Vec<usize> = if total_natural <= avail || total_natural == 0 {
        natural.clone()
    } else {
        // Scale each column proportionally. Floor at 1 cell so columns
        // don't vanish; the renderer collapses 1–3 cell columns to `…`.
        natural
            .iter()
            .map(|n| ((*n * avail) / total_natural).max(1))
            .collect()
    };

    fn truncate_to(s: &str, budget: usize) -> String {
        if budget == 0 {
            return String::new();
        }
        let full_width = crate::display_width::display_width(s);
        if full_width <= budget {
            return s.to_string();
        }
        // Cell needs truncation but budget is too narrow to convey any
        // content + ellipsis — collapse to a single `…`.
        if budget < 2 {
            return "\u{2026}".to_string();
        }
        let mut acc = String::new();
        let mut used = 0usize;
        // Walk graphemes so presentation sequences (⚠️, 🏔️) stay intact.
        for (_offset, grapheme, w) in crate::display_width::grapheme_widths(s) {
            if used + w + 1 > budget {
                acc.push('\u{2026}');
                return acc;
            }
            acc.push_str(grapheme);
            used += w;
            if used == budget {
                return acc;
            }
        }
        acc
    }

    fn pad_cell(s: &str, budget: usize, align: MdAlign) -> String {
        let w = crate::display_width::display_width(s);
        let slack = budget.saturating_sub(w);
        match align {
            MdAlign::Right => format!("{}{}", " ".repeat(slack), s),
            MdAlign::Center => {
                let left = slack / 2;
                let right = slack - left;
                format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
            }
            MdAlign::None | MdAlign::Left => format!("{}{}", s, " ".repeat(slack)),
        }
    }

    let border = |left: &str, mid: &str, right: &str| -> Line<'static> {
        let mut s = String::from(left);
        for (i, w) in widths.iter().enumerate() {
            s.push_str(&"\u{2500}".repeat(w + 2));
            if i + 1 < widths.len() {
                s.push_str(mid);
            }
        }
        s.push_str(right);
        Line::from(Span::styled(s, theme::dim_style()))
    };

    let render_row = |cells: &[String]| -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled("\u{2502}".to_string(), theme::dim_style()));
        for (i, cell) in cells.iter().enumerate() {
            let budget = widths[i];
            let trimmed = truncate_to(cell, budget);
            let align = alignments.get(i).copied().unwrap_or(MdAlign::None);
            let padded = pad_cell(&trimmed, budget, align);
            spans.push(Span::raw(format!(" {padded} ")));
            spans.push(Span::styled("\u{2502}".to_string(), theme::dim_style()));
        }
        Line::from(spans)
    };

    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(border("\u{250C}", "\u{252C}", "\u{2510}"));
    let mut iter = grid.into_iter();
    if let Some(header) = iter.next() {
        out.push(render_row(&header));
        out.push(border("\u{251C}", "\u{253C}", "\u{2524}"));
    }
    for row in iter {
        out.push(render_row(&row));
    }
    out.push(border("\u{2514}", "\u{2534}", "\u{2518}"));
    out
}

// ── ChatState / ChatEntry ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub request_id: String,
    pub tool_name: String,
    pub arguments_summary: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct PendingElicitation {
    /// JSON-RPC request id to respond to. Echoed verbatim.
    pub request_id: serde_json::Value,
    /// Session this elicitation belongs to. Set at install time; routing
    /// (`try_install_elicitation`) and the sidebar status derivation
    /// (`ChatState::sidebar_status`) key off it.
    pub session_id: String,
    /// Prompt text shown above the choice list.
    pub message: String,
    /// User-visible choice titles, in wire order. The `choice-N` const
    /// is the index into this vec.
    pub choices: Vec<String>,
    /// Whether this is a multi-select (checkbox) prompt.
    pub multi: bool,
    /// Multi-select lower bound (inclusive). Ignored for single-select.
    pub min_items: usize,
    /// Multi-select upper bound (inclusive). Ignored for single-select.
    pub max_items: usize,
    /// Highlighted row.
    pub cursor: usize,
    /// Per-row checkbox state for multi-select. Empty / unused for
    /// single-select.
    pub selected: Vec<bool>,
}

impl PendingElicitation {
    /// Number of currently-checked rows (multi-select only).
    pub fn selected_count(&self) -> usize {
        self.selected.iter().filter(|&&b| b).count()
    }

    /// Whether the current selection satisfies the multi-select
    /// `min_items`/`max_items` bounds. Always `true` for single-select
    /// (the cursor itself is the answer).
    pub fn selection_valid(&self) -> bool {
        if !self.multi {
            return !self.choices.is_empty();
        }
        let n = self.selected_count();
        n >= self.min_items && n <= self.max_items
    }

    /// Build the `accept` content payload for the current selection, or
    /// `None` if the selection is invalid (e.g. too few boxes checked).
    pub fn accept_content(&self) -> Option<serde_json::Value> {
        if !self.selection_valid() {
            return None;
        }
        if self.multi {
            let consts: Vec<serde_json::Value> = self
                .selected
                .iter()
                .enumerate()
                .filter(|&(_, &on)| on)
                .map(|(i, _)| serde_json::json!(format!("choice-{i}")))
                .collect();
            Some(serde_json::json!({ "choices": consts }))
        } else {
            Some(serde_json::json!({ "choice": format!("choice-{}", self.cursor) }))
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChatEntry {
    AgentMessage(Arc<str>),
    /// A response committed after the prompt RPC returned but before its
    /// terminal notification arrived. Late chunks extend this buffer in place;
    /// the next ordering boundary freezes it back into `AgentMessage`.
    AgentMessageContinuation(String),
    AgentThought(Arc<str>),
    /// Local system/info message (e.g. "Attached: photo.png").
    SystemMessage(Arc<str>),
    UserMessage {
        text: Option<Arc<str>>,
        attachments: Vec<Arc<str>>,
    },
    Tool {
        tool_call_id: Arc<str>,
        name: Arc<str>,
        /// Pre-serialised JSON of the tool input. Storing the
        /// rendered string instead of a `serde_json::Value` tree
        /// drops the per-entry parsed-tree footprint (one
        /// allocation per Value node) to a single `Arc<str>`.
        input_json: Arc<str>,
        /// Tool output. `None` while the call is in flight,
        /// `Some(Arc<str>)` once the result arrives.
        result: Option<Arc<str>>,
    },
}

#[derive(Debug)]
enum SessionOverlay {
    None,
    List {
        sessions: Vec<SessionEntry>,
        list_state: ListState,
    },
}

/// Active model / model_provider picker overlay. `None` when no picker is open.
/// The model_provider variant is two-stage: pick a model_provider, then (after a
/// catalog fetch) pick a model from it.
#[derive(Debug, Clone, Default)]
enum ModelPickerOverlay {
    /// No picker open.
    #[default]
    None,
    /// Catalog fetch in flight — drawn as a modal so the user sees a
    /// waiting state instead of a frozen UI while the models load.
    Loading,
    /// Single-stage model picker over the active model_provider's catalog.
    Model(crate::widgets::PickerState),
    ConfiguredProviderStage(crate::widgets::PickerState),
}

impl ModelPickerOverlay {
    fn is_open(&self) -> bool {
        !matches!(self, Self::None)
    }

    fn item_count(&self) -> usize {
        match self {
            Self::Model(p) | Self::ConfiguredProviderStage(p) => p.items.len(),
            Self::Loading => 1,
            Self::None => 0,
        }
    }

    fn picker_mut(&mut self) -> Option<&mut crate::widgets::PickerState> {
        match self {
            Self::Model(p) | Self::ConfiguredProviderStage(p) => Some(p),
            Self::Loading | Self::None => None,
        }
    }
}

/// Tracks what kind of update has invalidated the rendered lines cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinesDirty {
    /// Cache is up-to-date.
    Clean,
    /// New entries were appended at the tail; the render window has not shifted.
    /// `rebuild_lines` can extend `cached_lines` instead of rebuilding from scratch,
    /// avoiding re-parsing markdown for unchanged `AgentMessage` entries.
    Appended,
    /// The final cached entry changed without shifting the render window.
    TailChanged(usize),
    /// Full rebuild required (entry mutation, selection/thoughts change, reset).
    Full,
}

const MAX_RENDERED_ENTRIES: usize = 1_000;
const RENDER_WINDOW_SHIFT_ENTRIES: usize = MAX_RENDERED_ENTRIES / 2;

/// Scrollbar drag captured on mouse-down on the track.
#[derive(Debug, Clone, Copy)]
struct ScrollbarDrag {
    start_scroll: u16,
    start_row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleHitTarget {
    Agent,
    ModelProvider,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TitleHitRect {
    target: TitleHitTarget,
    rect: Rect,
}

/// A clickable copy affordance from the last draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyHitKind {
    Code,
    Message,
    Transcript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopyHitRegion {
    rect: Rect,
    text: Arc<str>,
    kind: CopyHitKind,
    group: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedCodeBlock {
    header_row: u16,
    block_end: u16,
    header_label: (u16, u16),
    footer_row: u16,
    footer_label: Option<(u16, u16)>,
    text: Arc<str>,
    group: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatContextMenuAction {
    SendNow,
    Copy,
    Edit,
    Delete,
}

const TRANSCRIPT_CONTEXT_ACTIONS: &[ChatContextMenuAction] = &[ChatContextMenuAction::Copy];
const QUEUE_CONTEXT_ACTIONS: &[ChatContextMenuAction] = &[
    ChatContextMenuAction::SendNow,
    ChatContextMenuAction::Copy,
    ChatContextMenuAction::Edit,
    ChatContextMenuAction::Delete,
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChatContextMenuTarget {
    Transcript(CopyHitRegion),
    Queue(u64),
}

impl ChatContextMenuTarget {
    fn actions(&self) -> &'static [ChatContextMenuAction] {
        match self {
            Self::Transcript(_) => TRANSCRIPT_CONTEXT_ACTIONS,
            Self::Queue(_) => QUEUE_CONTEXT_ACTIONS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatContextMenu {
    rect: Rect,
    target: ChatContextMenuTarget,
    selected: usize,
}

impl ChatContextMenu {
    fn selected_action(&self) -> Option<ChatContextMenuAction> {
        self.target.actions().get(self.selected).copied()
    }

    fn select_step(&mut self, delta: isize) {
        let count = self.target.actions().len();
        if count > 0 {
            self.selected = (self.selected as isize + delta).clamp(0, count as isize - 1) as usize;
        }
    }

    fn action_at(&self, column: u16, row: u16) -> Option<usize> {
        if self.rect.width <= 2 || self.rect.height <= 2 {
            return None;
        }
        let inner = Rect::new(
            self.rect.x + 1,
            self.rect.y + 1,
            self.rect.width - 2,
            self.rect.height - 2,
        );
        if !mouse::in_rect(column, row, inner) {
            return None;
        }
        let index = usize::from(row.saturating_sub(inner.y));
        (index < self.target.actions().len()).then_some(index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChatContextMenuRequest {
    CopyTranscript(CopyHitRegion),
    Queue {
        id: u64,
        action: ChatContextMenuAction,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyFeedbackTarget {
    Code(usize),
    Overlay(Rect),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CopyFeedback {
    target: CopyFeedbackTarget,
    shown_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueItemStatus {
    Pending,
    Injected,
}

#[derive(Debug, Clone)]
pub(crate) struct QueuedMessage {
    pub id: u64,
    pub text: String,
    pub attachments: Vec<PendingAttachment>,
    pub status: QueueItemStatus,
}

#[derive(Debug)]
pub struct ChatState {
    pub session_id: String,
    pub agent_alias: String,
    /// Durable projected conversation-entry count, not visible bubbles.
    pub message_count: usize,
    session_name: Option<String>,
    model_provider_ref: Option<String>,
    model: Option<String>,
    /// Working directory for this session (shown above input bar).
    pub cwd: Option<String>,
    /// Cached git branch for `cwd`, refreshed by the daemon on a polling
    /// interval (`GIT_BRANCH_REFRESH_INTERVAL`). `None` means either "not a
    /// git repo" or "not fetched yet".
    pub git_branch: Option<String>,
    /// First user message of the session, pulled from the persisted message
    /// store. Shown as a pinned recovery row at the top of the panel so the
    /// original ask stays visible across scroll and after a session reload.
    pub first_message: Option<String>,
    /// Cached short commit hash for `cwd`, refreshed alongside `git_branch`.
    /// `None` means "not a git repo", "unborn branch", or "not fetched yet".
    pub git_hash: Option<String>,
    /// Monotonic timestamp of the last completed `session/git_branch` reply,
    /// used to throttle re-fetches.
    pub git_branch_last_fetch: Option<Instant>,
    pub input_bar: InputBarState,
    /// Attachments owned by the currently dispatched turn. Once an input-bar
    /// submission leaves the composer, this is the sole owner until the turn
    /// reaches a terminal outcome.
    active_turn_attachments: Vec<PendingAttachment>,
    entries: Vec<ChatEntry>,
    streaming_text: String,
    streaming_thought: String,
    pending_approval: Option<PendingApproval>,
    pending_elicitation: Option<PendingElicitation>,
    /// Why the sidebar shows this session red. Set when a turn ends
    /// `Failed`; cleared by the next prompt, a completed turn, a session
    /// reset, or a successful re-attach.
    last_error: Option<SessionError>,
    pub turn_in_flight: bool,
    /// Monotonic local turn identity. Prompt responses use it to avoid
    /// settling a newer queued turn after the prior terminal notification.
    turn_generation: u64,
    /// Entry marker for the optimistic user row of the in-flight prompt.
    /// Prompt RPC errors may remove only this generation's row; transport
    /// closure and terminal notifications leave it intact.
    /// `(generation, entry index, prior durable count)` for the optimistic row.
    optimistic_user_message: Option<(u64, usize, usize)>,
    /// Set when any streaming text was flushed during the current turn.
    /// Used by `commit_turn` to decide whether `full_text` is a fallback
    /// (no streaming happened) or a duplicate (streaming already committed).
    turn_had_streaming_text: bool,
    /// Agent-message entry committed by prompt-response fallback while its
    /// terminal notification may still be in flight. Continuation chunks for
    /// the same local generation extend this entry instead of creating a
    /// second `Agent:` block.
    prompt_settled_stream_entry: Option<(u64, usize)>,
    /// Set when any `ToolCall` event arrived during the current turn.
    /// Used by `commit_turn` to distinguish "empty completion with tool
    /// calls" (normal — tool output is the visible record) from "empty
    /// completion with nothing at all" (needs a diagnostic row).
    turn_had_tool_calls: bool,
    /// Fine-grained label for the input-bar title while a turn is active.
    /// Lockstep with `turn_in_flight` (`Idle` ↔ `false`) but adds the
    /// thinking / responding / tool-call breakdown for the UI.
    pub turn_status: TurnStatus,
    /// Anchor for the dots animation — reset each time a turn begins so
    /// the pulse starts from phase 0.
    turn_started_at: Instant,
    show_thoughts: bool,
    /// Browse mode cursor (most-recently moved position).
    browse_cursor: Option<usize>,
    /// Anchor for range selection; set when Shift+↑/↓ is first pressed.
    /// Range is `min(anchor, cursor)..=max(anchor, cursor)`.
    browse_anchor: Option<usize>,
    /// Ctrl+click multi-select set, independent of cursor/anchor range.
    browse_multi: std::collections::BTreeSet<usize>,
    /// Entry index where mouse went down while browse mode is active. Used
    /// only to extend in-app range selection during a drag; mouse-up never
    /// copies implicitly.
    mouse_down_entry: Option<usize>,
    /// Visible transcript cells from the last draw. Character-level selection
    /// uses this exact rendered grid so Markdown wrapping has one source of truth.
    transcript_snapshot: Option<TranscriptSnapshot>,
    /// Normal-mode character selection within `transcript_snapshot`.
    transcript_selection: Option<TranscriptSelection>,
    /// Per-entry hit rects from the last draw.
    entry_rects: Vec<(usize, ratatui::layout::Rect)>,
    /// Visible tool-header hit rects from the last draw.
    tool_header_rects: Vec<(usize, ratatui::layout::Rect)>,
    /// Visible file-tool footer hit rects from the last draw.
    tool_footer_rects: Vec<(usize, ratatui::layout::Rect)>,
    /// Per-tool disclosure overrides; file tools otherwise default to preview.
    tool_disclosures: BTreeMap<Arc<str>, ToolDisclosure>,
    /// Clickable `[Copy]` labels from the last draw.
    copy_hit_regions: Vec<CopyHitRegion>,
    /// Full code-block targets used by right-click context-menu resolution.
    context_copy_regions: Vec<CopyHitRegion>,
    /// Active transcript or queue context menu.
    context_menu: Option<ChatContextMenu>,
    /// Temporary `[Copied]` overlay for copy labels.
    copy_feedback: Option<CopyFeedback>,
    /// Clickable provider/model title spans from the last draw.
    title_hit_rects: Vec<TitleHitRect>,
    /// Scrollbar track rect from the last draw.
    scrollbar_track_rect: Option<ratatui::layout::Rect>,
    /// Active scrollbar drag anchor.
    scrollbar_drag: Option<ScrollbarDrag>,
    session_overlay: SessionOverlay,
    scroll_offset: u16,
    pinned_to_bottom: bool,
    last_total_rows: u16,
    last_inner_height: u16,
    /// Cached rendered lines from committed entries.
    cached_lines: Vec<Line<'static>>,
    /// Source-derived separator before each wrapped screen row in `cached_lines`.
    cached_row_breaks: Vec<TranscriptRowBreak>,
    /// Per-entry unwrapped-line ranges in `cached_lines` — `(entry_idx,
    /// start, end_exclusive)`. Used by mouse hit-testing.
    cached_line_ranges: Vec<(usize, usize, usize)>,
    /// Per-entry disclosure footer line indices in `cached_lines`.
    cached_tool_footer_lines: BTreeMap<usize, usize>,
    /// Per-line wrapped screen-row spans derived from `cached_lines` at
    /// `cached_render_width`. This is the line-level index for viewport
    /// slicing; it is rebuilt atomically with the rendered-line cache.
    cached_line_screen_ranges: Vec<(u16, u16)>,
    /// Per-entry screen-row ranges: `(entry_idx, screen_start, screen_end,
    /// content_width)`. Unlike `cached_line_ranges` (unwrapped line indices),
    /// these account for markdown wrapping so mouse hit-testing (`entry_rects`)
    /// lands on the correct screen rows for agent messages, code blocks, and
    /// tables. `content_width` is the widest rendered column extent of the
    /// entry (clamped to the viewport), so hit-testing ignores the blank space
    /// beside short messages — a click there dismisses the highlight instead of
    /// re-selecting the entry.
    cached_screen_ranges: Vec<(usize, u16, u16, u16)>,
    /// Copy projections derived from fenced regions in `cached_lines`.
    /// Keeping their full copy text in the render cache avoids rescanning a
    /// large visible fence on every steady-state draw.
    cached_code_blocks: Vec<CachedCodeBlock>,
    /// Fine-grained dirty tracking — see [`LinesDirty`].
    dirty: LinesDirty,
    /// How many entries from `entries[cached_render_start..]` are represented in
    /// `cached_lines`.  Valid only when `dirty != Full`.
    cached_entry_count: usize,
    /// The `entries` index where the render window starts for the current cache.
    cached_render_start: usize,
    /// The render width the current `cached_lines` were laid out for.
    /// A width change forces a full rebuild because tables compute their
    /// column budgets from it.
    cached_render_width: u16,
    cached_total_rows: u16,
    /// Cumulative token count for this session: every Usage event from the
    /// provider (input + cached + output) is added on arrival. Cleared on
    /// session reset only.
    pub context_input_tokens: Option<u64>,
    /// Preemptive-trim budget for this session (the bar fills toward this).
    pub context_max_tokens: Option<u64>,
    /// Model's full context window; when present, the bar denominator so the
    /// trim budget shows as a marker rather than the 100% point.
    pub context_model_window: Option<u64>,
    /// Outbound message queue; the front dispatches when the session is free.
    message_queue: VecDeque<QueuedMessage>,
    /// Monotonic id source for queued messages.
    next_queue_id: u64,
    /// Set on Cancel/Fail; freezes auto-dispatch until the user resumes.
    queue_paused: bool,
    resume_override: bool,
    cancel_started_at: Option<Instant>,
    queue_sidebar_cols: u16,
    /// Selected queued message id for sidebar edit/delete.
    queue_sel: Option<u64>,
    /// Per-item clickable rects from the last sidebar draw, mapping a queued
    /// message id to its header-row rect. Drives left-click selection.
    queue_item_rects: Vec<(u64, ratatui::layout::Rect)>,
    /// Inner sidebar rect from the last draw, for scroll-wheel hit-testing.
    queue_sidebar_rect: Option<ratatui::layout::Rect>,
    /// Scroll offset (in rendered rows) into the queue sidebar.
    queue_scroll: u16,
    /// Latest info-bar message (queue/attach notices, model-switch op notes,
    /// errors). `None` hides the bar. Auto-cleared in the tick loop once
    /// [`crate::widgets::INFO_BAR_TTL`] elapses.
    pub info_message: Option<crate::widgets::InfoMessage>,
    /// Active model / model_provider picker overlay.
    model_picker: ModelPickerOverlay,
    /// Exact close-cell target from the last Todo panel draw.
    todo_close_hit_rect: Option<ratatui::layout::Rect>,
    /// Live TodoWrite tracker panel for this session. Read-only; fed by
    /// `SessionUpdate::Plan`, toggled by the user, laid out per config.
    todo_tracker: crate::todo_tracker::TodoTracker,
}

impl ChatState {
    #[cfg(test)]
    pub fn new(
        session_id: String,
        agent_alias: String,
        todo_settings: crate::todo_tracker::TodoTrackerSettings,
    ) -> Self {
        Self::with_shared_commands(session_id, agent_alias, todo_settings, &[])
    }

    fn with_shared_commands(
        session_id: String,
        agent_alias: String,
        todo_settings: crate::todo_tracker::TodoTrackerSettings,
        commands: &[crate::wire::CommandDescriptor],
    ) -> Self {
        Self {
            session_id,
            agent_alias,
            message_count: 0,
            session_name: None,
            model_provider_ref: None,
            model: None,
            cwd: None,
            git_branch: None,
            first_message: None,
            git_hash: None,
            git_branch_last_fetch: None,
            input_bar: InputBarState::with_shared_commands(commands),
            active_turn_attachments: Vec::new(),
            entries: Vec::new(),
            streaming_text: String::new(),
            streaming_thought: String::new(),
            pending_approval: None,
            pending_elicitation: None,
            last_error: None,
            turn_in_flight: false,
            turn_generation: 0,
            optimistic_user_message: None,
            turn_had_streaming_text: false,
            prompt_settled_stream_entry: None,
            turn_had_tool_calls: false,
            turn_status: TurnStatus::Idle,
            turn_started_at: Instant::now(),
            show_thoughts: true,
            browse_cursor: None,
            browse_anchor: None,
            browse_multi: std::collections::BTreeSet::new(),
            mouse_down_entry: None,
            transcript_snapshot: None,
            transcript_selection: None,
            entry_rects: Vec::new(),
            tool_header_rects: Vec::new(),
            tool_footer_rects: Vec::new(),
            tool_disclosures: BTreeMap::new(),
            copy_hit_regions: Vec::new(),
            context_copy_regions: Vec::new(),
            context_menu: None,
            copy_feedback: None,
            title_hit_rects: Vec::new(),
            scrollbar_track_rect: None,
            scrollbar_drag: None,
            session_overlay: SessionOverlay::None,
            scroll_offset: 0,
            pinned_to_bottom: true,
            last_total_rows: 0,
            last_inner_height: 0,
            cached_lines: Vec::new(),
            cached_row_breaks: Vec::new(),
            cached_line_ranges: Vec::new(),
            cached_tool_footer_lines: BTreeMap::new(),
            cached_line_screen_ranges: Vec::new(),
            cached_screen_ranges: Vec::new(),
            cached_code_blocks: Vec::new(),
            dirty: LinesDirty::Full,
            cached_entry_count: 0,
            cached_render_start: 0,
            cached_render_width: 0,
            cached_total_rows: 0,
            context_input_tokens: None,
            context_max_tokens: None,
            context_model_window: None,
            message_queue: VecDeque::new(),
            next_queue_id: 0,
            queue_paused: false,
            resume_override: false,
            cancel_started_at: None,
            queue_sidebar_cols: 36,
            queue_sel: None,
            queue_item_rects: Vec::new(),
            queue_sidebar_rect: None,
            queue_scroll: 0,
            info_message: None,
            model_picker: ModelPickerOverlay::None,
            todo_close_hit_rect: None,
            todo_tracker: crate::todo_tracker::TodoTracker::from_settings(todo_settings),
        }
    }

    fn mark_dirty_append(&mut self) {
        match self.dirty {
            LinesDirty::Clean => self.dirty = LinesDirty::Appended,
            LinesDirty::TailChanged(_) => self.dirty = LinesDirty::Full,
            LinesDirty::Appended | LinesDirty::Full => {}
        }
        // Full is sticky — don't downgrade.
    }

    fn mark_dirty_tail(&mut self, entry_index: usize) {
        match self.dirty {
            LinesDirty::Clean => self.dirty = LinesDirty::TailChanged(entry_index),
            LinesDirty::TailChanged(index) if index == entry_index => {}
            LinesDirty::Appended => {
                // An intervening append does not make previously cached text fresh.
                if (self.cached_render_start
                    ..self
                        .cached_render_start
                        .saturating_add(self.cached_entry_count))
                    .contains(&entry_index)
                {
                    self.dirty = LinesDirty::Full;
                }
            }
            LinesDirty::TailChanged(_) | LinesDirty::Full => self.dirty = LinesDirty::Full,
        }
    }

    /// Whether text input currently belongs to the composer rather than a
    /// modal, picker, explorer, or transcript-browse surface.
    fn composer_owns_text_input(&self) -> bool {
        !self.model_picker.is_open()
            && self.pending_elicitation().is_none()
            && self.pending_approval().is_none()
            && matches!(self.session_overlay, SessionOverlay::None)
            && self.context_menu.is_none()
            && !self.input_bar.has_file_explorer()
            && !self.input_bar.has_attachment_manager()
            && !self.in_browse_mode()
    }

    /// Route a key to an input-bar-owned overlay and retain its user feedback.
    /// Returns true only when that overlay consumed the key before pane-level
    /// shortcuts get a chance to process it.
    fn handle_input_bar_overlay_key(&mut self, key: KeyEvent) -> bool {
        if self.pending_approval().is_some()
            || (!self.input_bar.has_file_explorer() && !self.input_bar.has_attachment_manager())
        {
            return false;
        }

        self.clear_mouse_highlight();
        let action = self.input_bar.handle_key(key);
        let cleanup_notice = self.input_bar.take_cleanup_report().notice();
        // Explorer confirmation can reject an attachment (for example a file
        // over the size limit). Preserve that feedback instead of swallowing it.
        if let InputBarAction::StatusMessage(message) = action {
            self.set_info_notice(append_cleanup_notice(message, cleanup_notice));
        } else if let Some(message) = cleanup_notice {
            self.set_info_notice(message);
        }
        self.mark_dirty_full();
        true
    }

    fn mark_dirty_full(&mut self) {
        self.dirty = LinesDirty::Full;
    }

    fn clear_transcript_selection(&mut self) {
        self.transcript_selection = None;
        self.copy_hit_regions.clear();
        self.context_copy_regions.clear();
        self.context_menu = None;
        self.copy_feedback = None;
    }

    fn begin_transcript_drag(&mut self, column: u16, row: u16) -> bool {
        let Some(snapshot) = &self.transcript_snapshot else {
            return false;
        };
        let Some(point) = snapshot.point_at(column, row) else {
            self.clear_transcript_selection();
            return false;
        };
        if snapshot.row_text_bounds(point.row).is_none() {
            self.clear_transcript_selection();
            return false;
        }

        self.copy_feedback = None;
        self.transcript_selection = Some(TranscriptSelection {
            anchor: point,
            head: point,
            dragged: false,
        });
        true
    }

    fn update_transcript_drag(&mut self, column: u16, row: u16) -> bool {
        let Some(anchor) = self.transcript_selection.map(|selection| selection.anchor) else {
            return false;
        };
        let Some(head) = self
            .transcript_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.point_at(column, row))
        else {
            return false;
        };

        self.transcript_selection = Some(TranscriptSelection {
            anchor,
            head,
            dragged: head != anchor,
        });
        true
    }

    fn finish_transcript_drag(&mut self) {
        if self
            .transcript_selection
            .is_some_and(|selection| !selection.dragged)
        {
            self.transcript_selection = None;
        }
    }

    fn transcript_selected_text(&self) -> Option<String> {
        self.transcript_snapshot
            .as_ref()?
            .selected_text(self.transcript_selection?)
    }

    fn set_transcript_snapshot(&mut self, snapshot: TranscriptSnapshot) {
        if self
            .transcript_snapshot
            .as_ref()
            .is_some_and(|current| current != &snapshot)
        {
            self.clear_transcript_selection();
        }
        self.transcript_snapshot = Some(snapshot);
    }

    fn clear_mouse_highlight(&mut self) {
        self.mouse_down_entry = None;
        self.clear_transcript_selection();
    }

    fn clear_browse_selection(&mut self) {
        let lines_changed = self.mouse_down_entry.is_some()
            || self.browse_cursor.is_some()
            || self.browse_anchor.is_some()
            || !self.browse_multi.is_empty();
        if lines_changed || self.context_menu.is_some() || self.copy_feedback.is_some() {
            self.mouse_down_entry = None;
            self.browse_cursor = None;
            self.browse_anchor = None;
            self.browse_multi.clear();
            self.context_menu = None;
            self.copy_feedback = None;
        }
        if lines_changed {
            self.mark_dirty_full();
        }
    }

    // ── Browse-mode helpers ───────────────────────────────────────

    /// True when browse mode is active (cursor is set).
    fn in_browse_mode(&self) -> bool {
        self.browse_cursor.is_some()
    }

    fn copy_current_selection(&mut self) -> bool {
        let text = self.current_selection_text();
        let feedback_anchor = self
            .copy_hit_regions
            .iter()
            .find(|region| matches!(region.kind, CopyHitKind::Message | CopyHitKind::Transcript))
            .map(|region| region.rect)
            .or_else(|| {
                self.transcript_snapshot
                    .as_ref()?
                    .selection_anchor_rect(self.transcript_selection?)
            });
        if !self.copy_text_and_clear_selection(&text) {
            return false;
        }
        if let Some(anchor) = feedback_anchor {
            self.set_overlay_copy_feedback(anchor);
        }
        true
    }

    fn copy_text_and_clear_selection(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        crate::mouse::copy_osc52(text);
        self.clear_mouse_highlight();
        self.clear_browse_selection();
        self.set_info_notice(crate::i18n::t("zc-chat-copied-clipboard"));
        true
    }

    fn current_selection_text(&self) -> String {
        if self.transcript_selection.is_some() {
            return self.transcript_selected_text().unwrap_or_default();
        }
        self.yank_selection()
    }

    fn dismiss_context_menu(&mut self) {
        self.context_menu = None;
    }

    fn open_transcript_context_menu(&mut self, column: u16, row: u16) -> bool {
        let Some(bounds) = self
            .transcript_snapshot
            .as_ref()
            .map(|snapshot| snapshot.area)
        else {
            return false;
        };
        if !mouse::in_rect(column, row, bounds) {
            return false;
        }

        let selected_target = match self.transcript_selection {
            Some(selection) => {
                let Some(snapshot) = self.transcript_snapshot.as_ref() else {
                    return false;
                };
                let Some(text) = snapshot.selected_text(selection) else {
                    return false;
                };
                let Some(rect) = snapshot.selection_anchor_rect(selection) else {
                    return false;
                };
                Some(CopyHitRegion {
                    rect,
                    text: text.into(),
                    kind: CopyHitKind::Transcript,
                    group: 0,
                })
            }
            None => None,
        };

        // Code blocks are nested inside message rows, so resolve their more
        // specific target before falling back to the containing message.
        let target = selected_target.or_else(|| {
            self.context_copy_regions
                .iter()
                .find(|region| mouse::in_rect(column, row, region.rect))
                .cloned()
                .or_else(|| {
                    self.entry_rects
                        .iter()
                        .find(|(_, rect)| row >= rect.y && row < rect.y.saturating_add(rect.height))
                        .and_then(|(idx, rect)| {
                            let text = self.yank_single_entry(*idx);
                            (!text.is_empty()).then_some(CopyHitRegion {
                                rect: *rect,
                                text: text.into(),
                                kind: CopyHitKind::Message,
                                group: *idx,
                            })
                        })
                })
        });
        let Some(target) = target else {
            return false;
        };
        let target = ChatContextMenuTarget::Transcript(target);
        let Some(rect) = context_menu_rect(column, row, bounds, target.actions()) else {
            return false;
        };
        self.context_menu = Some(ChatContextMenu {
            rect,
            target,
            selected: 0,
        });
        true
    }

    fn open_queue_context_menu(&mut self, column: u16, row: u16) -> bool {
        let id = self
            .queue_item_rects
            .iter()
            .find(|(_, rect)| mouse::in_rect(column, row, *rect))
            .map(|(id, _)| *id);
        let (Some(id), Some(bounds)) = (id, self.queue_sidebar_rect) else {
            return false;
        };
        self.select_queued_by_id(id);
        let target = ChatContextMenuTarget::Queue(id);
        let Some(rect) = context_menu_rect(column, row, bounds, target.actions()) else {
            return false;
        };
        self.context_menu = Some(ChatContextMenu {
            rect,
            target,
            selected: 0,
        });
        self.mark_dirty_full();
        true
    }

    fn context_menu_select_step(&mut self, delta: isize) {
        if let Some(menu) = self.context_menu.as_mut() {
            menu.select_step(delta);
            self.mark_dirty_full();
        }
    }

    fn context_menu_select_at(&mut self, column: u16, row: u16) -> bool {
        let Some(menu) = self.context_menu.as_mut() else {
            return false;
        };
        let Some(index) = menu.action_at(column, row) else {
            return false;
        };
        menu.selected = index;
        true
    }

    fn take_context_menu_request(&mut self) -> Option<ChatContextMenuRequest> {
        let menu = self.context_menu.take()?;
        let action = menu.selected_action()?;
        match (menu.target, action) {
            (ChatContextMenuTarget::Transcript(target), ChatContextMenuAction::Copy) => {
                Some(ChatContextMenuRequest::CopyTranscript(target))
            }
            (ChatContextMenuTarget::Queue(id), action) => {
                Some(ChatContextMenuRequest::Queue { id, action })
            }
            (ChatContextMenuTarget::Transcript(_), _) => None,
        }
    }

    fn toggle_tool_header_at(&mut self, column: u16, row: u16) -> bool {
        let Some(entry_idx) = self
            .tool_header_rects
            .iter()
            .find(|(_, rect)| mouse::in_rect(column, row, *rect))
            .map(|(idx, _)| *idx)
        else {
            return false;
        };
        let Some(ChatEntry::Tool {
            tool_call_id,
            name,
            input_json,
            ..
        }) = self.entries.get(entry_idx)
        else {
            return false;
        };
        let tool_call_id = Arc::clone(tool_call_id);
        let current = self
            .tool_disclosures
            .get(&tool_call_id)
            .copied()
            .unwrap_or_else(|| default_tool_disclosure(name, input_json));
        let next = match current {
            ToolDisclosure::Collapsed => match default_tool_disclosure(name, input_json) {
                ToolDisclosure::Preview => ToolDisclosure::Preview,
                ToolDisclosure::Collapsed | ToolDisclosure::Full => ToolDisclosure::Full,
            },
            ToolDisclosure::Preview | ToolDisclosure::Full => ToolDisclosure::Collapsed,
        };
        self.tool_disclosures.insert(tool_call_id, next);
        self.clear_transcript_selection();
        self.mark_dirty_full();
        true
    }

    fn disclosure_for_entry(&self, entry: &ChatEntry) -> ToolDisclosure {
        let ChatEntry::Tool {
            tool_call_id,
            name,
            input_json,
            ..
        } = entry
        else {
            return ToolDisclosure::Collapsed;
        };
        self.tool_disclosures
            .get(tool_call_id)
            .copied()
            .unwrap_or_else(|| default_tool_disclosure(name, input_json))
    }

    fn toggle_tool_footer_at(&mut self, column: u16, row: u16) -> bool {
        let Some(entry_idx) = self
            .tool_footer_rects
            .iter()
            .find(|(_, rect)| mouse::in_rect(column, row, *rect))
            .map(|(idx, _)| *idx)
        else {
            return false;
        };
        let Some(ChatEntry::Tool { tool_call_id, .. }) = self.entries.get(entry_idx) else {
            return false;
        };
        let tool_call_id = Arc::clone(tool_call_id);
        let current = self
            .tool_disclosures
            .get(&tool_call_id)
            .copied()
            .unwrap_or(ToolDisclosure::Preview);
        let next = if matches!(current, ToolDisclosure::Full) {
            ToolDisclosure::Preview
        } else {
            ToolDisclosure::Full
        };
        self.tool_disclosures.insert(tool_call_id, next);
        self.clear_transcript_selection();
        self.mark_dirty_full();
        true
    }

    /// Yank a single entry's body text for explicit copy actions.
    fn yank_single_entry(&self, idx: usize) -> String {
        self.entries
            .get(idx)
            .map(clipboard_text)
            .unwrap_or_default()
    }

    /// Build the clipboard string. Single = body. Multi = role-prefixed.
    fn yank_selection(&self) -> String {
        let sel = self.selected_entries();
        let count = sel.len();
        if count == 0 {
            return String::new();
        }
        let with_label = count > 1;
        sel.into_iter()
            .filter_map(|i| self.entries.get(i))
            .map(|e| {
                if with_label {
                    labelled_clipboard_text(e)
                } else {
                    clipboard_text(e)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Enter browse mode: jump cursor to last entry, clear anchor.
    fn enter_browse_mode(&mut self) {
        self.clear_transcript_selection();
        if !self.entries.is_empty() {
            self.browse_cursor = Some(self.entries.len() - 1);
            self.browse_anchor = None;
            self.mark_dirty_full();
        }
    }

    /// Leave browse mode: clear both cursor and anchor, return to input.
    fn exit_browse_mode(&mut self) {
        self.browse_cursor = None;
        self.mouse_down_entry = None;
        self.browse_anchor = None;
        self.copy_hit_regions.clear();
        self.context_copy_regions.clear();
        self.context_menu = None;
        self.copy_feedback = None;
        self.mark_dirty_full();
    }

    /// Move the cursor up by `n` entries (older messages).  Clamps at 0.
    /// If `extend` is true, sets/keeps the anchor for range selection.
    /// Scrolls so the cursor entry is at the top of the viewport.
    fn browse_move_up(&mut self, n: usize, extend: bool) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let cur = self.browse_cursor.unwrap_or(len - 1);
        if extend && self.browse_anchor.is_none() {
            self.browse_anchor = Some(cur);
        } else if !extend {
            self.browse_anchor = None;
        }
        let next = cur.saturating_sub(n);
        self.browse_cursor = Some(next);
        self.scroll_entry_into_view(next);
        self.pinned_to_bottom = false;
        self.mark_dirty_full();
    }

    /// Move the cursor down by `n` entries (newer messages).  Clamps at last entry.
    /// If `extend` is true, sets/keeps the anchor for range selection.
    /// Scrolls so the cursor entry is at the top of the viewport.
    fn browse_move_down(&mut self, n: usize, extend: bool) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let cur = self.browse_cursor.unwrap_or(0);
        if extend && self.browse_anchor.is_none() {
            self.browse_anchor = Some(cur);
        } else if !extend {
            self.browse_anchor = None;
        }
        let next = cur.saturating_add(n).min(len - 1);
        self.browse_cursor = Some(next);
        self.scroll_entry_into_view(next);
        self.pinned_to_bottom =
            self.scroll_offset >= self.last_total_rows.saturating_sub(self.last_inner_height);
        self.mark_dirty_full();
    }

    /// Adjust `scroll_offset` so the entry at `entry_idx` is visible at the
    /// top of the viewport. If the entry is taller than the viewport, its
    /// top is shown.  Does nothing when `cached_screen_ranges` is empty
    /// (pre-render path).
    fn scroll_entry_into_view(&mut self, entry_idx: usize) {
        let Some(&(_, lo, _hi, _)) = self
            .cached_screen_ranges
            .iter()
            .find(|(idx, _, _, _)| *idx == entry_idx)
        else {
            return;
        };
        let inner_h = self.last_inner_height;
        if inner_h == 0 {
            return;
        }
        let total = self.last_total_rows;
        let max = total.saturating_sub(inner_h);

        // Align the entry's top with the viewport top.
        self.scroll_offset = lo.min(max);
    }

    /// The selected range as `(lo, hi)` indices, inclusive.
    /// Returns `None` when not in browse mode.
    fn browse_range(&self) -> Option<(usize, usize)> {
        let cur = self.browse_cursor?;
        let anchor = self.browse_anchor.unwrap_or(cur);
        let lo = cur.min(anchor);
        let hi = cur.max(anchor);
        Some((lo, hi))
    }

    /// True when `idx` falls inside the current browse selection range.
    fn is_in_browse_range(&self, idx: usize) -> bool {
        self.browse_range()
            .is_some_and(|(lo, hi)| idx >= lo && idx <= hi)
    }

    /// True when `idx` should render highlighted in browse mode.
    fn is_entry_highlighted(&self, idx: usize) -> bool {
        if self.browse_multi.contains(&idx) {
            return true;
        }
        if self.is_in_browse_range(idx) {
            return true;
        }
        self.browse_cursor == Some(idx)
    }

    /// Total selection: multi-select set ∪ browse range ∪ lone cursor.
    fn selected_entries(&self) -> std::collections::BTreeSet<usize> {
        let mut out = self.browse_multi.clone();
        if let Some((lo, hi)) = self.browse_range() {
            for i in lo..=hi {
                out.insert(i);
            }
        } else if let Some(c) = self.browse_cursor {
            out.insert(c);
        }
        out
    }

    fn rebuild_lines(&mut self, width: u16) {
        if self.cached_render_width != width {
            self.dirty = LinesDirty::Full;
            self.cached_render_width = width;
        }
        let total = self.entries.len();
        let natural_start = total.saturating_sub(MAX_RENDERED_ENTRIES);
        let mut start = if self.pinned_to_bottom || self.cached_render_width == 0 {
            natural_start
        } else {
            self.cached_render_start.min(natural_start)
        };
        if let Some(cursor) = self.browse_cursor {
            if cursor < start {
                start = cursor;
            } else if cursor >= start.saturating_add(MAX_RENDERED_ENTRIES) {
                start = cursor
                    .saturating_add(1)
                    .saturating_sub(MAX_RENDERED_ENTRIES);
            }
        }
        start = start.min(natural_start);
        let end = start.saturating_add(MAX_RENDERED_ENTRIES).min(total);

        // A prompt-response fallback may commit the current stream just before
        // its final chunks arrive. Re-render only that final entry: earlier
        // markdown and row metadata remain valid.
        if let LinesDirty::TailChanged(entry_index) = self.dirty
            && start == self.cached_render_start
            && entry_index + 1 == end
            && let Some(range_pos) = self
                .cached_line_ranges
                .iter()
                .position(|&(index, _, _)| index == entry_index)
            && range_pos + 1 == self.cached_line_ranges.len()
        {
            let line_start = self.cached_line_ranges[range_pos].1;
            self.cached_lines.truncate(line_start);
            self.cached_line_ranges.truncate(range_pos);

            let mut changed_lines = Vec::new();
            let footer_line = render_entry_into(
                &self.entries[entry_index],
                self.is_entry_highlighted(entry_index),
                self.show_thoughts,
                self.disclosure_for_entry(&self.entries[entry_index]),
                width,
                &mut changed_lines,
            );
            self.cached_tool_footer_lines.remove(&entry_index);
            if let Some(footer_line) = footer_line {
                self.cached_tool_footer_lines
                    .insert(entry_index, line_start + footer_line);
            }
            let line_end = line_start + changed_lines.len();
            self.cached_lines.extend(changed_lines);
            self.cached_line_ranges
                .push((entry_index, line_start, line_end));
            self.cached_row_breaks = row_breaks_for_lines(&self.cached_lines, width);
            self.dirty = LinesDirty::Clean;
            self.rebuild_screen_ranges(width);
            return;
        }

        // Incremental append path.
        if self.dirty == LinesDirty::Appended && start == self.cached_render_start {
            let render_from = start + self.cached_entry_count;
            let show_thoughts = self.show_thoughts;
            let mut new_lines = Vec::new();
            let mut new_ranges = Vec::new();
            for (rel_idx, entry) in self.entries[render_from..end].iter().enumerate() {
                let abs_idx = render_from + rel_idx;
                let before = new_lines.len();
                let disclosure = self.disclosure_for_entry(entry);
                let footer_line = render_entry_into(
                    entry,
                    self.is_entry_highlighted(abs_idx),
                    show_thoughts,
                    disclosure,
                    width,
                    &mut new_lines,
                );
                let after = new_lines.len();
                if after > before {
                    let base = self.cached_lines.len();
                    new_ranges.push((abs_idx, base + before, base + after));
                }
                if let Some(footer_line) = footer_line {
                    self.cached_tool_footer_lines
                        .insert(abs_idx, self.cached_lines.len() + footer_line);
                }
            }
            self.cached_row_breaks
                .extend(row_breaks_for_lines(&new_lines, width));
            self.cached_lines.extend(new_lines);
            self.cached_line_ranges.extend(new_ranges);
            self.cached_entry_count = end - start;
            self.dirty = LinesDirty::Clean;
            self.rebuild_screen_ranges(width);
            return;
        }

        // Full rebuild path.
        let mut lines = Vec::new();
        let mut ranges = Vec::new();
        let mut footer_lines = BTreeMap::new();
        let show_thoughts = self.show_thoughts;
        for (rel_idx, entry) in self.entries[start..end].iter().enumerate() {
            let abs_idx = start + rel_idx;
            let before = lines.len();
            let disclosure = self.disclosure_for_entry(entry);
            let footer_line = render_entry_into(
                entry,
                self.is_entry_highlighted(abs_idx),
                show_thoughts,
                disclosure,
                width,
                &mut lines,
            );
            let after = lines.len();
            if after > before {
                ranges.push((abs_idx, before, after));
            }
            if let Some(footer_line) = footer_line {
                footer_lines.insert(abs_idx, footer_line);
            }
        }
        self.cached_row_breaks = row_breaks_for_lines(&lines, width);
        self.cached_lines = lines;
        self.cached_line_ranges = ranges;
        self.cached_tool_footer_lines = footer_lines;
        self.cached_entry_count = end - start;
        self.cached_render_start = start;
        self.dirty = LinesDirty::Clean;
        self.rebuild_screen_ranges(width);
    }

    /// Resolve the ordered cached-entry indices whose screen rows overlap the
    /// viewport.
    fn visible_cached_entry_range(&self, scroll: u16, height: u16) -> Range<usize> {
        if height == 0 {
            return 0..0;
        }
        let view_end = scroll.saturating_add(height);
        let first = self
            .cached_screen_ranges
            .partition_point(|&(_, _, screen_hi, _)| screen_hi <= scroll);
        let end = self
            .cached_screen_ranges
            .partition_point(|&(_, screen_lo, _, _)| screen_lo < view_end);
        first.min(end)..end
    }

    /// Resolve the complete cached viewport once at entry and line granularity.
    /// Both indexes are generated from `cached_lines` during cache rebuilds;
    /// steady-state draws only perform ordered lookups plus visible work.
    fn visible_cached_window(&self, scroll: u16, height: u16) -> VisibleCachedWindow {
        let entries = self.visible_cached_entry_range(scroll, height);
        if height == 0 {
            return VisibleCachedWindow {
                entries,
                lines: 0..0,
                screen_lo: 0,
            };
        }

        let view_end = scroll.saturating_add(height);
        let first = self
            .cached_line_screen_ranges
            .partition_point(|&(_, screen_hi)| screen_hi <= scroll);
        let end = self
            .cached_line_screen_ranges
            .partition_point(|&(screen_lo, _)| screen_lo < view_end);
        let lines = first.min(end)..end;
        let screen_lo = self
            .cached_line_screen_ranges
            .get(lines.start)
            .map_or(0, |&(screen_lo, _)| screen_lo);
        VisibleCachedWindow {
            entries,
            lines,
            screen_lo,
        }
    }

    fn visible_line_slice(
        &self,
        scroll: u16,
        window: &VisibleCachedWindow,
    ) -> (Vec<Line<'static>>, u16) {
        if window.lines.is_empty() {
            return (Vec::new(), 0);
        }
        let local_scroll = scroll.saturating_sub(window.screen_lo);
        (
            self.cached_lines[window.lines.clone()].to_vec(),
            local_scroll,
        )
    }

    /// Builds the transient overlay lines — the live streaming text (with
    /// its agent label), the thinking line, and the approval padding rows —
    /// exactly as they are appended below the committed history on transient
    /// frames. Rebuilt fresh every frame by design; never cached.
    fn build_overlay_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        if !self.streaming_text.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("{} ", crate::i18n::t("zc-chat-label-agent")),
                theme::agent_label_style(),
            )]));
            lines.extend(markdown_to_lines(&self.streaming_text, width));
        }
        if self.show_thoughts && !self.streaming_thought.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("(thinking) ", theme::thought_style()),
                Span::styled(self.streaming_thought.clone(), theme::dim_style()),
            ]));
        }
        if self.pending_approval.is_some() {
            for _ in 0..APPROVAL_OVERLAY_HEIGHT {
                lines.push(Line::default());
            }
        }
        lines
    }

    /// Like `visible_line_slice`, but the virtual buffer is cached history
    /// rows (`0..cached_total_rows`) followed by `overlay` rows — the
    /// transient streaming/thinking/approval lines, which are rebuilt fresh
    /// every frame and never cached. Slices the history the same bounded way
    /// `visible_line_slice` does, and appends the (small) overlay in full
    /// once the viewport window reaches it, letting the `Paragraph`'s local
    /// scroll handle any partial visibility.
    fn visible_transient_slice(
        &self,
        scroll: u16,
        height: u16,
        window: &VisibleCachedWindow,
        overlay: Vec<Line<'static>>,
    ) -> (Vec<Line<'static>>, u16) {
        let cached_total_rows = self.cached_total_rows;
        if scroll >= cached_total_rows {
            // Window is entirely within the overlay (includes the empty
            // history case, where cached_total_rows is 0).
            return (overlay, scroll - cached_total_rows);
        }
        let (mut lines, local_scroll) = self.visible_line_slice(scroll, window);
        if scroll.saturating_add(height) > cached_total_rows {
            lines.extend(overlay);
        }
        (lines, local_scroll)
    }

    /// Recompute every screen-space index derived from `cached_lines`.
    /// Cache rebuilds may remain history-sized; steady-state frames use these
    /// ordered indexes without rescanning committed entries or lines.
    fn rebuild_screen_ranges(&mut self, width: u16) {
        self.cached_line_screen_ranges.clear();
        self.cached_screen_ranges.clear();
        self.cached_code_blocks.clear();
        let mut screen_cursor = 0u16;
        let mut pending_fence: Option<(u16, u16, u16, usize, Option<String>, String)> = None;

        for line in &self.cached_lines {
            let line_start = screen_cursor;
            screen_cursor = screen_cursor.saturating_add(wrapped_rows(line, width));
            self.cached_line_screen_ranges
                .push((line_start, screen_cursor));

            let first = line.spans.first().map(|s| s.content.as_ref()).unwrap_or("");
            if first.starts_with('\u{250c}') {
                let lang = header_fence_lang(line);
                pending_fence = label_cells(line, " [Copy] ").map(|(col, cells)| {
                    (
                        line_start,
                        col,
                        cells,
                        line_start as usize,
                        lang,
                        String::new(),
                    )
                });
            } else if first.starts_with('\u{2514}') {
                if let Some((header_row, header_col, header_cells, group, lang, body)) =
                    pending_fence.take()
                {
                    self.cached_code_blocks.push(CachedCodeBlock {
                        header_row,
                        block_end: screen_cursor,
                        header_label: (header_col, header_cells),
                        footer_row: line_start,
                        footer_label: label_cells(line, " [Copy] "),
                        text: Arc::<str>::from(fenced_text(lang.as_deref(), &body)),
                        group,
                    });
                }
            } else if let Some((_, _, _, _, _, body)) = pending_fence.as_mut() {
                let full: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                let body_text = full.strip_prefix("  ").unwrap_or(&full);
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(body_text);
            }
        }

        self.cached_total_rows = screen_cursor;
        for &(entry_idx, lo, hi) in &self.cached_line_ranges {
            if lo >= hi {
                continue;
            }
            // Widest rendered column extent of the entry, clamped to the
            // viewport. Lines wider than `width` wrap to full-width rows, so the
            // clamp yields the true on-screen extent. Hit-testing uses this so
            // the blank space beside a short message is treated as outside the
            // entry.
            let content_width = self.cached_lines[lo..hi]
                .iter()
                .map(|l| l.width() as u16)
                .max()
                .unwrap_or(0)
                .min(width);
            let Some(&(screen_lo, _)) = self.cached_line_screen_ranges.get(lo) else {
                continue;
            };
            let Some(&(_, screen_hi)) = self.cached_line_screen_ranges.get(hi - 1) else {
                continue;
            };
            self.cached_screen_ranges
                .push((entry_idx, screen_lo, screen_hi, content_width));
        }
    }

    fn rebuild_copy_regions(&mut self, scroll: u16, body: Rect) -> usize {
        let mut regions: Vec<CopyHitRegion> = Vec::new();
        let mut context_regions: Vec<CopyHitRegion> = Vec::new();
        let view_end = scroll.saturating_add(body.height);
        let first = self
            .cached_code_blocks
            .partition_point(|block| block.block_end <= scroll);
        let visible_blocks = self.cached_code_blocks[first..]
            .iter()
            .take_while(|block| block.header_row < view_end);
        let mut visited_blocks = 0;
        for block in visible_blocks {
            visited_blocks += 1;
            if let Some(region) = code_context_region(
                block.header_row,
                block.block_end,
                scroll,
                body,
                &block.text,
                block.group,
            ) {
                context_regions.push(region);
            }
            if let Some(region) = copy_region(
                block.header_row,
                block.header_label.0,
                block.header_label.1,
                scroll,
                body,
                &block.text,
                block.group,
            ) {
                regions.push(region);
            }
            if let Some((footer_col, footer_cells)) = block.footer_label
                && let Some(region) = copy_region(
                    block.footer_row,
                    footer_col,
                    footer_cells,
                    scroll,
                    body,
                    &block.text,
                    block.group,
                )
            {
                regions.push(region);
            }
        }
        self.copy_hit_regions = regions;
        self.context_copy_regions = context_regions;
        visited_blocks
    }

    fn message_copy_region(&self, body: Rect) -> Option<CopyHitRegion> {
        let selected = self.selected_entries();
        let idx = if selected.len() == 1 {
            *selected.iter().next()?
        } else {
            return None;
        };
        let (_, rect) = self
            .entry_rects
            .iter()
            .find(|(entry_idx, _)| *entry_idx == idx)?;
        if rect.height == 0 {
            return None;
        }
        let text = self.yank_single_entry(idx);
        if text.is_empty() {
            return None;
        }
        let label = message_copy_label();
        Some(CopyHitRegion {
            rect: centered_message_copy_rect(&label, *rect, body)?,
            text: text.into(),
            kind: CopyHitKind::Message,
            group: idx,
        })
    }

    fn rebuild_message_copy_region(&mut self, body: Rect) {
        if let Some(region) = self.message_copy_region(body) {
            self.copy_hit_regions.push(region);
        }
    }

    fn render_window_end(&self) -> usize {
        self.cached_render_start
            .saturating_add(self.cached_entry_count)
            .min(self.entries.len())
    }

    fn shift_render_window(&mut self, new_start: usize) {
        if self.cached_render_width == 0 || new_start == self.cached_render_start {
            return;
        }

        let anchor = self
            .cached_screen_ranges
            .iter()
            .find(|(_, _lo, hi, _)| *hi > self.scroll_offset)
            .map(|(idx, lo, _hi, _)| (*idx, self.scroll_offset.saturating_sub(*lo)));

        self.cached_render_start = new_start;
        self.dirty = LinesDirty::Full;
        self.rebuild_lines(self.cached_render_width);
        self.last_total_rows = self.cached_total_rows;

        if let Some((anchor_idx, intra_entry_row)) = anchor
            && let Some((_, lo, _hi, _)) = self
                .cached_screen_ranges
                .iter()
                .find(|(idx, _, _, _)| *idx == anchor_idx)
        {
            self.scroll_offset = lo.saturating_add(intra_entry_row);
        }
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.clear_transcript_selection();
        self.pinned_to_bottom = false;
        if lines > self.scroll_offset && self.cached_render_start > 0 {
            let new_start = self
                .cached_render_start
                .saturating_sub(RENDER_WINDOW_SHIFT_ENTRIES);
            self.shift_render_window(new_start);
        }
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.clear_transcript_selection();
        let mut max = self.last_total_rows.saturating_sub(self.last_inner_height);
        if self.scroll_offset.saturating_add(lines) > max
            && self.render_window_end() < self.entries.len()
        {
            let natural_start = self.entries.len().saturating_sub(MAX_RENDERED_ENTRIES);
            let new_start = self
                .cached_render_start
                .saturating_add(RENDER_WINDOW_SHIFT_ENTRIES)
                .min(natural_start);
            self.shift_render_window(new_start);
            max = self.last_total_rows.saturating_sub(self.last_inner_height);
        }
        self.scroll_offset = self.scroll_offset.saturating_add(lines).min(max);
        if self.scroll_offset >= max && self.render_window_end() == self.entries.len() {
            self.pinned_to_bottom = true;
        }
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.last_inner_height.max(1));
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.last_inner_height.max(1));
    }

    pub fn scroll_to_top(&mut self) {
        self.clear_transcript_selection();
        self.pinned_to_bottom = false;
        self.cached_render_start = 0;
        self.mark_dirty_full();
        self.scroll_offset = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.clear_transcript_selection();
        self.cached_render_start = self.entries.len().saturating_sub(MAX_RENDERED_ENTRIES);
        self.mark_dirty_full();
        let max = self.last_total_rows.saturating_sub(self.last_inner_height);
        self.scroll_offset = max;
        self.pinned_to_bottom = true;
    }

    pub fn title(&self) -> String {
        self.title_parts()
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("  ")
    }

    fn title_parts(&self) -> Vec<(Option<TitleHitTarget>, String)> {
        let short = self.session_id.get(..7).unwrap_or(self.session_id.as_str());
        let mut parts: Vec<(Option<TitleHitTarget>, String)> = Vec::with_capacity(5);
        parts.push((Some(TitleHitTarget::Agent), self.agent_alias.clone()));
        if let Some(ref name) = self.session_name {
            parts.push((None, format!("— {name}")));
        }
        parts.push((None, short.to_string()));
        if let Some(ref provider) = self.model_provider_ref {
            parts.push((Some(TitleHitTarget::ModelProvider), provider.clone()));
        }
        if let Some(ref model) = self.model {
            parts.push((Some(TitleHitTarget::Model), model.clone()));
        }
        parts
    }

    fn refresh_title_hit_rects(&mut self, area: Rect) {
        self.title_hit_rects.clear();
        let mut x = area.x.saturating_add(2);
        let right = area.x.saturating_add(area.width);
        for (idx, (target, text)) in self.title_parts().into_iter().enumerate() {
            if idx > 0 {
                x = x.saturating_add(2);
            }
            let width = crate::display_width::display_width(text.as_str()) as u16;
            if let Some(target) = target
                && width > 0
                && x < right
            {
                self.title_hit_rects.push(TitleHitRect {
                    target,
                    rect: Rect::new(x, area.y, width.min(right.saturating_sub(x)), 1),
                });
            }
            x = x.saturating_add(width);
        }
    }

    fn title_hit_target_at(&self, col: u16, row: u16) -> Option<TitleHitTarget> {
        self.title_hit_rects
            .iter()
            .find(|hit| mouse::in_rect(col, row, hit.rect))
            .map(|hit| hit.target)
    }

    pub fn set_model_identity(&mut self, model_provider_ref: Option<&str>, model: Option<&str>) {
        if let Some(r) = model_provider_ref {
            self.model_provider_ref = Some(r.to_string());
        }
        if let Some(m) = model {
            self.model = Some(m.to_string());
        }
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[ChatEntry] {
        &self.entries
    }

    #[cfg(test)]
    pub fn current_agent_text(&self) -> &str {
        &self.streaming_text
    }

    #[cfg(test)]
    pub fn current_thought_text(&self) -> &str {
        &self.streaming_thought
    }

    pub fn pending_approval(&self) -> Option<&PendingApproval> {
        self.pending_approval.as_ref()
    }

    pub fn take_pending_approval(&mut self) -> Option<PendingApproval> {
        self.pending_approval.take()
    }

    pub fn pending_elicitation(&self) -> Option<&PendingElicitation> {
        self.pending_elicitation.as_ref()
    }

    #[cfg(test)]
    pub fn take_pending_elicitation(&mut self) -> Option<PendingElicitation> {
        self.pending_elicitation.take()
    }

    /// Install a pending elicitation modal. Replaces any prior one (the
    /// daemon serializes elicitations per session, so a second arrival
    /// before the first is answered is a protocol anomaly we resolve by
    /// keeping the newest).
    pub fn set_pending_elicitation(&mut self, e: PendingElicitation) {
        self.pending_elicitation = Some(e);
        self.mark_dirty_full();
    }

    /// Commit any accumulated streaming thought as an entry. Called at the two
    /// natural flush points: when a tool call interrupts thinking, and when the
    /// first response text chunk arrives after a thinking phase.
    fn flush_streaming_thought(&mut self) {
        let thought = std::mem::take(&mut self.streaming_thought);
        if !thought.is_empty() {
            self.entries
                .push(ChatEntry::AgentThought(Arc::<str>::from(thought)));
            self.mark_dirty_append();
        }
    }

    /// Commit any accumulated streaming text as an `AgentMessage` entry.
    /// Called when a tool call interrupts the text stream so that pre-tool
    /// text is committed in conversation order before the `Tool` entry.
    /// Returns `true` if any text was flushed.
    fn flush_streaming_text(&mut self) -> bool {
        let text = std::mem::take(&mut self.streaming_text);
        if !text.is_empty() {
            self.entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(text)));
            self.mark_dirty_append();
            true
        } else {
            false
        }
    }

    fn append_to_prompt_settled_stream(&mut self, text: &str) -> bool {
        let Some((generation, entry_index)) = self.prompt_settled_stream_entry else {
            return false;
        };
        if generation != self.turn_generation {
            self.prompt_settled_stream_entry = None;
            return false;
        }
        let Some(ChatEntry::AgentMessageContinuation(existing)) = self.entries.get_mut(entry_index)
        else {
            self.prompt_settled_stream_entry = None;
            return false;
        };
        existing.push_str(text);
        self.mark_dirty_tail(entry_index);
        true
    }

    fn freeze_prompt_settled_stream(&mut self) {
        let Some((_, entry_index)) = self.prompt_settled_stream_entry.take() else {
            return;
        };
        let Some(entry) = self.entries.get_mut(entry_index) else {
            return;
        };
        if let ChatEntry::AgentMessageContinuation(text) = entry {
            *entry = ChatEntry::AgentMessage(Arc::<str>::from(std::mem::take(text)));
        }
    }

    pub fn apply_update(&mut self, update: SessionUpdate) {
        // Ignore notifications that belong to a different session.
        let update_sid = match &update {
            SessionUpdate::AgentMessageChunk { session_id, .. }
            | SessionUpdate::AgentThoughtChunk { session_id, .. }
            | SessionUpdate::ToolCall { session_id, .. }
            | SessionUpdate::ToolResult { session_id, .. }
            | SessionUpdate::ApprovalRequest { session_id, .. }
            | SessionUpdate::ContextUsage { session_id, .. }
            | SessionUpdate::HistoryTrimmed { session_id, .. }
            | SessionUpdate::TurnComplete { session_id, .. }
            | SessionUpdate::Plan { session_id, .. } => session_id.as_str(),
        };
        if update_sid != self.session_id {
            return;
        }

        match update {
            SessionUpdate::AgentMessageChunk { text, .. } => {
                if !self.turn_in_flight && self.append_to_prompt_settled_stream(&text) {
                    return;
                }
                // Flush any accumulated thought before the response text begins
                // so it appears inline at the right position, not piled at the end.
                if self.streaming_text.is_empty() {
                    self.flush_streaming_thought();
                }
                self.streaming_text.push_str(&text);
                // Guard: don't mutate turn_status after commit_turn has already
                // set us back to Idle. Late-arriving notifications (broadcast
                // channel lag) can otherwise flip the input bar back to the
                // working animator even though the turn is done.
                if self.turn_in_flight {
                    self.turn_status = TurnStatus::Responding;
                }
            }
            SessionUpdate::AgentThoughtChunk { text, .. } => {
                self.freeze_prompt_settled_stream();
                self.streaming_thought.push_str(&text);
                if self.turn_in_flight {
                    self.turn_status = TurnStatus::Thinking;
                }
            }
            SessionUpdate::ToolCall {
                tool_call_id,
                name,
                raw_input,
                ..
            } => {
                self.freeze_prompt_settled_stream();
                // Flush any accumulated text and thought before the tool call
                // so that pre-tool agent text and thinking both appear in
                // conversation order before the Tool entry.
                if self.flush_streaming_text() {
                    self.turn_had_streaming_text = true;
                }
                self.flush_streaming_thought();
                self.turn_had_tool_calls = true;
                if self.turn_in_flight {
                    self.turn_status = TurnStatus::CallingTool(name.clone());
                }
                self.entries.push(ChatEntry::Tool {
                    tool_call_id: Arc::<str>::from(tool_call_id),
                    name: Arc::<str>::from(name),
                    input_json: Arc::<str>::from(
                        serde_json::to_string(&raw_input).unwrap_or_default(),
                    ),
                    result: None,
                });
                self.mark_dirty_append();
            }
            SessionUpdate::ToolResult {
                tool_call_id,
                raw_output,
                ..
            } => {
                // Cap stored output so large tool responses (bash, file reads) don't
                // accumulate unboundedly. The same bound is applied to restored cards.
                let raw_output = bounded_tool_output(raw_output);
                for entry in self.entries.iter_mut().rev() {
                    if let ChatEntry::Tool {
                        tool_call_id: id,
                        result,
                        ..
                    } = entry
                        && id.as_ref() == tool_call_id.as_str()
                    {
                        *result = Some(Arc::<str>::from(raw_output));
                        self.mark_dirty_full(); // mutation of existing entry
                        break;
                    }
                }
                if self.turn_in_flight && matches!(self.turn_status, TurnStatus::CallingTool(_)) {
                    self.turn_status = TurnStatus::Working;
                }
            }
            SessionUpdate::ApprovalRequest {
                request_id,
                tool_name,
                arguments_summary,
                timeout_secs,
                ..
            } => {
                self.pending_approval = Some(PendingApproval {
                    request_id,
                    tool_name,
                    arguments_summary,
                    timeout_secs,
                });
                if self.turn_in_flight {
                    self.turn_status = TurnStatus::WaitingForApproval;
                }
            }
            SessionUpdate::ContextUsage {
                input_tokens,
                max_context_tokens,
                model_context_window,
                ..
            } => {
                self.context_input_tokens = input_tokens;
                // Budget and capacity are one authoritative per-call snapshot.
                // In particular, `None` capacity is meaningful: compatibility
                // fallback routes omit it and must clear a prior configured
                // route's denominator instead of retaining stale state.
                self.context_max_tokens = max_context_tokens;
                self.context_model_window = model_context_window;
            }
            SessionUpdate::HistoryTrimmed {
                dropped_messages,
                kept_turns,
                reason,
                token_budget,
                tokens_before,
                tokens_after,
                tokens_before_source,
                tokens_after_source,
                unsatisfiable_floor,
                ..
            } => {
                self.freeze_prompt_settled_stream();
                let dropped = dropped_messages.to_string();
                let kept = kept_turns.to_string();
                // The unsatisfiable newest-turn/schema floor is flagged
                // explicitly by the runtime: the retained request cannot fit
                // the configured budget even though history MAY have been
                // trimmed on the way to that floor, so the notice must not
                // claim a successful trim.
                let at_floor = unsatisfiable_floor == Some(true);
                let notice = if at_floor {
                    crate::i18n::t_args(
                        "zc-chat-history-trimmed-floor",
                        &[
                            ("reason", &reason),
                            ("after", &tokens_after.unwrap_or_default().to_string()),
                            ("budget", &token_budget.unwrap_or_default().to_string()),
                        ],
                    )
                } else {
                    match (tokens_before, tokens_after) {
                        (Some(before), Some(after)) => {
                            let mut notice = crate::i18n::t_args(
                                "zc-chat-history-trimmed-tokens",
                                &[
                                    ("reason", &reason),
                                    ("before", &before.to_string()),
                                    ("after", &after.to_string()),
                                    ("dropped", &dropped),
                                    ("kept", &kept),
                                ],
                            );
                            // The configured budget is context, never the trim
                            // target: recovery trims toward a provider-overflow
                            // target, so the notice must not present the
                            // configured limit as governing the trim.
                            if let Some(budget) = token_budget {
                                notice.push_str(&crate::i18n::t_args(
                                    "zc-chat-history-trimmed-token-budget-clause",
                                    &[("budget", &budget.to_string())],
                                ));
                            }
                            let before_label = tokens_before_source.as_deref().and_then(|source| {
                                crate::i18n::try_t(&token_source_fluent_key(source))
                            });
                            let after_label = tokens_after_source.as_deref().and_then(|source| {
                                crate::i18n::try_t(&token_source_fluent_key(source))
                            });
                            if let (Some(before_label), Some(after_label)) =
                                (before_label, after_label)
                            {
                                notice.push(' ');
                                notice.push_str(&crate::i18n::t_args(
                                    "zc-chat-history-trimmed-token-sources",
                                    &[("before", &before_label), ("after", &after_label)],
                                ));
                            }
                            notice
                        }
                        _ => crate::i18n::t_args(
                            "zc-chat-history-trimmed",
                            &[("reason", &reason), ("dropped", &dropped), ("kept", &kept)],
                        ),
                    }
                };
                self.entries
                    .push(ChatEntry::SystemMessage(Arc::<str>::from(notice)));
                self.mark_dirty_append();
            }
            SessionUpdate::TurnComplete {
                client_turn_generation,
                message_count,
                outcome,
                content,
                ..
            } => {
                if client_turn_generation
                    .is_some_and(|generation| generation != self.turn_generation)
                {
                    return;
                }
                if let Some(message_count) = message_count {
                    self.message_count = message_count;
                }
                match outcome {
                    TurnEndOutcome::Completed => {
                        self.last_error = None;
                        self.commit_turn(content, true);
                    }
                    TurnEndOutcome::Cancelled | TurnEndOutcome::Failed => {
                        if outcome == TurnEndOutcome::Failed {
                            // The daemon's session-loss sentinel (see
                            // `handle_session_prompt`) means the session must be
                            // re-attached before the next prompt can run.
                            self.last_error = Some(if content.ends_with("session_not_found") {
                                SessionError::SessionLost
                            } else {
                                SessionError::TurnFailed
                            });
                        }
                        self.entries
                            .push(ChatEntry::SystemMessage(Arc::<str>::from(content.as_str())));
                        self.mark_dirty_append();
                        self.commit_turn(String::new(), false);
                    }
                }
            }
            // Whole-list replace: hand the authoritative plan to the
            // tracker, which runs the auto-pop rule. Session routing is
            // already enforced by the session_id check above.
            SessionUpdate::Plan { entries, .. } => {
                self.todo_tracker.set_plan(entries);
            }
        }
    }

    pub fn commit_turn(&mut self, full_text: String, clean: bool) {
        self.freeze_prompt_settled_stream();
        if self.flush_streaming_text() {
            self.turn_had_streaming_text = true;
        }
        self.flush_streaming_thought();
        // If no streaming text was accumulated during this turn, use the
        // daemon-provided final text as a fallback so the turn is never
        // invisible to the user.
        if !self.turn_had_streaming_text && !full_text.is_empty() {
            self.entries
                .push(ChatEntry::AgentMessage(Arc::<str>::from(full_text)));
            self.mark_dirty_append();
        } else if clean
            && !self.turn_had_streaming_text
            && !self.turn_had_tool_calls
            && full_text.is_empty()
        {
            // Clean completion with no streamed text, no tool calls, and
            // no final content — render a diagnostic so the user knows the
            // turn finished rather than silently vanishing.
            self.entries
                .push(ChatEntry::SystemMessage(Arc::<str>::from(crate::i18n::t(
                    "zc-turn-no-output",
                ))));
            self.mark_dirty_append();
        }
        self.turn_had_streaming_text = false;
        self.turn_had_tool_calls = false;
        self.settle_turn_lifecycle(clean);
    }

    fn settle_turn_from_prompt_response(&mut self) {
        self.freeze_prompt_settled_stream();
        let text = std::mem::take(&mut self.streaming_text);
        if !text.is_empty() {
            self.turn_had_streaming_text = true;
            let entry_index = self.entries.len();
            self.entries.push(ChatEntry::AgentMessageContinuation(text));
            self.prompt_settled_stream_entry = Some((self.turn_generation, entry_index));
            self.mark_dirty_append();
        }
        self.flush_streaming_thought();
        if self
            .prompt_settled_stream_entry
            .is_some_and(|(_, entry_index)| entry_index + 1 != self.entries.len())
        {
            self.freeze_prompt_settled_stream();
        }
        // Preserve per-turn provenance for a delayed terminal notification;
        // the next turn resets both flags when its user message is committed.
        self.mark_dirty_append();
        self.settle_turn_lifecycle(false);
    }

    fn settle_turn_lifecycle(&mut self, clean: bool) {
        self.turn_in_flight = false;
        self.optimistic_user_message = None;
        self.turn_status = TurnStatus::Idle;
        self.cancel_started_at = None;
        let mut cleanup_report = self.cleanup_active_turn_attachments();
        cleanup_report.merge(self.input_bar.take_cleanup_report());
        self.surface_cleanup_report(cleanup_report);
        if !clean && !self.resume_override && !self.message_queue.is_empty() {
            self.queue_paused = true;
        }
        self.resume_override = false;
    }

    pub fn enter_cancelling(&mut self) {
        self.turn_status = TurnStatus::Cancelling;
        self.cancel_started_at = Some(Instant::now());
    }

    pub fn cancel_watchdog_expired(&self) -> bool {
        matches!(self.turn_status, TurnStatus::Cancelling)
            && self
                .cancel_started_at
                .is_some_and(|t| t.elapsed() >= CANCEL_WATCHDOG)
    }

    /// Traffic-light status for the agent sidebar, in priority order:
    /// error > needs-human > running > ready. `Cancelling` counts as
    /// running (the turn is still winding down); a pending elicitation
    /// counts only when it targets this session (defense against a stale
    /// modal surviving a session switch).
    /// Terminal-facing turn status. An operator wait outranks whatever the
    /// turn was doing, so the terminal reads as blocked while a prompt is up
    /// and returns to the turn's own state once it is answered.
    pub(crate) fn terminal_status(&self) -> TurnStatus {
        if self
            .pending_elicitation
            .as_ref()
            .is_some_and(|e| e.session_id == self.session_id)
        {
            TurnStatus::WaitingForInput
        } else if self.pending_approval.is_some() {
            TurnStatus::WaitingForApproval
        } else {
            self.turn_status.clone()
        }
    }

    pub(crate) fn sidebar_status(&self) -> SidebarStatus {
        if self.last_error.is_some() {
            SidebarStatus::Errored
        } else if self.pending_approval.is_some()
            || self
                .pending_elicitation
                .as_ref()
                .is_some_and(|e| e.session_id == self.session_id)
        {
            SidebarStatus::NeedsHuman
        } else if self.turn_in_flight {
            SidebarStatus::Running
        } else {
            SidebarStatus::Ready
        }
    }

    pub fn push_user_message(&mut self, text: Option<String>, attachments: Vec<String>) {
        self.freeze_prompt_settled_stream();
        // A new prompt supersedes the previous failure: the red dot clears
        // until the daemon reports otherwise.
        self.last_error = None;
        if self.first_message.is_none()
            && let Some(ref t) = text
            && !t.trim().is_empty()
        {
            self.first_message = Some(t.clone());
        }
        let entry_index = self.entries.len();
        self.entries.push(ChatEntry::UserMessage {
            text: text.map(Arc::<str>::from),
            attachments: attachments.into_iter().map(Arc::<str>::from).collect(),
        });
        self.mark_dirty_append();
        let prior_message_count = self.message_count;
        self.message_count = prior_message_count.saturating_add(1);
        self.turn_in_flight = true;
        self.turn_generation = self.turn_generation.wrapping_add(1);
        self.optimistic_user_message =
            Some((self.turn_generation, entry_index, prior_message_count));
        self.turn_had_streaming_text = false;
        self.turn_had_tool_calls = false;
        // Start a fresh status + animation anchor. We're `Working` until the
        // first chunk (thought / message / tool-call) tells us otherwise.
        self.turn_status = TurnStatus::Working;
        self.turn_started_at = Instant::now();
    }

    fn own_active_turn_attachments(&mut self, attachments: Vec<PendingAttachment>) {
        // Recovery can abandon a turn without its terminal notification.
        // Never overwrite the prior turn's clipboard-owned temporary files.
        let cleanup_report = self.cleanup_active_turn_attachments();
        self.surface_cleanup_report(cleanup_report);
        self.active_turn_attachments = attachments;
    }

    fn cleanup_active_turn_attachments(&mut self) -> CleanupReport {
        cleanup_attachment_temps(&std::mem::take(&mut self.active_turn_attachments))
    }

    fn remove_optimistic_user_message(&mut self, generation: u64) {
        let Some((marked_generation, entry_index, prior_message_count)) =
            self.optimistic_user_message.take()
        else {
            return;
        };
        if marked_generation != generation {
            return;
        }
        self.message_count = prior_message_count;
        if matches!(
            self.entries.get(entry_index),
            Some(ChatEntry::UserMessage { .. })
        ) {
            self.entries.remove(entry_index);
            self.first_message = self.entries.iter().find_map(|entry| {
                let ChatEntry::UserMessage {
                    text: Some(text), ..
                } = entry
                else {
                    return None;
                };
                let display = strip_enrichment_prefix(text.as_ref());
                (!display.trim().is_empty()).then(|| display.to_string())
            });
            self.mark_dirty_full();
        }
    }

    const QUEUE_CAP: usize = 32;
    const QUEUE_SIDEBAR_COLS_MIN: u16 = 24;
    const QUEUE_SIDEBAR_COLS_MAX: u16 = 80;
    const QUEUE_SIDEBAR_COLS_STEP: u16 = 4;
    const QUEUE_CHAT_COLS_MIN: u16 = 20;

    fn alloc_queue_id(&mut self) -> u64 {
        let id = self.next_queue_id;
        self.next_queue_id = self.next_queue_id.wrapping_add(1);
        id
    }

    pub fn enqueue_message(
        &mut self,
        text: String,
        attachments: Vec<PendingAttachment>,
    ) -> Result<(), String> {
        if text.trim().is_empty() && attachments.is_empty() {
            let cleanup_report = cleanup_attachment_temps(&attachments);
            self.surface_cleanup_report(cleanup_report);
            return Err(crate::i18n::t("zc-queue-empty"));
        }
        let pending = self.message_queue.len();
        if pending >= Self::QUEUE_CAP {
            let cleanup_report = cleanup_attachment_temps(&attachments);
            self.surface_cleanup_report(cleanup_report);
            return Err(crate::i18n::t_args(
                "zc-queue-full",
                &[("cap", &Self::QUEUE_CAP.to_string())],
            ));
        }
        let id = self.alloc_queue_id();
        self.message_queue.push_back(QueuedMessage {
            id,
            text,
            attachments,
            status: QueueItemStatus::Pending,
        });
        Ok(())
    }

    pub fn inject_message(
        &mut self,
        text: String,
        attachments: Vec<PendingAttachment>,
    ) -> Result<(), String> {
        if text.trim().is_empty() && attachments.is_empty() {
            let cleanup_report = cleanup_attachment_temps(&attachments);
            self.surface_cleanup_report(cleanup_report);
            return Err(crate::i18n::t("zc-queue-empty"));
        }
        if self.message_queue.len() >= Self::QUEUE_CAP {
            let cleanup_report = cleanup_attachment_temps(&attachments);
            self.surface_cleanup_report(cleanup_report);
            return Err(crate::i18n::t_args(
                "zc-queue-full",
                &[("cap", &Self::QUEUE_CAP.to_string())],
            ));
        }
        let id = self.alloc_queue_id();
        let insert_at = self
            .message_queue
            .iter()
            .position(|m| m.status == QueueItemStatus::Pending)
            .unwrap_or(self.message_queue.len());
        self.message_queue.insert(
            insert_at,
            QueuedMessage {
                id,
                text,
                attachments,
                status: QueueItemStatus::Injected,
            },
        );
        // An inject is the force-send-now intent: resume the queue and let it
        // survive a cancel auto-pause, unlike a plain queued submission.
        self.queue_paused = false;
        if self.turn_in_flight {
            self.resume_override = true;
        }
        Ok(())
    }

    fn next_dispatch_index(&self) -> Option<usize> {
        if self.turn_in_flight {
            return None;
        }
        if let Some(idx) = self
            .message_queue
            .iter()
            .position(|m| m.status == QueueItemStatus::Injected)
        {
            return Some(idx);
        }
        if self.queue_paused {
            return None;
        }
        self.message_queue
            .iter()
            .position(|m| m.status == QueueItemStatus::Pending)
    }

    pub fn take_next_dispatchable(&mut self) -> Option<QueuedMessage> {
        let idx = self.next_dispatch_index()?;
        let msg = self.message_queue.remove(idx)?;
        self.resume_override = false;
        if self.queue_sel == Some(msg.id) {
            self.queue_sel = None;
        }
        Some(msg)
    }

    /// Flip the queue pause state. Returns the new paused value so the caller
    /// can pump on resume and surface the right notice.
    pub fn toggle_queue_pause(&mut self) -> bool {
        self.queue_paused = !self.queue_paused;
        self.queue_paused
    }

    pub fn queue_paused(&self) -> bool {
        self.queue_paused
    }

    /// Clear an explicit pause without bypassing the cancel auto-pause: a
    /// cancelled turn settles into the paused state and the backlog waits for a
    /// deliberate resume. Returns true if the queue was paused.
    pub fn resume_queue(&mut self) -> bool {
        let was_paused = self.queue_paused;
        self.queue_paused = false;
        was_paused
    }

    pub fn queue_len(&self) -> usize {
        self.message_queue.len()
    }

    fn reconnect_queue_state(&self) -> ReconnectQueueState {
        ReconnectQueueState {
            messages: self.message_queue.clone(),
            next_id: self.next_queue_id,
            paused: self.queue_paused,
            selected: self.queue_sel,
            composer_text: self.input_bar.input().to_string(),
            composer_attachments: self.input_bar.reconnect_file_attachments(),
        }
    }

    /// Restore the queue and composer state the client owns across a transport
    /// rebuild. Transcript, pending interactions, and terminal turn state come
    /// from (or are reconciled with) the daemon instead of being snapshotted.
    fn restore_reconnect_state(
        &mut self,
        queue: ReconnectQueueState,
        interrupted: bool,
        recovery_required: bool,
    ) {
        self.message_queue = queue.messages;
        self.next_queue_id = queue.next_id;
        self.queue_paused = queue.paused;
        self.queue_sel = queue
            .selected
            .filter(|id| self.message_queue.iter().any(|message| message.id == *id));
        self.input_bar
            .load_for_edit(queue.composer_text, queue.composer_attachments);
        self.resume_override = false;
        if recovery_required {
            self.last_error = Some(SessionError::ResyncFailed);
        }
        if interrupted {
            self.entries
                .push(ChatEntry::SystemMessage(Arc::<str>::from(crate::i18n::t(
                    "zc-chat-reconnect-interrupted",
                ))));
            self.mark_dirty_append();
        }
    }

    /// Store a transient note for the info bar (queue/attach/detach feedback).
    /// Routes through the shared `info_message` bar so it inherits TTL auto-clear
    /// and consistent rendering with model-switch notes.
    pub fn set_info_notice(&mut self, msg: String) {
        self.info_message = Some(crate::widgets::InfoMessage::note(msg));
    }

    fn surface_cleanup_report(&mut self, report: CleanupReport) {
        if let Some(message) = report.notice() {
            self.set_info_notice(message);
        }
    }

    fn set_overlay_copy_feedback(&mut self, anchor: Rect) {
        if let Some(rect) = centered_copy_feedback_rect(&message_copied_label(), anchor) {
            self.set_copy_feedback(CopyFeedbackTarget::Overlay(rect));
        }
    }

    fn set_copy_feedback(&mut self, target: CopyFeedbackTarget) {
        self.copy_feedback = Some(CopyFeedback {
            target,
            shown_at: Instant::now(),
        });
    }

    /// Drop the active info-bar message (on submit, inject, or turn start).
    pub fn clear_info_notice(&mut self) {
        self.info_message = None;
        self.copy_feedback = None;
    }

    fn expire_copy_feedback(&mut self) {
        let expired = self
            .copy_feedback
            .is_some_and(|feedback| feedback.shown_at.elapsed() >= COPY_FEEDBACK_TTL);
        if expired {
            self.copy_feedback = None;
        }
    }

    /// The queue sidebar is open exactly when the queue is non-empty. There is
    /// no manual toggle: it appears with the first queued message and closes
    /// when the queue drains, so its presence always reflects real state.
    pub fn queue_sidebar_open(&self) -> bool {
        !self.message_queue.is_empty()
    }

    /// Default the sidebar selection to the front item when nothing is selected
    /// yet (e.g. the first message just opened the sidebar). Keeps keyboard
    /// delete/edit working without a manual open step.
    pub fn ensure_queue_selection(&mut self) {
        if self.queue_sel.is_none()
            && let Some(front) = self.message_queue.front()
        {
            self.queue_sel = Some(front.id);
        }
    }

    /// Select a queued item by id (mouse left-click in the sidebar). Ignores
    /// ids no longer present. Returns true when the selection changed.
    pub fn select_queued_by_id(&mut self, id: u64) -> bool {
        if self.message_queue.iter().any(|m| m.id == id) && self.queue_sel != Some(id) {
            self.queue_sel = Some(id);
            self.mark_dirty_full();
            true
        } else {
            false
        }
    }

    /// Hit-test a screen point against the last sidebar draw and select the
    /// queued item under it, if any. Returns true when something was selected.
    pub fn queue_click_at(&mut self, col: u16, row: u16) -> bool {
        let hit = self
            .queue_item_rects
            .iter()
            .find(|(_, r)| mouse::in_rect(col, row, *r))
            .map(|(id, _)| *id);
        match hit {
            Some(id) => self.select_queued_by_id(id),
            None => false,
        }
    }

    /// True when the point lies within the last drawn sidebar inner rect.
    pub fn point_in_queue_sidebar(&self, col: u16, row: u16) -> bool {
        self.queue_sidebar_rect
            .is_some_and(|r| mouse::in_rect(col, row, r))
    }

    /// Scroll the queue sidebar by `delta` rows (negative = up). Clamped to the
    /// content overflow recorded on the last draw.
    pub fn queue_scroll_by(&mut self, delta: i16) {
        let new = (self.queue_scroll as i32 + delta as i32).max(0) as u16;
        if new != self.queue_scroll {
            self.queue_scroll = new;
            self.mark_dirty_full();
        }
    }

    pub fn widen_queue_sidebar(&mut self) {
        self.queue_sidebar_cols = (self.queue_sidebar_cols + Self::QUEUE_SIDEBAR_COLS_STEP)
            .min(Self::QUEUE_SIDEBAR_COLS_MAX);
        self.mark_dirty_full();
    }

    pub fn narrow_queue_sidebar(&mut self) {
        self.queue_sidebar_cols = self
            .queue_sidebar_cols
            .saturating_sub(Self::QUEUE_SIDEBAR_COLS_STEP)
            .max(Self::QUEUE_SIDEBAR_COLS_MIN);
        self.mark_dirty_full();
    }

    /// Queue sidebar width in columns for a given chat area width. The stored
    /// column width is clamped to the absolute range, then to whatever leaves
    /// the chat column its floor on a terminal too narrow for both.
    pub fn queue_sidebar_width(&self, area_width: u16) -> u16 {
        let upper =
            Self::QUEUE_SIDEBAR_COLS_MAX.min(area_width.saturating_sub(Self::QUEUE_CHAT_COLS_MIN));
        let lower = Self::QUEUE_SIDEBAR_COLS_MIN.min(upper);
        self.queue_sidebar_cols.clamp(lower, upper)
    }

    fn editable_ids(&self) -> Vec<u64> {
        self.message_queue.iter().map(|m| m.id).collect()
    }

    pub fn queue_select_step(&mut self, delta: isize) {
        let ids = self.editable_ids();
        if ids.is_empty() {
            self.queue_sel = None;
            return;
        }
        let cur = self
            .queue_sel
            .and_then(|id| ids.iter().position(|&x| x == id))
            .unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(ids.len() as isize) as usize;
        self.queue_sel = Some(ids[next]);
        self.mark_dirty_full();
    }

    fn selected_queue_id(&self) -> Option<u64> {
        self.queue_sel
            .filter(|id| self.message_queue.iter().any(|message| message.id == *id))
    }

    fn queued_text(&self, id: u64) -> Option<String> {
        self.message_queue
            .iter()
            .find(|message| message.id == id)
            .map(|message| message.text.clone())
    }

    fn promote_queued_by_id(&mut self, id: u64) -> bool {
        let Some(position) = self
            .message_queue
            .iter()
            .position(|message| message.id == id)
        else {
            return false;
        };
        let pending = self.message_queue[position].status == QueueItemStatus::Pending;
        if pending {
            let Some(mut message) = self.message_queue.remove(position) else {
                return false;
            };
            message.status = QueueItemStatus::Injected;
            let insert_at = self
                .message_queue
                .iter()
                .position(|queued| queued.status == QueueItemStatus::Pending)
                .unwrap_or(self.message_queue.len());
            self.message_queue.insert(insert_at, message);
        }
        let resumed = self.resume_queue();
        if self.turn_in_flight {
            self.resume_override = true;
        }
        if pending || resumed {
            self.mark_dirty_full();
        }
        true
    }

    fn delete_queued_by_id(&mut self, id: u64) -> bool {
        let Some(position) = self
            .message_queue
            .iter()
            .position(|message| message.id == id)
        else {
            return false;
        };
        if let Some(message) = self.message_queue.remove(position) {
            let cleanup_report = cleanup_attachment_temps(&message.attachments);
            self.surface_cleanup_report(cleanup_report);
        }
        let ids = self.editable_ids();
        self.queue_sel = ids.get(position.min(ids.len().saturating_sub(1))).copied();
        self.mark_dirty_full();
        true
    }

    #[cfg(test)]
    pub fn delete_selected_queued(&mut self) {
        if let Some(id) = self.selected_queue_id() {
            self.delete_queued_by_id(id);
        }
    }

    fn take_queued_for_edit(&mut self, id: u64) -> Option<(String, Vec<PendingAttachment>)> {
        let position = self
            .message_queue
            .iter()
            .position(|message| message.id == id)?;
        let message = self.message_queue.remove(position)?;
        self.queue_sel = self.editable_ids().first().copied();
        self.mark_dirty_full();
        Some((message.text, message.attachments))
    }

    #[cfg(test)]
    pub fn take_selected_for_edit(&mut self) -> Option<(String, Vec<PendingAttachment>)> {
        self.take_queued_for_edit(self.selected_queue_id()?)
    }

    /// Slash-command queue removal. `None` clears the whole queue; `Some(n)`
    /// removes the 1-based item shown in the sidebar. Returns a user-facing
    /// info-bar message. `Some(0)` is the invalid-index sentinel from a
    /// malformed `/clear-queue` arg.
    pub fn clear_queue_cmd(&mut self, index: Option<usize>) -> String {
        let count = self.message_queue.len();
        match index {
            None => {
                if count == 0 {
                    return crate::i18n::t("zc-queue-clear-empty");
                }
                let cleanup_report = self.clear_queue();
                self.mark_dirty_full();
                append_cleanup_notice(
                    crate::i18n::t_args("zc-queue-cleared-all", &[("count", &count.to_string())]),
                    cleanup_report.notice(),
                )
            }
            Some(n) => {
                if count == 0 {
                    return crate::i18n::t("zc-queue-clear-empty");
                }
                if n == 0 || n > count {
                    return crate::i18n::t_args(
                        "zc-queue-clear-invalid",
                        &[("index", &n.to_string()), ("count", &count.to_string())],
                    );
                }
                let pos = n - 1;
                let mut cleanup_report = CleanupReport::default();
                if let Some(msg) = self.message_queue.remove(pos) {
                    cleanup_report = cleanup_attachment_temps(&msg.attachments);
                    if self.queue_sel == Some(msg.id) {
                        let ids = self.editable_ids();
                        self.queue_sel = ids.get(pos.min(ids.len().saturating_sub(1))).copied();
                    }
                }
                self.mark_dirty_full();
                append_cleanup_notice(
                    crate::i18n::t_args("zc-queue-cleared-one", &[("index", &n.to_string())]),
                    cleanup_report.notice(),
                )
            }
        }
    }

    fn clear_queue(&mut self) -> CleanupReport {
        let mut cleanup_report = CleanupReport::default();
        for msg in self.message_queue.drain(..) {
            cleanup_report.merge(cleanup_attachment_temps(&msg.attachments));
        }
        self.next_queue_id = 0;
        self.queue_paused = false;
        self.resume_override = false;
        self.queue_sel = None;
        cleanup_report
    }

    fn prepare_for_notification_resync(&mut self) {
        self.freeze_prompt_settled_stream();
        self.pending_approval = None;
        self.pending_elicitation = None;
        self.streaming_text.clear();
        self.streaming_thought.clear();
        self.turn_in_flight = false;
        self.optimistic_user_message = None;
        self.turn_had_streaming_text = false;
        self.turn_had_tool_calls = false;
        self.turn_status = TurnStatus::Idle;
        self.cancel_started_at = None;
        self.resume_override = false;
        let cleanup_report = self.cleanup_active_turn_attachments();
        self.surface_cleanup_report(cleanup_report);
        self.set_info_notice(crate::i18n::t("zc-chat-resyncing"));
    }

    fn replace_history_after_notification_resync(
        &mut self,
        messages: Vec<crate::client::MessageEntry>,
        strip_runtime_enrichment: bool,
    ) {
        self.entries.clear();
        self.first_message = None;
        self.cached_lines.clear();
        self.cached_row_breaks.clear();
        self.cached_line_ranges.clear();
        self.cached_screen_ranges.clear();
        self.cached_entry_count = 0;
        self.cached_render_start = 0;
        self.cached_render_width = 0;
        self.cached_total_rows = 0;
        self.clear_transcript_selection();
        self.load_history(messages, strip_runtime_enrichment);
        self.entries
            .push(ChatEntry::SystemMessage(Arc::<str>::from(crate::i18n::t(
                "zc-chat-resynced",
            ))));
        self.info_message = None;
        self.last_error = None;
        self.mark_dirty_full();
    }

    fn load_history(
        &mut self,
        messages: Vec<crate::client::MessageEntry>,
        strip_runtime_enrichment: bool,
    ) {
        for m in messages {
            match m.kind {
                crate::client::MessageEntryKind::ToolCall => {
                    let input_json = m
                        .tool_input
                        .as_ref()
                        .and_then(|value| serde_json::to_string(value).ok())
                        .unwrap_or_else(|| "null".to_string());
                    self.entries.push(ChatEntry::Tool {
                        tool_call_id: Arc::<str>::from(m.tool_call_id.unwrap_or_default()),
                        name: Arc::<str>::from(
                            m.tool_name.unwrap_or_else(|| "unknown".to_string()),
                        ),
                        input_json: Arc::<str>::from(input_json),
                        result: m.tool_output.map(bounded_tool_output).map(Arc::<str>::from),
                    });
                    continue;
                }
                crate::client::MessageEntryKind::ToolResult => {
                    let tool_call_id = m.tool_call_id.unwrap_or_default();
                    let output = bounded_tool_output(m.tool_output.unwrap_or(m.content));
                    if let Some(ChatEntry::Tool { result, .. }) =
                        self.entries.iter_mut().rev().find(|entry| {
                            matches!(
                                entry,
                                ChatEntry::Tool {
                                    tool_call_id: id,
                                    result: None,
                                    ..
                                } if id.as_ref() == tool_call_id
                            )
                        })
                    {
                        *result = Some(Arc::<str>::from(output));
                    } else {
                        self.entries.push(ChatEntry::Tool {
                            tool_call_id: Arc::<str>::from(tool_call_id),
                            name: Arc::<str>::from(
                                m.tool_name.unwrap_or_else(|| "unknown".to_string()),
                            ),
                            input_json: Arc::<str>::from("null"),
                            result: Some(Arc::<str>::from(output)),
                        });
                    }
                    continue;
                }
                crate::client::MessageEntryKind::Message
                | crate::client::MessageEntryKind::Unknown => {}
            }
            match m.role() {
                crate::client::MessageRole::User => {
                    let display = if strip_runtime_enrichment {
                        strip_enrichment_prefix(&m.content)
                    } else {
                        &m.content
                    };
                    if self.first_message.is_none() && !display.trim().is_empty() {
                        self.first_message = Some(display.to_string());
                    }
                    self.entries.push(ChatEntry::UserMessage {
                        text: Some(Arc::<str>::from(m.content)),
                        attachments: vec![],
                    });
                }
                crate::client::MessageRole::Assistant => {
                    self.entries
                        .push(ChatEntry::AgentMessage(Arc::<str>::from(m.content)));
                }
                crate::client::MessageRole::System | crate::client::MessageRole::Other => {}
            }
        }
        self.mark_dirty_full();
    }
    /// Reset conversational state for a new or switched session.
    /// Re-materialize this pane's state for a newly-entered session.
    ///
    /// `todo_settings` is resolved fresh by the caller at the transition
    /// boundary so restart and saved-session switch pick up Config-pane edits,
    /// exactly like a brand-new session does.
    pub fn reset_for_session(
        &mut self,
        session_id: String,
        name: Option<String>,
        todo_settings: crate::todo_tracker::TodoTrackerSettings,
    ) {
        self.session_id = session_id;
        self.session_name = name;
        self.model_provider_ref = None;
        self.model = None;
        self.input_bar.reset();
        let mut cleanup_report = self.input_bar.take_cleanup_report();
        self.entries.clear();
        self.streaming_text.clear();
        self.streaming_thought.clear();
        self.cached_lines.clear();
        self.cached_row_breaks.clear();
        self.cached_line_ranges.clear();
        self.cached_line_screen_ranges.clear();
        self.cached_screen_ranges.clear();
        self.cached_code_blocks.clear();
        self.entry_rects.clear();
        self.tool_header_rects.clear();
        self.tool_footer_rects.clear();
        self.tool_disclosures.clear();
        self.cached_tool_footer_lines.clear();
        self.copy_hit_regions.clear();
        self.context_copy_regions.clear();
        self.context_menu = None;
        self.copy_feedback = None;
        self.dirty = LinesDirty::Full;
        self.cached_entry_count = 0;
        self.cached_render_start = 0;
        self.cached_render_width = 0;
        self.pending_approval = None;
        self.pending_elicitation = None;
        self.last_error = None;
        self.turn_in_flight = false;
        self.message_count = 0;
        self.turn_generation = self.turn_generation.wrapping_add(1);
        self.prompt_settled_stream_entry = None;
        self.turn_status = TurnStatus::Idle;
        self.cancel_started_at = None;
        self.browse_cursor = None;
        self.browse_anchor = None;
        self.mouse_down_entry = None;
        self.transcript_snapshot = None;
        self.transcript_selection = None;
        self.browse_multi.clear();
        // Reset branch cache: new session may have a different cwd.
        self.git_branch = None;
        self.first_message = None;
        self.git_hash = None;
        self.git_branch_last_fetch = None;
        // Context usage is per-session; clear so we don't show stale numbers
        // from the previous session before the first LLM call fires a new
        // ContextUsage event.
        self.context_input_tokens = None;
        self.context_max_tokens = None;
        self.context_model_window = None;
        // The TodoWrite plan is per-session; drop it (and its show/hide state)
        // so a switched-to session doesn't inherit the previous plan's tasks.
        // Rebuilding from freshly resolved settings also applies any Config-pane
        // edit made since this pane's `ChatState` was constructed.
        self.todo_tracker.reset_for_session(todo_settings);
        self.todo_close_hit_rect = None;
        cleanup_report.merge(self.cleanup_active_turn_attachments());
        cleanup_report.merge(self.clear_queue());
        self.surface_cleanup_report(cleanup_report);
    }
}

/// Strip the runtime's date/time enrichment prefix from an ACP-persisted user
/// message. ACP stores the Agent's provider-visible history, while normal Chat
/// sessions store raw prompts and must preserve an identical user-authored
/// prefix. Content without the runtime envelope passes through unchanged.
fn strip_enrichment_prefix(content: &str) -> &str {
    let Some(rest) = content.strip_prefix("[CURRENT DATE & TIME:") else {
        return content;
    };
    let Some(bracket_end) = rest.find(']') else {
        return content;
    };
    rest[bracket_end + 1..].trim_start()
}

/// Body-only clipboard text.
fn clipboard_text(entry: &ChatEntry) -> String {
    match entry {
        ChatEntry::UserMessage { text, attachments } => {
            let base = text.as_deref().unwrap_or("");
            if attachments.is_empty() {
                base.to_string()
            } else {
                let label = attachments
                    .iter()
                    .map(|a| a.as_ref())
                    .collect::<Vec<&str>>()
                    .join(", ");
                format!("{base} [{label}]")
            }
        }
        ChatEntry::AgentMessage(t) => t.to_string(),
        ChatEntry::AgentMessageContinuation(t) => t.clone(),
        ChatEntry::AgentThought(t) => format!("(thinking) {t}"),
        ChatEntry::SystemMessage(t) => t.to_string(),
        ChatEntry::Tool {
            name,
            input_json,
            result,
            ..
        } => match result {
            Some(r) => format!("[tool: {name}] {input_json}\n  \u{2514}\u{2500} {r}"),
            None => format!("[tool: {name}] {input_json}"),
        },
    }
}

/// Role-prefixed clipboard text. Used when ≥2 entries are yanked.
fn labelled_clipboard_text(entry: &ChatEntry) -> String {
    match entry {
        ChatEntry::UserMessage { .. } => {
            crate::i18n::t_args("zc-chat-clipboard-you", &[("text", &clipboard_text(entry))])
        }
        ChatEntry::AgentMessage(_) | ChatEntry::AgentMessageContinuation(_) => crate::i18n::t_args(
            "zc-chat-clipboard-agent",
            &[("text", &clipboard_text(entry))],
        ),
        _ => clipboard_text(entry),
    }
}

/// Suspend the TUI, open `$VISUAL` / `$EDITOR` with `content`, return the edited text.
/// Restores raw mode and alternate screen before returning.
/// Falls back to `content` unchanged if no editor is available or the process fails.
pub async fn open_editor_for_content(content: &str) -> String {
    let Some(editor) = crate::editor::editor_from_env_or_path() else {
        return content.to_string();
    };

    let tmp = match tempfile::NamedTempFile::new() {
        Ok(f) => f,
        Err(_) => return content.to_string(),
    };
    if std::fs::write(tmp.path(), content).is_err() {
        return content.to_string();
    }

    crossterm::terminal::disable_raw_mode().ok();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::PopKeyboardEnhancementFlags,
        crossterm::terminal::LeaveAlternateScreen
    );

    let path = tmp.path().to_owned();
    let status = tokio::process::Command::new(&editor)
        .arg(&path)
        .status()
        .await;

    crossterm::terminal::enable_raw_mode().ok();
    // The editor owned the terminal and may have set its own title, so the
    // cached view of it is no longer true. Without this the next sync dedupes
    // against a value the terminal no longer shows and never corrects it.
    crate::osc_status::invalidate();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
    );
    if crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
            )
        );
    }

    if status.map(|s| s.success()).unwrap_or(false) {
        std::fs::read_to_string(&path).unwrap_or_else(|_| content.to_string())
    } else {
        content.to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;

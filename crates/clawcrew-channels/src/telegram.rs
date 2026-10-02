use anyhow::Context;
use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use reqwest::multipart::{Form, Part};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use clawcrew_api::channel::{
    Channel, ChannelMessage, ChannelModelPickerRequest, ListenerHealth, ProgressEvent, SendMessage,
};
use clawcrew_config::schema::{
    Config, DEFAULT_MULTI_MESSAGE_DELAY_MS, StreamMode, TELEGRAM_OFFICIAL_API_BASE_URL,
};
use clawcrew_runtime::i18n;
use clawcrew_runtime::security::pairing::PairingGuard;

/// How long a successful `getUpdates` exchange stays evidence that the listener
/// is working.
///
/// `getUpdates` long-polls with `timeout: 30`, so even an idle-but-healthy
/// channel completes an exchange about every 30 seconds. Three times that
/// leaves room for a slow round trip without letting a blackholed request —
/// which the default runtime client has no timeout to cut short — keep
/// reporting the last success indefinitely.
const POLL_HEALTH_STALE_AFTER: Duration = Duration::from_secs(90);

/// Ceiling on one complete voice-drop notice attempt — both `sendMessage`
/// requests (HTML and the plaintext fallback), their response-body reads, and
/// the inter-chunk pauses.
///
/// The notice is sent from inside the update-processing path, before the
/// permanent skip advances the offset, with a client that has no request
/// timeout. Unbounded, a stalled request or response body would pin the offset
/// and stop the whole listener — the health monitor can report that state but
/// cannot cancel the wait. The drop is permanent either way, so on timeout the
/// notice is abandoned, not retried.
const VOICE_DROP_NOTICE_TIMEOUT: Duration = Duration::from_secs(10);

static ORPHAN_THINK_TAG_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"</?(?:redacted_)?think(?:ing)?>").expect("ORPHAN_THINK_TAG_RE must compile")
});

/// Telegram's maximum message length for text messages
const TELEGRAM_MAX_MESSAGE_LENGTH: usize = 4096;

/// Prefix for synthetic draft ids returned by `send_draft` in MultiMessage mode.
const TELEGRAM_MULTI_MESSAGE_SYNTHETIC_PREFIX: &str = "multi_message_synthetic:";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MultiDraftKey {
    recipient: String,
    draft_id: String,
}

/// Error from `send_text_chunks` that reports how many physical chunks were
/// delivered before a chunk failed on both HTML and plain-text send attempts,
/// so the caller can resume from the first unsent chunk instead of re-sending
/// everything (which would duplicate the chunks Telegram already accepted).
#[derive(Debug)]
struct SendChunksError {
    delivered: usize,
    source: anyhow::Error,
}

impl std::fmt::Display for SendChunksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sent {} chunk(s) then failed: {}",
            self.delivered, self.source
        )
    }
}

#[derive(Debug, Clone)]
struct MultiDraftState {
    /// Sanitized visible text already delivered to Telegram for this draft.
    /// Flushes send only the suffix of `latest_visible` after this prefix, so
    /// both flush paths account against the same canonical string.
    sent_text: String,
    thread_id: Option<String>,
    /// Latest sanitized narration from the orchestrator (not sent until flush).
    latest_visible: String,
    /// Serializes flushes racing between the draft-updater task and the
    /// approval path so the same suffix is never sent twice.
    flush_lock: Arc<tokio::sync::Mutex<()>>,
    /// Completion instant of the last successful send, for inter-message pacing.
    last_sent_at: Option<std::time::Instant>,
    /// Physical chunks of the current unsent suffix already delivered to
    /// Telegram. Reset to 0 once a flush fully delivers its suffix; on a
    /// partial failure this is advanced to the count of chunks that
    /// succeeded, so the next flush resumes instead of re-sending them.
    delivered_chunks: usize,
    /// Concatenation of the first `delivered_chunks` chunk partitions from
    /// the attempt that produced them. Re-validated against the current
    /// split on the next flush before trusting `delivered_chunks` as a skip
    /// count, since tag-rewriting can change what the earlier chunks are.
    delivered_prefix: String,
}

impl MultiDraftState {
    fn new(thread_id: Option<String>) -> Self {
        Self {
            sent_text: String::new(),
            thread_id,
            latest_visible: String::new(),
            flush_lock: Arc::new(tokio::sync::Mutex::new(())),
            last_sent_at: None,
            delivered_chunks: 0,
            delivered_prefix: String::new(),
        }
    }
}

/// Bounded resend attempts for the pending intermediate narration suffix during
/// `finalize_multi_message_draft`. Finalize is the terminal lifecycle event —
/// there is no later production caller to resume a retained draft — so the retry
/// must happen here. A transient Telegram failure resolves within these attempts
/// (in-order delivery preserved); a permanent failure drops the narration with a
/// WARN and still delivers the final answer, rather than stranding unreachable
/// draft state.
const MULTI_MESSAGE_FINALIZE_RETRIES: u32 = 3;

/// Strip think blocks and orphan tag fragments before multi-message delivery.
fn sanitize_multi_message_visible_text(text: &str) -> String {
    let stripped = clawcrew_tool_call_parser::strip_tool_result_blocks(text);
    ORPHAN_THINK_TAG_RE
        .replace_all(&stripped, "")
        .trim()
        .to_string()
}
const TELEGRAM_CONTINUED_PREFIX: &str = "(continued)\n\n";
const TELEGRAM_CONTINUES_SUFFIX: &str = "\n\n(continues...)";
const TELEGRAM_FENCE_REOPEN: &str = "```\n";
const TELEGRAM_FENCE_CLOSE: &str = "```";
const TELEGRAM_ACK_REACTIONS: &[&str] = &["⚡️", "👌", "👀", "🔥", "👍"];
const TELEGRAM_MEDIA_GROUP_SETTLE_DELAY: Duration = Duration::from_millis(700);
const TELEGRAM_IDLE_POLL_TIMEOUT_SECS: u64 = 30;
const TELEGRAM_PENDING_MEDIA_GROUP_POLL_TIMEOUT_SECS: u64 = 1;
const TELEGRAM_POLL_LIMIT: usize = 100;

type MediaGroupKey = (i64, String);

/// Raw, not-yet-dispatched updates for one Telegram media group.
///
/// The map that owns these values is deliberately local to `listen`: it is
/// the canonical transient state only while an album is waiting to settle.
#[derive(Debug)]
struct PendingMediaGroup {
    updates: Vec<serde_json::Value>,
    /// Caption/mention context from album members we will never materialize
    /// (for example a video in a photo/video album). These are deliberately
    /// kept out of `updates` so they cannot drive downloads or image markers,
    /// but their captions still participate in aggregation and the mention
    /// gate -- otherwise an album can lose the user's only caption, or be
    /// silently rejected under `mention_only`.
    unsupported: Vec<UnsupportedMember>,
    last_seen: Instant,
    last_seen_poll_generation: u64,
    /// Set while `getUpdates` returned a full page and an older update is still
    /// unacknowledged, so the offset cannot reach past that page and a later
    /// member of this album cannot be observed yet. Settling here would split
    /// the album into two turns.
    saturated_page_blocked: bool,
}

/// The text-only residue of an album member that will never be downloaded.
///
/// Only what caption aggregation, the mention gate, and album scope validation
/// need is retained; the full `message` object is intentionally dropped so
/// this can never reach `parse_attachment_metadata()` or `getFile`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UnsupportedMember {
    /// Telegram update identity used to preserve acknowledgement ordering.
    update_id: i64,
    /// Kept so unsupported captions interleave with supported ones in the
    /// album's real `message_id` order rather than being appended at the end.
    message_id: i64,
    caption: Option<String>,
    scope: MediaGroupScope,
}

/// Security-relevant scope shared by every member of one Telegram album.
///
/// Optional fields are retained instead of dropping malformed members so the
/// settled batch can fail closed before any attachment download.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MediaGroupScope {
    chat_id: Option<i64>,
    media_group_id: Option<String>,
    thread_id: Option<i64>,
    sender: Option<String>,
}

/// One settled album ready for dispatch: the materializable updates plus the
/// text-only context of its unsupported members.
#[derive(Debug, Clone)]
struct MediaGroupBatch {
    key: MediaGroupKey,
    updates: Vec<serde_json::Value>,
    unsupported: Vec<UnsupportedMember>,
    last_seen: Instant,
    last_seen_poll_generation: u64,
    /// Carried through dispatch so a transient failure restores the album with
    /// the same page-boundary state it had while pending.
    saturated_page_blocked: bool,
}

/// One unacknowledged update in Telegram's global delivery order.
///
/// Ordinary updates own their raw payload here. Media-group members instead
/// reference the listener-local group map, which remains the sole owner of
/// album payloads while they settle.
#[derive(Debug, Clone)]
struct QueuedTelegramUpdate {
    update_id: Option<i64>,
    payload: QueuedTelegramUpdatePayload,
    delivered: bool,
}

#[derive(Debug, Clone)]
enum QueuedTelegramUpdatePayload {
    Ordinary(serde_json::Value),
    MediaGroup(MediaGroupKey),
}

/// Metadata for an incoming document or photo attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IncomingAttachment {
    file_id: String,
    file_name: Option<String>,
    file_size: Option<u64>,
    caption: Option<String>,
    /// Sender-declared MIME type (documents only; Telegram photos carry none).
    mime_type: Option<String>,
    kind: IncomingAttachmentKind,
}

/// The kind of incoming attachment (document vs photo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncomingAttachmentKind {
    Document,
    Photo,
}
const TELEGRAM_BIND_COMMAND: &str = "/bind";
/// Telegram Bot API allows at most 100 commands via setMyCommands.
const TELEGRAM_MAX_BOT_COMMANDS: usize = 100;
/// setMyCommands also refuses on total BODY SIZE, and reports that refusal with the
/// same `BOT_COMMANDS_TOO_MUCH` string it uses for too many commands. The size limit
/// is undocumented, so it is measured: against the live API, a body of 100 commands
/// serializing to 9,714 bytes is accepted and the same shape at 9,814 bytes is
/// refused. 100 commands each carrying a description at
/// `TELEGRAM_COMMAND_DESCRIPTION_MAX_LEN` serializes to roughly 14,800 bytes, so the
/// two per-item caps can both be satisfied and the request still be rejected. Budget
/// well under the measured edge.
const TELEGRAM_MAX_BOT_COMMANDS_BODY_BYTES: usize = 8192;
/// Telegram command names: 1-32 lowercase a-z, 0-9, and underscore.
const TELEGRAM_COMMAND_NAME_MAX_LEN: usize = 32;
/// Telegram command descriptions nominally allow up to 256 characters per the API docs,
/// but empirical testing shows the API returns errors for descriptions substantially
/// longer than 100 characters. This conservative cap avoids that in practice.
const TELEGRAM_COMMAND_DESCRIPTION_MAX_LEN: usize = 100;
const TELEGRAM_MODEL_PICKER_PREFIX: &str = "zcmodel:";
const TELEGRAM_MODEL_PICKER_TTL: Duration = Duration::from_secs(5 * 60);
/// Bounded wait for the runtime to confirm it consumed a picker selection
/// before the callback reports it as queued. Keeps a stuck or stopped
/// consumer from pinning the callback answer forever.
const TELEGRAM_MODEL_PICKER_DELIVERY_ACK_TIMEOUT: Duration = Duration::from_secs(5);
// Leave one Telegram keyboard button for Cancel even if every route belongs
// to a distinct provider alias.
const TELEGRAM_MODEL_PICKER_MAX_OPTIONS: usize = 99;
const TELEGRAM_MODEL_PICKER_MAX_PENDING: usize = 512;
const TELEGRAM_MODEL_PICKER_MAX_FIELD_BYTES: usize = 256;
const TELEGRAM_MODEL_PICKER_BUTTON_CHARS: usize = 64;
const TELEGRAM_MODEL_PICKER_PAGE_SIZE: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelPickerOption {
    hint: String,
    model_provider: String,
    model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelPickerCategory {
    provider_ref: String,
    options: Vec<ModelPickerOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelPickerSelection {
    model_provider: String,
    model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelPickerContext {
    owner_agent_alias: String,
    current: ModelPickerSelection,
    categories: Vec<ModelPickerCategory>,
}

/// Why a configured route is left out of the picker. Every exclusion is
/// logged when the picker is built, so an operator can see which
/// `[[model_routes]]` entries are not selectable and why instead of a route
/// vanishing silently.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ModelPickerExclusion {
    /// A field would not survive the `/model <hint>` command boundary or
    /// the bounded Telegram field size (`is_safe_model_picker_field`).
    UnsafeField,
    /// The route is not an exact configured and live runtime route on a
    /// configured provider.
    Unresolvable,
    /// `/model <hint>` resolves first-match by hint or model identifier,
    /// and that first match is a different route: this target could never
    /// be selected through its own hint.
    ShadowedByRoute { shadowing_hint: String },
    /// The same provider and model is already presented under an earlier
    /// hint; the target stays selectable through that hint.
    DuplicateTarget { presented_as: String },
}

impl ModelPickerExclusion {
    fn reason(&self) -> &'static str {
        match self {
            Self::UnsafeField => "unsafe_field",
            Self::Unresolvable => "unresolvable",
            Self::ShadowedByRoute { .. } => "shadowed_by_route",
            Self::DuplicateTarget { .. } => "duplicate_target",
        }
    }
}

struct ModelPickerPage<'a> {
    options: &'a [ModelPickerOption],
    total_pages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ModelPickerAction {
    OpenCategory { provider_ref: String, page: usize },
    Select(ModelPickerOption),
    Back,
    Cancel,
}

#[derive(Debug, Clone)]
struct PendingModelPicker {
    created_at: Instant,
    expires_at: Instant,
    requesting_user_id: String,
    reply_target: String,
    thread_ts: Option<String>,
    channel_alias: String,
    picker_message_id: i64,
    owner_agent_alias: String,
    current: ModelPickerSelection,
    runtime_routes: Arc<Vec<ModelPickerOption>>,
    action: ModelPickerAction,
}

#[derive(Debug)]
enum ModelPickerCallbackOutcome {
    Queued(Box<ChannelMessage>),
    Rendered {
        text: String,
        reply_markup: serde_json::Value,
    },
    Cancelled,
    Rejected,
}

/// Resolve a localized CLI string by Fluent key, using the process-global active locale.
fn telegram_cli_string(key: &str) -> String {
    i18n::get_required_cli_string(key)
}

/// Sanitize a skill name into a valid Telegram command name.
/// Telegram commands must be 1-32 characters, lowercase a-z, 0-9, underscore only.
fn sanitize_telegram_command_name(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    for ch in raw.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            result.push(lower);
        } else if !result.ends_with('_') {
            // Replace non-alphanumeric with underscore, collapsing consecutive runs.
            result.push('_');
        }
    }

    let trimmed = result.trim_matches('_');
    if trimmed.len() <= TELEGRAM_COMMAND_NAME_MAX_LEN {
        trimmed.to_string()
    } else {
        trimmed[..TELEGRAM_COMMAND_NAME_MAX_LEN]
            .trim_end_matches('_')
            .to_string()
    }
}

/// Truncate a description to the conservative `TELEGRAM_COMMAND_DESCRIPTION_MAX_LEN` cap.
/// The API nominally supports 256 characters, but empirical testing shows errors occur
/// for descriptions substantially longer than 100 characters.
fn truncate_telegram_command_description(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= TELEGRAM_COMMAND_DESCRIPTION_MAX_LEN {
        return trimmed.to_string();
    }
    let mut truncated: String = trimmed
        .chars()
        .take(TELEGRAM_COMMAND_DESCRIPTION_MAX_LEN - 1)
        .collect();
    truncated.push('…');
    truncated
}

/// Serialized length of the `setMyCommands` request body for `commands`.
///
/// A serialization failure cannot make the real body smaller, so it reports 0 and
/// lets the request itself surface the problem rather than dropping every command.
fn telegram_bot_commands_body_len(commands: &[serde_json::Value]) -> usize {
    serde_json::to_string(&serde_json::json!({ "commands": commands })).map_or(0, |s| s.len())
}

/// Drop trailing commands until the `setMyCommands` body fits
/// `TELEGRAM_MAX_BOT_COMMANDS_BODY_BYTES`, and report how many were dropped.
///
/// This is a second cap, applied after the count cap: the two are independent, and
/// a command set inside the count limit can still exceed the size limit.
fn fit_telegram_bot_commands_to_body_budget(commands: &mut Vec<serde_json::Value>) -> usize {
    let mut dropped = 0;
    while !commands.is_empty()
        && telegram_bot_commands_body_len(commands) > TELEGRAM_MAX_BOT_COMMANDS_BODY_BYTES
    {
        commands.pop();
        dropped += 1;
    }
    dropped
}

/// Split a message into chunks that respect Telegram's 4096 character limit.
/// Tries to split at word boundaries when possible, and handles continuation.
/// The split budget includes continuation markers and synthetic code fences
/// exactly as `send_text_chunks` will send them.
fn split_message_for_telegram(message: &str) -> Vec<String> {
    if message.chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;
    let mut in_code_block = false;

    while !remaining.is_empty() {
        let has_previous = !chunks.is_empty();

        if telegram_chunk_send_len(remaining, in_code_block, has_previous, false)
            <= TELEGRAM_MAX_MESSAGE_LENGTH
        {
            let chunk = build_telegram_chunk(remaining, in_code_block, false);
            chunks.push(chunk);
            break;
        }

        let max_take = max_nonfinal_telegram_raw_chars(remaining, in_code_block, has_previous);
        let hard_split = byte_index_after_chars(remaining, max_take);
        let chunk_end = preferred_telegram_split_end(
            remaining,
            hard_split,
            max_take,
            in_code_block,
            has_previous,
        );

        let raw_chunk = &remaining[..chunk_end];
        let starts_in_code_block = in_code_block;
        in_code_block = code_block_state_after(raw_chunk, in_code_block);
        chunks.push(build_telegram_chunk(raw_chunk, starts_in_code_block, true));
        remaining = &remaining[chunk_end..];
    }

    chunks
}

fn build_telegram_chunk(raw_chunk: &str, starts_in_code_block: bool, has_next: bool) -> String {
    let reopen_prefix = if starts_in_code_block {
        TELEGRAM_FENCE_REOPEN
    } else {
        ""
    };
    let ends_in_code_block = code_block_state_after(raw_chunk, starts_in_code_block);
    let needs_synthetic_close = has_next && ends_in_code_block;
    let mut chunk = String::with_capacity(
        reopen_prefix.len()
            + raw_chunk.len()
            + if needs_synthetic_close {
                "\n```".len()
            } else {
                0
            },
    );
    chunk.push_str(reopen_prefix);
    chunk.push_str(raw_chunk);
    if needs_synthetic_close {
        if !chunk.ends_with('\n') {
            chunk.push('\n');
        }
        chunk.push_str(TELEGRAM_FENCE_CLOSE);
    }
    chunk
}

fn format_telegram_text_chunk(chunk: &str, index: usize, total: usize) -> String {
    if total <= 1 {
        return chunk.to_string();
    }

    if index == 0 {
        format!("{chunk}{TELEGRAM_CONTINUES_SUFFIX}")
    } else if index == total - 1 {
        format!("{TELEGRAM_CONTINUED_PREFIX}{chunk}")
    } else {
        format!("{TELEGRAM_CONTINUED_PREFIX}{chunk}{TELEGRAM_CONTINUES_SUFFIX}")
    }
}

fn telegram_chunk_marker_len(has_previous: bool, has_next: bool) -> usize {
    let prefix_len = if has_previous {
        TELEGRAM_CONTINUED_PREFIX.chars().count()
    } else {
        0
    };
    let suffix_len = if has_next {
        TELEGRAM_CONTINUES_SUFFIX.chars().count()
    } else {
        0
    };
    prefix_len + suffix_len
}

fn telegram_chunk_body_len(raw_chunk: &str, starts_in_code_block: bool, has_next: bool) -> usize {
    let reopen_len = if starts_in_code_block {
        TELEGRAM_FENCE_REOPEN.chars().count()
    } else {
        0
    };
    let raw_len = raw_chunk.chars().count();
    let ends_in_code_block = code_block_state_after(raw_chunk, starts_in_code_block);
    let synthetic_close_len = if has_next && ends_in_code_block {
        TELEGRAM_FENCE_CLOSE.chars().count() + usize::from(!raw_chunk.ends_with('\n'))
    } else {
        0
    };

    reopen_len + raw_len + synthetic_close_len
}

fn telegram_chunk_send_len(
    raw_chunk: &str,
    starts_in_code_block: bool,
    has_previous: bool,
    has_next: bool,
) -> usize {
    telegram_chunk_marker_len(has_previous, has_next)
        + telegram_chunk_body_len(raw_chunk, starts_in_code_block, has_next)
}

fn max_nonfinal_telegram_raw_chars(
    remaining: &str,
    starts_in_code_block: bool,
    has_previous: bool,
) -> usize {
    let remaining_chars = remaining.chars().count();
    let marker_len = telegram_chunk_marker_len(has_previous, true);
    let reopen_len = if starts_in_code_block {
        TELEGRAM_FENCE_REOPEN.chars().count()
    } else {
        0
    };
    let upper = remaining_chars
        .saturating_sub(1)
        .min(TELEGRAM_MAX_MESSAGE_LENGTH - marker_len - reopen_len);

    for take in (1..=upper).rev() {
        let end = byte_index_after_chars(remaining, take);
        if telegram_chunk_send_len(&remaining[..end], starts_in_code_block, has_previous, true)
            <= TELEGRAM_MAX_MESSAGE_LENGTH
        {
            return take;
        }
    }

    1
}

fn byte_index_after_chars(s: &str, char_count: usize) -> usize {
    if char_count == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_count)
        .map_or(s.len(), |(idx, _)| idx)
}

fn preferred_telegram_split_end(
    remaining: &str,
    hard_split: usize,
    max_take: usize,
    starts_in_code_block: bool,
    has_previous: bool,
) -> usize {
    let search_area = &remaining[..hard_split];
    let candidate_fits = |end: usize| {
        end > 0
            && end < remaining.len()
            && telegram_chunk_send_len(&remaining[..end], starts_in_code_block, has_previous, true)
                <= TELEGRAM_MAX_MESSAGE_LENGTH
    };

    if let Some(pos) = search_area.rfind('\n') {
        let end = pos + '\n'.len_utf8();
        if search_area[..pos].chars().count() >= max_take / 2 && candidate_fits(end) {
            return end;
        }
    }

    if let Some(pos) = search_area.rfind(' ') {
        let end = pos + ' '.len_utf8();
        if candidate_fits(end) {
            return end;
        }
    }

    hard_split
}

fn code_block_state_after(text: &str, mut in_code_block: bool) -> bool {
    for line in text.split('\n') {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        }
    }
    in_code_block
}

fn pick_uniform_index(len: usize) -> usize {
    debug_assert!(len > 0);
    let upper = len as u64;
    let reject_threshold = (u64::MAX / upper) * upper;

    loop {
        let value = rand::random::<u64>();
        if value < reject_threshold {
            #[allow(clippy::cast_possible_truncation)]
            return (value % upper) as usize;
        }
    }
}

fn random_telegram_ack_reaction() -> &'static str {
    TELEGRAM_ACK_REACTIONS[pick_uniform_index(TELEGRAM_ACK_REACTIONS.len())]
}

fn build_telegram_ack_reaction_request(
    chat_id: &str,
    message_id: i64,
    emoji: &str,
) -> serde_json::Value {
    serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "reaction": [{
            "type": "emoji",
            "emoji": emoji
        }]
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelegramAttachmentKind {
    Image,
    Document,
    Video,
    Audio,
    Voice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelegramAttachment {
    kind: TelegramAttachmentKind,
    target: String,
}

impl TelegramAttachmentKind {
    fn from_marker(marker: &str) -> Option<Self> {
        match marker.trim().to_ascii_uppercase().as_str() {
            "IMAGE" | "PHOTO" => Some(Self::Image),
            "DOCUMENT" | "FILE" => Some(Self::Document),
            "VIDEO" => Some(Self::Video),
            "AUDIO" => Some(Self::Audio),
            "VOICE" => Some(Self::Voice),
            _ => None,
        }
    }
}

fn telegram_audio_send_spec(
    format: &str,
) -> anyhow::Result<(&'static str, &'static str, &'static str, &'static str)> {
    Ok(match format.trim().to_ascii_lowercase().as_str() {
        "opus" | "ogg" => ("sendVoice", "voice", "voice.ogg", "audio/ogg"),
        "mp3" | "mpeg" => ("sendAudio", "audio", "voice.mp3", "audio/mpeg"),
        "wav" => ("sendAudio", "audio", "voice.wav", "audio/wav"),
        "aac" => ("sendAudio", "audio", "voice.aac", "audio/aac"),
        "flac" => ("sendAudio", "audio", "voice.flac", "audio/flac"),
        // Raw PCM is not a container format; reject so the caller reconfigures
        // the TTS provider to emit a supported container format.
        "pcm" => {
            return Err(anyhow::Error::msg(
                "Telegram does not accept raw PCM audio; \
                 configure the TTS provider to output opus, mp3, wav, aac, or flac",
            ));
        }
        _ => (
            "sendAudio",
            "audio",
            "voice.bin",
            "application/octet-stream",
        ),
    })
}

/// Build the user-facing content string for an incoming attachment.
///
/// An attachment earns the `[IMAGE:/path]` marker when the multimodal loader
/// will actually accept it (`provider_loadable_image_mime()`), regardless of
/// whether Telegram delivered it as a photo or a document — so an image sent
/// "as file", even extensionless, is still marked as an image rather than
/// falling back to the `[Document: name] /path` form.
///
/// The check is deliberately the *loadable* one rather than the conservative
/// `looks_like_image()`. A marker the loader rejects is worse than no marker:
/// preparation drops it in favour of a "could not be loaded" note, and the
/// `[Document: ...]` line that would have kept the saved path reachable was
/// never emitted. Formats outside the provider's set therefore stay documents,
/// which leaves both the bytes and a usable path in the model's hands.
/// The disposition Telegram commits to for an inbound attachment, resolved once
/// against the provider's loadability contract.
///
/// `parse_attachment_metadata` only yields documents and photos, so an
/// attachment is either a loadable image the provider will accept or a
/// document. The rendered text and the typed envelope both read this one
/// verdict, so a document the loader would reject cannot be re-decided as an
/// image by a later payload-only classifier.
fn attachment_marker_kind(
    attachment: &clawcrew_api::media::MediaAttachment,
) -> clawcrew_api::media::MarkerKind {
    if attachment.provider_loadable_image_mime().is_some() {
        clawcrew_api::media::MarkerKind::Image
    } else {
        clawcrew_api::media::MarkerKind::Document
    }
}

fn format_attachment_content(
    attachment: &clawcrew_api::media::MediaAttachment,
    local_path: &Path,
) -> String {
    match attachment_marker_kind(attachment) {
        clawcrew_api::media::MarkerKind::Image => format!("[IMAGE:{}]", local_path.display()),
        _ => format!(
            "[Document: {}] {}",
            attachment.file_name,
            local_path.display()
        ),
    }
}

fn safe_attachment_filename(raw: &str) -> String {
    Path::new(raw)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("document")
        .to_string()
}

fn media_group_document_storage_filename(
    display_filename: &str,
    chat_id: &str,
    message_id: i64,
) -> String {
    format!("document_{chat_id}_{message_id}_{display_filename}")
}

fn is_http_url(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://")
}

fn infer_attachment_kind_from_target(target: &str) -> Option<TelegramAttachmentKind> {
    let normalized = target
        .split('?')
        .next()
        .unwrap_or(target)
        .split('#')
        .next()
        .unwrap_or(target);

    let extension = Path::new(normalized)
        .extension()
        .and_then(|ext| ext.to_str())?
        .to_ascii_lowercase();

    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => Some(TelegramAttachmentKind::Image),
        "mp4" | "mov" | "mkv" | "avi" | "webm" => Some(TelegramAttachmentKind::Video),
        "mp3" | "m4a" | "wav" | "flac" => Some(TelegramAttachmentKind::Audio),
        "ogg" | "oga" | "opus" => Some(TelegramAttachmentKind::Voice),
        "pdf" | "txt" | "md" | "csv" | "json" | "zip" | "tar" | "gz" | "doc" | "docx" | "xls"
        | "xlsx" | "ppt" | "pptx" => Some(TelegramAttachmentKind::Document),
        _ => None,
    }
}

fn parse_path_only_attachment(message: &str) -> Option<TelegramAttachment> {
    let trimmed = message.trim();
    if trimmed.is_empty() || trimmed.contains('\n') {
        return None;
    }

    let candidate = trimmed.trim_matches(|c| matches!(c, '`' | '"' | '\''));
    if candidate.chars().any(char::is_whitespace) {
        return None;
    }

    let candidate = candidate.strip_prefix("file://").unwrap_or(candidate);
    let kind = infer_attachment_kind_from_target(candidate)?;

    if !is_http_url(candidate) && !Path::new(candidate).exists() {
        return None;
    }

    Some(TelegramAttachment {
        kind,
        target: candidate.to_string(),
    })
}

/// Delegate to the shared `strip_tool_call_tags` in the orchestrator module.
fn strip_tool_call_tags(message: &str) -> String {
    crate::orchestrator::strip_tool_call_tags(message)
}

fn find_matching_close(s: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_attachment_markers(message: &str) -> (String, Vec<TelegramAttachment>) {
    let mut cleaned = String::with_capacity(message.len());
    let mut attachments = Vec::new();
    let mut cursor = 0;

    while cursor < message.len() {
        let Some(open_rel) = message[cursor..].find('[') else {
            cleaned.push_str(&message[cursor..]);
            break;
        };

        let open = cursor + open_rel;
        cleaned.push_str(&message[cursor..open]);

        let Some(close_rel) = find_matching_close(&message[open + 1..]) else {
            cleaned.push_str(&message[open..]);
            break;
        };

        let close = open + 1 + close_rel;
        let marker = &message[open + 1..close];

        let parsed = marker.split_once(':').and_then(|(kind, target)| {
            let kind = TelegramAttachmentKind::from_marker(kind)?;
            let target = target.trim();
            if target.is_empty() {
                return None;
            }
            Some(TelegramAttachment {
                kind,
                target: target.to_string(),
            })
        });

        if let Some(attachment) = parsed {
            attachments.push(attachment);
        } else {
            cleaned.push_str(&message[open..=close]);
        }

        cursor = close + 1;
    }

    (cleaned.trim().to_string(), attachments)
}

/// Telegram Bot API maximum file download size (20 MB).
const TELEGRAM_MAX_FILE_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;

/// Default minimum interval between Telegram draft edits.
const TELEGRAM_DRAFT_UPDATE_INTERVAL_MS: u64 = 1000;

/// Telegram channel — long-polls the Bot API for updates
pub struct TelegramChannel {
    bot_token: String,
    /// The alias key under `[channels.telegram.<alias>]` this handle is
    /// bound to. Used to scope peer-group writes and resolver lookups.
    alias: String,
    /// Resolves inbound external peers from canonical state at message-time.
    /// No cache (see AGENTS.md "ABSOLUTE RULE — SINGLE SOURCE OF TRUTH").
    peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    persist: Option<Arc<RwLock<Config>>>,
    pairing: Option<PairingGuard>,
    typing_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    stream_mode: StreamMode,
    draft_update_interval_ms: u64,
    last_draft_edit: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    /// Per-draft MultiMessage streaming state keyed by `(recipient, draft_id)`.
    multi_message_drafts: Mutex<std::collections::HashMap<MultiDraftKey, MultiDraftState>>,
    mention_only: bool,
    /// When `false`, group-chat sessions are shared per chat/topic instead of
    /// per sender. See `with_per_user_session`.
    per_user_session: bool,
    passive_group_context: bool,
    bot_username: Mutex<Option<String>>,
    bot_id: Mutex<Option<i64>>,
    /// Outcome of the most recent `getUpdates` exchange and when it completed,
    /// or `None` before the first one. Read by `listener_health` so a
    /// supervisor can tell a connected channel from one that is long-polling a
    /// rejecting endpoint, without issuing a probe of its own.
    ///
    /// The timestamp is load-bearing: a success is only evidence for as long as
    /// [`POLL_HEALTH_STALE_AFTER`], because a request that blackholes leaves the
    /// previous success sitting here forever.
    poll_health: Mutex<Option<(bool, tokio::time::Instant)>>,
    /// Base URL for the Telegram Bot API. Defaults to `https://api.telegram.org`.
    /// Override for local Bot API servers or testing.
    api_base: String,
    transcription: Option<clawcrew_config::schema::TranscriptionConfig>,
    transcription_manager: Option<std::sync::Arc<super::transcription::TranscriptionManager>>,
    voice_transcriptions: Mutex<std::collections::HashMap<String, String>>,
    workspace_dir: Option<std::path::PathBuf>,
    ack_reactions: bool,
    tts_manager: Option<Arc<super::tts::TtsManager>>,
    voice_chats: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Resolves voice peers from canonical config at call-time.
    /// See AGENTS.md "ABSOLUTE RULE — SINGLE SOURCE OF TRUTH" — no cache.
    voice_peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    pending_voice:
        Arc<std::sync::Mutex<std::collections::HashMap<String, (String, std::time::Instant)>>>,
    /// Per-channel proxy URL override.
    proxy_url: Option<String>,
    #[cfg(test)]
    fixture_http_client: Option<reqwest::Client>,
    /// Pre-computed tool command specs (name, description) for bot command registration.
    tool_command_specs: Vec<(String, String)>,
    /// Pending approval requests: callback_data key → oneshot sender.
    /// `listen()` resolves these when a matching `callback_query` arrives.
    pending_approvals:
        Arc<tokio::sync::Mutex<std::collections::HashMap<String, crate::util::PendingApproval>>>,
    /// Opaque, short-lived callback tokens for model-picker keyboards.
    pending_model_pickers: tokio::sync::Mutex<HashMap<String, PendingModelPicker>>,
    /// Seconds to wait for the operator to tap an inline-keyboard button on a
    /// tool approval prompt before auto-denying. Configurable via
    /// `channels.telegram.approval_timeout_secs`. Default: 120.
    approval_timeout_secs: u64,
    /// Bound on one complete voice-drop notice attempt. Always
    /// [`VOICE_DROP_NOTICE_TIMEOUT`] in production; tests shrink it so a
    /// stalled-notice regression does not have to wait out the real ceiling.
    voice_drop_notice_timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditMessageResult {
    Success,
    NotModified,
    Failed(reqwest::StatusCode),
}

/// Outcome of attempting to parse a single incoming Telegram update.
///
/// ⚠️ This enum is a *parser* outcome, not the acknowledgement source of
/// truth. `SkipPermanent` is overloaded: it means both "this parser does not
/// apply, try the next one" and "this update is genuinely, permanently
/// handled" (unauthorized sender, mention gate, duration/size limits, missing
/// config). Those two meanings are only safe to conflate because the parser
/// chain is mutually exclusive and attempted in a fixed order (text → voice →
/// attachment), so a `SkipPermanent` that falls out of the *last* parser is
/// always a genuine permanent skip.
///
/// Whether an update is acknowledged is therefore decided by the listener's
/// ordered acknowledgement queue together with [`UpdateOutcome`].
/// `RetryTransient` is reserved for fallible I/O (file download,
/// transcription, disk writes) so the caller can leave the update
/// unacknowledged and retry it on the next poll instead of silently dropping
/// it.
// `pub(crate)` so orchestrator regressions can receive the disposition returned
// by `try_parse_attachment_message` and unwrap the parsed message.
pub(crate) enum UpdateDisposition {
    // Boxed: `ChannelMessage` is far larger than the unit variants, and this
    // enum is constructed on every incoming update regardless of outcome.
    Parsed(Box<ChannelMessage>),
    SkipPermanent,
    RetryTransient,
}

enum AttachmentMaterialization {
    Ready {
        content: String,
        attachment: clawcrew_api::media::MediaAttachment,
    },
    SkipPermanent,
    RetryTransient,
}

enum MediaGroupDispatchOutcome {
    Delivered(MediaGroupKey),
    Retry(MediaGroupBatch),
    ReceiverClosed,
}

/// Result of routing one queued update through its delivery path.
///
/// Both the startup/restart probe and the main long-poll loop use the same
/// queue. The offset moves only across its delivered prefix, never while a
/// transient failure or dropped receiver could still cause an update to be
/// lost.
enum UpdateOutcome {
    /// The update was delivered or permanently skipped; the queue may mark it
    /// complete and keep processing the batch.
    Advanced,
    /// A transient failure occurred. The caller should stop processing the
    /// rest of this batch so the next poll retries starting at the
    /// still-unadvanced offset.
    StopBatch,
    /// The channel receiver has been dropped; the whole listen loop must
    /// exit immediately.
    ReceiverClosed,
}

/// Why a Telegram `getFile` lookup failed, classified for retry purposes.
///
/// The offset repair in this PR only helps if a failure that can never
/// succeed is distinguished from one that can. Telegram answers an invalid or
/// expired `file_id` with `200 OK` and an `ok: false` envelope carrying
/// `error_code: 400`; treating that as transient head-of-line blocks every
/// later update indefinitely, because the offset never advances past an
/// update whose download will never succeed.
///
/// Classification is deliberately conservative: only a confidently permanent
/// vendor rejection is `Permanent`. It requires structured evidence from the
/// Bot API itself — `ok: false` *and* an `error_code` on an explicit
/// allowlist of terminal conditions this implementation can substantiate.
/// Every other response — 5xx, 429, 408, state-dependent or unrecognised 4xx
/// codes, transport errors, malformed bodies, body-less non-2xx responses —
/// stays `Transient`, because retrying a recoverable failure is safe while
/// skipping a recoverable one loses a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileLookupFailure {
    /// Retrying may succeed: 5xx, 429, 408, any 4xx outside the terminal
    /// allowlist, transport failure, malformed, body-less, or otherwise
    /// unrecognised response.
    Transient,
    /// Retrying can never succeed: an explicit `ok: false` carrying an
    /// `error_code` on the terminal allowlist (invalid/expired file id, file
    /// too big, forbidden). Safe to acknowledge and move past.
    Permanent,
}

/// Why a voice message was dropped for good, and what its sender is told.
///
/// A voice note that disappears without a word is indistinguishable, from the
/// sender's side, from a bot that never heard them: the message was delivered,
/// no answer came, and no reason was given. Every permanent drop therefore
/// carries a short human sentence. Transient failures are deliberately absent:
/// the update is retried from the same offset, so a notice would be sent again
/// on every attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceDropReason {
    /// The recording is longer than `transcription.max_duration_secs`.
    TooLong { limit_secs: u64 },
    /// Telegram will never hand us this file: expired id, too big, forbidden.
    FileUnavailable,
    /// Transcription succeeded but produced nothing usable — silence, noise.
    EmptyTranscript,
}

impl VoiceDropReason {
    /// The sentence the sender sees, resolved through the Fluent catalogue
    /// like every other user-facing channel string. Vendor and engine
    /// diagnostics stay in the log: the sender gets the reason, never the
    /// internals.
    ///
    /// The wording is deliberately generic over voice notes and audio
    /// uploads — this parser accepts both — and the advice has to survive the
    /// causes it cannot distinguish: a permanent retrieval failure includes
    /// files Telegram refuses as too big, where "send it again" would invite
    /// the sender to hit the same wall twice.
    pub(crate) fn notice(self) -> String {
        match self {
            Self::TooLong { limit_secs } => {
                let limit_secs = limit_secs.to_string();
                i18n::get_required_cli_string_with_args(
                    "channel-telegram-voice-drop-too-long",
                    &[("limit_secs", limit_secs.as_str())],
                )
            }
            Self::FileUnavailable => {
                i18n::get_required_cli_string("channel-telegram-voice-drop-file-unavailable")
            }
            Self::EmptyTranscript => {
                i18n::get_required_cli_string("channel-telegram-voice-drop-empty-transcript")
            }
        }
    }
}

/// A `getFile` failure with the vendor diagnostics preserved.
///
/// The previous code mapped every failure to a single generic
/// "missing file_path in response" string, discarding Telegram's
/// `error_code` and `description` — the exact evidence an operator needs to
/// tell an expired file id from an outage.
#[derive(Debug)]
pub(crate) struct FileLookupError {
    pub(crate) kind: FileLookupFailure,
    pub(crate) message: String,
}

impl std::fmt::Display for FileLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl FileLookupError {
    fn transient(message: impl Into<String>) -> Self {
        Self {
            kind: FileLookupFailure::Transient,
            message: message.into(),
        }
    }

    fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: FileLookupFailure::Permanent,
            message: message.into(),
        }
    }

    /// Map a `getFile` response onto a classified failure.
    ///
    /// `status` is the HTTP status; `body` is the parsed JSON envelope when
    /// one could be parsed. Telegram returns errors both as non-2xx statuses
    /// and as `200 OK` with `ok: false`, so both shapes are inspected.
    ///
    /// Only `error_code` values on an explicit allowlist of substantiated
    /// terminal conditions are `Permanent`; every other structured code is
    /// left `Transient` so an uncommon or future recoverable rejection can
    /// never silently consume the update.
    pub(crate) fn classify(status: reqwest::StatusCode, body: Option<&serde_json::Value>) -> Self {
        let ok_flag = body
            .and_then(|b| b.get("ok"))
            .and_then(serde_json::Value::as_bool);
        let error_code = body
            .and_then(|b| b.get("error_code"))
            .and_then(serde_json::Value::as_i64);
        let description = body
            .and_then(|b| b.get("description"))
            .and_then(serde_json::Value::as_str);

        let detail = format!(
            "Telegram getFile failed (http {}, error_code {}, ok {}): {}",
            status.as_u16(),
            error_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".to_string()),
            ok_flag
                .map(|b| b.to_string())
                .unwrap_or_else(|| "-".to_string()),
            description.unwrap_or("no description"),
        );

        // The only `error_code` values this implementation can substantiate
        // as terminal for `getFile`:
        //
        // * 400 Bad Request — invalid, expired, or malformed `file_id`, and
        //   "file is too big". The same `file_id` can never resolve later.
        // * 403 Forbidden — the bot lost access to the file's chat. Retrying
        //   the lookup with the same credentials cannot regain it.
        //
        // Everything else stays transient *by construction*. A 4xx status
        // does not prove permanence: HTTP defines state-dependent and
        // explicitly retryable 4xx conditions (409 Conflict, 425 Too Early),
        // Telegram documents `error_code` contents as subject to change, and
        // a future or uncommon code could well be recoverable. Codes that are
        // global rather than per-update — 401 (bad token), 404 (unknown
        // method) — are also left transient: they resolve when an operator
        // fixes the deployment, and acknowledging updates in the meantime
        // would discard them permanently.
        const TERMINAL_ERROR_CODES: [i64; 2] = [400, 403];

        // Permanence requires *structured vendor evidence* of a terminal
        // rejection: Telegram must both mark the call failed (`ok: false`)
        // and name a reason on the allowlist above. A bare HTTP status is not
        // enough — a body-less or malformed 4xx can come from an
        // intermediary rather than the Bot API. Guessing permanence there
        // would acknowledge and discard the update this path exists to
        // preserve, so anything unrecognised stays transient and is retried
        // instead.
        let terminal_rejection = ok_flag == Some(false)
            && error_code
                .map(|c| TERMINAL_ERROR_CODES.contains(&c))
                .unwrap_or(false);

        if terminal_rejection {
            Self::permanent(detail)
        } else {
            Self::transient(detail)
        }
    }
}

fn normalize_telegram_api_base(api_base: &str) -> String {
    api_base.trim_end_matches('/').to_string()
}

impl TelegramChannel {
    fn is_safe_model_picker_field(value: &str) -> bool {
        // Hints are interpolated into `/model <hint>` command syntax, which
        // normalizes whitespace and strips backticks before matching routes,
        // and treats a leading `--` token as a scope flag (`--user`,
        // `--agent`) or falls back to the help ladder. Only canonical hints
        // may enter the picker: already trimmed, single spaces between
        // tokens, no backticks, and no `--`-prefixed token anywhere. That
        // keeps every displayed route inside the session-scoped command
        // domain and guarantees the selection command round-trips back to
        // the exact configured route instead of a literal model write.
        !value.is_empty()
            && value.len() <= TELEGRAM_MODEL_PICKER_MAX_FIELD_BYTES
            && !value.chars().any(char::is_control)
            && !value.contains('`')
            && value.split_whitespace().collect::<Vec<_>>().join(" ") == value
            && !value
                .split_whitespace()
                .any(|token| token.starts_with("--"))
    }

    fn configured_model_provider(config: &Config, provider_ref: &str) -> bool {
        provider_ref
            .split_once('.')
            .filter(|(family, alias)| !family.is_empty() && !alias.is_empty())
            .is_some_and(|(family, alias)| config.providers.models.find(family, alias).is_some())
    }

    /// Mirror of the text resolver's match rule (`apply_model_ref`): a
    /// `/model <hint>` argument matches a route by model identifier or by
    /// hint, ASCII case-insensitively, first match wins.
    fn model_picker_route_matches_argument(hint: &str, model: &str, argument: &str) -> bool {
        model.eq_ignore_ascii_case(argument) || hint.eq_ignore_ascii_case(argument)
    }

    /// Classify why `selected` cannot be offered, or `None` when selecting
    /// it through `/model <hint>` resolves to exactly this route.
    fn model_picker_route_exclusion(
        config: &Config,
        runtime_routes: &[ModelPickerOption],
        selected: &ModelPickerOption,
    ) -> Option<ModelPickerExclusion> {
        if !Self::is_safe_model_picker_field(&selected.hint)
            || !Self::is_safe_model_picker_field(&selected.model_provider)
            || !Self::is_safe_model_picker_field(&selected.model)
        {
            return Some(ModelPickerExclusion::UnsafeField);
        }
        let is_selected_route = |route: &clawcrew_config::schema::ModelRouteConfig| {
            route.hint == selected.hint
                && route.model_provider == selected.model_provider
                && route.model == selected.model
        };
        if !Self::configured_model_provider(config, &selected.model_provider)
            || !config.model_routes.iter().any(&is_selected_route)
            || !runtime_routes.iter().any(|route| route == selected)
        {
            return Some(ModelPickerExclusion::Unresolvable);
        }
        // The selection command is `/model <hint>`, resolved first-match
        // against the configured routes and the live runtime routes alike.
        // If either list resolves the hint to a different route first, this
        // target is unreachable through its own hint and must not be shown.
        let shadowing_configured = config
            .model_routes
            .iter()
            .find(|route| {
                Self::model_picker_route_matches_argument(&route.hint, &route.model, &selected.hint)
            })
            .filter(|route| !is_selected_route(route))
            .map(|route| route.hint.clone());
        let shadowing_runtime = runtime_routes
            .iter()
            .find(|route| {
                Self::model_picker_route_matches_argument(&route.hint, &route.model, &selected.hint)
            })
            .filter(|route| *route != selected)
            .map(|route| route.hint.clone());
        shadowing_configured
            .or(shadowing_runtime)
            .map(|shadowing_hint| ModelPickerExclusion::ShadowedByRoute { shadowing_hint })
    }

    fn model_picker_route_resolves_to(
        config: &Config,
        runtime_routes: &[ModelPickerOption],
        selected: &ModelPickerOption,
    ) -> bool {
        Self::model_picker_route_exclusion(config, runtime_routes, selected).is_none()
    }

    /// Operator-visible record of a route the picker will not offer. Lossy
    /// exclusions (the target is unreachable) are warnings that name the
    /// `[[model_routes]]` entry; a deduplicated target is only a debug
    /// note because it stays selectable through the hint shown first.
    fn log_model_picker_exclusion(
        channel_alias: &str,
        route_index: usize,
        route: &ModelPickerOption,
        exclusion: &ModelPickerExclusion,
    ) {
        let mut attrs = ::serde_json::json!({
            "channel_alias": channel_alias,
            "route_index": route_index,
            "reason": exclusion.reason(),
        });
        // A route that failed the field check may carry control characters
        // or oversized values: identify it by position only.
        if !matches!(exclusion, ModelPickerExclusion::UnsafeField) {
            attrs["hint"] = ::serde_json::Value::String(route.hint.clone());
            attrs["model_provider"] = ::serde_json::Value::String(route.model_provider.clone());
            attrs["model"] = ::serde_json::Value::String(route.model.clone());
        }
        match exclusion {
            ModelPickerExclusion::ShadowedByRoute { shadowing_hint }
                if Self::is_safe_model_picker_field(shadowing_hint) =>
            {
                attrs["shadowing_hint"] = ::serde_json::Value::String(shadowing_hint.clone());
            }
            ModelPickerExclusion::ShadowedByRoute { .. } => {
                attrs["shadowing_hint_unsafe"] = ::serde_json::Value::Bool(true);
            }
            ModelPickerExclusion::DuplicateTarget { presented_as } => {
                attrs["presented_as"] = ::serde_json::Value::String(presented_as.clone());
            }
            ModelPickerExclusion::UnsafeField | ModelPickerExclusion::Unresolvable => {}
        }
        if matches!(exclusion, ModelPickerExclusion::DuplicateTarget { .. }) {
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(attrs),
                "Telegram model picker presents an already shown target once"
            );
        } else {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(attrs),
                "Telegram model picker excluded a configured route; rename or fix it in config.toml"
            );
        }
    }

    fn model_picker_context(
        config: &Config,
        channel_alias: &str,
        runtime_routes: &[ModelPickerOption],
    ) -> Option<ModelPickerContext> {
        config
            .channels
            .telegram
            .get(channel_alias)
            .filter(|channel| channel.enabled)?;

        let channel_ref = format!("telegram.{channel_alias}");
        let owner_alias = config.agent_for_channel(&channel_ref)?;
        let mut owners = config.agents.iter().filter(|(_, agent)| {
            agent.enabled
                && agent
                    .channels
                    .iter()
                    .any(|bound| bound.as_str() == channel_ref)
        });
        let (unique_owner_alias, owner) = owners.next()?;
        if unique_owner_alias != owner_alias || owners.next().is_some() {
            return None;
        }

        let current_provider = owner.model_provider.as_str();
        let (current_family, current_alias) = current_provider.split_once('.')?;
        let current_model = config
            .providers
            .models
            .find(current_family, current_alias)?
            .model
            .clone()
            .unwrap_or_default();

        let mut categories: Vec<ModelPickerCategory> = Vec::new();
        // Presented (provider, model) targets and the hint each is shown
        // under; its length is the number of options offered so far.
        let mut presented_targets: HashMap<(String, String), String> = HashMap::new();
        for (route_index, route) in runtime_routes.iter().enumerate() {
            if presented_targets.len() >= TELEGRAM_MODEL_PICKER_MAX_OPTIONS {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "channel_alias": channel_alias,
                            "max_options": TELEGRAM_MODEL_PICKER_MAX_OPTIONS,
                            "routes_not_evaluated": runtime_routes.len() - route_index,
                        })),
                    "Telegram model picker option cap reached; remaining routes are not shown"
                );
                break;
            }
            let target = (route.model_provider.clone(), route.model.clone());
            let exclusion = Self::model_picker_route_exclusion(config, runtime_routes, route)
                .or_else(|| {
                    presented_targets.get(&target).map(|presented_as| {
                        ModelPickerExclusion::DuplicateTarget {
                            presented_as: presented_as.clone(),
                        }
                    })
                });
            if let Some(exclusion) = exclusion {
                Self::log_model_picker_exclusion(channel_alias, route_index, route, &exclusion);
                continue;
            }
            presented_targets.insert(target, route.hint.clone());
            let option = route.clone();
            if let Some(category) = categories
                .iter_mut()
                .find(|category| category.provider_ref == route.model_provider)
            {
                category.options.push(option);
            } else {
                categories.push(ModelPickerCategory {
                    provider_ref: route.model_provider.clone(),
                    options: vec![option],
                });
            }
        }
        if categories.is_empty() {
            return None;
        }

        Some(ModelPickerContext {
            owner_agent_alias: unique_owner_alias.to_string(),
            current: ModelPickerSelection {
                model_provider: current_provider.to_string(),
                model: current_model,
            },
            categories,
        })
    }

    fn model_picker_callback_data(token: &str) -> Option<String> {
        if uuid::Uuid::parse_str(token).ok()?.to_string() != token {
            return None;
        }
        let data = format!("{TELEGRAM_MODEL_PICKER_PREFIX}{token}");
        (data.len() <= 64).then_some(data)
    }

    fn parse_model_picker_callback_data(data: &str) -> Option<&str> {
        if data.len() > 64 {
            return None;
        }
        let token = data.strip_prefix(TELEGRAM_MODEL_PICKER_PREFIX)?;
        if uuid::Uuid::parse_str(token).ok()?.to_string() != token {
            return None;
        }
        Some(token)
    }

    fn model_picker_page(
        category: &ModelPickerCategory,
        page: usize,
    ) -> Option<ModelPickerPage<'_>> {
        let total_pages = category
            .options
            .len()
            .div_ceil(TELEGRAM_MODEL_PICKER_PAGE_SIZE);
        if total_pages == 0 || page >= total_pages {
            return None;
        }
        let start = page * TELEGRAM_MODEL_PICKER_PAGE_SIZE;
        let end = (start + TELEGRAM_MODEL_PICKER_PAGE_SIZE).min(category.options.len());
        Some(ModelPickerPage {
            options: &category.options[start..end],
            total_pages,
        })
    }

    fn model_picker_route_available(
        config: &Config,
        runtime_routes: &[ModelPickerOption],
        selected: &ModelPickerOption,
    ) -> bool {
        Self::model_picker_route_resolves_to(config, runtime_routes, selected)
    }

    fn model_picker_selection_command(selected: &ModelPickerOption) -> String {
        format!("/model {}", selected.hint)
    }

    fn truncate_model_picker_button(text: &str) -> String {
        if text.chars().count() <= TELEGRAM_MODEL_PICKER_BUTTON_CHARS {
            return text.to_string();
        }
        let mut truncated = text
            .chars()
            .take(TELEGRAM_MODEL_PICKER_BUTTON_CHARS - 1)
            .collect::<String>();
        truncated.push('…');
        truncated
    }

    fn model_picker_provider_label(provider_ref: &str) -> String {
        provider_ref.to_string()
    }

    fn model_picker_category_reply_markup(
        buttons: &[(String, &ModelPickerCategory)],
        cancel_token: &str,
        current: &ModelPickerSelection,
    ) -> Option<serde_json::Value> {
        let mut rows = buttons
            .chunks(2)
            .map(|chunk| {
                chunk
                    .iter()
                    .map(|(token, category)| {
                        let callback_data = Self::model_picker_callback_data(token)?;
                        let selected = category.provider_ref == current.model_provider
                            && category
                                .options
                                .iter()
                                .any(|option| option.model == current.model);
                        let marker = if selected { "✓ " } else { "" };
                        Some(serde_json::json!({
                            "text": Self::truncate_model_picker_button(&format!(
                                "{marker}{} ({})",
                                Self::model_picker_provider_label(&category.provider_ref),
                                category.options.len(),
                            )),
                            "callback_data": callback_data,
                        }))
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>()?;
        rows.push(vec![serde_json::json!({
            "text": format!("✗ {}", i18n::get_required_cli_string("channel-telegram-model-picker-cancel")),
            "callback_data": Self::model_picker_callback_data(cancel_token)?,
        })]);
        Some(serde_json::json!({ "inline_keyboard": rows }))
    }

    fn model_picker_models_reply_markup(
        buttons: &[(String, ModelPickerOption)],
        current: &ModelPickerSelection,
        page: usize,
        total_pages: usize,
        indicator_token: &str,
        previous_token: Option<&str>,
        next_token: Option<&str>,
        back_token: &str,
        cancel_token: &str,
    ) -> Option<serde_json::Value> {
        let mut rendered = Vec::with_capacity(buttons.len());
        for (token, option) in buttons {
            let selected =
                option.model_provider == current.model_provider && option.model == current.model;
            let marker = if selected { "✓ " } else { "" };
            rendered.push(serde_json::json!({
                "text": Self::truncate_model_picker_button(&format!("{marker}{}", option.model)),
                "callback_data": Self::model_picker_callback_data(token)?,
            }));
        }
        let mut rows = rendered
            .chunks(2)
            .map(<[serde_json::Value]>::to_vec)
            .collect::<Vec<_>>();
        if total_pages > 1 {
            let mut nav = Vec::new();
            if let Some(token) = previous_token {
                nav.push(serde_json::json!({
                    "text": i18n::get_required_cli_string("channel-telegram-model-picker-previous"),
                    "callback_data": Self::model_picker_callback_data(token)?,
                }));
            }
            nav.push(serde_json::json!({
                "text": format!("{}/{}", page + 1, total_pages),
                "callback_data": Self::model_picker_callback_data(indicator_token)?,
            }));
            if let Some(token) = next_token {
                nav.push(serde_json::json!({
                    "text": i18n::get_required_cli_string("channel-telegram-model-picker-next"),
                    "callback_data": Self::model_picker_callback_data(token)?,
                }));
            }
            rows.push(nav);
        }
        rows.push(vec![
            serde_json::json!({
                "text": i18n::get_required_cli_string("channel-telegram-model-picker-back"),
                "callback_data": Self::model_picker_callback_data(back_token)?,
            }),
            serde_json::json!({
                "text": format!("✗ {}", i18n::get_required_cli_string("channel-telegram-model-picker-cancel")),
                "callback_data": Self::model_picker_callback_data(cancel_token)?,
            }),
        ]);
        Some(serde_json::json!({ "inline_keyboard": rows }))
    }

    fn model_picker_selection_message(
        state: &PendingModelPicker,
        current_sender: String,
    ) -> Option<ChannelMessage> {
        let ModelPickerAction::Select(selected) = &state.action else {
            return None;
        };
        Some(ChannelMessage {
            id: format!("telegram_model_picker_{}", uuid::Uuid::new_v4()),
            sender: current_sender,
            platform_sender_id: Some(state.requesting_user_id.clone()),
            reply_target: state.reply_target.clone(),
            content: Self::model_picker_selection_command(selected),
            channel: "telegram".into(),
            channel_alias: Some(state.channel_alias.clone()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: state.thread_ts.clone(),
            ..Default::default()
        })
    }

    async fn insert_pending_model_picker_batch(&self, entries: Vec<(String, PendingModelPicker)>) {
        let mut pending = self.pending_model_pickers.lock().await;
        Self::reserve_pending_model_picker_capacity(&mut pending, entries.len());
        pending.extend(entries);
    }

    async fn remove_pending_model_picker_keyboard(&self, anchor: &PendingModelPicker) {
        self.pending_model_pickers
            .lock()
            .await
            .retain(|_, state| !Self::same_model_picker_keyboard(anchor, state));
    }

    fn reserve_pending_model_picker_capacity(
        pending: &mut HashMap<String, PendingModelPicker>,
        additional: usize,
    ) {
        let now = Instant::now();
        let expired_keyboards = pending
            .values()
            .filter(|candidate| candidate.expires_at < now)
            .cloned()
            .collect::<Vec<_>>();
        pending.retain(|_, candidate| {
            !expired_keyboards
                .iter()
                .any(|expired| Self::same_model_picker_keyboard(expired, candidate))
        });
        while pending.len().saturating_add(additional) > TELEGRAM_MODEL_PICKER_MAX_PENDING {
            let Some(oldest) = pending
                .values()
                .min_by_key(|candidate| candidate.created_at)
                .cloned()
            else {
                break;
            };
            pending.retain(|_, candidate| !Self::same_model_picker_keyboard(&oldest, candidate));
        }
    }

    fn same_model_picker_keyboard(left: &PendingModelPicker, right: &PendingModelPicker) -> bool {
        left.requesting_user_id == right.requesting_user_id
            && left.reply_target == right.reply_target
            && left.thread_ts == right.thread_ts
            && left.channel_alias == right.channel_alias
            && left.picker_message_id == right.picker_message_id
            && left.owner_agent_alias == right.owner_agent_alias
    }

    async fn model_picker_keyboard_snapshot(
        &self,
        callback: &serde_json::Value,
    ) -> Vec<(String, PendingModelPicker)> {
        let Some(token) = callback
            .get("data")
            .and_then(serde_json::Value::as_str)
            .and_then(Self::parse_model_picker_callback_data)
        else {
            return Vec::new();
        };
        let pending = self.pending_model_pickers.lock().await;
        let Some(anchor) = pending.get(token) else {
            return Vec::new();
        };
        pending
            .iter()
            .filter(|(_, state)| Self::same_model_picker_keyboard(anchor, state))
            .map(|(token, state)| (token.clone(), state.clone()))
            .collect()
    }

    async fn restore_model_picker_keyboard(&self, snapshot: Vec<(String, PendingModelPicker)>) {
        let Some((_, anchor)) = snapshot.first() else {
            return;
        };
        let now = Instant::now();
        let mut pending = self.pending_model_pickers.lock().await;
        pending.retain(|_, state| !Self::same_model_picker_keyboard(anchor, state));
        let valid = snapshot
            .into_iter()
            .filter(|(_, state)| state.expires_at >= now)
            .collect::<Vec<_>>();
        Self::reserve_pending_model_picker_capacity(&mut pending, valid.len());
        pending.extend(valid);
    }

    fn callback_model_picker_reply_target(callback: &serde_json::Value) -> Option<String> {
        let message = callback.get("message")?;
        let chat_id = message.get("chat")?.get("id")?.as_i64()?.to_string();
        let thread_id = message
            .get("message_thread_id")
            .and_then(serde_json::Value::as_i64);
        Some(thread_id.map_or(chat_id.clone(), |thread| format!("{chat_id}:{thread}")))
    }

    fn callback_model_picker_thread(callback: &serde_json::Value) -> Option<String> {
        callback
            .get("message")?
            .get("message_thread_id")?
            .as_i64()
            .map(|thread| thread.to_string())
    }

    fn callback_model_picker_message_id(callback: &serde_json::Value) -> Option<i64> {
        callback.get("message")?.get("message_id")?.as_i64()
    }

    fn callback_model_picker_user_id(callback: &serde_json::Value) -> Option<String> {
        callback
            .get("from")?
            .get("id")?
            .as_i64()
            .map(|id| id.to_string())
    }

    fn telegram_sender_identity(from: &serde_json::Value) -> Option<String> {
        let username = from
            .get("username")
            .and_then(serde_json::Value::as_str)
            .filter(|value| *value != "unknown")
            .map(str::to_string);
        username.or_else(|| from.get("id")?.as_i64().map(|id| id.to_string()))
    }

    fn model_picker_state_matches_callback(
        &self,
        state: &PendingModelPicker,
        callback: &serde_json::Value,
    ) -> bool {
        state.expires_at >= Instant::now()
            && state.requesting_user_id
                == Self::callback_model_picker_user_id(callback).unwrap_or_default()
            && state.channel_alias == self.alias
            && Self::callback_model_picker_reply_target(callback).as_deref()
                == Some(state.reply_target.as_str())
            && Self::callback_model_picker_thread(callback) == state.thread_ts
            && Self::callback_model_picker_message_id(callback) == Some(state.picker_message_id)
            && self.model_picker_callback_user_is_allowed(callback)
    }

    fn model_picker_callback_user_is_allowed(&self, callback: &serde_json::Value) -> bool {
        let Some(from) = callback.get("from") else {
            return false;
        };
        let username = from
            .get("username")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let user_id = from
            .get("id")
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());
        let mut identities = vec![username];
        if let Some(user_id) = user_id.as_deref() {
            identities.push(user_id);
        }
        self.is_any_user_allowed(identities)
    }

    async fn prevalidate_model_picker_callback(&self, callback: &serde_json::Value) -> bool {
        let Some(token) = callback
            .get("data")
            .and_then(serde_json::Value::as_str)
            .and_then(Self::parse_model_picker_callback_data)
        else {
            return false;
        };
        let state = {
            let mut pending = self.pending_model_pickers.lock().await;
            let Some(state) = pending.get(token).cloned() else {
                return false;
            };
            if state.expires_at < Instant::now() {
                pending.remove(token);
                return false;
            }
            state
        };
        if !self.model_picker_state_matches_callback(&state, callback) {
            return false;
        }
        let Some(config) = &self.persist else {
            return false;
        };
        let live = config.read();
        let Some(context) =
            Self::model_picker_context(&live, &self.alias, state.runtime_routes.as_ref())
        else {
            return false;
        };
        if context.owner_agent_alias != state.owner_agent_alias {
            return false;
        }
        match &state.action {
            ModelPickerAction::OpenCategory { provider_ref, page } => context
                .categories
                .iter()
                .find(|category| category.provider_ref == *provider_ref)
                .and_then(|category| Self::model_picker_page(category, *page))
                .is_some(),
            ModelPickerAction::Select(option) => {
                Self::model_picker_route_available(&live, state.runtime_routes.as_ref(), option)
            }
            ModelPickerAction::Back | ModelPickerAction::Cancel => true,
        }
    }

    async fn model_picker_callback_requires_queue(&self, callback: &serde_json::Value) -> bool {
        let Some(token) = callback
            .get("data")
            .and_then(serde_json::Value::as_str)
            .and_then(Self::parse_model_picker_callback_data)
        else {
            return false;
        };
        self.pending_model_pickers
            .lock()
            .await
            .get(token)
            .is_some_and(|state| matches!(state.action, ModelPickerAction::Select(_)))
    }

    async fn process_model_picker_callback(
        &self,
        callback: &serde_json::Value,
    ) -> ModelPickerCallbackOutcome {
        let Some(token) = callback
            .get("data")
            .and_then(serde_json::Value::as_str)
            .and_then(Self::parse_model_picker_callback_data)
        else {
            return ModelPickerCallbackOutcome::Rejected;
        };
        let Some(config) = &self.persist else {
            return ModelPickerCallbackOutcome::Rejected;
        };
        let mut pending = self.pending_model_pickers.lock().await;
        let Some(state) = pending.get(token).cloned() else {
            return ModelPickerCallbackOutcome::Rejected;
        };
        if !self.model_picker_state_matches_callback(&state, callback) {
            return ModelPickerCallbackOutcome::Rejected;
        }
        let context = {
            let live = config.read();
            let Some(mut context) =
                Self::model_picker_context(&live, &self.alias, state.runtime_routes.as_ref())
            else {
                return ModelPickerCallbackOutcome::Rejected;
            };
            if context.owner_agent_alias != state.owner_agent_alias {
                return ModelPickerCallbackOutcome::Rejected;
            }
            if let ModelPickerAction::Select(option) = &state.action
                && !Self::model_picker_route_available(&live, state.runtime_routes.as_ref(), option)
            {
                return ModelPickerCallbackOutcome::Rejected;
            }
            context.current = state.current.clone();
            context
        };

        pending.retain(|_, candidate| {
            candidate.channel_alias != state.channel_alias
                || candidate.reply_target != state.reply_target
                || candidate.thread_ts != state.thread_ts
                || candidate.picker_message_id != state.picker_message_id
                || candidate.requesting_user_id != state.requesting_user_id
        });

        match &state.action {
            ModelPickerAction::Select(_) => callback
                .get("from")
                .and_then(Self::telegram_sender_identity)
                .and_then(|current_sender| {
                    Self::model_picker_selection_message(&state, current_sender)
                })
                .map_or(ModelPickerCallbackOutcome::Rejected, |message| {
                    ModelPickerCallbackOutcome::Queued(Box::new(message))
                }),
            ModelPickerAction::Cancel => ModelPickerCallbackOutcome::Cancelled,
            ModelPickerAction::Back => {
                let buttons = context
                    .categories
                    .iter()
                    .map(|category| (uuid::Uuid::new_v4().to_string(), category))
                    .collect::<Vec<_>>();
                let cancel_token = uuid::Uuid::new_v4().to_string();
                let Some(reply_markup) = Self::model_picker_category_reply_markup(
                    &buttons,
                    &cancel_token,
                    &context.current,
                ) else {
                    return ModelPickerCallbackOutcome::Rejected;
                };
                Self::reserve_pending_model_picker_capacity(
                    &mut pending,
                    buttons.len().saturating_add(1),
                );
                for (token, category) in buttons {
                    pending.insert(
                        token,
                        PendingModelPicker {
                            action: ModelPickerAction::OpenCategory {
                                provider_ref: category.provider_ref.clone(),
                                page: 0,
                            },
                            ..state.clone()
                        },
                    );
                }
                pending.insert(
                    cancel_token,
                    PendingModelPicker {
                        action: ModelPickerAction::Cancel,
                        ..state.clone()
                    },
                );
                ModelPickerCallbackOutcome::Rendered {
                    text: i18n::get_required_cli_string_with_args(
                        "channel-telegram-model-picker-provider-title",
                        &[
                            ("provider", context.current.model_provider.as_str()),
                            ("model", context.current.model.as_str()),
                        ],
                    ),
                    reply_markup,
                }
            }
            ModelPickerAction::OpenCategory { provider_ref, page } => {
                let Some(category) = context
                    .categories
                    .iter()
                    .find(|category| category.provider_ref == *provider_ref)
                else {
                    return ModelPickerCallbackOutcome::Rejected;
                };
                let Some(paged) = Self::model_picker_page(category, *page) else {
                    return ModelPickerCallbackOutcome::Rejected;
                };
                let buttons = paged
                    .options
                    .iter()
                    .cloned()
                    .map(|option| (uuid::Uuid::new_v4().to_string(), option))
                    .collect::<Vec<_>>();
                let previous_token = (*page > 0).then(|| uuid::Uuid::new_v4().to_string());
                let next_token =
                    (*page + 1 < paged.total_pages).then(|| uuid::Uuid::new_v4().to_string());
                let indicator_token = uuid::Uuid::new_v4().to_string();
                let back_token = uuid::Uuid::new_v4().to_string();
                let cancel_token = uuid::Uuid::new_v4().to_string();
                let Some(reply_markup) = Self::model_picker_models_reply_markup(
                    &buttons,
                    &context.current,
                    *page,
                    paged.total_pages,
                    &indicator_token,
                    previous_token.as_deref(),
                    next_token.as_deref(),
                    &back_token,
                    &cancel_token,
                ) else {
                    return ModelPickerCallbackOutcome::Rejected;
                };
                let navigation_tokens =
                    usize::from(previous_token.is_some()) + usize::from(next_token.is_some()) + 3;
                Self::reserve_pending_model_picker_capacity(
                    &mut pending,
                    buttons.len().saturating_add(navigation_tokens),
                );
                for (token, option) in buttons {
                    pending.insert(
                        token,
                        PendingModelPicker {
                            action: ModelPickerAction::Select(option),
                            ..state.clone()
                        },
                    );
                }
                if let Some(token) = previous_token {
                    pending.insert(
                        token,
                        PendingModelPicker {
                            action: ModelPickerAction::OpenCategory {
                                provider_ref: provider_ref.clone(),
                                page: page - 1,
                            },
                            ..state.clone()
                        },
                    );
                }
                if let Some(token) = next_token {
                    pending.insert(
                        token,
                        PendingModelPicker {
                            action: ModelPickerAction::OpenCategory {
                                provider_ref: provider_ref.clone(),
                                page: page + 1,
                            },
                            ..state.clone()
                        },
                    );
                }
                pending.insert(
                    indicator_token,
                    PendingModelPicker {
                        action: ModelPickerAction::OpenCategory {
                            provider_ref: provider_ref.clone(),
                            page: *page,
                        },
                        ..state.clone()
                    },
                );
                pending.insert(
                    back_token,
                    PendingModelPicker {
                        action: ModelPickerAction::Back,
                        ..state.clone()
                    },
                );
                pending.insert(
                    cancel_token,
                    PendingModelPicker {
                        action: ModelPickerAction::Cancel,
                        ..state.clone()
                    },
                );
                ModelPickerCallbackOutcome::Rendered {
                    text: i18n::get_required_cli_string_with_args(
                        "channel-telegram-model-picker-model-title",
                        &[("provider", category.provider_ref.as_str())],
                    ),
                    reply_markup,
                }
            }
        }
    }

    /// Telegram's Bot API answers every method call with a JSON envelope
    /// whose `ok` field carries the application-level result: a 2xx status
    /// with `ok: false` means the request did not succeed. Callers that
    /// only inspect the HTTP status would misread application-level
    /// rejections as success.
    fn telegram_api_envelope_ok(body: &serde_json::Value) -> bool {
        body.get("ok").and_then(serde_json::Value::as_bool) == Some(true)
    }

    async fn answer_model_picker_callback(&self, callback_id: &str, text: String) {
        let mut body = serde_json::json!({ "callback_query_id": callback_id });
        if !text.is_empty() {
            body["text"] = serde_json::Value::String(text.chars().take(180).collect());
        }
        match self
            .http_client()
            .post(self.api_url("answerCallbackQuery"))
            .json(&body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let envelope_ok = response
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .as_ref()
                    .is_some_and(Self::telegram_api_envelope_ok);
                if !(status.is_success() && envelope_ok) {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "status": status.as_u16(),
                                "telegram_ok": envelope_ok,
                            })),
                        "Telegram model picker callback acknowledgement failed"
                    );
                }
            }
            Err(error) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "error": clawcrew_runtime::security::scrub(&error.to_string()),
                        })),
                    "Telegram model picker callback acknowledgement failed"
                );
            }
        }
    }

    async fn disable_model_picker_keyboard(&self, callback: &serde_json::Value) {
        let (Some(chat_id), Some(message_id)) = (
            callback
                .get("message")
                .and_then(|message| message.get("chat"))
                .and_then(|chat| chat.get("id"))
                .and_then(serde_json::Value::as_i64),
            Self::callback_model_picker_message_id(callback),
        ) else {
            return;
        };
        self.disable_model_picker_keyboard_at(chat_id, message_id)
            .await;
    }

    async fn disable_model_picker_keyboard_at(&self, chat_id: i64, message_id: i64) {
        match self
            .http_client()
            .post(self.api_url("editMessageReplyMarkup"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "message_id": message_id,
                "reply_markup": { "inline_keyboard": [] },
            }))
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let envelope_ok = response
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .as_ref()
                    .is_some_and(Self::telegram_api_envelope_ok);
                if !(status.is_success() && envelope_ok) {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "status": status.as_u16(),
                                "telegram_ok": envelope_ok,
                                "picker_message_id": message_id,
                            })),
                        "Telegram model picker keyboard cleanup failed"
                    );
                }
            }
            Err(error) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "error": clawcrew_runtime::security::scrub(&error.to_string()),
                            "picker_message_id": message_id,
                        })),
                    "Telegram model picker keyboard cleanup failed"
                );
            }
        }
    }

    async fn edit_model_picker_message(
        &self,
        callback: &serde_json::Value,
        text: String,
        reply_markup: serde_json::Value,
    ) -> bool {
        let (Some(chat_id), Some(message_id)) = (
            callback
                .get("message")
                .and_then(|message| message.get("chat"))
                .and_then(|chat| chat.get("id"))
                .and_then(serde_json::Value::as_i64),
            Self::callback_model_picker_message_id(callback),
        ) else {
            return false;
        };
        self.edit_model_picker_message_at(chat_id, message_id, text, reply_markup)
            .await
    }

    async fn edit_model_picker_message_at(
        &self,
        chat_id: i64,
        message_id: i64,
        text: String,
        reply_markup: serde_json::Value,
    ) -> bool {
        match self
            .http_client()
            .post(self.api_url("editMessageText"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "message_id": message_id,
                "text": text,
                "reply_markup": reply_markup,
            }))
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let envelope_ok = response
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .as_ref()
                    .is_some_and(Self::telegram_api_envelope_ok);
                if status.is_success() && envelope_ok {
                    true
                } else {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "status": status.as_u16(),
                                "telegram_ok": envelope_ok,
                                "picker_message_id": message_id,
                            })),
                        "Telegram model picker edit failed; restoring prior keyboard"
                    );
                    false
                }
            }
            Err(error) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "error": clawcrew_runtime::security::scrub(&error.to_string()),
                            "picker_message_id": message_id,
                        })),
                    "Telegram model picker edit failed; restoring prior keyboard"
                );
                false
            }
        }
    }

    /// `try_send` is the atomic accept/reject boundary for the queue
    /// handoff: it either enqueues the selection or returns it
    /// (`TrySendError::Full`/`Closed`), so a failed handoff hands the
    /// message back and the caller can restore the picker cohort instead
    /// of silently dropping the one-shot selection. A receiver that closes
    /// *after* a successful enqueue silently discards the queued item; the
    /// caller covers that boundary with the delivery acknowledgement in
    /// `crate::model_picker_delivery`.
    fn deliver_model_picker_selection(
        tx: &tokio::sync::mpsc::Sender<ChannelMessage>,
        message: ChannelMessage,
    ) -> Result<(), Box<ChannelMessage>> {
        tx.try_send(message).map_err(|error| match error {
            tokio::sync::mpsc::error::TrySendError::Full(message)
            | tokio::sync::mpsc::error::TrySendError::Closed(message) => Box::new(message),
        })
    }

    async fn handle_model_picker_callback(
        &self,
        callback: &serde_json::Value,
        tx: &tokio::sync::mpsc::Sender<ChannelMessage>,
    ) {
        let callback_id = callback
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !self.prevalidate_model_picker_callback(callback).await {
            self.answer_model_picker_callback(
                callback_id,
                i18n::get_required_cli_string("channel-telegram-model-picker-rejected"),
            )
            .await;
            return;
        }
        let permit = if self.model_picker_callback_requires_queue(callback).await {
            match tx.try_reserve() {
                Ok(permit) => Some(permit),
                Err(_) => {
                    self.answer_model_picker_callback(
                        callback_id,
                        i18n::get_required_cli_string("channel-telegram-model-picker-unavailable"),
                    )
                    .await;
                    return;
                }
            }
        } else {
            None
        };
        let previous_keyboard = self.model_picker_keyboard_snapshot(callback).await;
        match self.process_model_picker_callback(callback).await {
            ModelPickerCallbackOutcome::Queued(message) => {
                let Some(permit) = permit else {
                    self.restore_model_picker_keyboard(previous_keyboard).await;
                    self.answer_model_picker_callback(
                        callback_id,
                        i18n::get_required_cli_string("channel-telegram-model-picker-unavailable"),
                    )
                    .await;
                    return;
                };
                // The early `try_reserve` only proved capacity existed before
                // the one-shot selection token was consumed. Release the
                // reservation and hand off atomically: `try_send` either
                // enqueues or returns the message, so a closed/full queue
                // restores the picker instead of dropping the selection.
                drop(permit);
                let message = *message;
                let message_id = message.id.clone();
                // Register the delivery acknowledgement before the handoff
                // so a runtime that consumes the selection immediately
                // cannot confirm into a not-yet-registered id.
                let mut delivery_ack = crate::model_picker_delivery::register(&message_id);
                if Self::deliver_model_picker_selection(tx, message).is_err() {
                    crate::model_picker_delivery::cancel(&message_id);
                    self.restore_model_picker_keyboard(previous_keyboard).await;
                    self.answer_model_picker_callback(
                        callback_id,
                        i18n::get_required_cli_string("channel-telegram-model-picker-unavailable"),
                    )
                    .await;
                    return;
                }
                // Only an enqueued selection needs the abort/timeout
                // revocation marker; the guard drop distinguishes on it.
                delivery_ack.mark_enqueued();
                // `try_send` only proved the queue accepted the selection;
                // a receiver dropped before consumption silently discards
                // it. Report `queued` only once the runtime confirms the
                // selection reached runtime command handling, bounded so a
                // stuck consumer cannot pin the callback answer. No picker
                // lock is held across this wait.
                let confirmed = tokio::time::timeout(
                    TELEGRAM_MODEL_PICKER_DELIVERY_ACK_TIMEOUT,
                    delivery_ack.wait(),
                )
                .await;
                if matches!(confirmed, Ok(Ok(()))) {
                    tokio::join!(
                        self.disable_model_picker_keyboard(callback),
                        self.answer_model_picker_callback(
                            callback_id,
                            i18n::get_required_cli_string("channel-telegram-model-picker-queued"),
                        ),
                    );
                } else {
                    // The bounded ack wait elapsed. The claim decides the
                    // outcome: if the route mutation already ran, the
                    // selection succeeded and must be reported as queued;
                    // otherwise revoke it so the late dispatch leaves the
                    // route change inert instead of applying it after the
                    // UI reported failure.
                    match crate::model_picker_delivery::revoke(&message_id) {
                        crate::model_picker_delivery::RevokeOutcome::Won => {
                            self.restore_model_picker_keyboard(previous_keyboard).await;
                            self.answer_model_picker_callback(
                                callback_id,
                                i18n::get_required_cli_string(
                                    "channel-telegram-model-picker-unavailable",
                                ),
                            )
                            .await;
                        }
                        crate::model_picker_delivery::RevokeOutcome::AlreadyApplied => {
                            // With a live queue the missing registration
                            // means the route mutation really did run. If
                            // the queue is already closed, the registration
                            // was reclaimed by teardown (`clear_abandoned`)
                            // while this callback was still in flight — the
                            // route can never apply, so report unavailability
                            // instead of a phantom queued state.
                            if tx.is_closed() {
                                self.restore_model_picker_keyboard(previous_keyboard).await;
                                self.answer_model_picker_callback(
                                    callback_id,
                                    i18n::get_required_cli_string(
                                        "channel-telegram-model-picker-unavailable",
                                    ),
                                )
                                .await;
                            } else {
                                tokio::join!(
                                    self.disable_model_picker_keyboard(callback),
                                    self.answer_model_picker_callback(
                                        callback_id,
                                        i18n::get_required_cli_string(
                                            "channel-telegram-model-picker-queued",
                                        ),
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            ModelPickerCallbackOutcome::Rendered { text, reply_markup } => {
                drop(permit);
                if self
                    .edit_model_picker_message(callback, text, reply_markup)
                    .await
                {
                    self.answer_model_picker_callback(callback_id, String::new())
                        .await;
                } else {
                    self.restore_model_picker_keyboard(previous_keyboard).await;
                    self.answer_model_picker_callback(
                        callback_id,
                        i18n::get_required_cli_string("channel-telegram-model-picker-unavailable"),
                    )
                    .await;
                }
            }
            ModelPickerCallbackOutcome::Cancelled => {
                drop(permit);
                tokio::join!(
                    self.disable_model_picker_keyboard(callback),
                    self.answer_model_picker_callback(
                        callback_id,
                        i18n::get_required_cli_string("channel-telegram-model-picker-cancelled"),
                    ),
                );
            }
            ModelPickerCallbackOutcome::Rejected => {
                drop(permit);
                self.answer_model_picker_callback(
                    callback_id,
                    i18n::get_required_cli_string("channel-telegram-model-picker-rejected"),
                )
                .await;
            }
        }
    }

    pub fn new(
        bot_token: String,
        alias: impl Into<String>,
        peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
        mention_only: bool,
    ) -> Self {
        let alias = alias.into();
        let has_peers = crate::allowlist::grants_anyone(&peer_resolver());
        let pairing = if has_peers {
            None
        } else {
            // Chat-channel bind codes are retyped by hand into a Telegram/
            // LINE/WeChat message, so they deliberately keep the six-digit
            // numeric shape. The shared-policy change re-scoped the *gateway* pairing code, not
            // this one; changing it here would be an unreviewed UX change.
            let guard = PairingGuard::new(
                true,
                &[],
                clawcrew_config::pairing::PairingCodePolicy::numeric_compat(),
            );
            if let Some(code) = guard.pairing_code() {
                // Surface the one-time bind code through the structured log,
                // not just stdout. A backgrounded daemon (launchd/systemd/
                // GUI-spawned) discards stdout, so the println! alone leaves
                // the operator with no way to retrieve the code. The log
                // lands in runtime-trace.jsonl, the gateway log stream, and
                // `clawcrew service logs`. Tag it `Channel` (not the default
                // `Internal`) so it survives the web Logs page's default
                // hide-internal filter and is visible without unticking it.
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_category(::clawcrew_log::EventCategory::Channel)
                        .with_attrs(::serde_json::json!({
                            "alias": alias.as_str(),
                            "pairing_code": code.as_str(),
                        })),
                    "Telegram pairing required; one-time bind code issued"
                );
                println!("  🔐 Telegram pairing required. One-time bind code: {code}");
                println!("     Send `{TELEGRAM_BIND_COMMAND} <code>` from your Telegram account.");
            }
            Some(guard)
        };

        Self {
            bot_token,
            alias,
            peer_resolver,
            persist: None,
            pairing,
            stream_mode: StreamMode::Off,
            draft_update_interval_ms: TELEGRAM_DRAFT_UPDATE_INTERVAL_MS,
            last_draft_edit: Mutex::new(std::collections::HashMap::new()),
            multi_message_drafts: Mutex::new(std::collections::HashMap::new()),
            typing_handle: Mutex::new(None),
            mention_only,
            per_user_session: true,
            passive_group_context: false,
            bot_username: Mutex::new(None),
            bot_id: Mutex::new(None),
            poll_health: Mutex::new(None),
            api_base: TELEGRAM_OFFICIAL_API_BASE_URL.to_string(),
            transcription: None,
            transcription_manager: None,
            voice_transcriptions: Mutex::new(std::collections::HashMap::new()),
            workspace_dir: None,
            ack_reactions: true,
            tts_manager: None,
            voice_chats: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            voice_peer_resolver: Arc::new(Vec::new) as Arc<dyn Fn() -> Vec<String> + Send + Sync>,
            pending_voice: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            proxy_url: None,
            #[cfg(test)]
            fixture_http_client: None,
            tool_command_specs: Vec::new(),
            pending_approvals: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            pending_model_pickers: tokio::sync::Mutex::new(HashMap::new()),
            approval_timeout_secs: 120,
            voice_drop_notice_timeout: VOICE_DROP_NOTICE_TIMEOUT,
        }
    }

    /// Shrink the voice-drop notice bound so a stalled-notice test does not
    /// wait out the production ceiling. Test-only: the ceiling is not
    /// operator-tunable — it exists to protect the listener, not to be tuned.
    #[cfg(test)]
    fn with_voice_drop_notice_timeout(mut self, timeout: Duration) -> Self {
        self.voice_drop_notice_timeout = timeout;
        self
    }

    /// Set the resolver used to resolve voice-chat peers live (no cached state).
    pub fn with_voice_peer_resolver(
        mut self,
        voice_peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    ) -> Self {
        self.voice_peer_resolver = voice_peer_resolver;
        self
    }

    /// Override the approval prompt timeout (default 120s).
    pub fn with_approval_timeout_secs(mut self, secs: u64) -> Self {
        self.approval_timeout_secs = secs;
        self
    }

    /// Record unaddressed group messages as passive context instead of dropping them.
    pub fn with_passive_group_context(mut self, enabled: bool) -> Self {
        self.passive_group_context = enabled;
        self
    }

    fn should_record_passive_group_context(
        passive_group_context: bool,
        is_group: bool,
        addressed_to_bot: bool,
    ) -> bool {
        passive_group_context && is_group && !addressed_to_bot
    }

    /// Configure whether Telegram-native acknowledgement reactions are sent.
    pub fn with_ack_reactions(mut self, enabled: bool) -> Self {
        self.ack_reactions = enabled;
        self
    }

    /// Set by the orchestrator from `[channels.telegram.<alias>].per_user_session`.
    /// When `false`, group-chat messages carry `ReplyTarget` conversation scope,
    /// so every member of a group (or forum topic) shares one session keyed on
    /// the chat/topic. When `true` (default), group sessions stay sender-scoped.
    /// Direct messages are always sender-scoped either way.
    pub fn with_per_user_session(mut self, enabled: bool) -> Self {
        self.per_user_session = enabled;
        self
    }

    /// Conversation scope for an inbound Telegram message: room-scoped for
    /// group/supergroup chats when `per_user_session = false`, sender-scoped
    /// otherwise. `reply_target` already carries `chat_id:message_thread_id`
    /// for forum topics, so room scope still isolates topics from each other.
    ///
    /// `passive_group_context` selects room scope too: an observation filed
    /// in the observed member's own session could answer nobody.
    fn conversation_scope_for(
        &self,
        message: &serde_json::Value,
    ) -> clawcrew_api::channel::ChannelConversationScope {
        if (!self.per_user_session || self.passive_group_context) && Self::is_group_message(message)
        {
            clawcrew_api::channel::ChannelConversationScope::ReplyTarget
        } else {
            clawcrew_api::channel::ChannelConversationScope::Sender
        }
    }

    /// Returns `true` if `identity` belongs to a peer group configured with
    /// `output_modality = "voice"` for this channel. Resolved live from config
    /// via `voice_peer_resolver` so it stays correct across hot-reloads.
    ///
    /// `identity` is a sender identity. A peer group names senders, and a
    /// group's chat id does not identify the member who asked for the reply;
    /// only a private chat's address is the peer's own id.
    pub(crate) fn is_voice_peer(&self, identity: &str) -> bool {
        Self::voice_peer_identity_matches(&(self.voice_peer_resolver)(), identity)
    }

    /// Canonical voice-peer match for a resolved peer list: an optional leading
    /// `@` and ASCII case are ignored, and `"*"` matches anyone. Mirrors the
    /// shape inbound admission uses, so a configured peer matches the sender
    /// identity it was written for.
    fn voice_peer_identity_matches(peers: &[String], identity: &str) -> bool {
        let identity = Self::normalize_identity(identity);
        if identity.is_empty() {
            return false;
        }
        let peers: Vec<String> = peers
            .iter()
            .map(|peer| Self::normalize_identity(peer))
            .collect();
        crate::allowlist::is_user_allowed(
            &peers,
            &identity,
            crate::allowlist::Match::CaseInsensitive,
        )
    }

    /// Senderless voice-peer check for a destination chat address, used where
    /// no inbound sender exists (proactive delivery) or where a voice note
    /// accompanies a text reply. Kept as the literal destination comparison
    /// this channel has always used: a peer group names senders, so a
    /// destination only stands in for one by coincidence, and widening the
    /// match here would change proactive modality that sender-side resolution
    /// does not cover.
    fn destination_is_voice_peer(&self, recipient: &str) -> bool {
        (self.voice_peer_resolver)().iter().any(|p| p == recipient)
    }

    /// Set a per-channel proxy URL that overrides the global proxy config.
    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    /// Store pre-computed tool command specs for bot command registration.
    pub fn with_tool_command_specs(mut self, specs: Vec<(String, String)>) -> Self {
        self.tool_command_specs = specs;
        self
    }

    /// Configure workspace directory for saving downloaded attachments.
    pub fn with_workspace_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.workspace_dir = Some(dir);
        self
    }

    /// Configure streaming mode for progressive draft updates or multi-message delivery.
    pub fn with_streaming(
        mut self,
        stream_mode: StreamMode,
        draft_update_interval_ms: u64,
    ) -> Self {
        self.stream_mode = stream_mode;
        self.draft_update_interval_ms = if draft_update_interval_ms == 0 {
            TELEGRAM_DRAFT_UPDATE_INTERVAL_MS
        } else {
            draft_update_interval_ms
        };
        self
    }

    /// Canonical source: `[channels.telegram.<alias>].multi_message_delay_ms`.
    fn resolve_multi_message_delay_ms(&self) -> u64 {
        self.persist
            .as_ref()
            .and_then(|config| {
                config
                    .read()
                    .channels
                    .telegram
                    .get(&self.alias)
                    .map(|tg| tg.multi_message_delay_ms)
            })
            .unwrap_or(DEFAULT_MULTI_MESSAGE_DELAY_MS)
    }

    fn new_multi_message_draft_id() -> String {
        format!(
            "{TELEGRAM_MULTI_MESSAGE_SYNTHETIC_PREFIX}{}",
            uuid::Uuid::new_v4().as_simple()
        )
    }

    fn multi_draft_key(recipient: &str, draft_id: &str) -> MultiDraftKey {
        MultiDraftKey {
            recipient: recipient.to_string(),
            draft_id: draft_id.to_string(),
        }
    }

    fn is_multi_message_synthetic_draft(message_id: &str) -> bool {
        message_id.starts_with(TELEGRAM_MULTI_MESSAGE_SYNTHETIC_PREFIX)
    }

    /// Sleep out the remainder of `multi_message_delay_ms` since the last
    /// successful send, pacing consecutive multi-message posts without a
    /// trailing delay after the final one.
    async fn pace_multi_message_send(&self, last_sent_at: Option<std::time::Instant>) {
        let delay = Duration::from_millis(self.resolve_multi_message_delay_ms());
        if delay.is_zero() {
            return;
        }
        if let Some(last) = last_sent_at {
            let elapsed = last.elapsed();
            if elapsed < delay {
                tokio::time::sleep(delay - elapsed).await;
            }
        }
    }

    /// Most recent successful multi-message send across all in-flight drafts
    /// for `recipient`. Used to pace the approval prompt so the inline
    /// keyboard does not crowd the just-delivered pre-tool narration.
    fn latest_multi_message_send_at(&self, recipient: &str) -> Option<std::time::Instant> {
        let drafts = self.multi_message_drafts.lock();
        drafts
            .iter()
            .filter(|(key, _)| key.recipient == recipient)
            .filter_map(|(_, draft)| draft.last_sent_at)
            .max()
    }

    /// Send the unsent suffix of the draft's sanitized narration for one agent
    /// turn. `sent_text` advances only after a successful `sendMessage`.
    async fn flush_unsent(&self, recipient: &str, message_id: &str) -> anyhow::Result<()> {
        if !Self::is_multi_message_synthetic_draft(message_id) {
            return Ok(());
        }

        // A voice-configured peer (`output_modality = "voice"`) has opted out of
        // text streaming: multi_message's permanent per-turn narration is a
        // text-delivery affordance. Such a peer receives its reply as one unit at
        // finalize — a voice note by default, or the complete text if the agent
        // routes this reply to text — never mid-turn permanent narration. This is
        // the same contract ordinary `send()`/`finalize_draft` enforce, which
        // likewise withhold intermediate text from a voice peer.
        //
        // The gate is the stable per-peer config predicate, NOT the per-reply
        // `suppress_voice` routing override: the override is chosen by the agent
        // mid-turn and is not knowable when a narration turn flushes, so consulting
        // it here would be a race. No content is dropped —
        // `finalize_multi_message_draft` (which does see `suppress_voice`) delivers
        // the complete text, the accumulated narration and the final answer, to a
        // text-routed voice peer.
        if self.destination_is_voice_peer(recipient) {
            return Ok(());
        }

        let key = Self::multi_draft_key(recipient, message_id);
        let (chat_id, parsed_thread) = Self::parse_reply_target(recipient);

        // Serialize flushes per draft: the turn-boundary flush (draft-updater
        // task) and the approval-path flush (agent loop) can race, and both
        // would otherwise read the same unsent suffix and send it twice.
        let flush_lock = {
            let drafts = self.multi_message_drafts.lock();
            let Some(draft) = drafts.get(&key) else {
                return Ok(());
            };
            draft.flush_lock.clone()
        };
        let _flush_guard = flush_lock.lock().await;

        let (current, unsent, thread_id, last_sent_at) = {
            let mut drafts = self.multi_message_drafts.lock();
            let Some(draft) = drafts.get_mut(&key) else {
                return Ok(());
            };
            let current = draft.latest_visible.clone();
            // Never slice by byte offset: `sent_text` must be a literal prefix
            // of the current buffer. Sanitization can rewrite already-delivered
            // text (e.g. a think block closing across the sent boundary); in
            // that case resync without sending anything rather than emit a
            // garbled fragment — `finalize_draft` still delivers the final
            // turn in full.
            let Some(unsent) = current
                .strip_prefix(draft.sent_text.as_str())
                .map(str::to_string)
            else {
                draft.sent_text = current;
                draft.delivered_chunks = 0;
                draft.delivered_prefix = String::new();
                return Ok(());
            };
            let thread_id = draft.thread_id.clone().or(parsed_thread);
            (current, unsent, thread_id, draft.last_sent_at)
        };

        let cleaned = strip_tool_call_tags(unsent.trim());
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            // Nothing user-visible in this turn (e.g. a bare tool-call
            // envelope): mark it consumed instead of POSTing an empty message
            // and retrying it on every subsequent flush.
            let mut drafts = self.multi_message_drafts.lock();
            if let Some(draft) = drafts.get_mut(&key) {
                draft.sent_text = current;
                draft.delivered_chunks = 0;
                draft.delivered_prefix = String::new();
            }
            return Ok(());
        }

        self.pace_multi_message_send(last_sent_at).await;

        let skip = {
            let drafts = self.multi_message_drafts.lock();
            let d = drafts.get(&key);
            let delivered = d.map(|d| d.delivered_chunks).unwrap_or(0);
            let prefix = d.map(|d| d.delivered_prefix.clone()).unwrap_or_default();
            let chunks = split_message_for_telegram(cleaned);
            let ok = delivered > 0
                && chunks.len() >= delivered
                && chunks[..delivered].concat() == prefix;
            if ok { delivered } else { 0 }
        };
        match self
            .send_text_chunks(cleaned, &chat_id, thread_id.as_deref(), skip)
            .await
        {
            Ok(_) => {
                let mut drafts = self.multi_message_drafts.lock();
                if let Some(draft) = drafts.get_mut(&key) {
                    draft.sent_text = current;
                    draft.last_sent_at = Some(std::time::Instant::now());
                    draft.delivered_chunks = 0;
                    draft.delivered_prefix = String::new();
                }
            }
            Err(e) => {
                {
                    let mut drafts = self.multi_message_drafts.lock();
                    if let Some(draft) = drafts.get_mut(&key) {
                        let chunks = split_message_for_telegram(cleaned);
                        draft.delivered_chunks = e.delivered;
                        draft.delivered_prefix = chunks
                            .iter()
                            .take(e.delivered)
                            .cloned()
                            .collect::<Vec<_>>()
                            .concat();
                        // A partial failure still physically sent `e.delivered`
                        // chunks; record it so the next pace (finalize / approval
                        // prompt) spaces off the real last send.
                        if e.delivered > 0 {
                            draft.last_sent_at = Some(std::time::Instant::now());
                        }
                    }
                }
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                    "Telegram multi-message turn send failed"
                );
                return Ok(());
            }
        }

        Ok(())
    }

    async fn finalize_multi_message_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
        suppress_voice: bool,
    ) -> anyhow::Result<()> {
        let text = strip_tool_call_tags(text);
        let (chat_id, parsed_thread) = Self::parse_reply_target(recipient);

        // Voice-only contract: an unsuppressed voice peer receives the reply as
        // a single voice note only — no permanent narration and no final-answer
        // `sendMessage`. `suppress_voice = true` (explicit text-only routing
        // override) opts back into text delivery. This mirrors the ordinary
        // `send()`/`finalize_draft` behavior so multi_message mode does not
        // bypass the modality contract.
        //
        // Only a STATICALLY-configured voice peer suppresses narration here. A
        // per-turn `send_via(voice)` route on a text-default peer is unknowable
        // while narration streams, and narration is published as separate,
        // permanent messages — so it stays text and is not retracted; that route
        // makes only the final answer a voice note (see the `send()` contract).
        let voice_only = !suppress_voice && self.destination_is_voice_peer(recipient);

        let key = Self::multi_draft_key(recipient, message_id);
        let flush_lock = {
            let drafts = self.multi_message_drafts.lock();
            let Some(draft) = drafts.get(&key) else {
                // No draft to finalize: deliver voice for the final text only,
                // matching the non-multi-message finalize path.
                if !suppress_voice {
                    self.try_queue_voice_reply(recipient, &text, true, false);
                }
                return Ok(());
            };
            draft.flush_lock.clone()
        };
        // Wait out any in-flight turn flush so the final send cannot interleave
        // with an intermediate one for the same draft.
        let _flush_guard = flush_lock.lock().await;
        let (thread_id, mut last_sent_at, pending) = {
            let mut drafts = self.multi_message_drafts.lock();
            let Some(draft) = drafts.remove(&key) else {
                if !suppress_voice {
                    self.try_queue_voice_reply(recipient, &text, true, false);
                }
                return Ok(());
            };
            // Any intermediate narration a partial flush left undelivered must be
            // sent before the final turn, otherwise removing the draft here loses
            // it. `strip_prefix` yields the unsent suffix; `None` (sanitization
            // rewrote already-delivered text) skips the resend, matching
            // `flush_unsent`'s own resync behavior.
            let pending = draft
                .latest_visible
                .strip_prefix(draft.sent_text.as_str())
                .map(|unsent| {
                    (
                        unsent.to_string(),
                        draft.delivered_chunks,
                        draft.delivered_prefix.clone(),
                    )
                });
            (
                draft.thread_id.clone().or(parsed_thread),
                draft.last_sent_at,
                pending,
            )
        };
        self.last_draft_edit.lock().remove(&chat_id);

        // Deliver the pending intermediate suffix before the final turn, resuming
        // past already-accepted physical chunks (validated prefix, same guard as
        // `flush_unsent`) so nothing is duplicated. Finalize is the terminal
        // lifecycle event: the draft was removed above and there is no later
        // production caller to resume it, so the retry happens in-line here. A
        // transient failure resolves within `MULTI_MESSAGE_FINALIZE_RETRIES`
        // (in-order delivery preserved); a permanent failure drops the narration
        // with a WARN and still delivers the final answer below, rather than
        // re-inserting unreachable orphaned draft state.
        if let Some((unsent, delivered_chunks, delivered_prefix)) = pending
            && !voice_only
        {
            let cleaned = strip_tool_call_tags(unsent.trim());
            let cleaned = cleaned.trim();
            if !cleaned.is_empty() {
                let chunks = split_message_for_telegram(cleaned);
                // Absolute count of physical chunks Telegram already accepted;
                // advanced across retries so an accepted chunk is never re-sent.
                let mut skip = if delivered_chunks > 0
                    && chunks.len() >= delivered_chunks
                    && chunks[..delivered_chunks].concat() == delivered_prefix
                {
                    delivered_chunks
                } else {
                    0
                };
                let mut attempt = 0u32;
                loop {
                    self.pace_multi_message_send(last_sent_at).await;
                    match self
                        .send_text_chunks(cleaned, &chat_id, thread_id.as_deref(), skip)
                        .await
                    {
                        Ok(_) => {
                            last_sent_at = Some(std::time::Instant::now());
                            break;
                        }
                        Err(e) => {
                            // Resume past chunks this attempt physically delivered
                            // so a retry never duplicates them.
                            if e.delivered > skip {
                                skip = e.delivered;
                                last_sent_at = Some(std::time::Instant::now());
                            }
                            attempt += 1;
                            if attempt >= MULTI_MESSAGE_FINALIZE_RETRIES {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        ::serde_json::json!({
                                            "error": format!("{}", e.source),
                                            "chunks_delivered": skip,
                                        })
                                    ),
                                    "Telegram multi-message pending narration undeliverable after retries; dropping it and delivering the final answer"
                                );
                                break;
                            }
                        }
                    }
                }
            }
        }

        let (text_without_markers, attachments) = parse_attachment_markers(&text);
        // `finalize_draft` receives the final agent-turn text (`delivered_response`),
        // not the draft updater's accumulated multi-turn buffer. Intermediate turns
        // are emitted via `flush_draft_turn`; send the final turn in full.
        let remainder = sanitize_multi_message_visible_text(&text_without_markers);

        if !remainder.is_empty() && !voice_only {
            self.pace_multi_message_send(last_sent_at).await;
            // Progress-preserving send: if Telegram accepts an earlier physical
            // chunk of the final answer and a later one fails, this returns
            // `FinalizePartialDelivery` (via `finalize_send_chunks`) instead of a
            // bare error, so the orchestrator's finalize fallback does not resend
            // the whole answer from chunk zero and duplicate the accepted prefix.
            self.finalize_send_chunks(&remainder, &chat_id, thread_id.as_deref())
                .await?;
        }

        // Attachments are permanent Bot API sends, so they fall under the same
        // voice-only guard as narration and final text: an unsuppressed voice
        // peer receives only the voice note. A `suppress_voice = true` (text
        // routed) peer still gets its media, matching ordinary text delivery.
        if !voice_only {
            for attachment in &attachments {
                self.send_attachment(&chat_id, thread_id.as_deref(), attachment)
                    .await?;
            }
        }

        // Queue the voice reply only after the pending narration and the final
        // text have been delivered, so a resend failure or a failed final send is
        // never overtaken by an immediate TTS reply (send_via modality="text"
        // still suppresses it entirely).
        if !suppress_voice {
            self.try_queue_voice_reply(recipient, &text, true, false);
        }

        Ok(())
    }

    /// Override the Telegram Bot API base URL.
    /// Useful for local Bot API servers or testing.
    pub fn with_api_base(mut self, api_base: String) -> Self {
        self.api_base = normalize_telegram_api_base(&api_base);
        self
    }

    /// Configure voice transcription from a `[transcription]` snapshot.
    ///
    /// Compatibility and test path. The daemon routes every channel through
    /// `with_transcription_manager` with a manager built from live
    /// config and the owning agent's resolved provider; this path can only see
    /// the legacy section, so it binds a lone registered provider and
    /// otherwise leaves the choice unbound (see
    /// `transcription::manager_from_snapshot`).
    pub fn with_transcription(self, config: clawcrew_config::schema::TranscriptionConfig) -> Self {
        let manager = super::transcription::manager_from_snapshot(&config);
        self.with_transcription_manager(config, manager)
    }

    /// Store an already-built transcription manager, or nothing. The config is
    /// recorded only alongside a manager, so a channel never advertises
    /// transcription it cannot perform.
    pub(crate) fn with_transcription_manager(
        mut self,
        config: clawcrew_config::schema::TranscriptionConfig,
        manager: Option<std::sync::Arc<super::transcription::TranscriptionManager>>,
    ) -> Self {
        if let Some(manager) = manager {
            self.transcription_manager = Some(manager);
            self.transcription = Some(config);
        }
        self
    }

    pub fn with_tts(mut self, config: &clawcrew_config::schema::Config) -> Self {
        if config.tts.enabled {
            let owner = config.agent_for_channel(&format!("telegram.{}", self.alias));
            match super::tts::TtsManager::from_config_for_agent(config, owner) {
                Ok(m) => self.tts_manager = Some(Arc::new(m)),
                Err(e) => ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                    "TTS disabled"
                ),
            }
        }
        self
    }

    /// Parse reply_target into (chat_id, optional thread_id).
    fn parse_reply_target(reply_target: &str) -> (String, Option<String>) {
        if let Some((chat_id, thread_id)) = reply_target.split_once(':') {
            (chat_id.to_string(), Some(thread_id.to_string()))
        } else {
            (reply_target.to_string(), None)
        }
    }

    fn extract_update_message_target(update: &serde_json::Value) -> Option<(String, i64)> {
        let message = update.get("message")?;
        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)?
            .to_string();
        let message_id = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)?;
        Some((chat_id, message_id))
    }

    fn extract_media_group_key(update: &serde_json::Value) -> Option<MediaGroupKey> {
        let message = update.get("message")?;
        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)?;
        let media_group_id = message
            .get("media_group_id")
            .and_then(serde_json::Value::as_str)?
            .to_string();
        Some((chat_id, media_group_id))
    }

    fn update_id(update: &serde_json::Value) -> Option<i64> {
        update.get("update_id").and_then(serde_json::Value::as_i64)
    }

    fn update_message_id(update: &serde_json::Value) -> Option<i64> {
        update
            .get("message")
            .and_then(|message| message.get("message_id"))
            .and_then(serde_json::Value::as_i64)
    }

    fn is_supported_media_group_update(update: &serde_json::Value) -> bool {
        update
            .get("message")
            .and_then(Self::parse_attachment_metadata)
            .is_some()
    }

    fn is_context_only_media_group_update(update: &serde_json::Value) -> bool {
        update
            .get("message")
            .and_then(|message| message.get("video"))
            .is_some()
    }

    fn should_defer_media_group_update(
        pending: &std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        update: &serde_json::Value,
    ) -> bool {
        let Some(key) = Self::extract_media_group_key(update) else {
            return false;
        };
        Self::is_supported_media_group_update(update) || pending.contains_key(&key)
    }

    fn is_duplicate_media_group_member(
        existing: &serde_json::Value,
        candidate: &serde_json::Value,
    ) -> bool {
        let same_update_id = Self::update_id(existing)
            .zip(Self::update_id(candidate))
            .is_some_and(|(existing, candidate)| existing == candidate);
        let same_message_id = Self::update_message_id(existing)
            .zip(Self::update_message_id(candidate))
            .is_some_and(|(existing, candidate)| existing == candidate);
        same_update_id || same_message_id
    }

    /// Record the caption and security scope of an album member that will
    /// never be downloaded. Deduplicated by `message_id` so a member repeated
    /// across polls cannot duplicate its context in the aggregate.
    fn retain_unsupported_member(
        group: &mut PendingMediaGroup,
        update: &serde_json::Value,
    ) -> bool {
        let Some(update_id) = Self::update_id(update) else {
            return false;
        };
        let Some(message_id) = Self::update_message_id(update) else {
            return false;
        };
        if group
            .unsupported
            .iter()
            .any(|member| member.message_id == message_id)
        {
            return false;
        }
        // A member already retained as materializable must never be
        // double-counted as text-only.
        if group
            .updates
            .iter()
            .any(|existing| Self::is_duplicate_media_group_member(existing, update))
        {
            return false;
        }
        let caption = update
            .get("message")
            .and_then(|message| message.get("caption"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let scope = update
            .get("message")
            .map(Self::media_group_scope)
            .unwrap_or_default();
        group.unsupported.push(UnsupportedMember {
            update_id,
            message_id,
            caption,
            scope,
        });
        true
    }

    fn buffer_media_group_update(
        pending: &mut std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        update: &serde_json::Value,
        now: Instant,
        poll_generation: u64,
    ) -> bool {
        let Some(key) = Self::extract_media_group_key(update) else {
            return false;
        };

        // Videos are context-only members of photo/video albums: retain them
        // even when one arrives before the first materializable sibling. The
        // existing listener-local map remains the single owner of transient
        // album state. Grouped audio keeps its independent parser behavior.
        if !Self::is_supported_media_group_update(update) {
            if !Self::is_context_only_media_group_update(update) {
                return true;
            }

            let group = pending.entry(key).or_insert_with(|| PendingMediaGroup {
                updates: Vec::new(),
                unsupported: Vec::new(),
                last_seen: now,
                last_seen_poll_generation: poll_generation,
                saturated_page_blocked: false,
            });
            if Self::retain_unsupported_member(group, update) {
                group.last_seen = now;
                group.last_seen_poll_generation = poll_generation;
            }
            return true;
        }

        let group = pending.entry(key).or_insert_with(|| PendingMediaGroup {
            updates: Vec::new(),
            unsupported: Vec::new(),
            last_seen: now,
            last_seen_poll_generation: poll_generation,
            saturated_page_blocked: false,
        });
        if group
            .updates
            .iter()
            .any(|existing| Self::is_duplicate_media_group_member(existing, update))
        {
            return true;
        }

        group.updates.push(update.clone());
        group.last_seen = now;
        group.last_seen_poll_generation = poll_generation;
        true
    }

    fn take_settled_media_groups(
        pending: &mut std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        now: Instant,
        completed_poll_generation: u64,
    ) -> Vec<MediaGroupBatch> {
        Self::take_media_groups_matching(pending, |_, group| {
            !group.saturated_page_blocked
                && now.saturating_duration_since(group.last_seen)
                    >= TELEGRAM_MEDIA_GROUP_SETTLE_DELAY
                && group.last_seen_poll_generation < completed_poll_generation
        })
    }

    fn take_prior_media_groups_for_update(
        pending: &mut std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        update: &serde_json::Value,
        now: Instant,
        completed_poll_generation: u64,
    ) -> Vec<MediaGroupBatch> {
        let Some(message) = update.get("message") else {
            return Vec::new();
        };
        let Some(chat_id) = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
        else {
            return Vec::new();
        };
        let Some(message_id) = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)
        else {
            return Vec::new();
        };
        let Some(update_id) = Self::update_id(update) else {
            return Vec::new();
        };

        Self::take_media_groups_matching(pending, |key, group| {
            !group.saturated_page_blocked
                && key.0 == chat_id
                && !group.updates.is_empty()
                && now.saturating_duration_since(group.last_seen)
                    >= TELEGRAM_MEDIA_GROUP_SETTLE_DELAY
                && group.last_seen_poll_generation < completed_poll_generation
                && group.updates.iter().all(|member| {
                    Self::update_id(member).is_some_and(|id| id < update_id)
                        && Self::update_message_id(member).is_some_and(|id| id < message_id)
                })
                // Retained text-only members carry their own ordering identity,
                // so a later unsupported sibling must hold the group pending
                // just like a later materializable one.
                && group
                    .unsupported
                    .iter()
                    .all(|member| member.update_id < update_id && member.message_id < message_id)
        })
    }

    fn take_media_groups_matching(
        pending: &mut std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        mut should_take: impl FnMut(&MediaGroupKey, &PendingMediaGroup) -> bool,
    ) -> Vec<MediaGroupBatch> {
        let mut matching_keys: Vec<(MediaGroupKey, i64)> = pending
            .iter()
            .filter(|(key, group)| !group.saturated_page_blocked && should_take(key, group))
            .map(|(key, group)| {
                let earliest_update_id = group
                    .updates
                    .iter()
                    .filter_map(Self::update_id)
                    .chain(group.unsupported.iter().map(|member| member.update_id))
                    .min()
                    .unwrap_or(i64::MAX);
                (key.clone(), earliest_update_id)
            })
            .collect();
        matching_keys.sort_by(|(left_key, left_id), (right_key, right_id)| {
            left_id.cmp(right_id).then_with(|| left_key.cmp(right_key))
        });

        matching_keys
            .into_iter()
            .filter_map(|(key, _)| pending.remove_entry(&key))
            .map(|(key, mut group)| {
                group
                    .updates
                    .sort_by_key(|update| Self::update_message_id(update).unwrap_or(i64::MAX));
                MediaGroupBatch {
                    key,
                    updates: group.updates,
                    unsupported: group.unsupported,
                    last_seen: group.last_seen,
                    last_seen_poll_generation: group.last_seen_poll_generation,
                    saturated_page_blocked: group.saturated_page_blocked,
                }
            })
            .collect()
    }

    fn media_group_poll_timeout_secs(
        pending: &std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
    ) -> u64 {
        if pending.is_empty() {
            TELEGRAM_IDLE_POLL_TIMEOUT_SECS
        } else {
            TELEGRAM_PENDING_MEDIA_GROUP_POLL_TIMEOUT_SECS
        }
    }

    fn media_group_sender_scope(message: &serde_json::Value) -> Option<String> {
        if let Some(id) = message
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_i64)
        {
            return Some(format!("user:{id}"));
        }
        if let Some(id) = message
            .get("sender_chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
        {
            return Some(format!("chat:{id}"));
        }
        message
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(serde_json::Value::as_str)
            .map(Self::normalize_identity)
            .filter(|username| !username.is_empty())
            .map(|username| format!("username:{username}"))
    }

    fn media_group_scope(message: &serde_json::Value) -> MediaGroupScope {
        MediaGroupScope {
            chat_id: message
                .get("chat")
                .and_then(|chat| chat.get("id"))
                .and_then(serde_json::Value::as_i64),
            media_group_id: message
                .get("media_group_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            thread_id: message
                .get("message_thread_id")
                .and_then(serde_json::Value::as_i64),
            sender: Self::media_group_sender_scope(message),
        }
    }

    fn media_group_scopes_match(anchor: &MediaGroupScope, candidate: &MediaGroupScope) -> bool {
        anchor.chat_id.is_some()
            && anchor.media_group_id.is_some()
            && anchor.sender.is_some()
            && anchor == candidate
    }

    fn media_group_members_share_scope(
        anchor: &serde_json::Value,
        candidate: &serde_json::Value,
    ) -> bool {
        Self::media_group_scopes_match(
            &Self::media_group_scope(anchor),
            &Self::media_group_scope(candidate),
        )
    }

    fn try_add_ack_reaction_nonblocking(&self, chat_id: String, message_id: i64) {
        let client = self.http_client();
        let url = self.api_url("setMessageReaction");
        let emoji = random_telegram_ack_reaction().to_string();
        let body = build_telegram_ack_reaction_request(&chat_id, message_id, &emoji);

        clawcrew_spawn::spawn!(async move {
            let response = match client.post(&url).json(&body).send().await {
                Ok(resp) => resp,
                Err(err) => {
                    ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"chat_id": chat_id, "message_id": message_id, "err": err.to_string()})), "failed to add ACK reaction to chat_id=, message_id=");
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let err_body = response.text().await.unwrap_or_default();
                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"chat_id": chat_id, "message_id": message_id, "status": status.to_string(), "err_body": err_body})), "add ACK reaction failed for chat_id=, message_id=: status=, body=");
            }
        });
    }

    fn http_client(&self) -> reqwest::Client {
        #[cfg(test)]
        if let Some(client) = &self.fixture_http_client {
            return client.clone();
        }

        clawcrew_config::schema::build_channel_proxy_client(
            "channel.telegram",
            self.proxy_url.as_deref(),
        )
    }

    fn normalize_identity(value: &str) -> String {
        value.trim().trim_start_matches('@').to_string()
    }

    /// write a paired user into `peer_groups` and save. The long-running
    /// daemon sets this from the orchestrator; tests and one-shot
    /// callers leave it unset (pairing works at runtime, doesn't persist).
    pub fn with_persistence(mut self, config: Arc<RwLock<Config>>) -> Self {
        self.persist = Some(config);
        self
    }

    /// The conflict message when a matching `ignore` denies `identity`.
    ///
    /// Asked before `try_pair`, because pairing consumes the one-time code.
    fn pairing_deny_conflict(&self, identities: &[String]) -> Option<String> {
        let config = self.persist.as_ref()?;
        // The same set `is_any_user_allowed` judges at message time. Checking
        // only the identity that would be *written* lets an `ignore` naming the
        // username pass a bind whose numeric id is the one persisted, and every
        // later message from that account is then rejected by the inbound gate.
        let normalized: Vec<String> = identities
            .iter()
            .map(|identity| Self::normalize_identity(identity))
            .collect();
        let borrowed: Vec<&str> = normalized.iter().map(String::as_str).collect();
        let cfg = config.read();
        crate::identity_persist::external_peer_deny_conflict(
            &cfg,
            "telegram",
            &self.alias,
            &borrowed,
            |entry, user| Self::normalize_identity(entry) == user,
        )
    }

    async fn persist_allowed_identity(&self, identity: &str) -> anyhow::Result<()> {
        let Some(config) = &self.persist else {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"identity": identity})),
                "paired identity not persisted (no persistence handle wired)"
            );
            return Ok(());
        };
        let normalized = Self::normalize_identity(identity);
        if normalized.is_empty() {
            anyhow::bail!("Cannot persist empty Telegram identity");
        }
        // Through the shared writer, which selects its target group by the
        // `channel` field the runtime reader authorizes by rather than by the
        // `peer_groups` map key. Selecting by key wrote the grant into whatever
        // group happened to be named `telegram_<alias>`, even one whose
        // `channel` points at a different instance, and reported success.
        crate::identity_persist::persist_external_peer(
            Some(config),
            "telegram",
            &self.alias,
            &normalized,
            |entry, user| Self::normalize_identity(entry) == user,
        )
        .await
    }

    fn extract_bind_code(text: &str) -> Option<&str> {
        let mut parts = text.split_whitespace();
        let command = parts.next()?;
        let base_command = command.split('@').next().unwrap_or(command);
        if base_command != TELEGRAM_BIND_COMMAND {
            return None;
        }
        parts.next().map(str::trim).filter(|code| !code.is_empty())
    }

    fn pairing_code_active(&self) -> bool {
        self.pairing
            .as_ref()
            .and_then(PairingGuard::pairing_code)
            .is_some()
    }

    /// Whether any peer group has authorized someone on this channel.
    ///
    /// Effective grants only. The resolved list also carries the denies for
    /// `ignore`, so a config holding nothing but denies, or nothing but grants
    /// its own denies shadow, has authorized no one and the channel is still
    /// unpaired.
    fn has_authorized_peer(&self) -> bool {
        crate::allowlist::grants_anyone(&(self.peer_resolver)())
    }

    /// Build the operator-facing `clawcrew channel bind-telegram` command for
    /// this channel's alias. The CLI defaults to the `default` alias, so only
    /// non-default aliases need the explicit `--alias` flag — emitting it for
    /// the default case would just be noise.
    fn suggested_bind_command(alias: &str, identity: &str) -> String {
        if alias == "default" {
            format!("clawcrew channel bind-telegram {identity}")
        } else {
            format!("clawcrew channel bind-telegram {identity} --alias {alias}")
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.api_base, self.bot_token)
    }

    /// Register the bot's slash commands with Telegram via `setMyCommands`.
    /// Called once at startup so that users see a command menu when pressing `/`.
    /// Includes built-in runtime commands, user-installed skill commands, and
    /// enabled tool commands from the configuration.
    async fn register_bot_commands(&self) {
        let mut commands: Vec<serde_json::Value> = vec![
            serde_json::json!({ "command": "new",    "description": telegram_cli_string("channel-telegram-cmd-new-desc") }),
            serde_json::json!({ "command": "clear",  "description": telegram_cli_string("channel-telegram-cmd-clear-desc") }),
            serde_json::json!({ "command": "stop",   "description": telegram_cli_string("channel-telegram-cmd-stop-desc") }),
            serde_json::json!({ "command": "model",  "description": telegram_cli_string("channel-telegram-cmd-model-desc") }),
            serde_json::json!({ "command": "models", "description": telegram_cli_string("channel-telegram-cmd-models-desc") }),
            serde_json::json!({ "command": "config", "description": telegram_cli_string("channel-telegram-cmd-config-desc") }),
        ];

        // Track registered names to deduplicate across skills and tools.
        let mut used_names: std::collections::HashSet<String> = commands
            .iter()
            .filter_map(|c| c.get("command").and_then(|v| v.as_str()).map(String::from))
            .collect();

        // Collect commands from installed skills.
        if let Some(ref workspace_dir) = self.workspace_dir {
            let skills = clawcrew_runtime::skills::load_skills(workspace_dir);

            for skill in &skills {
                let sanitized = sanitize_telegram_command_name(&skill.name);
                if sanitized.is_empty() {
                    ::clawcrew_log::record!(
                        DEBUG,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                        &format!(
                            "Skipping skill '{}': name produces empty Telegram command",
                            skill.name
                        )
                    );
                    continue;
                }
                if used_names.contains(&sanitized) {
                    ::clawcrew_log::record!(
                        DEBUG,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                        &format!(
                            "Skipping skill '{}': command /{sanitized} conflicts with an existing command",
                            skill.name
                        )
                    );
                    continue;
                }
                let description = if skill.description.is_empty() {
                    format!("Run the {name} skill", name = skill.name)
                } else {
                    truncate_telegram_command_description(&skill.description)
                };
                used_names.insert(sanitized.clone());
                commands.push(serde_json::json!({
                    "command": sanitized,
                    "description": description,
                }));
            }
        }

        // Collect commands from enabled tools.
        for (name, description) in &self.tool_command_specs {
            let sanitized = sanitize_telegram_command_name(name);
            if sanitized.is_empty() || used_names.contains(&sanitized) {
                continue;
            }
            used_names.insert(sanitized.clone());
            commands.push(serde_json::json!({
                "command": sanitized,
                "description": truncate_telegram_command_description(description),
            }));
        }

        // Telegram allows at most 100 commands.
        let total_before_cap = commands.len();
        commands.truncate(TELEGRAM_MAX_BOT_COMMANDS);
        if total_before_cap > TELEGRAM_MAX_BOT_COMMANDS {
            let registered = commands.len();
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "TELEGRAM_MAX_BOT_COMMANDS": TELEGRAM_MAX_BOT_COMMANDS,
                        "total_before_cap": total_before_cap,
                        "registered": registered,
                    })),
                // Stable literal per the logging contract: per-event
                // measurements (limit, configured, registered) ride solely in
                // `attributes` above, never in the message.
                "Telegram command registration truncated to the platform limit"
            );
        }

        // Second, independent cap: setMyCommands also refuses on total body size,
        // reporting it as BOT_COMMANDS_TOO_MUCH. A set inside the count limit can
        // still exceed it, which is why this runs after the truncation above.
        let before_body_cap = commands.len();
        let dropped_for_body = fit_telegram_bot_commands_to_body_budget(&mut commands);
        if dropped_for_body > 0 {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "TELEGRAM_MAX_BOT_COMMANDS_BODY_BYTES":
                            TELEGRAM_MAX_BOT_COMMANDS_BODY_BYTES,
                        "before_body_cap": before_body_cap,
                        "registered": commands.len(),
                        "dropped": dropped_for_body,
                    })),
                // Stable literal per the logging contract: per-event measurements
                // ride solely in `attributes` above, never in the message.
                "Telegram command registration trimmed to the platform body-size limit"
            );
        }

        let url = self.api_url("setMyCommands");
        let body = serde_json::json!({ "commands": commands });

        match self.http_client().post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                    &format!(
                        "Telegram bot commands registered successfully ({} commands)",
                        commands.len()
                    )
                );
            }
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"status": status.to_string(), "text": text})
                        ),
                    "Failed to register Telegram bot commands:"
                );
            }
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                    "Failed to register Telegram bot commands"
                );
            }
        }
    }

    /// Whether a destination should receive a TTS voice reply: the session is
    /// in input-driven voice mode, or — where the runtime has no inbound sender
    /// to consult — the target chat address is itself a configured voice peer.
    fn is_voice_chat(&self, recipient: &str) -> bool {
        self.voice_chats
            .lock()
            .map(|vs| vs.contains(recipient))
            .unwrap_or(false)
            || self.destination_is_voice_peer(recipient)
    }

    fn try_queue_voice_reply(&self, recipient: &str, content: &str, immediate: bool, force: bool) {
        if (!force && !self.is_voice_chat(recipient)) || self.tts_manager.is_none() {
            return;
        }

        // Only queue substantive natural-language replies for voice.
        // Skip tool outputs: URLs, JSON, code blocks, errors, short status.
        if let Some(reason) = crate::util::voice_reply_skip_reason(content) {
            // Stable literal per the logging contract: the classification and
            // per-event measurements ride solely in `attributes` above.
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Skip)
                    .with_attrs(::serde_json::json!({
                        "recipient": recipient,
                        "reason": reason,
                        "content_len": content.len(),
                        "immediate": immediate,
                    })),
                "voice reply skipped"
            );
            return;
        }

        let (chat_id, thread_id) = Self::parse_reply_target(recipient);
        let voice_chats = self.voice_chats.clone();
        let voice_peer_resolver = self.voice_peer_resolver.clone();
        let api_base = self.api_base.clone();
        let bot_token = self.bot_token.clone();
        let Some(tts_manager) = self.tts_manager.clone() else {
            return;
        };

        if immediate {
            // Finalize path: text is already the final answer — no debounce.
            let text = content.to_string();
            let recipient = recipient.to_string();
            let proxy_url = self.proxy_url.clone();
            clawcrew_spawn::spawn!(async move {
                let is_config_voice_peer = voice_peer_resolver().contains(&recipient);
                if !is_config_voice_peer && let Ok(mut vc) = voice_chats.lock() {
                    vc.remove(&recipient);
                }
                match Self::synthesize_and_send_voice(
                    &api_base,
                    &bot_token,
                    proxy_url.as_deref(),
                    &chat_id,
                    thread_id.as_deref(),
                    &text,
                    &tts_manager,
                )
                .await
                {
                    Ok(()) => {
                        ::clawcrew_log::record!(
                            INFO,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            ),
                            &format!("voice reply sent ({} chars)", text.len())
                        );
                    }
                    Err(e) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                            "TTS voice reply failed"
                        );
                    }
                }
            });
            return;
        }

        // Send path: debounce to coalesce multi-part tool-chain responses.
        if let Ok(mut pv) = self.pending_voice.lock() {
            pv.insert(
                recipient.to_string(),
                (content.to_string(), std::time::Instant::now()),
            );
        }

        let pending = self.pending_voice.clone();
        let recipient = recipient.to_string();
        let proxy_url = self.proxy_url.clone();
        clawcrew_spawn::spawn!(async move {
            // Wait 10 seconds — long enough for the agent to finish its
            // full tool chain and send the final answer.
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            // Atomic check-and-remove: only one task gets the value
            let to_voice = pending.lock().ok().and_then(|mut pv| {
                if let Some((_, ts)) = pv.get(&recipient)
                    && ts.elapsed().as_secs() >= 8
                {
                    return pv.remove(&recipient).map(|(text, _)| text);
                }
                None
            });

            if let Some(text) = to_voice {
                let is_config_voice_peer = voice_peer_resolver().contains(&recipient);
                if !is_config_voice_peer && let Ok(mut vc) = voice_chats.lock() {
                    vc.remove(&recipient);
                }
                match Self::synthesize_and_send_voice(
                    &api_base,
                    &bot_token,
                    proxy_url.as_deref(),
                    &chat_id,
                    thread_id.as_deref(),
                    &text,
                    &tts_manager,
                )
                .await
                {
                    Ok(()) => {
                        ::clawcrew_log::record!(
                            INFO,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            ),
                            &format!("voice reply sent ({} chars)", text.len())
                        );
                    }
                    Err(e) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                            "TTS voice reply failed"
                        );
                    }
                }
            }
        });
    }

    /// Synthesize text to speech and send as a Telegram voice note (static version for spawned tasks).
    async fn synthesize_and_send_voice(
        api_base: &str,
        bot_token: &str,
        proxy_url: Option<&str>,
        chat_id: &str,
        thread_id: Option<&str>,
        text: &str,
        tts_manager: &crate::tts::TtsManager,
    ) -> anyhow::Result<()> {
        let audio_bytes = tts_manager.synthesize_opus(text).await?;
        let audio_len = audio_bytes.len();
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"audio_len": audio_len})),
            "synthesized bytes of audio"
        );

        if audio_bytes.is_empty() {
            anyhow::bail!("TTS returned empty audio");
        }

        // synthesize_opus already transcodes to OGG/Opus via ffmpeg internally
        let (method, field, filename, mime) = telegram_audio_send_spec("opus")?;

        let url = format!("{api_base}/bot{bot_token}/{method}");
        // The same per-channel proxy every other Telegram request uses; the
        // global proxy alone dropped a configured `proxy_url` for voice uploads.
        let client =
            clawcrew_config::schema::build_channel_proxy_client("channel.telegram", proxy_url);

        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part(
                field,
                reqwest::multipart::Part::bytes(audio_bytes)
                    .file_name(filename)
                    .mime_str(mime)?,
            );

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        let resp = client.post(&url).multipart(form).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{method} failed: status={status}, body={body}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"audio_len": audio_len})),
            "sent voice note ( bytes)"
        );
        Ok(())
    }

    async fn classify_edit_message_response(resp: reqwest::Response) -> EditMessageResult {
        let status = resp.status();
        if status.is_success() {
            // the Bot API envelope is the outcome: only an explicit boolean
            // ok:true makes a 2xx a successful edit. A truncated body, a
            // proxy-generated 200, a missing or non-boolean ok field would
            // otherwise silently report a card rewrite that never happened.
            let bytes = resp.bytes().await.unwrap_or_default();
            let envelope = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
            let ok = envelope
                .as_ref()
                .and_then(|v| v.get("ok").and_then(|b| b.as_bool()));
            match ok {
                Some(true) => return EditMessageResult::Success,
                Some(false) => {
                    let description = envelope
                        .as_ref()
                        .and_then(|v| v.get("description"))
                        .and_then(|d| d.as_str())
                        .unwrap_or_default();
                    if description.contains("message is not modified") {
                        return EditMessageResult::NotModified;
                    }
                    return EditMessageResult::Failed(status);
                }
                // missing field, wrong type, unparseable body, read failure
                None => return EditMessageResult::Failed(status),
            }
        }

        let body = resp.text().await.unwrap_or_default();
        if body.contains("message is not modified") {
            return EditMessageResult::NotModified;
        }

        EditMessageResult::Failed(status)
    }

    /// Shortest window a deadline-firing waiter waits for a callback that
    /// claimed the resolution first. The callback sends immediately after
    /// claiming, so this only needs to cover the task-scheduling gap.
    const CALLBACK_CLAIM_GRACE: Duration = Duration::from_secs(5);

    /// Decide the approval outcome after the wait deadline has fired. The
    /// pending entry is the single resolution claim: winning its removal
    /// makes the runtime's timed-out deny the outcome, while losing the
    /// claim means a callback claimed first and its response is in flight
    /// on the channel, so it is consumed here instead of overridden.
    async fn resolve_after_deadline(
        &self,
        approval_id: &str,
        rx: &mut tokio::sync::oneshot::Receiver<clawcrew_api::channel::ChannelApprovalResponse>,
    ) -> clawcrew_api::channel::AttributedApprovalResponse {
        let claimed = self
            .pending_approvals
            .lock()
            .await
            .remove(approval_id)
            .is_some();
        if claimed {
            return clawcrew_api::channel::AttributedApprovalResponse::from_runtime(
                clawcrew_api::channel::ChannelApprovalResponse::Deny,
                clawcrew_api::channel::ApprovalSource::TimedOut,
            );
        }
        match tokio::time::timeout(Self::CALLBACK_CLAIM_GRACE, rx).await {
            Ok(Ok(response)) => {
                clawcrew_api::channel::AttributedApprovalResponse::operator(response)
            }
            _ => clawcrew_api::channel::AttributedApprovalResponse::from_runtime(
                clawcrew_api::channel::ChannelApprovalResponse::Deny,
                clawcrew_api::channel::ApprovalSource::TimedOut,
            ),
        }
    }

    async fn fetch_bot_username(&self) -> anyhow::Result<String> {
        let resp = self.http_client().get(self.api_url("getMe")).send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("Failed to fetch bot info: {}", resp.status());
        }

        let data: serde_json::Value = resp.json().await?;
        let result = data
            .get("result")
            .context("missing result in getMe response")?;
        let username = result
            .get("username")
            .and_then(|u| u.as_str())
            .context("Bot username not found in response")?;

        // Cache the bot's user ID for reply-to-self detection
        if let Some(id) = result.get("id").and_then(|i| i.as_i64()) {
            let mut cache = self.bot_id.lock();
            *cache = Some(id);
        }

        Ok(username.to_string())
    }

    async fn get_bot_username(&self) -> Option<String> {
        {
            let cache = self.bot_username.lock();
            if let Some(ref username) = *cache {
                return Some(username.clone());
            }
        }

        match self.fetch_bot_username().await {
            Ok(username) => {
                let mut cache = self.bot_username.lock();
                *cache = Some(username.clone());
                Some(username)
            }
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                    "Failed to fetch bot username"
                );
                None
            }
        }
    }

    /// Record the outcome of one `getUpdates` exchange.
    ///
    /// The poll loop already knows whether the Bot API accepted the call; a bad
    /// token 404s on every attempt while the loop keeps retrying, so `listen()`
    /// never returns and liveness alone says nothing. Keeping the last outcome
    /// here lets `listener_health` answer that question without a second call.
    fn record_poll_health(&self, ok: bool) {
        *self.poll_health.lock() = Some((ok, tokio::time::Instant::now()));
    }

    fn is_telegram_username_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_'
    }

    fn find_bot_mention_spans(text: &str, bot_username: &str) -> Vec<(usize, usize)> {
        let bot_username = bot_username.trim_start_matches('@');
        if bot_username.is_empty() {
            return Vec::new();
        }

        let mut spans = Vec::new();

        for (at_idx, ch) in text.char_indices() {
            if ch != '@' {
                continue;
            }

            if at_idx > 0 {
                let prev = text[..at_idx].chars().next_back().unwrap_or(' ');
                if Self::is_telegram_username_char(prev) {
                    continue;
                }
            }

            let username_start = at_idx + 1;
            let mut username_end = username_start;

            for (rel_idx, candidate_ch) in text[username_start..].char_indices() {
                if Self::is_telegram_username_char(candidate_ch) {
                    username_end = username_start + rel_idx + candidate_ch.len_utf8();
                } else {
                    break;
                }
            }

            if username_end == username_start {
                continue;
            }

            let mention_username = &text[username_start..username_end];
            if mention_username.eq_ignore_ascii_case(bot_username) {
                spans.push((at_idx, username_end));
            }
        }

        spans
    }

    fn contains_bot_mention(text: &str, bot_username: &str) -> bool {
        !Self::find_bot_mention_spans(text, bot_username).is_empty()
    }

    fn normalize_incoming_content(text: &str, _bot_username: &str) -> Option<String> {
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    fn is_group_message(message: &serde_json::Value) -> bool {
        message
            .get("chat")
            .and_then(|c| c.get("type"))
            .and_then(|t| t.as_str())
            .map(|t| t == "group" || t == "supergroup")
            .unwrap_or(false)
    }

    /// Check whether `message` is a reply to a message sent by the bot
    /// itself. When true, the `mention_only` gate should be bypassed.
    fn is_reply_to_bot(message: &serde_json::Value, bot_id: i64) -> bool {
        message
            .get("reply_to_message")
            .and_then(|r| r.get("from"))
            .and_then(|f| f.get("id"))
            .and_then(|i| i.as_i64())
            .is_some_and(|id| id == bot_id)
    }

    fn check_media_mention_gate(
        &self,
        message: &serde_json::Value,
        caption: Option<&str>,
    ) -> Option<Option<String>> {
        let is_group = Self::is_group_message(message);
        if !self.mention_only || !is_group {
            return Some(caption.map(String::from));
        }
        let bot_username_guard = self.bot_username.lock();
        let bot_username = bot_username_guard.as_ref()?;

        // If the user is replying directly to the bot's message, bypass the
        // mention check — replies are an unambiguous signal of intent.
        if let Some(caption) = caption
            && let Some(bot_id) = *self.bot_id.lock()
            && Self::is_reply_to_bot(message, bot_id)
        {
            return Some(Self::normalize_incoming_content(caption, bot_username));
        }

        let caption = caption?;
        if !Self::contains_bot_mention(caption, bot_username) {
            return None;
        }
        Some(Self::normalize_incoming_content(caption, bot_username))
    }

    /// Single-identifier convenience kept for the unit tests; the polling
    /// path authorizes the whole identity set at once.
    #[cfg(test)]
    fn is_user_allowed(&self, username: &str) -> bool {
        self.is_any_user_allowed([username])
    }

    /// A Telegram sender is known by both a username and a numeric ID, so they
    /// are evaluated together against one snapshot of the peer list. Asking per
    /// identifier lets a deny on one be defeated by a wildcard reached through
    /// the other.
    fn is_any_user_allowed<'a, I>(&self, identities: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        let owned: Vec<String> = identities
            .into_iter()
            .map(Self::normalize_identity)
            .collect();
        let identities: Vec<&str> = owned.iter().map(String::as_str).collect();
        let peers: Vec<String> = (self.peer_resolver)()
            .into_iter()
            .map(|p| Self::normalize_identity(&p))
            .filter(|p| !p.is_empty())
            .collect();
        crate::allowlist::is_identity_allowed(
            &peers,
            &identities,
            crate::allowlist::Match::Sensitive,
        )
    }

    fn approval_callback_context(callback: &serde_json::Value) -> (Vec<String>, Option<String>) {
        let mut identities = Vec::with_capacity(2);
        if let Some(username) = callback
            .pointer("/from/username")
            .and_then(serde_json::Value::as_str)
            .filter(|username| !username.is_empty())
        {
            identities.push(username.to_string());
        }
        if let Some(user_id) = callback
            .pointer("/from/id")
            .and_then(serde_json::Value::as_i64)
        {
            identities.push(user_id.to_string());
        }
        let chat_id = callback
            .pointer("/message/chat/id")
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());
        (identities, chat_id)
    }

    /// True when `message` carries content one of the update parsers
    /// would actually process for an authorized sender.
    ///
    /// Acceptance is resolved from the canonical typed parsers rather
    /// than from raw JSON key presence, so this predicate cannot drift
    /// from what the parsers accept: `text` must deserialize as a string
    /// (`parse_update_message`), a `voice`/`audio` payload must yield
    /// metadata via `parse_voice_metadata`, and a `document`/`photo`
    /// payload must yield an `IncomingAttachment` via
    /// `parse_attachment_metadata` (which rejects a missing/non-string
    /// `file_id` and an empty `photo` array). Telegram response JSON is
    /// an external trust boundary, so its shape is validated before any
    /// behavior — including the approval notice — is triggered.
    ///
    /// The live config gates are retained alongside the typed checks:
    /// voice/audio only counts when the transcription config and manager
    /// `try_parse_voice_message` requires are both present, and
    /// document/photo only when the workspace dir
    /// `try_parse_attachment_message` downloads into is set.
    ///
    /// This covers the config-shaped and shape-shaped bails; the
    /// content-shaped permanent bails — over-`max_duration_secs`
    /// voice/audio and over-`TELEGRAM_MAX_FILE_DOWNLOAD_BYTES`
    /// attachments — are checked separately by
    /// `message_exceeds_parser_limits`, kept out of this predicate
    /// specifically so a captioned `/bind <code>` still reaches the
    /// pairing branch in `handle_unauthorized_message` even on media
    /// those limits would otherwise reject.
    fn message_has_processable_content(&self, message: &serde_json::Value) -> bool {
        if message
            .get("text")
            .and_then(serde_json::Value::as_str)
            .is_some()
        {
            return true;
        }
        if self.transcription.is_some()
            && self.transcription_manager.is_some()
            && Self::parse_voice_metadata(message).is_some()
        {
            return true;
        }
        self.workspace_dir.is_some() && Self::parse_attachment_metadata(message).is_some()
    }

    /// True when `message` is a voice/audio or document/photo update that
    /// one of the update parsers would permanently bail on for size or
    /// duration alone, independent of authorization — mirroring
    /// `try_parse_voice_message`'s over-`max_duration_secs` bail (using
    /// the same `self.transcription` config the parser reads) and
    /// `try_parse_attachment_message`'s over-`TELEGRAM_MAX_FILE_DOWNLOAD_BYTES`
    /// bail (the same constant the parser checks). An authorized sender's
    /// identical update would be silently dropped for this reason, so an
    /// unauthorized sender must not receive the approval notice for it
    /// either. Deliberately excluded from `message_has_processable_content`
    /// so the captioned `/bind <code>` pairing path in
    /// `handle_unauthorized_message` — checked before this — is not
    /// gated by it.
    fn message_exceeds_parser_limits(&self, message: &serde_json::Value) -> bool {
        if let Some((_, duration)) = Self::parse_voice_metadata(message)
            && let Some(config) = self.transcription.as_ref()
            && duration > config.max_duration_secs
        {
            return true;
        }
        if let Some(attachment) = Self::parse_attachment_metadata(message)
            && let Some(size) = attachment.file_size
            && size > TELEGRAM_MAX_FILE_DOWNLOAD_BYTES
        {
            return true;
        }
        false
    }

    async fn handle_unauthorized_message(&self, update: &serde_json::Value) {
        let Some(message) = update.get("message") else {
            return;
        };

        // Only updates an authorized sender would have gotten processed
        // deserve the unauthorized notice. Everything else — service
        // messages (joins/leaves/pins), stickers, locations, contacts,
        // or media this deployment is not configured to process — stays
        // silent exactly as before; a notice would either spam group
        // chats on every join, or promise processing that can never
        // happen.
        if !self.message_has_processable_content(message) {
            return;
        }

        // Media updates carry no top-level `text`, only an optional
        // `caption`; fall back to it so a captioned `/bind <code>` still
        // reaches the pairing flow below. Captionless media yields "",
        // which simply finds no bind code and falls through to the
        // unauthorized-approval notice — the same outcome unauthorized
        // text senders already get.
        let text = message
            .get("text")
            .and_then(serde_json::Value::as_str)
            .or_else(|| message.get("caption").and_then(serde_json::Value::as_str))
            .unwrap_or("");

        let username_opt = message
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(serde_json::Value::as_str);
        let username = username_opt.unwrap_or("unknown");
        let normalized_username = Self::normalize_identity(username);

        let sender_id = message
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_i64);
        let sender_id_str = sender_id.map(|id| id.to_string());
        let normalized_sender_id = sender_id_str.as_deref().map(Self::normalize_identity);

        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());

        let Some(chat_id) = chat_id else {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                "missing chat_id in message, skipping"
            );
            return;
        };

        let identities = Self::authorization_identities(message);

        if self.is_any_user_allowed(identities.iter().map(String::as_str)) {
            return;
        }

        if let Some(code) = Self::extract_bind_code(text) {
            if let Some(pairing) = self.pairing.as_ref() {
                let bind_identity = normalized_sender_id.clone().or_else(|| {
                    if normalized_username.is_empty() || normalized_username == "unknown" {
                        None
                    } else {
                        Some(normalized_username.clone())
                    }
                });

                // Before the pairing transition: a denied identity can never be
                // persisted, and `try_pair` would spend the operator's only code
                // to reach that verdict, leaving the sender no way to retry.
                if bind_identity.is_some()
                    && let Some(conflict) = self.pairing_deny_conflict(&identities)
                {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({"conflict": conflict})),
                        "refusing bind before consuming pairing code"
                    );
                    let _ = self
                        .send(&SendMessage::new(
                            "❌ This account is denied by an `ignore` entry in the runtime config. Ask the operator to remove it, then retry with the same code.",
                            &chat_id,
                        ))
                        .await;
                    return;
                }

                // Reserved, not paired: the code is held aside and the token is
                // only minted by `commit()`. Dropping the reservation restores
                // the code, so a persistence failure below cannot spend the
                // operator's one-time secret on a binding that did not happen.
                match pairing.reserve_pair(code, &chat_id).await {
                    Ok(Some(reservation)) => {
                        if let Some(identity) = bind_identity {
                            match Box::pin(self.persist_allowed_identity(&identity)).await {
                                Ok(()) => {
                                    // Durable write landed, so the pairing may
                                    // now consume the code and mint the token.
                                    let _ = reservation.commit();
                                    let _ = self
                                        .send(&SendMessage::new(
                                            "✅ Telegram account bound successfully. You can talk to ClawCrew now.",
                                            &chat_id,
                                        ))
                                        .await;
                                    ::clawcrew_log::record!(
                                        INFO,
                                        ::clawcrew_log::Event::new(
                                            module_path!(),
                                            ::clawcrew_log::Action::Note
                                        )
                                        .with_attrs(::serde_json::json!({"identity": identity})),
                                        "paired and allowlisted identity="
                                    );
                                }
                                Err(e) => {
                                    // The write is the binding. Leaving the
                                    // reservation uncommitted drops the token
                                    // and hands the code back, so a sender the
                                    // admission matcher still rejects never
                                    // holds the only spent code.
                                    drop(reservation);
                                    ::clawcrew_log::record!(
                                        ERROR,
                                        ::clawcrew_log::Event::new(
                                            module_path!(),
                                            ::clawcrew_log::Action::Fail
                                        )
                                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                                        .with_attrs(::serde_json::json!({"e": e.to_string()})),
                                        "rolled back bind: could not persist allowlist"
                                    );
                                    let _ = self
                                        .send(&SendMessage::new(
                                            "❌ Could not save the binding, so nothing was changed. Your code is still valid; ask the operator to check the config file, then retry.",
                                            &chat_id,
                                        ))
                                        .await;
                                }
                            }
                        } else {
                            // Nothing to persist means nothing was bound.
                            drop(reservation);
                            let _ = self
                                .send(&SendMessage::new(
                                    "❌ Could not identify your Telegram account. Ensure your account has a username or stable user ID, then retry.",
                                    &chat_id,
                                ))
                                .await;
                        }
                    }
                    Ok(None) => {
                        let _ = self
                            .send(&SendMessage::new(
                                "❌ Invalid binding code. Ask operator for the latest code and retry.",
                                &chat_id,
                            ))
                            .await;
                    }
                    Err(lockout_secs) => {
                        let _ = self
                            .send(&SendMessage::new(
                                format!("⏳ Too many invalid attempts. Retry in {lockout_secs}s."),
                                &chat_id,
                            ))
                            .await;
                    }
                }
            } else {
                let _ = self
                    .send(&SendMessage::new(
                        "ℹ️ Telegram pairing is not active. Ask operator to add your user ID to the matching peer_groups.telegram_<alias>.external_peers entry in config.toml.",
                        &chat_id,
                    ))
                    .await;
            }
            return;
        }

        // No bind code — this is heading for the approval notice. Bail
        // silently here if the parsers would have permanently rejected
        // this exact update on size/duration alone: an authorized
        // sender's identical voice/attachment would be dropped for the
        // same reason, so the notice must not promise processing that
        // can never happen.
        if self.message_exceeds_parser_limits(message) {
            return;
        }

        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown),
            &format!(
                "ignoring message from unauthorized user: username={username}, sender_id={}. \
Allowlist Telegram username (without '@') or numeric user ID.",
                sender_id_str.as_deref().unwrap_or("unknown")
            )
        );

        let suggested_identity = normalized_sender_id
            .clone()
            .or_else(|| {
                if normalized_username.is_empty() || normalized_username == "unknown" {
                    None
                } else {
                    Some(normalized_username.clone())
                }
            })
            .unwrap_or_else(|| "YOUR_TELEGRAM_ID".to_string());

        // Emit the bind command scoped to THIS channel's alias. The CLI
        // handler defaults to the `default` alias, so an alias-less command
        // would silently bind the wrong peer group for a non-default agent
        // and the bot would keep demanding approval.
        let bind_command = Self::suggested_bind_command(&self.alias, &suggested_identity);

        let _ = self
            .send(&SendMessage::new(
                format!(
                    "🔐 This bot requires operator approval.\n\nCopy this command to the operator terminal:\n`{bind_command}`\n\nAfter the operator runs it, send your message again."
                ),
                &chat_id,
            ))
            .await;

        // Only offer the `/bind <code>` path while the channel is genuinely
        // unpaired. Once peers exist (resolved live), the one-time code is
        // moot and the hint just confuses an operator who already authorized
        // someone — the "already assigned but still asks" complaint.
        if self.pairing_code_active() && !self.has_authorized_peer() {
            let _ = self
                .send(&SendMessage::new(
                    "ℹ️ If the operator provides a one-time pairing code, you can also run `/bind <code>`.",
                    &chat_id,
                ))
                .await;
        }
    }

    /// Get the file path for a Telegram file ID via the Bot API.
    ///
    /// Failures carry the vendor's HTTP status, `ok` flag, `error_code`, and
    /// `description`, classified as [`FileLookupFailure::Permanent`] or
    /// `Transient` so the caller can acknowledge an update whose download can
    /// never succeed instead of retrying it forever.
    async fn get_file_path(&self, file_id: &str) -> Result<String, FileLookupError> {
        let url = self.api_url("getFile");
        let resp = match self
            .http_client()
            .get(&url)
            .query(&[("file_id", file_id)])
            .send()
            .await
        {
            Ok(r) => r,
            // A transport failure says nothing about the file id.
            Err(e) => {
                return Err(FileLookupError::transient(format!(
                    "Failed to call Telegram getFile: {e}"
                )));
            }
        };

        let status = resp.status();
        let body: Option<serde_json::Value> = resp.json().await.ok();

        // The happy path: a usable file_path regardless of envelope noise.
        if let Some(path) = body
            .as_ref()
            .and_then(|b| b.get("result"))
            .and_then(|r| r.get("file_path"))
            .and_then(serde_json::Value::as_str)
        {
            return Ok(path.to_string());
        }

        Err(FileLookupError::classify(status, body.as_ref()))
    }

    /// Download a file from the Telegram CDN.
    async fn download_file(&self, file_path: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!("{}/file/bot{}/{file_path}", self.api_base, self.bot_token);
        let resp = self
            .http_client()
            .get(&url)
            .send()
            .await
            .context("Failed to download Telegram file")?;

        if !resp.status().is_success() {
            anyhow::bail!("Telegram file download failed: {}", resp.status());
        }

        Ok(resp.bytes().await?.to_vec())
    }

    /// Extract (file_id, duration) from a voice or audio message.
    fn parse_voice_metadata(message: &serde_json::Value) -> Option<(String, u64)> {
        let voice = message.get("voice").or_else(|| message.get("audio"))?;
        let file_id = voice.get("file_id")?.as_str()?.to_string();
        let duration = voice
            .get("duration")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        Some((file_id, duration))
    }

    /// Extract attachment metadata from an incoming Telegram message (document or photo).
    /// Returns `None` for text-only, voice, and other unsupported message types.
    fn parse_attachment_metadata(message: &serde_json::Value) -> Option<IncomingAttachment> {
        // Try document first
        if let Some(doc) = message.get("document") {
            let file_id = doc.get("file_id")?.as_str()?.to_string();
            let file_name = doc
                .get("file_name")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            let file_size = doc.get("file_size").and_then(serde_json::Value::as_u64);
            let mime_type = doc
                .get("mime_type")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            let caption = message
                .get("caption")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            return Some(IncomingAttachment {
                file_id,
                file_name,
                file_size,
                caption,
                mime_type,
                kind: IncomingAttachmentKind::Document,
            });
        }

        // Try photo (array of PhotoSize, take last = highest resolution)
        if let Some(photos) = message.get("photo").and_then(serde_json::Value::as_array) {
            let best = photos.last()?;
            let file_id = best.get("file_id")?.as_str()?.to_string();
            let file_size = best.get("file_size").and_then(serde_json::Value::as_u64);
            let caption = message
                .get("caption")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            return Some(IncomingAttachment {
                file_id,
                file_name: None,
                file_size,
                caption,
                mime_type: None,
                kind: IncomingAttachmentKind::Photo,
            });
        }

        None
    }

    fn allowed_attachment_sender(&self, message: &serde_json::Value) -> Option<String> {
        let (_, _, sender_identity) = Self::extract_sender_info(message);
        let identities = Self::authorization_identities(message);
        self.is_any_user_allowed(identities.iter().map(String::as_str))
            .then_some(sender_identity)
    }

    /// Download and persist one attachment, returning only its prompt marker.
    /// Authorization, mention gating, captions, replies, and forwarding are
    /// intentionally handled by the caller so an album applies them once.
    async fn materialize_attachment_content(
        &self,
        attachment: &IncomingAttachment,
        chat_id: &str,
        message_id: i64,
        disambiguate_document_name: bool,
    ) -> AttachmentMaterialization {
        if let Some(size) = attachment.file_size
            && size > TELEGRAM_MAX_FILE_DOWNLOAD_BYTES
        {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                &format!(
                    "Skipping attachment: file size {size} bytes exceeds {} MB limit",
                    TELEGRAM_MAX_FILE_DOWNLOAD_BYTES / (1024 * 1024)
                )
            );
            return AttachmentMaterialization::SkipPermanent;
        }

        // Ensure workspace directory is configured
        let Some(workspace) = self.workspace_dir.as_ref().or_else(|| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                "Cannot save attachment: workspace_dir not configured"
            );
            None
        }) else {
            return AttachmentMaterialization::SkipPermanent;
        };

        let save_dir = workspace.join("telegram_files");
        if let Err(e) = tokio::fs::create_dir_all(&save_dir).await {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                "Failed to create telegram_files directory"
            );
            return AttachmentMaterialization::RetryTransient;
        }

        // Download file from Telegram
        let tg_file_path = match self.get_file_path(&attachment.file_id).await {
            Ok(p) => p,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "error": clawcrew_runtime::security::scrub(&format!("{}", e)),
                            "classification": format!("{:?}", e.kind),
                        })),
                    "Failed to get attachment file path"
                );
                // A permanently rejected file id can never download; retrying
                // it head-of-line blocks every later update forever.
                return match e.kind {
                    FileLookupFailure::Permanent => AttachmentMaterialization::SkipPermanent,
                    FileLookupFailure::Transient => AttachmentMaterialization::RetryTransient,
                };
            }
        };

        let file_data = match self.download_file(&tg_file_path).await {
            Ok(d) => d,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                    "Failed to download attachment"
                );
                return AttachmentMaterialization::RetryTransient;
            }
        };

        // Determine local filename
        let (local_filename, display_filename) = match &attachment.file_name {
            Some(name) => {
                let display_filename = safe_attachment_filename(name);
                let local_filename = if disambiguate_document_name
                    && attachment.kind == IncomingAttachmentKind::Document
                {
                    media_group_document_storage_filename(&display_filename, chat_id, message_id)
                } else {
                    display_filename.clone()
                };
                (local_filename, display_filename)
            }
            None => {
                // For photos, derive extension from Telegram file path
                let ext = tg_file_path.rsplit('.').next().unwrap_or("jpg");
                let filename = format!("photo_{chat_id}_{message_id}.{ext}");
                (filename.clone(), filename)
            }
        };

        let local_path = save_dir.join(&local_filename);
        if let Err(e) = tokio::fs::write(&local_path, &file_data).await {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                &format!("Failed to save attachment to {}", local_path.display())
            );
            return AttachmentMaterialization::RetryTransient;
        }

        // Preserve the typed envelope as the source of truth for both the
        // rendered marker and downstream attachment classification. Albums
        // collect these envelopes alongside their ordered content markers.
        let mut media_attachment = clawcrew_api::media::MediaAttachment {
            file_name: display_filename,
            data: file_data,
            mime_type: attachment.mime_type.clone(),
            marker: None,
        };
        let marker_kind = attachment_marker_kind(&media_attachment);
        media_attachment.marker = Some(clawcrew_api::media::RenderedMarker {
            target: local_path.display().to_string(),
            kind: marker_kind,
        });
        let content = format_attachment_content(&media_attachment, &local_path);

        AttachmentMaterialization::Ready {
            content,
            attachment: media_attachment,
        }
    }

    fn finalize_attachment_message(
        &self,
        message: &serde_json::Value,
        sender_identity: String,
        mut content: String,
        gated_caption: Option<&str>,
        attachments: Vec<clawcrew_api::media::MediaAttachment>,
    ) -> Option<ChannelMessage> {
        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string())?;
        let message_id = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let (_, sender_id, _) = Self::extract_sender_info(message);
        let thread_id = Self::topic_thread_id(message);
        let reply_target = if let Some(ref tid) = thread_id {
            format!("{chat_id}:{tid}")
        } else {
            chat_id.clone()
        };

        if let Some(caption) = gated_caption
            && !caption.is_empty()
        {
            use std::fmt::Write;
            let _ = write!(content, "\n\n{caption}");
        }

        // Prepend reply context if replying to another message
        if let Some(quote) = self.extract_reply_context(message) {
            content = format!("{quote}\n\n{content}");
        }

        // Prepend forwarding attribution when the message was forwarded
        if let Some(attr) = Self::format_forward_attribution(message) {
            content = Self::prepend_forward_attribution(&attr, content);
        }

        Some(ChannelMessage {
            id: format!("telegram_{chat_id}_{message_id}"),
            sender: sender_identity,
            platform_sender_id: sender_id,
            reply_target,
            content,
            channel: "telegram".into(),
            channel_alias: Some(self.alias.clone()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: thread_id,
            interruption_scope_id: None,
            attachments,
            subject: None,
            conversation_scope: self.conversation_scope_for(message),

            ..Default::default()
        })
    }

    /// Attempt to parse a Telegram update as a document/photo attachment.
    ///
    /// Downloads the file to `{workspace_dir}/telegram_files/` and returns a
    /// parsed message with matching rendered content and typed attachment
    /// metadata. `pub(crate)` lets orchestrator regressions drive the real
    /// parser through the typed media boundary.
    pub(crate) async fn try_parse_attachment_message(
        &self,
        update: &serde_json::Value,
    ) -> UpdateDisposition {
        let Some(message) = update.get("message") else {
            return UpdateDisposition::SkipPermanent;
        };
        let Some(attachment) = Self::parse_attachment_metadata(message) else {
            return UpdateDisposition::SkipPermanent;
        };
        let Some(sender_identity) = self.allowed_attachment_sender(message) else {
            return UpdateDisposition::SkipPermanent;
        };

        // Apply mention_only gate before downloading. Photo / document
        // updates carry no `text` field, so the text-only gate in
        // `parse_update_message` can never see them and they used to slip
        // through unconditionally.
        let Some(gated_caption) =
            self.check_media_mention_gate(message, attachment.caption.as_deref())
        else {
            return UpdateDisposition::SkipPermanent;
        };
        let Some((chat_id, message_id)) = Self::extract_update_message_target(update) else {
            return UpdateDisposition::SkipPermanent;
        };
        let (content, media_attachment) = match self
            .materialize_attachment_content(&attachment, &chat_id, message_id, false)
            .await
        {
            AttachmentMaterialization::Ready {
                content,
                attachment,
            } => (content, attachment),
            AttachmentMaterialization::SkipPermanent => {
                return UpdateDisposition::SkipPermanent;
            }
            AttachmentMaterialization::RetryTransient => {
                return UpdateDisposition::RetryTransient;
            }
        };
        self.finalize_attachment_message(
            message,
            sender_identity,
            content,
            gated_caption.as_deref(),
            vec![media_attachment],
        )
        .map(|message| UpdateDisposition::Parsed(Box::new(message)))
        .unwrap_or(UpdateDisposition::SkipPermanent)
    }

    /// Materialize one settled Telegram media group as one inbound message.
    /// Group scope, authorization, and mention gating are validated before
    /// any file download. Individual attachment failures do not discard
    /// successfully materialized siblings.
    /// Album parsing with the text-only context of unsupported members.
    ///
    /// `unsupported` never reaches `parse_attachment_metadata()` or `getFile`;
    /// it only widens caption aggregation and the mention gate so a caption or
    /// mention carried by a member we cannot download is not silently dropped.
    async fn try_parse_media_group_with_unsupported(
        &self,
        updates: &[serde_json::Value],
        unsupported: &[UnsupportedMember],
    ) -> UpdateDisposition {
        let mut ordered: Vec<&serde_json::Value> = updates.iter().collect();
        ordered.sort_by_key(|update| Self::update_message_id(update).unwrap_or(i64::MAX));

        // An album with no materializable members must dispatch nothing and
        // download nothing, exactly as before -- unsupported context alone can
        // never produce a turn.
        let Some(anchor_update) = ordered.first().copied() else {
            return UpdateDisposition::SkipPermanent;
        };
        let Some(anchor_message) = anchor_update.get("message") else {
            return UpdateDisposition::SkipPermanent;
        };
        let anchor_scope = Self::media_group_scope(anchor_message);
        let supported_share_scope = ordered.iter().all(|update| {
            update.get("message").is_some_and(|message| {
                Self::media_group_members_share_scope(anchor_message, message)
            })
        });
        let unsupported_share_scope = unsupported
            .iter()
            .all(|member| Self::media_group_scopes_match(&anchor_scope, &member.scope));
        if !supported_share_scope || !unsupported_share_scope {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                "Rejecting Telegram media group with mixed chat, sender, or thread scope"
            );
            return UpdateDisposition::SkipPermanent;
        }

        let Some(sender_identity) = self.allowed_attachment_sender(anchor_message) else {
            return UpdateDisposition::SkipPermanent;
        };

        // Caption aggregation spans the whole album -- materialized members and
        // text-only ones -- merged in `message_id` order before dedup + join.
        let mut captioned: Vec<(i64, &str)> = ordered
            .iter()
            .filter_map(|update| {
                let message_id = Self::update_message_id(update)?;
                let caption = update
                    .get("message")
                    .and_then(|message| message.get("caption"))
                    .and_then(serde_json::Value::as_str)?;
                Some((message_id, caption))
            })
            .collect();
        captioned.extend(
            unsupported
                .iter()
                .filter_map(|member| Some((member.message_id, member.caption.as_deref()?))),
        );
        captioned.sort_by_key(|(message_id, _)| *message_id);

        let mut seen_captions = std::collections::HashSet::new();
        let shared_caption = captioned
            .into_iter()
            .map(|(_, caption)| caption)
            .filter(|caption| !caption.trim().is_empty())
            .filter(|caption| seen_captions.insert((*caption).to_string()))
            .collect::<Vec<_>>()
            .join("\n\n");
        let Some(gated_caption) = self.check_media_mention_gate(
            anchor_message,
            (!shared_caption.is_empty()).then_some(shared_caption.as_str()),
        ) else {
            return UpdateDisposition::SkipPermanent;
        };

        let mut contents = Vec::with_capacity(ordered.len());
        let mut attachments = Vec::with_capacity(ordered.len());
        for update in ordered {
            let Some(message) = update.get("message") else {
                continue;
            };
            let Some(attachment) = Self::parse_attachment_metadata(message) else {
                continue;
            };
            let Some((chat_id, message_id)) = Self::extract_update_message_target(update) else {
                continue;
            };
            match self
                .materialize_attachment_content(&attachment, &chat_id, message_id, true)
                .await
            {
                AttachmentMaterialization::Ready {
                    content,
                    attachment,
                } => {
                    contents.push(content);
                    attachments.push(attachment);
                }
                AttachmentMaterialization::SkipPermanent => {}
                AttachmentMaterialization::RetryTransient => {
                    return UpdateDisposition::RetryTransient;
                }
            }
        }

        if contents.is_empty() {
            return UpdateDisposition::SkipPermanent;
        }

        self.finalize_attachment_message(
            anchor_message,
            sender_identity,
            contents.join("\n\n"),
            gated_caption.as_deref(),
            attachments,
        )
        .map(|message| UpdateDisposition::Parsed(Box::new(message)))
        .unwrap_or(UpdateDisposition::SkipPermanent)
    }

    /// Tell the sender why their voice message will not be answered.
    ///
    /// Best effort by design: if the notice itself cannot be delivered the
    /// drop is still permanent, so the failure is logged and swallowed rather
    /// than turned into a retry of the original update.
    ///
    /// The whole attempt is bounded by [`VOICE_DROP_NOTICE_TIMEOUT`]. This
    /// runs before the permanent skip lets the offset advance, and the
    /// sending client has no request timeout of its own — an unbounded await
    /// on a stalled request or response body would head-of-line block every
    /// later update on this listener. Rejections that never touched the
    /// network before (an over-duration recording) must not start doing so
    /// just because they now say goodbye.
    async fn notify_voice_drop(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        reason: VoiceDropReason,
    ) {
        let notice = reason.notice();
        let attempt = self.send_text_chunks(&notice, chat_id, thread_id, 0);
        match tokio::time::timeout(self.voice_drop_notice_timeout, attempt).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "error": clawcrew_runtime::security::scrub(&format!("{}", e)),
                            "reason": format!("{reason:?}"),
                        })),
                    "Failed to notify sender about skipped voice message"
                );
            }
            Err(_) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "timeout_secs": self.voice_drop_notice_timeout.as_secs_f64(),
                            "reason": format!("{reason:?}"),
                        })),
                    "Timed out notifying sender about skipped voice message; abandoning the notice"
                );
            }
        }
    }

    /// Attempt to parse a Telegram update as a voice message and transcribe it.
    /// Returns `SkipPermanent` if the message is not a voice message, transcription is
    /// disabled, or the message exceeds duration limits; `RetryTransient` if download or
    /// transcription I/O fails.
    ///
    /// Every permanent drop that reaches an allowed sender is announced to them
    /// (see [`VoiceDropReason`]): silence is indistinguishable from a bot that
    /// never received the recording. Transient failures stay silent — the same
    /// update is retried, and a notice per attempt would be spam.
    async fn try_parse_voice_message(&self, update: &serde_json::Value) -> UpdateDisposition {
        let Some(config) = self.transcription.as_ref() else {
            return UpdateDisposition::SkipPermanent;
        };
        let Some(manager) = self.transcription_manager.as_deref() else {
            return UpdateDisposition::SkipPermanent;
        };
        let Some(message) = update.get("message") else {
            return UpdateDisposition::SkipPermanent;
        };

        let Some((file_id, duration)) = Self::parse_voice_metadata(message) else {
            return UpdateDisposition::SkipPermanent;
        };

        // The duration check used to run here, before the sender was known.
        // It now runs once the chat is resolved and the sender has passed the
        // allowlist and mention gate, so the skip can be explained to them —
        // and so a stranger's oversized recording still costs nothing: the
        // check stays ahead of every download.
        let (_, sender_id, sender_identity) = Self::extract_sender_info(message);

        let identities = Self::authorization_identities(message);

        if !self.is_any_user_allowed(identities.iter().map(String::as_str)) {
            return UpdateDisposition::SkipPermanent;
        }

        let voice_caption = message.get("caption").and_then(serde_json::Value::as_str);
        if self
            .check_media_mention_gate(message, voice_caption)
            .is_none()
        {
            return UpdateDisposition::SkipPermanent;
        }

        let Some(chat_id) = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string())
        else {
            return UpdateDisposition::SkipPermanent;
        };

        let message_id = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        let thread_id = Self::topic_thread_id(message);

        let reply_target = if let Some(ref tid) = thread_id {
            format!("{}:{}", chat_id, tid)
        } else {
            chat_id.clone()
        };

        if duration > config.max_duration_secs {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                &format!(
                    "Skipping voice message: duration {duration}s exceeds limit {}s",
                    config.max_duration_secs
                )
            );
            self.notify_voice_drop(
                &chat_id,
                thread_id.as_deref(),
                VoiceDropReason::TooLong {
                    limit_secs: config.max_duration_secs,
                },
            )
            .await;
            return UpdateDisposition::SkipPermanent;
        }

        // Download and transcribe
        let file_path = match self.get_file_path(&file_id).await {
            Ok(p) => p,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "error": clawcrew_runtime::security::scrub(&format!("{}", e)),
                            "classification": format!("{:?}", e.kind),
                        })),
                    "Failed to get voice file path"
                );
                // See the attachment path: a permanent vendor rejection must
                // not hold the offset, or the batch never drains.
                return match e.kind {
                    FileLookupFailure::Permanent => {
                        self.notify_voice_drop(
                            &chat_id,
                            thread_id.as_deref(),
                            VoiceDropReason::FileUnavailable,
                        )
                        .await;
                        UpdateDisposition::SkipPermanent
                    }
                    FileLookupFailure::Transient => UpdateDisposition::RetryTransient,
                };
            }
        };

        let file_name = file_path
            .rsplit('/')
            .next()
            .unwrap_or("voice.ogg")
            .to_string();

        let audio_data = match self.download_file(&file_path).await {
            Ok(d) => d,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                    "Failed to download voice file"
                );
                return UpdateDisposition::RetryTransient;
            }
        };

        let text = match manager.transcribe(&audio_data, &file_name).await {
            Ok(t) => t,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                    "Voice transcription failed"
                );
                return UpdateDisposition::RetryTransient;
            }
        };

        if text.trim().is_empty() {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "Voice transcription returned empty text, skipping"
            );
            self.notify_voice_drop(
                &chat_id,
                thread_id.as_deref(),
                VoiceDropReason::EmptyTranscript,
            )
            .await;
            return UpdateDisposition::SkipPermanent;
        }

        // Enter voice-chat mode so outgoing replies get a TTS voice note
        if let Ok(mut vc) = self.voice_chats.lock() {
            vc.insert(reply_target.clone());
        }

        // Cache transcription for reply-context lookups
        {
            let mut cache = self.voice_transcriptions.lock();
            if cache.len() >= 100 {
                cache.clear();
            }
            cache.insert(format!("{chat_id}:{message_id}"), text.clone());
        }

        let content = if let Some(quote) = self.extract_reply_context(message) {
            format!("{quote}\n\n[Voice] {text}")
        } else {
            format!("[Voice] {text}")
        };

        // Prepend forwarding attribution when the message was forwarded
        let content = if let Some(attr) = Self::format_forward_attribution(message) {
            Self::prepend_forward_attribution(&attr, content)
        } else {
            content
        };

        UpdateDisposition::Parsed(Box::new(ChannelMessage {
            id: format!("telegram_{chat_id}_{message_id}"),
            sender: sender_identity,
            platform_sender_id: sender_id,
            reply_target,
            content,
            channel: "telegram".into(),
            channel_alias: Some(self.alias.clone()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: thread_id,
            interruption_scope_id: None,
            attachments: vec![],
            subject: None,
            conversation_scope: self.conversation_scope_for(message),

            ..Default::default()
        }))
    }

    /// The identifiers this sender can be authorized by.
    ///
    /// Deliberately not `extract_sender_info`'s `username`, which substitutes
    /// the display placeholder `"unknown"` when Telegram sends no username at
    /// all. That string is a label, not an identifier, and passing it to the
    /// allowlist let a sender with no usable identity ride a wildcard grant. A
    /// sender genuinely named `unknown` still authorizes, because presence is
    /// read from the JSON field rather than from the placeholder's spelling.
    fn authorization_identities(message: &serde_json::Value) -> Vec<String> {
        let from = message.get("from");
        let mut out = Vec::new();
        if let Some(username) = from
            .and_then(|from| from.get("username"))
            .and_then(serde_json::Value::as_str)
        {
            out.push(username.to_string());
        }
        if let Some(id) = from
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_i64)
        {
            out.push(id.to_string());
        }
        out
    }

    /// Extract sender username and display identity from a Telegram message object.
    fn extract_sender_info(message: &serde_json::Value) -> (String, Option<String>, String) {
        let from = message.get("from");
        let username = from
            .and_then(|from| from.get("username"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let sender_id = from
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());
        let sender_identity = from
            .and_then(Self::telegram_sender_identity)
            .unwrap_or_else(|| "unknown".to_string());
        (username, sender_id, sender_identity)
    }

    /// Build a forwarding attribution prefix from Telegram forward fields.
    /// Returns `Some("[Forwarded from ...] ")` when the message is forwarded,
    /// `None` otherwise.
    fn format_forward_attribution(message: &serde_json::Value) -> Option<String> {
        if let Some(origin) = message.get("forward_origin") {
            let origin_type = origin.get("type").and_then(serde_json::Value::as_str)?;
            let label = match origin_type {
                "user" => {
                    let sender = origin.get("sender_user")?;
                    Self::format_forwarded_user_label(sender, "unknown")
                }
                "hidden_user" => origin
                    .get("sender_user_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown hidden user")
                    .to_string(),
                "chat" => {
                    let title = origin
                        .get("sender_chat")
                        .and_then(|chat| chat.get("title"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown chat");
                    format!("chat: {title}")
                }
                "channel" => {
                    let title = origin
                        .get("chat")
                        .and_then(|chat| chat.get("title"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown channel");
                    format!("channel: {title}")
                }
                _ => "unknown source".to_string(),
            };
            Some(format!("[Forwarded from {label}] "))
        } else if let Some(from_chat) = message.get("forward_from_chat") {
            // Forwarded from a channel or group
            let title = from_chat
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown channel");
            Some(format!("[Forwarded from channel: {title}] "))
        } else if let Some(from_user) = message.get("forward_from") {
            // Forwarded from a user (privacy allows identity)
            let label = Self::format_forwarded_user_label(from_user, "unknown");
            Some(format!("[Forwarded from {label}] "))
        } else {
            // Forwarded from a user who hides their identity
            message
                .get("forward_sender_name")
                .and_then(serde_json::Value::as_str)
                .map(|name| format!("[Forwarded from {name}] "))
        }
    }

    fn prepend_forward_attribution(attr: &str, content: String) -> String {
        let attr = attr.trim_end();
        if content.starts_with("> ") {
            format!("{attr}\n\n{content}")
        } else {
            format!("{attr} {content}")
        }
    }

    fn format_forwarded_user_label(user: &serde_json::Value, fallback: &str) -> String {
        if let Some(username) = user.get("username").and_then(serde_json::Value::as_str) {
            return format!("@{username}");
        }

        let Some(first_name) = user.get("first_name").and_then(serde_json::Value::as_str) else {
            return fallback.to_string();
        };

        let mut label = first_name.to_string();
        if let Some(last_name) = user.get("last_name").and_then(serde_json::Value::as_str) {
            label.push(' ');
            label.push_str(last_name);
        }
        label
    }

    /// Extract reply context from a Telegram `reply_to_message`, if present.
    fn extract_reply_context(&self, message: &serde_json::Value) -> Option<String> {
        let reply = message.get("reply_to_message")?;

        let reply_mid = reply.get("message_id").and_then(serde_json::Value::as_i64);
        let thread_id = message
            .get("message_thread_id")
            .and_then(serde_json::Value::as_i64);
        if let (Some(rmid), Some(tid)) = (reply_mid, thread_id)
            && rmid == tid
        {
            return None;
        }

        let reply_sender = reply
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                reply
                    .get("from")
                    .and_then(|from| from.get("first_name"))
                    .and_then(serde_json::Value::as_str)
            })
            .unwrap_or("unknown");

        let reply_text = if let Some(text) = reply.get("text").and_then(serde_json::Value::as_str) {
            text.to_string()
        } else if reply.get("voice").is_some() || reply.get("audio").is_some() {
            let reply_mid = reply.get("message_id").and_then(serde_json::Value::as_i64);
            let chat_id = message
                .get("chat")
                .and_then(|c| c.get("id"))
                .and_then(serde_json::Value::as_i64);
            if let (Some(mid), Some(cid)) = (reply_mid, chat_id) {
                self.voice_transcriptions
                    .lock()
                    .get(&format!("{cid}:{mid}"))
                    .map(|t| format!("[Voice] {t}"))
                    .unwrap_or_else(|| "[Voice message]".to_string())
            } else {
                "[Voice message]".to_string()
            }
        } else if reply.get("photo").is_some() {
            "[Photo]".to_string()
        } else if reply.get("document").is_some() {
            "[Document]".to_string()
        } else if reply.get("video").is_some() {
            "[Video]".to_string()
        } else if reply.get("sticker").is_some() {
            "[Sticker]".to_string()
        } else {
            "[Message]".to_string()
        };

        // Format as blockquote with sender attribution
        let quoted_lines: String = reply_text
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        Some(format!("> @{reply_sender}:\n{quoted_lines}"))
    }

    /// Forum-topic thread id for history keying and reply routing, if this
    /// message belongs to a genuine forum topic. Telegram also sets
    /// `message_thread_id` for ordinary reply-threads in supergroups, which are
    /// NOT topic boundaries and must continue the main chat's conversation
    /// history — so gate on `is_topic_message` and treat a non-topic thread as
    /// the main chat (no `thread_ts`, no `:tid` on `reply_target`).
    fn topic_thread_id(message: &serde_json::Value) -> Option<String> {
        if message
            .get("is_topic_message")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        {
            return None;
        }
        message
            .get("message_thread_id")
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string())
    }

    fn parse_update_message(&self, update: &serde_json::Value) -> Option<ChannelMessage> {
        let message = update.get("message")?;

        let text = message.get("text").and_then(serde_json::Value::as_str)?;

        let (_, sender_id, sender_identity) = Self::extract_sender_info(message);

        let identities = Self::authorization_identities(message);

        if !self.is_any_user_allowed(identities.iter().map(String::as_str)) {
            return None;
        }

        let is_group = Self::is_group_message(message);
        let mut passive_context = false;
        if self.mention_only && is_group {
            let bot_username = self.bot_username.lock();
            let bot_username = bot_username.as_ref()?;
            // A direct reply to the bot's message is an unambiguous signal
            // of intent, so it counts as addressed alongside an @-mention.
            let addressed = Self::contains_bot_mention(text, bot_username) || {
                let bot_id = *self.bot_id.lock();
                bot_id.is_some_and(|id| Self::is_reply_to_bot(message, id))
            };
            if !addressed {
                if Self::should_record_passive_group_context(
                    self.passive_group_context,
                    is_group,
                    addressed,
                ) {
                    passive_context = true;
                } else {
                    return None;
                }
            }
        }

        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string())?;

        let message_id = message
            .get("message_id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        // Extract thread/topic ID for forum support
        let thread_id = Self::topic_thread_id(message);

        // reply_target: chat_id or chat_id:thread_id format
        let reply_target = if let Some(ref tid) = thread_id {
            format!("{}:{}", chat_id, tid)
        } else {
            chat_id.clone()
        };

        let content = if self.mention_only && is_group && !passive_context {
            let bot_username = self.bot_username.lock();
            let bot_username = bot_username.as_ref()?;
            Self::normalize_incoming_content(text, bot_username)?
        } else {
            text.to_string()
        };

        let content = if let Some(quote) = self.extract_reply_context(message) {
            format!("{quote}\n\n{content}")
        } else {
            content
        };

        // Prepend forwarding attribution when the message was forwarded
        let content = if let Some(attr) = Self::format_forward_attribution(message) {
            Self::prepend_forward_attribution(&attr, content)
        } else {
            content
        };

        // Exit input-driven voice mode when a sender switches back to typing.
        // A sender configured for voice output (output_modality = "voice") keeps
        // the conversation in voice mode regardless of whether they send text or
        // voice. The peer group names their identity, not this chat's address.
        // A passive observation is not that participant, so it leaves the room's
        // voice mode alone.
        let sender_is_voice_peer = self.is_voice_peer(&sender_identity)
            || sender_id
                .as_deref()
                .is_some_and(|id| self.is_voice_peer(id));
        if !passive_context
            && !sender_is_voice_peer
            && let Ok(mut vc) = self.voice_chats.lock()
        {
            vc.remove(&reply_target);
        }

        Some(ChannelMessage {
            id: format!("telegram_{chat_id}_{message_id}"),
            sender: sender_identity,
            platform_sender_id: sender_id,
            reply_target,
            content,
            channel: "telegram".into(),
            channel_alias: Some(self.alias.clone()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: thread_id,
            interruption_scope_id: None,
            attachments: vec![],
            subject: None,
            passive_context,
            conversation_scope: self.conversation_scope_for(message),

            ..Default::default()
        })
    }

    /// Convert Markdown to Telegram HTML format.
    /// Telegram HTML supports: <b>, <i>, <u>, <s>, <code>, <pre>, <a href="...">
    /// This mirrors OpenClaw's markdownToTelegramHtml approach.
    fn markdown_to_telegram_html(text: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let mut result_lines: Vec<String> = Vec::new();

        for line in &lines {
            let trimmed_line = line.trim_start();
            if trimmed_line.starts_with("```") {
                // Preserve fence lines so the second-pass block parser can consume them
                // without interference from inline backtick handling.
                result_lines.push(trimmed_line.to_string());
                continue;
            }

            let mut line_out = String::new();

            // Handle code blocks (``` ... ```) - handled at text level below
            // Handle headers: ## Title → <b>Title</b>
            let stripped = line.trim_start_matches('#');
            let header_level = line.len() - stripped.len();
            if header_level > 0 && line.starts_with('#') && stripped.starts_with(' ') {
                let title = Self::escape_html(stripped.trim());
                result_lines.push(format!("<b>{title}</b>"));
                continue;
            }

            // Inline formatting
            let mut i = 0;
            let bytes = line.as_bytes();
            let len = bytes.len();
            while i < len {
                // Bold: **text** or __text__
                if i + 1 < len
                    && bytes[i] == b'*'
                    && bytes[i + 1] == b'*'
                    && let Some(end) = line[i + 2..].find("**")
                {
                    let inner = Self::escape_html(&line[i + 2..i + 2 + end]);
                    let _ = write!(line_out, "<b>{inner}</b>");
                    i += 4 + end;
                    continue;
                }
                if i + 1 < len
                    && bytes[i] == b'_'
                    && bytes[i + 1] == b'_'
                    && let Some(end) = line[i + 2..].find("__")
                {
                    let inner = Self::escape_html(&line[i + 2..i + 2 + end]);
                    let _ = write!(line_out, "<b>{inner}</b>");
                    i += 4 + end;
                    continue;
                }
                // Italic: *text* or _text_ (single)
                if bytes[i] == b'*'
                    && (i == 0 || bytes[i - 1] != b'*')
                    && let Some(end) = line[i + 1..].find('*')
                    && end > 0
                {
                    let inner = Self::escape_html(&line[i + 1..i + 1 + end]);
                    let _ = write!(line_out, "<i>{inner}</i>");
                    i += 2 + end;
                    continue;
                }
                // Inline code: `code`
                if bytes[i] == b'`'
                    && (i == 0 || bytes[i - 1] != b'`')
                    && let Some(end) = line[i + 1..].find('`')
                {
                    let inner = Self::escape_html(&line[i + 1..i + 1 + end]);
                    let _ = write!(line_out, "<code>{inner}</code>");
                    i += 2 + end;
                    continue;
                }
                // Markdown link: [text](url)
                if bytes[i] == b'['
                    && let Some(bracket_end) = line[i + 1..].find(']')
                {
                    let text_part = &line[i + 1..i + 1 + bracket_end];
                    let after_bracket = i + 1 + bracket_end + 1; // position after ']'
                    if after_bracket < len
                        && bytes[after_bracket] == b'('
                        && let Some(paren_end) = line[after_bracket + 1..].find(')')
                    {
                        let url = &line[after_bracket + 1..after_bracket + 1 + paren_end];
                        if url.starts_with("http://") || url.starts_with("https://") {
                            let text_html = Self::escape_html(text_part);
                            let url_html = Self::escape_html(url);
                            let _ = write!(line_out, "<a href=\"{url_html}\">{text_html}</a>");
                            i = after_bracket + 1 + paren_end + 1;
                            continue;
                        }
                    }
                }
                // Strikethrough: ~~text~~
                if i + 1 < len
                    && bytes[i] == b'~'
                    && bytes[i + 1] == b'~'
                    && let Some(end) = line[i + 2..].find("~~")
                {
                    let inner = Self::escape_html(&line[i + 2..i + 2 + end]);
                    let _ = write!(line_out, "<s>{inner}</s>");
                    i += 4 + end;
                    continue;
                }
                // Default: escape HTML entities
                let Some(ch) = line[i..].chars().next() else {
                    break;
                };
                match ch {
                    '<' => line_out.push_str("&lt;"),
                    '>' => line_out.push_str("&gt;"),
                    '&' => line_out.push_str("&amp;"),
                    '"' => line_out.push_str("&quot;"),
                    '\'' => line_out.push_str("&#39;"),
                    _ => line_out.push(ch),
                }
                i += ch.len_utf8();
            }
            result_lines.push(line_out);
        }

        // Second pass: handle ``` code blocks across lines
        let joined = result_lines.join("\n");
        let mut final_out = String::with_capacity(joined.len());
        let mut in_code_block = false;
        let mut code_buf = String::new();

        for line in joined.split('\n') {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code_block {
                    in_code_block = false;
                    let escaped = code_buf.trim_end_matches('\n');
                    // Telegram HTML parse mode supports <pre> and <code>, but not class attributes.
                    let _ = writeln!(final_out, "<pre><code>{escaped}</code></pre>");
                    code_buf.clear();
                } else {
                    in_code_block = true;
                    code_buf.clear();
                }
            } else if in_code_block {
                code_buf.push_str(line);
                code_buf.push('\n');
            } else {
                final_out.push_str(line);
                final_out.push('\n');
            }
        }
        if in_code_block && !code_buf.is_empty() {
            let _ = writeln!(final_out, "<pre><code>{}</code></pre>", code_buf.trim_end());
        }

        final_out.trim_end_matches('\n').to_string()
    }

    fn escape_html(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }

    /// Sends `message` as one or more physical Telegram messages, skipping
    /// the first `skip_chunks` (already delivered by an earlier call). On
    /// success returns the total chunk count; on a chunk failing both HTML
    /// and plain-text send attempts, returns how many chunks were delivered
    /// before the failure so the caller can resume without duplicating them.
    async fn send_text_chunks(
        &self,
        message: &str,
        chat_id: &str,
        thread_id: Option<&str>,
        skip_chunks: usize,
    ) -> Result<usize, SendChunksError> {
        let chunks = split_message_for_telegram(message);

        for (index, chunk) in chunks.iter().enumerate() {
            if index < skip_chunks {
                continue;
            }

            let text = format_telegram_text_chunk(chunk, index, chunks.len());

            let mut markdown_body = serde_json::json!({
                "chat_id": chat_id,
                "text": Self::markdown_to_telegram_html(&text),
                "parse_mode": "HTML"
            });

            // Add message_thread_id for forum topic support
            if let Some(tid) = thread_id {
                markdown_body["message_thread_id"] = serde_json::Value::String(tid.to_string());
            }

            let markdown_resp = self
                .http_client()
                .post(self.api_url("sendMessage"))
                .json(&markdown_body)
                .send()
                .await
                .map_err(|e| SendChunksError {
                    delivered: index,
                    source: e.into(),
                })?;

            if markdown_resp.status().is_success() {
                if index < chunks.len() - 1 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                continue;
            }

            let markdown_status = markdown_resp.status();
            let markdown_err = markdown_resp.text().await.unwrap_or_default();
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"status": markdown_status.to_string()})),
                "Telegram sendMessage with Markdown failed; retrying without parse_mode"
            );

            let mut plain_body = serde_json::json!({
                "chat_id": chat_id,
                "text": text,
            });

            // Add message_thread_id for forum topic support
            if let Some(tid) = thread_id {
                plain_body["message_thread_id"] = serde_json::Value::String(tid.to_string());
            }
            let plain_resp = self
                .http_client()
                .post(self.api_url("sendMessage"))
                .json(&plain_body)
                .send()
                .await
                .map_err(|e| SendChunksError {
                    delivered: index,
                    source: e.into(),
                })?;

            if !plain_resp.status().is_success() {
                let plain_status = plain_resp.status();
                let plain_err = plain_resp.text().await.unwrap_or_default();
                return Err(SendChunksError {
                    delivered: index,
                    source: anyhow::Error::msg(format!(
                        "Telegram sendMessage failed (markdown {markdown_status}: {markdown_err}; plain {plain_status}: {plain_err})"
                    )),
                });
            }

            if index < chunks.len() - 1 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        Ok(chunks.len())
    }

    /// Finalize-time chunked send that never duplicates an accepted prefix.
    ///
    /// `send_text_chunks` posts a long answer one physical Telegram message at a
    /// time and reports how many chunks it accepted before a failure. On such a
    /// partial failure this first *resumes* from the accepted prefix, so a
    /// transient error still completes the answer without re-posting earlier
    /// chunks. If the resume also fails after some chunks were accepted, it
    /// returns [`FinalizePartialDelivery`] so the orchestrator's generic
    /// finalize fallback does not resend the whole answer and duplicate what
    /// Telegram already delivered. A failure before any chunk is accepted
    /// (`delivered == 0`) is returned as the plain source error: nothing is on
    /// the wire, so a full-message fallback is safe.
    async fn finalize_send_chunks(
        &self,
        text: &str,
        chat_id: &str,
        thread_id: Option<&str>,
    ) -> anyhow::Result<()> {
        match self.send_text_chunks(text, chat_id, thread_id, 0).await {
            Ok(_) => Ok(()),
            Err(SendChunksError {
                delivered: 0,
                source,
            }) => Err(source),
            Err(SendChunksError { delivered, .. }) => {
                // Some chunks are already posted. Resume from the first unsent
                // chunk rather than restarting, then report the accepted prefix
                // if it still cannot finish.
                match self
                    .send_text_chunks(text, chat_id, thread_id, delivered)
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(e) => Err(clawcrew_api::channel::FinalizePartialDelivery {
                        delivered: e.delivered.max(delivered),
                    }
                    .into()),
                }
            }
        }
    }

    async fn send_media_by_url(
        &self,
        method: &str,
        media_field: &str,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
        });
        body[media_field] = serde_json::Value::String(url.to_string());

        if let Some(tid) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(tid.to_string());
        }

        if let Some(cap) = caption {
            body["caption"] = serde_json::Value::String(cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url(method))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("{method} by URL failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(
                ::serde_json::json!({"method": method, "chat_id": chat_id, "url": url})
            ),
            "sent to"
        );
        Ok(())
    }

    async fn send_attachment(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        attachment: &TelegramAttachment,
    ) -> anyhow::Result<()> {
        let target = attachment.target.trim();

        if is_http_url(target) {
            let result = match attachment.kind {
                TelegramAttachmentKind::Image => {
                    self.send_photo_by_url(chat_id, thread_id, target, None)
                        .await
                }
                TelegramAttachmentKind::Document => {
                    self.send_document_by_url(chat_id, thread_id, target, None)
                        .await
                }
                TelegramAttachmentKind::Video => {
                    self.send_video_by_url(chat_id, thread_id, target, None)
                        .await
                }
                TelegramAttachmentKind::Audio => {
                    self.send_audio_by_url(chat_id, thread_id, target, None)
                        .await
                }
                TelegramAttachmentKind::Voice => {
                    self.send_voice_by_url(chat_id, thread_id, target, None)
                        .await
                }
            };

            // If sending media by URL failed (e.g. Telegram can't fetch the URL,
            // wrong content type, etc.), fall back to sending the URL as a text link
            // instead of losing the reply entirely.
            if let Err(e) = result {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"url": target, "error": clawcrew_runtime::security::scrub(&format!("{}", e))})
                        ),
                    "Telegram send media by URL failed; falling back to text link"
                );
                let kind_label = match attachment.kind {
                    TelegramAttachmentKind::Image => "Image",
                    TelegramAttachmentKind::Document => "Document",
                    TelegramAttachmentKind::Video => "Video",
                    TelegramAttachmentKind::Audio => "Audio",
                    TelegramAttachmentKind::Voice => "Voice",
                };
                let fallback_text = format!("{kind_label}: {target}");
                self.send_text_chunks(&fallback_text, chat_id, thread_id, 0)
                    .await
                    .map_err(|e| e.source)?;
            }

            return Ok(());
        }

        // Remap Docker container workspace path (/workspace/...) to the host
        // workspace directory so files written by the containerised runtime
        // can be found and sent by the host-side Telegram sender.
        let remapped;
        let target = if let Some(rel) = target.strip_prefix("/workspace/") {
            if let Some(ws) = &self.workspace_dir {
                remapped = ws.join(rel);
                remapped.to_str().unwrap_or(target)
            } else {
                target
            }
        } else {
            target
        };

        let path = Path::new(target);
        if !path.exists() {
            anyhow::bail!("Telegram attachment path not found: {target}");
        }

        match attachment.kind {
            TelegramAttachmentKind::Image => self.send_photo(chat_id, thread_id, path, None).await,
            TelegramAttachmentKind::Document => {
                self.send_document(chat_id, thread_id, path, None).await
            }
            TelegramAttachmentKind::Video => self.send_video(chat_id, thread_id, path, None).await,
            TelegramAttachmentKind::Audio => self.send_audio(chat_id, thread_id, path, None).await,
            TelegramAttachmentKind::Voice => self.send_voice(chat_id, thread_id, path, None).await,
        }
    }

    /// Send a document/file to a Telegram chat
    pub async fn send_document(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendDocument"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "file_name": file_name})),
            "document sent to"
        );
        Ok(())
    }

    /// Send a document from bytes (in-memory) to a Telegram chat
    pub async fn send_document_bytes(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_bytes: Vec<u8>,
        file_name: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendDocument"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "file_name": file_name})),
            "document sent to"
        );
        Ok(())
    }

    /// Send a photo to a Telegram chat
    pub async fn send_photo(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("photo.jpg");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendPhoto"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "file_name": file_name})),
            "photo sent to"
        );
        Ok(())
    }

    /// Send a photo from bytes (in-memory) to a Telegram chat
    pub async fn send_photo_bytes(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_bytes: Vec<u8>,
        file_name: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendPhoto"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "file_name": file_name})),
            "photo sent to"
        );
        Ok(())
    }

    /// Send a video to a Telegram chat
    pub async fn send_video(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video.mp4");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("video", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendVideo"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendVideo failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "file_name": file_name})),
            "video sent to"
        );
        Ok(())
    }

    /// Send an audio file to a Telegram chat
    pub async fn send_audio(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.mp3");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("audio", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendAudio"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendAudio failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "file_name": file_name})),
            "audio sent to"
        );
        Ok(())
    }

    /// Send a voice message to a Telegram chat
    pub async fn send_voice(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        file_path: &Path,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("voice.ogg");

        let file_bytes = tokio::fs::read(file_path).await?;
        let part = Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part("voice", part);

        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendVoice"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendVoice failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "file_name": file_name})),
            "voice sent to"
        );
        Ok(())
    }

    /// Send a file by URL (Telegram will download it)
    pub async fn send_document_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "document": url
        });

        if let Some(tid) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(tid.to_string());
        }

        if let Some(cap) = caption {
            body["caption"] = serde_json::Value::String(cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendDocument"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendDocument by URL failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "url": url})),
            "document (URL) sent to"
        );
        Ok(())
    }

    /// Send a photo by URL (Telegram will download it)
    pub async fn send_photo_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "photo": url
        });

        if let Some(tid) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(tid.to_string());
        }

        if let Some(cap) = caption {
            body["caption"] = serde_json::Value::String(cap.to_string());
        }

        let resp = self
            .http_client()
            .post(self.api_url("sendPhoto"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Telegram sendPhoto by URL failed: {err}");
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"chat_id": chat_id, "url": url})),
            "photo (URL) sent to"
        );
        Ok(())
    }

    /// Send a video by URL (Telegram will download it)
    pub async fn send_video_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        self.send_media_by_url("sendVideo", "video", chat_id, thread_id, url, caption)
            .await
    }

    /// Send an audio file by URL (Telegram will download it)
    pub async fn send_audio_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        self.send_media_by_url("sendAudio", "audio", chat_id, thread_id, url, caption)
            .await
    }

    /// Send a voice message by URL (Telegram will download it)
    pub async fn send_voice_by_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        self.send_media_by_url("sendVoice", "voice", chat_id, thread_id, url, caption)
            .await
    }

    /// handle a tool-approval `callback_query`: resolve the pending approval,
    /// dismiss the spinner, and rewrite the prompt with the outcome, dropping
    /// the buttons
    /// best-effort rewrite, a stale tap is a no-op
    async fn handle_approval_callback(&self, cb: &serde_json::Value) {
        use crate::util::PendingApprovalResolution;

        let cb_id = cb
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let cb_data = cb
            .get("data")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        let Some(rest) = cb_data.strip_prefix("approval:") else {
            return;
        };
        let Some((approval_id, action)) = rest.rsplit_once(':') else {
            return;
        };

        let response = match action {
            "approve" => Some(clawcrew_api::channel::ChannelApprovalResponse::Approve),
            "always" => Some(clawcrew_api::channel::ChannelApprovalResponse::AlwaysApprove),
            "deny" => Some(clawcrew_api::channel::ChannelApprovalResponse::Deny),
            other => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"other": other})),
                    "Unknown approval callback action"
                );
                None
            }
        };

        let has_response = response.is_some();
        let (identities, callback_chat_id) = Self::approval_callback_context(cb);
        let responder_allowed = self.is_any_user_allowed(identities.iter().map(String::as_str));
        let (resolution, resolved_tool) = match (response, callback_chat_id.as_deref()) {
            (Some(response), Some(chat_id)) => {
                crate::util::resolve_pending_approval_with_tool(
                    &self.pending_approvals,
                    approval_id,
                    response,
                    responder_allowed,
                    chat_id,
                )
                .await
            }
            _ => (PendingApprovalResolution::NotFound, None),
        };

        if matches!(resolution, PendingApprovalResolution::Rejected) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"approval_id": approval_id})),
                "Telegram approval callback was not accepted"
            );
        } else if matches!(resolution, PendingApprovalResolution::ReceiverClosed) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "approval_id": clawcrew_runtime::security::scrub(approval_id)
                    })),
                "approval callback lost the resolution race; card left untouched"
            );
        }

        let answer_text = match (action, resolution) {
            ("approve", PendingApprovalResolution::Resolved) => format!(
                "✅ {}",
                i18n::get_required_cli_string("channel-telegram-approval-ack-approved")
            ),
            ("always", PendingApprovalResolution::Resolved) => format!(
                "✅✅ {}",
                i18n::get_required_cli_string("channel-telegram-approval-ack-always-approved")
            ),
            ("deny", PendingApprovalResolution::Resolved) => format!(
                "❌ {}",
                i18n::get_required_cli_string("channel-telegram-approval-ack-denied")
            ),
            ("approve" | "always" | "deny", PendingApprovalResolution::Rejected) => format!(
                "⚠️ {}",
                i18n::get_required_cli_string("channel-telegram-approval-ack-not-accepted")
            ),
            (
                "approve" | "always" | "deny",
                PendingApprovalResolution::NotFound | PendingApprovalResolution::ReceiverClosed,
            ) if has_response => format!(
                "⏳ {}",
                i18n::get_required_cli_string("channel-telegram-approval-ack-already-resolved")
            ),
            _ => format!(
                "⚠️ {}",
                i18n::get_required_cli_string("channel-telegram-approval-ack-unknown")
            ),
        };
        let answer_body = serde_json::json!({
            "callback_query_id": cb_id,
            "text": answer_text,
        });
        if let Err(e) = self
            .http_client()
            .post(self.api_url("answerCallbackQuery"))
            .json(&answer_body)
            .send()
            .await
        {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                "answerCallbackQuery failed"
            );
        }

        // rewrite the prompt in place, dropping the keyboard
        if let Some(tool_name) = resolved_tool {
            let chat_id = cb
                .get("message")
                .and_then(|m| m.get("chat"))
                .and_then(|c| c.get("id"))
                .and_then(serde_json::Value::as_i64);
            let message_id = cb
                .get("message")
                .and_then(|m| m.get("message_id"))
                .and_then(serde_json::Value::as_i64);
            let user_first_name = cb
                .get("from")
                .and_then(|f| f.get("first_name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Operator");

            if let (Some(chat_id), Some(message_id)) = (chat_id, message_id) {
                // an InlineKeyboardMarkup with zero rows is the Bot API's
                // documented way to drop the buttons; a bare {} is not a
                // valid markup object (inline_keyboard is a required field)
                let edit_body = serde_json::json!({
                    "chat_id": chat_id,
                    "message_id": message_id,
                    "text": format!("{} by {}: {}", answer_text, user_first_name, tool_name),
                    "reply_markup": { "inline_keyboard": [] },
                });
                match self
                    .http_client()
                    .post(self.api_url("editMessageText"))
                    .json(&edit_body)
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if let EditMessageResult::Failed(status) =
                            Self::classify_edit_message_response(resp).await
                        {
                            ::clawcrew_log::record!(
                                WARN,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                .with_attrs(::serde_json::json!({"status": status.to_string()})),
                                "editMessageText (approval resolution) failed"
                            );
                        }
                    }
                    Err(e) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                            "editMessageText (approval resolution) request failed"
                        );
                    }
                }
            }
        }
    }

    /// Fixed, bounded delay between retries of a transiently failing update.
    /// The attempt count is diagnostic only.
    const TRANSIENT_RETRY_DELAY_SECS: u64 = 2;

    async fn pause_for_transient_update(
        uid: Option<i64>,
        transient_retry: &mut Option<(i64, u32)>,
    ) -> UpdateOutcome {
        let attempts = if let Some(uid) = uid {
            let attempts = match *transient_retry {
                Some((tracked_uid, n)) if tracked_uid == uid => n.saturating_add(1),
                _ => 1,
            };
            *transient_retry = Some((uid, attempts));
            attempts
        } else {
            1
        };
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                .with_attrs(::serde_json::json!({
                    "update_id": uid,
                    "attempts": attempts,
                    "retry_delay_secs": Self::TRANSIENT_RETRY_DELAY_SECS,
                })),
            "Transient failure parsing update; leaving offset unadvanced so the next poll retries it"
        );
        tokio::time::sleep(std::time::Duration::from_secs(
            Self::TRANSIENT_RETRY_DELAY_SECS,
        ))
        .await;
        UpdateOutcome::StopBatch
    }

    /// Route a single update from a `getUpdates` batch through the shared
    /// delivered/permanent-skip/retry-transient disposition path.
    ///
    /// The listener acknowledgement queue owns offset advancement. This
    /// helper only classifies and dispatches one ordinary update so the same
    /// path serves both the startup probe and the main loop.
    async fn process_update(
        &self,
        update: &serde_json::Value,
        tx: &tokio::sync::mpsc::Sender<ChannelMessage>,
        transient_retry: &mut Option<(i64, u32)>,
    ) -> UpdateOutcome {
        let uid = update.get("update_id").and_then(serde_json::Value::as_i64);

        // ── Handle callback_query (inline keyboard taps) ──
        if let Some(cb) = update.get("callback_query") {
            let cb_data = cb
                .get("data")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();

            if cb_data.starts_with(TELEGRAM_MODEL_PICKER_PREFIX) {
                self.handle_model_picker_callback(cb, tx).await;
                // Terminal for inbound processing, same rationale as the
                // approval branch below: acknowledging the picker callback
                // must not hold up the offset.
                if uid.is_some() {
                    *transient_retry = None;
                }
                return UpdateOutcome::Advanced;
            }

            self.handle_approval_callback(cb).await;

            // A callback_query is terminal for inbound processing: there is
            // no message to deliver downstream, so nothing can be lost by
            // acknowledging it. The spinner dismissal above is best-effort:
            // it does perform HTTP I/O and
            // its failure is logged, but a failed spinner dismissal must not
            // hold up the offset, since retrying the update would re-run the
            // approval side effect that has already been applied.
            if uid.is_some() {
                *transient_retry = None;
            }
            return UpdateOutcome::Advanced;
        }

        // `parse_update_message` handles text messages and has no fallible
        // I/O, so its `None` always means "not applicable", fall through to
        // the voice parser next. The voice and attachment parsers can
        // additionally fail transiently on download/transcription I/O; a
        // transient failure must abort this update's processing entirely
        // (not fall through to the next parser) so the offset stays put and
        // the next poll retries it.
        let disposition = if let Some(m) = self.parse_update_message(update) {
            UpdateDisposition::Parsed(Box::new(m))
        } else {
            match self.try_parse_voice_message(update).await {
                UpdateDisposition::SkipPermanent => self.try_parse_attachment_message(update).await,
                other => other,
            }
        };

        let msg = match disposition {
            UpdateDisposition::Parsed(m) => m,
            UpdateDisposition::SkipPermanent => {
                Box::pin(self.handle_unauthorized_message(update)).await;
                if uid.is_some() {
                    *transient_retry = None;
                }
                return UpdateOutcome::Advanced;
            }
            UpdateDisposition::RetryTransient => {
                return Self::pause_for_transient_update(uid, transient_retry).await;
            }
        };

        if self.dispatch_incoming_message(tx, update, *msg).await {
            if uid.is_some() {
                *transient_retry = None;
            }
            UpdateOutcome::Advanced
        } else {
            UpdateOutcome::ReceiverClosed
        }
    }
}

impl ::clawcrew_api::attribution::Attributable for TelegramChannel {
    fn role(&self) -> ::clawcrew_api::attribution::Role {
        ::clawcrew_api::attribution::Role::Channel(
            ::clawcrew_api::attribution::ChannelKind::Telegram,
        )
    }
    fn alias(&self) -> &str {
        &self.alias
    }
}

impl TelegramChannel {
    async fn dispatch_incoming_message(
        &self,
        tx: &tokio::sync::mpsc::Sender<ChannelMessage>,
        update: &serde_json::Value,
        msg: ChannelMessage,
    ) -> bool {
        // Silent observation: a passive message must not tell the room the
        // bot saw it, so it gets neither an ack reaction nor a typing hint.
        if !msg.passive_context {
            if self.ack_reactions
                && let Some((reaction_chat_id, reaction_message_id)) =
                    Self::extract_update_message_target(update)
            {
                self.try_add_ack_reaction_nonblocking(reaction_chat_id, reaction_message_id);
            }

            // Send one typing indicator for the logical inbound message. A media
            // group reaches this helper only after all members are materialized.
            let typing_body = serde_json::json!({
                "chat_id": &msg.reply_target,
                "action": "typing"
            });
            let _ = self
                .http_client()
                .post(self.api_url("sendChatAction"))
                .json(&typing_body)
                .send()
                .await;
        }

        tx.send(msg).await.is_ok()
    }

    fn enqueue_update_batch(
        queue: &mut std::collections::VecDeque<QueuedTelegramUpdate>,
        pending_media_groups: &mut std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        updates: &[serde_json::Value],
        now: Instant,
        poll_generation: u64,
    ) {
        for update in updates {
            let update_id = Self::update_id(update);
            if update_id.is_some_and(|candidate| {
                queue
                    .iter()
                    .any(|queued| queued.update_id == Some(candidate))
            }) {
                continue;
            }

            Self::buffer_media_group_update(pending_media_groups, update, now, poll_generation);
            let payload = Self::extract_media_group_key(update)
                .filter(|_| Self::should_defer_media_group_update(pending_media_groups, update))
                .map(QueuedTelegramUpdatePayload::MediaGroup)
                .unwrap_or_else(|| QueuedTelegramUpdatePayload::Ordinary(update.clone()));
            queue.push_back(QueuedTelegramUpdate {
                update_id,
                payload,
                delivered: false,
            });
        }

        // A full page is a truncated view of the backlog: updates past its last
        // one exist but stay invisible until the acknowledgement offset moves
        // beyond the page. So a pending album whose position in the update
        // ordering is still behind an older unacknowledged update cannot be
        // shown complete by another poll: the same page comes back, its
        // duplicates never refresh the debounce, and settling now would dispatch
        // a partial album and turn the members past the page into a second turn.
        // Groups the offset is already free to move past are left eligible, so
        // the oldest work still settles and pagination keeps advancing. That
        // eligibility is what the rule can promise: an album settles only after
        // a page that began at or before its earliest member, so every member
        // within one page of that member has been seen. An album spread across
        // more than a full page of updates is out of reach, because nothing
        // older is left to release the offset and holding it would stall
        // polling instead of completing the album.
        let page_saturated = updates.len() >= TELEGRAM_POLL_LIMIT;
        for group in pending_media_groups.values_mut() {
            group.saturated_page_blocked = page_saturated
                && Self::pending_media_group_first_update_id(group).is_some_and(
                    |first_update_id| {
                        Self::has_unacknowledged_update_before(queue, first_update_id)
                    },
                );
        }
    }

    /// Telegram order of the earliest member held for this album, counting the
    /// text-only members that carry their own acknowledgement identity.
    fn pending_media_group_first_update_id(group: &PendingMediaGroup) -> Option<i64> {
        group
            .updates
            .iter()
            .filter_map(Self::update_id)
            .chain(group.unsupported.iter().map(|member| member.update_id))
            .min()
    }

    /// Whether an update older than `update_id` is still unacknowledged, which
    /// pins the delivered prefix (and therefore the next poll's offset) below
    /// it. Delivered entries behind an undelivered one stay queued, so only
    /// undelivered entries hold the offset back.
    fn has_unacknowledged_update_before(
        queue: &std::collections::VecDeque<QueuedTelegramUpdate>,
        update_id: i64,
    ) -> bool {
        queue.iter().any(|queued| {
            !queued.delivered
                && queued
                    .update_id
                    .is_some_and(|queued_id| queued_id < update_id)
        })
    }

    fn restore_media_group_batch(
        pending: &mut std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        batch: MediaGroupBatch,
    ) {
        pending.insert(
            batch.key,
            PendingMediaGroup {
                updates: batch.updates,
                unsupported: batch.unsupported,
                last_seen: batch.last_seen,
                last_seen_poll_generation: batch.last_seen_poll_generation,
                saturated_page_blocked: batch.saturated_page_blocked,
            },
        );
    }

    fn media_group_batch_first_update_id(batch: &MediaGroupBatch) -> Option<i64> {
        batch
            .updates
            .iter()
            .filter_map(Self::update_id)
            .chain(batch.unsupported.iter().map(|member| member.update_id))
            .min()
    }

    async fn dispatch_media_group_batch(
        &self,
        tx: &tokio::sync::mpsc::Sender<ChannelMessage>,
        batch: MediaGroupBatch,
    ) -> MediaGroupDispatchOutcome {
        let disposition = self
            .try_parse_media_group_with_unsupported(&batch.updates, &batch.unsupported)
            .await;
        match disposition {
            UpdateDisposition::Parsed(message) => {
                let Some(anchor_update) = batch.updates.first() else {
                    return MediaGroupDispatchOutcome::Delivered(batch.key);
                };
                if self
                    .dispatch_incoming_message(tx, anchor_update, *message)
                    .await
                {
                    MediaGroupDispatchOutcome::Delivered(batch.key)
                } else {
                    MediaGroupDispatchOutcome::ReceiverClosed
                }
            }
            UpdateDisposition::SkipPermanent => {
                if let Some(anchor_update) = batch.updates.first() {
                    Box::pin(self.handle_unauthorized_message(anchor_update)).await;
                }
                MediaGroupDispatchOutcome::Delivered(batch.key)
            }
            UpdateDisposition::RetryTransient => MediaGroupDispatchOutcome::Retry(batch),
        }
    }

    fn mark_media_group_delivered(
        queue: &mut std::collections::VecDeque<QueuedTelegramUpdate>,
        key: &MediaGroupKey,
    ) {
        for queued in queue {
            if matches!(
                &queued.payload,
                QueuedTelegramUpdatePayload::MediaGroup(candidate) if candidate == key
            ) {
                queued.delivered = true;
            }
        }
    }

    fn advance_delivered_prefix(
        queue: &mut std::collections::VecDeque<QueuedTelegramUpdate>,
        offset: &mut i64,
    ) {
        while queue.front().is_some_and(|queued| queued.delivered) {
            let Some(queued) = queue.pop_front() else {
                break;
            };
            if let Some(update_id) = queued.update_id {
                *offset = update_id + 1;
            }
        }
    }

    async fn dispatch_media_group_batches(
        &self,
        tx: &tokio::sync::mpsc::Sender<ChannelMessage>,
        pending_media_groups: &mut std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        queue: &mut std::collections::VecDeque<QueuedTelegramUpdate>,
        batches: Vec<MediaGroupBatch>,
        transient_retry: &mut Option<(i64, u32)>,
    ) -> UpdateOutcome {
        let mut batches = batches.into_iter();
        while let Some(batch) = batches.next() {
            match self.dispatch_media_group_batch(tx, batch).await {
                MediaGroupDispatchOutcome::Delivered(key) => {
                    Self::mark_media_group_delivered(queue, &key);
                    *transient_retry = None;
                }
                MediaGroupDispatchOutcome::Retry(batch) => {
                    let uid = Self::media_group_batch_first_update_id(&batch);
                    Self::restore_media_group_batch(pending_media_groups, batch);
                    for remaining in batches {
                        Self::restore_media_group_batch(pending_media_groups, remaining);
                    }
                    return Self::pause_for_transient_update(uid, transient_retry).await;
                }
                MediaGroupDispatchOutcome::ReceiverClosed => {
                    for remaining in batches {
                        Self::restore_media_group_batch(pending_media_groups, remaining);
                    }
                    return UpdateOutcome::ReceiverClosed;
                }
            }
        }
        UpdateOutcome::Advanced
    }

    async fn process_queued_updates(
        &self,
        tx: &tokio::sync::mpsc::Sender<ChannelMessage>,
        queue: &mut std::collections::VecDeque<QueuedTelegramUpdate>,
        pending_media_groups: &mut std::collections::HashMap<MediaGroupKey, PendingMediaGroup>,
        offset: &mut i64,
        transient_retry: &mut Option<(i64, u32)>,
        now: Instant,
        completed_poll_generation: u64,
    ) -> UpdateOutcome {
        let mut index = 0;
        while index < queue.len() {
            if queue[index].delivered {
                index += 1;
                continue;
            }

            let payload = queue[index].payload.clone();
            let outcome = match payload {
                QueuedTelegramUpdatePayload::Ordinary(update) => {
                    let prior_groups = Self::take_prior_media_groups_for_update(
                        pending_media_groups,
                        &update,
                        now,
                        completed_poll_generation,
                    );
                    let group_outcome = self
                        .dispatch_media_group_batches(
                            tx,
                            pending_media_groups,
                            queue,
                            prior_groups,
                            transient_retry,
                        )
                        .await;
                    if !matches!(group_outcome, UpdateOutcome::Advanced) {
                        group_outcome
                    } else {
                        let ordinary_outcome =
                            self.process_update(&update, tx, transient_retry).await;
                        if matches!(ordinary_outcome, UpdateOutcome::Advanced) {
                            queue[index].delivered = true;
                        }
                        ordinary_outcome
                    }
                }
                QueuedTelegramUpdatePayload::MediaGroup(key) => {
                    let is_settled = pending_media_groups.get(&key).is_some_and(|group| {
                        !group.saturated_page_blocked
                            && now.saturating_duration_since(group.last_seen)
                                >= TELEGRAM_MEDIA_GROUP_SETTLE_DELAY
                            && group.last_seen_poll_generation < completed_poll_generation
                    });
                    if !is_settled {
                        index += 1;
                        continue;
                    }
                    let batches =
                        Self::take_media_groups_matching(pending_media_groups, |candidate, _| {
                            candidate == &key
                        });
                    self.dispatch_media_group_batches(
                        tx,
                        pending_media_groups,
                        queue,
                        batches,
                        transient_retry,
                    )
                    .await
                }
            };

            match outcome {
                UpdateOutcome::Advanced => index += 1,
                UpdateOutcome::StopBatch => {
                    Self::advance_delivered_prefix(queue, offset);
                    return UpdateOutcome::StopBatch;
                }
                UpdateOutcome::ReceiverClosed => {
                    Self::advance_delivered_prefix(queue, offset);
                    return UpdateOutcome::ReceiverClosed;
                }
            }
        }

        let settled =
            Self::take_settled_media_groups(pending_media_groups, now, completed_poll_generation);
        let settled_outcome = self
            .dispatch_media_group_batches(tx, pending_media_groups, queue, settled, transient_retry)
            .await;
        if !matches!(settled_outcome, UpdateOutcome::Advanced) {
            Self::advance_delivered_prefix(queue, offset);
            return settled_outcome;
        }

        Self::advance_delivered_prefix(queue, offset);
        UpdateOutcome::Advanced
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    fn self_handle(&self) -> Option<String> {
        self.bot_username.lock().clone()
    }

    /// Telegram users mention the bot as `@bot_username` in chat. The
    /// cached `bot_username` from `getMe` is already the bare form;
    /// prepend `@` to match what arrives in inbound message text.
    fn self_addressed_mention(&self) -> Option<String> {
        self.self_handle().map(|name| {
            let trimmed = name.trim_start_matches('@');
            format!("@{trimmed}")
        })
    }

    fn supports_draft_updates(&self) -> bool {
        self.stream_mode != StreamMode::Off
    }

    async fn present_model_picker(
        &self,
        request: &ChannelModelPickerRequest,
    ) -> anyhow::Result<bool> {
        if request.channel_alias != self.alias || request.requesting_user_id.is_empty() {
            return Ok(false);
        }
        let Some(config) = &self.persist else {
            return Ok(false);
        };
        let runtime_routes = Arc::new(
            request
                .model_routes
                .iter()
                .map(|route| ModelPickerOption {
                    hint: route.hint.clone(),
                    model_provider: route.model_provider.clone(),
                    model: route.model.clone(),
                })
                .collect::<Vec<_>>(),
        );
        let context = {
            let live = config.read();
            let Some(mut context) =
                Self::model_picker_context(&live, &self.alias, runtime_routes.as_ref())
            else {
                return Ok(false);
            };
            if context.owner_agent_alias != request.owner_agent_alias {
                return Ok(false);
            }
            context.current = ModelPickerSelection {
                model_provider: request.current_model_provider.clone(),
                model: request.current_model.clone(),
            };
            context
        };

        let buttons = context
            .categories
            .iter()
            .map(|category| (uuid::Uuid::new_v4().to_string(), category))
            .collect::<Vec<_>>();
        let cancel_token = uuid::Uuid::new_v4().to_string();
        let Some(reply_markup) =
            Self::model_picker_category_reply_markup(&buttons, &cancel_token, &context.current)
        else {
            return Ok(false);
        };
        let (chat_id, thread_id) = Self::parse_reply_target(&request.reply_target);
        let chat_id_number = chat_id
            .parse::<i64>()
            .context("Telegram model picker reply target is not a numeric chat ID")?;
        let picker_text = i18n::get_required_cli_string_with_args(
            "channel-telegram-model-picker-provider-title",
            &[
                ("provider", context.current.model_provider.as_str()),
                ("model", context.current.model.as_str()),
            ],
        );
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": picker_text,
        });
        if let Some(thread_id) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(thread_id);
        }
        let response = self
            .http_client()
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Telegram sendMessage (model picker) failed: {}",
                response.status()
            );
        }
        let response_body: serde_json::Value = response.json().await?;
        if !Self::telegram_api_envelope_ok(&response_body) {
            anyhow::bail!("Telegram sendMessage (model picker) returned a non-ok envelope");
        }
        let picker_message_id = response_body
            .get("result")
            .and_then(|result| result.get("message_id"))
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| {
                anyhow::Error::msg("Telegram model picker response omitted message_id")
            })?;

        let created_at = Instant::now();
        let base = PendingModelPicker {
            created_at,
            expires_at: created_at + TELEGRAM_MODEL_PICKER_TTL,
            requesting_user_id: request.requesting_user_id.clone(),
            reply_target: request.reply_target.clone(),
            thread_ts: request.thread_ts.clone(),
            channel_alias: self.alias.clone(),
            picker_message_id,
            owner_agent_alias: context.owner_agent_alias,
            current: context.current,
            runtime_routes,
            action: ModelPickerAction::Cancel,
        };
        let anchor = base.clone();
        let mut pending = buttons
            .into_iter()
            .map(|(token, category)| {
                (
                    token,
                    PendingModelPicker {
                        action: ModelPickerAction::OpenCategory {
                            provider_ref: category.provider_ref.clone(),
                            page: 0,
                        },
                        ..base.clone()
                    },
                )
            })
            .collect::<Vec<_>>();
        pending.push((cancel_token, base));
        self.insert_pending_model_picker_batch(pending).await;
        if !self
            .edit_model_picker_message_at(
                chat_id_number,
                picker_message_id,
                picker_text,
                reply_markup,
            )
            .await
        {
            self.disable_model_picker_keyboard_at(chat_id_number, picker_message_id)
                .await;
            self.remove_pending_model_picker_keyboard(&anchor).await;
            anyhow::bail!("Telegram model picker keyboard update failed");
        }
        Ok(true)
    }

    fn supports_multi_message_streaming(&self) -> bool {
        self.stream_mode == StreamMode::MultiMessage
    }

    fn supports_turn_flush_narration(&self) -> bool {
        // Telegram is the only channel that implements `flush_draft_turn` /
        // `discard_draft_turn`; scope the orchestrator's narration-policy +
        // flush-barrier path to it so channels that stream paragraphs another
        // way (e.g. Matrix `update_draft`) do not run outbound hooks on phantom
        // flushes that deliver nothing.
        self.stream_mode == StreamMode::MultiMessage
    }

    fn multi_message_delay_ms(&self) -> u64 {
        self.resolve_multi_message_delay_ms()
    }

    async fn send_draft(&self, message: &SendMessage) -> anyhow::Result<Option<String>> {
        match self.stream_mode {
            StreamMode::Off => Ok(None),
            StreamMode::Partial => {
                let (chat_id, thread_id) = Self::parse_reply_target(&message.recipient);
                let initial_text = if message.content.is_empty() {
                    "...".to_string()
                } else {
                    message.content.clone()
                };

                let mut body = serde_json::json!({
                    "chat_id": chat_id,
                    "text": initial_text,
                });
                if let Some(tid) = thread_id {
                    body["message_thread_id"] = serde_json::Value::String(tid.to_string());
                }

                let resp = self
                    .http_client()
                    .post(self.api_url("sendMessage"))
                    .json(&body)
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    let err = resp.text().await.unwrap_or_default();
                    anyhow::bail!("Telegram sendMessage (draft) failed: {err}");
                }

                let resp_json: serde_json::Value = resp.json().await?;
                let message_id = resp_json
                    .get("result")
                    .and_then(|r| r.get("message_id"))
                    .and_then(|id| id.as_i64())
                    .map(|id| id.to_string());

                self.last_draft_edit
                    .lock()
                    .insert(chat_id.to_string(), std::time::Instant::now());

                Ok(message_id)
            }
            StreamMode::MultiMessage => {
                let draft_id = Self::new_multi_message_draft_id();
                let (_, thread_id) = Self::parse_reply_target(&message.recipient);
                self.multi_message_drafts.lock().insert(
                    Self::multi_draft_key(&message.recipient, &draft_id),
                    MultiDraftState::new(thread_id),
                );
                Ok(Some(draft_id))
            }
        }
    }

    async fn update_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        match self.stream_mode {
            StreamMode::Off => Ok(()),
            StreamMode::Partial => {
                let (chat_id, _) = Self::parse_reply_target(recipient);

                // Rate-limit edits per chat
                {
                    let last_edits = self.last_draft_edit.lock();
                    if let Some(last_time) = last_edits.get(&chat_id) {
                        let elapsed =
                            u64::try_from(last_time.elapsed().as_millis()).unwrap_or(u64::MAX);
                        if elapsed < self.draft_update_interval_ms {
                            return Ok(());
                        }
                    }
                }

                // Truncate to Telegram limit for mid-stream edits (UTF-8 safe)
                let display_text = if text.len() > TELEGRAM_MAX_MESSAGE_LENGTH {
                    let mut end = 0;
                    for (idx, ch) in text.char_indices() {
                        let next = idx + ch.len_utf8();
                        if next > TELEGRAM_MAX_MESSAGE_LENGTH {
                            break;
                        }
                        end = next;
                    }
                    &text[..end]
                } else {
                    text
                };

                let message_id_parsed = match message_id.parse::<i64>() {
                    Ok(id) => id,
                    Err(e) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                .with_attrs(
                                    ::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e)), "message_id": message_id})
                                ),
                            "Invalid Telegram message_id ''"
                        );
                        return Ok(());
                    }
                };

                let body = serde_json::json!({
                    "chat_id": chat_id,
                    "message_id": message_id_parsed,
                    "text": display_text,
                });

                let resp = self
                    .http_client()
                    .post(self.api_url("editMessageText"))
                    .json(&body)
                    .send()
                    .await?;

                if resp.status().is_success() {
                    self.last_draft_edit
                        .lock()
                        .insert(chat_id.clone(), std::time::Instant::now());
                } else {
                    let status = resp.status();
                    let err = resp.text().await.unwrap_or_default();
                    ::clawcrew_log::record!(DEBUG, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"error": format!("{}", err), "status": status.to_string()})), "editMessageText failed");
                }

                Ok(())
            }
            StreamMode::MultiMessage => {
                // Multi-message drafts are never edited in place: the deliverable
                // state (`latest_visible`) is owned solely by `flush_draft_turn`,
                // which sets it from the policy-checked narration immediately before
                // it flushes. Tracking the raw, un-policy-checked accumulation here
                // would let `finalize`'s pending-resume resurrect narration the
                // outbound hook cancelled — content never approved for delivery.
                Ok(())
            }
        }
    }

    async fn flush_draft_turn(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        if self.stream_mode != StreamMode::MultiMessage
            || !Self::is_multi_message_synthetic_draft(message_id)
        {
            return Ok(());
        }
        // Refresh the canonical buffer from the turn text, then flush from it.
        // Both flush paths must account against the same sanitized string —
        // never against caller-supplied text with different normalization.
        let visible = sanitize_multi_message_visible_text(text);
        {
            let key = Self::multi_draft_key(recipient, message_id);
            let mut drafts = self.multi_message_drafts.lock();
            if let Some(draft) = drafts.get_mut(&key) {
                draft.latest_visible = visible;
            }
        }
        self.flush_unsent(recipient, message_id).await
    }

    async fn discard_draft_turn(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        if self.stream_mode != StreamMode::MultiMessage
            || !Self::is_multi_message_synthetic_draft(message_id)
        {
            return Ok(());
        }
        // A hook cancelled this narration turn. The orchestrator passes the
        // *owned* (accepted-turns) snapshot here — the cancelled turn was never
        // added to it — so resync the pending buffer to that snapshot and the
        // cancelled turn's streamed narration is excluded from what a later
        // flush sends. Its suffix accounting then stays aligned with what policy
        // approved, and the cancelled narration is never resurrected.
        //
        // Deliberately do NOT touch the delivery bookkeeping
        // (`sent_text` / `delivered_chunks` / `delivered_prefix`): every byte of
        // the snapshot is *accepted* narration that must still be delivered, and
        // an earlier accepted turn may have only partially delivered. Overwriting
        // these with the full snapshot would mark that turn's unsent remainder as
        // consumed and silently drop it at finalize. Cancelling a later turn must
        // not mutate delivery ownership of an earlier one.
        let visible = sanitize_multi_message_visible_text(text);
        let key = Self::multi_draft_key(recipient, message_id);
        let mut drafts = self.multi_message_drafts.lock();
        if let Some(draft) = drafts.get_mut(&key) {
            draft.latest_visible = visible;
        }
        Ok(())
    }

    // No `update_draft_progress` override: raw legacy tool-status text (tool
    // name, arguments, paths, credential-shaped values) must never reach the Bot
    // API, which cannot retract a sent message. The trait default no-op drops it;
    // typed, policy-checked progress renders through `update_draft_lifecycle`
    // below. Enforced by `raw_tool_status_never_reaches_telegram`.
    async fn update_draft_lifecycle(
        &self,
        recipient: &str,
        message_id: &str,
        event: ProgressEvent,
    ) -> anyhow::Result<()> {
        if self.stream_mode == StreamMode::Partial {
            let status_line = crate::util::localized_lifecycle_progress(event);
            return self.update_draft(recipient, message_id, &status_line).await;
        }
        Ok(())
    }

    async fn finalize_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
        suppress_voice: bool,
    ) -> anyhow::Result<()> {
        if self.stream_mode == StreamMode::MultiMessage {
            return self
                .finalize_multi_message_draft(recipient, message_id, text, suppress_voice)
                .await;
        }

        let text = &strip_tool_call_tags(text);
        let (chat_id, thread_id) = Self::parse_reply_target(recipient);

        // Queue TTS voice reply — immediate mode since text is already final.
        // Skipped when suppress_voice is set (explicit text-only routing override).
        if !suppress_voice {
            self.try_queue_voice_reply(recipient, text, true, false);
        }

        // Clean up rate-limit tracking for this chat
        self.last_draft_edit.lock().remove(&chat_id);

        // Voice-only peers: delete the draft placeholder and let the voice
        // bubble be the sole reply. Bypassed when suppress_voice forces text.
        if !suppress_voice && self.destination_is_voice_peer(recipient) {
            if let Ok(id) = message_id.parse::<i64>() {
                let _ = self
                    .http_client()
                    .post(self.api_url("deleteMessage"))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": id,
                    }))
                    .send()
                    .await;
            }
            return Ok(());
        }

        // Parse attachments before processing
        let (text_without_markers, attachments) = parse_attachment_markers(text);

        // Parse message ID once for reuse
        let msg_id = match message_id.parse::<i64>() {
            Ok(id) => Some(id),
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e)), "message_id": message_id})
                        ),
                    "Invalid Telegram message_id ''"
                );
                None
            }
        };

        // If we have attachments, delete the draft and send fresh messages
        // (Telegram editMessageText can't add attachments)
        if !attachments.is_empty() {
            // Delete the draft message
            if let Some(id) = msg_id {
                let _ = self
                    .http_client()
                    .post(self.api_url("deleteMessage"))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": id,
                    }))
                    .send()
                    .await;
            }

            // Send text without markers
            if !text_without_markers.is_empty() {
                self.finalize_send_chunks(&text_without_markers, &chat_id, thread_id.as_deref())
                    .await?;
            }

            // Send attachments
            for attachment in &attachments {
                self.send_attachment(&chat_id, thread_id.as_deref(), attachment)
                    .await?;
            }

            return Ok(());
        }

        // If text exceeds limit, delete draft and send as chunked messages
        if text.len() > TELEGRAM_MAX_MESSAGE_LENGTH {
            if let Some(id) = msg_id {
                let _ = self
                    .http_client()
                    .post(self.api_url("deleteMessage"))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": id,
                    }))
                    .send()
                    .await;
            }

            // Fall back to chunked send
            return self
                .finalize_send_chunks(text, &chat_id, thread_id.as_deref())
                .await;
        }

        let Some(id) = msg_id else {
            return self
                .finalize_send_chunks(text, &chat_id, thread_id.as_deref())
                .await;
        };

        // Try editing with HTML formatting
        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": id,
            "text": Self::markdown_to_telegram_html(text),
            "parse_mode": "HTML",
        });

        let resp = self
            .http_client()
            .post(self.api_url("editMessageText"))
            .json(&body)
            .send()
            .await?;

        match Self::classify_edit_message_response(resp).await {
            EditMessageResult::Success | EditMessageResult::NotModified => return Ok(()),
            EditMessageResult::Failed(status) => {
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({"status": status.to_string()})),
                    "Telegram finalize_draft HTML edit failed; retrying without parse_mode"
                );
            }
        }

        // HTML failed — retry without parse_mode
        let plain_body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": id,
            "text": text,
        });

        let resp = self
            .http_client()
            .post(self.api_url("editMessageText"))
            .json(&plain_body)
            .send()
            .await?;

        match Self::classify_edit_message_response(resp).await {
            EditMessageResult::Success | EditMessageResult::NotModified => return Ok(()),
            EditMessageResult::Failed(status) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"status": status.to_string()})),
                    "Telegram finalize_draft plain edit failed; attempting delete+send fallback"
                );
            }
        }

        let delete_resp = self
            .http_client()
            .post(self.api_url("deleteMessage"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "message_id": id,
            }))
            .send()
            .await;

        match delete_resp {
            Ok(resp) if resp.status().is_success() => {
                self.finalize_send_chunks(text, &chat_id, thread_id.as_deref())
                    .await
            }
            Ok(resp) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"status": resp.status().to_string()})),
                    "Telegram finalize_draft delete failed; skipping sendMessage to avoid duplicate"
                );
                Ok(())
            }
            Err(err) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"err": err.to_string()})),
                    "Telegram finalize_draft delete request failed: ; skipping sendMessage to avoid duplicate"
                );
                Ok(())
            }
        }
    }

    async fn cancel_draft(&self, recipient: &str, message_id: &str) -> anyhow::Result<()> {
        let (chat_id, _) = Self::parse_reply_target(recipient);
        self.last_draft_edit.lock().remove(&chat_id);

        if Self::is_multi_message_synthetic_draft(message_id) {
            self.multi_message_drafts
                .lock()
                .remove(&Self::multi_draft_key(recipient, message_id));
            return Ok(());
        }

        let message_id = match message_id.parse::<i64>() {
            Ok(id) => id,
            Err(e) => {
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(
                            ::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e)), "message_id": message_id})
                        ),
                    "Invalid Telegram draft message_id ''"
                );
                return Ok(());
            }
        };

        let response = self
            .http_client()
            .post(self.api_url("deleteMessage"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "message_id": message_id,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({"status": status.to_string(), "body": body})),
                "deleteMessage failed"
            );
        }

        Ok(())
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        // Strip tool_call tags before processing to prevent Markdown parsing failures
        let content = strip_tool_call_tags(&message.content);

        // Parse recipient: "chat_id" or "chat_id:thread_id" format
        let (chat_id, thread_id) = match message.recipient.split_once(':') {
            Some((chat, thread)) => (chat, Some(thread)),
            None => (message.recipient.as_str(), None),
        };

        // Voice chat mode: queue a voice note. Suppressed messages (errors,
        // system notices) are never voiced.
        if !message.suppress_voice {
            self.try_queue_voice_reply(&message.recipient, &content, false, message.force_voice);
        }

        // Voice-only peers (or explicit force_voice): the voice note is the sole
        // FINAL reply — skip the final text. In multi_message mode, narration
        // already streamed earlier this turn is delivered as separate, permanent
        // messages that cannot be retracted; a per-turn voice route governs the
        // final answer only and does not convert or delete that narration.
        if !message.suppress_voice
            && (self.destination_is_voice_peer(&message.recipient) || message.force_voice)
        {
            return Ok(());
        }

        let (text_without_markers, attachments) = parse_attachment_markers(&content);

        if !attachments.is_empty() {
            if !text_without_markers.is_empty() {
                self.send_text_chunks(&text_without_markers, chat_id, thread_id, 0)
                    .await
                    .map_err(|e| e.source)?;
            }

            for attachment in &attachments {
                self.send_attachment(chat_id, thread_id, attachment).await?;
            }

            return Ok(());
        }

        if let Some(attachment) = parse_path_only_attachment(&content) {
            self.send_attachment(chat_id, thread_id, &attachment)
                .await?;
            return Ok(());
        }

        self.send_text_chunks(&content, chat_id, thread_id, 0)
            .await
            .map(|_| ())
            .map_err(|e| e.source)
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut offset: i64 = 0;
        let mut poll_generation: u64 = 0;
        let mut pending_media_groups: std::collections::HashMap<MediaGroupKey, PendingMediaGroup> =
            std::collections::HashMap::new();
        let mut queued_updates = std::collections::VecDeque::new();
        // Single-slot transient-retry tracker: (update_id, attempts so far).
        // One slot is sufficient because a transient failure via
        // `process_update` stops processing of the current update batch (be
        // it the startup probe's batch below or the main loop's), so at most
        // one update can be head-of-line blocking retries at any time. Once
        // the attempt count grows only for operator diagnostics. It never
        // changes the delivery disposition: an unclassified I/O failure
        // cannot become safe to acknowledge merely because it repeated.
        // Shared across both the startup probe and the main loop so a
        // transient failure on a queued startup update is tracked the same
        // way as one seen mid-run.
        let mut transient_retry: Option<(i64, u32)> = None;

        if self.mention_only {
            let _ = self.get_bot_username().await;
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            "channel listening for messages..."
        );

        loop {
            let url = self.api_url("getUpdates");
            let probe = serde_json::json!({
                "offset": offset,
                "limit": TELEGRAM_POLL_LIMIT,
                "timeout": 0,
                "allowed_updates": ["message", "callback_query"]
            });
            match self.http_client().post(&url).json(&probe).send().await {
                Err(e) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(
                                ::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})
                            ),
                        "startup probe error; retrying in 5s"
                    );
                    self.record_poll_health(false);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Ok(resp) => {
                    match resp.json::<serde_json::Value>().await {
                        Err(e) => {
                            ::clawcrew_log::record!(
                                WARN,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                .with_attrs(::serde_json::json!({"e": e.to_string()})),
                                "startup probe parse error: ; retrying in 5s"
                            );
                            self.record_poll_health(false);
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                        Ok(data) => {
                            let ok = data
                                .get("ok")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            if ok {
                                // Slot claimed. Route any queued updates through the
                                // same delivered/permanent-skip/retry-transient
                                // disposition path as the main loop below, instead of
                                // blindly advancing the offset past them: a transient
                                // failure or a dropped receiver here must leave the
                                // offset unadvanced too, so the update survives until
                                // a later poll (in this probe or the main loop) can
                                // actually deliver it.
                                if let Some(results) =
                                    data.get("result").and_then(serde_json::Value::as_array)
                                {
                                    poll_generation = poll_generation.saturating_add(1);
                                    let probe_completed_at = Instant::now();
                                    Self::enqueue_update_batch(
                                        &mut queued_updates,
                                        &mut pending_media_groups,
                                        results,
                                        probe_completed_at,
                                        poll_generation,
                                    );
                                    if matches!(
                                        self.process_queued_updates(
                                            &tx,
                                            &mut queued_updates,
                                            &mut pending_media_groups,
                                            &mut offset,
                                            &mut transient_retry,
                                            probe_completed_at,
                                            poll_generation,
                                        )
                                        .await,
                                        UpdateOutcome::ReceiverClosed
                                    ) {
                                        return Ok(());
                                    }
                                }
                                self.record_poll_health(true);
                                break; // Probe succeeded; enter the long-poll loop.
                            }

                            self.record_poll_health(false);
                            let error_code = data
                                .get("error_code")
                                .and_then(serde_json::Value::as_i64)
                                .unwrap_or_default();
                            if error_code == 409 {
                                ::clawcrew_log::record!(
                                    DEBUG,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    ),
                                    "Startup probe: slot busy (409), retrying in 5s"
                                );
                            } else {
                                let desc = data
                                    .get("description")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("unknown");
                                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"error_code": error_code, "desc": desc})), "Startup probe: API error : ; retrying in 5s");
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        }

        ::clawcrew_log::record!(
            DEBUG,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            "Startup probe succeeded; entering main long-poll loop."
        );

        self.register_bot_commands().await;

        loop {
            if self.mention_only {
                let missing_username = self.bot_username.lock().is_none();
                if missing_username {
                    let _ = self.get_bot_username().await;
                }
            }

            let url = self.api_url("getUpdates");
            let poll_timeout_secs = Self::media_group_poll_timeout_secs(&pending_media_groups);
            let body = serde_json::json!({
                "offset": offset,
                "limit": TELEGRAM_POLL_LIMIT,
                "timeout": poll_timeout_secs,
                "allowed_updates": ["message", "callback_query"]
            });

            let resp = match self.http_client().post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(
                                ::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})
                        ),
                        "poll error"
                    );
                    self.record_poll_health(false);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(
                                ::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})
                        ),
                        "parse error"
                    );
                    self.record_poll_health(false);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let ok = data
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            self.record_poll_health(ok);
            if !ok {
                let error_code = data
                    .get("error_code")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or_default();
                let description = data
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown Telegram API error");

                if error_code == 409 {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"description": description})),
                        "Telegram polling conflict (409): . \
Ensure only one `clawcrew` process is using this bot token."
                    );
                    // Back off for 35 seconds — longer than Telegram's 30-second poll
                    // timeout — so any competing session (e.g. a stale connection from
                    // a previous daemon) has time to expire before we retry.
                    tokio::time::sleep(std::time::Duration::from_secs(35)).await;
                } else {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                        &format!(
                            "Telegram getUpdates API error (code={}): {description}",
                            error_code
                        )
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                continue;
            }

            poll_generation = poll_generation.saturating_add(1);
            // Debounce against the time this response was observed, not the
            // time its updates finish processing. Slow downloads or channel
            // backpressure must not make an album look quiet prematurely.
            let poll_completed_at = Instant::now();

            if let Some(results) = data.get("result").and_then(serde_json::Value::as_array) {
                Self::enqueue_update_batch(
                    &mut queued_updates,
                    &mut pending_media_groups,
                    results,
                    poll_completed_at,
                    poll_generation,
                );
            }

            match self
                .process_queued_updates(
                    &tx,
                    &mut queued_updates,
                    &mut pending_media_groups,
                    &mut offset,
                    &mut transient_retry,
                    poll_completed_at,
                    poll_generation,
                )
                .await
            {
                UpdateOutcome::Advanced | UpdateOutcome::StopBatch => {}
                UpdateOutcome::ReceiverClosed => return Ok(()),
            }
        }
    }

    fn listener_health(&self) -> Option<ListenerHealth> {
        Some(match *self.poll_health.lock() {
            None => ListenerHealth::Pending,
            Some((false, _)) => ListenerHealth::Unhealthy,
            Some((true, at)) if at.elapsed() < POLL_HEALTH_STALE_AFTER => ListenerHealth::Healthy,
            // The last exchange succeeded, but nothing has completed since.
            // A blackholed request keeps `listen()` alive with no timeout to
            // end it, so the stale success must stop counting as evidence.
            Some((true, _)) => ListenerHealth::Unhealthy,
        })
    }

    async fn health_check(&self) -> bool {
        let timeout_duration = Duration::from_secs(5);

        match tokio::time::timeout(
            timeout_duration,
            self.http_client().get(self.api_url("getMe")).send(),
        )
        .await
        {
            Ok(Ok(resp)) => resp.status().is_success(),
            Ok(Err(e)) => {
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({"error": clawcrew_runtime::security::scrub(&format!("{}", e))})),
                    "health check failed"
                );
                false
            }
            Err(_) => {
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                    "health check timed out after 5s"
                );
                false
            }
        }
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        self.stop_typing(recipient).await?;

        let client = self.http_client();
        let url = self.api_url("sendChatAction");
        let chat_id = recipient.to_string();

        let handle = clawcrew_spawn::spawn!(async move {
            loop {
                let body = serde_json::json!({
                    "chat_id": &chat_id,
                    "action": "typing"
                });
                let _ = client.post(&url).json(&body).send().await;
                // Telegram typing indicator expires after 5s; refresh at 4s
                tokio::time::sleep(Duration::from_secs(4)).await;
            }
        });

        let mut guard = self.typing_handle.lock();
        *guard = Some(handle);

        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        let mut guard = self.typing_handle.lock();
        if let Some(handle) = guard.take() {
            handle.abort();
        }
        Ok(())
    }

    /// Delegates to [`Self::request_approval_attributed`] and drops the
    /// provenance, so the prompt/timeout logic lives in exactly one place.
    async fn request_approval(
        &self,
        recipient: &str,
        request: &clawcrew_api::channel::ChannelApprovalRequest,
    ) -> anyhow::Result<Option<clawcrew_api::channel::ChannelApprovalResponse>> {
        Ok(self
            .request_approval_attributed(recipient, request)
            .await?
            .map(|attributed| attributed.response))
    }

    async fn request_approval_attributed(
        &self,
        recipient: &str,
        request: &clawcrew_api::channel::ChannelApprovalRequest,
    ) -> anyhow::Result<Option<clawcrew_api::channel::AttributedApprovalResponse>> {
        use clawcrew_api::channel::ChannelApprovalResponse;

        // The runtime emits StreamDelta::FlushBarrier before this approval prompt;
        // its channel handler flushes ONLY the owning draft (by draft_id) and the
        // agent loop waits on the ack, so pre-tool narration for this turn is
        // already delivered. A recipient-wide flush here would also publish other
        // concurrent turns' incomplete drafts, so it is intentionally omitted.

        // Pace the approval prompt after the pre-tool narration: reintroduce
        // the multi_message inter-message gap between the last narration
        // message and the inline keyboard, so the prompt doesn't arrive glued
        // to it. No-op when nothing was just sent (last send is old/absent →
        // elapsed already exceeds the delay).
        if self.stream_mode == StreamMode::MultiMessage {
            let last_sent_at = self.latest_multi_message_send_at(recipient);
            self.pace_multi_message_send(last_sent_at).await;
        }

        // Parse recipient for chat_id + optional thread_id ("chat_id:thread_id" format).
        let (chat_id, thread_id) = recipient
            .split_once(':')
            .map_or((recipient, None), |(c, t)| (c, Some(t)));

        // Unique key embedded in callback_data so listen() can route the tap.
        let approval_id = uuid::Uuid::new_v4().to_string();

        let heading = i18n::get_required_cli_string("channel-approval-heading");
        let tool_label = i18n::get_required_cli_string("channel-approval-tool-label");
        let tap_instruction = i18n::get_required_cli_string("channel-approval-tap-instruction");
        let btn_approve = i18n::get_required_cli_string("channel-approval-btn-approve");
        let btn_deny = i18n::get_required_cli_string("channel-approval-btn-deny");
        let btn_always = i18n::get_required_cli_string("channel-approval-btn-always");

        let tool = Self::escape_html(&request.tool_name);
        let args = Self::escape_html(&request.arguments_summary);
        // Back-to-back cards from one message are otherwise indistinguishable
        // before the operator taps, so say which call this is.
        let position = Self::escape_html(&crate::util::approval_position_line(
            request.position_counter(),
        ));
        let text = format!(
            "\u{1f527} <b>{heading}</b>\n\n\
             {position}\
             {tool_label}: <code>{tool}</code>\n\
             {args}\n\n\
             {tap_instruction}",
        );

        let reply_markup = serde_json::json!({
            "inline_keyboard": [[
                { "text": format!("✅ {btn_approve}"),  "callback_data": format!("approval:{}:approve", approval_id) },
                { "text": format!("❌ {btn_deny}"),     "callback_data": format!("approval:{}:deny", approval_id) },
                { "text": format!("✅✅ {btn_always}"), "callback_data": format!("approval:{}:always", approval_id) },
            ]]
        });

        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML",
            "reply_markup": reply_markup,
        });
        if let Some(tid) = thread_id {
            body["message_thread_id"] = serde_json::Value::String(tid.to_string());
        }

        // Register the oneshot BEFORE sending the message to avoid a race
        // where the user taps the button before the sender is in the map.
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        self.pending_approvals.lock().await.insert(
            approval_id.clone(),
            crate::util::PendingApproval {
                sender: tx,
                destination: chat_id.to_string(),
                tool_name: request.tool_name.clone(),
            },
        );

        let resp = self
            .http_client()
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await;

        let send_ok = match resp {
            Ok(r) if r.status().is_success() => true,
            Ok(r) => {
                let status = r.status();
                let err = r.text().await.unwrap_or_default();
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"status": status.to_string(), "err": err})
                        ),
                    "Telegram sendMessage (approval) with HTML failed; retrying without parse_mode"
                );

                // Fallback: plain text, no parse_mode, keep the buttons.
                // Unescaped position line: this send has no parse_mode, so the
                // HTML-escaped one above would show its entities literally.
                let plain_position =
                    crate::util::approval_position_line(request.position_counter());
                let plain_text = format!(
                    "🔧 {heading}\n\n{plain_position}{tool_label}: {}\n{}\n\n{tap_instruction}",
                    request.tool_name, request.arguments_summary
                );
                let mut plain_body = serde_json::json!({
                    "chat_id": chat_id,
                    "text": plain_text,
                    "reply_markup": reply_markup,
                });
                if let Some(tid) = thread_id {
                    plain_body["message_thread_id"] = serde_json::Value::String(tid.to_string());
                }

                let plain_resp = self
                    .http_client()
                    .post(self.api_url("sendMessage"))
                    .json(&plain_body)
                    .send()
                    .await;

                match plain_resp {
                    Ok(r) if r.status().is_success() => true,
                    Ok(r) => {
                        let status = r.status();
                        let err = r.text().await.unwrap_or_default();
                        self.pending_approvals.lock().await.remove(&approval_id);
                        anyhow::bail!("Telegram sendMessage (approval) failed ({status}): {err}");
                    }
                    Err(e) => {
                        self.pending_approvals.lock().await.remove(&approval_id);
                        return Err(e.into());
                    }
                }
            }
            Err(e) => {
                self.pending_approvals.lock().await.remove(&approval_id);
                return Err(e.into());
            }
        };

        if !send_ok {
            self.pending_approvals.lock().await.remove(&approval_id);
            anyhow::bail!("Telegram sendMessage (approval) failed after fallback");
        }

        // Wait for the user to tap a button. Timeout is configurable via
        // `channels.telegram.approval_timeout_secs` (default 120s). The
        // pending entry is the single resolution claim: exactly one side
        // removes it, and that side decides. When the deadline fires the
        // waiter claims the entry and denies; if a callback already claimed
        // it, the operator's response is in flight on the channel and is
        // consumed instead of overridden, so the published card can never
        // disagree with the runtime's recorded outcome.
        let result =
            match tokio::time::timeout(Duration::from_secs(self.approval_timeout_secs), &mut rx)
                .await
            {
                Ok(Ok(response)) => Some(
                    clawcrew_api::channel::AttributedApprovalResponse::operator(response),
                ),
                Ok(Err(_)) => {
                    // Sender dropped — clean up and deny. Nobody tapped.
                    self.pending_approvals.lock().await.remove(&approval_id);
                    Some(
                        clawcrew_api::channel::AttributedApprovalResponse::from_runtime(
                            ChannelApprovalResponse::Deny,
                            clawcrew_api::channel::ApprovalSource::Unreachable,
                        ),
                    )
                }
                Err(_) => Some(self.resolve_after_deadline(&approval_id, &mut rx).await),
            };

        Ok(result)
    }
}

#[cfg(test)]
impl UpdateDisposition {
    /// Unwrap a `Parsed` disposition in tests, panicking with `context` on the
    /// skip/retry variants. Keeps parser regressions terse now that the parser
    /// returns a disposition rather than an `Option`. Defined alongside the
    /// test module rather than beside the enum so the first `#[cfg(test)]` in
    /// this file stays after the production code, keeping the config-isolation
    /// architecture gate's test-region scan off `persist_allowed_identity`.
    pub(crate) fn expect_parsed(self, context: &str) -> ChannelMessage {
        match self {
            UpdateDisposition::Parsed(msg) => *msg,
            UpdateDisposition::SkipPermanent => panic!("{context}: got SkipPermanent"),
            UpdateDisposition::RetryTransient => panic!("{context}: got RetryTransient"),
        }
    }
}

#[cfg(test)]
mod tests;

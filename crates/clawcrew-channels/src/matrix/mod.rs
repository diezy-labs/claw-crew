//! Matrix channel using matrix-rust-sdk 0.16.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use tokio::sync::{Mutex as TokioMutex, RwLock as TokioRwLock, mpsc, oneshot};

use matrix_sdk::{
    Client, RoomMemberships,
    ruma::{
        OwnedEventId, OwnedRoomId, OwnedUserId,
        api::client::{
            membership::invite_user::v3::{
                InvitationRecipient, InviteUserId, Request as InviteUserRequest,
            },
            room::{Visibility as MatrixVisibility, create_room::v3::Request as CreateRoomRequest},
        },
        events::{InitialStateEvent, room::encryption::RoomEncryptionEventContent},
    },
};

use clawcrew_api::channel::{
    Channel, ChannelApprovalRequest, ChannelApprovalResponse, ChannelMessage, DraftProgress,
    DraftProgressKind, RoomCreationOptions, RoomVisibility, SendMessage,
};
use clawcrew_config::schema::{MatrixConfig, MatrixStreamMode};
use clawcrew_runtime::agent::loop_::DRAFT_PLACEHOLDER;

// ─── markers ───────────────────────────────────────────────────────────────
mod markers {
    //! Parse `[image:url]`, `[audio:url]`, `[video:url]`, `[file:url]`, `[voice:url]`
    //! markers from outbound text. Strips them from the body and returns the kinds
    //! + targets so the caller can upload the corresponding media.

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum MarkerKind {
        Image,
        Audio,
        Video,
        File,
        Voice,
    }

    impl MarkerKind {
        fn from_keyword(kw: &str) -> Option<Self> {
            match kw.to_ascii_lowercase().as_str() {
                "image" | "img" | "photo" => Some(Self::Image),
                "audio" => Some(Self::Audio),
                "video" => Some(Self::Video),
                "file" | "document" | "doc" => Some(Self::File),
                "voice" => Some(Self::Voice),
                _ => None,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct Marker {
        pub kind: MarkerKind,
        pub target: String,
    }

    /// Scan `text` for marker substrings. Returns the cleaned text and any markers.
    /// Malformed/unknown markers are left in the text untouched.
    pub(super) fn parse(text: &str) -> (String, Vec<Marker>) {
        let mut out = String::with_capacity(text.len());
        let mut markers = Vec::new();
        let mut chars = text.char_indices().peekable();

        while let Some((start, ch)) = chars.next() {
            if ch != '[' {
                out.push(ch);
                continue;
            }

            let rest = &text[start + 1..];
            let Some(close_rel) = rest.find(']') else {
                out.push(ch);
                continue;
            };
            if rest[..close_rel].contains('\n') {
                out.push(ch);
                continue;
            }
            let inner = &rest[..close_rel];
            let Some(colon) = inner.find(':') else {
                out.push(ch);
                continue;
            };
            let kw = &inner[..colon];
            let target = inner[colon + 1..].trim();

            let Some(kind) = MarkerKind::from_keyword(kw) else {
                out.push(ch);
                continue;
            };
            if target.is_empty() {
                out.push(ch);
                continue;
            }

            markers.push(Marker {
                kind,
                target: target.to_string(),
            });
            let consume_until = start + 1 + close_rel + 1;
            while let Some(&(idx, _)) = chars.peek() {
                if idx >= consume_until {
                    break;
                }
                chars.next();
            }
        }

        // Tidy whitespace left behind by stripped markers.
        let cleaned = out
            .lines()
            .map(|l| l.trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n");

        (cleaned.trim().to_string(), markers)
    }
}

// ─── mention ───────────────────────────────────────────────────────────────
mod mention {
    use matrix_sdk::ruma::UserId;

    pub(super) fn is_mentioned(
        bot_user_id: &UserId,
        bot_display_name: Option<&str>,
        m_mentions_user_ids: Option<&[String]>,
        body: &str,
    ) -> bool {
        if let Some(ids) = m_mentions_user_ids {
            for id in ids {
                if id == bot_user_id.as_str() {
                    return true;
                }
            }
            // Honour the explicit list when set — older clients without
            // `m.mentions` still hit the body-scan fallback below.
            if !ids.is_empty() {
                return false;
            }
        }

        let body_lc = body.to_ascii_lowercase();
        if body_lc.contains(&bot_user_id.as_str().to_ascii_lowercase()) {
            return true;
        }
        let localpart = bot_user_id.localpart().to_ascii_lowercase();
        if body_lc.contains(&format!("@{localpart}")) {
            return true;
        }
        if let Some(name) = bot_display_name
            && !name.is_empty()
        {
            let n = name.to_ascii_lowercase();
            if body_lc.contains(&n) {
                return true;
            }
        }
        false
    }

    /// Group `mention_only` admits either an explicit mention or a direct reply
    /// to a bot message (Telegram parity: replies are unambiguous intent).
    pub(super) fn admit_group_message(is_mentioned: bool, is_reply_to_bot: bool) -> bool {
        is_mentioned || is_reply_to_bot
    }

    /// True when a Matrix event JSON `sender` equals `user_id`.
    /// Production admission uses `TimelineEvent::sender()`; this helper remains
    /// for focused unit coverage of the JSON sender comparison.
    #[cfg(test)]
    pub(super) fn sender_is_user(raw_json: &str, user_id: &UserId) -> bool {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw_json) else {
            return false;
        };
        v.get("sender").and_then(|s| s.as_str()) == Some(user_id.as_str())
    }
}

// ─── allowlist ─────────────────────────────────────────────────────────────
mod allowlist {
    pub(super) fn user_allowed(allowed_users: &[String], sender: &str) -> bool {
        crate::allowlist::is_user_allowed(
            allowed_users,
            sender,
            crate::allowlist::Match::CaseInsensitive,
        )
    }

    /// Matches a voice-peer entry against a joined member's user ID the same
    /// way the runtime matches an inbound sender: a leading `@` is optional on
    /// either side and the comparison ignores ASCII case. Without this, a group
    /// written as `alice:server` would voice ordinary replies (which the
    /// runtime resolves) but silently fail to voice proactive sends (which have
    /// to consult room membership), and a half-working config is exactly the
    /// failure this gate exists to remove.
    pub(super) fn voice_peer_matches(entry: &str, user_id: &str) -> bool {
        entry
            .trim_start_matches('@')
            .eq_ignore_ascii_case(user_id.trim_start_matches('@'))
    }

    /// Voice verdict that needs no member list: an empty voice-peer set voices
    /// nobody, and a `["*"]` set voices every room. `None` means the room's
    /// membership decides, which costs a homeserver round-trip.
    pub(super) fn voice_peers_verdict(voice_peers: &[String]) -> Option<bool> {
        if voice_peers.is_empty() {
            return Some(false);
        }
        if voice_peers.iter().any(|peer| peer == "*") {
            return Some(true);
        }
        None
    }

    pub(super) fn room_allowed_static(allowed_rooms: &[String], room_id: &str) -> bool {
        if allowed_rooms.is_empty() {
            return true;
        }
        allowed_rooms
            .iter()
            .any(|r| r == room_id || r.eq_ignore_ascii_case(room_id))
    }
}

// ─── approval ──────────────────────────────────────────────────────────────
mod approval {
    use rand::{Rng, RngExt};
    use clawcrew_api::channel::{ChannelApprovalResponse, SendMessage};

    pub(super) const TOKEN_LEN: usize = 8;
    const TOKEN_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

    pub(super) fn generate_token<R: Rng>(rng: &mut R) -> String {
        (0..TOKEN_LEN)
            .map(|_| TOKEN_ALPHABET[rng.random_range(0..TOKEN_ALPHABET.len())] as char)
            .collect()
    }

    pub(super) fn generate_token_default() -> String {
        let mut rng = rand::rng();
        generate_token(&mut rng)
    }

    /// Build the outbound `SendMessage` for an approval prompt: the rendered
    /// prompt text to the requesting recipient, with voice synthesis
    /// suppressed. Kept as a small, pure helper so tests can assert its shape
    /// (recipient, voice suppression) without standing up a live client.
    pub(super) fn build_prompt_message(prompt: String, recipient: &str) -> SendMessage {
        SendMessage::new(prompt, recipient).suppress_voice()
    }

    /// Try to parse an approval reply. Returns `Some((token, response))` if the
    /// body matches `<TOKEN> (approve|deny|always|yes|no)` (case-insensitive).
    pub(super) fn parse_reply(body: &str) -> Option<(String, ChannelApprovalResponse)> {
        let trimmed = body.trim();
        let mut parts = trimmed.split_whitespace();
        let token = parts.next()?;
        if token.len() != TOKEN_LEN {
            return None;
        }
        if !token.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        let verb = parts.next()?.to_ascii_lowercase();
        if parts.next().is_some() {
            return None;
        }
        let response = match verb.as_str() {
            crate::util::APPROVAL_REPLY_APPROVE
            | crate::util::APPROVAL_REPLY_YES
            | crate::util::APPROVAL_REPLY_YES_SHORT => ChannelApprovalResponse::Approve,
            crate::util::APPROVAL_REPLY_DENY
            | crate::util::APPROVAL_REPLY_NO
            | crate::util::APPROVAL_REPLY_NO_SHORT => ChannelApprovalResponse::Deny,
            crate::util::APPROVAL_REPLY_ALWAYS => ChannelApprovalResponse::AlwaysApprove,
            _ => return None,
        };
        Some((token.to_uppercase(), response))
    }
}

// ─── room management ──────────────────────────────────────────────────────
mod room_management {
    use super::*;

    pub(super) fn build_create_room_request(
        options: &RoomCreationOptions,
    ) -> Result<CreateRoomRequest> {
        let mut request = CreateRoomRequest::new();
        request.name = options.name.clone();
        request.topic = options.topic.clone();
        request.invite = options
            .invites
            .iter()
            .map(|user_id| {
                user_id
                    .parse::<OwnedUserId>()
                    .with_context(|| format!("matrix: invalid invite user id '{user_id}'"))
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(visibility) = options.visibility {
            request.visibility = match visibility {
                RoomVisibility::Private => MatrixVisibility::Private,
                RoomVisibility::Public => MatrixVisibility::Public,
            };
        }
        if options.encryption.unwrap_or(false) {
            request.initial_state.push(
                InitialStateEvent::with_empty_state_key(
                    RoomEncryptionEventContent::with_recommended_defaults(),
                )
                .to_raw_any(),
            );
        }
        Ok(request)
    }

    pub(super) fn build_invite_user_request(
        room_id: &str,
        user_id: &str,
    ) -> Result<InviteUserRequest> {
        let room_id = room_id
            .parse::<OwnedRoomId>()
            .with_context(|| format!("matrix: invalid room id '{room_id}'"))?;
        let user_id = user_id
            .parse::<OwnedUserId>()
            .with_context(|| format!("matrix: invalid user id '{user_id}'"))?;
        Ok(InviteUserRequest::new(
            room_id,
            InvitationRecipient::from(InviteUserId::new(user_id)),
        ))
    }
}

// ─── context (thread-root preamble) ────────────────────────────────────────
mod context {
    //! Inject the thread root as a `[Thread root from @x]: ...` preamble on the
    //! first inbound message we see in each thread. After a restart we re-inject
    //! exactly once per active thread (in-memory tracking only).

    use std::{collections::HashSet, sync::Arc};

    use matrix_sdk::ruma::{OwnedEventId, events::room::message::MessageType};
    use tokio::sync::RwLock;

    pub(super) fn format_preamble(sender: &str, body: &str) -> String {
        let body = body.trim();
        if body.is_empty() {
            format!("[Thread root from {sender}]\n\n")
        } else {
            format!("[Thread root from {sender}]: {body}\n\n")
        }
    }

    /// Returns `true` iff this thread had not been seen before — caller should
    /// fetch the root and inject the preamble. Also marks the thread seen.
    pub(super) async fn claim_first_visit(
        threads_seen: &Arc<RwLock<HashSet<OwnedEventId>>>,
        thread_id: &OwnedEventId,
    ) -> bool {
        let mut guard = threads_seen.write().await;
        guard.insert(thread_id.clone())
    }

    /// Pre-mark a thread — used when the bot starts the thread itself, so the
    /// next inbound thread message doesn't get a preamble pointing at the bot.
    pub(super) async fn mark_seen(
        threads_seen: &Arc<RwLock<HashSet<OwnedEventId>>>,
        thread_id: OwnedEventId,
    ) {
        threads_seen.write().await.insert(thread_id);
    }

    pub(super) fn body_for(msg: &MessageType) -> String {
        match msg {
            MessageType::Text(t) => t.body.clone(),
            MessageType::Notice(n) => n.body.clone(),
            MessageType::Emote(e) => e.body.clone(),
            MessageType::Image(_) => "[image]".to_string(),
            MessageType::File(_) => "[file]".to_string(),
            MessageType::Audio(_) => "[audio]".to_string(),
            MessageType::Video(_) => "[video]".to_string(),
            MessageType::Location(_) => "[location]".to_string(),
            other => other.body().to_string(),
        }
    }
}

// ─── streaming ─────────────────────────────────────────────────────────────
mod streaming {
    use std::{
        collections::{HashMap, VecDeque},
        time::{Duration, Instant},
    };

    use anyhow::{Result, bail};
    use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId};
    use clawcrew_runtime::agent::loop_::{
        DRAFT_PLACEHOLDER, REASONING_FULL_PREFIX, is_thinking_status_text, thinking_status_round,
    };

    use super::{DraftProgress, DraftProgressKind, MatrixStreamMode, markers};

    const MULTI_MESSAGE_SYNTHETIC_PREFIX: &str = "multi_message_synthetic:";

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub(super) struct DraftKey {
        room_id: OwnedRoomId,
        draft_id: String,
    }

    pub(super) fn draft_key(room_id: OwnedRoomId, draft_id: &str) -> Result<DraftKey> {
        let draft_id = draft_id.trim();
        if draft_id.is_empty() {
            bail!("matrix: draft message id is empty");
        }
        Ok(DraftKey {
            room_id,
            draft_id: draft_id.to_string(),
        })
    }

    pub(super) fn new_multi_message_draft_id() -> String {
        format!(
            "{MULTI_MESSAGE_SYNTHETIC_PREFIX}{}",
            uuid::Uuid::new_v4().simple()
        )
    }

    #[derive(Debug, Clone)]
    pub(super) struct PartialDraft {
        pub event_id: OwnedEventId,
        pub thread_anchor: Option<OwnedEventId>,
        pub last_text: String,
        pub last_edit: Instant,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum PartialFinalizeAction {
        EditDraft,
        RedactDraft,
        EmptyError,
    }

    /// Matrix-only progress draft for `stream_mode = "single_message"`.
    /// It owns only live Matrix event state; the canonical config values stay
    /// on `MatrixConfig`. Lines are stored as a deque so enforcing
    /// `stream_draft_lines` is an O(1) pop from the front instead of repeatedly
    /// re-splitting the rendered draft.
    #[derive(Debug, Clone)]
    pub(super) struct SingleDraft {
        pub event_id: OwnedEventId,
        pub thread_anchor: Option<OwnedEventId>,
        /// Source of truth for the current visible progress window.
        pub lines: VecDeque<DraftProgress>,
        /// Last body confirmed by a successful Matrix edit, used to decide
        /// whether retained drafts need a final flush.
        pub last_text: String,
        /// Last edit attempt timestamp, used only for Matrix edit throttling.
        pub last_edit: Instant,
    }

    /// MultiMessage streaming state. The runtime calls `update_draft` repeatedly
    /// with the accumulated agent output; we send each `\n\n`-bounded paragraph
    /// as its own room message, threaded under `thread_anchor` when present.
    /// `sent_so_far` is a byte counter into the accumulated text — everything
    /// before that index has already been emitted.
    #[derive(Debug, Clone)]
    pub(super) struct MultiDraft {
        pub thread_anchor: Option<OwnedEventId>,
        pub sent_so_far: usize,
    }

    /// Live draft storage for the Matrix stream mode selected at channel
    /// construction. A channel handle has an immutable `MatrixConfig`, so
    /// keeping only the active draft map prevents impossible cross-mode state
    /// while preserving concurrent draft isolation within that mode.
    #[derive(Default, Debug)]
    pub(super) enum State {
        #[default]
        Off,
        Partial(HashMap<DraftKey, PartialDraft>),
        Single(HashMap<DraftKey, SingleDraft>),
        Multi(HashMap<DraftKey, MultiDraft>),
    }

    impl State {
        /// Create the draft store matching the immutable Matrix stream mode.
        pub(super) fn for_stream_mode(mode: MatrixStreamMode) -> Self {
            match mode {
                MatrixStreamMode::Off => Self::Off,
                MatrixStreamMode::Partial => Self::Partial(HashMap::new()),
                MatrixStreamMode::SingleMessage => Self::Single(HashMap::new()),
                MatrixStreamMode::MultiMessage => Self::Multi(HashMap::new()),
            }
        }
    }

    pub(super) fn insert_partial(
        state: &mut State,
        key: DraftKey,
        draft: PartialDraft,
    ) -> Result<()> {
        let State::Partial(drafts) = state else {
            bail!("matrix: partial draft state unavailable");
        };
        drafts.insert(key, draft);
        Ok(())
    }

    pub(super) fn partial_for_update<'a>(
        state: &'a mut State,
        key: &DraftKey,
    ) -> Option<&'a mut PartialDraft> {
        match state {
            State::Partial(drafts) => drafts.get_mut(key),
            _ => None,
        }
    }

    pub(super) fn take_partial(state: &mut State, key: &DraftKey) -> Option<PartialDraft> {
        match state {
            State::Partial(drafts) => drafts.remove(key),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn partial_contains(state: &State, key: &DraftKey) -> bool {
        matches!(state, State::Partial(drafts) if drafts.contains_key(key))
    }

    #[cfg(test)]
    pub(super) fn partial_len(state: &State) -> usize {
        match state {
            State::Partial(drafts) => drafts.len(),
            _ => 0,
        }
    }

    pub(super) fn insert_single(
        state: &mut State,
        key: DraftKey,
        draft: SingleDraft,
    ) -> Result<()> {
        let State::Single(drafts) = state else {
            bail!("matrix: single-message draft state unavailable");
        };
        drafts.insert(key, draft);
        Ok(())
    }

    /// Return the editable `single_message` draft for this room+draft id, if it
    /// is still active.
    pub(super) fn single_for_update<'a>(
        state: &'a mut State,
        key: &DraftKey,
    ) -> Option<&'a mut SingleDraft> {
        match state {
            State::Single(drafts) => drafts.get_mut(key),
            _ => None,
        }
    }

    /// Remove the `single_message` draft from live state at finalize/cancel so
    /// a late progress update cannot keep editing a completed response.
    pub(super) fn take_single(state: &mut State, key: &DraftKey) -> Option<SingleDraft> {
        match state {
            State::Single(drafts) => drafts.remove(key),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn single_contains(state: &State, key: &DraftKey) -> bool {
        matches!(state, State::Single(drafts) if drafts.contains_key(key))
    }

    pub(super) fn insert_multi(state: &mut State, key: DraftKey, draft: MultiDraft) -> Result<()> {
        let State::Multi(drafts) = state else {
            bail!("matrix: multi-message draft state unavailable");
        };
        drafts.insert(key, draft);
        Ok(())
    }

    pub(super) fn multi_for_update<'a>(
        state: &'a mut State,
        key: &DraftKey,
    ) -> Option<&'a mut MultiDraft> {
        match state {
            State::Multi(drafts) => drafts.get_mut(key),
            _ => None,
        }
    }

    pub(super) fn take_multi(state: &mut State, key: &DraftKey) -> Option<MultiDraft> {
        match state {
            State::Multi(drafts) => drafts.remove(key),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn multi_contains(state: &State, key: &DraftKey) -> bool {
        matches!(state, State::Multi(drafts) if drafts.contains_key(key))
    }

    /// Thread anchor of the live draft for `key`, without consuming it. The
    /// finalize path needs it to place a voice note in the same thread as the
    /// text reply it accompanies.
    pub(super) fn peek_thread_anchor(state: &State, key: &DraftKey) -> Option<OwnedEventId> {
        match state {
            State::Off => None,
            State::Partial(m) => m.get(key).and_then(|d| d.thread_anchor.clone()),
            State::Single(m) => m.get(key).and_then(|d| d.thread_anchor.clone()),
            State::Multi(m) => m.get(key).and_then(|d| d.thread_anchor.clone()),
        }
    }

    pub(super) fn partial_should_edit(
        existing: &PartialDraft,
        new_text: &str,
        now: Instant,
        min_interval: Duration,
    ) -> bool {
        if existing.last_text == new_text {
            return false;
        }
        now.saturating_duration_since(existing.last_edit) >= min_interval
    }

    pub(super) fn partial_visible_text(text: &str) -> Option<String> {
        let (cleaned, _) = markers::parse(text);
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_string())
        }
    }

    /// Append one progress update to a single-message draft, dropping the
    /// oldest entries once the configured window is full. A zero limit
    /// intentionally means "unlimited".
    pub(super) fn push_single_progress_line(draft: &mut SingleDraft, text: &str, max_lines: usize) {
        push_single_progress(draft, legacy_single_progress(text), max_lines);
    }

    /// Classify callers of the legacy text-only API using the historical
    /// display convention. Typed callers bypass this heuristic.
    pub(super) fn legacy_single_progress(text: &str) -> DraftProgress {
        let kind = if text.starts_with(REASONING_FULL_PREFIX) && !is_thinking_status_text(text) {
            DraftProgressKind::Reasoning
        } else {
            DraftProgressKind::Status
        };
        DraftProgress {
            kind,
            text: text.to_string(),
        }
    }

    /// Append progress while retaining the source kind supplied by the
    /// orchestrator, including when reasoning renders like generated status.
    pub(super) fn push_single_progress(
        draft: &mut SingleDraft,
        progress: DraftProgress,
        max_lines: usize,
    ) {
        let progress = normalize_matrix_progress(progress);
        if progress.text.is_empty() {
            return;
        }
        if merge_single_progress_line(draft, &progress) {
            trim_single_visible_lines(draft, max_lines);
            return;
        }
        draft.lines.push_back(progress);
        trim_single_visible_lines(draft, max_lines);
    }

    /// Reasoning arrives as provider stream fragments. Keep it as one Matrix
    /// transcript entry and let `draft_update_interval_ms` decide how often
    /// that growing text is edited into the room.
    fn merge_single_progress_line(draft: &mut SingleDraft, progress: &DraftProgress) -> bool {
        if let Some(incoming_round) = single_thinking_status_round(progress)
            && let Some(existing) = draft.lines.back_mut()
            && is_single_thinking_status(existing)
        {
            if incoming_round >= single_thinking_status_round(existing).unwrap_or(0) {
                *existing = progress.clone();
            }
            return true;
        }

        if let Some(fragment) = progress.text.strip_prefix(REASONING_FULL_PREFIX)
            && let Some(existing) = draft.lines.back_mut()
            && is_single_reasoning_progress(existing)
        {
            existing.text.push_str(fragment);
            return true;
        }

        false
    }

    fn single_thinking_status_round(progress: &DraftProgress) -> Option<usize> {
        matches!(progress.kind, DraftProgressKind::Status)
            .then(|| thinking_status_round(&progress.text))
            .flatten()
    }

    fn is_single_thinking_status(progress: &DraftProgress) -> bool {
        single_thinking_status_round(progress).is_some()
    }

    fn is_single_reasoning_progress(progress: &DraftProgress) -> bool {
        matches!(progress.kind, DraftProgressKind::Reasoning)
            && progress.text.starts_with(REASONING_FULL_PREFIX)
    }

    fn visible_line_count(progress: &DraftProgress) -> usize {
        single_render_line(progress).split('\n').count().max(1)
    }

    fn trim_visible_lines_from_front(
        progress: &DraftProgress,
        remove_lines: usize,
    ) -> DraftProgress {
        if remove_lines == 0 {
            return progress.clone();
        }

        let rendered = single_render_line(progress);
        let total = visible_line_count(progress);
        if remove_lines >= total {
            let mut empty = progress.clone();
            empty.text.clear();
            return empty;
        }

        let retained = rendered
            .split('\n')
            .skip(remove_lines)
            .collect::<Vec<_>>()
            .join("\n");
        let text = if is_single_reasoning_progress(progress)
            && !retained.starts_with(REASONING_FULL_PREFIX)
        {
            format!("{REASONING_FULL_PREFIX}{retained}")
        } else {
            retained
        };
        DraftProgress {
            kind: progress.kind,
            text,
        }
    }

    fn trim_single_visible_lines(draft: &mut SingleDraft, max_lines: usize) {
        if max_lines == 0 {
            return;
        }
        let mut total_lines = draft.lines.iter().map(visible_line_count).sum::<usize>();
        while total_lines > max_lines {
            let remove_lines = total_lines - max_lines;
            let Some(front) = draft.lines.front_mut() else {
                break;
            };
            let front_lines = visible_line_count(front);
            if remove_lines >= front_lines {
                draft.lines.pop_front();
                total_lines = total_lines.saturating_sub(front_lines);
            } else {
                *front = trim_visible_lines_from_front(front, remove_lines);
                break;
            }
        }
    }

    /// Keep multiline reasoning readable while preventing progress content from
    /// becoming Matrix Markdown or HTML formatting. This is deliberately an
    /// output transport encoder: it is applied once at insertion, never while
    /// re-rendering a retained draft.
    /// Tool/status progress remains one logical line; only raw reasoning gets
    /// real newlines.
    #[cfg(test)]
    pub(super) fn normalize_matrix_progress_line(text: &str) -> String {
        let kind = if text.starts_with(REASONING_FULL_PREFIX) && !is_thinking_status_text(text) {
            DraftProgressKind::Reasoning
        } else {
            DraftProgressKind::Status
        };
        normalize_matrix_progress(DraftProgress {
            kind,
            text: text.to_string(),
        })
        .text
    }

    fn normalize_matrix_progress(mut progress: DraftProgress) -> DraftProgress {
        if is_single_thinking_status(&progress) {
            return progress;
        }

        let preserve_newlines = is_single_reasoning_progress(&progress);
        let text = progress.text;
        let mut normalized = String::with_capacity(text.len().saturating_mul(2));
        let mut chars = text.trim_end_matches(&['\r', '\n'][..]).chars().peekable();
        let mut line_start = true;
        let mut leading_spaces = 0usize;
        while let Some(ch) = chars.next() {
            match ch {
                '\n' if preserve_newlines => {
                    normalized.push('\n');
                    line_start = true;
                    leading_spaces = 0;
                }
                '\n' => normalized.push('␊'),
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    if preserve_newlines {
                        normalized.push('\n');
                        line_start = true;
                        leading_spaces = 0;
                    } else {
                        normalized.push('␊');
                    }
                }
                '\t' => normalized.push('␉'),
                '\u{000b}' => normalized.push('␋'),
                '\u{000c}' => normalized.push('␌'),
                '\u{001b}' => normalized.push('␛'),
                '\u{007f}' => normalized.push('␡'),
                c if c.is_control() => normalized.push('�'),
                ' ' if line_start => {
                    leading_spaces += 1;
                    if leading_spaces == 4 {
                        // A non-breaking first indentation space keeps literal
                        // progress out of CommonMark's indented-code mode.
                        let start = normalized.len().saturating_sub(3);
                        normalized.replace_range(start..start + 1, "\u{00a0}");
                        normalized.push(' ');
                    } else {
                        normalized.push(' ');
                    }
                }
                c if c.is_ascii_punctuation() => {
                    normalized.push('\\');
                    normalized.push(c);
                }
                c => normalized.push(c),
            }
            if !matches!(ch, ' ' | '\n' | '\r') {
                line_start = false;
            }
        }
        progress.text = normalized;
        progress
    }

    fn single_render_line(progress: &DraftProgress) -> &str {
        if is_single_thinking_status(progress) {
            progress.text.trim_end_matches('\n')
        } else {
            &progress.text
        }
    }

    #[derive(Clone)]
    struct VisibleProgressUnit {
        text: String,
        reasoning: bool,
    }

    fn visible_progress_units(draft: &SingleDraft) -> Vec<VisibleProgressUnit> {
        draft
            .lines
            .iter()
            .flat_map(|progress| {
                if let Some(reasoning) = progress.text.strip_prefix(REASONING_FULL_PREFIX)
                    && is_single_reasoning_progress(progress)
                {
                    reasoning
                        .split('\n')
                        .map(|text| VisibleProgressUnit {
                            text: text.to_string(),
                            reasoning: true,
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![VisibleProgressUnit {
                        text: single_render_line(progress).to_string(),
                        reasoning: false,
                    }]
                }
            })
            .collect()
    }

    fn render_visible_progress_units(units: &[VisibleProgressUnit]) -> String {
        let mut text = String::new();
        let mut previous_reasoning = false;
        for unit in units {
            if !text.is_empty() {
                text.push('\n');
            }
            if unit.reasoning && !previous_reasoning {
                text.push_str(REASONING_FULL_PREFIX);
            }
            text.push_str(&unit.text);
            previous_reasoning = unit.reasoning;
        }
        text
    }

    fn oversized_progress_alert(unit: &VisibleProgressUnit) -> String {
        let alert = clawcrew_runtime::i18n::get_required_cli_string(
            "channel-runtime-matrix-progress-item-too-large",
        );
        if unit.reasoning {
            return format!("{REASONING_FULL_PREFIX}{alert}");
        }
        let text = unit.text.trim_end();
        if let Some((marker, subject)) = text.split_once(' ')
            && matches!(marker, "⏳" | "✅" | "❌")
        {
            let subject = subject
                .split_once("\\:")
                .map_or(subject, |(tool, _)| tool)
                .trim();
            return format!("{marker} {subject}: {alert}");
        }
        format!("⚠️ {alert}")
    }

    fn single_visible_text_with_fit<F>(draft: &SingleDraft, fits: F) -> String
    where
        F: Fn(&str) -> bool,
    {
        let units = visible_progress_units(draft);
        let mut start = units.len();
        while start > 0 {
            let candidate = render_visible_progress_units(&units[start - 1..]);
            if !fits(&candidate) {
                break;
            }
            start -= 1;
        }
        if start == units.len() {
            return units
                .last()
                .map(oversized_progress_alert)
                .filter(|alert| fits(alert))
                .unwrap_or_default();
        }
        render_visible_progress_units(&units[start..])
    }

    /// Render the newest complete physical reasoning lines and atomic progress
    /// entries that fit within a byte budget. A separately supplied exact
    /// Matrix-event fitter is used in production; this source-byte version is
    /// retained for local state tests.
    #[cfg(test)]
    pub(super) fn single_visible_text_with_budget(draft: &SingleDraft, max_bytes: usize) -> String {
        if max_bytes == 0 {
            return String::new();
        }
        single_visible_text_with_fit(draft, |text| text.len() <= max_bytes)
    }

    pub(super) fn single_visible_text_with_edit_budget(
        draft: &SingleDraft,
        max_bytes: usize,
    ) -> String {
        single_visible_text_with_fit(draft, |text| {
            super::outbound::serialized_edit_content_len(text, &draft.event_id)
                .is_some_and(|actual| actual <= max_bytes)
        })
    }

    /// Check the Matrix edit-attempt interval before rendering the draft body.
    /// Progress lines are still recorded first, so retained drafts can flush
    /// the latest transcript during finalization without paying render cost on
    /// every debounced tick.
    pub(super) fn single_edit_interval_elapsed(
        existing: &SingleDraft,
        now: Instant,
        min_interval: Duration,
    ) -> bool {
        now.saturating_duration_since(existing.last_edit) >= min_interval
    }

    /// Avoid duplicate Matrix edits after rendering confirms that the visible
    /// transcript is unchanged.
    pub(super) fn single_render_changed(existing: &SingleDraft, new_text: &str) -> bool {
        existing.last_text != new_text
    }

    /// Mark a rendered single-message draft body as Matrix-visible only after
    /// the edit request succeeds. Keeping `last_text` as a delivery checkpoint
    /// lets retained drafts flush again during finalization after a failed edit.
    pub(super) fn mark_single_edit_delivered(
        existing: &mut SingleDraft,
        event_id: &OwnedEventId,
        visible_text: String,
        delivered_at: Instant,
    ) -> bool {
        if existing.event_id.as_str() != event_id.as_str() {
            return false;
        }
        existing.last_text = visible_text;
        existing.last_edit = delivered_at;
        true
    }

    /// Finalization sequence for Matrix `single_message` mode. Encoding the
    /// delete/send ordering here keeps the user-visible timeline rule testable
    /// without mocking Matrix network calls.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum SingleFinalizePlan {
        DeleteDraftThenSendFinal,
        KeepDraftThenSendFinal,
        SendFinalOnly,
        Noop,
    }

    /// Cleanup needed for a retained single-message draft before the final
    /// answer can be posted. Retention applies to durable progress transcripts,
    /// not to the initial placeholder.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum SingleRetainedDraftAction {
        DeletePlaceholder,
        Flush(String),
        KeepCurrent,
    }

    impl SingleFinalizePlan {
        pub(super) fn deletes_draft_first(self) -> bool {
            matches!(self, Self::DeleteDraftThenSendFinal)
        }

        pub(super) fn keeps_draft(self) -> bool {
            matches!(self, Self::KeepDraftThenSendFinal)
        }

        pub(super) fn sends_final(self) -> bool {
            !matches!(self, Self::Noop)
        }
    }

    /// Choose the single-message finalization sequence from durable state and
    /// operator config. A missing draft only suppresses empty final text; when a
    /// draft existed, final delivery is attempted even if the text is empty so
    /// the normal Matrix send path reports any invalid final body.
    pub(super) fn single_finalize_plan(
        has_draft: bool,
        delete_draft: bool,
        final_has_text: bool,
    ) -> SingleFinalizePlan {
        match (has_draft, delete_draft, final_has_text) {
            (true, true, _) => SingleFinalizePlan::DeleteDraftThenSendFinal,
            (true, false, _) => SingleFinalizePlan::KeepDraftThenSendFinal,
            (false, _, true) => SingleFinalizePlan::SendFinalOnly,
            (false, _, false) => SingleFinalizePlan::Noop,
        }
    }

    /// Decide how to make a retained draft coherent before the final answer is
    /// sent. This bypasses edit debounce at finalize time so a kept progress
    /// transcript cannot lag behind the in-memory sliding buffer.
    pub(super) fn single_retained_draft_action(
        draft: &SingleDraft,
        max_bytes: usize,
    ) -> SingleRetainedDraftAction {
        if draft.lines.is_empty() {
            return SingleRetainedDraftAction::DeletePlaceholder;
        }
        let visible_text = single_visible_text_with_edit_budget(draft, max_bytes);
        if visible_text.is_empty() {
            return if draft.last_text == DRAFT_PLACEHOLDER {
                // The buffered lines are not Matrix-visible until an edit
                // succeeds. Retention must not leave a placeholder behind
                // when no visible progress body can be established.
                SingleRetainedDraftAction::DeletePlaceholder
            } else {
                SingleRetainedDraftAction::KeepCurrent
            };
        }
        if visible_text == draft.last_text {
            SingleRetainedDraftAction::KeepCurrent
        } else {
            SingleRetainedDraftAction::Flush(visible_text)
        }
    }

    /// Cancel removes drafts when the operator requested deletion or when
    /// Matrix still only shows the initial placeholder. The line buffer may
    /// contain undelivered progress after a failed edit, so the delivery
    /// checkpoint is the correct source for the user-visible state.
    pub(super) fn single_cancel_deletes_draft(draft: &SingleDraft, delete_draft: bool) -> bool {
        delete_draft || draft.last_text == DRAFT_PLACEHOLDER
    }

    pub(super) fn decide_partial_finalize_action(
        text_is_empty_after_delivery: bool,
        any_attachment_landed: bool,
    ) -> PartialFinalizeAction {
        match (text_is_empty_after_delivery, any_attachment_landed) {
            (false, _) => PartialFinalizeAction::EditDraft,
            (true, true) => PartialFinalizeAction::RedactDraft,
            (true, false) => PartialFinalizeAction::EmptyError,
        }
    }

    /// Find the next paragraph break (`\n\n`) in `new_text`, ignoring any
    /// breaks that fall inside an open ```fenced``` code block. Returns the
    /// byte offset of the first `\n` of the break, or `None` if no break is
    /// found yet (caller should buffer and retry on the next update).
    pub(super) fn next_paragraph_break(new_text: &str) -> Option<usize> {
        let bytes = new_text.as_bytes();
        let mut in_fence = false;
        let mut i = 0;
        while i < bytes.len() {
            // Detect opening or closing ```code fence``` at line start.
            if bytes[i] == b'`'
                && i + 2 < bytes.len()
                && bytes[i + 1] == b'`'
                && bytes[i + 2] == b'`'
                && (i == 0 || bytes[i - 1] == b'\n')
            {
                in_fence = !in_fence;
                i += 3;
                continue;
            }
            if !in_fence && bytes[i] == b'\n' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

// ─── session ───────────────────────────────────────────────────────────────
mod session {
    //! Persist the Matrix login session next to the SDK SQLite crypto store so
    //! `restore_session()` can reattach without re-running the login flow.

    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Serialize};

    pub(super) const SESSION_FILE: &str = "session.json";

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub(super) struct SessionBlob {
        pub user_id: String,
        pub device_id: String,
        pub access_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub refresh_token: Option<String>,
    }

    pub(super) fn path(state_dir: &Path) -> PathBuf {
        state_dir.join(SESSION_FILE)
    }

    pub(super) fn load(state_dir: &Path) -> anyhow::Result<Option<SessionBlob>> {
        let p = path(state_dir);
        if !p.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&p).map_err(|e| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "path": p.display().to_string(),
                        "error": format!("{}", e),
                    })),
                "matrix: failed to read session blob"
            );
            anyhow::Error::msg(format!("read matrix session blob {}: {e}", p.display()))
        })?;
        match serde_json::from_slice::<SessionBlob>(&bytes) {
            Ok(blob) => Ok(Some(blob)),
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                    &format!(
                        "matrix: session blob {} is corrupt JSON ({e}); treating as missing so auto-recovery can re-login",
                        p.display()
                    )
                );
                Ok(None)
            }
        }
    }

    pub(super) fn save(state_dir: &Path, blob: &SessionBlob) -> anyhow::Result<()> {
        std::fs::create_dir_all(state_dir).map_err(|e| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "path": state_dir.display().to_string(),
                        "error": format!("{}", e),
                    })),
                "matrix: failed to create state dir"
            );
            anyhow::Error::msg(format!(
                "create matrix state dir {}: {e}",
                state_dir.display()
            ))
        })?;
        let p = path(state_dir);
        let json = serde_json::to_vec_pretty(blob)?;
        write_with_owner_only(&p, &json).map_err(|e| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "path": p.display().to_string(),
                        "error": format!("{}", e),
                    })),
                "matrix: failed to write session blob"
            );
            anyhow::Error::msg(format!("write matrix session blob {}: {e}", p.display()))
        })?;
        Ok(())
    }

    /// Write the session blob with `0o600` permissions on Unix so the
    /// access token isn't world-readable under a permissive umask.
    /// Windows falls back to default ACLs (the std-lib write).
    #[cfg(unix)]
    fn write_with_owner_only(path: &Path, contents: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents)
    }

    #[cfg(not(unix))]
    fn write_with_owner_only(path: &Path, contents: &[u8]) -> std::io::Result<()> {
        std::fs::write(path, contents)
    }
}

// ─── client ────────────────────────────────────────────────────────────────
mod client {
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use anyhow::{Context as _, Result, bail};
    use matrix_sdk::{
        Client, SessionMeta, SessionTokens,
        authentication::matrix::MatrixSession,
        config::RequestConfig,
        ruma::{OwnedRoomId, RoomAliasId},
    };
    use serde::Deserialize;
    use tokio::sync::RwLock;

    use super::session;
    use clawcrew_config::schema::MatrixConfig;

    const WHOAMI_ENDPOINT: &str = "_matrix/client/v3/account/whoami";

    pub(super) const CLIENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
    const WHOAMI_TIMEOUT: Duration = Duration::from_secs(30);
    const WHOAMI_ERROR_BODY_PREVIEW_BYTES: usize = 4096;
    const WHOAMI_ERROR_BODY_DISPLAY_CHARS: usize = 256;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct AccessTokenIdentity {
        pub user_id: String,
        pub device_id: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct WhoamiResponse {
        user_id: String,
        #[serde(default)]
        device_id: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct MatrixErrorResponse {
        #[serde(default)]
        errcode: Option<String>,
        #[serde(default)]
        error: Option<String>,
    }

    pub(super) fn store_dir(state_dir: &Path) -> PathBuf {
        state_dir.join("store")
    }

    pub(super) async fn build(config: &MatrixConfig, state_dir: &Path) -> Result<Client> {
        build_attempt(config, state_dir, 0).await
    }

    fn wipe_state(state_dir: &Path) -> Result<()> {
        let session = session::path(state_dir);
        if session.exists()
            && let Err(e) = std::fs::remove_file(&session)
        {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "path": session.display().to_string(),
                        "phase": "corruption_recovery",
                        "error": format!("{}", e),
                    })),
                "matrix: failed to remove session blob during corruption recovery"
            );
            return Err(anyhow::Error::msg(format!(
                "matrix: failed to remove {} during corruption recovery: {e}. Fix permissions or wipe the directory manually.",
                session.display()
            )));
        }
        let store = store_dir(state_dir);
        if store.exists()
            && let Err(e) = std::fs::remove_dir_all(&store)
        {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "path": store.display().to_string(),
                        "phase": "corruption_recovery",
                        "error": format!("{}", e),
                    })),
                "matrix: failed to remove store dir during corruption recovery"
            );
            return Err(anyhow::Error::msg(format!(
                "matrix: failed to remove {} during corruption recovery: {e}. Fix permissions or wipe the directory manually.",
                store.display()
            )));
        }
        Ok(())
    }

    pub(super) fn store_has_orphan_data(state_dir: &Path) -> bool {
        let store = store_dir(state_dir);
        let Ok(mut entries) = std::fs::read_dir(&store) else {
            return false;
        };
        entries.any(|e| e.is_ok())
    }

    pub(super) fn can_password_relogin(config: &MatrixConfig) -> bool {
        let has_password = config
            .password
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let has_user_id = config
            .user_id
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        has_password && has_user_id
    }

    pub(super) fn saved_session_is_foreign(
        config: &MatrixConfig,
        blob: &session::SessionBlob,
    ) -> bool {
        let Some(want) = config.user_id.as_deref().filter(|s| !s.is_empty()) else {
            return false;
        };
        if !want.contains(':') {
            return false;
        }
        want != blob.user_id.as_str()
    }

    async fn build_attempt(
        config: &MatrixConfig,
        state_dir: &Path,
        recovery_attempts: u32,
    ) -> Result<Client> {
        // Hard recursion bound: at most one auto-wipe + relogin cycle per call.
        if recovery_attempts > 1 {
            bail!(
                "matrix: corruption recovery looped — aborting to avoid an infinite restart cycle. \
                 Wipe ~/.clawcrew/state/matrix/ manually and restart."
            );
        }

        let saved = session::load(state_dir)?;

        // A saved session that belongs to a different account would run this
        // channel block as the wrong Matrix identity. Wipe and re-login fresh
        // under the configured account instead of impersonating.
        if let Some(blob) = saved.as_ref()
            && saved_session_is_foreign(config, blob)
        {
            return recover_or_bail(
                config,
                state_dir,
                recovery_attempts,
                &format!(
                    "saved session user_id ({}) does not match configured channels.matrix user_id ({}); store belongs to a different account.",
                    blob.user_id,
                    config.user_id.as_deref().unwrap_or_default()
                ),
            )
            .await;
        }

        if let (Some(blob), Some(want)) = (
            saved.as_ref(),
            config.device_id.as_deref().filter(|s| !s.is_empty()),
        ) && want != blob.device_id
        {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                &format!(
                    "matrix: configured channels.matrix.device-id ({want}) differs from the saved session ({}). \
                 Honoring the saved device_id (canonical, assigned by the homeserver). \
                 Update channels.matrix.device-id to match (or clear it) to silence this warning, \
                 or wipe {} entirely to register a different device.",
                    blob.device_id,
                    state_dir.display()
                )
            );
        }

        if saved.is_none() && store_has_orphan_data(state_dir) {
            return recover_or_bail(
                config,
                state_dir,
                recovery_attempts,
                "found crypto store data without a saved session.json — orphan state from a prior install or interrupted run.",
            )
            .await;
        }

        let store = store_dir(state_dir);
        std::fs::create_dir_all(&store)
            .with_context(|| format!("create matrix store dir {}", store.display()))?;

        let client = Client::builder()
            .server_name_or_homeserver_url(&config.homeserver)
            .sqlite_store(&store, None)
            // Widen the per-request timeout past the sync long-poll window so
            // an idle `/sync` never trips the SDK's default 30s request
            // deadline before the homeserver's own long-poll returns.
            .request_config(RequestConfig::new().timeout(CLIENT_REQUEST_TIMEOUT))
            .build()
            .await
            .context("build matrix client")?;

        // Step 1: restore an existing session, or fresh-login.
        if let Some(blob) = saved {
            let saved_device_id = blob.device_id.clone();
            let session = MatrixSession {
                meta: SessionMeta {
                    user_id: blob.user_id.parse().context("parse stored user_id")?,
                    device_id: blob.device_id.into(),
                },
                tokens: SessionTokens {
                    access_token: blob.access_token,
                    refresh_token: blob.refresh_token,
                },
            };
            match client
                .matrix_auth()
                .restore_session(session, matrix_sdk::store::RoomLoadSettings::default())
                .await
            {
                Ok(()) => ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                    "matrix: restored session from session.json"
                ),
                Err(e) => {
                    // restore_session failed despite a matching device_id —
                    // the access token is probably revoked, or the saved
                    // session disagrees with the local crypto store.
                    drop(client);
                    return recover_or_bail(
                        config,
                        state_dir,
                        recovery_attempts,
                        &format!(
                            "restore_session failed for device_id {saved_device_id}: {e}. \
                             The access token is likely revoked or the local crypto store is inconsistent."
                        ),
                    )
                    .await;
                }
            }

            let otk_corruption_flagged = client
                .state_store()
                .get_kv_data(matrix_sdk::store::StateStoreDataKey::OneTimeKeyAlreadyUploaded)
                .await
                .ok()
                .flatten()
                .is_some();
            if otk_corruption_flagged {
                drop(client);
                return recover_or_bail(
                    config,
                    state_dir,
                    recovery_attempts,
                    "matrix-sdk has flagged the local crypto store as out-of-sync with server-side one-time keys (StateStoreDataKey::OneTimeKeyAlreadyUploaded). The local store has lost track of OTKs that the server still records — fresh sends would fail to decrypt. The SDK has no in-place fix for this state.",
                )
                .await;
            }
        } else {
            login_fresh(&client, config).await?;
            if let Some(blob) = session_blob_from(&client)
                && let Err(e) = session::save(state_dir, &blob)
            {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                    "matrix: failed to persist session.json"
                );
            }
        }

        if let Some(key) = config.recovery_key.as_deref()
            && !key.is_empty()
        {
            run_recovery(&client, key).await;
        }

        Ok(client)
    }

    /// Either auto-wipe + retry (when password + user_id are configured) or
    /// bail with operator-actionable instructions.
    async fn recover_or_bail(
        config: &MatrixConfig,
        state_dir: &Path,
        recovery_attempts: u32,
        reason: &str,
    ) -> Result<Client> {
        if can_password_relogin(config) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                &format!(
                    "matrix: {reason} Auto-recovering: wiping {} and re-authenticating with password.",
                    state_dir.display()
                )
            );
            wipe_state(state_dir)?;
            return Box::pin(build_attempt(config, state_dir, recovery_attempts + 1)).await;
        }
        bail!(
            "matrix: {reason}\n\
             Cannot auto-recover because channels.matrix.password and channels.matrix.user-id are not both set.\n\
             Either:\n  \
             • configure channels.matrix.password (and user-id) so the next start can re-authenticate, or\n  \
             • wipe the state directory manually:  rm -rf {}",
            state_dir.display(),
        );
    }

    async fn login_fresh(client: &Client, config: &MatrixConfig) -> Result<()> {
        // Prefer password when set: it creates a server-side device matching
        // `config.device_id`, so subsequent crypto operations don't fight with
        // a token bound to a different device.
        if let Some(pw) = config.password.as_deref().filter(|s| !s.is_empty()) {
            return password_login(client, config, pw).await;
        }
        if config
            .access_token
            .as_deref()
            .is_some_and(|t| !t.is_empty())
        {
            return access_token_login(client, config).await;
        }
        bail!("matrix login requires either access_token or user_id+password")
    }

    async fn password_login(client: &Client, config: &MatrixConfig, password: &str) -> Result<()> {
        let user_id = config
            .user_id
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure),
                    "matrix.user_id is required for password login"
                );
                anyhow::Error::msg("matrix.user_id is required for password login")
            })?;
        let mut login = client
            .matrix_auth()
            .login_username(&user_id, password)
            .initial_device_display_name("ClawCrew");
        if let Some(d) = config.device_id.as_deref()
            && !d.is_empty()
        {
            login = login.device_id(d);
        }
        login.send().await.context("password login failed")?;
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            "matrix: logged in via password"
        );
        Ok(())
    }

    async fn access_token_login(client: &Client, config: &MatrixConfig) -> Result<()> {
        let identity = resolve_access_token_identity(config, &client.homeserver()).await?;
        let user_id = identity.user_id.parse().context("parse matrix.user_id")?;
        let device_id = identity.device_id.ok_or_else(|| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure),
                "matrix: access-token login requires a Matrix device_id"
            );
            anyhow::Error::msg("matrix: access-token login requires a Matrix device_id")
        })?;
        let session = MatrixSession {
            meta: SessionMeta {
                user_id,
                device_id: device_id.into(),
            },
            tokens: SessionTokens {
                access_token: config.access_token.clone().ok_or_else(|| {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure),
                        "matrix.access_token is required for token login"
                    );
                    anyhow::Error::msg("matrix.access_token is required for token login")
                })?,
                refresh_token: None,
            },
        };
        client
            .matrix_auth()
            .restore_session(session, matrix_sdk::store::RoomLoadSettings::default())
            .await
            .context("attach matrix session via access_token")?;
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            "matrix: logged in via access_token"
        );
        Ok(())
    }

    fn non_empty_config_value(value: Option<&str>) -> Option<String> {
        value
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
    }

    pub(super) async fn resolve_access_token_identity(
        config: &MatrixConfig,
        homeserver: &reqwest::Url,
    ) -> Result<AccessTokenIdentity> {
        let configured_user_id = non_empty_config_value(config.user_id.as_deref());
        let configured_device_id = non_empty_config_value(config.device_id.as_deref());

        if let (Some(user_id), Some(device_id)) =
            (configured_user_id.as_ref(), configured_device_id.as_ref())
        {
            return Ok(AccessTokenIdentity {
                user_id: user_id.clone(),
                device_id: Some(device_id.clone()),
            });
        }

        let whoami = fetch_access_token_whoami(config, homeserver).await?;

        if let Some(ref configured) = configured_user_id
            && configured != &whoami.user_id
        {
            bail!(
                "matrix: configured channels.matrix.user-id ({configured}) does not match Matrix whoami user_id ({})",
                whoami.user_id
            );
        }

        if let (Some(configured), Some(actual)) = (&configured_device_id, &whoami.device_id)
            && configured != actual
        {
            bail!(
                "matrix: configured channels.matrix.device-id ({configured}) does not match Matrix whoami device_id ({actual})"
            );
        }

        if configured_device_id.is_none() && whoami.device_id.is_none() {
            bail!(
                "matrix: whoami response did not include device_id; configure channels.matrix.device-id for access-token login"
            );
        }

        Ok(AccessTokenIdentity {
            user_id: configured_user_id.unwrap_or(whoami.user_id),
            device_id: configured_device_id.or(whoami.device_id),
        })
    }

    async fn fetch_access_token_whoami(
        config: &MatrixConfig,
        homeserver: &reqwest::Url,
    ) -> Result<WhoamiResponse> {
        let access_token = config
            .access_token
            .as_deref()
            .context("matrix: whoami requires access_token")?;
        let url = matrix_client_api_url(homeserver, WHOAMI_ENDPOINT);
        let response = clawcrew_config::schema::apply_runtime_proxy_to_builder(
            reqwest::Client::builder().timeout(WHOAMI_TIMEOUT),
            "channel.matrix",
        )
        .build()
        .context("matrix: build whoami HTTP client")?
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await
        .context("matrix: whoami request failed")?;
        let status = response.status();

        if !status.is_success() {
            let body = read_whoami_error_body_preview(response).await;
            bail!("matrix: whoami request failed with HTTP {status}: {body}");
        }

        let mut whoami = response
            .json::<WhoamiResponse>()
            .await
            .context("matrix: failed to parse whoami response")?;
        whoami.user_id = whoami.user_id.trim().to_string();
        if whoami.user_id.is_empty() {
            bail!("matrix: whoami response did not include user_id");
        }
        whoami.device_id = whoami
            .device_id
            .map(|device_id| device_id.trim().to_string())
            .filter(|device_id| !device_id.is_empty());

        Ok(whoami)
    }

    async fn read_whoami_error_body_preview(mut response: reqwest::Response) -> String {
        let mut preview = Vec::new();
        let mut truncated = false;

        while preview.len() < WHOAMI_ERROR_BODY_PREVIEW_BYTES {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(err) => return format!("failed to read response body: {err}"),
            };
            let remaining = WHOAMI_ERROR_BODY_PREVIEW_BYTES - preview.len();
            if chunk.len() > remaining {
                preview.extend_from_slice(&chunk[..remaining]);
                truncated = true;
                break;
            }
            preview.extend_from_slice(&chunk);
        }

        if preview.len() == WHOAMI_ERROR_BODY_PREVIEW_BYTES {
            truncated = true;
        }

        format_whoami_error_body_preview(&preview, truncated)
    }

    fn format_whoami_error_body_preview(preview: &[u8], truncated: bool) -> String {
        if let Ok(error) = serde_json::from_slice::<MatrixErrorResponse>(preview) {
            let errcode = error
                .errcode
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let message = error
                .error
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let formatted = match (errcode, message) {
                (Some(errcode), Some(message)) => Some(format!("{errcode}: {message}")),
                (Some(errcode), None) => Some(errcode.to_string()),
                (None, Some(message)) => Some(message.to_string()),
                (None, None) => None,
            };
            if let Some(formatted) = formatted {
                return truncate_with_ellipsis(&formatted, WHOAMI_ERROR_BODY_DISPLAY_CHARS);
            }
        }

        let body = String::from_utf8_lossy(preview).trim().to_string();
        if body.is_empty() {
            return "<empty response body>".to_string();
        }
        let mut body = truncate_with_ellipsis(&body, WHOAMI_ERROR_BODY_DISPLAY_CHARS);
        if truncated {
            body.push_str(" [truncated]");
        }
        body
    }

    fn truncate_with_ellipsis(value: &str, max_chars: usize) -> String {
        let mut chars = value.chars();
        let mut truncated: String = chars.by_ref().take(max_chars).collect();
        if chars.next().is_some() {
            truncated.push_str("...");
        }
        truncated
    }

    fn matrix_client_api_url(homeserver: &reqwest::Url, endpoint_path: &str) -> reqwest::Url {
        let mut url = homeserver.clone();
        let base_path = url.path().trim_end_matches('/');
        let endpoint_path = endpoint_path.trim_start_matches('/');
        let full_path = if base_path.is_empty() || base_path == "/" {
            format!("/{endpoint_path}")
        } else {
            format!("{base_path}/{endpoint_path}")
        };
        url.set_path(&full_path);
        url.set_query(None);
        url.set_fragment(None);
        url
    }

    fn session_blob_from(client: &Client) -> Option<session::SessionBlob> {
        let session = client.matrix_auth().session()?;
        Some(session::SessionBlob {
            user_id: session.meta.user_id.to_string(),
            device_id: session.meta.device_id.to_string(),
            access_token: session.tokens.access_token,
            refresh_token: session.tokens.refresh_token,
        })
    }

    async fn run_recovery(client: &Client, key: &str) {
        use matrix_sdk::encryption::recovery::RecoveryState;

        let recovery = client.encryption().recovery();
        if matches!(recovery.state(), RecoveryState::Enabled) {
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "matrix: recovery already enabled, skipping recover()"
            );
            return;
        }

        let stripped_len = key.chars().filter(|c| !c.is_whitespace()).count();
        diagnose_secret_storage(client, stripped_len).await;

        match recovery.recover_and_fix_backup(key).await {
            Ok(()) => ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "matrix: E2EE recovery completed (cross-signing + room keys imported; key backup repaired if inconsistent)"
            ),
            Err(e) => ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"e": e.to_string()})),
                "matrix: E2EE recovery failed: ; full error chain = . If the input length above is unexpected (base58 keys are typically ~58 chars, passphrases vary), the wrong value may be in channels.matrix.recovery-key."
            ),
        }
    }

    async fn diagnose_secret_storage(client: &Client, input_len: usize) {
        use matrix_sdk::ruma::events::secret_storage::{
            default_key::SecretStorageDefaultKeyEventContent, key::SecretStorageKeyEventContent,
        };
        use matrix_sdk::ruma::events::{GlobalAccountDataEventType, StaticEventContent};

        let account = client.account();
        let default_key = match account
            .fetch_account_data_static::<SecretStorageDefaultKeyEventContent>()
            .await
        {
            Ok(Some(raw)) => match raw.deserialize() {
                Ok(content) => Some(content),
                Err(e) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                        "matrix: cannot deserialize default secret-storage key event"
                    );
                    None
                }
            },
            Ok(None) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"input_len": input_len})),
                    "matrix: server has no m.secret_storage.default_key set; recovery cannot proceed (input_len=). Set up Secure Backup in Element first."
                );
                return;
            }
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                    "matrix: failed to fetch default secret-storage key event"
                );
                return;
            }
        };
        let Some(default_key) = default_key else {
            return;
        };
        let key_id = default_key.key_id;

        // Fetch the actual key event for the default key id so we can see
        // whether it has passphrase info (affects which decode path the SDK
        // tries first inside SecretStorageKey::from_account_data).
        let event_type = GlobalAccountDataEventType::SecretStorageKey(key_id.clone());
        match account.fetch_account_data(event_type).await {
            Ok(Some(raw)) => {
                let json = raw.json().get();
                let has_passphrase =
                    json.contains("\"passphrase\"") && json.contains("\"iterations\"");
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                    &format!(
                        "matrix: secret-storage diagnostics: default_key_id={key_id}, \
                     has_passphrase_info={has_passphrase}, input_len={input_len}. \
                     {}",
                        if has_passphrase {
                            "SDK will try passphrase derivation first; if your input is a base58 key the passphrase MAC will fail and the error you see may be the passphrase error rather than the base58 fallback's error."
                        } else {
                            "SDK will use base58 decoding directly."
                        }
                    )
                );
                let _ = SecretStorageKeyEventContent::TYPE; // keep import live
            }
            Ok(None) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"key_id": key_id})),
                    "matrix: default key id has no corresponding key event on the account — secret storage is in an inconsistent state. Re-running Secure Backup setup in Element will repair this."
                );
            }
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"error": format!("{}", e), "key_id": key_id})
                        ),
                    "matrix: failed to fetch key event for"
                );
            }
        }
    }

    pub(super) fn normalize_recipient(id_or_alias: &str) -> (&str, bool) {
        if !id_or_alias.contains("||") {
            return (id_or_alias, false);
        }
        let chosen = id_or_alias
            .split("||")
            .map(str::trim)
            .filter(|s| s.starts_with('!') || s.starts_with('#'))
            .last()
            .unwrap_or(id_or_alias);
        (chosen, true)
    }

    pub(super) async fn resolve_room(
        client: &Client,
        cache: &Arc<RwLock<HashMap<String, OwnedRoomId>>>,
        id_or_alias: &str,
    ) -> Result<OwnedRoomId> {
        let (id_or_alias, normalized) = normalize_recipient(id_or_alias);
        if normalized {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"id_or_alias": id_or_alias})),
                "matrix: recipient contains `||`; using as the room target. Update channels.matrix or cron `delivery.to` to a plain room id/alias to silence this warning."
            );
        }
        if id_or_alias.starts_with('!') {
            return id_or_alias
                .parse::<matrix_sdk::ruma::OwnedRoomId>()
                .with_context(|| format!("parse room id {id_or_alias}"));
        }
        if !id_or_alias.starts_with('#') {
            bail!("matrix: not a room id or alias: {id_or_alias}");
        }
        if let Some(id) = cache.read().await.get(id_or_alias) {
            return Ok(id.clone());
        }
        let alias: &RoomAliasId = id_or_alias
            .try_into()
            .with_context(|| format!("parse room alias {id_or_alias}"))?;
        let resp = client
            .resolve_room_alias(alias)
            .await
            .with_context(|| format!("resolve room alias {id_or_alias}"))?;
        cache
            .write()
            .await
            .insert(id_or_alias.to_string(), resp.room_id.clone());
        Ok(resp.room_id)
    }
}

// ─── inbound ───────────────────────────────────────────────────────────────
mod inbound {
    use std::{
        collections::{HashMap, HashSet},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, SystemTime},
    };

    use matrix_sdk::{
        Client, Room, RoomState,
        config::SyncSettings,
        event_handler::RawEvent,
        ruma::{
            OwnedEventId, OwnedUserId,
            events::{
                AnySyncTimelineEvent,
                reaction::ReactionEventContent,
                relation::Annotation,
                room::{
                    encrypted::OriginalSyncRoomEncryptedEvent,
                    message::{MessageType, OriginalSyncRoomMessageEvent},
                },
            },
            serde::Raw,
        },
    };
    use serde_json::Value as JsonValue;
    use tokio::sync::{Mutex as TokioMutex, RwLock as TokioRwLock, mpsc};

    use super::{allowlist, approval, context as ctx_mod, mention};
    use clawcrew_api::{channel::ChannelMessage, media::MediaAttachment};
    use clawcrew_config::schema::MatrixConfig;

    pub(super) const SYNC_LONGPOLL_TIMEOUT: Duration = Duration::from_secs(30);

    #[derive(Clone)]
    pub(super) struct HandlerCtx {
        pub config: Arc<MatrixConfig>,
        /// ClawCrew alias for `[channels.matrix.<alias>]` so session_key
        /// construction can scope by bot instance.
        pub alias: String,
        /// Resolves inbound external peers from canonical state at message-time.
        /// No cache (see AGENTS.md "ABSOLUTE RULE — SINGLE SOURCE OF TRUTH").
        pub peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
        pub transcription: Option<super::TranscriptionResolver>,
        pub workspace_dir: Option<Arc<std::path::PathBuf>>,
        pub tx: mpsc::Sender<ChannelMessage>,
        pub pending_approvals: Arc<TokioMutex<HashMap<String, crate::util::PendingApproval>>>,
        pub threads_seen: Arc<TokioRwLock<HashSet<OwnedEventId>>>,
        pub bot_user_id: OwnedUserId,
        pub bot_display_name: Arc<TokioRwLock<Option<String>>>,
        pub initial_sync_done: Arc<AtomicBool>,
        /// Event ids of inbound events that arrived as `m.room.encrypted` and
        /// could not be decrypted. Tracked so the bot reacts ❓ exactly once
        /// per event across sync catchup deliveries.
        pub undecryptable_seen: Arc<TokioMutex<HashSet<OwnedEventId>>>,
    }

    /// Register the inbound event handlers on `client`. The returned guards
    /// keep the handlers alive; drop them to deregister.
    pub(super) fn register_event_handlers(
        client: &Client,
        ctx: &HandlerCtx,
    ) -> (
        matrix_sdk::event_handler::EventHandlerDropGuard,
        matrix_sdk::event_handler::EventHandlerDropGuard,
    ) {
        let handler_ctx = ctx.clone();
        let message_handler = client.add_event_handler(
            move |ev: OriginalSyncRoomMessageEvent, room: Room, raw: RawEvent| {
                let ctx = handler_ctx.clone();
                async move {
                    if let Err(e) = handle_message(ctx, ev, room, raw).await {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                            "matrix: handle_message failed"
                        );
                    }
                }
            },
        );
        let message_guard = client.event_handler_drop_guard(message_handler);

        // Surface inbound events the SDK couldn't decrypt by reacting ❓ on
        // the encrypted event so the operator notices a key gap in chat
        // instead of silent dropping. Best-effort: prophylactic in normally-
        // healthy rooms where decryption succeeds.
        let encrypted_ctx = ctx.clone();
        let encrypted_handler =
            client.add_event_handler(move |ev: OriginalSyncRoomEncryptedEvent, room: Room| {
                let ctx = encrypted_ctx.clone();
                async move {
                    handle_undecryptable(ctx, ev, room).await;
                }
            });
        let encrypted_guard = client.event_handler_drop_guard(encrypted_handler);

        (message_guard, encrypted_guard)
    }

    pub(super) async fn run_sync_loop(client: Client, ctx: HandlerCtx) -> anyhow::Result<()> {
        let (_message_handler_guard, _encrypted_handler_guard) =
            register_event_handlers(&client, &ctx);

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            "matrix: starting sync loop"
        );
        let sync_settings = SyncSettings::default().timeout(SYNC_LONGPOLL_TIMEOUT);
        if let Err(e) = client.sync_once(sync_settings.clone()).await {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "phase": "initial_sync",
                        "error": format!("{}", e),
                    })),
                "matrix: initial sync failed"
            );
            return Err(anyhow::Error::msg(format!(
                "matrix initial sync failed: {e}"
            )));
        }
        ctx.initial_sync_done.store(true, Ordering::SeqCst);
        client.sync(sync_settings).await.map_err(|e| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "phase": "sync_loop",
                        "error": format!("{}", e),
                    })),
                "matrix: sync loop failed"
            );
            anyhow::Error::msg(format!("matrix sync loop failed: {e}"))
        })
    }

    /// React ❓ on any inbound event the SDK delivered as still-encrypted
    /// (decryption failed or no keys available). Skips the bot's own
    /// events, non-Joined rooms, and any event already reacted to in this
    /// process. Reaction send failures are warn-logged, not propagated.
    async fn handle_undecryptable(ctx: HandlerCtx, ev: OriginalSyncRoomEncryptedEvent, room: Room) {
        if room.state() != RoomState::Joined {
            return;
        }
        if ev.sender == ctx.bot_user_id {
            return;
        }
        let event_id = ev.event_id.clone();
        let already = {
            let mut seen = ctx.undecryptable_seen.lock().await;
            !seen.insert(event_id.clone())
        };
        if already {
            return;
        }
        ::clawcrew_log::record!(
            DEBUG,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            &format!(
                "matrix: reacting ❓ to undecryptable event {} from {}",
                event_id, ev.sender
            )
        );
        let content =
            ReactionEventContent::new(Annotation::new(event_id.clone(), "❓".to_string()));
        if let Err(e) = room.send(content).await {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(
                        ::serde_json::json!({"error": format!("{}", e), "event_id": event_id})
                    ),
                "matrix: failed to react ❓ on undecryptable event"
            );
        }
    }

    async fn handle_message(
        ctx: HandlerCtx,
        ev: OriginalSyncRoomMessageEvent,
        room: Room,
        raw: RawEvent,
    ) -> anyhow::Result<()> {
        if room.state() != RoomState::Joined {
            return Ok(());
        }
        if ev.sender == ctx.bot_user_id {
            return Ok(());
        }

        let body = ctx_mod::body_for(&ev.content.msgtype);
        let sender = ev.sender.as_str();
        let room_id = room.room_id().as_str();
        let allowed_peers = (ctx.peer_resolver)();
        let sender_allowed = allowlist::user_allowed(&allowed_peers, sender);
        let room_allowed = allowlist::room_allowed_static(&ctx.config.allowed_rooms, room_id);

        if let Some((token, response)) = approval::parse_reply(&body)
            && crate::util::resolve_pending_approval(
                &ctx.pending_approvals,
                &token,
                response,
                sender_allowed && room_allowed,
                room_id,
            )
            .await
            .suppresses_message()
        {
            return Ok(());
        }

        if !sender_allowed {
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({"sender": sender})),
                "matrix: drop message from non-allowed sender"
            );
            return Ok(());
        }
        if !room_allowed {
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({"room_id": room_id})),
                "matrix: drop message from non-allowed room"
            );
            return Ok(());
        }

        // Fetch the reply parent at most once for the mention_only gate and
        // the parent-media path below (Room::event always hits the homeserver).
        let reply_target = extract_in_reply_to(&raw);
        let mut cached_reply_parent = None;

        if ctx.config.mention_only && is_group_room(&room).await {
            let display_name = ctx.bot_display_name.read().await.clone();
            let mention_user_ids = extract_mentions_user_ids(&raw);
            let mentioned = mention::is_mentioned(
                &ctx.bot_user_id,
                display_name.as_deref(),
                mention_user_ids.as_deref(),
                &body,
            );
            // Reply-to-bot bypasses the mention gate (Telegram parity). Fetch the
            // parent only when the body/mention list alone would drop the turn.
            let reply_to_bot = if mentioned {
                false
            } else if let Some(reply_id) = reply_target.as_ref() {
                match room.event(reply_id, None).await {
                    Ok(timeline_event) => {
                        let is_bot = timeline_event
                            .sender()
                            .as_ref()
                            .is_some_and(|sender| sender == &ctx.bot_user_id);
                        cached_reply_parent = Some(timeline_event);
                        is_bot
                    }
                    Err(e) => {
                        ::clawcrew_log::record!(
                            DEBUG,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_attrs(::serde_json::json!({
                                "error": format!("{e}"),
                                "reply_id": reply_id,
                            })),
                            "matrix: failed to fetch reply parent for mention_only gate"
                        );
                        false
                    }
                }
            } else {
                false
            };
            if !mention::admit_group_message(mentioned, reply_to_bot) {
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({"sender": sender})),
                    "matrix: drop unmentioned message from"
                );
                return Ok(());
            }
        }

        let thread_id = extract_thread_id(&raw);
        let mut content = body.clone();
        if let Some(tid) = thread_id.as_ref()
            && ctx_mod::claim_first_visit(&ctx.threads_seen, tid).await
        {
            match room.event(tid, None).await {
                Ok(timeline_event) => {
                    if let Some((root_sender, root_body)) =
                        extract_root_summary(timeline_event.into_raw())
                    {
                        content = format!(
                            "{}{}",
                            ctx_mod::format_preamble(&root_sender, &root_body),
                            content
                        );
                    }
                }
                Err(e) => ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": format!("{}", e), "tid": tid})),
                    "matrix: failed to fetch thread root"
                ),
            }
        }

        let media_kind = match &ev.content.msgtype {
            MessageType::Image(m) => Some(MediaInfo::new(
                m.source.clone(),
                m.body.clone(),
                m.info.as_ref().and_then(|i| i.mimetype.clone()),
                MediaCategory::Image,
            )),
            MessageType::File(m) => Some(MediaInfo::new(
                m.source.clone(),
                m.body.clone(),
                m.info.as_ref().and_then(|i| i.mimetype.clone()),
                MediaCategory::File,
            )),
            MessageType::Video(m) => Some(MediaInfo::new(
                m.source.clone(),
                m.body.clone(),
                m.info.as_ref().and_then(|i| i.mimetype.clone()),
                MediaCategory::Video,
            )),
            MessageType::Audio(m) => {
                let kind = if is_voice_message(&raw) {
                    MediaCategory::Voice
                } else {
                    MediaCategory::Audio
                };
                let mime = m.info.as_ref().and_then(|i| i.mimetype.clone());
                let file_name = m
                    .filename
                    .as_deref()
                    .filter(|f| !f.is_empty())
                    .unwrap_or(&m.body)
                    .to_string();
                Some(MediaInfo::new(m.source.clone(), file_name, mime, kind))
            }
            _ => None,
        };
        // Read off this event alone: a reply whose parent is a voice note
        // stays text-origin.
        let voice_origin = media_kind
            .as_ref()
            .is_some_and(|info| matches!(info.kind, MediaCategory::Voice));

        if let Some(info) = media_kind {
            content = attach_media(
                &room,
                &info,
                ctx.workspace_dir.as_deref(),
                &body,
                content,
                ctx.transcription.as_ref(),
            )
            .await;
        } else if let Some(reply_id) = reply_target.as_ref() {
            let parent = if let Some(cached) = cached_reply_parent.take() {
                Ok(cached)
            } else {
                room.event(reply_id, None).await
            };
            match parent {
                Ok(timeline_event) => {
                    if let Some(info) = parent_media_info(timeline_event.raw().clone()) {
                        content = attach_media(
                            &room,
                            &info,
                            ctx.workspace_dir.as_deref(),
                            "",
                            content,
                            ctx.transcription.as_ref(),
                        )
                        .await;
                    }
                }
                Err(e) => {
                    ::clawcrew_log::record!(DEBUG, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"error": format!("{}", e), "reply_target": reply_id})), "matrix: could not fetch in_reply_to parent")
                }
            }
        }
        let attachments: Vec<MediaAttachment> = Vec::new();

        let outbound_anchor =
            resolve_outbound_anchor(thread_id.as_ref(), &ev.event_id, ctx.config.reply_in_thread);
        // When the bot is the one starting the thread, mark its root seen
        // so the next inbound that lands inside it does not re-fetch and
        // re-inject a root preamble (the agent already saw the root in this
        // same turn).
        if thread_id.is_none() && ctx.config.reply_in_thread {
            ctx_mod::mark_seen(&ctx.threads_seen, ev.event_id.clone()).await;
        }

        let interruption_scope =
            interruption_scope_from_anchor(outbound_anchor.as_deref(), &ev.event_id);

        let msg = ChannelMessage {
            id: ev.event_id.to_string(),
            sender: sender.to_string(),
            reply_target: room.room_id().to_string(),
            content,
            channel: "matrix".to_string(),
            channel_alias: Some(ctx.alias.clone()),
            timestamp: SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: outbound_anchor.clone(),
            interruption_scope_id: interruption_scope,
            attachments,
            subject: None,
            voice_origin,
            ..Default::default()
        };

        if let Err(e) = ctx.tx.send(msg).await {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                "matrix: failed to forward inbound message"
            );
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) async fn handle_message_for_test(
        ctx: HandlerCtx,
        ev: OriginalSyncRoomMessageEvent,
        room: Room,
        raw: RawEvent,
    ) -> anyhow::Result<()> {
        handle_message(ctx, ev, room, raw).await
    }

    async fn is_group_room(room: &Room) -> bool {
        !matches!(room.is_direct().await, Ok(true))
    }

    pub(super) fn extract_mentions_user_ids(raw: &RawEvent) -> Option<Vec<String>> {
        let v: JsonValue = serde_json::from_str(raw.get()).ok()?;
        let mentions = v.get("content")?.get("m.mentions")?;
        let arr = mentions.get("user_ids")?.as_array()?;
        Some(
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect(),
        )
    }

    pub(super) fn resolve_outbound_anchor(
        thread_id: Option<&OwnedEventId>,
        event_id: &OwnedEventId,
        reply_in_thread: bool,
    ) -> Option<String> {
        thread_id.map(ToString::to_string).or_else(|| {
            if reply_in_thread {
                Some(event_id.to_string())
            } else {
                None
            }
        })
    }

    pub(super) fn interruption_scope_from_anchor(
        outbound_anchor: Option<&str>,
        event_id: &OwnedEventId,
    ) -> Option<String> {
        match outbound_anchor {
            Some(anchor) if anchor == event_id.as_str() => None,
            other => other.map(ToString::to_string),
        }
    }

    pub(super) fn extract_thread_id(raw: &RawEvent) -> Option<OwnedEventId> {
        let v: JsonValue = serde_json::from_str(raw.get()).ok()?;
        let relates = v.get("content")?.get("m.relates_to")?;
        let rel_type = relates.get("rel_type")?.as_str()?;
        if rel_type != "m.thread" {
            return None;
        }
        let root = relates.get("event_id")?.as_str()?;
        root.parse().ok()
    }

    pub(super) fn extract_in_reply_to(raw: &RawEvent) -> Option<OwnedEventId> {
        let v: JsonValue = serde_json::from_str(raw.get()).ok()?;
        let relates = v.get("content")?.get("m.relates_to")?;
        let in_reply_to = relates.get("m.in_reply_to")?;
        let event_id = in_reply_to.get("event_id")?.as_str()?;
        event_id.parse().ok()
    }

    pub(super) fn is_voice_message(raw: &RawEvent) -> bool {
        let v: JsonValue = match serde_json::from_str(raw.get()) {
            Ok(v) => v,
            Err(_) => return false,
        };
        v.get("content")
            .and_then(|c| c.get("org.matrix.msc3245.voice"))
            .is_some()
    }

    fn extract_root_summary(raw: Raw<AnySyncTimelineEvent>) -> Option<(String, String)> {
        let json: JsonValue = serde_json::from_str(raw.json().get()).ok()?;
        let sender = json.get("sender")?.as_str()?.to_string();
        let body = json
            .get("content")
            .and_then(|c| c.get("body"))
            .and_then(|b| b.as_str())
            .unwrap_or("")
            .to_string();
        Some((sender, body))
    }

    pub(super) enum MediaCategory {
        Image,
        Video,
        Audio,
        Voice,
        File,
    }

    /// Voice notes (MSC3245) are the only inbound media ClawCrew transcribes.
    /// Whether transcription is enabled at all is owned by the channel's
    /// transcription resolver, which reads it from live config.
    pub(super) fn should_transcribe(kind: &MediaCategory) -> bool {
        matches!(kind, MediaCategory::Voice)
    }

    async fn attach_media(
        room: &Room,
        info: &MediaInfo,
        workspace_dir: Option<&std::path::PathBuf>,
        body_hint: &str,
        content: String,
        transcription: Option<&super::TranscriptionResolver>,
    ) -> String {
        let mut content = content;
        match save_media_to_workspace(room, info, workspace_dir).await {
            Ok(Some(path)) => {
                let marker = format_media_marker(info, &path);
                let placeholder = matches!(body_hint, "[image]" | "[file]" | "[audio]" | "[video]");
                content = if body_hint.is_empty() {
                    if content.is_empty() {
                        marker
                    } else {
                        format!("{content}\n\n{marker}")
                    }
                } else if placeholder || body_hint == info.file_name || content == body_hint {
                    marker
                } else {
                    format!("{content}\n\n{marker}")
                };

                if should_transcribe(&info.kind)
                    && let Some(resolver) = transcription
                {
                    let transcribe_name =
                        transcription_safe_filename(&info.file_name, info.mime.as_deref());
                    match transcribe_from_disk(resolver, &path, &transcribe_name).await {
                        Ok(Some(text)) if !text.trim().is_empty() => {
                            content = format!("[voice transcript]: {text}\n\n{content}");
                        }
                        Ok(_) => {}
                        Err(e) => ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                            "matrix: voice transcription failed"
                        ),
                    }
                }
            }
            Ok(None) => {}
            Err(e) => ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                "matrix: media handling failed"
            ),
        }
        content
    }

    /// Walk a fetched timeline event's raw JSON looking for a media-typed
    /// `m.room.message` payload. Returns `None` if the event is not a
    /// recognized media message.
    pub(super) fn parent_media_info(
        raw: matrix_sdk::ruma::serde::Raw<matrix_sdk::ruma::events::AnySyncTimelineEvent>,
    ) -> Option<MediaInfo> {
        let json: JsonValue = serde_json::from_str(raw.json().get()).ok()?;
        let content = json.get("content")?;
        let msgtype = content.get("msgtype")?.as_str()?;
        let kind = match msgtype {
            "m.image" => MediaCategory::Image,
            "m.video" => MediaCategory::Video,
            "m.audio" if content.get("org.matrix.msc3245.voice").is_some() => MediaCategory::Voice,
            "m.audio" => MediaCategory::Audio,
            "m.file" => MediaCategory::File,
            _ => return None,
        };
        let body_str = content
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or("attachment");
        let filename_field = content.get("filename").and_then(|f| f.as_str());
        let mime = content
            .get("info")
            .and_then(|i| i.get("mimetype"))
            .and_then(|m| m.as_str());
        let file_name = filename_field
            .filter(|f| !f.is_empty())
            .unwrap_or(body_str)
            .to_string();
        let mime = mime.map(String::from);
        let source = if let Some(file) = content.get("file") {
            // Encrypted media: rebuild MediaSource::Encrypted from JSON.
            let encrypted: matrix_sdk::ruma::events::room::EncryptedFile =
                serde_json::from_value(file.clone()).ok()?;
            matrix_sdk::ruma::events::room::MediaSource::Encrypted(Box::new(encrypted))
        } else {
            let url = content.get("url").and_then(|u| u.as_str())?;
            matrix_sdk::ruma::events::room::MediaSource::Plain(matrix_sdk::ruma::OwnedMxcUri::from(
                url,
            ))
        };
        Some(MediaInfo::new(source, file_name, mime, kind))
    }

    pub(super) struct MediaInfo {
        pub source: matrix_sdk::ruma::events::room::MediaSource,
        pub file_name: String,
        pub mime: Option<String>,
        pub kind: MediaCategory,
    }

    impl MediaInfo {
        pub fn new(
            source: matrix_sdk::ruma::events::room::MediaSource,
            file_name: String,
            mime: Option<String>,
            kind: MediaCategory,
        ) -> Self {
            Self {
                source,
                file_name,
                mime,
                kind,
            }
        }
    }

    /// Download an inbound media file, persist it to `{workspace}/matrix_files/`,
    /// and return the on-disk path. Returns `Ok(None)` when no `workspace_dir`
    /// is configured (caller logs and falls back to the placeholder body).
    async fn save_media_to_workspace(
        room: &Room,
        info: &MediaInfo,
        workspace: Option<&std::path::PathBuf>,
    ) -> anyhow::Result<Option<std::path::PathBuf>> {
        let Some(workspace) = workspace else {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                &format!(
                    "matrix: cannot persist {} — channels.matrix workspace_dir not configured. Set CLAWCREW_DIR or run via the orchestrator.",
                    info.file_name
                )
            );
            return Ok(None);
        };
        let dir = workspace.join("matrix_files");
        std::fs::create_dir_all(&dir).map_err(|e| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "path": dir.display().to_string(),
                        "phase": "media_dir_create",
                        "error": format!("{}", e),
                    })),
                "matrix: failed to create media dir"
            );
            anyhow::Error::msg(format!("create {}: {e}", dir.display()))
        })?;
        let request = matrix_sdk::media::MediaRequestParameters {
            source: info.source.clone(),
            format: matrix_sdk::media::MediaFormat::File,
        };
        let source_kind = match &info.source {
            matrix_sdk::ruma::events::room::MediaSource::Plain(_) => "plain",
            matrix_sdk::ruma::events::room::MediaSource::Encrypted(_) => "encrypted",
        };
        let bytes = room
            .client()
            .media()
            .get_media_content(&request, true)
            .await
            .map_err(|e| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                    "get_media_content ()"
                );
                anyhow::Error::msg(format!("get_media_content ({source_kind}): {e}"))
            })?;

        let safe_name = sanitize_filename(&info.file_name, &info.kind, info.mime.as_deref());
        // Disambiguate by uuid prefix to avoid collisions across messages.
        let unique = format!("{}_{safe_name}", uuid::Uuid::new_v4().simple());
        let path = dir.join(unique);
        std::fs::write(&path, &bytes).map_err(|e| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "path": path.display().to_string(),
                        "phase": "media_write",
                        "error": format!("{}", e),
                    })),
                "matrix: failed to write media file"
            );
            anyhow::Error::msg(format!("write {}: {e}", path.display()))
        })?;
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            &format!(
                "matrix: saved {} bytes ({}) to {}",
                bytes.len(),
                source_kind,
                path.display()
            )
        );
        Ok(Some(path))
    }

    fn sanitize_filename(raw: &str, kind: &MediaCategory, mime: Option<&str>) -> String {
        let trimmed = raw.trim();
        let candidate = if trimmed.is_empty() || trimmed.starts_with('[') {
            // Placeholder body or empty — synthesise a sensible name.
            let ext = default_extension(kind, mime);
            format!("matrix_media.{ext}")
        } else {
            trimmed.to_string()
        };
        candidate
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn default_extension(kind: &MediaCategory, mime: Option<&str>) -> &'static str {
        if let Some(m) = mime {
            // Audio MIMEs resolve through the canonical transcription-side
            // mapping; non-audio types are display/on-disk naming only.
            if let Some(ext) = crate::transcription::extension_for_audio_mime(m) {
                return ext;
            }
            match m.split(';').next().unwrap_or(m).trim() {
                "image/png" => return "png",
                "image/jpeg" | "image/jpg" => return "jpg",
                "image/gif" => return "gif",
                "image/webp" => return "webp",
                "video/mp4" => return "mp4",
                "application/pdf" => return "pdf",
                _ => {}
            }
        }
        match kind {
            MediaCategory::Image => "jpg",
            MediaCategory::Video => "mp4",
            MediaCategory::Audio | MediaCategory::Voice => "ogg",
            MediaCategory::File => "bin",
        }
    }

    /// Internal name for `transcribe()` only — never stored or shown.
    /// Preserves a name whose extension the resolver accepts; substitutes a
    /// MIME-derived stand-in only when the MIME maps to an accepted format;
    /// otherwise passes the unsupported name through so the resolver rejects
    /// it locally before any request is sent.
    pub(super) fn transcription_safe_filename(file_name: &str, mime: Option<&str>) -> String {
        let ext_accepted = file_name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| crate::transcription::mime_for_audio(ext).is_some());
        if ext_accepted {
            return file_name.to_string();
        }
        match mime.and_then(crate::transcription::extension_for_audio_mime) {
            Some(ext) => format!("attachment.{ext}"),
            None => file_name.to_string(),
        }
    }

    fn format_media_marker(info: &MediaInfo, path: &std::path::Path) -> String {
        match info.kind {
            MediaCategory::Image => format!("[IMAGE:{}]", path.display()),
            _ => {
                let display_name = if info.file_name.trim().is_empty() {
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("attachment")
                        .to_string()
                } else {
                    info.file_name.clone()
                };
                format!("[Document: {display_name}] {}", path.display())
            }
        }
    }

    /// Resolves the manager from live config before touching the filesystem.
    /// `Ok(None)` means transcription is currently disabled.
    async fn transcribe_from_disk(
        resolver: &super::TranscriptionResolver,
        path: &std::path::Path,
        file_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let Some(manager) = resolver() else {
            return Ok(None);
        };
        let manager = manager?;
        let bytes = std::fs::read(path).map_err(|e| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "path": path.display().to_string(),
                        "phase": "transcription_read",
                        "error": format!("{}", e),
                    })),
                "matrix: failed to read media file for transcription"
            );
            anyhow::Error::msg(format!("read {}: {e}", path.display()))
        })?;
        manager.transcribe(&bytes, file_name).await.map(Some)
    }
}

/// Registers every configured provider — legacy `[transcription]` and typed
/// `[providers.transcription.<type>.<alias>]` alike — and binds
/// `agent_provider`.
///
/// When the owning agent states no preference, a lone registered provider is
/// bound so single-provider deployments keep working without an explicit
/// `transcription_provider`.
pub(crate) fn build_transcription_manager(
    config: &clawcrew_config::schema::Config,
    agent_provider: &str,
) -> anyhow::Result<crate::transcription::TranscriptionManager> {
    crate::transcription::build_channel_transcription_manager(config, agent_provider)
}

/// Resolves transcription state from live config at message time. `None` means
/// transcription is currently disabled; the inner `Result` carries provider
/// registration failures.
///
/// Held as a closure rather than a config snapshot so reloadable provider
/// policy is never copied into this long-lived channel handle
/// (see AGENTS.md "Single Source Of Truth").
/// Resolves the TTS manager from live config at send time. `None` means TTS is
/// disabled or the owning agent has no `tts_provider`; the inner `Result`
/// carries provider registration failures.
///
/// A closure rather than a snapshot, so reloadable provider policy is never
/// copied into this long-lived channel handle
/// (see AGENTS.md "Single Source Of Truth").
pub(crate) type TtsResolver =
    Arc<dyn Fn() -> Option<anyhow::Result<crate::tts::TtsManager>> + Send + Sync>;

pub(crate) type TranscriptionResolver = Arc<
    dyn Fn() -> Option<anyhow::Result<crate::transcription::TranscriptionManager>> + Send + Sync,
>;

/// Resolver over a legacy `[transcription]` section alone. Typed
/// `[providers.transcription.<type>.<alias>]` entries are unreachable this way,
/// so production always goes through the channel runtime's live-config
/// resolver; this exists to drive the inbound tests from a bare section.
#[cfg(test)]
pub(crate) fn legacy_transcription_resolver(
    transcription: clawcrew_config::schema::TranscriptionConfig,
) -> TranscriptionResolver {
    let config = Arc::new(clawcrew_config::schema::Config {
        transcription,
        ..Default::default()
    });
    Arc::new(move || {
        if !config.transcription.enabled {
            return None;
        }
        Some(build_transcription_manager(&config, ""))
    })
}

// ─── outbound ──────────────────────────────────────────────────────────────
mod outbound {
    use std::{collections::HashMap, sync::Arc};

    use anyhow::{Context as _, Result, bail};
    use futures_util::StreamExt;
    use matrix_sdk::{
        Client, Room, RoomState,
        attachment::{
            AttachmentConfig, AttachmentInfo, BaseAudioInfo, BaseFileInfo, BaseImageInfo,
            BaseVideoInfo,
        },
        room::{
            edit::EditedContent,
            reply::{EnforceThread, Reply},
        },
        ruma::{
            OwnedEventId, OwnedRoomId, UInt,
            events::{
                reaction::ReactionEventContent,
                relation::Annotation,
                room::message::{
                    AddMentions, MessageType, ReplacementMetadata, ReplyWithinThread,
                    RoomMessageEventContent, RoomMessageEventContentWithoutRelation,
                    TextMessageEventContent,
                },
            },
        },
    };
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::time::Duration;
    use tokio::sync::{Mutex as TokioMutex, RwLock as TokioRwLock};

    use super::{client, context as ctx_mod, markers};
    use clawcrew_api::{channel::SendMessage, media::MediaAttachment};

    pub(super) type ReactionKey = (OwnedRoomId, OwnedEventId, String);

    pub(super) struct Outbox<'a> {
        pub client: &'a Client,
        pub alias_cache: &'a Arc<TokioRwLock<HashMap<String, OwnedRoomId>>>,
        pub threads_seen: &'a Arc<TokioRwLock<std::collections::HashSet<OwnedEventId>>>,
        pub reaction_log: &'a Arc<TokioMutex<HashMap<ReactionKey, OwnedEventId>>>,
        pub reply_in_thread: bool,
        pub workspace_dir: Option<&'a Path>,
        /// Resolved from the canonical Matrix channel config for this send.
        /// Only single-message mode needs a final-response body budget.
        pub message_max_bytes: Option<usize>,
    }

    /// What `outbound::send` should do once all attachment uploads are done
    /// and the marker-stripped text is in hand. Extracted as a small enum so
    /// the empty-text-with-attachments contract can be unit-tested without
    /// the SDK in the loop.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum SendOutcome {
        /// Text is non-empty (with or without prior attachments). Caller
        /// proceeds to send the text message and returns its event_id.
        SendText,
        /// Text is empty but at least one attachment uploaded successfully.
        /// Caller skips the text send and returns the carried event_id.
        ReturnAttachment,
        /// Text is empty AND no attachment landed. Caller surfaces an error
        /// to the runtime so it can decide what to do.
        EmptyError,
    }

    fn prefix_utf8_bytes(text: &str, max_bytes: usize) -> &str {
        if text.len() <= max_bytes {
            return text;
        }
        let mut end = max_bytes.min(text.len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    }

    fn next_prefix_after_oversize(text: &str, actual: usize, max: usize) -> &str {
        let first_scalar_len = text.chars().next().map_or(0, char::len_utf8);
        let proportional = text.len().saturating_mul(max) / actual.max(1);
        let target = proportional.clamp(first_scalar_len, text.len().saturating_sub(1));
        prefix_utf8_bytes(text, target)
    }

    fn bounded_rendered_body<F>(text: &str, message_max_bytes: Option<usize>, render: F) -> String
    where
        F: Fn(&str) -> usize,
    {
        let Some(max_bytes) = message_max_bytes else {
            return text.to_string();
        };
        let mut candidate = text;
        loop {
            let actual = render(candidate);
            if actual <= max_bytes {
                return candidate.to_string();
            }
            let next = next_prefix_after_oversize(candidate, actual, max_bytes);
            // `MATRIX_MIN_MESSAGE_MAX_BYTES` guarantees a one-scalar Matrix
            // event fits. This guard also prevents an accidental infinite loop
            // if a future renderer violates that contract.
            if next.len() == candidate.len() {
                return candidate.to_string();
            }
            candidate = next;
        }
    }

    /// Apply an explicitly selected Matrix response budget after markers and
    /// attachments have been processed. The limit is checked against the
    /// serialized Markdown event, not merely the Markdown source; a large
    /// `formatted_body` must not escape the configured budget.
    pub(super) fn bounded_body(text: &str, message_max_bytes: Option<usize>) -> String {
        bounded_rendered_body(text, message_max_bytes, |candidate| {
            serde_json::to_vec(&RoomMessageEventContent::text_markdown(candidate))
                .map_or(usize::MAX, |serialized| serialized.len())
        })
    }

    /// Apply the same serialized-content budget to Matrix edits. Replacement
    /// events duplicate the rendered new content and add `m.relates_to`, so
    /// their ceiling is checked independently from a plain send.
    #[cfg(test)]
    pub(super) fn bounded_edit_body(
        text: &str,
        event_id: &OwnedEventId,
        message_max_bytes: Option<usize>,
    ) -> String {
        bounded_rendered_body(text, message_max_bytes, |candidate| {
            serialized_edit_content_len(candidate, event_id).unwrap_or(usize::MAX)
        })
    }

    pub(super) fn serialized_edit_content_len(
        text: &str,
        event_id: &OwnedEventId,
    ) -> Option<usize> {
        let new_content = RoomMessageEventContentWithoutRelation::new(MessageType::Text(
            TextMessageEventContent::markdown(text),
        ));
        serde_json::to_vec(
            &new_content.make_replacement(ReplacementMetadata::new(event_id.clone(), None)),
        )
        .ok()
        .map(|serialized| serialized.len())
    }

    /// Decide what `outbound::send` should do given the post-marker-strip
    /// text and whether at least one attachment landed. Pure function.
    pub(super) fn decide_send_outcome(
        text_is_empty_after_strip: bool,
        any_attachment_landed: bool,
    ) -> SendOutcome {
        match (text_is_empty_after_strip, any_attachment_landed) {
            (false, _) => SendOutcome::SendText,
            (true, true) => SendOutcome::ReturnAttachment,
            (true, false) => SendOutcome::EmptyError,
        }
    }

    /// Why a marker upload didn't reach the room. Drives both the textual
    /// "(note: I couldn't deliver…)" line and the emoji reactions on the
    /// agent's outgoing message so a chatter sees a hard refusal at a glance.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum MarkerFailure {
        /// Trust-boundary refusal: `validate_marker_target` rejected the
        /// target (path escapes workspace, disallowed scheme, etc.). The bot
        /// deliberately did not attempt the fetch.
        Refused,
        /// Post-validation failure: fetch error, file not found, upload
        /// rejected by the server, oversize body, timeout. The bot tried and
        /// couldn't complete the delivery.
        Failed,
    }

    pub(super) struct AttachmentDelivery {
        pub text: String,
        pub last_attachment_id: Option<OwnedEventId>,
        pub failed_markers: Vec<(String, MarkerFailure)>,
    }

    impl AttachmentDelivery {
        pub(super) fn failure_kinds(&self) -> Vec<MarkerFailure> {
            self.failed_markers.iter().map(|(_, kind)| *kind).collect()
        }
    }

    /// Pick the emoji reactions to apply to the agent's outgoing text/event
    /// based on which kinds of marker failures occurred. 🚫 means the bot
    /// refused for safety; ⚠️ means it tried and didn't make it. Both can
    /// fire on the same message when a batch mixes refusals and failures.
    pub(super) fn decide_reactions(failures: &[MarkerFailure]) -> Vec<&'static str> {
        let mut out = Vec::new();
        if failures.iter().any(|f| matches!(f, MarkerFailure::Refused)) {
            out.push("🚫");
        }
        if failures.iter().any(|f| matches!(f, MarkerFailure::Failed)) {
            out.push("⚠️");
        }
        out
    }

    /// 8 MiB cap on the body of an HTTP marker fetch. Matches WebFetchTool's
    /// streaming-cap pattern in `crates/clawcrew-tools/src/web_fetch.rs`.
    const MAX_MARKER_BYTES: usize = 8 * 1024 * 1024;
    /// 30-second connect+request timeout for HTTP marker fetches. Bounds the
    /// agent-driven fetch path so a hung target cannot stall the channel.
    const MARKER_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

    /// Resolved marker fetch target after sandboxing. `Local` paths are
    /// canonicalised and proven to live within the configured `workspace_dir`.
    /// `Http` URLs have an explicit `http`/`https` scheme.
    #[derive(Debug)]
    pub(super) enum MarkerTarget {
        Local(PathBuf),
        Http(reqwest::Url),
    }

    #[derive(Debug)]
    pub(super) enum ValidateError {
        /// Trust-boundary refusal: disallowed scheme, no workspace
        /// configured, or path resolved outside the workspace. The target
        /// was a real, reachable resource that policy declined.
        Refused(anyhow::Error),
        /// The path didn't resolve to anything on disk (ENOENT or similar
        /// during canonicalize). Treated as a delivery failure, not a
        /// safety event.
        NotFound(anyhow::Error),
    }

    impl std::fmt::Display for ValidateError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                ValidateError::Refused(e) | ValidateError::NotFound(e) => write!(f, "{e}"),
            }
        }
    }

    impl ValidateError {
        pub(super) fn as_marker_failure(&self) -> MarkerFailure {
            match self {
                ValidateError::Refused(_) => MarkerFailure::Refused,
                ValidateError::NotFound(_) => MarkerFailure::Failed,
            }
        }
    }

    pub(super) fn validate_marker_target(
        target: &str,
        workspace_dir: Option<&Path>,
    ) -> std::result::Result<MarkerTarget, ValidateError> {
        if target.starts_with("http://") || target.starts_with("https://") {
            let url = reqwest::Url::parse(target)
                .with_context(|| format!("parse marker URL {target}"))
                .map_err(ValidateError::Refused)?;
            let host_str = url.host_str().unwrap_or("");
            if clawcrew_tools::helpers::domain_guard::is_private_or_local_host(host_str) {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "target": target,
                            "host": host_str,
                            "reason": "ssrf_private_host",
                        })),
                    "matrix: marker target points to a private/local host"
                );
                return Err(ValidateError::Refused(anyhow::Error::msg(format!(
                    "matrix: marker target {target} resolves to a private or local host ({host_str}); refusing for SSRF safety. \
                     Use a public URL or attach the file from workspace_dir directly."
                ))));
            }
            return Ok(MarkerTarget::Http(url));
        }
        if target.contains("://") {
            let scheme = target.split("://").next().unwrap_or("?");
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "scheme": scheme,
                        "target": target,
                    })),
                "matrix: marker target uses disallowed scheme"
            );
            return Err(ValidateError::Refused(anyhow::Error::msg(format!(
                "matrix: marker target uses disallowed scheme {scheme:?}; only http/https and workspace-relative paths are accepted"
            ))));
        }
        if target.starts_with("data:") || target.starts_with("file:") {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "target": target,
                    })),
                "matrix: marker target uses disallowed data: or file: scheme"
            );
            return Err(ValidateError::Refused(anyhow::Error::msg(
                "matrix: marker target uses disallowed scheme; only http/https and workspace-relative paths are accepted",
            )));
        }

        let workspace = workspace_dir.ok_or_else(|| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "target": target,
                        "reason": "no_workspace_dir",
                    })),
                "matrix: marker target is local path but channel has no workspace_dir"
            );
            ValidateError::Refused(anyhow::Error::msg(format!(
                "matrix: marker target {target} is a local path but the channel was started without a workspace_dir, refusing for safety"
            )))
        })?;
        let workspace_canon = std::fs::canonicalize(workspace)
            .with_context(|| format!("canonicalize workspace {}", workspace.display()))
            .map_err(ValidateError::Refused)?;

        let target_path = Path::new(target);
        let absolute = if target_path.is_absolute() {
            target_path.to_path_buf()
        } else {
            workspace_canon.join(target_path)
        };
        let target_canon = match std::fs::canonicalize(&absolute) {
            Ok(p) => p,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "target": target,
                            "reason": "not_found",
                        })),
                    "matrix: marker target not found on disk"
                );
                return Err(ValidateError::NotFound(anyhow::Error::msg(format!(
                    "matrix: marker target {target} not found on disk"
                ))));
            }
            Err(e) => {
                return Err(ValidateError::Refused(
                    anyhow::Error::from(e).context(format!("canonicalize marker target {target}")),
                ));
            }
        };

        if !target_canon.starts_with(&workspace_canon) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "target": target,
                        "target_canon": target_canon.display().to_string(),
                        "workspace_canon": workspace_canon.display().to_string(),
                        "reason": "outside_workspace",
                    })),
                "matrix: marker target escapes workspace_dir"
            );
            return Err(ValidateError::Refused(anyhow::Error::msg(format!(
                "matrix: marker target {target} resolves to {} which is outside workspace_dir {}; refusing",
                target_canon.display(),
                workspace_canon.display(),
            ))));
        }
        Ok(MarkerTarget::Local(target_canon))
    }

    /// Maximum number of redirects to follow on a marker HTTP fetch. The
    /// `Policy::custom` closure below rejects any redirect target whose host
    /// is private/local, so the cap mainly bounds the worst-case public-→-public
    /// chain an attacker can construct.
    const MAX_MARKER_REDIRECTS: usize = 10;

    fn marker_http_client() -> &'static reqwest::Client {
        static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
        CLIENT.get_or_init(|| {
            let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_MARKER_REDIRECTS {
                    return attempt.error(std::io::Error::other(format!(
                        "Too many marker redirects (max {MAX_MARKER_REDIRECTS})"
                    )));
                }
                // `attempt.url()` borrows the attempt, so we copy out the
                // bits we need into owned Strings before `attempt.error(...)`,
                // which moves the attempt, can run.
                let target_str = attempt.url().as_str().to_string();
                let host = attempt.url().host_str().unwrap_or("").to_string();
                if clawcrew_tools::helpers::domain_guard::is_private_or_local_host(&host) {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "target": target_str,
                                "host": host,
                                "reason": "ssrf_redirect_to_private_host",
                            })),
                        "matrix: marker redirect targets a private/local host"
                    );
                    return attempt.error(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "Blocked marker redirect to private or local host ({host}); \
                             refusing for SSRF safety. Use a public URL or attach the file \
                             from workspace_dir directly."
                        ),
                    ));
                }
                attempt.follow()
            });
            clawcrew_config::schema::apply_runtime_proxy_to_builder(
                reqwest::Client::builder()
                    .timeout(MARKER_HTTP_TIMEOUT)
                    .redirect(redirect_policy)
                    .user_agent("clawcrew-matrix/1.0"),
                "channel.matrix",
            )
            .build()
            .expect("default reqwest client config never fails to build")
        })
    }

    pub(super) async fn fetch_http(url: reqwest::Url) -> Result<Vec<u8>> {
        let client = marker_http_client();
        let resp = client
            .get(url.clone())
            .send()
            .await
            .with_context(|| format!("fetch marker URL {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            bail!("matrix: marker URL {url} returned HTTP status {status}");
        }
        let mut stream = resp.bytes_stream();
        let mut buf = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.with_context(|| format!("stream chunk from {url}"))?;
            if buf.len().saturating_add(chunk.len()) > MAX_MARKER_BYTES {
                bail!("matrix: marker URL {url} exceeded {MAX_MARKER_BYTES}-byte cap; refusing");
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf)
    }

    pub(super) fn thread_anchor_from_message(
        outbox: &Outbox<'_>,
        message: &SendMessage,
    ) -> Option<OwnedEventId> {
        if outbox.reply_in_thread {
            message
                .thread_ts
                .as_deref()
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse().ok())
        } else {
            None
        }
    }

    pub(super) async fn deliver_attachments(
        outbox: &Outbox<'_>,
        room: &Room,
        mut text: String,
        markers: &[markers::Marker],
        attachments: &[MediaAttachment],
        thread_anchor: Option<&OwnedEventId>,
    ) -> Result<AttachmentDelivery> {
        let mut last_attachment_id: Option<OwnedEventId> = None;
        for att in attachments {
            let id = upload_attachment(room, att, AttachmentKind::Auto, thread_anchor).await?;
            last_attachment_id = Some(id);
        }

        // Track each failed marker with the reason: Refused (trust-boundary
        // rejection by validate_marker_target) vs Failed (everything else —
        // fetch error, upload rejection). Drives both the textual note and
        // the emoji reactions fired below.
        let mut failed_markers: Vec<(String, MarkerFailure)> = Vec::new();
        for marker in markers {
            let kind = match marker.kind {
                markers::MarkerKind::Image => AttachmentKind::Image,
                markers::MarkerKind::Audio => AttachmentKind::Audio,
                markers::MarkerKind::Video => AttachmentKind::Video,
                markers::MarkerKind::File => AttachmentKind::File,
                markers::MarkerKind::Voice => AttachmentKind::Voice,
            };
            let resolved = match validate_marker_target(&marker.target, outbox.workspace_dir) {
                Ok(t) => t,
                Err(e) => {
                    let kind = e.as_marker_failure();
                    let label = match kind {
                        MarkerFailure::Refused => "trust boundary",
                        MarkerFailure::Failed => "not found",
                    };
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                        &format!(
                            "matrix: skipping outbound marker for {} ({label}): {e}",
                            marker.target
                        )
                    );
                    failed_markers.push((marker.target.clone(), kind));
                    continue;
                }
            };
            let bytes = match resolved {
                MarkerTarget::Local(path) => match tokio::fs::read(&path).await {
                    Ok(b) => b,
                    Err(e) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                            &format!(
                                "matrix: skipping outbound marker for {} (read failed): {e}",
                                marker.target
                            )
                        );
                        failed_markers.push((marker.target.clone(), MarkerFailure::Failed));
                        continue;
                    }
                },
                MarkerTarget::Http(url) => match fetch_http(url).await {
                    Ok(b) => b,
                    Err(e) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                            &format!(
                                "matrix: skipping outbound marker for {} (http failed): {e}",
                                marker.target
                            )
                        );
                        failed_markers.push((marker.target.clone(), MarkerFailure::Failed));
                        continue;
                    }
                },
            };
            let file_name = derive_file_name(&marker.target);
            let mime = mime_for(&file_name, &kind);
            let att = MediaAttachment {
                file_name,
                data: bytes,
                mime_type: Some(mime),
                marker: None,
            };
            match upload_attachment(room, &att, kind, thread_anchor).await {
                Ok(id) => last_attachment_id = Some(id),
                Err(e) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                        &format!(
                            "matrix: skipping outbound marker for {} (upload failed): {e}",
                            marker.target
                        )
                    );
                    failed_markers.push((marker.target.clone(), MarkerFailure::Failed));
                }
            }
        }

        if !failed_markers.is_empty() {
            let targets: Vec<&str> = failed_markers.iter().map(|(t, _)| t.as_str()).collect();
            let note = if targets.len() == 1 {
                format!("(note: I couldn't deliver the file at {}.)", targets[0])
            } else {
                let joined = targets.join(", ");
                format!("(note: I couldn't deliver these files: {joined}.)")
            };
            text = if text.trim().is_empty() {
                note
            } else {
                format!("{text}\n\n{note}")
            };
        }

        Ok(AttachmentDelivery {
            text,
            last_attachment_id,
            failed_markers,
        })
    }

    pub(super) async fn send(outbox: &Outbox<'_>, message: &SendMessage) -> Result<OwnedEventId> {
        let room =
            resolve_joined_room(outbox.client, outbox.alias_cache, &message.recipient).await?;

        let (text, ms) = markers::parse(&message.content);

        // Build the thread anchor used by both attachment uploads and the
        // text reply, so attachments live in the same thread instead of
        // landing in the main timeline.
        let thread_anchor = thread_anchor_from_message(outbox, message);

        let delivery = deliver_attachments(
            outbox,
            &room,
            text,
            &ms,
            &message.attachments,
            thread_anchor.as_ref(),
        )
        .await?;

        // Decide whether to send the text, return the last attachment's
        // event_id, or surface an error. Marker-only messages used to error
        // here even though their attachment had landed; the runtime would
        // see Err and could retry, producing duplicate uploads.
        match decide_send_outcome(
            delivery.text.trim().is_empty(),
            delivery.last_attachment_id.is_some(),
        ) {
            SendOutcome::SendText => {}
            SendOutcome::ReturnAttachment => {
                // Safe by construction: ReturnAttachment is only returned
                // when last_attachment_id is Some.
                let kinds = delivery.failure_kinds();
                let attachment_id = delivery
                    .last_attachment_id
                    .expect("decide_send_outcome guarantees Some when ReturnAttachment");
                emit_failure_reactions(&room, &attachment_id, &kinds).await;
                return Ok(attachment_id);
            }
            SendOutcome::EmptyError => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"phase": "send"})),
                    "matrix: empty message body and no successful attachment"
                );
                return Err(anyhow::Error::msg(
                    "matrix: empty message body and no successful attachment",
                ));
            }
        }

        let event_id = if let (true, Some(anchor)) = (
            outbox.reply_in_thread,
            message.thread_ts.as_deref().filter(|s| !s.is_empty()),
        ) {
            send_threaded_reply(
                &room,
                &delivery.text,
                outbox.message_max_bytes,
                anchor,
                outbox.threads_seen,
            )
            .await?
        } else {
            let text = bounded_body(&delivery.text, outbox.message_max_bytes);
            let content = RoomMessageEventContent::text_markdown(&text);
            room.send(content).await?.response.event_id
        };

        let kinds = delivery.failure_kinds();
        emit_failure_reactions(&room, &event_id, &kinds).await;

        Ok(event_id)
    }

    /// Best-effort: apply 🚫 / ⚠️ reactions to the bot's just-sent message
    /// based on which kinds of marker failures occurred. Reaction send
    /// failures are logged but never propagated — the primary message
    /// already landed.
    pub(super) async fn emit_failure_reactions(
        room: &Room,
        event_id: &OwnedEventId,
        failures: &[MarkerFailure],
    ) {
        for emoji in decide_reactions(failures) {
            let content =
                ReactionEventContent::new(Annotation::new(event_id.clone(), emoji.to_string()));
            if let Err(e) = room.send(content).await {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"error": format!("{}", e), "emoji": emoji})
                        ),
                    "matrix: failed to send reaction on outgoing message"
                );
            }
        }
    }

    async fn send_threaded_reply(
        room: &Room,
        text: &str,
        message_max_bytes: Option<usize>,
        anchor_id: &str,
        threads_seen: &Arc<TokioRwLock<std::collections::HashSet<OwnedEventId>>>,
    ) -> Result<OwnedEventId> {
        let anchor: OwnedEventId = anchor_id
            .parse()
            .with_context(|| format!("parse thread anchor {anchor_id}"))?;
        let mut candidate = bounded_body(text, message_max_bytes);
        let reply_event = loop {
            let without_relation = RoomMessageEventContentWithoutRelation::new(MessageType::Text(
                TextMessageEventContent::markdown(candidate.as_str()),
            ));
            let event = room
                .make_reply_event(
                    without_relation,
                    Reply {
                        event_id: anchor.clone(),
                        enforce_thread: EnforceThread::Threaded(ReplyWithinThread::No),
                        add_mentions: AddMentions::No,
                    },
                )
                .await
                .map_err(|e| {
                    ::clawcrew_log::record!(
                        ERROR,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                        "make_reply_event failed"
                    );
                    anyhow::Error::msg(format!("make_reply_event failed: {e}"))
                })?;
            let Some(max_bytes) = message_max_bytes else {
                break event;
            };
            let actual =
                serde_json::to_vec(&event).map_or(usize::MAX, |serialized| serialized.len());
            if actual <= max_bytes {
                break event;
            }
            let next = next_prefix_after_oversize(&candidate, actual, max_bytes);
            if next.len() == candidate.len() {
                anyhow::bail!(
                    "matrix: configured message_max_bytes cannot contain a threaded reply event"
                );
            }
            candidate = next.to_string();
        };
        ctx_mod::mark_seen(threads_seen, anchor).await;
        let resp = room.send(reply_event).await?;
        Ok(resp.response.event_id)
    }

    pub(super) async fn edit(
        client: &Client,
        room_id: &str,
        event_id: &OwnedEventId,
        text: &str,
        message_max_bytes: Option<usize>,
    ) -> Result<String> {
        let room = client
            .get_room(&room_id.parse::<OwnedRoomId>()?)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"room_id": room_id})),
                    "matrix: room not joined"
                );
                anyhow::Error::msg(format!("matrix: room not joined: {room_id}"))
            })?;
        let candidate = text.to_string();
        let edit_event = {
            let new_content = RoomMessageEventContentWithoutRelation::new(MessageType::Text(
                TextMessageEventContent::markdown(candidate.as_str()),
            ));
            let event = room
                .make_edit_event(event_id, EditedContent::RoomMessage(new_content))
                .await
                .map_err(|e| {
                    ::clawcrew_log::record!(
                        ERROR,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                        "make_edit_event failed"
                    );
                    anyhow::Error::msg(format!("make_edit_event failed: {e}"))
                })?;
            if let Some(max_bytes) = message_max_bytes {
                let actual =
                    serde_json::to_vec(&event).map_or(usize::MAX, |serialized| serialized.len());
                if actual > max_bytes {
                    anyhow::bail!(
                        "matrix: selected single-message progress edit exceeds configured message_max_bytes"
                    );
                }
            }
            event
        };
        room.send(edit_event).await?;
        Ok(candidate)
    }

    pub(super) async fn redact(
        client: &Client,
        room_id: &str,
        event_id: &OwnedEventId,
        reason: Option<String>,
    ) -> Result<()> {
        let room = client
            .get_room(&room_id.parse::<OwnedRoomId>()?)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"room_id": room_id})),
                    "matrix: room not joined"
                );
                anyhow::Error::msg(format!("matrix: room not joined: {room_id}"))
            })?;
        room.redact(event_id, reason.as_deref(), None).await?;
        Ok(())
    }

    pub(super) async fn react(
        outbox: &Outbox<'_>,
        room_id: &str,
        event_id: &OwnedEventId,
        emoji: &str,
    ) -> Result<()> {
        let room = resolve_joined_room(outbox.client, outbox.alias_cache, room_id).await?;
        let content =
            ReactionEventContent::new(Annotation::new(event_id.clone(), emoji.to_string()));
        let resp = room.send(content).await?;
        outbox.reaction_log.lock().await.insert(
            (
                room.room_id().to_owned(),
                event_id.clone(),
                emoji.to_string(),
            ),
            resp.response.event_id,
        );
        Ok(())
    }

    pub(super) async fn unreact(
        outbox: &Outbox<'_>,
        room_id: &str,
        event_id: &OwnedEventId,
        emoji: &str,
    ) -> Result<()> {
        let room = resolve_joined_room(outbox.client, outbox.alias_cache, room_id).await?;
        let key = (
            room.room_id().to_owned(),
            event_id.clone(),
            emoji.to_string(),
        );
        let reaction_event_id = outbox.reaction_log.lock().await.remove(&key);
        if let Some(rid) = reaction_event_id {
            room.redact(&rid, Some("removing reaction"), None).await?;
        }
        Ok(())
    }

    pub(super) async fn resolve_joined_room(
        client: &Client,
        cache: &Arc<TokioRwLock<HashMap<String, OwnedRoomId>>>,
        recipient: &str,
    ) -> Result<Room> {
        let id = client::resolve_room(client, cache, recipient).await?;
        let room = client.get_room(&id).ok_or_else(|| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"recipient": recipient})),
                "matrix: bot is not in room"
            );
            anyhow::Error::msg(format!("matrix: bot is not in room {recipient}"))
        })?;
        if room.state() != RoomState::Joined {
            bail!("matrix: room {recipient} is not in joined state");
        }
        Ok(room)
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum AttachmentKind {
        Auto,
        Image,
        Audio,
        Video,
        File,
        Voice,
    }

    pub(super) async fn upload_attachment(
        room: &Room,
        att: &MediaAttachment,
        kind: AttachmentKind,
        thread_anchor: Option<&OwnedEventId>,
    ) -> Result<OwnedEventId> {
        let mime = attachment_mime(att);
        let config = attachment_config_for(att, kind, &mime, thread_anchor);
        let resp = room
            .send_attachment(att.file_name.clone(), &mime, att.data.clone(), config)
            .await
            .map_err(|e| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                    "send_attachment failed"
                );
                anyhow::Error::msg(format!("send_attachment failed: {e}"))
            })?;
        Ok(resp.event_id)
    }

    pub(super) fn attachment_config_for(
        att: &MediaAttachment,
        kind: AttachmentKind,
        mime: &mime_guess::Mime,
        thread_anchor: Option<&OwnedEventId>,
    ) -> AttachmentConfig {
        let mut config = AttachmentConfig::new().info(attachment_info_for(att, kind, mime));
        if let Some(anchor) = thread_anchor {
            config = config.reply(Some(Reply {
                event_id: anchor.clone(),
                enforce_thread: EnforceThread::Threaded(ReplyWithinThread::No),
                add_mentions: AddMentions::No,
            }));
        }
        config
    }

    pub(super) fn attachment_mime(att: &MediaAttachment) -> mime_guess::Mime {
        match att.mime_type.as_deref() {
            Some(m) => m
                .parse()
                .unwrap_or(mime_guess::mime::APPLICATION_OCTET_STREAM),
            None => mime_guess::from_path(&att.file_name)
                .first()
                .unwrap_or(mime_guess::mime::APPLICATION_OCTET_STREAM),
        }
    }

    fn attachment_info_for(
        att: &MediaAttachment,
        kind: AttachmentKind,
        mime: &mime_guess::Mime,
    ) -> AttachmentInfo {
        let size = UInt::try_from(att.data.len()).ok();
        match attachment_info_kind(kind, mime) {
            AttachmentKind::Image => AttachmentInfo::Image(BaseImageInfo {
                size,
                ..Default::default()
            }),
            AttachmentKind::Audio => AttachmentInfo::Audio(BaseAudioInfo {
                size,
                ..Default::default()
            }),
            AttachmentKind::Video => AttachmentInfo::Video(BaseVideoInfo {
                size,
                ..Default::default()
            }),
            // `duration` and `waveform` must both be `Some` for the SDK to emit
            // an `org.matrix.msc1767.audio` block at all, and without a
            // duration clients render a voice bubble stuck at `00:00` with no
            // seek bar. The length is read straight out of the Ogg container by
            // `opus_duration` -- no decoding, and no length asserted when the
            // bytes cannot be read with certainty, in which case the previous
            // zero is still sent. The waveform stays empty: it is optional in
            // MSC1767 and would need decoded PCM.
            AttachmentKind::Voice => AttachmentInfo::Voice(BaseAudioInfo {
                duration: Some(opus_duration(&att.data).unwrap_or(Duration::ZERO)),
                size,
                waveform: Some(Vec::new()),
            }),
            AttachmentKind::File | AttachmentKind::Auto => {
                AttachmentInfo::File(BaseFileInfo { size })
            }
        }
    }

    fn attachment_info_kind(kind: AttachmentKind, mime: &mime_guess::Mime) -> AttachmentKind {
        if kind == AttachmentKind::Voice {
            return AttachmentKind::Voice;
        }
        match mime.type_() {
            mime_guess::mime::IMAGE => AttachmentKind::Image,
            mime_guess::mime::AUDIO => AttachmentKind::Audio,
            mime_guess::mime::VIDEO => AttachmentKind::Video,
            _ => AttachmentKind::File,
        }
    }

    /// Playback length of an Ogg-Opus stream, read from the container alone.
    ///
    /// Opus always ticks at 48 kHz, so the length is the span the granule
    /// positions cover, less the priming samples `OpusHead` asks the decoder to
    /// discard:
    ///
    /// ```text
    /// samples = final_granule - start_granule - pre_skip
    /// ```
    ///
    /// `start_granule` is derived rather than stored. RFC 7845 section 4.5 lets
    /// the first audio page carry a granule larger than the samples completing
    /// on it, which is how a clip keeps the timeline of the recording it was cut
    /// from; taking that page's granule as the origin would report the offset as
    /// playback time. The origin is the granule minus the samples of the packets
    /// completing on the page, counted from Opus table-of-contents metadata
    /// (RFC 6716 section 3.1) without decoding audio.
    ///
    /// That page may instead carry a granule *below* those samples, but only
    /// when it also ends the stream: the tail is trimmed to finish somewhere
    /// other than a frame boundary, and the timeline starts at zero. The same
    /// granule on a page that does not end the stream is invalid.
    ///
    /// Only a single logical stream is measured. A page whose serial differs
    /// from the opening stream's, or a second beginning-of-stream page, belongs
    /// to a chained or multiplexed file, whose length one number cannot honestly
    /// describe.
    ///
    /// Returns `None` -- never a guess -- when the stream cannot be read end to
    /// end: a bad capture pattern, a first page that is not a lone `OpusHead`
    /// marked beginning-of-stream, a segment table or page body running past the
    /// end, an audio page opening mid-packet, a table-of-contents byte that will
    /// not parse, no page carrying a known granule, or a span shorter than
    /// `pre_skip`.
    pub(super) fn opus_duration(bytes: &[u8]) -> Option<Duration> {
        /// `OggS`, version, header type, granule, serial, sequence, checksum,
        /// segment count -- the fixed part of a page header.
        const PAGE_HEADER_LEN: usize = 27;
        /// `OpusHead`, version, channel count, `pre_skip`.
        const OPUS_HEAD_LEN: usize = 12;
        /// Opus granule positions always tick at 48 kHz, whatever the input
        /// sample rate was.
        const GRANULE_HZ: u64 = 48_000;
        /// Header-type bit for a page opening with the tail of a packet carried
        /// over from the page before it.
        const CONTINUED: u8 = 0x01;
        /// Header-type bit for a page that begins a logical stream.
        const BOS: u8 = 0x02;
        /// Header-type bit for a page that ends a logical stream.
        const EOS: u8 = 0x04;
        /// `OpusHead` then `OpusTags` precede the audio; the latter may span
        /// pages, so audio begins once both have completed.
        const HEADER_PACKETS: u32 = 2;

        let mut cursor = 0usize;
        let mut serial: Option<u32> = None;
        let mut pre_skip: Option<u64> = None;
        let mut header_packets = 0u32;
        let mut start_granule: Option<u64> = None;
        let mut last_granule: Option<u64> = None;

        while bytes.len() - cursor >= PAGE_HEADER_LEN {
            let header = bytes.get(cursor..cursor + PAGE_HEADER_LEN)?;
            if &header[..4] != b"OggS" {
                return None;
            }
            let header_type = header[5];
            let granule = u64::from_le_bytes(header[6..14].try_into().ok()?);
            let page_serial = u32::from_le_bytes(header[14..18].try_into().ok()?);
            let segments = usize::from(header[26]);
            // The segment table follows the fixed header; its bytes sum to the
            // page body length.
            let table_start = cursor.checked_add(PAGE_HEADER_LEN)?;
            let body_start = table_start.checked_add(segments)?;
            let table = bytes.get(table_start..body_start)?;
            let body_len = table.iter().map(|&n| usize::from(n)).sum::<usize>();
            let body_end = body_start.checked_add(body_len)?;
            let body = bytes.get(body_start..body_end)?;

            match serial {
                None => {
                    // The identification header opens the stream and holds its
                    // page alone.
                    if header[4] != 0
                        || header_type & BOS == 0
                        || body.len() < OPUS_HEAD_LEN
                        || &body[..8] != b"OpusHead"
                        || completed_packets(table) != 1
                    {
                        return None;
                    }
                    serial = Some(page_serial);
                    pre_skip = Some(u64::from(u16::from_le_bytes([body[10], body[11]])));
                }
                // Another logical stream, concatenated after this one or
                // interleaved with it. A reused serial still starts a new
                // stream when the page is marked beginning-of-stream.
                Some(open) if open != page_serial || header_type & BOS != 0 => return None,
                Some(_) => {}
            }

            if header_packets < HEADER_PACKETS {
                header_packets = header_packets.saturating_add(completed_packets(table));
                if header_packets > HEADER_PACKETS {
                    // Audio riding on the page that finishes `OpusTags`. The
                    // first audio packet opens a page of its own, so there is
                    // no page whose granule the origin can be derived from.
                    return None;
                }
            } else if start_granule.is_none() {
                // The first audio page fixes the origin the rest is measured
                // from, so it has to be whole and timed.
                if header_type & CONTINUED != 0 || granule == u64::MAX {
                    return None;
                }
                let samples = completed_packet_samples(table, body)?;
                start_granule = Some(match granule.checked_sub(samples) {
                    Some(origin) => origin,
                    // A granule below the samples completing on the page trims
                    // the tail, which only a page ending the stream may do. The
                    // timeline then starts at zero rather than being derivable
                    // by working backwards.
                    None if header_type & EOS != 0 => 0,
                    None => return None,
                });
            }

            // `u64::MAX` is the "granule not known for this page" marker.
            if granule != u64::MAX {
                last_granule = Some(granule);
            }
            // Always forward progress: `body_end` is at least one header past
            // `cursor`, even for a page with no segments.
            cursor = body_end;
        }
        if cursor != bytes.len() {
            // Trailing bytes that are not a page: the buffer is truncated or
            // is not what it claims to be.
            return None;
        }

        let samples = last_granule?
            .checked_sub(start_granule?)?
            .checked_sub(pre_skip?)?;
        Some(Duration::new(
            samples / GRANULE_HZ,
            // Exact, and cannot overflow: the remainder is below 48_000.
            ((samples % GRANULE_HZ) * 1_000_000_000 / GRANULE_HZ) as u32,
        ))
    }

    /// How many packets finish on a page, from its segment table.
    ///
    /// A lacing value below 255 ends a packet; a run of 255s carries one onto
    /// the following page.
    fn completed_packets(table: &[u8]) -> u32 {
        u32::try_from(table.iter().filter(|&&lacing| lacing < 255).count()).unwrap_or(u32::MAX)
    }

    /// Samples carried by the packets that finish on one page.
    ///
    /// A trailing run of 255s belongs to a packet continuing onto the next page
    /// and contributes nothing here, which is what makes this a count of the
    /// audio the page's granule position accounts for.
    fn completed_packet_samples(table: &[u8], body: &[u8]) -> Option<u64> {
        let mut total = 0u64;
        let mut offset = 0usize;
        let mut packet_len = 0usize;
        for &lacing in table {
            packet_len = packet_len.checked_add(usize::from(lacing))?;
            if lacing < 255 {
                let end = offset.checked_add(packet_len)?;
                total = total.checked_add(opus_packet_samples(body.get(offset..end)?)?)?;
                offset = end;
                packet_len = 0;
            }
        }
        Some(total)
    }

    /// Samples in one Opus packet, on the 48 kHz granule clock.
    ///
    /// The first byte is the table of contents (RFC 6716 section 3.1): its top
    /// five bits select the frame length and its bottom two how many frames the
    /// packet holds. Length needs nothing further -- the encoded audio itself is
    /// never touched.
    fn opus_packet_samples(packet: &[u8]) -> Option<u64> {
        /// Frame length per table-of-contents configuration, in samples at
        /// 48 kHz: SILK narrow, medium and wideband run 10/20/40/60 ms, hybrid
        /// super-wideband and fullband 10/20 ms, and CELT 2.5/5/10/20 ms per
        /// band. Expressed in samples so the 2.5 ms case stays a whole number.
        const FRAME_SAMPLES: [u64; 32] = [
            480, 960, 1920, 2880, 480, 960, 1920, 2880, 480, 960, 1920, 2880, 480, 960, 480, 960,
            120, 240, 480, 960, 120, 240, 480, 960, 120, 240, 480, 960, 120, 240, 480, 960,
        ];
        /// A packet holds at most 120 ms of audio.
        const MAX_PACKET_SAMPLES: u64 = 5_760;

        let toc = *packet.first()?;
        let frame = *FRAME_SAMPLES.get(usize::from(toc >> 3))?;
        let frames = match toc & 0x03 {
            0 => 1,
            1 | 2 => 2,
            // An arbitrary frame count, in the six low bits of the next byte.
            _ => u64::from(*packet.get(1)? & 0x3F),
        };
        let samples = frame.checked_mul(frames)?;
        (samples > 0 && samples <= MAX_PACKET_SAMPLES).then_some(samples)
    }

    /// Ogg/Opus is what `TtsManager::synthesize_opus` produces and what Matrix
    /// clients expect for a voice note.
    pub(super) const VOICE_NOTE_MIME: &str = "audio/ogg";
    pub(super) const VOICE_NOTE_FILE_NAME: &str = "voice.ogg";

    /// Deliver synthesized speech as an MSC3245 voice note, in the same
    /// thread as the text reply it accompanies.
    pub(super) async fn send_voice_note(
        room: &Room,
        audio: Vec<u8>,
        thread_anchor: Option<&OwnedEventId>,
    ) -> Result<OwnedEventId> {
        let att = MediaAttachment {
            file_name: VOICE_NOTE_FILE_NAME.to_string(),
            data: audio,
            mime_type: Some(VOICE_NOTE_MIME.to_string()),
            marker: None,
        };
        upload_attachment(room, &att, AttachmentKind::Voice, thread_anchor).await
    }

    fn derive_file_name(target: &str) -> String {
        target
            .rsplit_once('/')
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| target.to_string())
    }

    fn mime_for(file_name: &str, kind: &AttachmentKind) -> String {
        if let Some(m) = mime_guess::from_path(file_name).first() {
            return m.essence_str().to_string();
        }
        match kind {
            AttachmentKind::Image => "image/jpeg".to_string(),
            AttachmentKind::Audio | AttachmentKind::Voice => "audio/ogg".to_string(),
            AttachmentKind::Video => "video/mp4".to_string(),
            AttachmentKind::File | AttachmentKind::Auto => "application/octet-stream".to_string(),
        }
    }
}

// ─── public type ───────────────────────────────────────────────────────────

/// Matrix channel.
pub struct MatrixChannel {
    config: Arc<MatrixConfig>,
    /// The alias key under `[channels.matrix.<alias>]` this handle is
    /// bound to. Used to scope peer-group writes and resolver lookups.
    alias: String,
    /// Resolves inbound external peers from canonical state at message-time.
    /// No cache (see AGENTS.md "ABSOLUTE RULE — SINGLE SOURCE OF TRUTH").
    peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    state_dir: PathBuf,
    workspace_dir: Option<Arc<PathBuf>>,
    transcription: Option<TranscriptionResolver>,
    tts: Option<TtsResolver>,
    /// Recipients the operator placed in a voice-modality peer group.
    /// Resolved from live config on every send, never cached.
    voice_peers: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    client: tokio::sync::OnceCell<Client>,
    pending_approvals: Arc<TokioMutex<HashMap<String, crate::util::PendingApproval>>>,
    streaming_state: Arc<TokioRwLock<streaming::State>>,
    threads_seen: Arc<TokioRwLock<HashSet<OwnedEventId>>>,
    alias_cache: Arc<TokioRwLock<HashMap<String, OwnedRoomId>>>,
    reaction_log: Arc<TokioMutex<HashMap<outbound::ReactionKey, OwnedEventId>>>,
    bot_display_name: Arc<TokioRwLock<Option<String>>>,
    initial_sync_done: Arc<AtomicBool>,
    undecryptable_seen: Arc<TokioMutex<HashSet<OwnedEventId>>>,
    /// Resolved `ack_reactions` for this Matrix instance — the
    /// per-channel `MatrixConfig.ack_reactions` override falls back to
    /// `[channels].ack_reactions` here at construction time, so the
    /// read site doesn't need to re-resolve on every reaction.
    ack_reactions: bool,
}

impl MatrixChannel {
    /// Validate config and prepare the channel. The SDK Client is built lazily
    /// on first `listen()` or `send()` call.
    pub fn new(
        config: MatrixConfig,
        alias: impl Into<String>,
        peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
        state_dir: PathBuf,
    ) -> Result<Self> {
        if config.homeserver.trim().is_empty() {
            bail!("matrix: `homeserver` is required");
        }
        let has_token = config
            .access_token
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty());
        let has_password = config
            .password
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty());
        if !has_token && !has_password {
            bail!("matrix: configure either `access_token` or `password`");
        }
        let ack_reactions = config.ack_reactions.unwrap_or(true);
        let streaming_state = streaming::State::for_stream_mode(config.stream_mode);
        Ok(Self {
            config: Arc::new(config),
            alias: alias.into(),
            peer_resolver,
            state_dir,
            workspace_dir: None,
            transcription: None,
            tts: None,
            voice_peers: Arc::new(Vec::new),
            client: tokio::sync::OnceCell::new(),
            pending_approvals: Arc::new(TokioMutex::new(HashMap::new())),
            streaming_state: Arc::new(TokioRwLock::new(streaming_state)),
            threads_seen: Arc::new(TokioRwLock::new(HashSet::new())),
            alias_cache: Arc::new(TokioRwLock::new(HashMap::new())),
            reaction_log: Arc::new(TokioMutex::new(HashMap::new())),
            bot_display_name: Arc::new(TokioRwLock::new(None)),
            initial_sync_done: Arc::new(AtomicBool::new(false)),
            undecryptable_seen: Arc::new(TokioMutex::new(HashSet::new())),
            ack_reactions,
        })
    }

    /// Return the alias under `[channels.matrix.<alias>]` that this
    /// channel handle is bound to.
    pub fn alias(&self) -> &str {
        &self.alias
    }

    #[must_use]
    pub fn with_ack_reactions(mut self, ack_reactions: bool) -> Self {
        self.ack_reactions = ack_reactions;
        self
    }

    /// Replace the compatibility resolver with the channel runtime's
    /// live-config resolver, so typed provider entries and the owning agent's
    /// `transcription_provider` are honoured.
    pub(crate) fn with_transcription_manager_factory(
        mut self,
        factory: impl Fn() -> Option<anyhow::Result<crate::transcription::TranscriptionManager>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.transcription = Some(Arc::new(factory));
        self
    }

    /// Install the channel runtime's live-config TTS resolver, so the owning
    /// agent's `tts_provider` and reloaded provider policy are honoured.
    pub(crate) fn with_tts_manager_factory(
        mut self,
        factory: impl Fn() -> Option<anyhow::Result<crate::tts::TtsManager>> + Send + Sync + 'static,
    ) -> Self {
        self.tts = Some(Arc::new(factory));
        self
    }

    /// Install the resolver for recipients in a voice-modality peer group.
    pub(crate) fn with_voice_peer_resolver(
        mut self,
        voice_peers: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    ) -> Self {
        self.voice_peers = voice_peers;
        self
    }

    /// Configure the workspace directory used to persist downloaded media so
    /// the agent's vision/document pipelines can read inbound files via
    /// `[IMAGE:path]` / `[Document: name] path` markers.
    pub fn with_workspace_dir(mut self, dir: PathBuf) -> Self {
        self.workspace_dir = Some(Arc::new(dir));
        self
    }

    async fn ensure_client(&self) -> Result<&Client> {
        use ::clawcrew_log::__private::tracing::Instrument;
        self.client
            .get_or_try_init(|| {
                async {
                    let c = client::build(&self.config, &self.state_dir).await?;
                    if let Ok(Some(name)) = c.account().get_display_name().await {
                        *self.bot_display_name.write().await = Some(name);
                    }
                    Ok::<_, anyhow::Error>(c)
                }
                .instrument(::clawcrew_log::attribution_span!(self))
            })
            .await
    }

    fn outbox<'a>(&'a self, client: &'a Client) -> outbound::Outbox<'a> {
        outbound::Outbox {
            client,
            alias_cache: &self.alias_cache,
            threads_seen: &self.threads_seen,
            reaction_log: &self.reaction_log,
            reply_in_thread: self.config.reply_in_thread,
            workspace_dir: self.workspace_dir.as_deref().map(|p| p.as_path()),
            message_max_bytes: None,
        }
    }

    fn final_outbox<'a>(&'a self, client: &'a Client) -> outbound::Outbox<'a> {
        let mut outbox = self.outbox(client);
        outbox.message_max_bytes = (self.config.stream_mode == MatrixStreamMode::SingleMessage)
            .then_some(self.config.effective_message_max_bytes());
        outbox
    }

    /// The half of the voice decision that needs no homeserver: `suppress_voice`
    /// always wins, no TTS resolver means never, and an explicit `force_voice`
    /// always does. `None` means the answer depends on the target room.
    fn voice_intent(&self, message: &SendMessage) -> Option<bool> {
        if self.tts.is_none() || message.suppress_voice {
            return Some(false);
        }
        if message.force_voice {
            return Some(true);
        }
        None
    }

    /// Whether `recipient` is a room holding a member of a voice-modality peer
    /// group. Resolved from live config on every call.
    ///
    /// Peer groups name Matrix peers by user ID (`@user:server`), while an
    /// outbound recipient is always a room (`!room:server`), so the two are
    /// never comparable directly. For replies the runtime resolves the sender's
    /// group and reports it as `force_voice`; this path exists for sends with
    /// no inbound sender to consult — cron announcements and other proactive
    /// delivery — where the room is the only identity available.
    async fn room_has_voice_peer(&self, client: &Client, recipient: &str) -> bool {
        let voice_peers = (self.voice_peers)();
        if let Some(verdict) = allowlist::voice_peers_verdict(&voice_peers) {
            return verdict;
        }
        let Ok(room) = outbound::resolve_joined_room(client, &self.alias_cache, recipient).await
        else {
            return false;
        };
        let Ok(members) = room.members(RoomMemberships::JOIN).await else {
            return false;
        };
        members.iter().any(|m| {
            crate::allowlist::is_user_allowed_by(
                &voice_peers,
                m.user_id().as_str(),
                allowlist::voice_peer_matches,
            )
        })
    }

    /// Voice delivery is decided by configuration and the runtime's per-message
    /// intent, never by inspecting the reply text.
    async fn should_voice(&self, client: &Client, message: &SendMessage) -> bool {
        match self.voice_intent(message) {
            Some(verdict) => verdict,
            None => self.room_has_voice_peer(client, &message.recipient).await,
        }
    }

    /// Thread anchor for a voice note accompanying a plain send.
    fn voice_thread_anchor(&self, message: &SendMessage) -> Option<OwnedEventId> {
        if !self.config.reply_in_thread {
            return None;
        }
        message
            .thread_ts
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse().ok())
    }

    /// Thread anchor of a live draft, so a finalized reply's voice note lands
    /// beside its text rather than in the main timeline.
    async fn draft_thread_anchor(&self, recipient: &str, message_id: &str) -> Option<OwnedEventId> {
        let key = streaming_key(recipient, message_id).ok()?;
        let state = self.streaming_state.read().await;
        streaming::peek_thread_anchor(&state, &key)
    }

    /// Synthesize `text` and post it as a voice note beside the text reply.
    ///
    /// Never propagates. The text reply has already landed by this point, so a
    /// synthesis or upload failure must not make the runtime treat the whole
    /// send as failed and retry it.
    async fn deliver_voice_note(
        &self,
        client: &Client,
        recipient: &str,
        text: &str,
        thread_anchor: Option<OwnedEventId>,
    ) {
        let Some(resolver) = self.tts.as_ref() else {
            return;
        };
        let delivered: Result<Option<OwnedEventId>> = async {
            let Some(manager) = resolver() else {
                return Ok(None);
            };
            let manager = manager?;
            // Speak the reply the reader sees: file markers are rendered as
            // attachments, not read aloud.
            let spoken = markers::parse(text).0;
            if spoken.trim().is_empty() {
                return Ok(None);
            }
            let audio = manager.synthesize_opus(&spoken).await?;
            let room = outbound::resolve_joined_room(client, &self.alias_cache, recipient).await?;
            outbound::send_voice_note(&room, audio, thread_anchor.as_ref())
                .await
                .map(Some)
        }
        .await;

        if let Err(e) = delivered {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                "matrix: voice reply failed; the text reply was already delivered"
            );
        }
    }

    /// Edit-in-place draft update. Rate-limited per the configured interval.
    async fn partial_update(&self, recipient: &str, message_id: &str, text: &str) -> Result<()> {
        let client = self.ensure_client().await?;
        let key = streaming_key(recipient, message_id)?;
        let Some(visible_text) = streaming::partial_visible_text(text) else {
            return Ok(());
        };
        let event_id = {
            let mut state = self.streaming_state.write().await;
            let Some(draft) = streaming::partial_for_update(&mut state, &key) else {
                return Ok(());
            };
            let now = Instant::now();
            let interval = Duration::from_millis(self.config.draft_update_interval_ms.max(50));
            if !streaming::partial_should_edit(draft, &visible_text, now, interval) {
                return Ok(());
            }
            let event_id = draft.event_id.clone();
            draft.last_text = visible_text.clone();
            draft.last_edit = now;
            event_id
        };
        outbound::edit(client, recipient, &event_id, &visible_text, None)
            .await
            .map(|_| ())
    }

    /// Update the sliding progress transcript for `single_message` mode.
    /// Unlike Partial mode, assistant answer text is intentionally ignored
    /// elsewhere; this draft is only for durable status/progress entries.
    async fn single_update_progress(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> Result<()> {
        let key = streaming_key(recipient, message_id)?;
        let max_body_bytes = self.config.effective_message_max_bytes();
        let update = {
            let mut state = self.streaming_state.write().await;
            let Some(draft) = streaming::single_for_update(&mut state, &key) else {
                return Ok(());
            };
            streaming::push_single_progress_line(draft, text, self.config.stream_draft_lines);

            let now = Instant::now();
            let interval = Duration::from_millis(self.config.draft_update_interval_ms.max(50));
            if !streaming::single_edit_interval_elapsed(draft, now, interval) {
                return Ok(());
            }

            let visible_text =
                streaming::single_visible_text_with_edit_budget(draft, max_body_bytes);
            if visible_text.is_empty() || !streaming::single_render_changed(draft, &visible_text) {
                return Ok(());
            }
            draft.last_edit = now;
            (draft.event_id.clone(), visible_text)
        };
        let client = self.ensure_client().await?;
        let delivered = outbound::edit(
            client,
            recipient,
            &update.0,
            &update.1,
            Some(max_body_bytes),
        )
        .await?;
        {
            let mut state = self.streaming_state.write().await;
            if let Some(draft) = streaming::single_for_update(&mut state, &key) {
                streaming::mark_single_edit_delivered(draft, &update.0, delivered, Instant::now());
            }
        }
        Ok(())
    }

    /// Apply a burst of single-message progress entries and publish the
    /// coalesced transcript with one Matrix edit. The caller owns pacing.
    async fn single_update_progress_batch(
        &self,
        recipient: &str,
        message_id: &str,
        texts: &[String],
    ) -> Result<()> {
        let progress = texts
            .iter()
            .map(|text| streaming::legacy_single_progress(text))
            .collect::<Vec<_>>();
        self.single_update_typed_progress_batch(recipient, message_id, &progress)
            .await
    }

    /// Publish a coalesced Matrix progress batch while retaining each entry's
    /// semantic source through the channel-local draft buffer.
    async fn single_update_typed_progress_batch(
        &self,
        recipient: &str,
        message_id: &str,
        progress: &[DraftProgress],
    ) -> Result<()> {
        if progress.is_empty() {
            return Ok(());
        }
        let key = streaming_key(recipient, message_id)?;
        let max_body_bytes = self.config.effective_message_max_bytes();
        let update = {
            let mut state = self.streaming_state.write().await;
            let Some(draft) = streaming::single_for_update(&mut state, &key) else {
                return Ok(());
            };
            for entry in progress {
                streaming::push_single_progress(
                    draft,
                    entry.clone(),
                    self.config.stream_draft_lines,
                );
            }

            let visible_text =
                streaming::single_visible_text_with_edit_budget(draft, max_body_bytes);
            if visible_text.is_empty() || !streaming::single_render_changed(draft, &visible_text) {
                return Ok(());
            }
            draft.last_edit = Instant::now();
            (draft.event_id.clone(), visible_text)
        };
        let client = self.ensure_client().await?;
        let delivered = outbound::edit(
            client,
            recipient,
            &update.0,
            &update.1,
            Some(max_body_bytes),
        )
        .await?;
        {
            let mut state = self.streaming_state.write().await;
            if let Some(draft) = streaming::single_for_update(&mut state, &key) {
                streaming::mark_single_edit_delivered(draft, &update.0, delivered, Instant::now());
            }
        }
        Ok(())
    }

    /// MultiMessage paragraph emitter. Loops emitting one paragraph per
    /// `\n\n` boundary until the unsent buffer no longer contains a break,
    /// then returns to wait for more accumulated text. Each paragraph posts
    /// as an independent room message threaded under the captured anchor.
    async fn multi_update(&self, recipient: &str, message_id: &str, text: &str) -> Result<()> {
        let client = self.ensure_client().await?;
        let key = streaming_key(recipient, message_id)?;
        let delay = Duration::from_millis(self.config.multi_message_delay_ms);
        loop {
            let (paragraph, thread_anchor) = {
                let mut state = self.streaming_state.write().await;
                let Some(multi) = streaming::multi_for_update(&mut state, &key) else {
                    return Ok(());
                };
                // Detect a buffer reset (e.g. DraftEvent::Clear) and re-anchor
                // to the new shorter text.
                if text.len() < multi.sent_so_far {
                    multi.sent_so_far = 0;
                    return Ok(());
                }
                if text.len() == multi.sent_so_far {
                    return Ok(());
                }
                let unsent = &text[multi.sent_so_far..];
                let Some(break_at) = streaming::next_paragraph_break(unsent) else {
                    return Ok(());
                };
                let paragraph = unsent[..break_at].trim().to_string();
                multi.sent_so_far += break_at + 2; // +2 for the consumed "\n\n"
                (paragraph, multi.thread_anchor.clone())
            };
            if !paragraph.is_empty() {
                let mut msg = SendMessage::new(paragraph, recipient);
                msg.thread_ts = thread_anchor.as_ref().map(|e| e.to_string());
                if let Err(e) = outbound::send(&self.outbox(client), &msg).await {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                        "matrix: multi-message paragraph send failed"
                    );
                }
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn redact_single_draft_before_final(
        client: &Client,
        recipient: &str,
        event_id: &OwnedEventId,
        reason: &str,
    ) {
        if let Err(err) =
            outbound::redact(client, recipient, event_id, Some(reason.to_string())).await
        {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"err": err.to_string()})),
                "matrix: single-message draft cleanup failed before final send; sending final response anyway"
            );
        }
    }
}

impl ::clawcrew_api::attribution::Attributable for MatrixChannel {
    fn role(&self) -> ::clawcrew_api::attribution::Role {
        ::clawcrew_api::attribution::Role::Channel(::clawcrew_api::attribution::ChannelKind::Matrix)
    }
    fn alias(&self) -> &str {
        &self.alias
    }
}

#[async_trait]
impl Channel for MatrixChannel {
    fn name(&self) -> &str {
        "matrix"
    }

    fn self_handle(&self) -> Option<String> {
        self.client
            .get()
            .and_then(|c| c.user_id().map(|u| u.to_string()))
    }

    fn self_addressed_mention(&self) -> Option<String> {
        self.self_handle()
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {
        let client = self.ensure_client().await?;
        let _ = outbound::send(&self.outbox(client), message).await?;
        if self.should_voice(client, message).await {
            let anchor = self.voice_thread_anchor(message);
            self.deliver_voice_note(client, &message.recipient, &message.content, anchor)
                .await;
        }
        Ok(())
    }

    async fn send_final(&self, message: &SendMessage) -> Result<()> {
        let client = self.ensure_client().await?;
        let _ = outbound::send(&self.final_outbox(client), message).await?;
        if self.should_voice(client, message).await {
            let anchor = self.voice_thread_anchor(message);
            self.deliver_voice_note(client, &message.recipient, &message.content, anchor)
                .await;
        }
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {
        let client = self.ensure_client().await?.clone();
        let user_id = client
            .user_id()
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure),
                    "matrix: client has no user_id after login"
                );
                anyhow::Error::msg("matrix: client has no user_id after login")
            })?
            .to_owned();
        let ctx = inbound::HandlerCtx {
            config: self.config.clone(),
            alias: self.alias.clone(),
            peer_resolver: self.peer_resolver.clone(),
            transcription: self.transcription.clone(),
            workspace_dir: self.workspace_dir.clone(),
            tx,
            pending_approvals: self.pending_approvals.clone(),
            threads_seen: self.threads_seen.clone(),
            bot_user_id: user_id,
            bot_display_name: self.bot_display_name.clone(),
            initial_sync_done: self.initial_sync_done.clone(),
            undecryptable_seen: self.undecryptable_seen.clone(),
        };
        inbound::run_sync_loop(client, ctx).await
    }

    async fn health_check(&self) -> bool {
        match self.client.get() {
            Some(c) => c.matrix_auth().logged_in() && self.initial_sync_done.load(Ordering::SeqCst),
            None => false,
        }
    }

    async fn start_typing(&self, recipient: &str) -> Result<()> {
        let client = self.ensure_client().await?;
        let id = client::resolve_room(client, &self.alias_cache, recipient).await?;
        if let Some(room) = client.get_room(&id) {
            let _ = room.typing_notice(true).await;
        }
        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> Result<()> {
        let client = self.ensure_client().await?;
        let id = client::resolve_room(client, &self.alias_cache, recipient).await?;
        if let Some(room) = client.get_room(&id) {
            let _ = room.typing_notice(false).await;
        }
        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        // The orchestrator's streaming pipeline is gated on this returning
        // true. Partial, SingleMessage, and MultiMessage all need streaming
        // setup; the channel decides internally whether to edit answer text,
        // edit progress only, or emit paragraphs.
        !matches!(self.config.stream_mode, MatrixStreamMode::Off)
    }

    fn supports_multi_message_streaming(&self) -> bool {
        matches!(self.config.stream_mode, MatrixStreamMode::MultiMessage)
    }

    fn multi_message_delay_ms(&self) -> u64 {
        self.config.multi_message_delay_ms
    }

    async fn send_draft(&self, message: &SendMessage) -> Result<Option<String>> {
        let client = self.ensure_client().await?;
        let room_id = streaming_room(&message.recipient)?;
        match self.config.stream_mode {
            MatrixStreamMode::Off => Ok(None),
            MatrixStreamMode::Partial => {
                // Send the placeholder draft now so subsequent update_draft
                // calls have an event to edit.
                let event_id = outbound::send(&self.outbox(client), message).await?;
                let thread_anchor =
                    outbound::thread_anchor_from_message(&self.outbox(client), message);
                let key = streaming::draft_key(room_id, event_id.as_ref())?;
                let mut state = self.streaming_state.write().await;
                streaming::insert_partial(
                    &mut state,
                    key,
                    streaming::PartialDraft {
                        event_id: event_id.clone(),
                        thread_anchor,
                        last_text: message.content.clone(),
                        last_edit: Instant::now(),
                    },
                )?;
                Ok(Some(event_id.to_string()))
            }
            MatrixStreamMode::SingleMessage => {
                // Single-message mode starts with one editable progress draft.
                // Final answer text is never copied here; finalize_draft sends
                // the answer as a separate Matrix message.
                let event_id = outbound::send(&self.outbox(client), message).await?;
                let thread_anchor =
                    outbound::thread_anchor_from_message(&self.outbox(client), message);
                let key = streaming::draft_key(room_id, event_id.as_ref())?;
                let first_edit_ready = Instant::now();
                let mut state = self.streaming_state.write().await;
                streaming::insert_single(
                    &mut state,
                    key,
                    streaming::SingleDraft {
                        event_id: event_id.clone(),
                        thread_anchor,
                        lines: Default::default(),
                        last_text: message.content.clone(),
                        last_edit: first_edit_ready,
                    },
                )?;
                Ok(Some(event_id.to_string()))
            }
            MatrixStreamMode::MultiMessage => {
                // No initial message — paragraphs are emitted by update_draft
                // as they appear. Capture the thread anchor up front so each
                // paragraph lands in the same thread as the user's message.
                let thread_anchor = message
                    .thread_ts
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .and_then(|s| s.parse::<OwnedEventId>().ok());
                let draft_id = streaming::new_multi_message_draft_id();
                let key = streaming::draft_key(room_id, &draft_id)?;
                let mut state = self.streaming_state.write().await;
                streaming::insert_multi(
                    &mut state,
                    key,
                    streaming::MultiDraft {
                        thread_anchor,
                        sent_so_far: 0,
                    },
                )?;
                Ok(Some(draft_id))
            }
        }
    }

    async fn update_draft(&self, recipient: &str, message_id: &str, text: &str) -> Result<()> {
        match self.config.stream_mode {
            MatrixStreamMode::Off => Ok(()),
            MatrixStreamMode::Partial => self.partial_update(recipient, message_id, text).await,
            MatrixStreamMode::SingleMessage => Ok(()),
            MatrixStreamMode::MultiMessage => self.multi_update(recipient, message_id, text).await,
        }
    }

    async fn update_draft_progress(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> Result<()> {
        match self.config.stream_mode {
            MatrixStreamMode::Partial => self.update_draft(recipient, message_id, text).await,
            MatrixStreamMode::SingleMessage => {
                self.single_update_progress(recipient, message_id, text)
                    .await
            }
            // MultiMessage doesn't have an in-flight draft to update, and Off
            // means the orchestrator should not have created one.
            MatrixStreamMode::Off | MatrixStreamMode::MultiMessage => Ok(()),
        }
    }

    async fn update_draft_progress_batch(
        &self,
        recipient: &str,
        message_id: &str,
        texts: &[String],
    ) -> Result<()> {
        match self.config.stream_mode {
            MatrixStreamMode::SingleMessage => {
                self.single_update_progress_batch(recipient, message_id, texts)
                    .await
            }
            MatrixStreamMode::Off | MatrixStreamMode::MultiMessage => Ok(()),
            MatrixStreamMode::Partial => {
                for text in texts {
                    self.update_draft_progress(recipient, message_id, text)
                        .await?;
                }
                Ok(())
            }
        }
    }

    async fn update_typed_draft_progress_batch(
        &self,
        recipient: &str,
        message_id: &str,
        progress: &[DraftProgress],
    ) -> Result<()> {
        match self.config.stream_mode {
            MatrixStreamMode::SingleMessage => {
                self.single_update_typed_progress_batch(recipient, message_id, progress)
                    .await
            }
            MatrixStreamMode::Off | MatrixStreamMode::MultiMessage => Ok(()),
            MatrixStreamMode::Partial => {
                for entry in progress {
                    self.update_draft_progress(recipient, message_id, &entry.text)
                        .await?;
                }
                Ok(())
            }
        }
    }

    async fn finalize_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
        suppress_voice: bool,
    ) -> Result<()> {
        let client = self.ensure_client().await?;
        let key = streaming_key(recipient, message_id)?;
        // Read before the draft is consumed below, so the voice note can be
        // threaded with the text it accompanies.
        let voice_anchor = self.draft_thread_anchor(recipient, message_id).await;
        let finalized = match self.config.stream_mode {
            MatrixStreamMode::Off => Ok(()),
            MatrixStreamMode::Partial => {
                let draft = {
                    let mut state = self.streaming_state.write().await;
                    streaming::take_partial(&mut state, &key)
                };
                if let Some(draft) = draft {
                    let room =
                        outbound::resolve_joined_room(client, &self.alias_cache, recipient).await?;
                    let (cleaned_text, markers) = markers::parse(text);
                    let delivery = outbound::deliver_attachments(
                        &self.outbox(client),
                        &room,
                        cleaned_text,
                        &markers,
                        &[],
                        draft.thread_anchor.as_ref(),
                    )
                    .await?;

                    match streaming::decide_partial_finalize_action(
                        delivery.text.trim().is_empty(),
                        delivery.last_attachment_id.is_some(),
                    ) {
                        streaming::PartialFinalizeAction::EditDraft => {
                            let kinds = delivery.failure_kinds();
                            let any_attachment_landed = delivery.last_attachment_id.is_some();
                            if let Err(edit_err) = outbound::edit(
                                client,
                                recipient,
                                &draft.event_id,
                                &delivery.text,
                                None,
                            )
                            .await
                            {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        ::serde_json::json!({"edit_err": edit_err.to_string()})
                                    ),
                                    "matrix: partial finalize edit failed: ; sending cleaned text fallback"
                                );
                                let mut fallback = SendMessage::new(&delivery.text, recipient);
                                fallback.thread_ts =
                                    draft.thread_anchor.as_ref().map(|e| e.to_string());
                                match outbound::send(&self.outbox(client), &fallback).await {
                                    Ok(fallback_id) => {
                                        outbound::emit_failure_reactions(
                                            &room,
                                            &fallback_id,
                                            &kinds,
                                        )
                                        .await;
                                    }
                                    Err(send_err) if any_attachment_landed => {
                                        ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"send_err": send_err.to_string()})), "matrix: partial finalize cleaned text fallback failed after attachment upload: ; suppressing error to avoid duplicate attachment retry");
                                    }
                                    Err(send_err) => {
                                        return Err(edit_err).with_context(|| {
                                            format!(
                                                "matrix: partial finalize cleaned text fallback failed: {send_err}"
                                            )
                                        });
                                    }
                                }
                            } else {
                                outbound::emit_failure_reactions(&room, &draft.event_id, &kinds)
                                    .await;
                            }
                        }
                        streaming::PartialFinalizeAction::RedactDraft => {
                            if let Err(err) = outbound::redact(
                                client,
                                recipient,
                                &draft.event_id,
                                Some("attachment-only response delivered".to_string()),
                            )
                            .await
                            {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(::serde_json::json!({"err": err.to_string()})),
                                    "matrix: partial finalize redaction failed after attachment-only upload: ; leaving placeholder to avoid duplicate attachment retry"
                                );
                            }
                        }
                        streaming::PartialFinalizeAction::EmptyError => {
                            ::clawcrew_log::record!(
                                WARN,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Reject
                                )
                                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                                .with_attrs(::serde_json::json!({"phase": "partial_finalize"})),
                                "matrix: empty partial draft body and no successful attachment"
                            );
                            return Err(anyhow::Error::msg(
                                "matrix: empty partial draft body and no successful attachment",
                            ));
                        }
                    }
                }
                Ok(())
            }
            MatrixStreamMode::SingleMessage => {
                let draft = {
                    let mut state = self.streaming_state.write().await;
                    streaming::take_single(&mut state, &key)
                };
                let plan = streaming::single_finalize_plan(
                    draft.is_some(),
                    self.config.stream_draft_delete,
                    !text.trim().is_empty(),
                );

                if plan.deletes_draft_first()
                    && let Some(draft) = draft.as_ref()
                {
                    // Matrix implements message deletion through redaction.
                    // Delete before the final send so the user's timeline
                    // lands on the final answer rather than a trailing
                    // "message deleted" event after it.
                    Self::redact_single_draft_before_final(
                        client,
                        recipient,
                        &draft.event_id,
                        "streaming draft replaced by final response",
                    )
                    .await;
                }

                if plan.keeps_draft()
                    && let Some(draft) = draft.as_ref()
                {
                    match streaming::single_retained_draft_action(
                        draft,
                        self.config.effective_message_max_bytes(),
                    ) {
                        streaming::SingleRetainedDraftAction::DeletePlaceholder => {
                            Self::redact_single_draft_before_final(
                                client,
                                recipient,
                                &draft.event_id,
                                "empty streaming draft removed before final response",
                            )
                            .await;
                        }
                        streaming::SingleRetainedDraftAction::Flush(visible_text) => {
                            if let Err(edit_err) = outbound::edit(
                                client,
                                recipient,
                                &draft.event_id,
                                &visible_text,
                                Some(self.config.effective_message_max_bytes()),
                            )
                            .await
                            {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(::serde_json::json!({"err": edit_err.to_string()})),
                                    "matrix: single-message retained draft flush failed before final send"
                                );
                                if draft.last_text == DRAFT_PLACEHOLDER {
                                    // Buffered progress is not durable until
                                    // an edit succeeds. Remove the still-
                                    // visible placeholder before sending the
                                    // final answer rather than retaining an
                                    // empty progress event.
                                    Self::redact_single_draft_before_final(
                                        client,
                                        recipient,
                                        &draft.event_id,
                                        "unpublished streaming draft removed before final response",
                                    )
                                    .await;
                                }
                                // Once a successful edit establishes a
                                // durable transcript, retention is
                                // best-effort. A stale retained draft is less
                                // harmful than suppressing the separate final
                                // Matrix answer, so final delivery proceeds.
                            }
                        }
                        streaming::SingleRetainedDraftAction::KeepCurrent => {}
                    }
                }

                if plan.sends_final() {
                    let mut msg = SendMessage::new(text, recipient);
                    msg.thread_ts = draft
                        .as_ref()
                        .and_then(|draft| draft.thread_anchor.as_ref())
                        .map(|e| e.to_string());
                    outbound::send(&self.final_outbox(client), &msg).await?;
                }
                Ok(())
            }
            MatrixStreamMode::MultiMessage => {
                // Drain the trailing paragraph (or whatever's left after the
                // last \n\n boundary) as one final message.
                let multi = {
                    let mut state = self.streaming_state.write().await;
                    streaming::take_multi(&mut state, &key)
                };
                let Some(state) = multi else {
                    return Ok(());
                };
                let remainder = if text.len() > state.sent_so_far {
                    text[state.sent_so_far..].trim().to_string()
                } else {
                    String::new()
                };
                if !remainder.is_empty() {
                    let mut msg = SendMessage::new(remainder, recipient);
                    msg.thread_ts = state.thread_anchor.as_ref().map(|e| e.to_string());
                    outbound::send(&self.outbox(client), &msg).await?;
                }
                Ok(())
            }
        };

        // `finalize_draft` carries no force_voice: the runtime routes those
        // through send_final instead. Errors skip this by returning early.
        if finalized.is_ok()
            && !suppress_voice
            && self.tts.is_some()
            && self.room_has_voice_peer(client, recipient).await
        {
            self.deliver_voice_note(client, recipient, text, voice_anchor)
                .await;
        }
        finalized
    }

    async fn cancel_draft(&self, recipient: &str, message_id: &str) -> Result<()> {
        let client = self.ensure_client().await?;
        let key = streaming_key(recipient, message_id)?;
        match self.config.stream_mode {
            MatrixStreamMode::Off => Ok(()),
            MatrixStreamMode::Partial => {
                let draft = {
                    let mut state = self.streaming_state.write().await;
                    streaming::take_partial(&mut state, &key)
                };
                if let Some(d) = draft {
                    let _ = outbound::redact(
                        client,
                        recipient,
                        &d.event_id,
                        Some("cancelled".to_string()),
                    )
                    .await;
                }
                Ok(())
            }
            MatrixStreamMode::SingleMessage => {
                let draft = {
                    let mut state = self.streaming_state.write().await;
                    streaming::take_single(&mut state, &key)
                };
                if let Some(draft) = draft
                    && streaming::single_cancel_deletes_draft(
                        &draft,
                        self.config.stream_draft_delete,
                    )
                {
                    let _ = outbound::redact(
                        client,
                        recipient,
                        &draft.event_id,
                        Some("cancelled".to_string()),
                    )
                    .await;
                }
                Ok(())
            }
            MatrixStreamMode::MultiMessage => {
                // Already-sent paragraphs are independent room messages and
                // are not redacted on cancel — partial output is preferable
                // to silent disappearance. Just drop our state.
                let mut state = self.streaming_state.write().await;
                streaming::take_multi(&mut state, &key);
                Ok(())
            }
        }
    }

    async fn add_reaction(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        if !self.ack_reactions {
            return Ok(());
        }
        let client = self.ensure_client().await?;
        let event_id: OwnedEventId = message_id.parse()?;
        outbound::react(&self.outbox(client), channel_id, &event_id, emoji).await
    }

    async fn remove_reaction(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        if !self.ack_reactions {
            return Ok(());
        }
        let client = self.ensure_client().await?;
        let event_id: OwnedEventId = message_id.parse()?;
        outbound::unreact(&self.outbox(client), channel_id, &event_id, emoji).await
    }

    async fn redact_message(
        &self,
        channel_id: &str,
        message_id: &str,
        reason: Option<String>,
    ) -> Result<()> {
        let client = self.ensure_client().await?;
        let event_id: OwnedEventId = message_id.parse()?;
        outbound::redact(client, channel_id, &event_id, reason).await
    }

    async fn create_room(&self, options: &RoomCreationOptions) -> Result<String> {
        let client = self.ensure_client().await?;
        let request = room_management::build_create_room_request(options)?;
        let room = client.create_room(request).await?;
        Ok(room.room_id().to_string())
    }

    async fn invite_user(&self, room_id: &str, user_id: &str) -> Result<()> {
        let client = self.ensure_client().await?;
        let request = room_management::build_invite_user_request(room_id, user_id)?;
        client.send(request).await?;
        Ok(())
    }

    /// Delegates to [`Self::request_approval_attributed`] and drops the
    /// provenance, so the prompt/timeout logic lives in exactly one place.
    async fn request_approval(
        &self,
        recipient: &str,
        request: &ChannelApprovalRequest,
    ) -> Result<Option<ChannelApprovalResponse>> {
        Ok(self
            .request_approval_attributed(recipient, request)
            .await?
            .map(|attributed| attributed.response))
    }

    async fn request_approval_attributed(
        &self,
        recipient: &str,
        request: &ChannelApprovalRequest,
    ) -> Result<Option<clawcrew_api::channel::AttributedApprovalResponse>> {
        let client = self.ensure_client().await?;
        let destination = client::resolve_room(client, &self.alias_cache, recipient)
            .await?
            .to_string();
        let token = approval::generate_token_default();
        let prompt = crate::util::build_approve_deny_approval_prompt(
            &token,
            &request.tool_name,
            &request.arguments_summary,
            request.position_counter(),
        );

        let (tx, rx) = oneshot::channel();
        self.pending_approvals.lock().await.insert(
            token.clone(),
            crate::util::PendingApproval {
                sender: tx,
                destination,
                tool_name: request.tool_name.clone(),
            },
        );

        let send_msg = approval::build_prompt_message(prompt, recipient);
        if let Err(e) = self.send(&send_msg).await {
            self.pending_approvals.lock().await.remove(&token);
            return Err(e);
        }

        let timeout = Duration::from_secs(self.config.approval_timeout_secs.max(1));
        let result = tokio::time::timeout(timeout, rx).await;
        if result.is_err() {
            self.pending_approvals.lock().await.remove(&token);
        }
        // Only the first arm is an operator decision; the other two are the
        // runtime denying because nobody replied, and must say so.
        match result {
            Ok(Ok(resp)) => Ok(Some(
                clawcrew_api::channel::AttributedApprovalResponse::operator(resp),
            )),
            Ok(Err(_)) => Ok(Some(
                clawcrew_api::channel::AttributedApprovalResponse::from_runtime(
                    ChannelApprovalResponse::Deny,
                    clawcrew_api::channel::ApprovalSource::Unreachable,
                ),
            )),
            Err(_) => Ok(Some(
                clawcrew_api::channel::AttributedApprovalResponse::from_runtime(
                    ChannelApprovalResponse::Deny,
                    clawcrew_api::channel::ApprovalSource::TimedOut,
                ),
            )),
        }
    }
}

fn streaming_room(recipient: &str) -> Result<OwnedRoomId> {
    recipient
        .parse::<OwnedRoomId>()
        .with_context(|| format!("parse recipient room id {recipient}"))
}

fn streaming_key(recipient: &str, message_id: &str) -> Result<streaming::DraftKey> {
    streaming::draft_key(streaming_room(recipient)?, message_id)
}

// ─── tests ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests;

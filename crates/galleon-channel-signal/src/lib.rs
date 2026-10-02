//! `galleon-channel-signal` — RF-B1 ported Signal channel.
//!
//! This crate splits the `signal` channel out of the monolithic
//! `clawcrew-channels` crate into an independent feature-crate. The Signal
//! **protocol** body is ported for real from `clawcrew-channels::signal`: the
//! signal-cli JSON-RPC transport (`rpc_request`), the SSE event stream parser
//! and reconnect loop (`listen`), envelope → [`ChannelMessage`] processing
//! (`process_envelope`), native polls / reactions / typing / health, and the
//! full [`Channel`] + [`Attributable`] trait surface.
//!
//! Two couplings in the upstream body reached INTO `clawcrew-channels`-internal
//! infrastructure. They were the RF-B1 split's real seams; both are now closed
//! by sharing [`galleon_channel_core`] (which reuses the `clawcrew_config`
//! peer-policy SSOT without dragging the runtime):
//!
//! 1. **Allowlist policy — CLOSED.** `is_sender_allowed` now delegates to
//!    [`galleon_channel_core::allowlist::is_user_allowed`], the full
//!    grant/deny/`!name`/wildcard peer-policy (`clawcrew_config::schema`
//!    semantics) rather than the earlier exact-match-plus-`*` stopgap.
//! 2. **Approval prompt — SHARED, i18n is the remaining ceiling.** Token
//!    generation, reply parsing and the prompt wire shape come from
//!    [`galleon_channel_core::approval`]. The prompt text is plain English of
//!    the IDENTICAL wire shape (`<token> yes|no|always`); localizing it needs
//!    `clawcrew_runtime::i18n`, so **i18n stays the documented ceiling**
//!    (`APPROVAL_I18N_CEILING` in core) — not closable without the runtime.
//!
//! One coupling remains fully a local simplification:
//!
//! * **Structured logging + env proxy.** Upstream uses the `clawcrew_log`
//!    structured-event macro and `clawcrew_config` env-proxy fallback. Here
//!    logging is dropped (a proof crate needs no event bus) and only an
//!    EXPLICIT per-channel `proxy_url` is honoured (plain `reqwest::Proxy`);
//!    the env-proxy fallback is the ceiling.
//!
//! Everything else is a faithful port. No production path uses `unwrap`/`expect`
//! on fallible IO; errors propagate via `anyhow::Result` + `?`.

use clawcrew_api::channel::{
    ApprovalSource, AttributedApprovalResponse, Channel, ChannelApprovalRequest,
    ChannelApprovalResponse, ChannelMessage, SendMessage,
};
// RF-B1 seam #1 (allowlist) + #2 (approval parse/token/prompt): shared from core.
use galleon_channel_core::allowlist::{self, Match};
use galleon_channel_core::approval::{build_yesno_prompt, new_approval_token, parse_approval_reply};
use futures_util::StreamExt;
use lru::LruCache;
use parking_lot::Mutex as SyncMutex;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

const GROUP_TARGET_PREFIX: &str = "group:";
const RECENT_TARGETS_CAPACITY: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecipientTarget {
    Direct(String),
    Group(String),
}

/// `(targetAuthor, targetTimestamp_ms)` recovered by `add_reaction` /
/// `remove_reaction` from an opaque inbound id. Held in `recent_targets`.
#[derive(Debug, Clone)]
struct ReactionTarget {
    author: String,
    timestamp_ms: u64,
}

/// Signal channel handle — a thin JSON-RPC/SSE client to a signal-cli daemon.
#[derive(Clone)]
pub struct SignalChannel {
    http_url: String,
    account: String,
    /// Empty = no group filter (all groups accepted).
    group_ids: Vec<String>,
    /// When true, accept only DMs and reject all group traffic.
    dm_only: bool,
    /// The alias key under `[channels.signal.<alias>]` this handle is bound to.
    alias: String,
    /// Resolves inbound external peers from canonical state at message-time.
    /// No cache (single source of truth).
    peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    ignore_attachments: bool,
    ignore_stories: bool,
    /// Per-channel explicit proxy URL. `None` = direct connection.
    /// ponytail: env-proxy fallback (upstream `clawcrew_config` runtime proxy)
    /// is the documented ceiling — only an explicit URL is honoured here.
    proxy_url: Option<String>,
    pending_approvals: Arc<Mutex<HashMap<String, oneshot::Sender<ChannelApprovalResponse>>>>,
    /// Seconds to wait for an operator reply before treating silence as deny.
    approval_timeout_secs: u64,
    /// Opaque inbound message id → `(targetAuthor, targetTimestamp)` so outbound
    /// reactions can be addressed without embedding the Signal sender in the id.
    recent_targets: Arc<SyncMutex<LruCache<String, ReactionTarget>>>,
}

// ── signal-cli SSE event JSON shapes (ported verbatim) ──────────

#[derive(Debug, Deserialize)]
struct SseEnvelope {
    #[serde(default)]
    envelope: Option<Envelope>,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    source: Option<String>,
    #[serde(rename = "sourceNumber", default)]
    source_number: Option<String>,
    #[serde(rename = "sourceUuid", default)]
    source_uuid: Option<String>,
    #[serde(rename = "dataMessage", default)]
    data_message: Option<DataMessage>,
    #[serde(rename = "storyMessage", default)]
    story_message: Option<serde_json::Value>,
    #[serde(default)]
    timestamp: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DataMessage {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    timestamp: Option<u64>,
    #[serde(rename = "groupInfo", default)]
    group_info: Option<GroupInfo>,
    #[serde(default)]
    attachments: Option<Vec<serde_json::Value>>,
    #[serde(rename = "pollAnswer", default)]
    poll_answer: Option<PollAnswer>,
    #[serde(rename = "pollVote", default)]
    poll_vote: Option<PollAnswer>,
}

#[derive(Debug, Deserialize)]
struct GroupInfo {
    #[serde(rename = "groupId", default)]
    group_id: Option<String>,
}

/// Inbound poll-vote payload.
#[derive(Debug, Clone, Deserialize)]
pub struct PollAnswer {
    #[serde(rename = "pollId", default)]
    pub poll_id: Option<u64>,
    #[serde(rename = "selectedIndices", alias = "optionIndexes", default)]
    pub selected_indices: Vec<u32>,
    #[serde(rename = "selectedTitles", default)]
    pub selected_titles: Vec<String>,
}

impl SignalChannel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        http_url: String,
        account: String,
        group_ids: Vec<String>,
        dm_only: bool,
        alias: impl Into<String>,
        peer_resolver: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
        ignore_attachments: bool,
        ignore_stories: bool,
    ) -> Self {
        let http_url = http_url.trim_end_matches('/').to_string();
        Self {
            http_url,
            account,
            group_ids,
            dm_only,
            alias: alias.into(),
            peer_resolver,
            ignore_attachments,
            ignore_stories,
            proxy_url: None,
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            approval_timeout_secs: 300,
            recent_targets: Arc::new(SyncMutex::new(LruCache::new(
                NonZeroUsize::new(RECENT_TARGETS_CAPACITY)
                    .expect("RECENT_TARGETS_CAPACITY is a non-zero constant"),
            ))),
        }
    }

    /// Alias under `[channels.signal.<alias>]` this handle is bound to.
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Set a per-channel explicit proxy URL.
    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    pub fn with_approval_timeout_secs(mut self, secs: u64) -> Self {
        self.approval_timeout_secs = secs;
        self
    }

    /// Build the reqwest client. Honours an explicit `proxy_url` via
    /// `reqwest::Proxy::all` (behaviour-equivalent to upstream's explicit-proxy
    /// path); the env-proxy fallback is the documented ceiling.
    fn http_client(&self) -> anyhow::Result<Client> {
        let mut builder = Client::builder().connect_timeout(Duration::from_secs(10));
        if let Some(url) = self.proxy_url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            builder = builder.proxy(reqwest::Proxy::all(url)?);
        }
        Ok(builder.build()?)
    }

    /// Effective sender: prefer `sourceNumber` (E.164), then `source`, then `sourceUuid`.
    fn sender(envelope: &Envelope) -> Option<String> {
        envelope
            .source_number
            .as_deref()
            .filter(|sender| !sender.is_empty())
            .or(envelope.source.as_deref().filter(|sender| !sender.is_empty()))
            .or(envelope
                .source_uuid
                .as_deref()
                .filter(|sender| !sender.is_empty()))
            .map(String::from)
    }

    /// Full grant/deny/`!name`/wildcard peer-policy via
    /// [`galleon_channel_core::allowlist`]. Signal senders are E.164 numbers or
    /// UUIDs, so the comparison is case-sensitive. This closed the former
    /// exact-match-plus-`*` ceiling: a `!number` deny now outranks a `*` grant.
    fn is_sender_allowed(&self, sender: &str) -> bool {
        let peers = (self.peer_resolver)();
        allowlist::is_user_allowed(&peers, sender, Match::Sensitive)
    }

    fn is_e164(recipient: &str) -> bool {
        let Some(number) = recipient.strip_prefix('+') else {
            return false;
        };
        (2..=15).contains(&number.len()) && number.chars().all(|c| c.is_ascii_digit())
    }

    fn is_uuid(s: &str) -> bool {
        Uuid::parse_str(s).is_ok()
    }

    fn parse_recipient_target(recipient: &str) -> RecipientTarget {
        if let Some(group_id) = recipient.strip_prefix(GROUP_TARGET_PREFIX) {
            return RecipientTarget::Group(group_id.to_string());
        }
        if Self::is_e164(recipient) || Self::is_uuid(recipient) {
            RecipientTarget::Direct(recipient.to_string())
        } else {
            RecipientTarget::Group(recipient.to_string())
        }
    }

    fn build_reaction_params(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
        remove: bool,
    ) -> anyhow::Result<serde_json::Value> {
        let target = self.recent_targets.lock().get(message_id).cloned().ok_or_else(|| {
            anyhow::Error::msg(format!(
                "no recent inbound Signal message matches id {message_id} — may have been evicted from the lookup cache or never received"
            ))
        })?;

        let params = match Self::parse_recipient_target(channel_id) {
            RecipientTarget::Direct(number) => serde_json::json!({
                "recipient": [number],
                "emoji": emoji,
                "targetAuthor": target.author,
                "targetTimestamp": target.timestamp_ms,
                "remove": remove,
                "account": &self.account,
            }),
            RecipientTarget::Group(group_id) => serde_json::json!({
                "groupId": group_id,
                "emoji": emoji,
                "targetAuthor": target.author,
                "targetTimestamp": target.timestamp_ms,
                "remove": remove,
                "account": &self.account,
            }),
        };
        Ok(params)
    }

    /// JSON-RPC params for signal-cli's native `sendPollCreate`.
    fn build_poll_params(
        &self,
        recipient: &str,
        question: &str,
        options: &[String],
        multiple_choice: bool,
    ) -> serde_json::Value {
        match Self::parse_recipient_target(recipient) {
            RecipientTarget::Direct(number) => serde_json::json!({
                "recipient": [number],
                "account": &self.account,
                "question": question,
                "option": options,
                "no-multi": !multiple_choice,
            }),
            RecipientTarget::Group(group_id) => serde_json::json!({
                "group-id": group_id,
                "account": &self.account,
                "question": question,
                "option": options,
                "no-multi": !multiple_choice,
            }),
        }
    }

    fn matches_group(&self, data_msg: &DataMessage) -> bool {
        let incoming_group = data_msg
            .group_info
            .as_ref()
            .and_then(|g| g.group_id.as_deref());

        if self.dm_only {
            return incoming_group.is_none();
        }
        if self.group_ids.is_empty() {
            return true;
        }
        match incoming_group {
            Some(gid) => self.group_ids.iter().any(|allowed| allowed == gid),
            None => true,
        }
    }

    /// Send target: group id or the sender's number.
    fn reply_target(&self, data_msg: &DataMessage, sender: &str) -> String {
        if let Some(group_id) = data_msg
            .group_info
            .as_ref()
            .and_then(|g| g.group_id.as_deref())
        {
            format!("{GROUP_TARGET_PREFIX}{group_id}")
        } else {
            sender.to_string()
        }
    }

    /// Send a JSON-RPC request to the signal-cli daemon.
    async fn rpc_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let url = format!("{}/api/v1/rpc", self.http_url);
        let id = Uuid::new_v4().to_string();
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": id,
        });

        let resp = self
            .http_client()?
            .post(&url)
            .timeout(Duration::from_secs(30))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        // 201 = success with no body (e.g. typing indicators)
        if resp.status().as_u16() == 201 {
            return Ok(None);
        }

        let text = resp.text().await?;
        if text.is_empty() {
            return Ok(None);
        }

        let parsed: serde_json::Value = serde_json::from_str(&text)?;
        if let Some(err) = parsed.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown");
            anyhow::bail!("Signal RPC error {code}: {msg}");
        }
        Ok(parsed.get("result").cloned())
    }

    /// Process a single SSE envelope into 0+ [`ChannelMessage`]s.
    fn process_envelope(&self, envelope: &Envelope) -> Vec<ChannelMessage> {
        if self.ignore_stories && envelope.story_message.is_some() {
            return Vec::new();
        }
        let Some(data_msg) = envelope.data_message.as_ref() else {
            return Vec::new();
        };

        if self.ignore_attachments {
            let has_attachments = data_msg.attachments.as_ref().is_some_and(|a| !a.is_empty());
            if has_attachments
                && data_msg.message.is_none()
                && data_msg.poll_answer.is_none()
                && data_msg.poll_vote.is_none()
            {
                return Vec::new();
            }
        }

        // ponytail: upstream logs a structured DEBUG event here; dropped.
        let Some(sender) = Self::sender(envelope) else {
            return Vec::new();
        };

        if !self.is_sender_allowed(&sender) {
            return Vec::new();
        }
        if !self.matches_group(data_msg) {
            return Vec::new();
        }

        let target = self.reply_target(data_msg, &sender);

        let timestamp = data_msg.timestamp.or(envelope.timestamp).unwrap_or_else(|| {
            u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
            )
            .unwrap_or(u64::MAX)
        });

        let contents: Vec<String> = if let Some(pa) =
            data_msg.poll_answer.as_ref().or(data_msg.poll_vote.as_ref())
        {
            if !pa.selected_titles.is_empty() {
                pa.selected_titles.iter().map(|t| format!("[choice]{t}")).collect()
            } else if !pa.selected_indices.is_empty() {
                pa.selected_indices
                    .iter()
                    .map(|i| format!("[choice-index]{}", i + 1))
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            data_msg
                .message
                .as_deref()
                .filter(|t| !t.is_empty())
                .map(|t| vec![t.to_string()])
                .unwrap_or_default()
        };

        contents
            .into_iter()
            .enumerate()
            .map(|(idx, content)| {
                let id = format!("sig_{timestamp}_{}_{}", idx, Self::random_id_suffix());
                self.recent_targets.lock().put(
                    id.clone(),
                    ReactionTarget {
                        author: sender.clone(),
                        timestamp_ms: timestamp,
                    },
                );
                ChannelMessage {
                    id,
                    sender: sender.clone(),
                    reply_target: target.clone(),
                    content,
                    channel: "signal".to_string(),
                    channel_alias: Some(self.alias.clone()),
                    timestamp: timestamp / 1000, // millis -> secs
                    thread_ts: None,
                    interruption_scope_id: None,
                    attachments: vec![],
                    subject: None,
                    ..Default::default()
                }
            })
            .collect()
    }

    fn random_id_suffix() -> String {
        use rand::RngExt;
        const CHARSET: &[u8] = b"0123456789abcdef";
        let mut rng = rand::rng();
        (0..6)
            .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
            .collect()
    }

    /// Send a native multiple-choice poll (signal-cli `sendPollCreate`).
    pub async fn send_poll(
        &self,
        recipient: &str,
        question: &str,
        options: &[String],
        multiple_choice: bool,
    ) -> anyhow::Result<()> {
        if options.len() < 2 {
            anyhow::bail!(
                "Signal poll requires at least 2 options (got {}); render as text instead",
                options.len()
            );
        }
        let params = self.build_poll_params(recipient, question, options, multiple_choice);
        self.rpc_request("sendPollCreate", params).await?;
        Ok(())
    }
}

impl clawcrew_api::attribution::Attributable for SignalChannel {
    fn role(&self) -> clawcrew_api::attribution::Role {
        clawcrew_api::attribution::Role::Channel(clawcrew_api::attribution::ChannelKind::Signal)
    }
    fn alias(&self) -> &str {
        &self.alias
    }
}

#[async_trait::async_trait]
impl Channel for SignalChannel {
    fn name(&self) -> &str {
        "signal"
    }

    /// A Signal DM carries the bare sender as `reply_target`; a group message
    /// carries `group:<id>`. A `Direct` target is a DM.
    fn is_direct_message(&self, msg: &ChannelMessage) -> bool {
        matches!(
            Self::parse_recipient_target(&msg.reply_target),
            RecipientTarget::Direct(_)
        )
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let params = match Self::parse_recipient_target(&message.recipient) {
            RecipientTarget::Direct(number) => serde_json::json!({
                "recipient": [number],
                "message": &message.content,
                "account": &self.account,
            }),
            RecipientTarget::Group(group_id) => serde_json::json!({
                "groupId": group_id,
                "message": &message.content,
                "account": &self.account,
            }),
        };
        self.rpc_request("send", params).await?;
        Ok(())
    }

    async fn send_choice(
        &self,
        recipient: &str,
        prompt: &str,
        options: &[(String, String)],
    ) -> anyhow::Result<()> {
        let trimmed_prompt = prompt.trim();
        if options.is_empty() {
            if trimmed_prompt.is_empty() {
                return Ok(());
            }
            return self.send(&SendMessage::new(trimmed_prompt, recipient)).await;
        }
        if options.len() >= 2 {
            let labels: Vec<String> = options.iter().map(|(_, l)| l.clone()).collect();
            return self.send_poll(recipient, prompt, &labels, false).await;
        }
        // Single-option text fallback.
        let mut text = String::new();
        if !trimmed_prompt.is_empty() {
            text.push_str(trimmed_prompt);
            text.push_str("\n\n");
        }
        text.push_str("(reply with name or number)\n");
        for (idx, (_id, label)) in options.iter().enumerate() {
            text.push_str(&format!("{}. {}\n", idx + 1, label.trim()));
        }
        let trimmed = text.trim_end().to_string();
        self.send(&SendMessage::new(trimmed, recipient)).await
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut url = reqwest::Url::parse(&format!("{}/api/v1/events", self.http_url))?;
        url.query_pairs_mut().append_pair("account", &self.account);

        let mut retry_delay_secs = 2u64;
        let max_delay_secs = 60u64;

        loop {
            let resp = self
                .http_client()?
                .get(url.clone())
                .header("Accept", "text/event-stream")
                .send()
                .await;

            let resp = match resp {
                Ok(r) if r.status().is_success() => r,
                Ok(_) | Err(_) => {
                    // ponytail: upstream logs status/body/error; dropped here.
                    tokio::time::sleep(Duration::from_secs(retry_delay_secs)).await;
                    retry_delay_secs = (retry_delay_secs * 2).min(max_delay_secs);
                    continue;
                }
            };

            retry_delay_secs = 2;

            let mut bytes_stream = resp.bytes_stream();
            let mut buffer = String::new();
            let mut current_data = String::new();

            while let Some(chunk) = bytes_stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => break, // reconnect
                };
                let text = match String::from_utf8(chunk.to_vec()) {
                    Ok(t) => t,
                    Err(_) => continue, // skip invalid-UTF8 chunk
                };
                buffer.push_str(&text);

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.starts_with(':') {
                        continue; // SSE keepalive comment
                    }
                    if line.is_empty() {
                        // event boundary — dispatch accumulated data
                        if !current_data.is_empty() {
                            if let Ok(sse) = serde_json::from_str::<SseEnvelope>(&current_data) {
                                if let Some(ref envelope) = sse.envelope {
                                    if self.dispatch_envelope(envelope, &tx).await? {
                                        return Ok(());
                                    }
                                }
                            }
                            current_data.clear();
                        }
                    } else if let Some(data) = line.strip_prefix("data:") {
                        if !current_data.is_empty() {
                            current_data.push('\n');
                        }
                        current_data.push_str(data.trim_start());
                    }
                }
            }

            // flush any trailing event the stream ended mid-dispatch
            if !current_data.is_empty() {
                if let Ok(sse) = serde_json::from_str::<SseEnvelope>(&current_data) {
                    if let Some(ref envelope) = sse.envelope {
                        if self.dispatch_envelope(envelope, &tx).await? {
                            return Ok(());
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/api/v1/check", self.http_url);
        let Ok(client) = self.http_client() else {
            return false;
        };
        let Ok(resp) = client.get(&url).timeout(Duration::from_secs(10)).send().await else {
            return false;
        };
        resp.status().is_success()
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        let params = match Self::parse_recipient_target(recipient) {
            RecipientTarget::Direct(number) => serde_json::json!({
                "recipient": [number],
                "account": &self.account,
            }),
            RecipientTarget::Group(group_id) => serde_json::json!({
                "groupId": group_id,
                "account": &self.account,
            }),
        };
        self.rpc_request("sendTyping", params).await?;
        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        // signal-cli has no stop-typing RPC; typing auto-expires ~15s client-side.
        Ok(())
    }

    async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let params = self.build_reaction_params(channel_id, message_id, emoji, false)?;
        self.rpc_request("sendReaction", params).await?;
        Ok(())
    }

    async fn remove_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let params = self.build_reaction_params(channel_id, message_id, emoji, true)?;
        self.rpc_request("sendReaction", params).await?;
        Ok(())
    }

    async fn request_approval(
        &self,
        recipient: &str,
        request: &ChannelApprovalRequest,
    ) -> anyhow::Result<Option<ChannelApprovalResponse>> {
        Ok(self
            .request_approval_attributed(recipient, request)
            .await?
            .map(|attributed| attributed.response))
    }

    async fn request_approval_attributed(
        &self,
        recipient: &str,
        request: &ChannelApprovalRequest,
    ) -> anyhow::Result<Option<AttributedApprovalResponse>> {
        let token = new_approval_token();
        let text = build_yesno_prompt(
            &token,
            &request.tool_name,
            &request.arguments_summary,
            request.position_counter(),
        );

        let (tx, rx) = oneshot::channel();
        self.pending_approvals.lock().await.insert(token.clone(), tx);

        if let Err(err) = self.send(&SendMessage::new(text, recipient)).await {
            self.pending_approvals.lock().await.remove(&token);
            return Err(err);
        }

        // Only a real token-echo reply is an operator decision; the
        // dropped-sender and timeout arms are the runtime denying on its own.
        let attributed =
            match tokio::time::timeout(Duration::from_secs(self.approval_timeout_secs), rx).await {
                Ok(Ok(resp)) => AttributedApprovalResponse::operator(resp),
                Ok(Err(_)) => {
                    self.pending_approvals.lock().await.remove(&token);
                    AttributedApprovalResponse::from_runtime(
                        ChannelApprovalResponse::Deny,
                        ApprovalSource::Unreachable,
                    )
                }
                Err(_) => {
                    self.pending_approvals.lock().await.remove(&token);
                    AttributedApprovalResponse::from_runtime(
                        ChannelApprovalResponse::Deny,
                        ApprovalSource::TimedOut,
                    )
                }
            };
        Ok(Some(attributed))
    }
}

impl SignalChannel {
    /// Route one envelope's messages to `tx`, intercepting approval replies.
    /// Returns `Ok(true)` when the receiver dropped and `listen` should stop.
    async fn dispatch_envelope(
        &self,
        envelope: &Envelope,
        tx: &mpsc::Sender<ChannelMessage>,
    ) -> anyhow::Result<bool> {
        for msg in self.process_envelope(envelope) {
            if let Some((token, response)) = parse_approval_reply(&msg.content) {
                let mut map = self.pending_approvals.lock().await;
                if let Some(sender) = map.remove(&token) {
                    let _ = sender.send(response);
                    continue;
                }
            }
            if tx.send(msg).await.is_err() {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channel() -> SignalChannel {
        SignalChannel::new(
            "http://127.0.0.1:8686/".to_string(),
            "+1234567890".to_string(),
            Vec::new(),
            false,
            "signal_test_alias",
            Arc::new(|| vec!["+1111111111".into()]),
            false,
            false,
        )
    }

    #[test]
    fn skeleton_is_a_channel() {
        let ch = make_channel();
        let dyn_ch: &dyn Channel = &ch;
        assert_eq!(dyn_ch.name(), "signal");
        assert_eq!(ch.alias(), "signal_test_alias");
        assert!(matches!(
            clawcrew_api::attribution::Attributable::role(&ch),
            clawcrew_api::attribution::Role::Channel(
                clawcrew_api::attribution::ChannelKind::Signal
            )
        ));
    }

    #[test]
    fn trailing_slash_stripped_and_allowlist() {
        let ch = make_channel();
        assert_eq!(ch.http_url, "http://127.0.0.1:8686");
        assert!(ch.is_sender_allowed("+1111111111"));
        assert!(!ch.is_sender_allowed("+9999999999"));
    }

    #[test]
    fn wildcard_allows_anyone() {
        let ch = SignalChannel::new(
            "http://127.0.0.1:8686".into(),
            "+1234567890".into(),
            Vec::new(),
            false,
            "a",
            Arc::new(|| vec!["*".into()]),
            false,
            false,
        );
        assert!(ch.is_sender_allowed("+9999999999"));
    }

    #[test]
    fn recipient_target_classification() {
        assert_eq!(
            SignalChannel::parse_recipient_target("+14155550123"),
            RecipientTarget::Direct("+14155550123".into())
        );
        assert_eq!(
            SignalChannel::parse_recipient_target("group:abc=="),
            RecipientTarget::Group("abc==".into())
        );
        // Non-E164, non-UUID, no group prefix → treated as group id.
        assert_eq!(
            SignalChannel::parse_recipient_target("somegroupid"),
            RecipientTarget::Group("somegroupid".into())
        );
    }

    #[test]
    fn approval_reply_roundtrips_the_prompt_token() {
        let token = new_approval_token();
        let prompt = build_yesno_prompt(&token, "shell", "ls -la", Some((2, 3)));
        assert!(prompt.contains(&format!("[{token}]")));
        assert!(prompt.contains("Tool call 2 of 3"));
        // The yes/no/always keywords in the prompt must parse back out.
        let (parsed, resp) = parse_approval_reply(&format!("{token} yes")).expect("parse yes");
        assert_eq!(parsed, token);
        assert!(matches!(resp, ChannelApprovalResponse::Approve));
        assert!(matches!(
            parse_approval_reply(&format!("{token} always")).unwrap().1,
            ChannelApprovalResponse::AlwaysApprove
        ));
        assert!(parse_approval_reply("notoken yes").is_none());
    }

    #[test]
    fn poll_params_shape() {
        let ch = make_channel();
        let direct = ch.build_poll_params("+14155550123", "Q?", &["a".into(), "b".into()], false);
        assert_eq!(direct["no-multi"], serde_json::json!(true));
        assert!(direct.get("recipient").is_some());
        let group = ch.build_poll_params("group:gid", "Q?", &["a".into(), "b".into()], true);
        assert_eq!(group["no-multi"], serde_json::json!(false));
        assert_eq!(group["group-id"], serde_json::json!("gid"));
    }
}

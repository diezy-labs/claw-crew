use super::AnthropicRefusalError;
use super::ModelProvider;
use super::dispatch::{
    AcceptedRoute, AccountedCallReport, ProviderDispatch, current_dispatch_billable_usage,
    mark_current_dispatch_composite, stream_as_dispatch_composite,
    stream_with_exact_dispatch_route, with_exact_dispatch_route,
};
use super::safeguard_notice::{
    SafeguardFallbackKind, SafeguardFallbackNotice, commit_safeguard_fallback,
    take_last_safeguard_fallback,
};
use super::traits::{
    ChatMessage, ChatRequest, ChatResponse, StreamChunk, StreamError, StreamEvent, StreamOptions,
    StreamResult, TokenUsage,
};
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use parking_lot::Mutex as ParkingMutex;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Info about a model_provider fallback that occurred during a request.
#[derive(Debug, Clone)]
pub struct ProviderFallbackInfo {
    /// ModelProvider family that was originally requested.
    pub requested_provider: String,
    /// Model that was originally requested.
    pub requested_model: String,
    /// ModelProvider family that actually served the request.
    pub actual_provider: String,
    /// Model that actually served the request.
    pub actual_model: String,
}

/// Fallback metadata for a caller that needs exact configured-candidate
/// provenance in addition to the stable provider-family display fields.
#[derive(Debug, Clone)]
pub struct ProviderFallbackAttribution {
    /// Stable provider/model record for channel and direct-agent notices.
    pub fallback: ProviderFallbackInfo,
    /// Exact configured candidate that was requested before fallback.
    pub requested_candidate: String,
    /// Exact configured candidate that served the recovered request.
    pub actual_candidate: String,
}

tokio::task_local! {
    static PROVIDER_FALLBACK: RefCell<Option<ProviderFallbackAttribution>>;
}

tokio::task_local! {
    static PROVIDER_CONTEXT_TRUNCATED: RefCell<bool>;
}

tokio::task_local! {
    static RELIABLE_CALL_ACCOUNTING: Arc<ParkingMutex<ReliableCallAccounting>>;
}

tokio::task_local! {
    static STREAM_REFUSAL_RECOVERY: RefCell<Option<AnthropicRefusalError>>;
}

/// Seed a non-streaming recovery with the refusal that ended a pre-output
/// stream. Reliable consumes it to skip the exact already-billed candidate;
/// a direct Anthropic provider consumes it to return the same refusal without
/// replaying the HTTP request.
pub(crate) async fn scope_stream_refusal_recovery<F: std::future::Future>(
    refusal: AnthropicRefusalError,
    future: F,
) -> F::Output {
    STREAM_REFUSAL_RECOVERY
        .scope(RefCell::new(Some(refusal)), future)
        .await
}

pub(crate) fn take_stream_refusal_recovery() -> Option<AnthropicRefusalError> {
    STREAM_REFUSAL_RECOVERY
        .try_with(|cell| cell.borrow_mut().take())
        .ok()
        .flatten()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReliableEntryId {
    model_slot: usize,
    entry_index: usize,
}

/// Explicit outcome of the retry policy for one entry. Returned by the pure
/// [`ReliableModelProvider::stream_recovery_decision`]; callers must not infer
/// precedence from branch order — read the `match` arms instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryDecision {
    /// Attempt the entry with the given retry budget.
    Admit(u32),
    /// Skip the entry entirely (avoids replaying a failed stream entry).
    Skip,
}

/// Call-scoped outcome retained independently of the provider result.
///
/// In particular, callers must extract it before propagating an error: a
/// rejected attempt can have been billed even when Reliable eventually fails.
#[derive(Debug, Default)]
pub(crate) struct ReliableCallAccounting {
    accepted_route: Option<AcceptedRoute>,
    stream_resume_after: Option<ReliableEntryId>,
    stream_recovery_semantic_empty: bool,
    stream_recovery_semantic_empty_permission: bool,
    stream_recovery_failure: Option<ProviderErrorDiagnostic>,
}

impl ReliableCallAccounting {
    /// Transfer the selected stream's provisional physical attempt to the
    /// runtime semantic classifier. It becomes a rejected report only when
    /// that classifier rejects the completed stream response.
    #[doc(hidden)]
    fn into_report(mut self) -> AccountedCallReport {
        AccountedCallReport::new(self.accepted_route.take())
    }
}

/// An opaque per-call accounting scope for the landed dispatch seam.
///
/// The runtime retains this handle across cancellation so a dropped provider
/// future cannot erase reports from attempts that had already completed.
#[derive(Clone, Debug, Default)]
pub(crate) struct ReliableCallAccountingScope {
    accounting: Arc<ParkingMutex<ReliableCallAccounting>>,
}

impl ReliableCallAccountingScope {
    pub(crate) async fn scope<F: std::future::Future>(&self, future: F) -> F::Output {
        RELIABLE_CALL_ACCOUNTING
            .scope(self.accounting.clone(), future)
            .await
    }

    pub(crate) fn take(&self) -> AccountedCallReport {
        std::mem::take(&mut *self.accounting.lock()).into_report()
    }
}

#[cfg(test)]
async fn scope_reliable_call_accounting<F: std::future::Future>(
    future: F,
) -> (F::Output, AccountedCallReport) {
    let scope = ReliableCallAccountingScope::default();
    let output = scope.scope(future).await;
    (output, scope.take())
}

fn accounted_rejected_attempt_usage() -> Option<TokenUsage> {
    current_dispatch_billable_usage()
}

/// Preserve Reliable's exact-entry recovery policy, but only once the selected
/// stream has actually been polled.  Billing ownership belongs to dispatch;
/// this is continuation state, not an attempt record.
fn activate_stream_recovery_after_first_poll(model_slot: usize, entry_index: usize) {
    let _ = RELIABLE_CALL_ACCOUNTING.try_with(|accounting| {
        accounting.lock().stream_resume_after = Some(ReliableEntryId {
            model_slot,
            entry_index,
        });
    });
}

fn stream_with_recovery_identity<T>(
    stream: stream::BoxStream<'static, StreamResult<T>>,
    model_slot: usize,
    entry_index: usize,
) -> stream::BoxStream<'static, StreamResult<T>>
where
    T: Send + 'static,
{
    let mut stream = stream;
    let mut started = false;
    stream::poll_fn(move |cx| {
        if !started {
            started = true;
            activate_stream_recovery_after_first_poll(model_slot, entry_index);
        }
        stream.as_mut().poll_next(cx)
    })
    .boxed()
}

/// A later direct/primary recovery supersedes a stream's provisional fallback
/// candidate. Presentation still waits for runtime semantic acceptance.
pub(crate) fn clear_provisional_provider_route() {
    let _ = RELIABLE_CALL_ACCOUNTING.try_with(|accounting| accounting.lock().accepted_route = None);
}

fn record_accepted_attempt(
    entry: &ReliableModelProviderEntry,
    model: &str,
    fallback: Option<ProviderFallbackAttribution>,
) {
    let route = AcceptedRoute::new(
        entry.cooldown_key.clone(),
        entry.served_model(model).to_string(),
        fallback,
    );
    let _ = RELIABLE_CALL_ACCOUNTING
        .try_with(|accounting| accounting.lock().accepted_route = Some(route));
}

fn has_reliable_call_accounting() -> bool {
    RELIABLE_CALL_ACCOUNTING.try_with(|_| ()).is_ok()
}

pub(crate) fn mark_stream_recovery_semantic_empty() {
    let _ = RELIABLE_CALL_ACCOUNTING.try_with(|accounting| {
        let mut accounting = accounting.lock();
        accounting.stream_recovery_semantic_empty = true;
        accounting.stream_recovery_semantic_empty_permission = true;
    });
}

/// Preserve the classified stream failure while runtime attempts eligible
/// non-streaming recovery candidates.
pub(crate) fn record_stream_recovery_failure(error: &anyhow::Error) {
    let _ = RELIABLE_CALL_ACCOUNTING.try_with(|accounting| {
        accounting.lock().stream_recovery_failure = Some(provider_error_diagnostic(error));
    });
}

fn stream_recovery_was_semantic_empty() -> bool {
    RELIABLE_CALL_ACCOUNTING
        .try_with(|accounting| accounting.lock().stream_recovery_semantic_empty)
        .unwrap_or(false)
}

fn stream_recovery_failure_diagnostic() -> Option<ProviderErrorDiagnostic> {
    RELIABLE_CALL_ACCOUNTING
        .try_with(|accounting| accounting.lock().stream_recovery_failure.clone())
        .ok()
        .flatten()
}

/// Take (consume) the last model_provider fallback info, if any.
/// Must be called within a `scope_provider_fallback` scope.
pub fn take_last_provider_fallback() -> Option<ProviderFallbackInfo> {
    PROVIDER_FALLBACK
        .try_with(|cell| cell.borrow_mut().take())
        .ok()
        .flatten()
        .map(|attribution| attribution.fallback)
}

/// Take fallback metadata including the exact configured candidates.
pub fn take_last_provider_fallback_attribution() -> Option<ProviderFallbackAttribution> {
    PROVIDER_FALLBACK
        .try_with(|cell| cell.borrow_mut().take())
        .ok()
        .flatten()
}

/// Take whether Reliable shortened the provider-visible transcript while
/// recovering the current request from a context-window error.
///
/// This is intentionally distinct from fallback attribution: retrying the
/// same candidate must not render a fallback notice, but the caller must not
/// cache that response under the untrimmed request transcript.
pub fn take_last_provider_context_truncation() -> bool {
    PROVIDER_CONTEXT_TRUNCATED
        .try_with(|cell| std::mem::take(&mut *cell.borrow_mut()))
        .unwrap_or(false)
}

fn record_provider_context_truncation() {
    let _ = PROVIDER_CONTEXT_TRUNCATED.try_with(|cell| *cell.borrow_mut() = true);
}

/// Record the fallback that served the current successful provider request, or
/// clear stale attribution when the primary served it.
///
/// A fallback scope can span an agentic tool loop, which issues several model
/// requests. The caller-visible result must describe the request that produced
/// the final response, not an earlier request that only produced a tool call.
fn record_successful_provider_fallback(record: Option<&ProviderFallbackRecord>) {
    if let Some(record) = record {
        record.record();
    } else {
        let _ = PROVIDER_FALLBACK.try_with(|cell| *cell.borrow_mut() = None);
    }
}

/// Commit the route of a response the runtime has accepted semantically.
/// A primary/direct accepted response intentionally clears an earlier fallback
/// candidate in the same outer delivery scope.
pub(crate) fn commit_accepted_provider_route(route: Option<ProviderFallbackAttribution>) {
    let _ = PROVIDER_FALLBACK.try_with(|cell| *cell.borrow_mut() = route);
}

/// Run the given future within a provider-fallback scope.
/// Both `record_provider_fallback` (inside ReliableModelProvider) and
/// `take_last_provider_fallback` (post-loop channel code) must execute
/// within this scope for the data to be visible.
pub async fn scope_provider_fallback<F: std::future::Future>(future: F) -> F::Output {
    PROVIDER_FALLBACK
        .scope(
            RefCell::new(None),
            PROVIDER_CONTEXT_TRUNCATED.scope(RefCell::new(false), future),
        )
        .await
}

/// Record a model_provider fallback event.
/// No-ops when called outside a `scope_provider_fallback` scope.
fn record_provider_fallback(
    requested_provider: &str,
    requested_model: &str,
    actual_provider: &str,
    actual_model: &str,
    requested_candidate: &str,
    actual_candidate: &str,
) -> ProviderFallbackInfo {
    let fallback = ProviderFallbackInfo {
        requested_provider: requested_provider.to_string(),
        requested_model: requested_model.to_string(),
        actual_provider: actual_provider.to_string(),
        actual_model: actual_model.to_string(),
    };
    let _ = PROVIDER_FALLBACK.try_with(|cell| {
        *cell.borrow_mut() = Some(ProviderFallbackAttribution {
            fallback: fallback.clone(),
            requested_candidate: requested_candidate.to_string(),
            actual_candidate: actual_candidate.to_string(),
        });
    });
    // An accounted runtime call owns presentation timing. Legacy direct trait
    // callers still receive the historical immediate recovery record.
    if !has_reliable_call_accounting() {
        commit_accepted_provider_route(Some(ProviderFallbackAttribution {
            fallback: fallback.clone(),
            requested_candidate: requested_candidate.to_string(),
            actual_candidate: actual_candidate.to_string(),
        }));
    }
    fallback
}

/// Retain refusal accounting and presentation data independently from the
/// terminal cause. A later candidate failure may replace the terminal error,
/// while the billed usage and a successful-rescue notice must still survive.
fn remember_refusal(
    refusal_seen: &mut Option<AnthropicRefusalError>,
    rejected_attempt_usage: &mut Option<TokenUsage>,
    error: &anyhow::Error,
) {
    if let Some(refusal) = error.downcast_ref::<AnthropicRefusalError>() {
        accumulate_usage(rejected_attempt_usage, refusal.usage.as_deref());
        if refusal_seen.is_none() {
            *refusal_seen = Some(refusal.clone());
        }
    }
}

fn record_refusal_rescue(
    refusal_seen: &Option<AnthropicRefusalError>,
    requested_model: &str,
    served_model: &str,
) {
    let Some(refusal) = refusal_seen else {
        return;
    };
    let server_notice = take_last_safeguard_fallback()
        .filter(|notice| notice.kind == SafeguardFallbackKind::ServerSide);
    commit_safeguard_fallback(Some(SafeguardFallbackNotice {
        kind: if server_notice.is_some() {
            SafeguardFallbackKind::ClientAndServer
        } else {
            SafeguardFallbackKind::ClientSide
        },
        requested_model: requested_model.to_string(),
        served_model: server_notice
            .map(|notice| notice.served_model)
            .unwrap_or_else(|| served_model.to_string()),
        category: refusal.category.clone(),
    }));
}

struct ProviderFallbackRecord {
    requested_provider: String,
    requested_model: String,
    actual_provider: String,
    actual_model: String,
    requested_candidate: String,
    actual_candidate: String,
}

impl ProviderFallbackRecord {
    fn new_if_true_fallback(
        requested_provider: &str,
        requested_model: &str,
        actual_provider: &str,
        actual_model: &str,
        used_later_candidate: bool,
        requested_candidate: &str,
        actual_candidate: &str,
    ) -> Option<Self> {
        if !used_later_candidate && requested_model == actual_model {
            return None;
        }

        Some(Self {
            requested_provider: requested_provider.to_string(),
            requested_model: requested_model.to_string(),
            actual_provider: actual_provider.to_string(),
            actual_model: actual_model.to_string(),
            requested_candidate: requested_candidate.to_string(),
            actual_candidate: actual_candidate.to_string(),
        })
    }

    fn record(&self) {
        record_provider_fallback(
            &self.requested_provider,
            &self.requested_model,
            &self.actual_provider,
            &self.actual_model,
            &self.requested_candidate,
            &self.actual_candidate,
        );
    }

    fn info(&self) -> ProviderFallbackInfo {
        ProviderFallbackInfo {
            requested_provider: self.requested_provider.clone(),
            requested_model: self.requested_model.clone(),
            actual_provider: self.actual_provider.clone(),
            actual_model: self.actual_model.clone(),
        }
    }

    fn attribution(&self) -> ProviderFallbackAttribution {
        ProviderFallbackAttribution {
            fallback: self.info(),
            requested_candidate: self.requested_candidate.clone(),
            actual_candidate: self.actual_candidate.clone(),
        }
    }
}

fn stream_with_success_recording<T, IsFinal>(
    stream: stream::BoxStream<'static, StreamResult<T>>,
    fallback_record: Option<ProviderFallbackRecord>,
    accepted_route: AcceptedRoute,
    is_final: IsFinal,
) -> stream::BoxStream<'static, StreamResult<T>>
where
    T: Send + 'static,
    IsFinal: Fn(&T) -> bool + Send + 'static,
{
    stream::unfold(
        (
            stream,
            fallback_record,
            accepted_route,
            false,
            false,
            is_final,
        ),
        |(mut stream, fallback_record, accepted_route, saw_error, recorded, is_final)| async move {
            match stream.next().await {
                Some(event) => {
                    let mut saw_error = saw_error;
                    let mut recorded = recorded;
                    match &event {
                        Ok(value) if !saw_error && !recorded && is_final(value) => {
                            record_successful_provider_fallback(fallback_record.as_ref());
                            record_accepted_route(accepted_route.clone());
                            recorded = true;
                        }
                        Err(_) => {
                            saw_error = true;
                        }
                        Ok(_) => {}
                    }
                    Some((
                        event,
                        (
                            stream,
                            fallback_record,
                            accepted_route,
                            saw_error,
                            recorded,
                            is_final,
                        ),
                    ))
                }
                None => None,
            }
        },
    )
    .boxed()
}

fn record_accepted_route(route: AcceptedRoute) {
    let _ = RELIABLE_CALL_ACCOUNTING
        .try_with(|accounting| accounting.lock().accepted_route = Some(route));
}

pub fn transient_error_hint(err: &anyhow::Error) -> Option<&'static str> {
    if crate::model_refusal_from_error(err).is_some() {
        return Some(
            "The model's safety system declined this request. Rephrase it, or configure fallback_models on the provider to auto-switch models.",
        );
    }
    let msg = err.to_string();
    // 503 / service unavailable / high demand (Gemini, OpenAI, etc.)
    if msg.contains("503")
        || msg.to_ascii_lowercase().contains("unavailable")
        || msg.to_ascii_lowercase().contains("high demand")
        || msg.to_ascii_lowercase().contains("overloaded")
    {
        return Some(
            "I'm temporarily unable to reach my AI backend — please try again in a moment.",
        );
    }
    // 429 / quota / rate limit
    if msg.contains("429")
        || msg.to_ascii_lowercase().contains("rate limit")
        || msg.to_ascii_lowercase().contains("quota")
    {
        return Some("I've hit a usage limit — please try again shortly.");
    }
    None
}

/// Provider-declared terminal failures override retry/message heuristics.
fn has_typed_non_retryable_marker(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|source| source.is::<crate::traits::NonRetryableProviderError>())
}

/// First status-shaped HTTP client error code embedded in an error message:
/// a run of exactly three ASCII digits, not adjacent (either side) to an
/// ASCII alphanumeric character, whose value is in 400..500. Numbers glued to
/// units or words ("480s"), longer digit runs ("0409", "4800"), and values
/// outside the client range are not status codes. This keeps timing and
/// sizing numbers in provider messages (for example a stream-idle bound of
/// 480 s) from being misread as a 4xx client error.
fn embedded_client_status(message: &str) -> Option<u16> {
    let bytes = message.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_digit() {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        let status_shaped = end - start == 3
            && (start == 0 || !bytes[start - 1].is_ascii_alphanumeric())
            && (end == bytes.len() || !bytes[end].is_ascii_alphanumeric());
        if status_shaped
            && let Ok(code) = message[start..end].parse::<u16>()
            && (400..500).contains(&code)
        {
            return Some(code);
        }
        start = end;
    }
    None
}

/// Check if an error is non-retryable (client errors that won't resolve with retries).
pub fn is_non_retryable(err: &anyhow::Error) -> bool {
    // A provider's typed classification is definitive. Check the full chain
    // before text or status heuristics so recoverable-looking wording cannot
    // override an explicit provider safety decision.
    if has_typed_non_retryable_marker(err) {
        return true;
    }

    // A typed model refusal cannot be repaired by replaying the same request
    // against the same candidate. Advance directly to the next configured
    // provider/model entry.
    if err.downcast_ref::<AnthropicRefusalError>().is_some() {
        return true;
    }

    // Context window errors are NOT non-retryable — they can be recovered
    // by truncating conversation history, so let the retry loop handle them.
    if is_context_window_exceeded(err) {
        return false;
    }

    // Tool schema validation errors are NOT non-retryable — the model_provider's
    // built-in fallback in compatible.rs can recover by switching to
    // prompt-guided tool instructions.
    if is_tool_schema_error(err) {
        return false;
    }

    // 4xx errors are generally non-retryable (bad request, auth failure, etc.),
    // except 429 (rate-limit — transient) and 408 (timeout — worth retrying).
    if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>()
        && let Some(status) = reqwest_err.status()
    {
        let code = status.as_u16();
        return status.is_client_error() && code != 429 && code != 408;
    }
    // Fallback: parse status codes from stringified errors (some model_providers
    // embed codes in error messages rather than returning typed HTTP errors).
    // Only status-shaped numbers count (see `embedded_client_status`), so
    // elapsed times and other digit noise in a message never look like an
    // HTTP client error.
    let msg = err.to_string();
    if let Some(code) = embedded_client_status(&msg) {
        return code != 429 && code != 408;
    }

    // Heuristic: detect auth/model failures by keyword when no HTTP status
    // is available (e.g. gRPC or custom transport errors).
    let msg_lower = msg.to_lowercase();
    let auth_failure_hints = [
        "invalid api key",
        "incorrect api key",
        "missing api key",
        "api key not set",
        "authentication failed",
        "auth failed",
        "unauthorized",
        "forbidden",
        "permission denied",
        "access denied",
        "invalid token",
    ];

    if auth_failure_hints
        .iter()
        .any(|hint| msg_lower.contains(hint))
    {
        return true;
    }

    has_model_not_found_hint(&msg_lower)
}

/// Check if an error indicates an authentication/authorization failure.
/// Used by channels to evict cached model_providers whose OAuth tokens may have
/// expired so the next request triggers a fresh credential resolution.
pub fn is_auth_error(err: &anyhow::Error) -> bool {
    if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>()
        && let Some(status) = reqwest_err.status()
    {
        let code = status.as_u16();
        return code == 401 || code == 403;
    }

    let msg_lower = err.to_string().to_lowercase();
    let hints = [
        "401 unauthorized",
        "403 forbidden",
        "invalid api key",
        "incorrect api key",
        "authentication failed",
        "auth failed",
        "unauthorized",
        "invalid token",
        "token expired",
        "access_token",
    ];

    hints.iter().any(|hint| msg_lower.contains(hint))
}

fn is_missing_credential_error(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_lowercase();
    [
        "missing api key",
        "api key not set",
        "api key is required",
        "missing access token",
        "token not set",
        "anthropic credentials not set",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

pub fn is_tool_schema_error(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_lowercase();
    let hints = [
        "tool call validation failed",
        "was not in request",
        "not found in tool list",
        "invalid_tool_call",
    ];
    hints.iter().any(|hint| lower.contains(hint))
}

pub fn is_context_window_exceeded(err: &anyhow::Error) -> bool {
    let hints = [
        "exceeds the context window",
        "exceeds the available context size",
        "context window of this model",
        "maximum context length",
        "context length exceeded",
        "too many tokens",
        "token limit exceeded",
        "prompt is too long",
        "input is too long",
        "prompt exceeds max length",
    ];

    err.chain().any(|cause| {
        let lower = cause.to_string().to_lowercase();
        hints.iter().any(|hint| lower.contains(hint))
    })
}

/// Check if an error is a rate-limit (429) error.
fn is_rate_limited(err: &anyhow::Error) -> bool {
    if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>()
        && let Some(status) = reqwest_err.status()
    {
        return status.as_u16() == 429;
    }
    let msg = err.to_string();
    msg.contains("429")
        && (msg.contains("Too Many") || msg.contains("rate") || msg.contains("limit"))
}

fn is_non_retryable_rate_limit(err: &anyhow::Error) -> bool {
    if !is_rate_limited(err) {
        return false;
    }

    let msg = err.to_string();
    let lower = msg.to_lowercase();

    let business_hints = [
        "plan does not include",
        "doesn't include",
        "not include",
        "insufficient balance",
        "insufficient_balance",
        "insufficient quota",
        "insufficient_quota",
        "quota exhausted",
        "out of credits",
        "no available package",
        "package not active",
        "purchase package",
        "model not available for your plan",
    ];

    if business_hints.iter().any(|hint| lower.contains(hint)) {
        return true;
    }

    // Known model_provider business codes observed for 429 where retry is futile.
    for token in lower.split(|c: char| !c.is_ascii_digit()) {
        if let Ok(code) = token.parse::<u16>()
            && matches!(code, 1113 | 1311)
        {
            return true;
        }
    }

    false
}

/// Try to extract a Retry-After value (in milliseconds) from an error message.
/// Looks for patterns like `Retry-After: 5` or `retry_after: 2.5` in the error string.
fn parse_retry_after_ms(err: &anyhow::Error) -> Option<u64> {
    let msg = err.to_string();
    let lower = msg.to_lowercase();

    // Look for "retry-after: <number>" or "retry_after: <number>"
    for prefix in &[
        "retry-after:",
        "retry_after:",
        "retry-after ",
        "retry_after ",
    ] {
        if let Some(pos) = lower.find(prefix) {
            let after = &msg[pos + prefix.len()..];
            let num_str: String = after
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(secs) = num_str.parse::<f64>()
                && secs.is_finite()
                && secs >= 0.0
            {
                let millis = Duration::from_secs_f64(secs).as_millis();
                if let Ok(value) = u64::try_from(millis) {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn failure_reason(rate_limited: bool, non_retryable: bool) -> &'static str {
    if rate_limited && non_retryable {
        "rate_limited_non_retryable"
    } else if rate_limited {
        "rate_limited"
    } else if non_retryable {
        "non_retryable"
    } else {
        "retryable"
    }
}

fn compact_error_detail(err: &anyhow::Error) -> String {
    super::sanitize_api_error(&format!("{err:#}"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderErrorDiagnostic {
    kind: &'static str,
    phase: &'static str,
    hint: &'static str,
    endpoint: Option<String>,
}

/// A terminal Reliable failure that can be rendered safely at a user-facing
/// delivery boundary without exposing retry-attempt diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReliableProviderTerminalFailureKind {
    ContextWindow,
    CredentialsMissing,
    Authentication,
    RateLimited,
    ProviderServer,
    ModelNotFound,
    ClientRequest,
    Connection,
    Timeout,
    Other,
}

impl ReliableProviderTerminalFailureKind {
    fn from_diagnostic_kind(kind: &str) -> Self {
        match kind {
            "context_window" => Self::ContextWindow,
            "credentials_missing" => Self::CredentialsMissing,
            "auth" => Self::Authentication,
            "rate_limited" => Self::RateLimited,
            "provider_server" => Self::ProviderServer,
            "model_not_found" => Self::ModelNotFound,
            "client_error" => Self::ClientRequest,
            "connect" | "connect_timeout" | "dns" => Self::Connection,
            "timeout" => Self::Timeout,
            _ => Self::Other,
        }
    }
}

/// The typed terminal presentation cause for a Reliable provider failure.
///
/// `Display` intentionally remains the full diagnostic summary used by logs.
/// User-facing delivery must select a localized message from [`Self::kind`]
/// instead of exposing the retry envelope.
#[derive(Debug)]
pub struct ReliableProviderTerminalFailure {
    kind: ReliableProviderTerminalFailureKind,
    provider: Option<String>,
    endpoint: Option<String>,
    diagnostic: String,
    terminal_cause: Option<anyhow::Error>,
}

impl ReliableProviderTerminalFailure {
    pub fn new(
        kind: ReliableProviderTerminalFailureKind,
        endpoint: Option<String>,
        diagnostic: String,
    ) -> Self {
        Self {
            kind,
            provider: None,
            endpoint,
            diagnostic,
            terminal_cause: None,
        }
    }

    /// Classify a provider error into a safe terminal presentation cause.
    pub fn from_error(error: &anyhow::Error) -> Self {
        let diagnostic = provider_error_diagnostic(error);
        Self::new(
            ReliableProviderTerminalFailureKind::from_diagnostic_kind(diagnostic.kind),
            diagnostic.endpoint,
            format!(
                "provider error: kind={}; phase={}; hint={}",
                diagnostic.kind, diagnostic.phase, diagnostic.hint
            ),
        )
    }

    fn with_cause(
        provider: Option<&str>,
        diagnostic: ProviderErrorDiagnostic,
        failure_aggregate: String,
        terminal_cause: anyhow::Error,
    ) -> Self {
        Self {
            kind: ReliableProviderTerminalFailureKind::from_diagnostic_kind(diagnostic.kind),
            provider: provider
                .filter(|provider| !provider.is_empty())
                .map(str::to_owned),
            endpoint: diagnostic.endpoint,
            diagnostic: failure_aggregate,
            terminal_cause: Some(terminal_cause),
        }
    }

    pub fn kind(&self) -> ReliableProviderTerminalFailureKind {
        self.kind
    }

    /// Attach the configured provider identity used for safe user-facing text.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        let provider = provider.into();
        self.provider = (!provider.is_empty()).then_some(provider);
        self
    }

    /// Attach an underlying terminal cause while retaining diagnostic and kind mapping.
    pub fn with_terminal_cause(mut self, cause: anyhow::Error) -> Self {
        self.terminal_cause = Some(cause);
        self
    }

    /// The underlying terminal cause if one was attached.
    pub fn terminal_cause(&self) -> Option<&anyhow::Error> {
        self.terminal_cause.as_ref()
    }

    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    pub fn endpoint_is_local(&self) -> bool {
        self.endpoint.as_deref().is_some_and(|endpoint| {
            reqwest::Url::parse(endpoint)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .is_some_and(|host| {
                    host.eq_ignore_ascii_case("localhost")
                        || host
                            .parse::<IpAddr>()
                            .is_ok_and(|address| address.is_loopback())
                })
        })
    }
}

impl std::fmt::Display for ReliableProviderTerminalFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.diagnostic)
    }
}

impl std::error::Error for ReliableProviderTerminalFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.terminal_cause
            .as_ref()
            .map(|cause| cause.as_ref() as &(dyn std::error::Error + 'static))
    }
}

fn sanitized_url_endpoint(mut url: reqwest::Url) -> String {
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    super::sanitize_api_error(url.as_ref())
}

fn endpoint_from_error_text(text: &str) -> Option<String> {
    let start = text.find("https://").or_else(|| text.find("http://"))?;
    let raw = text[start..]
        .split(|c: char| c.is_whitespace() || matches!(c, ')' | ',' | ';' | '"'))
        .next()
        .unwrap_or("");
    let url = reqwest::Url::parse(raw)
        .or_else(|_| reqwest::Url::parse(raw.trim_end_matches([':', '.'])))
        .ok()?;
    Some(sanitized_url_endpoint(url))
}

fn http_status_from_error_text(text: &str) -> Option<u16> {
    for prefix in [
        "model_provider stream error: modelprovider error:",
        "modelprovider error:",
    ] {
        if let Some(after_prefix) = text.strip_prefix(prefix).map(str::trim_start)
            && let Some(code) = after_prefix
                .get(..3)
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|code| (400..600).contains(code))
                .filter(|_| {
                    after_prefix
                        .as_bytes()
                        .get(3)
                        .is_some_and(u8::is_ascii_whitespace)
                })
        {
            return Some(code);
        }
    }

    for marker in ["api error (", "http "] {
        let mut remainder = text;
        while let Some(start) = remainder.find(marker) {
            let after_marker = &remainder[start + marker.len()..];
            if let Some(code) = after_marker
                .get(..3)
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|code| (400..600).contains(code))
            {
                return Some(code);
            }
            remainder = after_marker;
        }
    }
    None
}

fn http_status_diagnostic(code: u16, endpoint: Option<String>) -> ProviderErrorDiagnostic {
    let (kind, hint) = if matches!(code, 401 | 403) {
        ("auth", "check provider credentials")
    } else if code == 429 {
        ("rate_limited", "wait, change key/quota, or switch provider")
    } else if (500..600).contains(&code) {
        (
            "provider_server",
            "provider returned a server error; retry or switch provider",
        )
    } else if code == 404 {
        (
            "model_not_found",
            "check the configured model id for this provider",
        )
    } else if (400..500).contains(&code) {
        (
            "client_error",
            "provider rejected the request; check config, model, or request shape",
        )
    } else {
        ("http_error", "inspect provider response or switch provider")
    };
    ProviderErrorDiagnostic {
        kind,
        phase: "http_response",
        hint,
        endpoint,
    }
}

fn http_status_is_authoritative(code: u16) -> bool {
    matches!(code, 401 | 403 | 404 | 429) || (500..600).contains(&code)
}

fn has_model_not_found_hint(message: &str) -> bool {
    message.split(": ").any(|segment| {
        let segment = segment.trim_start();

        [
            "model not found",
            "unknown model",
            "unsupported model",
            "invalid model",
        ]
        .iter()
        .any(|hint| segment.starts_with(hint))
            || ["model ", "requested model ", "the requested model "]
                .iter()
                .find_map(|prefix| segment.strip_prefix(prefix))
                .is_some_and(|model_detail| {
                    [
                        " not found",
                        " does not exist",
                        " is unknown",
                        " is unsupported",
                        " is not supported",
                        " is invalid",
                    ]
                    .iter()
                    .any(|hint| model_detail.contains(hint))
                        || model_detail == "unknown"
                })
    })
}

fn provider_error_diagnostic(err: &anyhow::Error) -> ProviderErrorDiagnostic {
    let error_detail = compact_error_detail(err);
    let lower = error_detail.to_lowercase();
    let endpoint = err
        .downcast_ref::<reqwest::Error>()
        .and_then(|reqwest_err| reqwest_err.url().cloned().map(sanitized_url_endpoint))
        .or_else(|| endpoint_from_error_text(&error_detail));
    let structured_status = err
        .downcast_ref::<reqwest::Error>()
        .and_then(reqwest::Error::status)
        .map(|status| status.as_u16());
    let text_status = http_status_from_error_text(&lower);

    let http_status = structured_status.or(text_status);

    if let Some(status) = http_status.filter(|status| http_status_is_authoritative(*status)) {
        return http_status_diagnostic(status, endpoint);
    }

    if is_context_window_exceeded(err) {
        return ProviderErrorDiagnostic {
            kind: "context_window",
            phase: "request_validation",
            hint: "reduce context or use a larger-context model",
            endpoint,
        };
    }

    if is_missing_credential_error(err) {
        return ProviderErrorDiagnostic {
            kind: "credentials_missing",
            phase: "configuration",
            hint: "configure provider credentials",
            endpoint,
        };
    }

    if is_auth_error(err) {
        return ProviderErrorDiagnostic {
            kind: "auth",
            phase: "http_response",
            hint: "check provider credentials",
            endpoint,
        };
    }

    if is_rate_limited(err) {
        return ProviderErrorDiagnostic {
            kind: "rate_limited",
            phase: "http_response",
            hint: "wait, change key/quota, or switch provider",
            endpoint,
        };
    }

    if let Some(status) = http_status {
        return http_status_diagnostic(status, endpoint);
    }

    if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>() {
        if reqwest_err.is_timeout() && reqwest_err.is_connect() {
            return ProviderErrorDiagnostic {
                kind: "connect_timeout",
                phase: "tls_or_connect",
                hint: "connection reached the host but timed out during connect/TLS; check VPN, firewall, routing, or switch provider",
                endpoint,
            };
        }

        if reqwest_err.is_timeout() {
            return ProviderErrorDiagnostic {
                kind: "timeout",
                phase: "request",
                hint: "provider request timed out; retry or switch provider",
                endpoint,
            };
        }

        if reqwest_err.is_connect() {
            return ProviderErrorDiagnostic {
                kind: "connect",
                phase: "connect",
                hint: "could not open provider connection; check network, VPN, or firewall",
                endpoint,
            };
        }
    }

    if (lower.contains("client error (connect)") && lower.contains("timed out"))
        || lower.contains("ssl connection timeout")
        || (lower.contains("tls") && lower.contains("timeout"))
    {
        return ProviderErrorDiagnostic {
            kind: "connect_timeout",
            phase: "tls_or_connect",
            hint: "connection reached the host but timed out during connect/TLS; check VPN, firewall, routing, or switch provider",
            endpoint,
        };
    }

    if lower.contains("client error (connect)") || lower.contains("connection refused") {
        return ProviderErrorDiagnostic {
            kind: "connect",
            phase: "connect",
            hint: "could not open provider connection; check network, VPN, or firewall",
            endpoint,
        };
    }

    if lower.contains("timed out") || lower.contains("timeout") {
        return ProviderErrorDiagnostic {
            kind: "timeout",
            phase: "request",
            hint: "provider request timed out; retry or switch provider",
            endpoint,
        };
    }

    if lower.contains("dns") || lower.contains("resolve") {
        return ProviderErrorDiagnostic {
            kind: "dns",
            phase: "dns",
            hint: "DNS resolution failed; check network or provider host",
            endpoint,
        };
    }

    if has_model_not_found_hint(&lower) {
        return ProviderErrorDiagnostic {
            kind: "model_not_found",
            phase: "http_response",
            hint: "check the configured model id for this provider",
            endpoint,
        };
    }

    ProviderErrorDiagnostic {
        kind: "provider_error",
        phase: "unknown",
        hint: "inspect provider error or switch provider",
        endpoint,
    }
}

fn provider_failure_attrs(
    provider_name: &str,
    model: &str,
    error_detail: &str,
    diagnostic: &ProviderErrorDiagnostic,
) -> serde_json::Value {
    serde_json::json!({
        "model_provider": provider_name,
        "model": model,
        "error": error_detail,
        "error_kind": diagnostic.kind,
        "error_phase": diagnostic.phase,
        "endpoint": diagnostic.endpoint.as_deref(),
        "hint": diagnostic.hint,
    })
}

fn provider_retry_attrs(
    provider_name: &str,
    model: &str,
    attempt: u32,
    backoff_ms: u64,
    reason: &str,
    error_detail: &str,
    diagnostic: &ProviderErrorDiagnostic,
) -> serde_json::Value {
    serde_json::json!({
        "model_provider": provider_name,
        "model": model,
        "attempt": attempt,
        "backoff_ms": backoff_ms,
        "reason": reason,
        "error": error_detail,
        "error_kind": diagnostic.kind,
        "error_phase": diagnostic.phase,
        "endpoint": diagnostic.endpoint.as_deref(),
        "hint": diagnostic.hint,
    })
}

fn provider_exhausted_attrs(
    provider_name: &str,
    model: &str,
    last_error_detail: Option<&str>,
    last_diagnostic: Option<&ProviderErrorDiagnostic>,
) -> serde_json::Value {
    serde_json::json!({
        "model_provider": provider_name,
        "model": model,
        "error": last_error_detail,
        "error_kind": last_diagnostic.map(|diagnostic| diagnostic.kind),
        "error_phase": last_diagnostic.map(|diagnostic| diagnostic.phase),
        "endpoint": last_diagnostic.and_then(|diagnostic| diagnostic.endpoint.as_deref()),
        "hint": last_diagnostic.map(|diagnostic| diagnostic.hint),
    })
}

fn is_context_turn_boundary(message: &ChatMessage) -> bool {
    message.role == "user"
        && !crate::multimodal::is_prompt_tool_result_message(message)
        && !message.is_pruned_context_separator()
}

fn context_truncation_limit(messages: &[ChatMessage]) -> &'static str {
    if messages.iter().any(is_context_turn_boundary) {
        "only one complete user turn remains"
    } else {
        "history contains no complete user turn"
    }
}

/// Truncate conversation history at a user-turn boundary near the oldest half.
/// Returns the number of non-system messages dropped while keeping at least the
/// most recent complete turn and preserving tool calls with all of their
/// results.
fn truncate_for_context(messages: &mut Vec<ChatMessage>) -> usize {
    let non_system: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role != "system")
        .map(|(i, _)| i)
        .collect();

    let turn_boundaries: Vec<usize> = non_system
        .iter()
        .enumerate()
        .filter_map(|(position, &message_index)| {
            is_context_turn_boundary(&messages[message_index]).then_some(position)
        })
        .collect();
    if turn_boundaries.len() <= 1 {
        return 0;
    }

    let target_drop = non_system.len() / 2;
    let Some(&last_boundary) = turn_boundaries.last() else {
        return 0;
    };
    let first_kept_position = turn_boundaries
        .iter()
        .copied()
        .skip(1)
        .find(|&position| position >= target_drop)
        .unwrap_or(last_boundary);
    let first_kept_index = non_system[first_kept_position];
    let mut original_index = 0usize;
    messages.retain(|message| {
        let keep = message.role == "system" || original_index >= first_kept_index;
        original_index += 1;
        keep
    });

    first_kept_position
}

const MAX_RETAINED_FAILURE_EVENTS: usize = 8;
const MAX_FAILURE_AGGREGATE_BYTES: usize = 2_048;

#[derive(Clone, Debug, Default)]
struct FailureEvents {
    total: usize,
    retained: Vec<String>,
}

impl FailureEvents {
    fn push(&mut self, event: String) {
        self.total += 1;
        if self.retained.len() < MAX_RETAINED_FAILURE_EVENTS {
            self.retained.push(event);
        }
    }

    fn next_index(&self) -> usize {
        self.total + 1
    }
}

fn push_failure(
    failures: &mut FailureEvents,
    attempt: u32,
    max_attempts: u32,
    reason: &'static str,
    diagnostic: Option<&ProviderErrorDiagnostic>,
) {
    // This aggregate can cross into model-visible tool results and durable
    // background results. Keep it to fields controlled by ClawCrew; the
    // provider response detail is retained in the structured attempt logs.
    let mut failure = format!(
        "event {} (retry {attempt}/{max_attempts}): {reason}",
        failures.next_index()
    );
    if let Some(diagnostic) = diagnostic {
        failure.push_str(&format!(
            "; kind={}; phase={}; hint={}",
            diagnostic.kind, diagnostic.phase, diagnostic.hint
        ));
    }
    failures.push(failure);
}

fn omitted_failure_marker(count: usize) -> String {
    format!("[{count} additional failure event(s) omitted]")
}

fn format_failure_aggregate(header: String, failures: &FailureEvents) -> String {
    let all_omitted_marker = omitted_failure_marker(failures.total);
    let minimum_suffix_len = if failures.total > 0 {
        1 + all_omitted_marker.len()
    } else {
        0
    };
    let mut output = if header.len() + minimum_suffix_len <= MAX_FAILURE_AGGREGATE_BYTES {
        header
    } else {
        format!(
            "Model provider failure after {} failure event(s). Events:",
            failures.total
        )
    };
    let mut retained_count = 0;

    for failure in &failures.retained {
        let candidate_retained_count = retained_count + 1;
        let omitted_after = failures.total - candidate_retained_count;
        let reserved_marker_len = if omitted_after > 0 {
            1 + omitted_failure_marker(omitted_after).len()
        } else {
            0
        };
        if output.len() + 1 + failure.len() + reserved_marker_len > MAX_FAILURE_AGGREGATE_BYTES {
            break;
        }
        output.push('\n');
        output.push_str(failure);
        retained_count = candidate_retained_count;
    }

    let omitted = failures.total - retained_count;
    if omitted > 0 {
        output.push('\n');
        output.push_str(&omitted_failure_marker(omitted));
    }
    debug_assert!(output.len() <= MAX_FAILURE_AGGREGATE_BYTES);
    output
}

fn failure_aggregate(failures: &FailureEvents) -> String {
    format_failure_aggregate(
        format!(
            "All model providers/models failed after {} failure event(s). Events:",
            failures.total
        ),
        failures,
    )
}

fn context_failure_aggregate(message: &str, failures: &FailureEvents) -> String {
    format_failure_aggregate(
        format!(
            "{message} Failed after {} failure event(s). Events:",
            failures.total
        ),
        failures,
    )
}

fn is_empty_completion(resp: &ChatResponse) -> bool {
    resp.is_semantically_empty_terminal()
}

fn is_empty_text_completion(text: &str) -> bool {
    clawcrew_api::model_provider::strip_think_tags(text).is_empty()
}

fn is_semantic_empty_completion_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<clawcrew_api::model_provider::SemanticEmptyTerminalCompletion>())
}

/// Extract billing metadata a Reliable terminal error preserves alongside its
/// actual cause. The caller still returns the original error unchanged.
pub(crate) fn terminal_error_usage(error: &anyhow::Error) -> Option<TokenUsage> {
    crate::rejected_attempt_usage_from_error(error).cloned()
}

/// A Reliable chat request exhausted its candidates after receiving rejected
/// semantic completions. The provider-reported usage is retained so the turn
/// loop can account for work that was billed even though no response was
/// accepted.
#[derive(Debug)]
pub struct ReliableRejectedCompletionUsage {
    pub usage: TokenUsage,
    failures: FailureEvents,
    terminal_cause: Option<anyhow::Error>,
}

impl ReliableRejectedCompletionUsage {
    fn new(usage: TokenUsage, failures: FailureEvents) -> Self {
        Self {
            usage,
            failures,
            terminal_cause: None,
        }
    }

    fn with_terminal_cause(
        usage: TokenUsage,
        failures: FailureEvents,
        cause: anyhow::Error,
    ) -> Self {
        Self {
            usage,
            failures,
            terminal_cause: Some(cause),
        }
    }
}

impl std::fmt::Display for ReliableRejectedCompletionUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", failure_aggregate(&self.failures))
    }
}

impl std::error::Error for ReliableRejectedCompletionUsage {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.terminal_cause
            .as_ref()
            .map(|cause| cause.as_ref() as &(dyn std::error::Error + 'static))
    }
}

/// The final candidate attempt completed successfully at the transport layer
/// but supplied neither usable text nor a native tool call. This remains typed
/// independently from optional rejected-attempt usage so delivery layers can
/// classify the actual terminal failure without guessing from accounting data.
#[derive(Debug)]
pub struct ReliableSemanticEmptyCompletion {
    failures: FailureEvents,
    rejected_usage: Option<ReliableRejectedCompletionUsage>,
    terminal_cause: clawcrew_api::model_provider::SemanticEmptyTerminalCompletion,
}

impl std::fmt::Display for ReliableSemanticEmptyCompletion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", failure_aggregate(&self.failures))
    }
}

impl std::error::Error for ReliableSemanticEmptyCompletion {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.rejected_usage
            .as_ref()
            .map(|usage| usage as &(dyn std::error::Error + 'static))
            .or(Some(&self.terminal_cause))
    }
}

fn reliable_terminal_error(
    failures: FailureEvents,
    rejected_attempt_usage: Option<TokenUsage>,
    final_cause_is_semantic_empty: bool,
) -> anyhow::Error {
    let rejected_attempt_usage = rejected_attempt_usage.or_else(accounted_rejected_attempt_usage);
    if final_cause_is_semantic_empty {
        let terminal_cause = clawcrew_api::model_provider::SemanticEmptyTerminalCompletion;
        return anyhow::Error::new(ReliableSemanticEmptyCompletion {
            failures: failures.clone(),
            rejected_usage: rejected_attempt_usage.map(|usage| {
                ReliableRejectedCompletionUsage::with_terminal_cause(
                    usage,
                    failures,
                    anyhow::Error::new(
                        clawcrew_api::model_provider::SemanticEmptyTerminalCompletion,
                    ),
                )
            }),
            terminal_cause,
        });
    }

    match rejected_attempt_usage {
        Some(usage) => anyhow::Error::new(ReliableRejectedCompletionUsage::new(usage, failures)),
        None => anyhow::Error::msg(failure_aggregate(&failures)),
    }
}

fn reliable_terminal_error_with_cause(
    provider: Option<&str>,
    failures: FailureEvents,
    rejected_attempt_usage: Option<TokenUsage>,
    final_cause_is_semantic_empty: bool,
    final_cause: Option<anyhow::Error>,
) -> anyhow::Error {
    let rejected_attempt_usage = rejected_attempt_usage.or_else(accounted_rejected_attempt_usage);
    if !final_cause_is_semantic_empty && let Some(cause) = final_cause {
        let terminal_failure = anyhow::Error::new(ReliableProviderTerminalFailure::with_cause(
            provider,
            provider_error_diagnostic(&cause),
            failure_aggregate(&failures),
            cause,
        ));
        if let Some(usage) = rejected_attempt_usage {
            return anyhow::Error::new(ReliableRejectedCompletionUsage::with_terminal_cause(
                usage,
                failures,
                terminal_failure,
            ));
        }
        return terminal_failure;
    }
    if !final_cause_is_semantic_empty && let Some(diagnostic) = stream_recovery_failure_diagnostic()
    {
        let terminal_failure = anyhow::Error::new(
            ReliableProviderTerminalFailure::new(
                ReliableProviderTerminalFailureKind::from_diagnostic_kind(diagnostic.kind),
                diagnostic.endpoint,
                failure_aggregate(&failures),
            )
            .with_provider(provider.unwrap_or_default()),
        );
        if let Some(usage) = rejected_attempt_usage {
            return anyhow::Error::new(ReliableRejectedCompletionUsage::with_terminal_cause(
                usage,
                failures,
                terminal_failure,
            ));
        }
        return terminal_failure;
    }
    reliable_terminal_error(
        failures,
        rejected_attempt_usage,
        final_cause_is_semantic_empty,
    )
}

pub(crate) fn accumulate_usage(total: &mut Option<TokenUsage>, usage: Option<&TokenUsage>) {
    let Some(usage) = usage else {
        return;
    };
    let accumulated = total.get_or_insert_with(TokenUsage::default);
    for (target, value) in [
        (&mut accumulated.input_tokens, usage.input_tokens),
        (&mut accumulated.output_tokens, usage.output_tokens),
        (
            &mut accumulated.cached_input_tokens,
            usage.cached_input_tokens,
        ),
    ] {
        if let Some(value) = value {
            *target = Some(target.unwrap_or(0).saturating_add(value));
        }
    }
}

fn combine_response_usage(response: &mut ChatResponse, prior_attempts: Option<TokenUsage>) {
    let Some(prior_attempts) = prior_attempts else {
        return;
    };
    let mut combined = Some(prior_attempts);
    accumulate_usage(&mut combined, response.usage.as_ref());
    response.usage = combined;
}

enum ReliableModelProviderEntryProvider {
    Direct(Box<dyn ModelProvider>),
    Pinned(crate::model_pin::ModelPinnedProvider),
    #[cfg(test)]
    DispatchObservedPinned {
        pinned_model: String,
        provider: Box<dyn ModelProvider>,
    },
}

impl ReliableModelProviderEntryProvider {
    fn as_model_provider(&self) -> &dyn ModelProvider {
        match self {
            Self::Direct(provider) => provider.as_ref(),
            Self::Pinned(provider) => provider,
            #[cfg(test)]
            Self::DispatchObservedPinned { provider, .. } => provider.as_ref(),
        }
    }

    fn served_model<'a>(&'a self, requested_model: &'a str) -> &'a str {
        match self {
            Self::Direct(_) => requested_model,
            Self::Pinned(provider) => provider.pinned_model(),
            #[cfg(test)]
            Self::DispatchObservedPinned { pinned_model, .. } => pinned_model,
        }
    }
}

pub(crate) struct ReliableModelProviderEntry {
    display_name: String,
    /// Exact configured candidate identity used for fallback attribution.
    ///
    /// This deliberately differs from `display_name`: two aliases can share a
    /// provider family and model while still being distinct fallback candidates.
    candidate_name: String,
    cooldown_key: String,
    provider: ReliableModelProviderEntryProvider,
}

impl ReliableModelProviderEntry {
    pub(crate) fn new(
        display_name: impl Into<String>,
        cooldown_key: impl Into<String>,
        provider: Box<dyn ModelProvider>,
    ) -> Self {
        let display_name = display_name.into();
        Self {
            candidate_name: display_name.clone(),
            display_name,
            cooldown_key: cooldown_key.into(),
            provider: ReliableModelProviderEntryProvider::Direct(provider),
        }
    }

    pub(crate) fn new_with_candidate(
        display_name: impl Into<String>,
        cooldown_key: impl Into<String>,
        candidate_name: impl Into<String>,
        provider: Box<dyn ModelProvider>,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            candidate_name: candidate_name.into(),
            cooldown_key: cooldown_key.into(),
            provider: ReliableModelProviderEntryProvider::Direct(provider),
        }
    }

    /// Build an entry that serves `pinned_model` regardless of the requested
    /// model. The [`crate::model_pin::ModelPinnedProvider`] wrapper is the
    /// source of truth for the pinned model; this entry reads it from the
    /// wrapper at use-time.
    pub(crate) fn new_pinned(
        display_name: impl Into<String>,
        cooldown_key: impl Into<String>,
        alias: &str,
        pinned_model: &str,
        inner: Box<dyn ModelProvider>,
    ) -> Self {
        let cooldown_key = cooldown_key.into();
        Self {
            display_name: display_name.into(),
            candidate_name: cooldown_key.clone(),
            cooldown_key,
            provider: ReliableModelProviderEntryProvider::Pinned(
                crate::model_pin::ModelPinnedProvider::builder(alias)
                    .pinned_model(pinned_model)
                    .inner(inner)
                    .build(),
            ),
        }
    }

    /// Build a test-only pinned entry whose provider observes Reliable's dispatch argument.
    #[cfg(test)]
    fn new_dispatch_observed_pinned(
        display_name: impl Into<String>,
        cooldown_key: impl Into<String>,
        pinned_model: impl Into<String>,
        provider: Box<dyn ModelProvider>,
    ) -> Self {
        let cooldown_key = cooldown_key.into();
        Self {
            display_name: display_name.into(),
            candidate_name: cooldown_key.clone(),
            cooldown_key,
            provider: ReliableModelProviderEntryProvider::DispatchObservedPinned {
                pinned_model: pinned_model.into(),
                provider,
            },
        }
    }

    /// Model this entry serves for `requested_model`: the pinned model when
    /// the entry is model-pinned, otherwise the requested model unchanged.
    fn served_model<'a>(&'a self, requested_model: &'a str) -> &'a str {
        self.provider.served_model(requested_model)
    }

    fn candidate_name(&self) -> &str {
        &self.candidate_name
    }

    fn provider(&self) -> &dyn ModelProvider {
        self.provider.as_model_provider()
    }
}

/// ModelProvider wrapper with retry + auth-key rotation. The model_provider Vec exists
/// for tests to exercise multi-provider failover; production wiring always
/// passes a single primary. Per-model failover chains are also test-only —
/// the schema no longer surfaces them.
pub struct ReliableModelProvider {
    /// `[providers.models.<family>.<alias>]` config-key alias.
    alias: String,
    model_providers: Vec<ReliableModelProviderEntry>,
    max_retries: u32,
    base_backoff_ms: u64,
    /// Extra API keys for rotation (index tracks round-robin position).
    api_keys: Vec<String>,
    key_index: AtomicUsize,
    /// Per-model failover chains. Test-only: model_name → [alt1, alt2, ...].
    model_fallbacks: HashMap<String, Vec<String>>,
    /// Transient provider cooldowns after retryable rate limits.
    /// Source of truth: live provider 429 / Retry-After evidence observed by
    /// this wrapper. It is intentionally in-memory and per wrapper instance.
    rate_limit_cooldowns: Mutex<HashMap<String, Instant>>,
}

impl ReliableModelProvider {
    pub fn new(
        alias: &str,
        model_providers: Vec<(String, Box<dyn ModelProvider>)>,
        max_retries: u32,
        base_backoff_ms: u64,
    ) -> Self {
        let model_providers = model_providers
            .into_iter()
            .map(|(display_name, provider)| {
                ReliableModelProviderEntry::new(display_name.clone(), display_name, provider)
            })
            .collect();

        Self::new_with_entries(alias, model_providers, max_retries, base_backoff_ms)
    }

    pub(crate) fn new_with_entries(
        alias: &str,
        model_providers: Vec<ReliableModelProviderEntry>,
        max_retries: u32,
        base_backoff_ms: u64,
    ) -> Self {
        Self {
            alias: alias.to_string(),
            model_providers,
            max_retries,
            base_backoff_ms: base_backoff_ms.max(50),
            api_keys: Vec::new(),
            key_index: AtomicUsize::new(0),
            model_fallbacks: HashMap::new(),
            rate_limit_cooldowns: Mutex::new(HashMap::new()),
        }
    }

    /// Build a provider whose entries mirror `push_pinned_entries`: every entry
    /// shares ONE `cooldown_key` (the `<family>.<alias>` reference) and differs
    /// only in its pinned model, exactly as a `fallback_models` list produces.
    ///
    /// Exposed for cross-crate regressions that need the same-alias
    /// pinned-model failover shape; the production builder reaches
    /// `new_pinned` directly.
    #[doc(hidden)]
    pub fn new_pinned_for_test(
        alias: &str,
        entries: Vec<(&str, &str, &str, std::sync::Arc<dyn ModelProvider>)>,
        max_retries: u32,
        base_backoff_ms: u64,
    ) -> Self {
        let model_providers = entries
            .into_iter()
            .map(|(cooldown_key, provider_alias, pinned_model, inner)| {
                ReliableModelProviderEntry::new_pinned(
                    cooldown_key,
                    cooldown_key,
                    provider_alias,
                    pinned_model,
                    Box::new(inner),
                )
            })
            .collect();
        Self::new_with_entries(alias, model_providers, max_retries, base_backoff_ms)
    }
    /// Set additional API keys for round-robin rotation on rate-limit errors.
    pub fn with_api_keys(mut self, keys: Vec<String>) -> Self {
        self.api_keys = keys;
        self
    }

    #[cfg(test)]
    pub fn with_model_fallbacks(mut self, fallbacks: HashMap<String, Vec<String>>) -> Self {
        self.model_fallbacks = fallbacks;
        self
    }

    /// Build the list of models to try: [original, alt1, alt2, ...]
    fn model_chain<'a>(&'a self, model: &'a str) -> Vec<&'a str> {
        let mut chain = vec![model];
        if let Some(fallbacks) = self.model_fallbacks.get(model) {
            chain.extend(fallbacks.iter().map(|s| s.as_str()));
        }
        chain
    }

    /// Advance to the next API key and return it, or None if no extra keys configured.
    fn rotate_key(&self) -> Option<&str> {
        if self.api_keys.is_empty() {
            return None;
        }
        let idx = self.key_index.fetch_add(1, Ordering::Relaxed) % self.api_keys.len();
        Some(&self.api_keys[idx])
    }

    /// Compute backoff duration, respecting Retry-After if present.
    fn compute_backoff(&self, base: u64, err: &anyhow::Error) -> u64 {
        if let Some(retry_after) = parse_retry_after_ms(err) {
            // Use Retry-After but cap at 30s to avoid indefinite waits
            retry_after.min(30_000).max(base)
        } else {
            base
        }
    }

    fn configured_provider_identity(&self) -> Option<&str> {
        self.model_providers
            .first()
            .map(ReliableModelProviderEntry::candidate_name)
            .filter(|provider| !provider.is_empty())
    }

    /// Default cooldown after a retryable 429 when Retry-After is absent.
    const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(10);

    /// Returns whether a cooldown is active and prunes expired cooldowns.
    fn provider_cooldown_active(&self, cooldown_key: &str) -> bool {
        let now = Instant::now();
        let mut cooldowns = self
            .rate_limit_cooldowns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match cooldowns.get(cooldown_key).copied() {
            Some(deadline) if now < deadline => true,
            Some(_) => {
                cooldowns.remove(cooldown_key);
                false
            }
            None => false,
        }
    }

    fn provider_should_skip_for_cooldown(&self, entry: &ReliableModelProviderEntry) -> bool {
        self.model_providers.len() > 1 && self.provider_cooldown_active(&entry.cooldown_key)
    }

    /// Admit an entry with its configured retry budget, except for the exact
    /// stream-failed entry, which is skipped to avoid replaying it — with two
    /// one-shot exceptions, each granting a single atomic non-stream attempt:
    /// the semantic-empty entry (when the budget permits it), and the
    /// single-candidate case (no other candidate exists, so a non-stream retry
    /// of the same entry is recovery, not replay). When both exceptions apply
    /// to the same entry, semantic-empty wins and the grants merge into one
    /// single attempt — never two.
    fn effective_retry_limit(
        &self,
        model_slot: usize,
        entry_index: usize,
        has_other_candidate: bool,
    ) -> Option<u32> {
        let max_retries = self.max_retries;
        RELIABLE_CALL_ACCOUNTING
            .try_with(|accounting| {
                let mut accounting = accounting.lock();
                let exact_failed_entry = accounting.stream_resume_after.is_some_and(|failed| {
                    model_slot == failed.model_slot && entry_index == failed.entry_index
                });
                let decision = Self::stream_recovery_decision(
                    max_retries,
                    exact_failed_entry,
                    accounting.stream_recovery_semantic_empty_permission,
                    has_other_candidate,
                );
                match decision {
                    RetryDecision::Admit(limit) => {
                        if exact_failed_entry {
                            // Consume one-shot recovery grants so each fires at
                            // most once. Clearing the resume marker merges the
                            // single-candidate grant into the semantic-empty
                            // attempt when both apply.
                            accounting.stream_recovery_semantic_empty_permission = false;
                            if !has_other_candidate {
                                accounting.stream_resume_after = None;
                            }
                        }
                        Some(limit)
                    }
                    RetryDecision::Skip => None,
                }
            })
            .unwrap_or(Some(max_retries))
    }

    /// Pure retry policy for a single entry: precedence is encoded in this
    /// `match` so each recovery mode is an explicit, independently testable
    /// decision rather than a branch in an if-chain. Stateful one-shot
    /// consumption lives in [`Self::effective_retry_limit`], not here.
    fn stream_recovery_decision(
        max_retries: u32,
        exact_failed_entry: bool,
        semantic_empty_permission: bool,
        has_other_candidate: bool,
    ) -> RetryDecision {
        if !exact_failed_entry {
            return RetryDecision::Admit(max_retries);
        }
        // Semantic-empty wins when both exceptions apply (see
        // `effective_retry_limit` for the merged single-attempt consumption).
        if max_retries > 0 && semantic_empty_permission {
            return RetryDecision::Admit(0);
        }
        // Single-candidate stream failure: no alternative entry exists, so one
        // non-stream attempt of the same entry is the only recovery path.
        if !has_other_candidate {
            return RetryDecision::Admit(0);
        }
        RetryDecision::Skip
    }

    fn record_cooldown_skip_failure(failures: &mut FailureEvents, max_attempts: u32) {
        let diagnostic = ProviderErrorDiagnostic {
            kind: "rate_limited",
            phase: "cooldown",
            hint: "wait for provider cooldown or switch provider",
            endpoint: None,
        };
        push_failure(
            failures,
            0,
            max_attempts,
            "rate_limit_cooldown",
            Some(&diagnostic),
        );
    }

    fn log_cooldown_skip(&self, provider_name: &str, model: &str) {
        ::clawcrew_log::record!(
            DEBUG,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(
                ::serde_json::json!({
                    "model_provider": provider_name,
                    "model": model,
                })
            ),
            "Skipping model_provider during rate-limit cooldown"
        );
    }

    fn set_rate_limit_cooldown(&self, cooldown_key: &str, err: &anyhow::Error) -> Duration {
        let cooldown = parse_retry_after_ms(err)
            .map(|ms| Duration::from_millis(ms.min(60_000)))
            .unwrap_or(Self::RATE_LIMIT_COOLDOWN);

        let mut cooldowns = self
            .rate_limit_cooldowns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cooldowns.insert(cooldown_key.to_string(), Instant::now() + cooldown);
        cooldown
    }

    fn cool_down_rate_limited_provider(
        &self,
        entry: &ReliableModelProviderEntry,
        model: &str,
        err: &anyhow::Error,
    ) -> Duration {
        let cooldown = self.set_rate_limit_cooldown(&entry.cooldown_key, err);
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(
                ::serde_json::json!({
                    "model_provider": entry.display_name,
                    "model": model,
                    "cooldown_ms": cooldown.as_millis(),
                })
            ),
            "ModelProvider rate-limited; trying next provider"
        );
        cooldown
    }

    /// Shared tail of the empty-completion retry path used by every chat method:
    /// record the empty attempt, warn, sleep the current backoff, then double it
    /// (capped). The caller owns the response-shape check and either retries
    /// or records its final failed attempt. See [`is_empty_completion`].
    async fn backoff_after_empty_completion(
        &self,
        failures: &mut FailureEvents,
        provider_name: &str,
        model: &str,
        attempt: u32,
        backoff_ms: &mut u64,
    ) {
        self.record_empty_completion_failure(failures, provider_name, model, attempt, true);
        tokio::time::sleep(Duration::from_millis(*backoff_ms)).await;
        *backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
    }

    /// Record an invalid but HTTP-successful provider response. The retry
    /// loops use this as an ordinary failure so that exhaustion advances to
    /// fallback instead of returning a successful blank turn.
    fn record_empty_completion_failure(
        &self,
        failures: &mut FailureEvents,
        provider_name: &str,
        model: &str,
        attempt: u32,
        retrying: bool,
    ) {
        push_failure(
            failures,
            attempt + 1,
            self.max_retries + 1,
            "empty_response",
            None,
        );
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                .with_attrs(::serde_json::json!({
                    "model_provider": provider_name,
                    "model": model,
                    "attempt": attempt + 1,
                    "retrying": retrying,
                })),
            if retrying {
                "Empty completion; retrying"
            } else {
                "Empty completion; retries exhausted"
            }
        );
    }
}

#[async_trait]
impl ModelProvider for ReliableModelProvider {
    fn has_stable_request_identity(&self, model: &str) -> bool {
        if self.model_providers.len() != 1
            || self
                .model_fallbacks
                .get(model)
                .is_some_and(|fallbacks| !fallbacks.is_empty())
        {
            return false;
        }

        self.model_providers
            .first()
            .is_some_and(|entry| entry.provider().has_stable_request_identity(model))
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        for entry in &self.model_providers {
            let provider_name = entry.display_name.as_str();
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({"model_provider": provider_name})),
                "Warming up model_provider connection pool"
            );
            if ProviderDispatch::from_ref(entry.provider())
                .warmup()
                .await
                .is_err()
            {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"model_provider": provider_name})),
                    "Warmup failed (non-fatal)"
                );
            }
        }
        Ok(())
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<String> {
        mark_current_dispatch_composite();
        let models = self.model_chain(model);
        let mut failures = FailureEvents::default();
        let mut refusal_seen = None;
        let mut rejected_attempt_usage = None;
        let mut final_cause_is_semantic_empty = stream_recovery_was_semantic_empty();
        let mut final_cause = None;
        let mut final_cause_provider = None;
        let mut terminal_provider_keys = HashSet::new();

        // Outer: model fallback chain. Middle: model_provider priority. Inner: retries.
        // Each iteration: attempt one (model_provider, model) call. On success, return
        // immediately. On non-retryable error, break to next model_provider. On
        // retryable error, sleep with exponential backoff and retry.
        for (model_slot, current_model) in models.iter().enumerate() {
            for (entry_index, entry) in self.model_providers.iter().enumerate() {
                if terminal_provider_keys.contains(&entry.cooldown_key) {
                    continue;
                }
                let provider_name = entry.display_name.as_str();
                let served_model = entry.served_model(current_model);
                if self.provider_should_skip_for_cooldown(entry) {
                    self.log_cooldown_skip(provider_name, served_model);
                    Self::record_cooldown_skip_failure(&mut failures, self.max_retries + 1);
                    continue;
                }

                let mut backoff_ms = self.base_backoff_ms;
                let mut last_error_detail: Option<String> = None;
                let mut last_diagnostic: Option<ProviderErrorDiagnostic> = None;

                for attempt in 0..=self.max_retries {
                    commit_safeguard_fallback(None);
                    match with_exact_dispatch_route(
                        entry.cooldown_key.clone(),
                        entry.served_model(current_model).to_string(),
                        ProviderDispatch::from_ref(entry.provider()).chat_with_system(
                            system_prompt,
                            message,
                            current_model,
                            temperature,
                        ),
                    )
                    .await
                    {
                        Ok(resp) => {
                            if is_empty_text_completion(&resp) {
                                if attempt < self.max_retries {
                                    self.backoff_after_empty_completion(
                                        &mut failures,
                                        provider_name,
                                        served_model,
                                        attempt,
                                        &mut backoff_ms,
                                    )
                                    .await;
                                    continue;
                                }
                                self.record_empty_completion_failure(
                                    &mut failures,
                                    provider_name,
                                    served_model,
                                    attempt,
                                    false,
                                );
                                final_cause_is_semantic_empty = true;
                                break;
                            }
                            if attempt > 0
                                || served_model != model
                                || model_slot != 0
                                || entry_index != 0
                                || self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.display_name.as_str())
                                    != Some(provider_name)
                            {
                                ::clawcrew_log::record!(INFO, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"model_provider": provider_name, "model": served_model, "attempt": attempt, "original_model": model})), "ModelProvider recovered (failover/retry)");
                                let primary = self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.candidate_name())
                                    .unwrap_or("");
                                let primary_provider = self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.display_name.as_str())
                                    .unwrap_or("");
                                let fallback_record = ProviderFallbackRecord::new_if_true_fallback(
                                    primary_provider,
                                    model,
                                    provider_name,
                                    served_model,
                                    model_slot != 0 || entry_index != 0,
                                    primary,
                                    entry.candidate_name(),
                                );
                                record_successful_provider_fallback(fallback_record.as_ref());
                                record_accepted_attempt(
                                    entry,
                                    current_model,
                                    fallback_record
                                        .as_ref()
                                        .map(ProviderFallbackRecord::attribution),
                                );
                            } else {
                                record_successful_provider_fallback(None);
                                record_accepted_attempt(entry, current_model, None);
                            }
                            record_refusal_rescue(&refusal_seen, model, served_model);
                            return Ok(resp);
                        }
                        Err(e) => {
                            remember_refusal(&mut refusal_seen, &mut rejected_attempt_usage, &e);
                            if is_semantic_empty_completion_error(&e) {
                                if attempt < self.max_retries {
                                    self.backoff_after_empty_completion(
                                        &mut failures,
                                        provider_name,
                                        served_model,
                                        attempt,
                                        &mut backoff_ms,
                                    )
                                    .await;
                                    continue;
                                }
                                self.record_empty_completion_failure(
                                    &mut failures,
                                    provider_name,
                                    served_model,
                                    attempt,
                                    false,
                                );
                                final_cause_is_semantic_empty = true;
                                break;
                            }
                            final_cause_is_semantic_empty = false;
                            // Context window exceeded: no history to truncate
                            // in chat_with_system, bail immediately.
                            if is_context_window_exceeded(&e) && !is_non_retryable(&e) {
                                let diagnostic = provider_error_diagnostic(&e);
                                push_failure(
                                    &mut failures,
                                    attempt + 1,
                                    self.max_retries + 1,
                                    "context_window",
                                    Some(&diagnostic),
                                );
                                let context_error = context_failure_aggregate(
                                    "Request exceeds model context window.",
                                    &failures,
                                );
                                return Err(reliable_terminal_error_with_cause(
                                    Some(entry.candidate_name()),
                                    failures,
                                    rejected_attempt_usage,
                                    false,
                                    Some(e),
                                )
                                .context(context_error));
                            }

                            let non_retryable_rate_limit = is_non_retryable_rate_limit(&e);
                            let non_retryable = is_non_retryable(&e) || non_retryable_rate_limit;
                            let rate_limited = is_rate_limited(&e);
                            let failure_reason = failure_reason(rate_limited, non_retryable);
                            let error_detail = compact_error_detail(&e);
                            let diagnostic = provider_error_diagnostic(&e);
                            last_error_detail = Some(error_detail.clone());
                            last_diagnostic = Some(diagnostic.clone());

                            push_failure(
                                &mut failures,
                                attempt + 1,
                                self.max_retries + 1,
                                failure_reason,
                                Some(&diagnostic),
                            );

                            // Rate-limit with rotatable keys: cycle to the next API key
                            // so the retry hits a different quota bucket.
                            if rate_limited
                                && !non_retryable_rate_limit
                                && let Some(new_key) = self.rotate_key()
                            {
                                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"model_provider": provider_name, "error": error_detail})), &format!("Rate limited; key rotation selected key ending ...{} \
                                     but cannot apply (ModelProvider trait has no set_api_key). \
                                     Retrying with original key.", &new_key[new_key.len().saturating_sub(4)..]));
                            }

                            if non_retryable {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        provider_failure_attrs(
                                            provider_name,
                                            served_model,
                                            &error_detail,
                                            &diagnostic,
                                        )
                                    ),
                                    "Non-retryable error, moving on"
                                );
                                if has_typed_non_retryable_marker(&e) {
                                    terminal_provider_keys.insert(entry.cooldown_key.clone());
                                }
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }

                            if rate_limited && self.model_providers.len() > 1 {
                                self.cool_down_rate_limited_provider(entry, served_model, &e);
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }

                            if attempt < self.max_retries {
                                let wait = self.compute_backoff(backoff_ms, &e);
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        provider_retry_attrs(
                                            provider_name,
                                            served_model,
                                            attempt + 1,
                                            wait,
                                            failure_reason,
                                            &error_detail,
                                            &diagnostic,
                                        )
                                    ),
                                    "ModelProvider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(wait)).await;
                                backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                            }
                            final_cause = Some(e);
                            final_cause_provider = Some(entry.candidate_name().to_string());
                        }
                    }
                }

                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(provider_exhausted_attrs(
                            provider_name,
                            served_model,
                            last_error_detail.as_deref(),
                            last_diagnostic.as_ref(),
                        )),
                    "Exhausted retries, trying next model_provider/model"
                );
            }

            if *current_model != model {
                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"original_model": model, "fallback_model": *current_model})), "Model fallback exhausted all model_providers, trying next fallback model");
            }
        }

        Err(reliable_terminal_error_with_cause(
            final_cause_provider
                .as_deref()
                .or_else(|| self.configured_provider_identity()),
            failures,
            rejected_attempt_usage,
            final_cause_is_semantic_empty,
            final_cause,
        ))
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<String> {
        mark_current_dispatch_composite();
        let models = self.model_chain(model);
        let mut failures = FailureEvents::default();
        let mut refusal_seen = None;
        let mut rejected_attempt_usage = None;
        let mut final_cause_is_semantic_empty = stream_recovery_was_semantic_empty();
        let mut final_cause = None;
        let mut final_cause_provider = None;
        let mut terminal_provider_keys = HashSet::new();
        let mut effective_messages = messages.to_vec();
        let mut context_truncated = false;

        for (model_slot, current_model) in models.iter().enumerate() {
            for (entry_index, entry) in self.model_providers.iter().enumerate() {
                if terminal_provider_keys.contains(&entry.cooldown_key) {
                    continue;
                }
                let provider_name = entry.display_name.as_str();
                let served_model = entry.served_model(current_model);
                if self.provider_should_skip_for_cooldown(entry) {
                    self.log_cooldown_skip(provider_name, served_model);
                    Self::record_cooldown_skip_failure(&mut failures, self.max_retries + 1);
                    continue;
                }

                let mut backoff_ms = self.base_backoff_ms;
                let mut last_error_detail: Option<String> = None;
                let mut last_diagnostic: Option<ProviderErrorDiagnostic> = None;

                for attempt in 0..=self.max_retries {
                    commit_safeguard_fallback(None);
                    match with_exact_dispatch_route(
                        entry.cooldown_key.clone(),
                        entry.served_model(current_model).to_string(),
                        ProviderDispatch::from_ref(entry.provider()).chat_with_history(
                            &effective_messages,
                            current_model,
                            temperature,
                        ),
                    )
                    .await
                    {
                        Ok(resp) => {
                            if is_empty_text_completion(&resp) {
                                if attempt < self.max_retries {
                                    self.backoff_after_empty_completion(
                                        &mut failures,
                                        provider_name,
                                        served_model,
                                        attempt,
                                        &mut backoff_ms,
                                    )
                                    .await;
                                    continue;
                                }
                                self.record_empty_completion_failure(
                                    &mut failures,
                                    provider_name,
                                    served_model,
                                    attempt,
                                    false,
                                );
                                final_cause_is_semantic_empty = true;
                                break;
                            }
                            if attempt > 0
                                || served_model != model
                                || model_slot != 0
                                || entry_index != 0
                                || context_truncated
                                || self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.display_name.as_str())
                                    != Some(provider_name)
                            {
                                ::clawcrew_log::record!(INFO, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"model_provider": provider_name, "model": served_model, "attempt": attempt, "original_model": model, "context_truncated": context_truncated})), "ModelProvider recovered (failover/retry)");
                                let primary = self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.candidate_name())
                                    .unwrap_or("");
                                let primary_provider = self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.display_name.as_str())
                                    .unwrap_or("");
                                let fallback_record = ProviderFallbackRecord::new_if_true_fallback(
                                    primary_provider,
                                    model,
                                    provider_name,
                                    served_model,
                                    model_slot != 0 || entry_index != 0,
                                    primary,
                                    entry.candidate_name(),
                                );
                                record_successful_provider_fallback(fallback_record.as_ref());
                                record_accepted_attempt(
                                    entry,
                                    current_model,
                                    fallback_record
                                        .as_ref()
                                        .map(ProviderFallbackRecord::attribution),
                                );
                            } else {
                                record_successful_provider_fallback(None);
                                record_accepted_attempt(entry, current_model, None);
                            }
                            record_refusal_rescue(&refusal_seen, model, served_model);
                            return Ok(resp);
                        }
                        Err(e) => {
                            remember_refusal(&mut refusal_seen, &mut rejected_attempt_usage, &e);
                            if is_semantic_empty_completion_error(&e) {
                                if attempt < self.max_retries {
                                    self.backoff_after_empty_completion(
                                        &mut failures,
                                        provider_name,
                                        served_model,
                                        attempt,
                                        &mut backoff_ms,
                                    )
                                    .await;
                                    continue;
                                }
                                self.record_empty_completion_failure(
                                    &mut failures,
                                    provider_name,
                                    served_model,
                                    attempt,
                                    false,
                                );
                                final_cause_is_semantic_empty = true;
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }
                            final_cause_is_semantic_empty = false;
                            // Context window exceeded: truncate history and retry
                            if is_context_window_exceeded(&e)
                                && !is_non_retryable(&e)
                                && !context_truncated
                            {
                                let diagnostic = provider_error_diagnostic(&e);
                                push_failure(
                                    &mut failures,
                                    attempt + 1,
                                    self.max_retries + 1,
                                    "context_window",
                                    Some(&diagnostic),
                                );
                                let dropped = truncate_for_context(&mut effective_messages);
                                if dropped > 0 {
                                    record_provider_context_truncation();
                                    context_truncated = true;
                                    ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"model_provider": provider_name, "model": served_model, "dropped": dropped, "remaining": effective_messages.len()})), "Context window exceeded; truncated history and retrying");
                                    continue; // Retry with truncated messages (counts as an attempt)
                                }
                                // No complete older turn can be removed safely.
                                let truncation_limit =
                                    context_truncation_limit(&effective_messages);
                                let context_error = context_failure_aggregate(
                                    &format!(
                                        "Request exceeds model context window and cannot be reduced without \
                                         breaking message/tool pairing ({truncation_limit}). Try using a model \
                                         with a larger context window, reducing the number of tools/skills, or \
                                         enabling compact_context in config."
                                    ),
                                    &failures,
                                );
                                return Err(reliable_terminal_error_with_cause(
                                    Some(entry.candidate_name()),
                                    failures,
                                    rejected_attempt_usage,
                                    false,
                                    Some(e),
                                )
                                .context(context_error));
                            }

                            let non_retryable_rate_limit = is_non_retryable_rate_limit(&e);
                            let non_retryable = is_non_retryable(&e) || non_retryable_rate_limit;
                            let rate_limited = is_rate_limited(&e);
                            let failure_reason = failure_reason(rate_limited, non_retryable);
                            let error_detail = compact_error_detail(&e);
                            let diagnostic = provider_error_diagnostic(&e);
                            last_error_detail = Some(error_detail.clone());
                            last_diagnostic = Some(diagnostic.clone());

                            push_failure(
                                &mut failures,
                                attempt + 1,
                                self.max_retries + 1,
                                failure_reason,
                                Some(&diagnostic),
                            );

                            if rate_limited
                                && !non_retryable_rate_limit
                                && let Some(new_key) = self.rotate_key()
                            {
                                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"model_provider": provider_name, "error": error_detail})), &format!("Rate limited; key rotation selected key ending ...{} \
                                     but cannot apply (ModelProvider trait has no set_api_key). \
                                     Retrying with original key.", &new_key[new_key.len().saturating_sub(4)..]));
                            }

                            if non_retryable {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        provider_failure_attrs(
                                            provider_name,
                                            served_model,
                                            &error_detail,
                                            &diagnostic,
                                        )
                                    ),
                                    "Non-retryable error, moving on"
                                );
                                if has_typed_non_retryable_marker(&e) {
                                    terminal_provider_keys.insert(entry.cooldown_key.clone());
                                }
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }

                            if rate_limited && self.model_providers.len() > 1 {
                                self.cool_down_rate_limited_provider(entry, served_model, &e);
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }

                            if attempt < self.max_retries {
                                let wait = self.compute_backoff(backoff_ms, &e);
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        provider_retry_attrs(
                                            provider_name,
                                            served_model,
                                            attempt + 1,
                                            wait,
                                            failure_reason,
                                            &error_detail,
                                            &diagnostic,
                                        )
                                    ),
                                    "ModelProvider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(wait)).await;
                                backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                            }
                            final_cause = Some(e);
                            final_cause_provider = Some(entry.candidate_name().to_string());
                        }
                    }
                }

                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(provider_exhausted_attrs(
                            provider_name,
                            served_model,
                            last_error_detail.as_deref(),
                            last_diagnostic.as_ref(),
                        )),
                    "Exhausted retries, trying next model_provider/model"
                );
            }
        }

        Err(reliable_terminal_error_with_cause(
            final_cause_provider
                .as_deref()
                .or_else(|| self.configured_provider_identity()),
            failures,
            rejected_attempt_usage,
            final_cause_is_semantic_empty,
            final_cause,
        ))
    }

    fn capabilities(&self) -> crate::traits::ProviderCapabilities {
        let mut capabilities = self
            .model_providers
            .first()
            .map(|entry| entry.provider().capabilities())
            .unwrap_or_default();
        // A request may advance past the primary after a retryable failure.
        // Report vision only when every reachable provider can accept images;
        // otherwise the turn engine must select a dedicated vision route before
        // dispatch instead of admitting an image that a fallback could reject.
        capabilities.vision = !self.model_providers.is_empty()
            && self
                .model_providers
                .iter()
                .all(|entry| entry.provider().supports_vision());
        capabilities.native_tool_calling = !self.model_providers.is_empty()
            && self
                .model_providers
                .iter()
                .all(|entry| entry.provider().supports_native_tools());
        capabilities
    }

    fn capabilities_for_model(&self, model: &str) -> crate::traits::ProviderCapabilities {
        let mut capabilities = self
            .model_providers
            .first()
            .map(|entry| entry.provider().capabilities_for_model(model))
            .unwrap_or_default();
        capabilities.vision = !self.model_providers.is_empty()
            && self
                .model_providers
                .iter()
                .all(|entry| entry.provider().capabilities_for_model(model).vision);
        capabilities.native_tool_calling = !self.model_providers.is_empty()
            && self.model_providers.iter().all(|entry| {
                entry
                    .provider()
                    .capabilities_for_model(model)
                    .native_tool_calling
            });
        capabilities
    }

    fn vision_limited_by(&self, model: &str) -> Option<String> {
        // Name the fallback entry that forces the ANDed `vision` above to
        // `false`, so error sites can blame it instead of the primary. When
        // the primary (index 0) is itself the non-vision entry there is no
        // fallback to blame - report `None` and let the caller fall back to
        // its ordinary "this model_provider" wording rather than mislabeling
        // the primary's own limitation as a fallback's.
        self.model_providers
            .iter()
            .enumerate()
            .find(|(_, entry)| !entry.provider().capabilities_for_model(model).vision)
            .and_then(|(index, entry)| (index != 0).then(|| entry.candidate_name().to_string()))
    }

    fn has_mixed_native_tool_support_for_model(&self, model: &str) -> bool {
        let mut has_native = false;
        let mut has_text_only = false;

        for entry in &self.model_providers {
            let provider = entry.provider();
            if provider.has_mixed_native_tool_support_for_model(model) {
                return true;
            }
            if provider.capabilities_for_model(model).native_tool_calling {
                has_native = true;
            } else {
                has_text_only = true;
            }
            if has_native && has_text_only {
                return true;
            }
        }

        false
    }

    fn supports_native_tools(&self) -> bool {
        // The turn loop selects one tool protocol before Reliable chooses a
        // candidate. A native request is therefore safe only when every
        // candidate the request may reach accepts native tool specifications.
        !self.model_providers.is_empty()
            && self
                .model_providers
                .iter()
                .all(|entry| entry.provider().supports_native_tools())
    }

    fn supports_vision(&self) -> bool {
        self.capabilities().vision
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<ChatResponse> {
        mark_current_dispatch_composite();
        let models = self.model_chain(model);
        let mut failures = FailureEvents::default();
        let mut refusal_seen = None;
        let mut final_cause_is_semantic_empty = stream_recovery_was_semantic_empty();
        let mut terminal_provider_keys = HashSet::new();
        let mut effective_messages = messages.to_vec();
        let mut context_truncated = false;
        let mut rejected_attempt_usage = None;
        let mut final_cause = None;
        let mut final_cause_provider = None;

        let has_other_candidate = models.len().saturating_mul(self.model_providers.len()) > 1;

        for (model_slot, current_model) in models.iter().enumerate() {
            for (entry_index, entry) in self.model_providers.iter().enumerate() {
                let Some(retry_limit) =
                    self.effective_retry_limit(model_slot, entry_index, has_other_candidate)
                else {
                    final_cause_provider = Some(entry.candidate_name().to_string());
                    continue;
                };
                if terminal_provider_keys.contains(&entry.cooldown_key) {
                    continue;
                }
                let provider_name = entry.display_name.as_str();
                let served_model = entry.served_model(current_model);
                if self.provider_should_skip_for_cooldown(entry) {
                    self.log_cooldown_skip(provider_name, served_model);
                    Self::record_cooldown_skip_failure(&mut failures, self.max_retries + 1);
                    continue;
                }

                let mut backoff_ms = self.base_backoff_ms;
                let mut last_error_detail: Option<String> = None;
                let mut last_diagnostic: Option<ProviderErrorDiagnostic> = None;

                for attempt in 0..=retry_limit {
                    commit_safeguard_fallback(None);
                    match with_exact_dispatch_route(
                        entry.cooldown_key.clone(),
                        entry.served_model(current_model).to_string(),
                        ProviderDispatch::from_ref(entry.provider()).chat_with_tools(
                            &effective_messages,
                            tools,
                            current_model,
                            temperature,
                        ),
                    )
                    .await
                    {
                        Ok(mut resp) => {
                            if is_empty_completion(&resp) {
                                if let Some(usage) = resp.usage.clone()
                                    && !has_reliable_call_accounting()
                                {
                                    accumulate_usage(&mut rejected_attempt_usage, Some(&usage));
                                }
                                if attempt < retry_limit {
                                    self.backoff_after_empty_completion(
                                        &mut failures,
                                        provider_name,
                                        served_model,
                                        attempt,
                                        &mut backoff_ms,
                                    )
                                    .await;
                                    continue;
                                }
                                self.record_empty_completion_failure(
                                    &mut failures,
                                    provider_name,
                                    served_model,
                                    attempt,
                                    false,
                                );
                                final_cause_is_semantic_empty = true;
                                break;
                            }
                            if let Some(usage) = rejected_attempt_usage.take()
                                && !has_reliable_call_accounting()
                            {
                                combine_response_usage(&mut resp, Some(usage));
                            }
                            if attempt > 0
                                || served_model != model
                                || model_slot != 0
                                || entry_index != 0
                                || context_truncated
                                || self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.display_name.as_str())
                                    != Some(provider_name)
                            {
                                ::clawcrew_log::record!(INFO, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"model_provider": provider_name, "model": served_model, "attempt": attempt, "original_model": model, "context_truncated": context_truncated})), "ModelProvider recovered (failover/retry)");
                                let primary = self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.candidate_name())
                                    .unwrap_or("");
                                let primary_provider = self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.display_name.as_str())
                                    .unwrap_or("");
                                let fallback_record = ProviderFallbackRecord::new_if_true_fallback(
                                    primary_provider,
                                    model,
                                    provider_name,
                                    served_model,
                                    model_slot != 0 || entry_index != 0,
                                    primary,
                                    entry.candidate_name(),
                                );
                                record_successful_provider_fallback(fallback_record.as_ref());
                                record_accepted_attempt(
                                    entry,
                                    current_model,
                                    fallback_record
                                        .as_ref()
                                        .map(ProviderFallbackRecord::attribution),
                                );
                            } else {
                                record_successful_provider_fallback(None);
                                record_accepted_attempt(entry, current_model, None);
                            }
                            record_refusal_rescue(&refusal_seen, model, served_model);
                            return Ok(resp);
                        }
                        Err(e) => {
                            remember_refusal(&mut refusal_seen, &mut rejected_attempt_usage, &e);
                            if is_semantic_empty_completion_error(&e) {
                                if attempt < retry_limit {
                                    self.backoff_after_empty_completion(
                                        &mut failures,
                                        provider_name,
                                        served_model,
                                        attempt,
                                        &mut backoff_ms,
                                    )
                                    .await;
                                    continue;
                                }
                                self.record_empty_completion_failure(
                                    &mut failures,
                                    provider_name,
                                    served_model,
                                    attempt,
                                    false,
                                );
                                final_cause_is_semantic_empty = true;
                                break;
                            }
                            final_cause_is_semantic_empty = false;
                            // Context window exceeded: truncate history and retry
                            if is_context_window_exceeded(&e)
                                && !is_non_retryable(&e)
                                && !context_truncated
                            {
                                let diagnostic = provider_error_diagnostic(&e);
                                push_failure(
                                    &mut failures,
                                    attempt + 1,
                                    self.max_retries + 1,
                                    "context_window",
                                    Some(&diagnostic),
                                );
                                let dropped = truncate_for_context(&mut effective_messages);
                                if dropped > 0 {
                                    record_provider_context_truncation();
                                    context_truncated = true;
                                    ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"model_provider": provider_name, "model": served_model, "dropped": dropped, "remaining": effective_messages.len()})), "Context window exceeded; truncated history and retrying");
                                    continue; // Retry with truncated messages (counts as an attempt)
                                }
                                // No complete older turn can be removed safely.
                                let truncation_limit =
                                    context_truncation_limit(&effective_messages);
                                let context_error = context_failure_aggregate(
                                    &format!(
                                        "Request exceeds model context window and cannot be reduced without \
                                         breaking message/tool pairing ({truncation_limit}). Try using a model \
                                         with a larger context window, reducing the number of tools/skills, or \
                                         enabling compact_context in config."
                                    ),
                                    &failures,
                                );
                                return Err(reliable_terminal_error_with_cause(
                                    Some(entry.candidate_name()),
                                    failures,
                                    rejected_attempt_usage,
                                    false,
                                    Some(e),
                                )
                                .context(context_error));
                            }

                            let non_retryable_rate_limit = is_non_retryable_rate_limit(&e);
                            let non_retryable = is_non_retryable(&e) || non_retryable_rate_limit;
                            let rate_limited = is_rate_limited(&e);
                            let failure_reason = failure_reason(rate_limited, non_retryable);
                            let error_detail = compact_error_detail(&e);
                            let diagnostic = provider_error_diagnostic(&e);
                            last_error_detail = Some(error_detail.clone());
                            last_diagnostic = Some(diagnostic.clone());

                            push_failure(
                                &mut failures,
                                attempt + 1,
                                self.max_retries + 1,
                                failure_reason,
                                Some(&diagnostic),
                            );

                            if rate_limited
                                && !non_retryable_rate_limit
                                && let Some(new_key) = self.rotate_key()
                            {
                                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"model_provider": provider_name, "error": error_detail})), &format!("Rate limited; key rotation selected key ending ...{} \
                                     but cannot apply (ModelProvider trait has no set_api_key). \
                                     Retrying with original key.", &new_key[new_key.len().saturating_sub(4)..]));
                            }

                            if non_retryable {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        provider_failure_attrs(
                                            provider_name,
                                            served_model,
                                            &error_detail,
                                            &diagnostic,
                                        )
                                    ),
                                    "Non-retryable error, moving on"
                                );
                                if has_typed_non_retryable_marker(&e) {
                                    terminal_provider_keys.insert(entry.cooldown_key.clone());
                                }
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }

                            if rate_limited && self.model_providers.len() > 1 {
                                self.cool_down_rate_limited_provider(entry, served_model, &e);
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }

                            if attempt < retry_limit {
                                let wait = self.compute_backoff(backoff_ms, &e);
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        provider_retry_attrs(
                                            provider_name,
                                            served_model,
                                            attempt + 1,
                                            wait,
                                            failure_reason,
                                            &error_detail,
                                            &diagnostic,
                                        )
                                    ),
                                    "ModelProvider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(wait)).await;
                                backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                            }
                            final_cause = Some(e);
                            final_cause_provider = Some(entry.candidate_name().to_string());
                        }
                    }
                }

                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(provider_exhausted_attrs(
                            provider_name,
                            served_model,
                            last_error_detail.as_deref(),
                            last_diagnostic.as_ref(),
                        )),
                    "Exhausted retries, trying next model_provider/model"
                );
            }
        }

        Err(reliable_terminal_error_with_cause(
            final_cause_provider
                .as_deref()
                .or_else(|| self.configured_provider_identity()),
            failures,
            rejected_attempt_usage,
            final_cause_is_semantic_empty,
            final_cause,
        ))
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<ChatResponse> {
        mark_current_dispatch_composite();
        let models = self.model_chain(model);
        let mut failures = FailureEvents::default();
        let mut streamed_refusal = take_stream_refusal_recovery();
        let mut refusal_seen = streamed_refusal.clone();
        let mut final_cause_is_semantic_empty = stream_recovery_was_semantic_empty();
        let mut terminal_provider_keys = HashSet::new();
        let mut effective_messages = request.messages.to_vec();
        let mut context_truncated = false;
        let mut rejected_attempt_usage = streamed_refusal
            .as_ref()
            .and_then(|refusal| refusal.usage.as_deref().cloned());
        // A streamed refusal is already a terminal typed failure for its
        // exact physical candidate. Retain it while skipping that candidate's
        // non-streaming replay; a later distinct failure deliberately
        // overwrites this cause below.
        let mut final_cause = streamed_refusal.as_ref().cloned().map(anyhow::Error::new);
        let mut final_cause_provider = streamed_refusal
            .as_ref()
            .and_then(|refusal| refusal.attempted_candidate.clone());

        let has_other_candidate = models.len().saturating_mul(self.model_providers.len()) > 1;

        for (model_slot, current_model) in models.iter().enumerate() {
            for (entry_index, entry) in self.model_providers.iter().enumerate() {
                let skip_streamed_refusal = streamed_refusal.as_ref().is_some_and(|refusal| {
                    refusal.requested_model == *current_model
                        && model_slot == 0
                        && refusal.attempted_candidate_index.map_or_else(
                            || {
                                refusal
                                    .attempted_candidate
                                    .as_deref()
                                    .map_or(entry_index == 0, |candidate| {
                                        candidate == entry.candidate_name()
                                    })
                            },
                            |index| index == entry_index,
                        )
                });
                if skip_streamed_refusal {
                    final_cause_provider = Some(entry.candidate_name().to_string());
                    streamed_refusal = None;
                    continue;
                }
                let Some(retry_limit) =
                    self.effective_retry_limit(model_slot, entry_index, has_other_candidate)
                else {
                    final_cause_provider = Some(entry.candidate_name().to_string());
                    continue;
                };
                if terminal_provider_keys.contains(&entry.cooldown_key) {
                    continue;
                }
                let provider_name = entry.display_name.as_str();
                let served_model = entry.served_model(current_model);
                if self.provider_should_skip_for_cooldown(entry) {
                    self.log_cooldown_skip(provider_name, served_model);
                    Self::record_cooldown_skip_failure(&mut failures, self.max_retries + 1);
                    continue;
                }

                let mut backoff_ms = self.base_backoff_ms;
                let mut last_error_detail: Option<String> = None;
                let mut last_diagnostic: Option<ProviderErrorDiagnostic> = None;

                for attempt in 0..=retry_limit {
                    commit_safeguard_fallback(None);
                    let req = ChatRequest {
                        messages: &effective_messages,
                        tools: request.tools,
                        thinking: request.thinking,
                    };
                    match with_exact_dispatch_route(
                        entry.cooldown_key.clone(),
                        entry.served_model(current_model).to_string(),
                        ProviderDispatch::from_ref(entry.provider()).chat(
                            req,
                            current_model,
                            temperature,
                        ),
                    )
                    .await
                    {
                        Ok(mut resp) => {
                            if is_empty_completion(&resp) {
                                if let Some(usage) = resp.usage.clone()
                                    && !has_reliable_call_accounting()
                                {
                                    accumulate_usage(&mut rejected_attempt_usage, Some(&usage));
                                }
                                if attempt < retry_limit {
                                    self.backoff_after_empty_completion(
                                        &mut failures,
                                        provider_name,
                                        served_model,
                                        attempt,
                                        &mut backoff_ms,
                                    )
                                    .await;
                                    continue;
                                }
                                self.record_empty_completion_failure(
                                    &mut failures,
                                    provider_name,
                                    served_model,
                                    attempt,
                                    false,
                                );
                                final_cause_is_semantic_empty = true;
                                break;
                            }
                            if let Some(usage) = rejected_attempt_usage.take()
                                && !has_reliable_call_accounting()
                            {
                                combine_response_usage(&mut resp, Some(usage));
                            }
                            if attempt > 0
                                || served_model != model
                                || model_slot != 0
                                || entry_index != 0
                                || context_truncated
                                || self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.display_name.as_str())
                                    != Some(provider_name)
                            {
                                ::clawcrew_log::record!(INFO, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"model_provider": provider_name, "model": served_model, "attempt": attempt, "original_model": model, "context_truncated": context_truncated})), "ModelProvider recovered (failover/retry)");
                                let primary = self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.candidate_name())
                                    .unwrap_or("");
                                let primary_provider = self
                                    .model_providers
                                    .first()
                                    .map(|entry| entry.display_name.as_str())
                                    .unwrap_or("");
                                let fallback_record = ProviderFallbackRecord::new_if_true_fallback(
                                    primary_provider,
                                    model,
                                    provider_name,
                                    served_model,
                                    model_slot != 0 || entry_index != 0,
                                    primary,
                                    entry.candidate_name(),
                                );
                                record_successful_provider_fallback(fallback_record.as_ref());
                                record_accepted_attempt(
                                    entry,
                                    current_model,
                                    fallback_record
                                        .as_ref()
                                        .map(ProviderFallbackRecord::attribution),
                                );
                            } else {
                                record_successful_provider_fallback(None);
                                record_accepted_attempt(entry, current_model, None);
                            }
                            record_refusal_rescue(&refusal_seen, model, served_model);
                            return Ok(resp);
                        }
                        Err(e) => {
                            remember_refusal(&mut refusal_seen, &mut rejected_attempt_usage, &e);
                            if is_semantic_empty_completion_error(&e) {
                                if attempt < retry_limit {
                                    self.backoff_after_empty_completion(
                                        &mut failures,
                                        provider_name,
                                        served_model,
                                        attempt,
                                        &mut backoff_ms,
                                    )
                                    .await;
                                    continue;
                                }
                                self.record_empty_completion_failure(
                                    &mut failures,
                                    provider_name,
                                    served_model,
                                    attempt,
                                    false,
                                );
                                final_cause_is_semantic_empty = true;
                                break;
                            }
                            final_cause_is_semantic_empty = false;
                            // Context window exceeded: truncate history and retry
                            if is_context_window_exceeded(&e)
                                && !is_non_retryable(&e)
                                && !context_truncated
                            {
                                let diagnostic = provider_error_diagnostic(&e);
                                push_failure(
                                    &mut failures,
                                    attempt + 1,
                                    self.max_retries + 1,
                                    "context_window",
                                    Some(&diagnostic),
                                );
                                let dropped = truncate_for_context(&mut effective_messages);
                                if dropped > 0 {
                                    record_provider_context_truncation();
                                    context_truncated = true;
                                    ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"model_provider": provider_name, "model": served_model, "dropped": dropped, "remaining": effective_messages.len()})), "Context window exceeded; truncated history and retrying");
                                    continue; // Retry with truncated messages (counts as an attempt)
                                }
                                // No complete older turn can be removed safely.
                                let truncation_limit =
                                    context_truncation_limit(&effective_messages);
                                let context_error = context_failure_aggregate(
                                    &format!(
                                        "Request exceeds model context window and cannot be reduced without \
                                         breaking message/tool pairing ({truncation_limit}). Try using a model \
                                         with a larger context window, reducing the number of tools/skills, or \
                                         enabling compact_context in config."
                                    ),
                                    &failures,
                                );
                                return Err(reliable_terminal_error_with_cause(
                                    Some(entry.candidate_name()),
                                    failures,
                                    rejected_attempt_usage,
                                    false,
                                    Some(e),
                                )
                                .context(context_error));
                            }

                            let non_retryable_rate_limit = is_non_retryable_rate_limit(&e);
                            let non_retryable = is_non_retryable(&e) || non_retryable_rate_limit;
                            let rate_limited = is_rate_limited(&e);
                            let failure_reason = failure_reason(rate_limited, non_retryable);
                            let error_detail = compact_error_detail(&e);
                            let diagnostic = provider_error_diagnostic(&e);
                            last_error_detail = Some(error_detail.clone());
                            last_diagnostic = Some(diagnostic.clone());

                            push_failure(
                                &mut failures,
                                attempt + 1,
                                self.max_retries + 1,
                                failure_reason,
                                Some(&diagnostic),
                            );

                            if rate_limited
                                && !non_retryable_rate_limit
                                && let Some(new_key) = self.rotate_key()
                            {
                                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"model_provider": provider_name, "error": error_detail})), &format!("Rate limited; key rotation selected key ending ...{} \
                                     but cannot apply (ModelProvider trait has no set_api_key). \
                                     Retrying with original key.", &new_key[new_key.len().saturating_sub(4)..]));
                            }

                            if non_retryable {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        provider_failure_attrs(
                                            provider_name,
                                            served_model,
                                            &error_detail,
                                            &diagnostic,
                                        )
                                    ),
                                    "Non-retryable error, moving on"
                                );
                                if has_typed_non_retryable_marker(&e) {
                                    terminal_provider_keys.insert(entry.cooldown_key.clone());
                                }
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }

                            if rate_limited && self.model_providers.len() > 1 {
                                self.cool_down_rate_limited_provider(entry, served_model, &e);
                                final_cause = Some(e);
                                final_cause_provider = Some(entry.candidate_name().to_string());
                                break;
                            }

                            if attempt < retry_limit {
                                let wait = self.compute_backoff(backoff_ms, &e);
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        provider_retry_attrs(
                                            provider_name,
                                            served_model,
                                            attempt + 1,
                                            wait,
                                            failure_reason,
                                            &error_detail,
                                            &diagnostic,
                                        )
                                    ),
                                    "ModelProvider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(wait)).await;
                                backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                            }
                            final_cause = Some(e);
                            final_cause_provider = Some(entry.candidate_name().to_string());
                        }
                    }
                }

                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(provider_exhausted_attrs(
                            provider_name,
                            served_model,
                            last_error_detail.as_deref(),
                            last_diagnostic.as_ref(),
                        )),
                    "Exhausted retries, trying next model_provider/model"
                );
            }

            if *current_model != model {
                ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"original_model": model, "fallback_model": *current_model})), "Model fallback exhausted all model_providers, trying next fallback model");
            }
        }

        Err(reliable_terminal_error_with_cause(
            final_cause_provider
                .as_deref()
                .or_else(|| self.configured_provider_identity()),
            failures,
            rejected_attempt_usage,
            final_cause_is_semantic_empty,
            final_cause,
        ))
    }

    fn supports_streaming(&self) -> bool {
        self.model_providers
            .iter()
            .any(|entry| entry.provider().supports_streaming())
    }

    fn supports_streaming_tool_events(&self) -> bool {
        self.model_providers
            .iter()
            .any(|entry| entry.provider().supports_streaming_tool_events())
    }

    fn stream_chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: Option<f64>,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
        mark_current_dispatch_composite();
        commit_safeguard_fallback(None);
        let needs_tool_events = request.tools.is_some_and(|tools| !tools.is_empty());

        for (entry_index, entry) in self.model_providers.iter().enumerate() {
            let provider_name = entry.display_name.as_str();
            let model_provider = entry.provider();
            if !model_provider.supports_streaming() || !options.enabled {
                continue;
            }

            if needs_tool_events && !model_provider.supports_streaming_tool_events() {
                continue;
            }

            if self.provider_should_skip_for_cooldown(entry) {
                self.log_cooldown_skip(provider_name, entry.served_model(model));
                continue;
            }

            let current_model = self
                .model_chain(model)
                .first()
                .copied()
                .unwrap_or(model)
                .to_string();
            let served_model = entry.served_model(&current_model).to_string();
            let streamed_candidate = entry.candidate_name().to_string();
            let fallback_record = ProviderFallbackRecord::new_if_true_fallback(
                self.model_providers
                    .first()
                    .map(|entry| entry.display_name.as_str())
                    .unwrap_or(""),
                model,
                provider_name,
                &served_model,
                entry_index != 0,
                self.model_providers
                    .first()
                    .map(|entry| entry.candidate_name())
                    .unwrap_or(""),
                entry.candidate_name(),
            );
            let req = ChatRequest {
                messages: request.messages,
                tools: request.tools,
                thinking: request.thinking,
            };
            let stream = stream_with_exact_dispatch_route(
                entry.cooldown_key.clone(),
                served_model.clone(),
                ProviderDispatch::from_ref(model_provider).stream_chat(
                    req,
                    &served_model,
                    temperature,
                    options,
                ),
            );
            let stream = stream
                .map(move |mut event| {
                    if let Err(StreamError::ModelRefusal(ref mut refusal)) = event {
                        refusal.attempted_candidate = Some(streamed_candidate.clone());
                        refusal.attempted_candidate_index = Some(entry_index);
                    }
                    event
                })
                .boxed();
            let stream = stream_with_recovery_identity(stream, 0, entry_index);
            let accepted_route = AcceptedRoute::new(
                entry.cooldown_key.clone(),
                served_model.clone(),
                fallback_record
                    .as_ref()
                    .map(ProviderFallbackRecord::attribution),
            );
            // Usage is billing metadata, not stream acceptance. A provider can
            // report usage and then fail before Final; recording the fallback
            // at Usage would leak a route that never produced an accepted
            // completion to legacy direct callers.
            return stream_with_success_recording(
                stream,
                fallback_record,
                accepted_route,
                |event| matches!(event, StreamEvent::Final),
            );
        }

        let message = if needs_tool_events {
            "No model_provider supports streaming tool events".to_string()
        } else {
            "No model_provider supports streaming".to_string()
        };
        stream_as_dispatch_composite(
            stream::once(async move { Err(super::traits::StreamError::ModelProvider(message)) })
                .boxed(),
        )
    }

    fn stream_chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: Option<f64>,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        mark_current_dispatch_composite();
        // Try each model_provider/model combination for streaming
        // For streaming, we use the first model_provider that supports it and has streaming enabled
        for (provider_index, entry) in self.model_providers.iter().enumerate() {
            let provider_name = entry.display_name.as_str();
            let model_provider = entry.provider();
            if !model_provider.supports_streaming() || !options.enabled {
                continue;
            }

            if self.provider_should_skip_for_cooldown(entry) {
                self.log_cooldown_skip(provider_name, entry.served_model(model));
                continue;
            }

            // Clone model_provider data for the stream
            // Try the first model in the chain for streaming
            let current_model = match self.model_chain(model).first() {
                Some(m) => (*m).to_string(),
                None => model.to_string(),
            };
            let served_model = entry.served_model(&current_model).to_string();
            let fallback_record = ProviderFallbackRecord::new_if_true_fallback(
                self.model_providers
                    .first()
                    .map(|entry| entry.display_name.as_str())
                    .unwrap_or(""),
                model,
                provider_name,
                &served_model,
                provider_index != 0,
                self.model_providers
                    .first()
                    .map(|entry| entry.candidate_name())
                    .unwrap_or(""),
                entry.candidate_name(),
            );

            // For streaming, we attempt once and propagate errors
            // The caller can retry the entire request if needed
            let stream = stream_with_exact_dispatch_route(
                entry.cooldown_key.clone(),
                served_model.clone(),
                ProviderDispatch::from_ref(model_provider).stream_chat_with_system(
                    system_prompt,
                    message,
                    &served_model,
                    temperature,
                    options,
                ),
            );
            let accepted_route = AcceptedRoute::new(
                entry.cooldown_key.clone(),
                served_model.clone(),
                fallback_record
                    .as_ref()
                    .map(ProviderFallbackRecord::attribution),
            );

            return stream_with_success_recording(
                stream,
                fallback_record,
                accepted_route,
                |chunk| chunk.is_final,
            );
        }

        // No streaming support available
        stream_as_dispatch_composite(
            stream::once(async move {
                Err(super::traits::StreamError::ModelProvider(
                    "No model_provider supports streaming".to_string(),
                ))
            })
            .boxed(),
        )
    }

    fn stream_chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: Option<f64>,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        mark_current_dispatch_composite();
        // Try each model_provider/model combination for streaming with history.
        // Mirrors stream_chat_with_system but delegates to the underlying
        // model_provider's stream_chat_with_history, preserving the full conversation.
        for (provider_index, entry) in self.model_providers.iter().enumerate() {
            let provider_name = entry.display_name.as_str();
            let model_provider = entry.provider();
            if !model_provider.supports_streaming() || !options.enabled {
                continue;
            }

            if self.provider_should_skip_for_cooldown(entry) {
                self.log_cooldown_skip(provider_name, entry.served_model(model));
                continue;
            }

            let current_model = match self.model_chain(model).first() {
                Some(m) => (*m).to_string(),
                None => model.to_string(),
            };
            let served_model = entry.served_model(&current_model).to_string();
            let fallback_record = ProviderFallbackRecord::new_if_true_fallback(
                self.model_providers
                    .first()
                    .map(|entry| entry.display_name.as_str())
                    .unwrap_or(""),
                model,
                provider_name,
                &served_model,
                provider_index != 0,
                self.model_providers
                    .first()
                    .map(|entry| entry.candidate_name())
                    .unwrap_or(""),
                entry.candidate_name(),
            );

            let stream = stream_with_exact_dispatch_route(
                entry.cooldown_key.clone(),
                served_model.clone(),
                ProviderDispatch::from_ref(model_provider).stream_chat_with_history(
                    messages,
                    &served_model,
                    temperature,
                    options,
                ),
            );
            let accepted_route = AcceptedRoute::new(
                entry.cooldown_key.clone(),
                served_model.clone(),
                fallback_record
                    .as_ref()
                    .map(ProviderFallbackRecord::attribution),
            );

            return stream_with_success_recording(
                stream,
                fallback_record,
                accepted_route,
                |chunk| chunk.is_final,
            );
        }

        // No streaming support available
        stream_as_dispatch_composite(
            stream::once(async move {
                Err(super::traits::StreamError::ModelProvider(
                    "No model_provider supports streaming".to_string(),
                ))
            })
            .boxed(),
        )
    }
}

impl ::clawcrew_api::attribution::Attributable for ReliableModelProvider {
    fn role(&self) -> ::clawcrew_api::attribution::Role {
        match self.model_providers.first() {
            Some(entry) => ::clawcrew_api::attribution::Attributable::role(entry.provider()),
            None => ::clawcrew_api::attribution::Role::System,
        }
    }

    fn alias(&self) -> &str {
        // Delegate to the primary inner provider for the same reason
        // as `role()`. Falls back to the wrapper's own configured alias
        // when no inner provider is registered.
        match self.model_providers.first() {
            Some(entry) => ::clawcrew_api::attribution::Attributable::alias(entry.provider()),
            None => &self.alias,
        }
    }
}

#[cfg(test)]
mod tests;

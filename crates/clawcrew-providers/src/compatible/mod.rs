//! Generic OpenAI-compatible model_provider.
//! Most LLM APIs follow the same `/v1/chat/completions` format.
//! This module provides a single implementation that works for all of them.

use crate::auth::AuthService;
use crate::multimodal;
use crate::openai::{NativeToolFunctionSpec, NativeToolSpec};
use crate::opencode_session::OPENCODE_SESSION_HEADER;
use crate::stream_guard::AbortOnDrop;
use crate::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    ModelProvider, StreamChunk, StreamError, StreamEvent, StreamOptions, StreamResult,
    ToolCall as ProviderToolCall,
};
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use reqwest::{
    Client, ClientBuilder,
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use serde::{Deserialize, Serialize};
use clawcrew_config::schema::{CacheTtl, ToolResultImagePolicy};

const TOOL_RESULT_IMAGE_OMITTED_NOTICE: &str = "[tool-result image omitted by provider policy]";

/// A model_provider that speaks the OpenAI-compatible chat completions API.
/// Used by: Venice, Vercel AI Gateway, Cloudflare AI Gateway, Moonshot,
/// Synthetic, `OpenCode` Zen, `OpenCode` Go, `Z.AI`, `GLM`, `MiniMax`, Bedrock, Qianfan, Groq, Mistral, `xAI`, etc.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone)]
pub struct OpenAiCompatibleModelProvider {
    /// `[providers.models.<alias>]` key this provider was constructed
    /// under. Used by the `Attributable` impl so log emissions carry the
    /// real composite (`<type>.<alias>`) instead of the bare type.
    pub alias: String,
    pub name: String,
    pub base_url: String,
    canonical_base_url: Option<&'static str>,
    pub credential: Option<String>,
    auth_service: Option<AuthService>,
    auth_model_provider: Option<String>,
    auth_profile_override: Option<String>,
    pub auth_header: AuthStyle,
    supports_vision: bool,
    tool_result_image_policy: ToolResultImagePolicy,
    /// Operator `[multimodal]` policy for this provider's image-marker
    /// expansion pass. Resolved once at build time.
    multimodal: clawcrew_config::schema::MultimodalConfig,
    user_agent: Option<String>,
    /// When true, collect all `system` messages and prepend their content
    /// to the first `user` message, then drop the system messages.
    /// Required for model_providers that reject `role: system` (e.g. MiniMax).
    merge_system_into_user: bool,
    /// Whether this model_provider supports OpenAI-style native tool calling.
    /// When false, tools are injected into the system prompt as text.
    native_tool_calling: bool,
    /// HTTP request timeout in seconds for LLM API calls. Default: 120.
    timeout_secs: u64,
    /// Extra HTTP headers to include in all API requests.
    extra_headers: std::collections::HashMap<String, String>,
    /// Optional reasoning effort for GPT-5/Codex-compatible backends.
    reasoning_effort: Option<String>,
    /// Whether stored assistant reasoning should be replayed on outbound
    /// assistant history messages. Some providers reject reasoning fields as
    /// input even though they may return them in responses.
    replay_assistant_reasoning: bool,
    /// Whether Anthropic prompt-cache breakpoints should be injected into
    /// outbound request bodies (system prompt; rolling last message).
    /// Drives `capabilities().prompt_caching` and the request-build path.
    cache_passthrough: bool,
    /// Cache entry lifetime carried by every breakpoint injected behind
    /// `cache_passthrough`. One TTL per request by design. Inert without
    /// passthrough: no markers are placed, so nothing carries a TTL.
    cache_ttl: Option<CacheTtl>,
    /// Custom API path suffix (e.g. "/v2/generate").
    /// When set, overrides the default `/chat/completions` path detection.
    api_path: Option<String>,
    /// Maximum output tokens to include in API requests.
    max_tokens: Option<u32>,
    /// models.dev catalog key for this model_provider (e.g. "xai").
    /// When set, `list_models` fetches from the models.dev catalog.
    models_dev_key: Option<String>,
    openrouter_vendor_prefix: Option<String>,
    local_model_tool_sanitize: bool,
    /// Some OpenAI-compatible local servers, such as Ollama, expose `/models`
    /// without authentication. Keep the default credential-gated for hosted
    /// providers so missing credentials still fall through to catalog sources.
    /// When `true`, the `/models` endpoint is treated as publicly accessible.
    public_model_listing: bool,
    /// Raw PEM bytes of a custom CA certificate for TLS connections.
    /// Loaded from disk once at construction; not refreshed across config reloads.
    tls_ca_cert_pem: Option<Vec<u8>>,
    /// Extra JSON fields merged into every API request body.
    extra_body: Option<serde_json::Value>,
    /// Memoized cleaned tool schemas: each registered schema is cleaned once
    /// per strategy per provider instance and then `Arc`-shared into every
    /// request body instead of being deep-copied per request. `Arc` so
    /// provider clones (e.g. the streaming path's owned copy) share one
    /// memo. Paths that rebuild the provider per call (e.g. the
    /// per-iteration vision route) start it empty each time.
    schema_cache: std::sync::Arc<clawcrew_api::schema::SchemaCleanCache>,
}

/// How the model_provider expects the API key to be sent.
#[derive(Debug, Clone)]
pub enum AuthStyle {
    /// `Authorization: Bearer <key>`
    Bearer,
    /// `x-api-key: <key>` (used by some Chinese model_providers)
    XApiKey,
    /// Custom header name
    Custom(String),
    /// Zhipu/GLM JWT auth: the credential is `id.secret`, and a short-lived
    /// JWT (HMAC-SHA256, 3.5 min expiry) is generated per request.
    /// Used by Z.AI and GLM model_providers.
    ZhipuJwt,
}

/// Sanitize a tool-call `arguments` string before it is re-serialized into an
/// outbound OpenAI-compatible chat-completions request.
///
/// Several strict upstream providers (Cohere, OpenInference, Nvidia …,
/// surfaced most often through OpenRouter) reject requests where
/// `tool_calls[].function.arguments` is not well-formed JSON. Smaller /
/// reasoning models sometimes emit a malformed arguments string; when that
/// happens the whole turn fails with HTTP 400 and the user receives the
/// generic fallback instead of the agent's response.
///
/// Contract:
/// - empty / whitespace-only → `"{}"` (every upstream accepts this)
/// - valid JSON → returned unchanged
/// - invalid JSON → WARN-logged with **safe metadata only** (function name,
///   payload length, stable error key), then `"{}"`. The raw arguments
///   string is **never** recorded, because tool-call arguments can contain
///   commands, URLs, credentials, file paths, or user content and WARN
///   events enter the broadcast and rolling-persistence path regardless of
///   the tool/LLM content-capture policy.
///
/// This is the single source of truth for the tool-call arguments
/// normalization contract. The streaming accumulator's
/// `StreamToolCallAccumulator::into_provider_tool_call` and all typed
/// providers' outbound `convert_messages` paths route through here.
pub(crate) fn sanitize_tool_arguments(function_name: &str, arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return "{}".to_string();
    }
    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(serde_json::Value::Object(_)) => return arguments.to_string(),
        Ok(_non_object) => {
            // Accept only JSON objects; null, arrays, strings, numbers, and
            // booleans do not satisfy a strict-provider function-arguments
            // contract (reported by Cohere, tracked by OpenRouter's
            // auto-exacto validator).
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "function": function_name,
                        "payload_len": arguments.len(),
                        "error_key": "tool_args_not_object",
                    })),
                "Non-object tool-call arguments being sent to strict upstream provider, dropping to empty object"
            );
            return "{}".to_string();
        }
        Err(_) => {}
    }
    ::clawcrew_log::record!(
        WARN,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
            .with_attrs(::serde_json::json!({
                "function": function_name,
                "payload_len": arguments.len(),
                "error_key": "tool_args_invalid_json",
            })),
        "Invalid JSON in tool-call arguments being sent to upstream provider, dropping to empty object"
    );
    "{}".to_string()
}

/// Generate a Zhipu JWT from an `id.secret` API key.
/// Returns `Authorization: Bearer <jwt>` value. Token is valid for 3.5 minutes.
fn zhipu_jwt_bearer(credential: &str) -> Result<String, String> {
    let (id, secret) = credential
        .split_once('.')
        .ok_or_else(|| "Zhipu API key must be in 'id.secret' format".to_string())?;
    if id.is_empty() || secret.is_empty() {
        return Err("Zhipu API key must contain non-empty id and secret components".to_string());
    }

    #[allow(clippy::cast_possible_truncation)] // millis won't exceed u64 until year 584 million
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let exp_ms = now_ms + 210_000; // 3.5 minutes

    // Header: {"alg":"HS256","typ":"JWT","sign_type":"SIGN"}
    let header_b64 = base64url_no_pad(br#"{"alg":"HS256","typ":"JWT","sign_type":"SIGN"}"#);
    let payload = serde_json::json!({
        "api_key": id,
        "exp": exp_ms,
        "timestamp": now_ms,
    })
    .to_string();
    let payload_b64 = base64url_no_pad(payload.as_bytes());

    let signing_input = format!("{header_b64}.{payload_b64}");
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let sig = ring::hmac::sign(&key, signing_input.as_bytes());
    let sig_b64 = base64url_no_pad(sig.as_ref());

    Ok(format!("Bearer {signing_input}.{sig_b64}"))
}

fn base64url_no_pad(data: &[u8]) -> String {
    use base64::engine::{Engine, general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD.encode(data)
}

/// Apply auth to a request builder (usable from spawned tasks without `&self`).
/// When `credential` is `None` (e.g. local LLM servers that require no API key),
/// the request is returned unchanged -- no auth header is added.
///
/// Within the OpenAI-compatible family builder this is where a stored
/// credential becomes an outbound header, and the requests built here --
/// chat, model listing, and context-window discovery
/// ([`crate::fetch_context_window`]) -- share it. That is what keeps a family
/// whose credential must be transformed before it is usable
/// (`AuthStyle::ZhipuJwt` mints a short-lived JWT from the stored `id.secret`)
/// from having one of those paths transform it while another sends it raw.
///
/// Scoped deliberately: providers implemented outside this module (Anthropic,
/// Gemini, Bedrock, …) authenticate their own way and make no claim here.
///
/// Fails closed. `AuthStyle::ZhipuJwt` mints a short-lived JWT from a stored
/// `id.secret`; when that minting fails the request is *not* built. The
/// alternative — attaching no `Authorization` header and sending anyway —
/// produces a request that can only ever be rejected upstream, and reports
/// the refusal as whatever status the provider happens to return rather than
/// as the local credential problem it is.
///
/// `provider` names the caller for the refusal record. Both the request path
/// and context-window discovery pass it explicitly rather than relying on an
/// ambient span, because discovery builds its probe outside any per-provider
/// span.
pub(crate) fn apply_auth_to_request(
    req: reqwest::RequestBuilder,
    style: &AuthStyle,
    credential: Option<&str>,
    provider: &str,
) -> anyhow::Result<reqwest::RequestBuilder> {
    let credential = match credential {
        Some(c) => c,
        None => return Ok(req),
    };
    Ok(match style {
        AuthStyle::Bearer => req.header("Authorization", format!("Bearer {credential}")),
        AuthStyle::XApiKey => req.header("x-api-key", credential),
        AuthStyle::Custom(header) => req.header(header, credential),
        AuthStyle::ZhipuJwt => req.header(
            "Authorization",
            zhipu_jwt_bearer_or_refuse(credential, provider)?,
        ),
    })
}

/// Mint the `Authorization` value for [`AuthStyle::ZhipuJwt`], or refuse.
///
/// Split out so the refusal is one place: the reason is logged against the
/// provider that owns the credential and turned into an operator-readable
/// error whose text is built only from the static failure reason, never from
/// the credential itself.
fn zhipu_jwt_bearer_or_refuse(credential: &str, provider: &str) -> anyhow::Result<String> {
    zhipu_jwt_bearer(credential).map_err(|reason| {
        ::clawcrew_log::record!(
            ERROR,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                .with_attrs(::serde_json::json!({
                    "model_provider": provider,
                    "auth_style": "zhipu_jwt",
                    "reason": &reason,
                    "error_key": "provider_credential_unusable",
                })),
            "compatible: stored credential could not be converted into a request token; \
             refusing to send the request"
        );
        anyhow::Error::msg(format!(
            "{provider}: stored credential cannot be converted into a request token \
             ({reason}); no request was sent"
        ))
    })
}

fn structured_api_error_message(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;

    let nested_message = object.get("error").and_then(|error| match error {
        serde_json::Value::Object(_) => structured_api_error_message(error),
        serde_json::Value::String(error) => serde_json::from_str(error)
            .ok()
            .and_then(|nested| structured_api_error_message(&nested)),
        _ => None,
    });
    if let Some(message) = nested_message {
        return Some(message);
    }

    for key in ["message", "detail"] {
        if let Some(message) = object
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty())
        {
            return Some(message.to_string());
        }
    }

    object
        .get("error")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
}

fn streaming_api_error(status: reqwest::StatusCode, body: &str) -> StreamError {
    let message = serde_json::from_str(body)
        .ok()
        .and_then(|value| structured_api_error_message(&value));
    let sanitized = super::sanitize_api_error(message.as_deref().unwrap_or(body));
    StreamError::ModelProvider(format!("{status}: {sanitized}"))
}

/// Upper bound on a `/models` catalog response buffered before parsing. Real
/// catalogs run to at most a few hundred KB (hundreds of models with pricing),
/// so this leaves generous headroom while stopping a misbehaving or compromised
/// router from making the client buffer an unbounded body — a boundary that
/// matters most on the public, credential-free listing path a `PUBLIC_MODEL_LISTING`
/// family (ZeroRouter, Kilo, AtlasCloud) exposes.
const MAX_MODELS_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

/// Read a response body into memory, refusing anything past `max_bytes`: a
/// declared `Content-Length` over the cap fails fast, and a stream that grows
/// past it fails as the bytes arrive (so a lying or absent `Content-Length`
/// cannot get around the bound). Mirrors the bounded reader in `clawcrew-channels`
/// so both behave identically, without taking a cross-crate dependency for it.
async fn read_body_capped(
    mut response: reqwest::Response,
    max_bytes: u64,
) -> anyhow::Result<Vec<u8>> {
    if let Some(content_length) = response.content_length()
        && content_length > max_bytes
    {
        anyhow::bail!(
            "response body content length {content_length} exceeds {max_bytes}-byte limit"
        );
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let next_len = u64::try_from(body.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if next_len > max_bytes {
            anyhow::bail!("response body exceeds {max_bytes}-byte limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    /// Pricing data from the provider's `/models` endpoint.
    /// Kilo Gateway: `{"pricing": {"prompt": "0", "completion": "0"}}`
    /// OpenRouter: `{"pricing": {"prompt": "0.000003", "completion": "0.000015"}}`
    /// Values are per-token rates (e.g. "0.000005" = $5/1M tokens).
    #[serde(default)]
    pricing: Option<clawcrew_api::model_provider::ModelPricing>,
}

fn normalize_model_ids(body: ModelsResponse) -> Vec<String> {
    let mut ids: Vec<String> = body
        .data
        .into_iter()
        .map(|e| e.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort();
    ids
}

/// Extract model IDs with pricing from a ModelsResponse.
/// Returns sorted list of `ModelInfo` with pricing data where available.
fn normalize_models_with_pricing(
    body: ModelsResponse,
) -> Vec<clawcrew_api::model_provider::ModelInfo> {
    use clawcrew_api::model_provider::ModelInfo;
    let mut models: Vec<ModelInfo> = body
        .data
        .into_iter()
        .filter(|e| !e.id.trim().is_empty())
        .map(|e| ModelInfo {
            id: e.id.trim().to_string(),
            pricing: e.pricing,
            // OpenAI-compatible `/v1/models` has no context-window field.
            context_window: None,
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models
}

/// Map a models.dev listing into `ModelInfo`, carrying through the catalog's
/// context window. A model the catalog gives no `limit.context` for stays
/// `None` — "unknown", never a stub value.
fn models_dev_to_model_info(
    models: Vec<(String, Option<usize>)>,
) -> Vec<clawcrew_api::model_provider::ModelInfo> {
    use clawcrew_api::model_provider::ModelInfo;
    models
        .into_iter()
        .map(|(id, context_window)| ModelInfo {
            id,
            pricing: None,
            context_window,
        })
        .collect()
}

/// Typed builder for [`OpenAiCompatibleModelProvider`].
///
/// `alias` (the config key this provider was constructed under) is the
/// only argument passed to [`OpenAiCompatibleModelProvider::builder`].
/// Every other field — including the semantically-required
/// `display_name` / `base_url` / `auth_style` — is set via a labelled
/// chain method so call sites read as prose instead of a comma-counted
/// tuple. `build()` panics if any of `display_name`, `base_url`, or
/// `auth_style` were omitted; there are no sensible defaults for those.
#[must_use]
pub struct OpenAiCompatibleBuilder {
    alias: String,
    name: Option<String>,
    base_url: Option<String>,
    canonical_base_url: Option<&'static str>,
    credential: Option<String>,
    auth_style: Option<AuthStyle>,
    supports_vision: bool,
    tool_result_image_policy: ToolResultImagePolicy,
    multimodal: clawcrew_config::schema::MultimodalConfig,
    user_agent: Option<String>,
    /// Set via [`OpenAiCompatibleBuilder::merge_system_into_user`] — the
    /// combined "merge + drop native tool calling" preset. Distinct from
    /// [`OpenAiCompatibleBuilder::merge_system_into_user_preserving_native`]
    /// which keeps native tools on.
    merge_system_into_user: bool,
    /// Set via [`OpenAiCompatibleBuilder::merge_system_into_user_preserving_native`]
    /// to enable the merge behaviour without disabling native tool calling.
    merge_system_into_user_preserve_native: bool,
    /// Set to `Some(false)` by [`OpenAiCompatibleBuilder::without_native_tools`].
    /// `None` preserves the default derived from `merge_system_into_user`.
    native_tool_calling_override: Option<bool>,
    timeout_secs: Option<u64>,
    extra_headers: std::collections::HashMap<String, String>,
    reasoning_effort: Option<String>,
    /// Set to `Some(false)` by
    /// [`OpenAiCompatibleBuilder::without_assistant_reasoning_replay`]. `None`
    /// preserves the default (replay enabled).
    replay_assistant_reasoning_override: Option<bool>,
    /// Set by [`OpenAiCompatibleBuilder::with_cache_passthrough`]. Default
    /// `false`: requests and capability reporting are unchanged.
    cache_passthrough: bool,
    /// Set by [`OpenAiCompatibleBuilder::with_cache_ttl`]. Default `None`:
    /// breakpoints keep the 5-minute API default.
    cache_ttl: Option<CacheTtl>,
    api_path: Option<String>,
    max_tokens: Option<u32>,
    models_dev_key: Option<String>,
    openrouter_vendor_prefix: Option<String>,
    local_model_tool_sanitize: bool,
    public_model_listing: bool,
    tls_ca_cert_path: Option<String>,
    extra_body: Option<serde_json::Value>,
    auth_model_provider: Option<String>,
    auth_service: Option<AuthService>,
    auth_profile_override: Option<String>,
}

impl OpenAiCompatibleBuilder {
    /// Human-readable display name (e.g. `"Groq"`, `"MiniMax"`). Surfaced
    /// in logs, `Attributable` output, and the onboarding UI. Required.
    pub fn display_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    /// Base URL for the provider's `/chat/completions` endpoint. Trailing
    /// slashes are stripped so callers need not care whether config
    /// supplied them. Required.
    pub fn base_url(mut self, base_url: &str) -> Self {
        self.base_url = Some(base_url.trim_end_matches('/').to_string());
        self
    }

    pub(crate) fn canonical_base_url(mut self, base_url: &'static str) -> Self {
        self.canonical_base_url = Some(base_url);
        self
    }

    /// Explicit API credential. `None` (the default) leaves this provider
    /// unauthenticated, which is how local LLM servers (Ollama,
    /// llama.cpp) are constructed. Whitespace-only inputs are normalized
    /// to `None` so a stray `Some("   ")` from config cannot produce a
    /// bogus `Bearer    ` header.
    pub fn credential(mut self, credential: Option<&str>) -> Self {
        self.credential = credential
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        self
    }

    /// How this provider expects the API key to be sent. Required.
    pub fn auth_style(mut self, style: AuthStyle) -> Self {
        self.auth_style = Some(style);
        self
    }

    /// Enable OpenAI-style multimodal (image) inputs on this provider.
    pub fn vision(mut self, supports_vision: bool) -> Self {
        self.supports_vision = supports_vision;
        self
    }

    /// Set the policy for image markers in native role=`tool` results.
    pub fn tool_result_image_policy(mut self, policy: ToolResultImagePolicy) -> Self {
        self.tool_result_image_policy = policy;
        self
    }

    /// Set the root `[multimodal]` policy used when expanding `[IMAGE:...]`
    /// markers into inline data URIs. Without this the provider falls back to
    /// library defaults and operator limits never reach the outbound request.
    pub fn multimodal(mut self, config: clawcrew_config::schema::MultimodalConfig) -> Self {
        self.multimodal = config;
        self
    }

    /// Set a custom `User-Agent` header for outbound requests.
    ///
    /// Required by providers whose routing / policy stack keys off the UA
    /// string (for example Kimi Code).
    pub fn user_agent(mut self, user_agent: &str) -> Self {
        self.user_agent = Some(user_agent.to_string());
        self
    }

    /// For providers that reject `role: system` outright (e.g. MiniMax).
    /// Collects all system messages and prepends their content to the first
    /// user message; also disables native tool calling because such providers
    /// generally reject OpenAI-style `tools` payloads as well.
    ///
    /// Prefer [`OpenAiCompatibleBuilder::merge_system_into_user_preserving_native`]
    /// when you want the merge behaviour but still want native tool calling
    /// (e.g. Bedrock).
    pub fn merge_system_into_user(mut self) -> Self {
        self.merge_system_into_user = true;
        self
    }

    /// Merge all system messages into the first user message before sending,
    /// preserving native tool calling. Use when the upstream rejects
    /// `role: system` but still accepts OpenAI-style `tools` payloads (e.g.
    /// Bedrock's Anthropic pass-through).
    pub fn merge_system_into_user_preserving_native(mut self) -> Self {
        self.merge_system_into_user_preserve_native = true;
        self
    }

    /// Disable native tool calling, forcing prompt-guided tool use instead.
    pub fn without_native_tools(mut self) -> Self {
        self.native_tool_calling_override = Some(false);
        self
    }

    /// Override the HTTP request timeout for LLM API calls. Values of 0
    /// are ignored (the default 120 s is kept) so a stray `Some(0)` from
    /// config cannot silently disable the safety timeout.
    pub fn timeout_secs(mut self, timeout_secs: u64) -> Self {
        if timeout_secs > 0 {
            self.timeout_secs = Some(timeout_secs);
        }
        self
    }

    /// Set extra HTTP headers to include in all API requests.
    pub fn extra_headers(mut self, headers: std::collections::HashMap<String, String>) -> Self {
        self.extra_headers = headers;
        self
    }

    /// Set reasoning effort for GPT-5/Codex-compatible chat-completions APIs.
    pub fn reasoning_effort(mut self, reasoning_effort: Option<String>) -> Self {
        self.reasoning_effort = reasoning_effort;
        self
    }

    /// Disable replay of stored assistant reasoning on outbound assistant
    /// history messages.
    pub fn without_assistant_reasoning_replay(mut self) -> Self {
        self.replay_assistant_reasoning_override = Some(false);
        self
    }

    /// Opt this provider into Anthropic prompt-cache passthrough: request
    /// bodies gain `cache_control` breakpoints (system prompt; rolling last
    /// message once the conversation has more than one non-system message),
    /// mirroring the native Anthropic provider's placement strategy, and
    /// gateway-reported cache usage is captured. The flag also reports
    /// `prompt_caching` in the provider capabilities.
    pub fn with_cache_passthrough(mut self) -> Self {
        self.cache_passthrough = true;
        self
    }

    /// Request the given cache entry lifetime on every breakpoint injected
    /// behind `cache_passthrough`. Effective only together with
    /// [`Self::with_cache_passthrough`]: without passthrough no breakpoints
    /// are placed, so the setting is inert (no parse-time warning — an
    /// operator may stage the value before switching passthrough on).
    /// Defaults to the 5-minute API default when unset.
    pub fn with_cache_ttl(mut self, cache_ttl: CacheTtl) -> Self {
        self.cache_ttl = Some(cache_ttl);
        self
    }

    /// Set a custom API path suffix for this model_provider.
    pub fn api_path(mut self, api_path: Option<String>) -> Self {
        self.api_path = api_path;
        self
    }

    /// Set the maximum output tokens for API requests.
    pub fn max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Set the models.dev catalog key for this model_provider.
    pub fn models_dev_key(mut self, key: &str) -> Self {
        self.models_dev_key = Some(key.to_string());
        self
    }

    /// Set the OpenRouter vendor prefix for this model_provider.
    pub fn openrouter_vendor_prefix(mut self, prefix: &str) -> Self {
        self.openrouter_vendor_prefix = Some(prefix.to_string());
        self
    }

    /// Opt into per-model conservative tool-schema sanitization.
    pub fn local_model_tool_sanitize(mut self) -> Self {
        self.local_model_tool_sanitize = true;
        self
    }

    /// Treat the `/models` endpoint as publicly accessible.
    pub fn public_model_listing(mut self) -> Self {
        self.public_model_listing = true;
        self
    }

    /// Path to a PEM-encoded custom CA certificate for TLS connections.
    /// The file is read once at [`Self::build`] time; failures are logged
    /// at WARN and TLS falls back to the system trust store.
    pub fn tls_ca_cert_path(mut self, path: &str) -> Self {
        self.tls_ca_cert_path = Some(path.to_string());
        self
    }

    /// Inject extra JSON fields into every API request body.
    pub fn extra_body(mut self, extra: serde_json::Value) -> Self {
        self.extra_body = Some(extra);
        self
    }

    /// Use a stored auth profile as a bearer credential when no explicit
    /// `api_key` was configured on this provider entry.
    pub fn auth_profile(
        mut self,
        model_provider: &str,
        auth_service: AuthService,
        profile_override: Option<String>,
    ) -> Self {
        self.auth_model_provider = Some(model_provider.to_string());
        self.auth_service = Some(auth_service);
        self.auth_profile_override = profile_override;
        self
    }

    /// Finalize the builder into a ready provider. Every optional construction
    /// value must be set on this builder; the returned provider has no
    /// post-construction mutators.
    ///
    /// # Panics
    /// Panics if [`Self::display_name`], [`Self::base_url`], or
    /// [`Self::auth_style`] was not called — those three fields carry no
    /// sensible default and every real call site sets them.
    pub fn build(self) -> OpenAiCompatibleModelProvider {
        let name = self
            .name
            .expect("OpenAiCompatibleBuilder: display_name() is required");
        let base_url = self
            .base_url
            .expect("OpenAiCompatibleBuilder: base_url() is required");
        let auth_style = self
            .auth_style
            .expect("OpenAiCompatibleBuilder: auth_style() is required");
        // Either merge preset can enable the shared merge behavior.
        let merge_system_into_user =
            self.merge_system_into_user || self.merge_system_into_user_preserve_native;
        // Default `native_tool_calling` is `!merge_system_into_user_disable_native`,
        // i.e. only the "combined preset" builder setter disables it. The
        // explicit `without_native_tools()` override wins if present.
        let native_tool_calling = self
            .native_tool_calling_override
            .unwrap_or(!self.merge_system_into_user);
        // Read the PEM bytes now so later HTTP clients incur no per-request I/O.
        // A read error is logged at WARN and TLS falls back to system roots —
        // preserving the established warning-and-fallback semantics.
        let tls_ca_cert_pem =
            self.tls_ca_cert_path
                .as_deref()
                .and_then(|path| match std::fs::read(path) {
                    Ok(bytes) => Some(bytes),
                    Err(e) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(
                                ::serde_json::json!({"path": path, "error": format!("{}", e)})
                            ),
                            "Failed to read CA certificate file — TLS will use system roots"
                        );
                        None
                    }
                });
        OpenAiCompatibleModelProvider {
            alias: self.alias,
            name,
            base_url,
            canonical_base_url: self.canonical_base_url,
            credential: self.credential,
            auth_service: self.auth_service,
            auth_model_provider: self.auth_model_provider,
            auth_profile_override: self.auth_profile_override,
            auth_header: auth_style,
            supports_vision: self.supports_vision,
            tool_result_image_policy: self.tool_result_image_policy,
            multimodal: self.multimodal,
            user_agent: self.user_agent,
            native_tool_calling,
            merge_system_into_user,
            timeout_secs: self.timeout_secs.unwrap_or(120),
            extra_headers: self.extra_headers,
            reasoning_effort: self.reasoning_effort,
            replay_assistant_reasoning: self.replay_assistant_reasoning_override.unwrap_or(true),
            cache_passthrough: self.cache_passthrough,
            cache_ttl: self.cache_ttl,
            api_path: self.api_path,
            max_tokens: self.max_tokens,
            models_dev_key: self.models_dev_key,
            openrouter_vendor_prefix: self.openrouter_vendor_prefix,
            local_model_tool_sanitize: self.local_model_tool_sanitize,
            public_model_listing: self.public_model_listing,
            tls_ca_cert_pem,
            extra_body: self.extra_body,
            schema_cache: std::sync::Arc::new(clawcrew_api::schema::SchemaCleanCache::new()),
        }
    }
}

impl OpenAiCompatibleModelProvider {
    /// Entry point for constructing an OpenAI-compatible provider.
    ///
    /// Only `alias` is taken as a positional argument; every other field
    /// is set via labelled chain methods on the returned
    /// [`OpenAiCompatibleBuilder`] so call sites remain readable. See
    /// [`OpenAiCompatibleBuilder::build`] for the fields that must be
    /// set before calling `build()`.
    pub fn builder(alias: &str) -> OpenAiCompatibleBuilder {
        OpenAiCompatibleBuilder {
            alias: alias.to_string(),
            name: None,
            base_url: None,
            canonical_base_url: None,
            credential: None,
            auth_style: None,
            supports_vision: false,
            tool_result_image_policy: ToolResultImagePolicy::default(),
            multimodal: clawcrew_config::schema::MultimodalConfig::default(),
            user_agent: None,
            merge_system_into_user: false,
            merge_system_into_user_preserve_native: false,
            native_tool_calling_override: None,
            timeout_secs: None,
            extra_headers: std::collections::HashMap::new(),
            reasoning_effort: None,
            replay_assistant_reasoning_override: None,
            cache_passthrough: false,
            cache_ttl: None,
            api_path: None,
            max_tokens: None,
            models_dev_key: None,
            openrouter_vendor_prefix: None,
            local_model_tool_sanitize: false,
            public_model_listing: false,
            tls_ca_cert_path: None,
            extra_body: None,
            auth_model_provider: None,
            auth_service: None,
            auth_profile_override: None,
        }
    }
    /// Add the configured custom CA certificate to a reqwest builder.
    /// The PEM bytes were loaded at construction, so this performs no disk I/O.
    fn add_tls_cert_to_builder(&self, builder: ClientBuilder) -> ClientBuilder {
        if let Some(ref pem) = self.tls_ca_cert_pem {
            match reqwest::Certificate::from_pem(pem) {
                Ok(cert) => return builder.add_root_certificate(cert),
                Err(e) => ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                    "Failed to parse CA certificate — TLS will use system roots"
                ),
            }
        }
        builder
    }

    /// Collect all `system` role messages and keep them in a provider-safe
    /// shape. Strict OpenAI-compatible endpoints accept a leading system
    /// message but reject system messages later in the history.
    /// Flatten system messages per `merge`. Returns the flattened list plus
    /// whether the system content was merged INTO a user message (or a
    /// synthetic user inserted for it): that first user message is the
    /// system-equivalent carrier and the only place a system-role
    /// breakpoint can ride when merging is on. `false` means a system-role
    /// message still exists on the wire (or there never was one).
    fn flatten_system_messages(messages: &[ChatMessage], merge: bool) -> (Vec<ChatMessage>, bool) {
        let mut saw_system = false;
        let mut system_content = String::new();
        let mut result: Vec<ChatMessage> = Vec::with_capacity(messages.len());

        for message in messages {
            if message.role == "system" {
                saw_system = true;
                if !message.content.is_empty() {
                    if !system_content.is_empty() {
                        system_content.push_str("\n\n");
                    }
                    system_content.push_str(&message.content);
                }
            } else {
                result.push(message.clone());
            }
        }

        if !saw_system {
            return (messages.to_vec(), false);
        }

        if system_content.is_empty() {
            return (result, false);
        }

        if !merge {
            result.insert(0, ChatMessage::system(system_content));
            return (result, false);
        }

        if let Some(first_user) = result.iter_mut().find(|m| m.role == "user") {
            first_user.content = format!("{system_content}\n\n{}", first_user.content);
        } else {
            // No user message found: insert a synthetic user message with system content
            result.insert(0, ChatMessage::user(&system_content));
        }

        (result, true)
    }

    fn http_client(&self) -> Client {
        let timeout = self.timeout_secs;
        let has_user_agent = self.user_agent.is_some();
        let has_extra_headers = !self.extra_headers.is_empty();
        let has_tls_cert = self.tls_ca_cert_pem.is_some();
        // An OpenCode client needs its own redirect policy, which the shared
        // cached client below does not carry.
        let endpoint = self.chat_completions_url();
        let targets_opencode = crate::opencode_session::is_opencode_target(&endpoint);

        if has_user_agent || has_extra_headers || has_tls_cert || targets_opencode {
            let mut headers = HeaderMap::new();
            if let Some(ua) = self.user_agent.as_deref()
                && let Ok(value) = HeaderValue::from_str(ua)
            {
                headers.insert(USER_AGENT, value);
            }
            for (key, value) in &self.extra_headers {
                match (
                    reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    (Ok(name), Ok(val)) => {
                        headers.insert(name, val);
                    }
                    _ => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"header": key})),
                            "Skipping invalid extra header name or value"
                        );
                    }
                }
            }

            let builder = Client::builder()
                .timeout(std::time::Duration::from_secs(timeout))
                .connect_timeout(std::time::Duration::from_secs(10))
                .default_headers(headers);
            let builder = crate::opencode_session::restrict_redirects(builder, &endpoint);
            let builder = self.add_tls_cert_to_builder(builder);
            let builder = clawcrew_config::schema::apply_runtime_proxy_to_builder(
                builder,
                "model_provider.compatible",
            );

            return builder.build().unwrap_or_else(|error| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"error": super::format_error_chain(&error)})
                        ),
                    "Failed to build proxied timeout client with custom headers or TLS certificate: "
                );
                Client::new()
            });
        }

        clawcrew_config::schema::build_runtime_proxy_client_with_timeouts(
            "model_provider.compatible",
            timeout,
            10,
        )
    }

    /// HTTP client for streaming SSE connections — no overall timeout (reqwest's
    /// total timeout kills long-running streams mid-response), but a `read_timeout`
    /// idle bound so a silent connection fails fast instead of hanging forever.
    /// The bound is derived as `max(STREAM_IDLE_TIMEOUT, timeout_secs)`: the 300 s
    /// floor applies when `timeout_secs` is unset or lower, and a higher
    /// `timeout_secs` raises the bound to match. Streaming paths must use this
    /// client instead of http_client().
    fn streaming_http_client(&self) -> Client {
        let endpoint = self.chat_completions_url();
        let has_user_agent = self.user_agent.is_some();
        let has_extra_headers = !self.extra_headers.is_empty();
        let has_tls_cert = self.tls_ca_cert_pem.is_some();

        if has_user_agent || has_extra_headers || has_tls_cert {
            let mut headers = HeaderMap::new();
            if let Some(ua) = self.user_agent.as_deref()
                && let Ok(value) = HeaderValue::from_str(ua)
            {
                headers.insert(USER_AGENT, value);
            }
            for (key, value) in &self.extra_headers {
                match (
                    reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    (Ok(name), Ok(val)) => {
                        headers.insert(name, val);
                    }
                    _ => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"header": key})),
                            "Skipping invalid extra header name or value"
                        );
                    }
                }
            }

            let builder = Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .read_timeout(super::stream_idle_timeout(self.timeout_secs).duration())
                .default_headers(headers);
            let builder = crate::opencode_session::restrict_redirects(builder, &endpoint);
            let builder = self.add_tls_cert_to_builder(builder);
            let builder = clawcrew_config::schema::apply_runtime_proxy_to_builder(
                builder,
                "provider.compatible",
            );
            return builder.build().unwrap_or_else(|error| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(
                            ::serde_json::json!({"error": super::format_error_chain(&error)})
                        ),
                    "Failed to build proxied streaming client with custom headers or TLS certificate: "
                );
                Client::new()
            });
        }

        let builder = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(super::stream_idle_timeout(self.timeout_secs).duration());
        let builder = crate::opencode_session::restrict_redirects(builder, &endpoint);
        let builder =
            clawcrew_config::schema::apply_runtime_proxy_to_builder(builder, "provider.compatible");
        builder.build().unwrap_or_else(|error| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": super::format_error_chain(&error)})),
                "Failed to build proxied streaming client: "
            );
            Client::new()
        })
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.base_url)
    }

    /// Inject Anthropic prompt-cache breakpoints behind `cache_passthrough`.
    ///
    /// The native Anthropic provider is the reference for the breakpoint
    /// gate: `AnthropicModelProvider::should_cache_conversation` and
    /// `apply_cache_to_last_message` in `anthropic.rs`. (a) The system prompt
    /// always carries a breakpoint when one exists on the wire. With
    /// `merge_system_into_user`, the system role disappears from the wire,
    /// so the carrier index of the merged system content (the first user
    /// message, or the synthetic user carrying it) takes over that role and
    /// is marked unconditionally. (b) Once the conversation has more than
    /// one non-system message, a rolling breakpoint lands on the last
    /// non-system message with a non-empty text part: the last message when
    /// it carries text, otherwise the nearest earlier non-system message
    /// that does, so an image-only turn rolls the breakpoint back instead
    /// of silently dropping it. System messages are never marked twice, and
    /// when nothing qualifies there is no rolling breakpoint. At most two
    /// breakpoints per request; only breakpoint-carrying messages convert
    /// from string content to block form, every other message serializes
    /// exactly as before.
    fn apply_cache_breakpoints<T: CacheBreakpointMessage>(
        &self,
        messages: &mut [T],
        merged_system_carrier: Option<usize>,
    ) {
        if !self.cache_passthrough {
            return;
        }
        // One TTL per request: every breakpoint this pass places carries
        // the same configured lifetime. `None` resolves to the 5-minute
        // API default, whose markers serialize without a `ttl` field.
        let cache_ttl = self.cache_ttl.unwrap_or_default();
        let carrier = merged_system_carrier.and_then(|idx| {
            (idx < messages.len() && messages[idx].cache_role() != "system").then_some(idx)
        });
        match carrier {
            Some(idx) => {
                if let Some(content) = messages[idx].cache_content() {
                    content.apply_cache_control(cache_ttl);
                }
            }
            None => {
                if let Some(system) = messages.iter_mut().find(|m| m.cache_role() == "system")
                    && let Some(content) = system.cache_content()
                {
                    content.apply_cache_control(cache_ttl);
                }
            }
        }
        let non_system_count = messages
            .iter()
            .filter(|m| m.cache_role() != "system")
            .count();
        if non_system_count > 1 {
            for message in messages.iter_mut().rev() {
                if message.cache_role() == "system" {
                    continue;
                }
                if let Some(content) = message.cache_content()
                    && content.apply_cache_control(cache_ttl)
                {
                    break;
                }
            }
        }
    }

    /// Index of the merged system-content carrier (the first user message)
    /// when `flatten_system_messages` reported a merge, else `None`.
    fn merged_system_carrier_index<T: CacheBreakpointMessage>(
        messages: &[T],
        system_merged: bool,
    ) -> Option<usize> {
        if system_merged {
            messages.iter().position(|m| m.cache_role() == "user")
        } else {
            None
        }
    }

    /// Build the full URL for chat completions, detecting if base_url already includes the path.
    /// This allows custom model_providers with non-standard endpoints (e.g., VolcEngine ARK uses
    /// `/api/coding/v3/chat/completions` instead of `/v1/chat/completions`).
    fn chat_completions_url(&self) -> String {
        // If a custom api_path is configured, use it directly.
        if let Some(ref api_path) = self.api_path {
            let separator = if api_path.starts_with('/') { "" } else { "/" };
            return format!("{}{separator}{api_path}", self.base_url);
        }

        let has_full_endpoint = reqwest::Url::parse(&self.base_url)
            .map(|url| {
                url.path()
                    .trim_end_matches('/')
                    .ends_with("/chat/completions")
            })
            .unwrap_or_else(|_| {
                self.base_url
                    .trim_end_matches('/')
                    .ends_with("/chat/completions")
            });

        if has_full_endpoint {
            self.base_url.clone()
        } else {
            format!("{}/chat/completions", self.base_url)
        }
    }

    fn requires_tool_stream(&self) -> bool {
        let host_requires_tool_stream = reqwest::Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| host == "api.z.ai" || host.ends_with(".z.ai"));

        host_requires_tool_stream || matches!(self.name.as_str(), "zai" | "z.ai")
    }

    fn tool_stream_for_tools(&self, has_tools: bool) -> Option<bool> {
        if has_tools && self.requires_tool_stream() {
            Some(true)
        } else {
            None
        }
    }

    /// Returns true if the given model requires system messages to be merged
    /// into the first user message because its prompt template cannot handle
    /// the `system` role reliably (e.g. DeepSeek V3.2 Jinja rendering errors).
    fn model_requires_system_merge(model: &str) -> bool {
        let id = model
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase();
        id.contains("deepseek-v3") || id.contains("deepseek_v3")
    }

    /// Whether system messages should be flattened into the first user message,
    /// either because the model_provider was configured that way or the model requires it.
    fn effective_merge_system(&self, model: &str) -> bool {
        self.merge_system_into_user || Self::model_requires_system_merge(model)
    }

    fn reasoning_effort_for_model(&self, model: &str) -> Option<String> {
        let effort = self.reasoning_effort.as_ref()?;
        let id = model
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase();
        // gpt-5*-chat-latest (gpt-5-chat-latest, gpt-5.1-chat-latest, ...) are
        // OpenAI's non-reasoning chat-router models; the Chat Completions API
        // rejects reasoning_effort for them. Treat them as a distinct family, the
        // same way the native openai.rs provider already special-cases them.
        let is_gpt5_chat_latest = id.starts_with("gpt-5") && id.ends_with("-chat-latest");
        let is_openai_reasoning_model = id == "o1"
            || id.starts_with("o1-")
            || id == "o3"
            || id.starts_with("o3-")
            || id == "o4"
            || id.starts_with("o4-")
            || (id.starts_with("gpt-5") && !is_gpt5_chat_latest);
        let is_likely_codex_supported = id.contains("codex") && id.starts_with("gpt-");

        (is_openai_reasoning_model || is_likely_codex_supported).then(|| effort.clone())
    }

    async fn resolve_credential(&self) -> anyhow::Result<Option<String>> {
        if self
            .credential
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
        {
            return Ok(self.credential.clone());
        }
        let (Some(auth), Some(model_provider)) = (&self.auth_service, &self.auth_model_provider)
        else {
            return Ok(None);
        };
        if model_provider == "xai" {
            return auth
                .get_valid_xai_access_token(self.auth_profile_override.as_deref())
                .await;
        }
        auth.get_provider_bearer_token(model_provider, self.auth_profile_override.as_deref())
            .await
    }

    fn assistant_reasoning_value(value: &serde_json::Value) -> Option<&str> {
        value
            .get("reasoning_content")
            .and_then(serde_json::Value::as_str)
            .or_else(|| value.get("reasoning").and_then(serde_json::Value::as_str))
    }

    fn assistant_reasoning_pair_for_replay(
        &self,
        value: &serde_json::Value,
    ) -> (Option<String>, Option<String>) {
        if !self.replay_assistant_reasoning {
            return (None, None);
        }
        let reasoning_content = value
            .get("reasoning_content")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        let reasoning = value
            .get("reasoning")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        (reasoning_content, reasoning)
    }
}

#[derive(Debug, Serialize)]
struct ApiChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptionsBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// Extra fields merged at the top level of the serialized JSON body.
    /// Mirrors `NativeChatRequest::extra_body` so config-driven extras
    /// (`provider_extra`, `chat_template_kwargs`) reach the no-tools request
    /// paths too, not just the native-tools path.
    #[serde(flatten)]
    extra_body: Option<serde_json::Value>,
}

/// OpenAI-compatible `stream_options.include_usage` toggle.
/// When set with streaming, providers emit a final SSE chunk carrying usage
/// counts (prompt_tokens / completion_tokens) so the agent can populate cost
/// records and the WebSocket done frame for streaming responses.
#[derive(Debug, Serialize, Clone, Copy)]
struct StreamOptionsBody {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: MessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Parts(Vec<MessagePart>),
}

impl MessageContent {
    /// Mark this content as an Anthropic prompt-cache breakpoint and return
    /// whether a markable text part was found: plain string content
    /// converts to the single-text-block wire form (block conversion
    /// touches only breakpoint-carrying messages); block-form content gains
    /// the marker on its last non-empty text part. A message ending in an
    /// image part therefore carries the breakpoint on its text, and the
    /// image part stays unmarked: the image is covered by the following
    /// turn's rolling breakpoint. Empty text is never marked, because the
    /// wire format rejects empty text blocks that carry `cache_control`.
    fn apply_cache_control(&mut self, cache_ttl: CacheTtl) -> bool {
        match self {
            MessageContent::Text(text) => {
                if text.is_empty() {
                    return false;
                }
                *self = MessageContent::Parts(vec![MessagePart::Text {
                    text: std::mem::take(text),
                    cache_control: Some(crate::anthropic::CacheControl::ephemeral_with_ttl(
                        cache_ttl,
                    )),
                }]);
                true
            }
            MessageContent::Parts(parts) => {
                for part in parts.iter_mut().rev() {
                    if let MessagePart::Text {
                        text,
                        cache_control,
                    } = part
                        && !text.is_empty()
                    {
                        *cache_control = Some(crate::anthropic::CacheControl::ephemeral_with_ttl(
                            cache_ttl,
                        ));
                        return true;
                    }
                }
                false
            }
        }
    }
}

/// Uniform (role, content) access over the two wire message shapes so one
/// breakpoint-injection routine serves both the no-tools and the native-tools
/// request types.
trait CacheBreakpointMessage {
    fn cache_role(&self) -> &str;
    fn cache_content(&mut self) -> Option<&mut MessageContent>;
}

impl CacheBreakpointMessage for Message {
    fn cache_role(&self) -> &str {
        &self.role
    }

    fn cache_content(&mut self) -> Option<&mut MessageContent> {
        Some(&mut self.content)
    }
}

impl CacheBreakpointMessage for NativeMessage {
    fn cache_role(&self) -> &str {
        &self.role
    }

    fn cache_content(&mut self) -> Option<&mut MessageContent> {
        self.content.as_mut()
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MessagePart {
    Text {
        text: String,
        /// Anthropic prompt-cache breakpoint, injected only behind
        /// `cache_passthrough` and only on breakpoint-carrying messages.
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<crate::anthropic::CacheControl>,
    },
    ImageUrl {
        image_url: ImageUrlPart,
    },
}

#[derive(Debug, Serialize)]
struct ImageUrlPart {
    url: String,
}

#[derive(Debug, Deserialize)]
struct ApiChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<UsageInfo>,
}

/// OpenAI-compatible chat response, either in the standard top-level shape or
/// wrapped by a gateway in a top-level `data` object.
///
/// Keep the direct variant first: a valid top-level response remains
/// authoritative when a provider also includes a `data` metadata field.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ApiChatResponseEnvelope {
    Direct(ApiChatResponse),
    Wrapped { data: ApiChatResponse },
}

impl ApiChatResponseEnvelope {
    fn into_response(self) -> ApiChatResponse {
        match self {
            Self::Direct(response) => response,
            Self::Wrapped { data } => data,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
struct UsageInfo {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default, deserialize_with = "deserialize_optional_token_count")]
    prompt_cache_hit_tokens: Option<u64>,
    /// Anthropic-shaped cache counters, forwarded by translating gateways
    /// on Anthropic-backed routes. `cache_read_input_tokens` is the
    /// authoritative cached-read count (it comes from the upstream API
    /// itself), so it takes precedence over the gateway-accounted OpenAI
    /// and DeepSeek shapes.
    #[serde(default, deserialize_with = "deserialize_optional_token_count")]
    cache_read_input_tokens: Option<u64>,
    /// Anthropic-shaped cache-write counter. Not part of `TokenUsage`
    /// (cached reads are the billing-relevant figure there); logged so
    /// write-premium spend is visible in logs.
    #[serde(default, deserialize_with = "deserialize_optional_token_count")]
    cache_creation_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct PromptTokensDetails {
    #[serde(default, deserialize_with = "deserialize_optional_token_count")]
    cached_tokens: Option<u64>,
    /// Cache-write tokens reported by translating gateways that forward
    /// Anthropic usage (`prompt_tokens_details.cache_creation_input_tokens`).
    #[serde(default, deserialize_with = "deserialize_optional_token_count")]
    cache_creation_input_tokens: Option<u64>,
}

impl UsageInfo {
    /// Cached-read tokens across the three coexisting provider shapes.
    /// Precedence: Anthropic `cache_read_input_tokens` (upstream-reported
    /// on translated routes) over DeepSeek `prompt_cache_hit_tokens` over
    /// OpenAI `prompt_tokens_details.cached_tokens` (gateway-accounted).
    fn cached_input_tokens(&self) -> Option<u64> {
        self.cache_read_input_tokens
            .or(self.prompt_cache_hit_tokens)
            .or_else(|| {
                self.prompt_tokens_details
                    .as_ref()
                    .and_then(|details| details.cached_tokens)
            })
    }

    fn cache_creation_input_tokens(&self) -> Option<u64> {
        self.prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cache_creation_input_tokens)
    }

    fn into_provider_usage(self) -> clawcrew_api::model_provider::TokenUsage {
        if let Some(creation) = self.cache_creation_input_tokens.filter(|count| *count > 0) {
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Success)
                    .with_attrs(::serde_json::json!({
                        "cache_creation_input_tokens": creation,
                        "cache_read_input_tokens": self.cache_read_input_tokens,
                    })),
                "gateway-reported Anthropic cache write (billed at the write premium)"
            );
        }
        let cached_input_tokens = self.cached_input_tokens();
        let cache_creation_input_tokens = self.cache_creation_input_tokens();
        clawcrew_api::model_provider::TokenUsage {
            input_tokens: self.prompt_tokens,
            output_tokens: self.completion_tokens,
            cached_input_tokens,
            cache_creation_input_tokens,
        }
    }
}

fn deserialize_optional_token_count<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(normalize_token_count_value))
}

fn normalize_token_count_value(value: serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                Some(value)
            } else if let Some(value) = number.as_i64() {
                if value < 0 {
                    None
                } else {
                    u64::try_from(value).ok()
                }
            } else {
                number.as_f64().and_then(normalize_token_count_float)
            }
        }
        serde_json::Value::String(value) => value
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(normalize_token_count_float),
        _ => None,
    }
}

fn normalize_token_count_float(value: f64) -> Option<u64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    if value < 1.0 {
        return Some(0);
    }
    if value > u64::MAX as f64 {
        return None;
    }
    Some(value.floor() as u64)
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

/// OpenAI Chat Completions may return assistant `message.content` as a string,
/// null, or an array of typed parts. Normalize it before storing the internal
/// response shape so compatible gateways that preserve typed parts still work,
/// while unsupported top-level content shapes still fail deserialization.
fn openai_assistant_content_plaintext(content: Option<OpenAiAssistantContent>) -> Option<String> {
    match content? {
        OpenAiAssistantContent::Text(s) => {
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        OpenAiAssistantContent::Parts(parts) => {
            let mut text = String::new();
            for part in parts {
                if part.kind.as_deref() != Some("text") {
                    continue;
                }
                let Some(part_text) = part.text.filter(|text| !text.is_empty()) else {
                    continue;
                };
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&part_text);
            }

            if text.is_empty() { None } else { Some(text) }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpenAiAssistantContent {
    Text(String),
    Parts(Vec<OpenAiAssistantContentPart>),
}

#[derive(Debug, Deserialize)]
struct OpenAiAssistantContentPart {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(from = "RawResponseMessage")]
struct ResponseMessage {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Deserialize)]
struct RawResponseMessage {
    #[serde(default)]
    content: Option<OpenAiAssistantContent>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCall>>,
}

impl From<RawResponseMessage> for ResponseMessage {
    fn from(raw: RawResponseMessage) -> Self {
        // Canonical field wins when both are present; the alias fills in only
        // when the canonical name is absent or null.
        let reasoning_content = raw.reasoning_content.or(raw.reasoning);
        ResponseMessage {
            content: openai_assistant_content_plaintext(raw.content),
            reasoning_content,
            tool_calls: raw.tool_calls,
        }
    }
}

impl ResponseMessage {
    /// Extract text content from the `content` field only. Does NOT fall
    /// back to `reasoning_content` — thinking/reasoning models (GLM-5.1,
    /// DeepSeek, Qwen) return their thinking in `reasoning_content` which
    /// must not leak into the user-visible response text. The
    /// `reasoning_content` is preserved separately in
    /// `ChatResponse.reasoning_content` for history round-tripping.
    ///
    /// Returns the `content` field as-is. Previously this stripped
    /// `<think>...</think>` blocks that some reasoning models (e.g. MiniMax)
    /// embedded inline in `content` instead of using a separate field, but
    /// that unconditional rewrite silently mangled responses whose `content`
    /// legitimately contained literal `<think>...</think>` markup (HTML, code
    /// samples, quoted discussion of the tag itself, and unclosed tails).
    /// Model providers that need inline think-block filtering should do it
    /// downstream of this response shape, with full visibility into the
    /// model's actual output.
    fn effective_content(&self) -> String {
        self.content
            .as_ref()
            .cloned()
            .filter(|c| !c.is_empty())
            .unwrap_or_default()
    }

    fn effective_content_optional(&self) -> Option<String> {
        self.content.as_ref().cloned().filter(|c| !c.is_empty())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    function: Option<Function>,

    // Compatibility: Some model_providers (e.g., older GLM) may use 'name' directly
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    arguments: Option<String>,

    // Compatibility: DeepSeek sometimes wraps arguments differently
    #[serde(
        rename = "parameters",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    parameters: Option<serde_json::Value>,

    /// See [`clawcrew_api::ToolCall::extra_content`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra_content: Option<serde_json::Value>,
}

impl ToolCall {
    /// Extract function name with fallback logic for various model_provider formats
    fn function_name(&self) -> Option<String> {
        // Standard OpenAI format: tool_calls[].function.name
        if let Some(ref func) = self.function
            && let Some(ref name) = func.name
        {
            return Some(name.clone());
        }
        // Fallback: direct name field
        self.name.clone()
    }

    /// Extract arguments with fallback logic and type conversion
    fn function_arguments(&self) -> Option<String> {
        // Standard OpenAI format: tool_calls[].function.arguments (string)
        if let Some(ref func) = self.function
            && let Some(ref args) = func.arguments
        {
            return Some(args.clone());
        }
        // Fallback: direct arguments field
        if let Some(ref args) = self.arguments {
            return Some(args.clone());
        }
        // Compatibility: Some model_providers return parameters as object instead of string
        if let Some(ref params) = self.parameters {
            return serde_json::to_string(params).ok();
        }
        None
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Function {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Serialize)]
struct NativeChatRequest<T = Vec<NativeToolSpec>> {
    model: String,
    messages: Vec<NativeMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptionsBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// Extra fields merged at the top level of the serialized JSON body.
    #[serde(flatten)]
    extra_body: Option<serde_json::Value>,
}

/// Ensure the serialized request body carries an explicit
/// `reasoning_effort: "none"`, reporting whether that changed the payload.
///
/// `extra_body` is flattened into [`NativeChatRequest`] and can supply the
/// canonical top-level value seen by the endpoint, so fallback decisions must
/// inspect and mutate the wire payload rather than only the typed field.
///
/// An *absent* `reasoning_effort` is not equivalent to `"none"`: endpoints
/// default an omitted effort to a non-`none` value, so a payload without the
/// field is rejected exactly like one carrying `"high"`. The key is therefore
/// inserted when missing.
///
/// Returns `false` once the payload already states exactly `"none"`, which is
/// the fixed point that bounds the retry to a single additional request. The
/// comparison is case-sensitive on purpose: a differently-spelled `"NONE"`
/// supplied through `provider_extra` can be rejected by a case-sensitive
/// endpoint, so it is normalized to the canonical lowercase value rather than
/// treated as already repaired. Rewriting a non-canonical spelling still
/// converges after one retry because the rewritten payload is the fixed point.
fn ensure_reasoning_effort_none(payload: &mut serde_json::Value) -> bool {
    let Some(object) = payload.as_object_mut() else {
        return false;
    };
    if matches!(
        object.get("reasoning_effort"),
        Some(serde_json::Value::String(effort)) if effort == "none"
    ) {
        return false;
    }
    object.insert(
        "reasoning_effort".to_string(),
        serde_json::Value::String("none".to_string()),
    );
    true
}

#[derive(Debug, Serialize)]
struct NativeMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    /// Raw reasoning content from thinking models; pass-through for model_providers
    /// that require it in assistant tool-call history messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "reasoning")]
    reasoning: Option<String>,
    /// Tool name for `role: "tool"` messages. Groq native tool calling
    /// requires this field on every tool-result message; omitting it causes
    /// HTTP 400 "Tools should have a name!"./
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

// ---------------------------------------------------------------
// Streaming support (SSE parser)
// ---------------------------------------------------------------

/// Server-Sent Event stream chunk for OpenAI-compatible streaming.
#[derive(Debug, Deserialize)]
struct StreamChunkResponse {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    /// Final-chunk usage counts. Populated only when the request includes
    /// `stream_options.include_usage: true` and the provider supports it.
    #[serde(default)]
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default)]
struct StreamDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    /// Native tool-calling deltas in OpenAI chat-completions streaming format.
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Debug, Deserialize, Default)]
struct RawStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}

impl<'de> Deserialize<'de> for StreamDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawStreamDelta::deserialize(deserializer)?;
        Ok(StreamDelta {
            content: raw.content,
            reasoning_content: raw.reasoning_content.or(raw.reasoning),
            tool_calls: raw.tool_calls,
        })
    }
}

#[derive(Debug, Deserialize)]
struct StreamToolCallDelta {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFunctionDelta>,
    // Compatibility: some model_providers stream name/arguments at top-level.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
    #[serde(default)]
    extra_content: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct StreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Default)]
struct StreamToolCallAccumulator {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    extra_content: Option<serde_json::Value>,
}

impl StreamToolCallAccumulator {
    fn apply_delta(&mut self, delta: &StreamToolCallDelta) {
        if let Some(id) = delta.id.as_ref().filter(|value| !value.is_empty()) {
            self.id = Some(id.clone());
        }

        let delta_name = delta
            .function
            .as_ref()
            .and_then(|function| function.name.as_ref())
            .or(delta.name.as_ref())
            .filter(|value| !value.is_empty());
        if let Some(name) = delta_name {
            self.name = Some(name.clone());
        }

        if let Some(arguments_delta) = delta
            .function
            .as_ref()
            .and_then(|function| function.arguments.as_ref())
            .or(delta.arguments.as_ref())
            .filter(|value| !value.is_empty())
        {
            self.arguments.push_str(arguments_delta);
        }

        // Last-write-wins: signature is opaque and delivered once per call.
        if let Some(extra) = delta.extra_content.as_ref() {
            self.extra_content = Some(extra.clone());
        }
    }

    fn into_provider_tool_call(
        self,
        targets_mistral_tool_call_contract: bool,
        used_tool_call_ids: &mut std::collections::HashSet<String>,
    ) -> Option<ProviderToolCall> {
        let name = self.name?;
        // Route through the shared `sanitize_tool_arguments` helper so the
        // normalization contract (empty/whitespace → "{}", invalid JSON →
        // WARN + "{}", valid JSON → passthrough) has a single source of truth.
        let normalized_arguments = sanitize_tool_arguments(&name, &self.arguments);

        Some(ProviderToolCall {
            id: reserve_tool_call_id_for_contract(
                targets_mistral_tool_call_contract,
                self.id,
                used_tool_call_ids,
            ),
            name,
            arguments: normalized_arguments,
            extra_content: self.extra_content,
        })
    }
}

fn parse_sse_chunk(line: &str) -> StreamResult<Option<StreamChunkResponse>> {
    let line = line.trim();

    if line.is_empty() || line.starts_with(':') {
        return Ok(None);
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(None);
    };
    let data = data.trim();

    if data == "[DONE]" {
        return Ok(None);
    }

    serde_json::from_str(data)
        .map(Some)
        .map_err(StreamError::Json)
}

/// Parse custom proxy tool events from SSE lines.
/// These are emitted by proxies like claude-max-api-proxy that execute tools
/// internally and forward observability events via custom SSE fields.
fn parse_proxy_tool_event(line: &str) -> Option<StreamEvent> {
    let data = line.trim().strip_prefix("data:")?.trim();
    let obj: serde_json::Value = serde_json::from_str(data).ok()?;

    if let Some(ts) = obj.get("x_tool_start") {
        let Some(name) = ts.get("name").and_then(|v| v.as_str()) else {
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                "proxy x_tool_start event missing required 'name' field"
            );
            return None;
        };
        let name = name.to_string();
        let args = ts
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("{}")
            .to_string();
        return Some(StreamEvent::PreExecutedToolCall { name, args });
    }

    if let Some(tr) = obj.get("x_tool_result") {
        let name = tr
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let output = tr
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some(StreamEvent::PreExecutedToolResult { name, output });
    }

    None
}

fn extract_sse_text_delta(choice: &StreamChoice) -> Option<String> {
    if let Some(content) = &choice.delta.content
        && !content.is_empty()
    {
        return Some(content.clone());
    }

    None
}

fn extract_sse_reasoning_delta(choice: &StreamChoice) -> Option<String> {
    choice
        .delta
        .reasoning_content
        .as_ref()
        .filter(|value| !value.is_empty())
        .cloned()
}

fn is_valid_mistral_tool_call_id(id: &str) -> bool {
    id.len() == 9 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn reserve_tool_call_id_for_contract(
    targets_mistral_tool_call_contract: bool,
    raw_id: Option<String>,
    used_ids: &mut std::collections::HashSet<String>,
) -> String {
    if !targets_mistral_tool_call_contract {
        let id = raw_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if used_ids.insert(id.clone()) {
            return id;
        }

        loop {
            let candidate = uuid::Uuid::new_v4().to_string();
            if used_ids.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    if let Some(id) = raw_id.as_deref()
        && is_valid_mistral_tool_call_id(id)
        && used_ids.insert(id.to_string())
    {
        return id.to_string();
    }

    let mut candidate = raw_id
        .as_deref()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(9)
        .collect::<String>();

    if candidate.len() < 9 {
        candidate.extend(
            uuid::Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(9 - candidate.len()),
        );
    }

    if used_ids.insert(candidate.clone()) {
        return candidate;
    }

    loop {
        let generated = uuid::Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(9)
            .collect::<String>();
        if used_ids.insert(generated.clone()) {
            return generated;
        }
    }
}

fn parse_sse_line(line: &str) -> StreamResult<Option<StreamChunk>> {
    let chunk = match parse_sse_chunk(line)? {
        Some(c) => c,
        None => return Ok(None),
    };

    if let Some(choice) = chunk.choices.first() {
        if let Some(content) = &choice.delta.content
            && !content.is_empty()
        {
            return Ok(Some(StreamChunk::delta(content.clone())));
        }
        if let Some(reasoning) = &choice.delta.reasoning_content
            && !reasoning.is_empty()
        {
            return Ok(Some(StreamChunk::reasoning(reasoning.clone())));
        }
    }

    Ok(None)
}

/// Convert SSE byte stream to text chunks.
/// Convert an SSE byte stream into structured chunks. `idle_timeout` is the
/// streaming client's read-idle bound; it names the bound that fired in
/// body-read timeout errors, including whether `timeout_secs` can raise it.
fn sse_bytes_to_chunks(
    response: reqwest::Response,
    count_tokens: bool,
    idle_timeout: super::StreamIdleBound,
) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

    let handle = ::clawcrew_spawn::spawn!(async move {
        let mut buffer = String::new();

        match response.error_for_status_ref() {
            Ok(_) => {}
            Err(e) => {
                let _ = tx
                    .send(Err(StreamError::Http(super::format_error_chain(&e))))
                    .await;
                return;
            }
        }

        let mut bytes_stream = response.bytes_stream();
        // Accumulate partial UTF-8 sequences that may be split across
        // HTTP/1.1 chunked transfer boundaries (e.g. 3-byte CJK chars).
        let mut utf8_buf: Vec<u8> = Vec::new();

        'stream: while let Some(item) = bytes_stream.next().await {
            match item {
                Ok(bytes) => {
                    utf8_buf.extend_from_slice(&bytes);
                    let text = match std::str::from_utf8(&utf8_buf) {
                        Ok(s) => {
                            let owned = s.to_string();
                            utf8_buf.clear();
                            owned
                        }
                        Err(e) => {
                            let valid_up_to = e.valid_up_to();
                            if valid_up_to == 0 && utf8_buf.len() < 4 {
                                // Could still be an incomplete multi-byte char; wait for more data
                                continue;
                            }
                            let valid =
                                String::from_utf8_lossy(&utf8_buf[..valid_up_to]).into_owned();
                            utf8_buf.drain(..valid_up_to);
                            valid
                        }
                    };
                    if text.is_empty() {
                        continue;
                    }

                    buffer.push_str(&text);

                    while let Some(pos) = buffer.find('\n') {
                        let line = buffer[..pos].to_string();
                        buffer.drain(..=pos);

                        if line.trim().strip_prefix("data:").map(str::trim) == Some("[DONE]") {
                            break 'stream;
                        }

                        match parse_sse_line(&line) {
                            Ok(Some(chunk)) => {
                                let chunk = if count_tokens {
                                    chunk.with_token_estimate()
                                } else {
                                    chunk
                                };
                                if tx.send(Ok(chunk)).await.is_err() {
                                    return; // Receiver dropped
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                let _ = tx.send(Err(e)).await;
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Http(super::stream_idle_error_message(
                            &e,
                            idle_timeout,
                        ))))
                        .await;
                    return;
                }
            }
        }

        let _ = tx.send(Ok(StreamChunk::final_chunk())).await;
    });

    let guard = AbortOnDrop::new(handle.abort_handle());
    stream::unfold((rx, guard), |(mut rx, guard)| async {
        rx.recv().await.map(|chunk| (chunk, (rx, guard)))
    })
    .boxed()
}

/// Convert SSE byte stream to structured streaming events.
pub(crate) fn sse_bytes_to_events(
    response: reqwest::Response,
    count_tokens: bool,
) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
    sse_bytes_to_events_for_contract(
        response,
        count_tokens,
        false,
        super::StreamIdleBound::Fixed(super::STREAM_IDLE_TIMEOUT),
    )
}

fn sse_bytes_to_events_for_contract(
    response: reqwest::Response,
    count_tokens: bool,
    targets_mistral_tool_call_contract: bool,
    idle_timeout: super::StreamIdleBound,
) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

    let handle = ::clawcrew_spawn::spawn!(async move {
        let mut buffer = String::new();
        let mut tool_calls: Vec<StreamToolCallAccumulator> = Vec::new();
        let mut used_tool_call_ids = std::collections::HashSet::new();
        let mut emitted_tool_calls = false;
        let mut saw_completion = false;

        match response.error_for_status_ref() {
            Ok(_) => {}
            Err(e) => {
                let _ = tx
                    .send(Err(StreamError::Http(super::format_error_chain(&e))))
                    .await;
                return;
            }
        }

        let mut bytes_stream = response.bytes_stream();
        // Accumulate partial UTF-8 sequences split across chunk boundaries.
        let mut utf8_buf: Vec<u8> = Vec::new();
        'stream: while let Some(item) = bytes_stream.next().await {
            match item {
                Ok(bytes) => {
                    utf8_buf.extend_from_slice(&bytes);
                    let text = match std::str::from_utf8(&utf8_buf) {
                        Ok(s) => {
                            let owned = s.to_string();
                            utf8_buf.clear();
                            owned
                        }
                        Err(e) => {
                            let valid_up_to = e.valid_up_to();
                            if valid_up_to == 0 && utf8_buf.len() < 4 {
                                continue;
                            }
                            let valid =
                                String::from_utf8_lossy(&utf8_buf[..valid_up_to]).into_owned();
                            utf8_buf.drain(..valid_up_to);
                            valid
                        }
                    };
                    if text.is_empty() {
                        continue;
                    }

                    buffer.push_str(&text);

                    while let Some(pos) = buffer.find('\n') {
                        let line = buffer[..pos].to_string();
                        buffer.drain(..=pos);

                        // Custom proxy events for pre-executed tool calls
                        // (e.g. claude-max-api-proxy streaming x_tool_start/x_tool_result)
                        if let Some(event) = parse_proxy_tool_event(&line) {
                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                            continue;
                        }

                        let chunk = match parse_sse_chunk(&line) {
                            Ok(Some(chunk)) => chunk,
                            Ok(None) => {
                                if line.trim().strip_prefix("data:").map(str::trim)
                                    == Some("[DONE]")
                                {
                                    saw_completion = true;
                                    break 'stream;
                                }
                                continue;
                            }
                            Err(e) => {
                                let _ = tx.send(Err(e)).await;
                                return;
                            }
                        };

                        let mut should_emit_tool_calls = false;
                        for choice in &chunk.choices {
                            if choice.finish_reason.is_some() {
                                saw_completion = true;
                            }
                            if let Some(reasoning_delta) = extract_sse_reasoning_delta(choice) {
                                let reasoning_chunk = StreamChunk::reasoning(reasoning_delta);
                                if tx
                                    .send(Ok(StreamEvent::TextDelta(reasoning_chunk)))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            if let Some(text_delta) = extract_sse_text_delta(choice) {
                                let mut text_chunk = StreamChunk::delta(text_delta);
                                if count_tokens {
                                    text_chunk = text_chunk.with_token_estimate();
                                }
                                if tx
                                    .send(Ok(StreamEvent::TextDelta(text_chunk)))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }

                            if let Some(deltas) = choice.delta.tool_calls.as_ref() {
                                for delta in deltas {
                                    let index = delta.index.unwrap_or(tool_calls.len());
                                    if index >= tool_calls.len() {
                                        tool_calls.resize_with(index + 1, Default::default);
                                    }
                                    if let Some(acc) = tool_calls.get_mut(index) {
                                        acc.apply_delta(delta);
                                    }
                                }
                            }

                            if choice.finish_reason.as_deref() == Some("tool_calls") {
                                should_emit_tool_calls = true;
                            }
                        }

                        if let Some(usage) = chunk.usage.clone() {
                            let token_usage = usage.into_provider_usage();
                            if tx.send(Ok(StreamEvent::Usage(token_usage))).await.is_err() {
                                return;
                            }
                        }

                        if should_emit_tool_calls && !emitted_tool_calls {
                            emitted_tool_calls = true;
                            for tool_call in tool_calls.drain(..).filter_map(|tool_call| {
                                tool_call.into_provider_tool_call(
                                    targets_mistral_tool_call_contract,
                                    &mut used_tool_call_ids,
                                )
                            }) {
                                if tx.send(Ok(StreamEvent::ToolCall(tool_call))).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Http(super::stream_idle_error_message(
                            &e,
                            idle_timeout,
                        ))))
                        .await;
                    return;
                }
            }
        }

        if !emitted_tool_calls {
            for tool_call in tool_calls.drain(..).filter_map(|tool_call| {
                tool_call.into_provider_tool_call(
                    targets_mistral_tool_call_contract,
                    &mut used_tool_call_ids,
                )
            }) {
                if tx.send(Ok(StreamEvent::ToolCall(tool_call))).await.is_err() {
                    return;
                }
            }
        }

        crate::stream_guard::finish_sse_stream(&tx, saw_completion, "[DONE] or finish_reason")
            .await;
    });

    let guard = AbortOnDrop::new(handle.abort_handle());
    stream::unfold((rx, guard), |(mut rx, guard)| async move {
        rx.recv().await.map(|event| (event, (rx, guard)))
    })
    .boxed()
}

fn parse_chat_response_body(name: &str, body: &str) -> anyhow::Result<ApiChatResponse> {
    serde_json::from_str::<ApiChatResponseEnvelope>(body)
        .map(ApiChatResponseEnvelope::into_response)
        .map_err(|_| {
            let sanitized = super::sanitize_api_error(body);
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "model_provider": name,
                        "body": &sanitized,
                    })),
                "compatible: unexpected chat-completions payload"
            );
            anyhow::Error::msg(format!(
                "{name} API returned an unexpected chat-completions payload; body={sanitized}"
            ))
        })
}

impl OpenAiCompatibleModelProvider {
    /// `Err` when the stored credential cannot be turned into a header for
    /// this provider's [`AuthStyle`]. Callers propagate it rather than send:
    /// the request would carry no credential at all and be rejected upstream.
    fn apply_auth_header(
        &self,
        req: reqwest::RequestBuilder,
        credential: Option<&str>,
    ) -> anyhow::Result<reqwest::RequestBuilder> {
        apply_auth_to_request(req, &self.auth_header, credential, &self.name)
    }

    /// OpenCode affinity header value for the calling conversation, or `None`
    /// when this provider does not target OpenCode.
    ///
    /// Classifies `chat_completions_url()`, the URL every header-carrying
    /// request is sent to, rather than `base_url`: an `api_path` is appended to
    /// the base, so the base alone need not name the destination host.
    ///
    /// Returns `None` when the operator has already pinned a valid header value
    /// through `extra_headers`: those are baked into the client's default
    /// headers, so adding a second value here would put the header on the wire
    /// twice. A pinned value the client builder skips as invalid does not count.
    fn opencode_session_value(&self) -> Option<String> {
        if crate::opencode_session::operator_pinned_session(&self.extra_headers) {
            return None;
        }
        crate::opencode_session::session_token(&self.chat_completions_url())
    }

    /// Attach the OpenCode affinity header, for request paths that build in the
    /// caller's task.
    ///
    /// Streaming paths must not use this: they build inside
    /// `clawcrew_spawn::spawn!`, where the conversation task-local is no longer
    /// readable. Those resolve `opencode_session_value` before the spawn.
    fn apply_opencode_session_header(
        &self,
        req: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        match self.opencode_session_value() {
            Some(session) => req.header(OPENCODE_SESSION_HEADER, session),
            None => req,
        }
    }

    fn convert_tool_specs(
        &self,
        tools: Option<&[clawcrew_api::tool::ToolSpec]>,
    ) -> Option<Vec<NativeToolSpec>> {
        tools.map(|items| {
            items
                .iter()
                .map(|tool| NativeToolSpec {
                    kind: "function".to_string(),
                    extra: serde_json::Map::new(),
                    function: NativeToolFunctionSpec {
                        extra: serde_json::Map::new(),
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        // Cleaned at most once per registered schema per
                        // provider instance (memoized), then `Arc`-shared into every request
                        // body — never deep-copied per request.
                        parameters: self.schema_cache.clean_shared(
                            &tool.parameters,
                            clawcrew_api::schema::CleaningStrategy::OpenAI,
                        ),
                    },
                })
                .collect()
        })
    }

    fn convert_tool_specs_for_model(
        &self,
        tools: Option<&[clawcrew_api::tool::ToolSpec]>,
        model: &str,
    ) -> Option<Vec<NativeToolSpec>> {
        let mut converted = self.convert_tool_specs(tools)?;
        if !self.local_model_tool_sanitize || !Self::should_sanitize_local_tool_schema(model) {
            return Some(converted);
        }
        // Preserve the pre-existing compatible-provider wire behavior in
        // this allocation-only change. The legacy sanitizer inspected a
        // top-level `parameters` extension even though ordinary OpenAI tool
        // specs place it under `function`; activating a nested rewrite is a
        // separate protocol change that needs its own compatibility contract.
        for tool in &mut converted {
            let Some(raw_parameters) = tool.extra.get("parameters").cloned() else {
                continue;
            };
            let cleaned = clawcrew_api::schema::SchemaCleanr::clean(
                raw_parameters,
                clawcrew_api::schema::CleaningStrategy::Conservative,
            );
            tool.extra.insert("parameters".to_string(), cleaned);
        }
        Some(converted)
    }

    fn should_sanitize_local_tool_schema(model: &str) -> bool {
        let lower = model.to_ascii_lowercase();
        model.is_empty() || lower.contains("gemma-4") || lower.contains("gemma4")
    }

    fn build_native_tool_chat_request(
        &self,
        effective_messages: &[ChatMessage],
        tools: Option<Vec<NativeToolSpec>>,
        model: &str,
        temperature: Option<f64>,
        allow_user_image_parts: bool,
        system_merged: bool,
    ) -> NativeChatRequest {
        let has_tool_entries = tools.as_ref().is_some_and(|tools| !tools.is_empty());
        let tool_choice = has_tool_entries.then(|| "auto".to_string());
        let mut messages =
            self.convert_messages_for_native(effective_messages, allow_user_image_parts);
        let carrier = Self::merged_system_carrier_index(&messages, system_merged);
        self.apply_cache_breakpoints(&mut messages, carrier);

        NativeChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            stream: Some(false),
            // Non-streaming path; `usage` is on the final response body, not
            // gated on `stream_options.include_usage`.
            stream_options: None,
            reasoning_effort: self.reasoning_effort_for_model(model),
            tool_stream: self.tool_stream_for_tools(has_tool_entries),
            tools,
            tool_choice,
            max_tokens: self.max_tokens,
            extra_body: self.extra_body.clone(),
        }
    }

    fn build_raw_native_tool_chat_request<'a>(
        &self,
        effective_messages: &[ChatMessage],
        tools: Option<&'a [serde_json::Value]>,
        model: &str,
        temperature: Option<f64>,
        allow_user_image_parts: bool,
        system_merged: bool,
    ) -> NativeChatRequest<&'a [serde_json::Value]> {
        let has_tool_entries = tools.is_some_and(|tools| !tools.is_empty());
        let mut messages =
            self.convert_messages_for_native(effective_messages, allow_user_image_parts);
        let carrier = Self::merged_system_carrier_index(&messages, system_merged);
        self.apply_cache_breakpoints(&mut messages, carrier);
        NativeChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            stream: Some(false),
            stream_options: None,
            reasoning_effort: self.reasoning_effort_for_model(model),
            tool_stream: self.tool_stream_for_tools(has_tool_entries),
            tools,
            tool_choice: has_tool_entries.then(|| "auto".to_string()),
            max_tokens: self.max_tokens,
            extra_body: self.extra_body.clone(),
        }
    }

    /// Streaming counterpart of [`Self::build_native_tool_chat_request`],
    /// used by `stream_chat` when native tools are present.
    fn build_streaming_native_tool_request(
        &self,
        model: &str,
        effective_messages: &[ChatMessage],
        tools: Option<Vec<NativeToolSpec>>,
        temperature: Option<f64>,
        options_enabled: bool,
        merge: bool,
        system_merged: bool,
    ) -> NativeChatRequest {
        // Guard on the converted tools being non-empty (not just the raw
        // input being non-empty): convert_tool_specs_for_model can sanitize
        // a non-empty input down to None, and tool_choice without a tools
        // field is an HTTP 400 on vLLM 0.19+. Computed before `tools` moves
        // into the request so the converted list is never copied.
        let tool_choice = tools
            .as_ref()
            .and_then(|t| (!t.is_empty()).then(|| "auto".to_string()));
        let mut messages = self.convert_messages_for_native(effective_messages, !merge);
        let carrier = Self::merged_system_carrier_index(&messages, system_merged);
        self.apply_cache_breakpoints(&mut messages, carrier);
        NativeChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            reasoning_effort: self.reasoning_effort_for_model(model),
            tool_stream: if options_enabled {
                self.tool_stream_for_tools(true)
            } else {
                None
            },
            stream: Some(options_enabled),
            // Mirror the no-tools path: opt the streaming response into a
            // final `usage` event so `/ws/chat` can record token usage
            // even when native tools are active.
            stream_options: options_enabled.then_some(StreamOptionsBody {
                include_usage: true,
            }),
            tools,
            tool_choice,
            max_tokens: self.max_tokens,
            extra_body: self.extra_body.clone(),
        }
    }

    async fn normalize_messages_for_upstream(
        &self,
        messages: &[ChatMessage],
    ) -> anyhow::Result<Vec<ChatMessage>> {
        let config = self.multimodal.clone();
        let sanitized;
        let messages = if self.tool_result_image_policy == ToolResultImagePolicy::Omit {
            sanitized = messages
                .iter()
                .map(|message| {
                    if message.role == "tool" {
                        ChatMessage {
                            role: message.role.clone(),
                            content: Self::sanitize_tool_result_message(&message.content),
                        }
                    } else {
                        message.clone()
                    }
                })
                .collect::<Vec<_>>();
            sanitized.as_slice()
        } else {
            messages
        };
        let prepared = multimodal::prepare_messages_for_provider(messages, &config).await?;
        Ok(prepared.messages)
    }

    fn to_message_content(
        role: &str,
        content: &str,
        allow_user_image_parts: bool,
    ) -> MessageContent {
        if role != "user" || !allow_user_image_parts {
            return MessageContent::Text(content.to_string());
        }
        Self::content_with_image_parts(content)
    }

    fn content_with_image_parts(content: &str) -> MessageContent {
        let (cleaned_text, image_refs) = multimodal::parse_image_markers(content);
        if image_refs.is_empty() {
            return MessageContent::Text(content.to_string());
        }

        let mut parts = Vec::with_capacity(image_refs.len() + 1);
        let trimmed_text = cleaned_text.trim();
        if !trimmed_text.is_empty() {
            parts.push(MessagePart::Text {
                text: trimmed_text.to_string(),
                cache_control: None,
            });
        }

        for image_ref in image_refs {
            parts.push(MessagePart::ImageUrl {
                image_url: ImageUrlPart { url: image_ref },
            });
        }

        MessageContent::Parts(parts)
    }

    fn sanitize_tool_result_content(content: &str) -> String {
        let mut cleaned = String::with_capacity(content.len());
        let mut cursor = 0;
        let mut removed_image_marker = false;

        while let Some(relative_start) = content[cursor..].find("[IMAGE:") {
            let start = cursor + relative_start;
            cleaned.push_str(&content[cursor..start]);
            removed_image_marker = true;

            let after_prefix = start + "[IMAGE:".len();
            cursor = content[after_prefix..]
                .find(']')
                .map(|relative_end| after_prefix + relative_end + 1)
                .unwrap_or(content.len());
            if cursor == content.len() {
                break;
            }
        }

        cleaned.push_str(&content[cursor..]);
        if !removed_image_marker {
            return content.to_string();
        }

        if !cleaned.is_empty() {
            cleaned.push_str("\n\n");
        }
        cleaned.push_str(TOOL_RESULT_IMAGE_OMITTED_NOTICE);
        cleaned
    }

    fn sanitize_tool_result_message(content: &str) -> String {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(content)
            && let Some(tool_content) = value.get_mut("content")
        {
            let raw_content = tool_content
                .as_str()
                .map(ToString::to_string)
                .unwrap_or_else(|| tool_content.to_string());
            let sanitized_content = Self::sanitize_tool_result_content(&raw_content);
            if sanitized_content == raw_content {
                return content.to_string();
            }
            *tool_content = serde_json::Value::String(sanitized_content);
            return value.to_string();
        }

        Self::sanitize_tool_result_content(content)
    }

    fn message_content_for_role(
        &self,
        role: &str,
        content: &str,
        allow_user_image_parts: bool,
        allow_tool_image_parts: bool,
    ) -> MessageContent {
        if role == "tool" {
            if self.tool_result_image_policy == ToolResultImagePolicy::Omit {
                return MessageContent::Text(Self::sanitize_tool_result_content(content));
            }
            if allow_tool_image_parts && allow_user_image_parts {
                return Self::content_with_image_parts(content);
            }
            return MessageContent::Text(content.to_string());
        }
        Self::to_message_content(role, content, allow_user_image_parts)
    }

    fn convert_messages_for_native(
        &self,
        messages: &[ChatMessage],
        allow_user_image_parts: bool,
    ) -> Vec<NativeMessage> {
        let targets_mistral_tool_call_contract = self.targets_mistral_tool_call_contract();
        let requires_string_tool_call_content = self.requires_string_tool_call_content();
        let mut used_tool_call_ids = std::collections::HashSet::new();
        let mut tool_call_id_map = std::collections::HashMap::new();
        let mut last_assistant_tool_call_ids: Vec<String> = Vec::new();
        let mut tool_name_map = std::collections::HashMap::new();

        messages
            .iter()
            .map(|message| {
                if message.role == "assistant"
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content)
                    && let Some(tool_calls_value) = value.get("tool_calls")
                    && let Ok(parsed_calls) =
                        serde_json::from_value::<Vec<ProviderToolCall>>(tool_calls_value.clone())
                {
                    let tool_calls = parsed_calls
                        .into_iter()
                        .map(|tc| {
                            let tc_id = tc.id.clone();
                            let tc_name = tc.name.clone();
                            tool_name_map.insert(tc_id, tc_name);
                            ToolCall {
                                id: Some({
                                    let normalized_id = reserve_tool_call_id_for_contract(
                                        targets_mistral_tool_call_contract,
                                        Some(tc.id.clone()),
                                        &mut used_tool_call_ids,
                                    );
                                    tool_call_id_map.insert(tc.id.clone(), normalized_id.clone());
                                    normalized_id
                                }),
                                kind: Some("function".to_string()),
                                function: Some(Function {
                                    name: Some(tc.name),
                                    arguments: Some(tc.arguments),
                                }),
                                name: None,
                                arguments: None,
                                parameters: None,
                                // Round-trip extra_content (e.g. Gemini
                                // thoughtSignature) — dropping it here was the bug.
                                extra_content: tc.extra_content,
                            }
                        })
                        .collect::<Vec<_>>();

                    last_assistant_tool_call_ids =
                        tool_calls.iter().filter_map(|tc| tc.id.clone()).collect();

                    let content = crate::request_payload::non_empty_string_field(&value, "content")
                        .map(MessageContent::Text)
                        .or_else(|| {
                            requires_string_tool_call_content
                                .then(|| MessageContent::Text(String::new()))
                        });

                    let (reasoning_content, reasoning) =
                        self.assistant_reasoning_pair_for_replay(&value);

                    return NativeMessage {
                        role: "assistant".to_string(),
                        content,
                        tool_call_id: None,
                        tool_calls: Some(tool_calls),
                        reasoning_content,
                        reasoning,
                        name: None,
                    };
                }

                if message.role == "assistant"
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content)
                    && value.get("tool_calls").is_none()
                    && Self::assistant_reasoning_value(&value).is_some()
                    && matches!(
                        value.get("content"),
                        None | Some(serde_json::Value::Null | serde_json::Value::String(_))
                    )
                {
                    let content = value
                        .get("content")
                        .and_then(serde_json::Value::as_str)
                        .map(|value| MessageContent::Text(value.to_string()));

                    let (reasoning_content, reasoning) =
                        self.assistant_reasoning_pair_for_replay(&value);

                    return NativeMessage {
                        role: "assistant".to_string(),
                        content,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content,
                        reasoning,
                        name: None,
                    };
                }

                if message.role == "tool"
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content)
                {
                    let mut tool_call_id = value
                        .get("tool_call_id")
                        .and_then(serde_json::Value::as_str)
                        .map(|raw_id| {
                            tool_call_id_map.get(raw_id).cloned().unwrap_or_else(|| {
                                let normalized_id = reserve_tool_call_id_for_contract(
                                    targets_mistral_tool_call_contract,
                                    Some(raw_id.to_string()),
                                    &mut used_tool_call_ids,
                                );
                                tool_call_id_map.insert(raw_id.to_string(), normalized_id.clone());
                                normalized_id
                            })
                        });
                    // Fallback: if the tool result JSON dropped the tool_call_id,
                    // borrow the first id from the most recent assistant message.
                    // Some multi-turn reconstruction paths strip this field, and
                    // strict backends (Groq, Mistral) reject null/missing ids.
                    if tool_call_id.is_none() && !last_assistant_tool_call_ids.is_empty() {
                        tool_call_id = last_assistant_tool_call_ids.first().cloned();
                    }
                    let content = value
                        .get("content")
                        .and_then(serde_json::Value::as_str)
                        .map(|value| {
                            self.message_content_for_role(
                                "tool",
                                value,
                                allow_user_image_parts,
                                true,
                            )
                        })
                        .or_else(|| {
                            Some(self.message_content_for_role(
                                "tool",
                                &message.content,
                                allow_user_image_parts,
                                false,
                            ))
                        });

                    // Groq native tool calling requires the tool `name` on
                    // every role-tool message; look it up from the paired
                    // assistant tool-call, falling back to any name carried
                    // in the tool message content itself./
                    let tool_name = value
                        .get("tool_call_id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|raw_id| tool_name_map.get(raw_id).cloned())
                        .or_else(|| {
                            value
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .map(ToString::to_string)
                        });

                    return NativeMessage {
                        role: "tool".to_string(),
                        content,
                        tool_call_id,
                        tool_calls: None,
                        reasoning_content: None,
                        reasoning: None,
                        name: tool_name,
                    };
                }

                NativeMessage {
                    role: message.role.clone(),
                    content: Some(self.message_content_for_role(
                        &message.role,
                        &message.content,
                        allow_user_image_parts,
                        false,
                    )),
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                    reasoning: None,
                    name: None,
                }
            })
            .collect()
    }

    fn strip_native_tool_messages(&self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        if self.native_tool_calling {
            return messages.to_vec();
        }
        let intermediate = messages.iter().enumerate().filter_map(|(index, msg)| {
            if ChatMessage::should_skip_internal_pruning_marker(messages, index) {
                return None;
            }
            if msg.role == "tool" {
                return None;
            }
            if msg.role == "assistant"
                && let Ok(value) = serde_json::from_str::<serde_json::Value>(&msg.content)
                && value.get("tool_calls").is_some()
            {
                let text = value
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                return if text.is_empty() {
                    None
                } else {
                    Some(ChatMessage::assistant(&text))
                };
            }
            Some(msg.clone())
        });

        let mut coalesced: Vec<ChatMessage> = Vec::with_capacity(messages.len());
        for msg in intermediate {
            match coalesced.last_mut() {
                Some(last) if last.role == msg.role && msg.role != "system" => {
                    if !last.content.is_empty() && !msg.content.is_empty() {
                        last.content.push_str("\n\n");
                    }
                    last.content.push_str(&msg.content);
                }
                _ => coalesced.push(msg),
            }
        }
        coalesced
    }

    fn with_prompt_guided_tool_instructions(
        messages: &[ChatMessage],
        tools: Option<&[clawcrew_api::tool::ToolSpec]>,
    ) -> Vec<ChatMessage> {
        let Some(tools) = tools else {
            return messages.to_vec();
        };

        if tools.is_empty() {
            return messages.to_vec();
        }

        let instructions = clawcrew_api::model_provider::build_tool_instructions_text(tools);
        let mut modified_messages = messages.to_vec();

        if let Some(system_message) = modified_messages.iter_mut().find(|m| m.role == "system") {
            if !system_message.content.is_empty() {
                system_message.content.push_str("\n\n");
            }
            system_message.content.push_str(&instructions);
        } else {
            modified_messages.insert(0, ChatMessage::system(instructions));
        }

        modified_messages
    }

    /// Whether this backend requires `content` to be a string on assistant
    /// tool-call messages.
    ///
    /// OpenAI accepts the field absent or null there, and omitting it is the
    /// default. Cloudflare Workers AI validates against a stricter schema and
    /// rejects the whole request with HTTP 400 (`AiError: Bad input ...
    /// required properties at '/messages/N' are 'role,content'`). The failure
    /// is intermittent in practice: a model that emits text alongside its tool
    /// call produces a non-empty content and succeeds, while the far more
    /// common no-text tool call fails.
    fn requires_string_tool_call_content(&self) -> bool {
        reqwest::Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(|h| h.to_ascii_lowercase()))
            .is_some_and(|host| {
                host == "api.cloudflare.com"
                    || host == "gateway.ai.cloudflare.com"
                    || host.ends_with(".cloudflare.com")
            })
    }

    fn targets_mistral_tool_call_contract(&self) -> bool {
        if self.name.eq_ignore_ascii_case("mistral") {
            return true;
        }

        reqwest::Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(|h| h.to_ascii_lowercase()))
            .is_some_and(|host| host == "mistral.ai" || host.ends_with(".mistral.ai"))
    }

    fn reserve_tool_call_id(
        &self,
        raw_id: Option<String>,
        used_ids: &mut std::collections::HashSet<String>,
    ) -> String {
        reserve_tool_call_id_for_contract(
            self.targets_mistral_tool_call_contract(),
            raw_id,
            used_ids,
        )
    }

    fn parse_native_response(&self, message: ResponseMessage) -> ProviderChatResponse {
        let text = message.effective_content_optional();
        let reasoning_content = message.reasoning_content.clone();
        let mut used_tool_call_ids = std::collections::HashSet::new();
        let tool_calls = message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tc| {
                let name = tc.function_name()?;
                let arguments = tc.function_arguments().unwrap_or_else(|| "{}".to_string());
                let normalized_arguments = sanitize_tool_arguments(&name, &arguments);
                Some(ProviderToolCall {
                    id: self.reserve_tool_call_id(tc.id, &mut used_tool_call_ids),
                    name,
                    arguments: normalized_arguments,
                    extra_content: tc.extra_content,
                })
            })
            .collect::<Vec<_>>();

        ProviderChatResponse {
            text,
            tool_calls,
            usage: None,
            reasoning_content,
        }
    }

    fn is_native_tool_schema_unsupported(status: reqwest::StatusCode, error: &str) -> bool {
        if !matches!(
            status,
            reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::UNPROCESSABLE_ENTITY
        ) {
            return false;
        }

        let lower = error.to_lowercase();
        [
            "unknown parameter: tools",
            "unsupported parameter: tools",
            "unrecognized field `tools`",
            "does not support tools",
            "function calling is not supported",
            "tool_choice",
            "tool call validation failed",
            "was not in request",
        ]
        .iter()
        .any(|hint| lower.contains(hint))
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleModelProvider {
    fn default_base_url(&self) -> Option<&str> {
        self.canonical_base_url
    }

    fn capabilities(&self) -> clawcrew_api::model_provider::ProviderCapabilities {
        clawcrew_api::model_provider::ProviderCapabilities {
            native_tool_calling: self.native_tool_calling,
            vision: self.supports_vision,
            prompt_caching: self.cache_passthrough,
            extended_thinking: false,
        }
    }

    async fn list_models(&self) -> anyhow::Result<Vec<String>> {
        // When a credential is present, hit the model_provider's native /models endpoint
        // (OpenAI-compatible: GET {base_url}/models). Local OpenAI-compatible
        // servers with a public catalog use the same path without an Authorization header.
        let list_credential = self.resolve_credential().await?;
        if list_credential.is_some() || self.public_model_listing {
            let url = self.models_url();
            let response = self
                .apply_auth_header(self.http_client().get(&url), list_credential.as_deref())?
                .send()
                .await
                .map_err(|e| {
                    ::clawcrew_log::record!(
                        ERROR,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "model_provider": &self.name,
                                "url": &url,
                                "phase": "model_list_request",
                                "error": super::format_error_chain(&e),
                            })),
                        "compatible: model list request failed"
                    );
                    anyhow::Error::msg(format!(
                        "{} model list request failed: {url}: {e}",
                        self.name
                    ))
                })?;
            if !response.status().is_success() {
                let status = response.status();
                anyhow::bail!("{} model list failed at {url}: HTTP {status}", self.name);
            }
            let raw = read_body_capped(response, MAX_MODELS_RESPONSE_BYTES)
                .await
                .map_err(|e| {
                    ::clawcrew_log::record!(
                        ERROR,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "model_provider": &self.name,
                                "phase": "model_list_read",
                                "error": format!("{e:#}"),
                            })),
                        "compatible: model list body was too large or could not be read"
                    );
                    anyhow::Error::msg(format!(
                        "{} model list body was not readable: {e}",
                        self.name
                    ))
                })?;
            let body: ModelsResponse = serde_json::from_slice(&raw).map_err(|e| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "model_provider": &self.name,
                            "phase": "model_list_parse",
                            "error": super::format_error_chain(&e),
                        })),
                    "compatible: model list returned invalid JSON"
                );
                anyhow::Error::msg(format!(
                    "{} model list returned invalid JSON: {e}",
                    self.name
                ))
            })?;
            return Ok(normalize_model_ids(body));
        }
        // No credential — try models.dev first, then OpenRouter as a
        // last-resort fallback for vendors that aren't in models.dev.
        if let Some(key) = &self.models_dev_key {
            match crate::models_dev::list_models_for(key).await {
                Ok(models) if !models.is_empty() => return Ok(models),
                Ok(_) => {} // empty → fall through to openrouter
                Err(e) => {
                    if self.openrouter_vendor_prefix.is_none() {
                        return Err(e);
                    }
                }
            }
        }
        match &self.openrouter_vendor_prefix {
            Some(prefix) => crate::openrouter_catalog::list_models_for_vendor(prefix).await,
            None => anyhow::bail!("live model listing is not supported for this model_provider"),
        }
    }

    async fn list_models_with_pricing(
        &self,
    ) -> anyhow::Result<Vec<clawcrew_api::model_provider::ModelInfo>> {
        // When a credential is present, hit the provider's native /models
        // endpoint — this returns pricing data that we can capture.
        let list_credential = self.resolve_credential().await?;
        if list_credential.is_some() || self.public_model_listing {
            let url = self.models_url();
            let response = self
                .apply_auth_header(self.http_client().get(&url), list_credential.as_deref())?
                .send()
                .await
                .map_err(|e| {
                    ::clawcrew_log::record!(
                        ERROR,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "model_provider": &self.name,
                                "url": &url,
                                "phase": "model_list_request",
                                "error": super::format_error_chain(&e),
                            })),
                        "compatible: model list request failed"
                    );
                    anyhow::Error::msg(format!(
                        "{} model list request failed: {url}: {e}",
                        self.name
                    ))
                })?;
            if !response.status().is_success() {
                let status = response.status();
                anyhow::bail!("{} model list failed at {url}: HTTP {status}", self.name);
            }
            let raw = read_body_capped(response, MAX_MODELS_RESPONSE_BYTES)
                .await
                .map_err(|e| {
                    ::clawcrew_log::record!(
                        ERROR,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "model_provider": &self.name,
                                "phase": "model_list_read",
                                "error": format!("{e:#}"),
                            })),
                        "compatible: model list body was too large or could not be read"
                    );
                    anyhow::Error::msg(format!(
                        "{} model list body was not readable: {e}",
                        self.name
                    ))
                })?;
            let body: ModelsResponse = serde_json::from_slice(&raw).map_err(|e| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "model_provider": &self.name,
                            "phase": "model_list_parse",
                            "error": super::format_error_chain(&e),
                        })),
                    "compatible: model list returned invalid JSON"
                );
                anyhow::Error::msg(format!(
                    "{} model list returned invalid JSON: {e}",
                    self.name
                ))
            })?;
            return Ok(normalize_models_with_pricing(body));
        }
        // No credential — try models.dev first (no pricing from that source),
        // then fall back to OpenRouter which does include pricing.
        if let Some(key) = &self.models_dev_key {
            match crate::models_dev::list_models_with_context_for(key).await {
                Ok(models) if !models.is_empty() => {
                    return Ok(models_dev_to_model_info(models));
                }
                Ok(_) => {} // empty → fall through to openrouter
                Err(_) if self.openrouter_vendor_prefix.is_none() => {
                    return Ok(Vec::new());
                }
                Err(_) => {} // fall through to openrouter
            }
        }
        match &self.openrouter_vendor_prefix {
            Some(prefix) => {
                crate::openrouter_catalog::list_models_for_vendor_with_pricing(prefix).await
            }
            None => Ok(Vec::new()),
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<String> {
        let credential = self.resolve_credential().await?;

        // Normalize image markers (e.g. local file paths from channel
        // attachments) into base64 data URIs before this message reaches the
        // upstream provider.
        let user_msg = ChatMessage {
            role: "user".to_string(),
            content: message.to_string(),
        };
        let normalized_user = self
            .normalize_messages_for_upstream(std::slice::from_ref(&user_msg))
            .await?
            .pop()
            .unwrap_or(user_msg);
        let normalized_message = normalized_user.content;

        let merge = self.effective_merge_system(model);
        let mut messages = Vec::new();

        if merge {
            let content = match system_prompt {
                Some(sys) => format!("{sys}\n\n{normalized_message}"),
                None => normalized_message,
            };
            messages.push(Message {
                role: "user".to_string(),
                content: Self::to_message_content("user", &content, !merge),
            });
        } else {
            if let Some(sys) = system_prompt {
                messages.push(Message {
                    role: "system".to_string(),
                    content: MessageContent::Text(sys.to_string()),
                });
            }
            messages.push(Message {
                role: "user".to_string(),
                content: Self::to_message_content("user", &normalized_message, true),
            });
        }

        let request = ApiChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            stream: Some(false),
            stream_options: None,
            reasoning_effort: self.reasoning_effort_for_model(model),
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: self.max_tokens,
            extra_body: self.extra_body.clone(),
        };
        // No cache breakpoints here, deliberately: this text-only helper
        // never surfaces response usage to dispatch accounting, so a
        // premium cache write it triggered could never be accounted for
        // (fallback re-entries through this path return `usage: None`).
        // The flag is honored on the structured paths (`chat`,
        // `chat_with_tools`, `stream_chat`), which capture usage; the
        // provider docs describe this usage-capture boundary.

        let url = self.chat_completions_url();

        let response = match self
            .apply_opencode_session_header(self.apply_auth_header(
                self.http_client().post(&url).json(&request),
                credential.as_deref(),
            )?)
            .send()
            .await
        {
            Ok(response) => response,
            Err(chat_error) => {
                return Err(chat_error.into());
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await?;
            let sanitized = super::sanitize_api_error(&error);
            anyhow::bail!("{} API error ({status}): {sanitized}", self.name);
        }

        let body = response.text().await?;
        let chat_response = parse_chat_response_body(&self.name, &body)?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| {
                if c.message.tool_calls.is_some()
                    && c.message
                        .tool_calls
                        .as_ref()
                        .is_some_and(|t: &Vec<_>| !t.is_empty())
                {
                    serde_json::to_string(&c.message)
                        .unwrap_or_else(|_| c.message.effective_content())
                } else {
                    c.message.effective_content()
                }
            })
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"model_provider": &self.name})),
                    "compatible: empty choices in response"
                );
                anyhow::Error::msg(format!("No response from {}", self.name))
            })
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<String> {
        let credential = self.resolve_credential().await?;

        let normalized = self.normalize_messages_for_upstream(messages).await?;
        let merge = self.effective_merge_system(model);
        let (effective_messages, _system_merged) =
            Self::flatten_system_messages(&normalized, merge);
        // Strip native tool constructs for non-native-tool model_providers.
        let effective_messages = self.strip_native_tool_messages(&effective_messages);
        let api_messages: Vec<Message> = effective_messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: self.message_content_for_role(&m.role, &m.content, !merge, false),
            })
            .collect();

        let request = ApiChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature,
            stream: Some(false),
            stream_options: None,
            reasoning_effort: self.reasoning_effort_for_model(model),
            tool_stream: None,
            tools: None,
            tool_choice: None,
            max_tokens: self.max_tokens,
            extra_body: self.extra_body.clone(),
        };
        // No cache breakpoints here, deliberately: this text-only helper
        // never surfaces response usage to dispatch accounting, so a
        // premium cache write it triggered could never be accounted for
        // (fallback re-entries through this path return `usage: None`).
        // The flag is honored on the structured paths (`chat`,
        // `chat_with_tools`, `stream_chat`), which capture usage; the
        // provider docs describe this usage-capture boundary.

        let url = self.chat_completions_url();
        let response = match self
            .apply_opencode_session_header(self.apply_auth_header(
                self.http_client().post(&url).json(&request),
                credential.as_deref(),
            )?)
            .send()
            .await
        {
            Ok(response) => response,
            Err(chat_error) => return Err(chat_error.into()),
        };

        if !response.status().is_success() {
            return Err(super::api_error(&self.name, response).await);
        }

        let body = response.text().await?;
        let chat_response = parse_chat_response_body(&self.name, &body)?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| {
                if c.message.tool_calls.is_some()
                    && c.message
                        .tool_calls
                        .as_ref()
                        .is_some_and(|t: &Vec<_>| !t.is_empty())
                {
                    serde_json::to_string(&c.message)
                        .unwrap_or_else(|_| c.message.effective_content())
                } else {
                    c.message.effective_content()
                }
            })
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"model_provider": &self.name})),
                    "compatible: empty choices in response"
                );
                anyhow::Error::msg(format!("No response from {}", self.name))
            })
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.resolve_credential().await?;

        let normalized = self.normalize_messages_for_upstream(messages).await?;
        let merge = self.effective_merge_system(model);
        let (effective_messages, system_merged) = Self::flatten_system_messages(&normalized, merge);
        let effective_messages = if self.native_tool_calling {
            effective_messages
        } else {
            self.strip_native_tool_messages(&effective_messages)
        };
        let request = self.build_raw_native_tool_chat_request(
            &effective_messages,
            (!tools.is_empty()).then_some(tools),
            model,
            temperature,
            !merge,
            system_merged,
        );
        let mut payload = serde_json::to_value(request)?;
        let tools_count = payload
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);

        let url = self.chat_completions_url();
        let response = loop {
            let response = match self
                .apply_opencode_session_header(self.apply_auth_header(
                    self.http_client().post(&url).json(&payload),
                    credential.as_deref(),
                )?)
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                        &format!(
                            "{} native tool call transport failed: {error}; falling back to history path",
                            self.name
                        )
                    );
                    let text = self.chat_with_history(messages, model, temperature).await?;
                    return Ok(ProviderChatResponse {
                        text: Some(text),
                        tool_calls: vec![],
                        usage: None,
                        reasoning_content: None,
                    });
                }
            };
            if response.status().is_success() {
                break response;
            }

            let status = response.status();
            let error = response.text().await?;
            if tools_count > 0
                && super::rejects_tools_with_reasoning_effort(status, &error)
                && ensure_reasoning_effort_none(&mut payload)
            {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Retry)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "provider": &self.name,
                            "alias": &self.alias,
                            "request_api": "chat_completions",
                            "model": model,
                            "stream": false,
                            "tools_count": tools_count,
                            "reasoning_effort_overridden": true,
                            "reasoning_effort_fallback": "none",
                            "reasoning_effort_override_reason": "endpoint_rejected_tools_with_reasoning",
                            "status": status.as_u16(),
                        })),
                    "compatible provider retrying with reasoning effort disabled after endpoint capability rejection"
                );
                continue;
            }

            return Err(super::api_error_from_parts(&self.name, status, &error));
        };

        let body = response.text().await?;
        let chat_response = parse_chat_response_body(&self.name, &body)?;
        let usage = chat_response.usage.map(UsageInfo::into_provider_usage);
        let choice = chat_response.choices.into_iter().next().ok_or_else(|| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"model_provider": &self.name})),
                "compatible: empty choices in response"
            );
            anyhow::Error::msg(format!("No response from {}", self.name))
        })?;

        let mut result = self.parse_native_response(choice.message);
        result.usage = usage;
        Ok(result)
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.resolve_credential().await?;

        let normalized = self
            .normalize_messages_for_upstream(request.messages)
            .await?;
        let merge = self.effective_merge_system(model);
        let (effective_messages, system_merged) = Self::flatten_system_messages(&normalized, merge);
        let effective_messages = if self.native_tool_calling {
            effective_messages
        } else {
            self.strip_native_tool_messages(&effective_messages)
        };

        let tools = self.convert_tool_specs_for_model(request.tools, model);
        let native_request = self.build_native_tool_chat_request(
            &effective_messages,
            tools,
            model,
            temperature,
            !merge,
            system_merged,
        );
        let mut payload = serde_json::to_value(native_request)?;
        let tools_count = payload
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        let reasoning_effort_omitted =
            self.reasoning_effort.is_some() && payload.get("reasoning_effort").is_none();
        let reasoning_effort_omission_reason =
            reasoning_effort_omitted.then_some("model_ineligible");
        if ::clawcrew_log::debug_enabled() {
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Send)
                    .with_attrs(::serde_json::json!({
                        "provider": &self.name,
                        "alias": &self.alias,
                        "request_api": "chat_completions",
                        "model": model,
                        "stream": false,
                        "native_tool_calling": self.native_tool_calling,
                        "tools_count": tools_count,
                        "tool_choice": payload.get("tool_choice"),
                        "reasoning_effort_omitted": reasoning_effort_omitted,
                        "reasoning_effort_omission_reason": reasoning_effort_omission_reason,
                    })),
                "compatible provider request prepared"
            );
        }

        let url = self.chat_completions_url();
        let response = loop {
            let response = match self
                .apply_opencode_session_header(self.apply_auth_header(
                    self.http_client().post(&url).json(&payload),
                    credential.as_deref(),
                )?)
                .send()
                .await
            {
                Ok(response) => response,
                Err(chat_error) => return Err(chat_error.into()),
            };
            if response.status().is_success() {
                break response;
            }

            let status = response.status();
            let error = response.text().await?;
            let sanitized = super::sanitize_api_error(&error);

            if tools_count > 0
                && super::rejects_tools_with_reasoning_effort(status, &error)
                && ensure_reasoning_effort_none(&mut payload)
            {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Retry)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "provider": &self.name,
                            "alias": &self.alias,
                            "request_api": "chat_completions",
                            "model": model,
                            "stream": false,
                            "tools_count": tools_count,
                            "reasoning_effort_overridden": true,
                            "reasoning_effort_fallback": "none",
                            "reasoning_effort_override_reason": "endpoint_rejected_tools_with_reasoning",
                            "status": status.as_u16(),
                        })),
                    "compatible provider retrying with reasoning effort disabled after endpoint capability rejection"
                );
                continue;
            }

            if Self::is_native_tool_schema_unsupported(status, &sanitized) {
                let fallback_messages =
                    Self::with_prompt_guided_tool_instructions(request.messages, request.tools);
                let text = self
                    .chat_with_history(&fallback_messages, model, temperature)
                    .await?;
                return Ok(ProviderChatResponse {
                    text: Some(text),
                    tool_calls: vec![],
                    usage: None,
                    reasoning_content: None,
                });
            }

            anyhow::bail!("{} API error ({status}): {sanitized}", self.name);
        };

        let body = response.text().await?;
        let native_response = parse_chat_response_body(&self.name, &body)?;
        let usage = native_response.usage.map(UsageInfo::into_provider_usage);
        let message = native_response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"model_provider": &self.name})),
                    "compatible: empty choices in response"
                );
                anyhow::Error::msg(format!("No response from {}", self.name))
            })?;

        let mut result = self.parse_native_response(message);
        result.usage = usage;
        Ok(result)
    }

    fn supports_native_tools(&self) -> bool {
        self.native_tool_calling
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_streaming_tool_events(&self) -> bool {
        // The responses API always supports streaming tool events.
        self.native_tool_calling
    }

    fn stream_chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: Option<f64>,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamEvent>> {
        if !options.enabled {
            return stream::once(async { Ok(StreamEvent::Final) }).boxed();
        }

        let provider = self.clone();
        // Resolved here, not in the spawned task: `spawn!` propagates the
        // tracing span but not task-locals, so the conversation scope is
        // unreadable past this point.
        let opencode_session = self.opencode_session_value();
        let messages_owned: Vec<ChatMessage> = request.messages.to_vec();
        let tools_owned: Option<Vec<clawcrew_api::tool::ToolSpec>> =
            request.tools.map(<[clawcrew_api::tool::ToolSpec]>::to_vec);
        let model = model.to_string();
        let count_tokens = options.count_tokens;
        let options_enabled = options.enabled;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamEvent>>(100);

        let handle = ::clawcrew_spawn::spawn!(async move {
            let normalized = match provider
                .normalize_messages_for_upstream(&messages_owned)
                .await
            {
                Ok(n) => n,
                Err(err) => {
                    let _ = tx
                        .send(Err(StreamError::ModelProvider(err.to_string())))
                        .await;
                    return;
                }
            };

            let merge = provider.effective_merge_system(&model);
            let (effective_messages, system_merged) =
                Self::flatten_system_messages(&normalized, merge);
            let effective_messages = provider.strip_native_tool_messages(&effective_messages);
            let tools = provider.convert_tool_specs_for_model(tools_owned.as_deref(), &model);
            let tools_count = tools.as_ref().map_or(0, Vec::len);
            let has_tools = tools_count > 0;
            let reasoning_effort = provider.reasoning_effort_for_model(&model);
            let reasoning_effort_omitted =
                provider.reasoning_effort.is_some() && reasoning_effort.is_none();
            let reasoning_effort_omission_reason =
                reasoning_effort_omitted.then_some("model_ineligible");

            let payload_result = if has_tools {
                serde_json::to_value(provider.build_streaming_native_tool_request(
                    &model,
                    &effective_messages,
                    tools,
                    temperature,
                    options_enabled,
                    merge,
                    system_merged,
                ))
            } else {
                let mut messages: Vec<Message> = effective_messages
                    .iter()
                    .map(|message| Message {
                        role: message.role.clone(),
                        content: provider.message_content_for_role(
                            &message.role,
                            &message.content,
                            !merge,
                            false,
                        ),
                    })
                    .collect();
                let carrier = Self::merged_system_carrier_index(&messages, system_merged);
                provider.apply_cache_breakpoints(&mut messages, carrier);

                serde_json::to_value(ApiChatRequest {
                    model: model.clone(),
                    messages,
                    temperature,
                    reasoning_effort: reasoning_effort.clone(),
                    tool_stream: if options_enabled {
                        provider.tool_stream_for_tools(false)
                    } else {
                        None
                    },
                    stream: Some(options_enabled),
                    stream_options: options_enabled.then_some(StreamOptionsBody {
                        include_usage: true,
                    }),
                    tools: None,
                    tool_choice: None,
                    max_tokens: provider.max_tokens,
                    extra_body: provider.extra_body.clone(),
                })
            };

            let mut payload = match payload_result {
                Ok(payload) => payload,
                Err(error) => {
                    let _ = tx.send(Err(StreamError::Json(error))).await;
                    return;
                }
            };
            if ::clawcrew_log::debug_enabled() {
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Send)
                        .with_attrs(::serde_json::json!({
                            "provider": &provider.name,
                            "alias": &provider.alias,
                            "request_api": "chat_completions",
                            "model": &model,
                            "stream": options_enabled,
                            "native_tool_calling": provider.native_tool_calling,
                            "tools_count": tools_count,
                            "tool_choice": payload.get("tool_choice"),
                            "reasoning_effort_omitted": reasoning_effort_omitted,
                            "reasoning_effort_omission_reason": reasoning_effort_omission_reason,
                        })),
                    "compatible streaming provider request prepared"
                );
            }

            let url = provider.chat_completions_url();
            let client = provider.streaming_http_client();
            let idle_timeout = super::stream_idle_timeout(provider.timeout_secs);
            let auth_header = provider.auth_header.clone();
            let credential = match provider.resolve_credential().await {
                Ok(credential) => credential,
                Err(error) => {
                    let _ = tx
                        .send(Err(StreamError::ModelProvider(error.to_string())))
                        .await;
                    return;
                }
            };
            let targets_mistral_tool_call_contract = provider.targets_mistral_tool_call_contract();

            let response = loop {
                let mut req_builder = client.post(&url).json(&payload);
                req_builder = match apply_auth_to_request(
                    req_builder,
                    &auth_header,
                    credential.as_deref(),
                    &provider.name,
                ) {
                    Ok(req_builder) => req_builder,
                    Err(error) => {
                        let _ = tx
                            .send(Err(StreamError::ModelProvider(error.to_string())))
                            .await;
                        return;
                    }
                };
                req_builder = req_builder.header("Accept", "text/event-stream");
                if let Some(session) = opencode_session.as_deref() {
                    req_builder = req_builder.header(OPENCODE_SESSION_HEADER, session);
                }

                let response = match req_builder.send().await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx
                            .send(Err(StreamError::Http(super::stream_idle_error_message(
                                &e,
                                idle_timeout,
                            ))))
                            .await;
                        return;
                    }
                };
                if response.status().is_success() {
                    break response;
                }

                let status = response.status();
                let error = match response.text().await {
                    Ok(text) => text,
                    Err(_) => format!("HTTP error: {}", status),
                };
                if tools_count > 0
                    && super::rejects_tools_with_reasoning_effort(status, &error)
                    && ensure_reasoning_effort_none(&mut payload)
                {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Retry)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({
                                "provider": &provider.name,
                                "alias": &provider.alias,
                                "request_api": "chat_completions",
                                "model": &model,
                                "stream": options_enabled,
                                "tools_count": tools_count,
                                "reasoning_effort_overridden": true,
                                "reasoning_effort_fallback": "none",
                                "reasoning_effort_override_reason": "endpoint_rejected_tools_with_reasoning",
                                "status": status.as_u16(),
                            })),
                        "compatible streaming provider retrying with reasoning effort disabled after endpoint capability rejection"
                    );
                    continue;
                }

                let _ = tx.send(Err(streaming_api_error(status, &error))).await;
                return;
            };

            let mut event_stream = sse_bytes_to_events_for_contract(
                response,
                count_tokens,
                targets_mistral_tool_call_contract,
                idle_timeout,
            );
            while let Some(event) = event_stream.next().await {
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        let guard = AbortOnDrop::new(handle.abort_handle());
        stream::unfold((rx, guard), |(mut rx, guard)| async move {
            rx.recv().await.map(|event| (event, (rx, guard)))
        })
        .boxed()
    }

    fn stream_chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: Option<f64>,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        let provider = self.clone();
        // Resolved here, not in the spawned task: `spawn!` propagates the
        // tracing span but not task-locals, so the conversation scope is
        // unreadable past this point.
        let opencode_session = self.opencode_session_value();
        let system_prompt_owned: Option<String> = system_prompt.map(str::to_string);
        let message_owned = message.to_string();
        let model = model.to_string();
        let count_tokens = options.count_tokens;
        let options_enabled = options.enabled;

        // Use a channel to bridge the async HTTP response to the stream
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

        let handle = ::clawcrew_spawn::spawn!(async move {
            // Normalize image markers in the user-supplied message before
            // forwarding upstream — seefor the OpenAI-compatible
            // remote-vs-local file path problem.
            let user_msg = ChatMessage {
                role: "user".to_string(),
                content: message_owned,
            };
            let normalized_user = match provider
                .normalize_messages_for_upstream(std::slice::from_ref(&user_msg))
                .await
            {
                Ok(mut msgs) => msgs.pop().unwrap_or(user_msg),
                Err(err) => {
                    let _ = tx
                        .send(Err(StreamError::ModelProvider(err.to_string())))
                        .await;
                    return;
                }
            };
            let normalized_message_content = normalized_user.content;

            let merge = provider.effective_merge_system(&model);
            let mut messages = Vec::new();
            if merge {
                let content = match system_prompt_owned.as_deref() {
                    Some(sys) => format!("{sys}\n\n{normalized_message_content}"),
                    None => normalized_message_content,
                };
                messages.push(Message {
                    role: "user".to_string(),
                    content: Self::to_message_content("user", &content, !merge),
                });
            } else {
                if let Some(sys) = system_prompt_owned {
                    messages.push(Message {
                        role: "system".to_string(),
                        content: MessageContent::Text(sys),
                    });
                }
                messages.push(Message {
                    role: "user".to_string(),
                    content: Self::to_message_content("user", &normalized_message_content, !merge),
                });
            }

            let request = ApiChatRequest {
                model: model.clone(),
                messages,
                temperature,
                stream: Some(options_enabled),
                stream_options: options_enabled.then_some(StreamOptionsBody {
                    include_usage: true,
                }),
                reasoning_effort: provider.reasoning_effort_for_model(&model),
                tool_stream: None,
                tools: None,
                tool_choice: None,
                max_tokens: provider.max_tokens,
                extra_body: provider.extra_body.clone(),
            };
            // No cache breakpoints here, deliberately: this legacy chunk
            // stream never converts the final usage chunk into a
            // `TokenUsage`, so a premium cache write it triggered could
            // never be accounted for. The flag is honored on the structured
            // streaming path (`stream_chat`), which emits
            // `StreamEvent::Usage`.

            let url = provider.chat_completions_url();
            let client = provider.streaming_http_client();
            let idle_timeout = super::stream_idle_timeout(provider.timeout_secs);
            let auth_header = provider.auth_header.clone();
            let credential = match provider.resolve_credential().await {
                Ok(credential) => credential,
                Err(error) => {
                    let _ = tx
                        .send(Err(StreamError::ModelProvider(error.to_string())))
                        .await;
                    return;
                }
            };

            // Build request with auth
            let mut req_builder = client.post(&url).json(&request);

            // Apply auth header, or refuse locally if the credential cannot be
            // turned into one.
            req_builder = match apply_auth_to_request(
                req_builder,
                &auth_header,
                credential.as_deref(),
                &provider.name,
            ) {
                Ok(req_builder) => req_builder,
                Err(error) => {
                    let _ = tx
                        .send(Err(StreamError::ModelProvider(error.to_string())))
                        .await;
                    return;
                }
            };

            // Set accept header for streaming
            req_builder = req_builder.header("Accept", "text/event-stream");
            if let Some(session) = opencode_session.as_deref() {
                req_builder = req_builder.header(OPENCODE_SESSION_HEADER, session);
            }

            // Send request
            let response = match req_builder.send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Http(super::stream_idle_error_message(
                            &e,
                            idle_timeout,
                        ))))
                        .await;
                    return;
                }
            };

            // Check status
            if !response.status().is_success() {
                let status = response.status();
                let error = match response.text().await {
                    Ok(e) => e,
                    Err(_) => format!("HTTP error: {}", status),
                };
                let _ = tx.send(Err(streaming_api_error(status, &error))).await;
                return;
            }

            // Convert to chunk stream and forward to channel
            let mut chunk_stream = sse_bytes_to_chunks(response, count_tokens, idle_timeout);
            while let Some(chunk) = chunk_stream.next().await {
                if tx.send(chunk).await.is_err() {
                    break; // Receiver dropped
                }
            }
        });

        // Convert channel receiver to stream
        let guard = AbortOnDrop::new(handle.abort_handle());
        stream::unfold((rx, guard), |(mut rx, guard)| async move {
            rx.recv().await.map(|chunk| (chunk, (rx, guard)))
        })
        .boxed()
    }

    fn stream_chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: Option<f64>,
        options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        let provider = self.clone();
        // Resolved here, not in the spawned task: `spawn!` propagates the
        // tracing span but not task-locals, so the conversation scope is
        // unreadable past this point.
        let opencode_session = self.opencode_session_value();
        let messages_owned: Vec<ChatMessage> = messages.to_vec();
        let model = model.to_string();
        let count_tokens = options.count_tokens;
        let options_enabled = options.enabled;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamResult<StreamChunk>>(100);

        let handle = ::clawcrew_spawn::spawn!(async move {
            let normalized = match provider
                .normalize_messages_for_upstream(&messages_owned)
                .await
            {
                Ok(n) => n,
                Err(err) => {
                    let _ = tx
                        .send(Err(StreamError::ModelProvider(err.to_string())))
                        .await;
                    return;
                }
            };

            let merge = provider.effective_merge_system(&model);
            let (effective_messages, _system_merged) =
                Self::flatten_system_messages(&normalized, merge);
            let effective_messages = provider.strip_native_tool_messages(&effective_messages);
            let api_messages: Vec<Message> = effective_messages
                .iter()
                .map(|m| Message {
                    role: m.role.clone(),
                    content: provider.message_content_for_role(&m.role, &m.content, !merge, false),
                })
                .collect();

            let request = ApiChatRequest {
                model: model.clone(),
                messages: api_messages,
                temperature,
                stream: Some(options_enabled),
                stream_options: options_enabled.then_some(StreamOptionsBody {
                    include_usage: true,
                }),
                reasoning_effort: provider.reasoning_effort_for_model(&model),
                tool_stream: None,
                tools: None,
                tool_choice: None,
                max_tokens: provider.max_tokens,
                extra_body: provider.extra_body.clone(),
            };
            // No cache breakpoints here, deliberately: this legacy chunk
            // stream never converts the final usage chunk into a
            // `TokenUsage`, so a premium cache write it triggered could
            // never be accounted for. The flag is honored on the structured
            // streaming path (`stream_chat`), which emits
            // `StreamEvent::Usage`.

            let url = provider.chat_completions_url();
            let client = provider.streaming_http_client();
            let idle_timeout = super::stream_idle_timeout(provider.timeout_secs);
            let auth_header = provider.auth_header.clone();
            let credential = match provider.resolve_credential().await {
                Ok(credential) => credential,
                Err(error) => {
                    let _ = tx
                        .send(Err(StreamError::ModelProvider(error.to_string())))
                        .await;
                    return;
                }
            };

            let mut req_builder = client.post(&url).json(&request);
            req_builder = match apply_auth_to_request(
                req_builder,
                &auth_header,
                credential.as_deref(),
                &provider.name,
            ) {
                Ok(req_builder) => req_builder,
                Err(error) => {
                    let _ = tx
                        .send(Err(StreamError::ModelProvider(error.to_string())))
                        .await;
                    return;
                }
            };
            req_builder = req_builder.header("Accept", "text/event-stream");
            if let Some(session) = opencode_session.as_deref() {
                req_builder = req_builder.header(OPENCODE_SESSION_HEADER, session);
            }

            let response = match req_builder.send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Http(super::stream_idle_error_message(
                            &e,
                            idle_timeout,
                        ))))
                        .await;
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let error = match response.text().await {
                    Ok(e) => e,
                    Err(_) => format!("HTTP error: {}", status),
                };
                let _ = tx.send(Err(streaming_api_error(status, &error))).await;
                return;
            }

            let mut chunk_stream = sse_bytes_to_chunks(response, count_tokens, idle_timeout);
            while let Some(chunk) = chunk_stream.next().await {
                if tx.send(chunk).await.is_err() {
                    break;
                }
            }
        });

        let guard = AbortOnDrop::new(handle.abort_handle());
        stream::unfold((rx, guard), |(mut rx, guard)| async move {
            rx.recv().await.map(|chunk| (chunk, (rx, guard)))
        })
        .boxed()
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        // Probe the catalog without invoking inference. Unsupported endpoints are
        // still useful for connection warmup, so do not reject HTTP error statuses.
        let url = self.models_url();
        let credential = self.resolve_credential().await?;
        let mut response = self
            .apply_auth_header(self.http_client().get(&url), credential.as_deref())?
            .send()
            .await?;
        // Drain without retaining the catalog so HTTP/1 connections can be reused.
        while response.chunk().await?.is_some() {}
        Ok(())
    }
}

impl ::clawcrew_api::attribution::Attributable for OpenAiCompatibleModelProvider {
    fn role(&self) -> ::clawcrew_api::attribution::Role {
        ::clawcrew_api::attribution::Role::Provider(
            ::clawcrew_api::attribution::ProviderKind::Model(
                ::clawcrew_api::attribution::ModelProviderKind::Plugin,
            ),
        )
    }
    fn alias(&self) -> &str {
        &self.alias
    }
}

#[cfg(test)]
mod tests;

use crate::agent::dispatcher::{NativeToolDispatcher, ToolDispatcher, XmlToolDispatcher};
use crate::agent::eval::AutoClassifyExt;
use crate::agent::prompt::{
    InteractionContext, PromptContext, SystemPromptBuilder, append_timestamp_orientation,
};
use crate::approval::ApprovalManager;
use crate::observability::{self, Observer, ObserverEvent};
use crate::platform;
use crate::security::SecurityPolicy;
use crate::sop::{SopAuditLogger, SopEngine};
use crate::tools::{self, Tool};
use anyhow::{Context, Result};
use chrono::{Datelike, Timelike};
use std::path::Path;
use std::sync::Arc;
use clawcrew_config::schema::Config;
use clawcrew_memory::{self, Memory, MemoryCategory};
#[cfg(test)]
use clawcrew_providers::ChatRequest;
use clawcrew_providers::{
    self, ChatMessage, ConversationMessage, ModelProvider, ToolResultMessage,
};

// Re-export TurnEvent from clawcrew-types for backwards compatibility.
pub use clawcrew_api::agent::TurnEvent;

/// The turn engine's single per-call limits authority. Aliased from the turn
/// module so the `Agent`, its builder, and `run_tool_call_loop` all thread one
/// type (see `crate::agent::turn::ContextLimitsResolver`).
use crate::agent::loop_::ContextLimitsResolver;

/// Provider handle, its `<type>.<alias>` reference, the resolved model name,
/// and the route resolver bound to that provider — the four values a session
/// needs to swap a model provider while keeping route-aware limits correct.
type SessionModelProvider = (
    Box<dyn ModelProvider>,
    String,
    String,
    Arc<clawcrew_providers::router::ModelRouteResolver>,
);

pub fn build_session_model_provider(
    config: &Config,
    model_provider_ref: &str,
    model_override: Option<&str>,
) -> Result<SessionModelProvider> {
    let (model_provider_name, model_provider_alias) = model_provider_ref
        .split_once('.')
        .map(|(t, a)| (t.to_string(), a.to_string()))
        .ok_or_else(|| {
            anyhow::Error::msg(format!(
                "model_provider reference `{model_provider_ref}` must be `<type>.<alias>`"
            ))
        })?;

    let entry = config
        .providers
        .models
        .find(&model_provider_name, &model_provider_alias);
    let model_name = model_override
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .or_else(|| {
            entry
                .and_then(|e| e.model.as_deref())
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| {
            anyhow::Error::msg(format!(
                "model_provider `{model_provider_ref}` has no `model` configured and no model \
                 override was supplied"
            ))
        })?;

    let model_provider_runtime_options = clawcrew_providers::provider_runtime_options_for_alias(
        config,
        &model_provider_name,
        &model_provider_alias,
    );

    let (model_provider, model_route_resolver) =
        clawcrew_providers::create_routed_model_provider_with_options_and_resolver(
            config,
            model_provider_ref,
            entry.and_then(|e| e.api_key.as_deref()),
            entry.and_then(|e| e.uri.as_deref()),
            &config.reliability,
            &config.model_routes,
            &model_name,
            &model_provider_runtime_options,
        )?;

    Ok((
        model_provider,
        model_provider_ref.to_string(),
        model_name,
        model_route_resolver,
    ))
}

/// Resolve the tool dispatcher with the same provider-capability fallback
/// used by fresh agent construction.
#[must_use]
pub fn tool_dispatcher_for_provider(
    agent_cfg: &clawcrew_config::schema::AliasedAgentConfig,
    model_provider: &dyn ModelProvider,
    model: &str,
) -> Box<dyn ToolDispatcher> {
    match agent_cfg.resolved.tool_dispatcher.as_str() {
        "native" => Box::new(NativeToolDispatcher),
        "xml" => Box::new(XmlToolDispatcher),
        _ if model_provider
            .capabilities_for_model(model)
            .native_tool_calling =>
        {
            Box::new(NativeToolDispatcher)
        }
        _ => Box::new(XmlToolDispatcher),
    }
}

// Debug so a failing routing assertion can print which variant and which
// source it actually got; without it the test just says "assertion failed".
#[derive(Debug)]
pub(crate) enum RoutedApproval {
    /// Use this response. `decider` names the channel that answered, for audit
    /// attribution; `None` for a bridge-synthesized fail-closed deny.
    ///
    /// `source` says whether a human actually decided. `decider` cannot answer
    /// that on its own: it is also `None` when a single non-fan-out channel
    /// relays a real operator answer.
    Decided {
        response: clawcrew_api::channel::ChannelApprovalResponse,
        decider: Option<String>,
        source: clawcrew_api::channel::ApprovalSource,
    },
    /// Explicit `InheritOriginator` — defer to the originating-channel fan-out.
    Fallthrough,
}

pub(crate) async fn resolve_routed_approval(
    handles: &tools::PerToolChannelHandle,
    route: &clawcrew_config::autonomy::ApprovalRoute,
    recipient: &str,
    request: &clawcrew_api::channel::ChannelApprovalRequest,
) -> RoutedApproval {
    let approver: Option<(String, Arc<dyn clawcrew_api::channel::Channel>)> = handles
        .read()
        .iter()
        .find(|(name, _)| name.as_str() == route.approver_channel)
        .map(|(name, channel)| (name.clone(), Arc::clone(channel)));

    // `source` is tracked alongside `reason` so the fail-closed deny below can
    // say WHY no operator decided, rather than leaving the caller to guess from
    // a missing decider.
    let (reason, source): (&str, clawcrew_api::channel::ApprovalSource) =
        if let Some((channel_name, channel)) = approver {
            let dur = std::time::Duration::from_secs(route.timeout_secs.max(1));
            // Attributed, not legacy: if the approver channel synthesizes its own
            // `Some(Deny)` (its inner timeout firing before this outer one), that
            // is a runtime denial and must not be relabelled as the approver's
            // decision just because a response came back.
            match tokio::time::timeout(dur, channel.request_approval_attributed(recipient, request))
                .await
            {
                Ok(Ok(Some(attributed))) => {
                    return RoutedApproval::Decided {
                        response: attributed.response,
                        decider: Some(channel_name),
                        source: attributed.source,
                    };
                }
                Ok(Ok(None)) => (
                    "approver returned no decision",
                    clawcrew_api::channel::ApprovalSource::Unreachable,
                ),
                Ok(Err(_)) => (
                    "approver channel unreachable",
                    clawcrew_api::channel::ApprovalSource::Unreachable,
                ),
                Err(_) => (
                    "approver timed out",
                    clawcrew_api::channel::ApprovalSource::TimedOut,
                ),
            }
        } else {
            (
                "approver channel not registered",
                clawcrew_api::channel::ApprovalSource::Unavailable,
            )
        };

    match route.on_no_approver {
        clawcrew_config::autonomy::OnNoApprover::Deny => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "tool": request.tool_name,
                        "approver_channel": route.approver_channel,
                        "reason": reason,
                        "policy": "deny",
                    })),
                "approval route fail-closed: denying gated tool"
            );
            RoutedApproval::Decided {
                response: clawcrew_api::channel::ChannelApprovalResponse::Deny,
                decider: None,
                // The runtime denied this, not a person. Carrying the specific
                // reason lets the tool result say so instead of reporting a
                // user denial that never happened.
                source,
            }
        }
        clawcrew_config::autonomy::OnNoApprover::InheritOriginator => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "tool": request.tool_name,
                        "approver_channel": route.approver_channel,
                        "reason": reason,
                        "policy": "inherit-originator",
                    })),
                "approval route falling back to originating channel"
            );
            RoutedApproval::Fallthrough
        }
    }
}

pub(crate) struct RoutedApprovalChannel {
    handles: tools::PerToolChannelHandle,
    route: clawcrew_config::autonomy::ApprovalRoute,
}

impl RoutedApprovalChannel {
    pub(crate) fn new(
        handles: tools::PerToolChannelHandle,
        route: clawcrew_config::autonomy::ApprovalRoute,
    ) -> Self {
        Self { handles, route }
    }
}

impl ::clawcrew_api::attribution::Attributable for RoutedApprovalChannel {
    fn role(&self) -> ::clawcrew_api::attribution::Role {
        ::clawcrew_api::attribution::Role::Channel(::clawcrew_api::attribution::ChannelKind::Cli)
    }
    fn alias(&self) -> &str {
        "approval-route"
    }
}

#[async_trait::async_trait]
impl clawcrew_api::channel::Channel for RoutedApprovalChannel {
    fn name(&self) -> &str {
        "approval-route"
    }

    async fn send(&self, _message: &clawcrew_api::channel::SendMessage) -> anyhow::Result<()> {
        Ok(())
    }

    async fn listen(
        &self,
        _tx: tokio::sync::mpsc::Sender<clawcrew_api::channel::ChannelMessage>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Non-attributed entry point: delegates to
    /// [`Self::request_approval_attributed`] and drops the attribution so the
    /// routing decision lives in exactly one place.
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
        match resolve_routed_approval(&self.handles, &self.route, recipient, request).await {
            // The deciding approver's name travels on the response itself;
            // `None` for a bridge-synthesized fail-closed deny.
            //
            // Cross-crate construction: `AttributedApprovalResponse` is
            // `#[non_exhaustive]`, so struct-literal syntax is forbidden from
            // here. Build via the dedicated constructors.
            RoutedApproval::Decided {
                response,
                decider,
                source,
            } => Ok(Some(
                clawcrew_api::channel::AttributedApprovalResponse::from_runtime(response, source)
                    .with_decider_opt(decider),
            )),
            // No originating channel to inherit on this path; let the gate apply
            // the non-interactive default (auto-deny).
            RoutedApproval::Fallthrough => Ok(None),
        }
    }
}

#[derive(Debug)]
struct HistoryTrimNotice {
    dropped_messages: usize,
    kept_turns: usize,
    reason: String,
}

impl HistoryTrimNotice {
    fn into_turn_event(self) -> TurnEvent {
        TurnEvent::HistoryTrimmed {
            dropped_messages: self.dropped_messages,
            kept_turns: self.kept_turns,
            reason: self.reason,
            // Message-limit trims carry no token accounting.
            token_budget: None,
            tokens_before: None,
            tokens_after: None,
            tokens_before_source: None,
            tokens_after_source: None,
            unsatisfiable_floor: None,
        }
    }
}

async fn forward_history_trim_notice(
    event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    notice: Option<HistoryTrimNotice>,
) {
    if let Some(notice) = notice {
        let _ = event_tx.send(notice.into_turn_event()).await;
    }
}

pub struct Agent {
    model_provider: Box<dyn ModelProvider>,
    /// Sealed per-agent tool set. Stored as a [`crate::tools::scoped::ScopedToolRegistry`]
    /// so it can only be handed to the turn engine after passing through
    /// `assemble()` (the seal).
    tools: crate::tools::scoped::ScopedToolRegistry,
    memory: Arc<dyn Memory>,
    observer: Arc<dyn Observer>,
    prompt_builder: SystemPromptBuilder,
    tool_dispatcher: Box<dyn ToolDispatcher>,
    /// Stable half of the engine's memory-context injection policy
    /// (recall limit, relevance floor, budgets). Threaded into `ToolLoop`
    /// as `TurnMemory.cfg` on every turn.
    memory_inject_cfg: crate::agent::memory_inject::MemoryInjectConfig,
    config: clawcrew_config::schema::AliasedAgentConfig,
    /// Resolves the structured-history cap from canonical config at use time.
    /// Daemon-backed sessions capture the shared live config handle so reloads
    /// affect existing sessions without duplicating config-derived state.
    structured_history_cap_resolver: Option<Arc<dyn Fn() -> usize + Send + Sync>>,
    /// Resolves limits from canonical config for the provider/model route that
    /// is active when a turn starts. The route itself remains the source of truth.
    context_limits_resolver: Option<ContextLimitsResolver>,
    multimodal_config: clawcrew_config::schema::MultimodalConfig,
    model_name: String,
    model_provider_name: String,
    temperature: Option<f64>,
    workspace_dir: std::path::PathBuf,
    /// Per-agent persona workspace (`<install>/agents/<alias>/workspace/`).
    /// Holds IDENTITY.md / SOUL.md / USER.md / AGENTS.md. Distinct from
    /// `workspace_dir`, which is the security sandbox root and can be the
    /// session cwd for IDE-driven sessions (ACP, gateway WS).
    agent_workspace_dir: std::path::PathBuf,
    identity_config: clawcrew_config::schema::IdentityConfig,
    interaction_context: Option<InteractionContext>,
    skills: Vec<crate::skills::Skill>,
    skills_prompt_mode: clawcrew_config::schema::SkillsPromptInjectionMode,
    auto_save: bool,
    memory_session_id: Option<String>,
    history: Vec<ConversationMessage>,
    /// True only when `history` contains the synthetic trim breadcrumb inserted
    /// by this Agent. User text is never inferred to be synthetic by content.
    history_has_trim_breadcrumb: bool,
    history_trim_generation: u64,
    classification_config: clawcrew_config::schema::QueryClassificationConfig,
    /// The exact immutable route table used by `model_provider` for hint
    /// dispatch. It is replaced atomically with the provider on model switch.
    model_route_resolver: Arc<clawcrew_providers::router::ModelRouteResolver>,
    response_cache: Option<Arc<clawcrew_memory::response_cache::ResponseCache>>,
    /// Pre-rendered security policy summary injected into the system prompt
    /// so the LLM knows the concrete constraints before making tool calls.
    security_summary: Option<String>,
    /// Compatibility fallback for configless builders. When an
    /// `ApprovalManager` exists, its autonomy level remains canonical.
    autonomy_level: crate::security::AutonomyLevel,
    /// False for isolated / ACP sessions built with `exclude_memory: true`.
    /// Drives `PromptContext::inject_memory` so `IdentitySection` cannot pull
    /// `MEMORY.md` into the provider-visible system prompt for a session that
    /// advertises persistent-memory isolation. Set from the same builder value
    /// that installs `NoneMemory` and forces `auto_save` off.
    inject_memory: bool,
    /// The shell this agent's runtime adapter will spawn, so the system
    /// prompt reports the dialect the agent actually executes under.
    /// `None` for a shell-less runtime.
    shell_profile: Option<clawcrew_api::runtime_traits::ShellProfile>,
    /// Cross-channel HITL: resolved from the active risk profile's
    /// `approval_route`. When set, the per-turn approval bridge asks the named
    /// approver channel (bounded + fail-closed) instead of the originating
    /// fan-out. `None` ⇒ today's behavior. See EPIC B.
    approval_route: Option<clawcrew_config::autonomy::ApprovalRoute>,
    /// Activated MCP tools for deferred loading mode.
    /// When MCP deferred loading is enabled, tools are activated via `tool_search`
    /// and stored here for lookup during tool execution.
    activated_tools: Option<Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>>,
    app_registry: Option<Arc<std::sync::RwLock<crate::platform::app_registry::AppRegistry>>>,
    /// Pre-rendered MCP pinned-resource system-prompt section, read once at
    /// construction from each server's `pinned_resources` and provenance-wrapped
    /// (`trust="untrusted-external"`). Empty when no pins are configured or all
    /// were skipped. Appended to the system prompt in `build_system_prompt`.
    mcp_pinned_section: String,
    mcp_deferred_section: String,
    /// Hook runner for tool-call auditing and lifecycle side effects.
    hook_runner: Option<Arc<crate::hooks::HookRunner>>,
    /// Approval manager for direct Agent execution paths such as ACP.
    approval_manager: Option<Arc<ApprovalManager>>,
    /// Agent alias, retained for opening attribution spans at external turn
    /// call sites (ACP, gateway WS) where the alias is otherwise unavailable.
    agent_alias: String,
    channel_handles: AgentChannelHandles,
    /// Per-session cache for resolved local image data URIs, threaded into
    /// the turn loop so each unique local image file is read + base64-encoded
    /// at most once per session even though the multimodal pipeline re-walks
    /// the full conversation history on every turn and tool iteration.
    image_cache: clawcrew_providers::multimodal::LocalImageCache,
    provider_switch_config: Option<ProviderSwitchConfig>,
    /// The generation cell the context-limits resolver reads. Direct ACP/WS
    /// agents retain their construction generation until reconnect; callers
    /// with an acknowledged live-refresh transaction may republish it together
    /// with `provider_switch_config.config` through `sync_config_generation`.
    config_generation: Option<ConfigGeneration>,
    /// Channel name stamped onto observer events to identify the calling surface
    /// (e.g. "agent", "wss", "gateway"). Defaults to "agent" for direct Agent callers.
    channel_name: String,
    #[cfg(any(test, feature = "test-util"))]
    turn_datetime: Option<Arc<dyn Fn() -> chrono::DateTime<chrono::Local> + Send + Sync>>,
    /// The `DelegateTool` this Agent's registry registered, in its concrete
    /// type. Test-only: `tools` erases it behind `dyn Tool`, so a regression
    /// otherwise cannot drive the *constructed* delegate's nested-registry
    /// build and can only re-derive the wiring by hand - which is precisely
    /// what must not be trusted for live-config threading. `None` when the
    /// agent has no configured delegation targets.
    ///
    /// `allow(dead_code)`: its only reader is the delegated live-config
    /// regression, which additionally needs `plugins-wasm-cranelift` to have a
    /// plugin tool to execute at all. Under a narrower test feature set the
    /// field is written and never read.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) delegate_tool: Option<Arc<crate::tools::DelegateTool>>,
}

impl Drop for Agent {
    fn drop(&mut self) {
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_category(::clawcrew_log::EventCategory::Agent)
                .with_attrs(::serde_json::json!({
                    "model_provider": self.model_provider_name,
                    "model": self.model_name,
                    "history_messages_freed": self.history.len(),
                })),
            "Agent dropped; conversation history and per-session state freed"
        );
    }
}

#[derive(Debug)]
pub struct StreamedTurnSuccess {
    pub response: String,
    pub new_messages: Vec<ConversationMessage>,
    /// Provider profile that served the final round.
    pub provider_name: String,
    /// Model that served the final round.
    pub model: String,
    /// Capacity + proactive-trim budget for the route that actually served the
    /// final LLM call. Sourced from the loop's served-route sink so a per-call
    /// vision switch is reflected here even when the provider returned no usage;
    /// consumers publish the terminal context snapshot from this pair. `None`
    /// only when no call was served (e.g. an immediate cache hit).
    pub final_context_limits: Option<clawcrew_config::schema::ResolvedContextLimits>,
    /// Display-only accepted safeguard attribution. Callers choose the
    /// transport presentation; `new_messages` remains undecorated.
    pub safeguard_fallback: Option<clawcrew_providers::SafeguardFallbackNotice>,
}

#[derive(Debug)]
pub struct StreamedTurnError {
    pub error: anyhow::Error,
    pub committed_response: String,
    pub new_messages: Vec<ConversationMessage>,
}

/// The one config generation an agent dispatches from. Provider rebuilding
/// (`try_apply_model_switch`) and context-limit resolution
/// (`context_limits_for_route`) both read this cell, so a route, the provider
/// box serving it, and the capacity/budget reported for it can never describe
/// different generations. Callers with an acknowledged refresh transaction
/// may republish it; direct ACP/WS agents pin it until reconnect.
pub type ConfigGeneration =
    std::sync::Arc<parking_lot::RwLock<std::sync::Arc<clawcrew_config::schema::Config>>>;

#[derive(Clone, Debug, Default)]
pub struct ProviderSwitchConfig {
    pub config: Option<std::sync::Arc<clawcrew_config::schema::Config>>,
    /// Live shared config this snapshot is refreshed from when the caller owns
    /// an acknowledged model-generation refresh transaction. `None` for
    /// one-shot/test agents and direct ACP/WS agents pinned until reconnect.
    pub live: Option<std::sync::Arc<parking_lot::RwLock<clawcrew_config::schema::Config>>>,
}

/// Bundle of late-bound channel-map handles owned by an Agent. Cloning is
/// cheap (Arc clones); the underlying maps are shared with the live tools.
#[derive(Clone, Default)]
pub struct AgentChannelHandles {
    pub ask_user: Option<tools::PerToolChannelHandle>,
    pub channel_room: Option<tools::PerToolChannelHandle>,
    pub reaction: tools::PerToolChannelHandle,
    pub poll: Option<tools::PerToolChannelHandle>,
    pub escalate: Option<tools::PerToolChannelHandle>,
}

impl AgentChannelHandles {
    /// Return references to all populated per-tool channel handles.
    fn populated_handles(&self) -> Vec<Option<&tools::PerToolChannelHandle>> {
        vec![
            self.ask_user.as_ref(),
            self.channel_room.as_ref(),
            Some(&self.reaction),
            self.poll.as_ref(),
            self.escalate.as_ref(),
        ]
    }

    /// Register a channel into every populated handle so all channel-driven
    /// tools can resolve it by name.
    pub fn register_channel(
        &self,
        name: impl Into<String>,
        channel: Arc<dyn clawcrew_api::channel::Channel>,
    ) {
        let name = name.into();
        for handle in self.populated_handles().into_iter().flatten() {
            handle.write().insert(name.clone(), Arc::clone(&channel));
        }
    }

    /// Remove a channel from every populated handle (used on session/stop).
    pub fn unregister_channel(&self, name: &str) {
        for handle in self.populated_handles().into_iter().flatten() {
            handle.write().remove(name);
        }
    }

    /// Look up a registered channel by name from any populated channel map.
    pub fn get_channel(&self, name: &str) -> Option<Arc<dyn clawcrew_api::channel::Channel>> {
        for handle in self.populated_handles().into_iter().flatten() {
            if let Some(channel) = handle.read().get(name) {
                return Some(Arc::clone(channel));
            }
        }
        None
    }
}

pub struct AgentBuilder {
    model_provider: Option<Box<dyn ModelProvider>>,
    tools: Option<crate::tools::scoped::ScopedToolRegistry>,
    memory: Option<Arc<dyn Memory>>,
    observer: Option<Arc<dyn Observer>>,
    prompt_builder: Option<SystemPromptBuilder>,
    tool_dispatcher: Option<Box<dyn ToolDispatcher>>,
    memory_inject_cfg: Option<crate::agent::memory_inject::MemoryInjectConfig>,
    config: Option<clawcrew_config::schema::AliasedAgentConfig>,
    structured_history_cap_resolver: Option<Arc<dyn Fn() -> usize + Send + Sync>>,
    context_limits_resolver: Option<ContextLimitsResolver>,
    multimodal_config: Option<clawcrew_config::schema::MultimodalConfig>,
    model_name: Option<String>,
    model_provider_name: Option<String>,
    temperature: Option<f64>,
    workspace_dir: Option<std::path::PathBuf>,
    agent_workspace_dir: Option<std::path::PathBuf>,
    identity_config: Option<clawcrew_config::schema::IdentityConfig>,
    interaction_context: Option<InteractionContext>,
    skills: Option<Vec<crate::skills::Skill>>,
    skills_prompt_mode: Option<clawcrew_config::schema::SkillsPromptInjectionMode>,
    auto_save: Option<bool>,
    memory_session_id: Option<String>,
    classification_config: Option<clawcrew_config::schema::QueryClassificationConfig>,
    model_route_resolver: Option<Arc<clawcrew_providers::router::ModelRouteResolver>>,
    allowed_tools: Option<Vec<String>>,
    response_cache: Option<Arc<clawcrew_memory::response_cache::ResponseCache>>,
    security_summary: Option<String>,
    autonomy_level: Option<crate::security::AutonomyLevel>,
    shell_profile: Option<clawcrew_api::runtime_traits::ShellProfile>,
    approval_route: Option<clawcrew_config::autonomy::ApprovalRoute>,
    activated_tools: Option<Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>>,
    app_registry: Option<Arc<std::sync::RwLock<crate::platform::app_registry::AppRegistry>>>,
    mcp_pinned_section: Option<String>,
    mcp_deferred_section: Option<String>,
    hook_runner: Option<Arc<crate::hooks::HookRunner>>,
    approval_manager: Option<Arc<ApprovalManager>>,
    agent_alias: Option<String>,
    channel_name: Option<String>,
    exclude_memory: bool,
    provider_switch_config: Option<ProviderSwitchConfig>,
    config_generation: Option<ConfigGeneration>,
    #[cfg(any(test, feature = "test-util"))]
    turn_datetime: Option<Arc<dyn Fn() -> chrono::DateTime<chrono::Local> + Send + Sync>>,
    #[cfg(test)]
    delegate_tool: Option<Arc<crate::tools::DelegateTool>>,
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model_provider: None,
            tools: None,
            memory: None,
            observer: None,
            prompt_builder: None,
            tool_dispatcher: None,
            memory_inject_cfg: None,
            config: None,
            structured_history_cap_resolver: None,
            context_limits_resolver: None,
            multimodal_config: None,
            model_name: None,
            model_provider_name: None,
            temperature: None,
            workspace_dir: None,
            agent_workspace_dir: None,
            identity_config: None,
            interaction_context: None,
            skills: None,
            skills_prompt_mode: None,
            auto_save: None,
            memory_session_id: None,
            classification_config: None,
            model_route_resolver: None,
            allowed_tools: None,
            response_cache: None,
            security_summary: None,
            autonomy_level: None,
            shell_profile: None,
            approval_route: None,
            activated_tools: None,
            app_registry: None,
            mcp_pinned_section: None,
            mcp_deferred_section: None,
            hook_runner: None,
            approval_manager: None,
            agent_alias: None,
            channel_name: None,
            config_generation: None,
            exclude_memory: false,
            provider_switch_config: None,
            #[cfg(any(test, feature = "test-util"))]
            turn_datetime: None,
            #[cfg(test)]
            delegate_tool: None,
        }
    }

    pub fn model_provider(mut self, model_provider: Box<dyn ModelProvider>) -> Self {
        self.model_provider = Some(model_provider);
        self
    }

    /// Set the agent's sealed tool set. Production callers obtain the
    /// [`crate::tools::scoped::ScopedToolRegistry`] from
    /// `ScopedToolRegistry::assemble(...)` (the `.registry` field of its
    /// output); raw `Vec<Box<dyn Tool>>` fixtures use
    /// `ScopedToolRegistry::from_raw_for_test`, available to this crate's unit
    /// tests and, via the dev-only `test-util` feature, to other crates' test
    /// builds. [`Self::build`] applies the `allowed_tools` and ACP
    /// memory-strip filters on top of the sealed set.
    pub fn tools(mut self, tools: crate::tools::scoped::ScopedToolRegistry) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn memory(mut self, memory: Arc<dyn Memory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn observer(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn prompt_builder(mut self, prompt_builder: SystemPromptBuilder) -> Self {
        self.prompt_builder = Some(prompt_builder);
        self
    }

    pub fn tool_dispatcher(mut self, tool_dispatcher: Box<dyn ToolDispatcher>) -> Self {
        self.tool_dispatcher = Some(tool_dispatcher);
        self
    }

    /// Stable half of the engine's memory-context injection policy. When
    /// unset, defaults preserve the legacy loader shape (recall limit 5,
    /// the schema-default relevance floor).
    pub fn memory_inject_cfg(
        mut self,
        cfg: crate::agent::memory_inject::MemoryInjectConfig,
    ) -> Self {
        self.memory_inject_cfg = Some(cfg);
        self
    }

    pub fn config(mut self, config: clawcrew_config::schema::AliasedAgentConfig) -> Self {
        self.config = Some(config);
        self
    }

    fn structured_history_cap_resolver(
        mut self,
        resolver: Arc<dyn Fn() -> usize + Send + Sync>,
    ) -> Self {
        self.structured_history_cap_resolver = Some(resolver);
        self
    }

    fn context_limits_resolver(mut self, resolver: ContextLimitsResolver) -> Self {
        self.context_limits_resolver = Some(resolver);
        self
    }

    #[cfg(test)]
    fn structured_max_history_messages(self, max: usize) -> Self {
        self.structured_history_cap_resolver(Arc::new(move || max))
    }

    pub fn multimodal_config(
        mut self,
        multimodal_config: clawcrew_config::schema::MultimodalConfig,
    ) -> Self {
        self.multimodal_config = Some(multimodal_config);
        self
    }

    pub fn model_name(mut self, model_name: String) -> Self {
        self.model_name = Some(model_name);
        self
    }

    pub fn model_provider_name(mut self, name: String) -> Self {
        self.model_provider_name = Some(name);
        self
    }

    pub fn temperature(mut self, temperature: Option<f64>) -> Self {
        self.temperature = temperature;
        self
    }

    pub fn workspace_dir(mut self, workspace_dir: std::path::PathBuf) -> Self {
        self.workspace_dir = Some(workspace_dir);
        self
    }

    pub fn agent_workspace_dir(mut self, agent_workspace_dir: std::path::PathBuf) -> Self {
        self.agent_workspace_dir = Some(agent_workspace_dir);
        self
    }

    pub fn identity_config(
        mut self,
        identity_config: clawcrew_config::schema::IdentityConfig,
    ) -> Self {
        self.identity_config = Some(identity_config);
        self
    }

    pub fn interaction_context(mut self, interaction: Option<InteractionContext>) -> Self {
        self.interaction_context = interaction;
        self
    }

    pub fn skills(mut self, skills: Vec<crate::skills::Skill>) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn skills_prompt_mode(
        mut self,
        skills_prompt_mode: clawcrew_config::schema::SkillsPromptInjectionMode,
    ) -> Self {
        self.skills_prompt_mode = Some(skills_prompt_mode);
        self
    }

    pub fn auto_save(mut self, auto_save: bool) -> Self {
        self.auto_save = Some(auto_save);
        self
    }

    pub fn memory_session_id(mut self, memory_session_id: Option<String>) -> Self {
        self.memory_session_id = memory_session_id;
        self
    }

    pub fn classification_config(
        mut self,
        classification_config: clawcrew_config::schema::QueryClassificationConfig,
    ) -> Self {
        self.classification_config = Some(classification_config);
        self
    }

    pub fn model_route_resolver(
        mut self,
        model_route_resolver: Arc<clawcrew_providers::router::ModelRouteResolver>,
    ) -> Self {
        self.model_route_resolver = Some(model_route_resolver);
        self
    }

    pub fn allowed_tools(mut self, allowed_tools: Option<Vec<String>>) -> Self {
        self.allowed_tools = allowed_tools;
        self
    }

    pub fn response_cache(
        mut self,
        cache: Option<Arc<clawcrew_memory::response_cache::ResponseCache>>,
    ) -> Self {
        self.response_cache = cache;
        self
    }

    pub fn security_summary(mut self, summary: Option<String>) -> Self {
        self.security_summary = summary;
        self
    }

    /// Set the prompt autonomy fallback used only when no `ApprovalManager`
    /// is attached. Retained for compatibility with existing builder users.
    pub fn autonomy_level(mut self, level: crate::security::AutonomyLevel) -> Self {
        self.autonomy_level = Some(level);
        self
    }

    /// Set the shell reported in the system prompt.
    ///
    /// Pass `RuntimeAdapter::shell_profile()` from the same adapter the
    /// agent's tools were built with, so the prompt cannot name a shell other
    /// than the one that will execute. Unset means no shell is reported.
    pub fn shell_profile(
        mut self,
        profile: Option<clawcrew_api::runtime_traits::ShellProfile>,
    ) -> Self {
        self.shell_profile = profile;
        self
    }

    pub fn approval_route(
        mut self,
        route: Option<clawcrew_config::autonomy::ApprovalRoute>,
    ) -> Self {
        self.approval_route = route;
        self
    }

    pub fn activated_tools(
        mut self,
        activated: Option<Arc<std::sync::Mutex<tools::ActivatedToolSet>>>,
    ) -> Self {
        self.activated_tools = activated;
        self
    }

    pub fn app_registry(
        mut self,
        app_registry: Option<Arc<std::sync::RwLock<crate::platform::app_registry::AppRegistry>>>,
    ) -> Self {
        self.app_registry = app_registry;
        self
    }

    pub fn mcp_pinned_section(mut self, section: Option<String>) -> Self {
        self.mcp_pinned_section = section;
        self
    }

    pub fn mcp_deferred_section(mut self, section: Option<String>) -> Self {
        self.mcp_deferred_section = section;
        self
    }

    pub fn hook_runner(mut self, runner: Option<Arc<crate::hooks::HookRunner>>) -> Self {
        self.hook_runner = runner;
        self
    }

    pub fn approval_manager(mut self, manager: Option<Arc<ApprovalManager>>) -> Self {
        self.approval_manager = manager;
        self
    }

    /// Set the agent alias used for turn-span attribution.
    pub fn agent_alias(mut self, alias: String) -> Self {
        self.agent_alias = Some(alias);
        self
    }

    pub fn channel_name(mut self, name: String) -> Self {
        self.channel_name = Some(name);
        self
    }

    /// Retain the concrete `DelegateTool` the registry built, for regressions
    /// that must drive the *constructed* delegate rather than a hand-rolled one.
    #[cfg(test)]
    fn delegate_tool(mut self, delegate_tool: Option<Arc<crate::tools::DelegateTool>>) -> Self {
        self.delegate_tool = delegate_tool;
        self
    }

    #[cfg(test)]
    fn turn_datetime<F>(mut self, provider: F) -> Self
    where
        F: Fn() -> chrono::DateTime<chrono::Local> + Send + Sync + 'static,
    {
        self.turn_datetime = Some(Arc::new(provider));
        self
    }

    pub fn exclude_memory(mut self, exclude: bool) -> Self {
        self.exclude_memory = exclude;
        self
    }

    pub fn provider_switch_config(mut self, cfg: ProviderSwitchConfig) -> Self {
        self.provider_switch_config = Some(cfg);
        self
    }

    /// Install the generation cell that provider rebuilding and context-limit
    /// resolution both read. Callers that supply one MUST derive the agent's
    /// `context_limits_resolver` from the SAME cell (see
    /// `Agent::context_generation_limits_resolver`); otherwise limits can be
    /// resolved from a config generation the provider box was not built from.
    pub fn config_generation(mut self, generation: ConfigGeneration) -> Self {
        self.config_generation = Some(generation);
        self
    }

    pub fn build(self) -> Result<Agent> {
        let mut tools = self.tools.ok_or_else(|| {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"missing_field": "tools"})),
                "AgentBuilder::build missing required field"
            );
            anyhow::Error::msg("tools are required")
        })?;
        let allowed = self.allowed_tools.clone();
        if let Some(ref allow_list) = allowed {
            tools.retain(|t| allow_list.iter().any(|name| name == t.name()));
        }

        // ACP sessions exclude persistent memory: strip memory tools,
        // replace the backend with NoneMemory, and force auto_save off.
        let exclude_memory = self.exclude_memory;
        if exclude_memory {
            tools.retain(|t| !clawcrew_tools::MEMORY_TOOL_NAMES.contains(&t.name()));
        }

        let memory: Arc<dyn Memory> = if exclude_memory {
            Arc::new(clawcrew_memory::NoneMemory::new("none"))
        } else {
            self.memory.ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"missing_field": "memory"})),
                    "AgentBuilder::build missing required field"
                );
                anyhow::Error::msg("memory is required")
            })?
        };
        let config = self.config.unwrap_or_default();
        let model_name = self.model_name.unwrap_or_else(|| "<unconfigured>".into());
        let model_provider_name = self
            .model_provider_name
            .unwrap_or_else(|| "<unconfigured>".into());
        let model_route_resolver = self.model_route_resolver.unwrap_or_else(|| {
            Arc::new(clawcrew_providers::router::ModelRouteResolver::new(
                Vec::new(),
                model_provider_name.clone(),
                model_name.clone(),
            ))
        });

        Ok(Agent {
            model_provider: self.model_provider.ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"missing_field": "model_provider"})),
                    "AgentBuilder::build missing required field"
                );
                anyhow::Error::msg("model_provider is required")
            })?,
            tools,
            memory: memory.clone(),
            observer: self.observer.ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"missing_field": "observer"})),
                    "AgentBuilder::build missing required field"
                );
                anyhow::Error::msg("observer is required")
            })?,
            prompt_builder: self
                .prompt_builder
                .unwrap_or_else(SystemPromptBuilder::with_defaults),
            tool_dispatcher: self.tool_dispatcher.ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"missing_field": "tool_dispatcher"})),
                    "AgentBuilder::build missing required field"
                );
                anyhow::Error::msg("tool_dispatcher is required")
            })?,
            memory_inject_cfg: self.memory_inject_cfg.unwrap_or_else(|| {
                crate::agent::memory_inject::MemoryInjectConfig::from_memory_config(
                    &clawcrew_config::schema::MemoryConfig::default(),
                    crate::agent::memory_inject::DEFAULT_RECALL_LIMIT,
                )
            }),
            config,
            structured_history_cap_resolver: self.structured_history_cap_resolver,
            context_limits_resolver: self.context_limits_resolver,
            multimodal_config: self.multimodal_config.unwrap_or_default(),
            model_name,
            model_provider_name,
            temperature: self.temperature,
            // Default for test callers that don't call workspace_dir().
            workspace_dir: self
                .workspace_dir
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from(".")),
            agent_workspace_dir: self.agent_workspace_dir.unwrap_or_else(|| {
                self.workspace_dir
                    .clone()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
            }),
            identity_config: self.identity_config.unwrap_or_default(),
            interaction_context: self.interaction_context,
            skills: self.skills.unwrap_or_default(),
            skills_prompt_mode: self.skills_prompt_mode.unwrap_or_default(),
            auto_save: if exclude_memory {
                false
            } else {
                self.auto_save.unwrap_or(false)
            },
            memory_session_id: self.memory_session_id,
            history: Vec::new(),
            history_has_trim_breadcrumb: false,
            history_trim_generation: 0,
            classification_config: self.classification_config.unwrap_or_default(),
            model_route_resolver,
            response_cache: self.response_cache,
            security_summary: self.security_summary,
            approval_route: self.approval_route,
            autonomy_level: self
                .autonomy_level
                .unwrap_or(crate::security::AutonomyLevel::Supervised),
            // One policy, one source: the same `exclude_memory` that strips
            // memory tools and installs `NoneMemory` also withholds MEMORY.md
            // from the prompt.
            inject_memory: !exclude_memory,
            shell_profile: self.shell_profile,
            activated_tools: self.activated_tools,
            app_registry: self.app_registry,
            mcp_pinned_section: self.mcp_pinned_section.unwrap_or_default(),
            mcp_deferred_section: self.mcp_deferred_section.unwrap_or_default(),
            hook_runner: self.hook_runner,
            approval_manager: self.approval_manager,
            agent_alias: self.agent_alias.unwrap_or_default(),
            channel_handles: AgentChannelHandles::default(),
            image_cache: clawcrew_providers::multimodal::LocalImageCache::new(),
            config_generation: self.config_generation,
            provider_switch_config: self.provider_switch_config,
            channel_name: self.channel_name.unwrap_or_else(|| "agent".to_string()),
            #[cfg(any(test, feature = "test-util"))]
            turn_datetime: self.turn_datetime,
            #[cfg(test)]
            delegate_tool: self.delegate_tool,
        })
    }
}

/// Identifies the single message in a replayed buffer that actually
/// received the provider-only recalled-memory preamble. The injector
/// targets one message (the last user message at injection time); every
/// other buffer a turn replays is either a pre-injection clone that never
/// contained the preamble or a slice positioned after it. Callers pass the
/// target only for the mutated history buffer, at the index the injected
/// message holds within the exact slice being replayed — never for
/// uninjected clones, which must replay byte-for-byte.
struct MemoryPreambleTarget<'a> {
    preamble: &'a str,
    index: usize,
}

impl Agent {
    pub fn builder() -> AgentBuilder {
        AgentBuilder::new()
    }

    /// Install a deterministic clock for downstream test fixtures.
    ///
    /// This method is available only to the crate's own tests or when the
    /// dev-only `test-util` feature is enabled. Production builds always use
    /// the live local clock.
    #[cfg(any(test, feature = "test-util"))]
    pub fn set_turn_datetime_for_test<F>(&mut self, provider: F)
    where
        F: Fn() -> chrono::DateTime<chrono::Local> + Send + Sync + 'static,
    {
        self.turn_datetime = Some(Arc::new(provider));
    }

    /// The full `Config` the agent was constructed from, when available. Sourced
    /// from `provider_switch_config` - the single canonical config snapshot the
    /// agent already carries for provider-alias resolution. `None` only on
    /// configless (test-builder) agents; every production construction path
    /// (`from_config` / `from_config_with_tui_env`) populates it. Used by the
    /// vision route to resolve the configured `vision_model_provider`'s
    /// alias-specific options (the `vision` override, endpoint URI, credentials).
    fn full_config(&self) -> Option<&clawcrew_config::schema::Config> {
        self.provider_switch_config
            .as_ref()
            .and_then(|cfg| cfg.config.as_deref())
    }

    fn tool_loop_cost_tracking_context(&self) -> crate::agent::loop_::ToolLoopCostTrackingContext {
        if let Ok(Some(context)) =
            crate::agent::loop_::TOOL_LOOP_COST_TRACKING_CONTEXT.try_with(Clone::clone)
        {
            return context;
        }

        crate::agent::loop_::ToolLoopCostTrackingContext::usage_only()
    }

    fn current_turn_datetime(&self) -> chrono::DateTime<chrono::Local> {
        #[cfg(any(test, feature = "test-util"))]
        if let Some(provider) = &self.turn_datetime {
            return provider();
        }

        chrono::Local::now()
    }

    /// Prefixes a user message with the current date/time in the labeled
    /// shape both embedded Agent turn paths store in history, including the
    /// streamed path used by RPC and ACP. Other runtime owners format their
    /// own user-message envelopes independently.
    fn enrich_user_message(&self, user_message: &str) -> String {
        let now = self.current_turn_datetime();
        let (year, month, day) = (now.year(), now.month(), now.day());
        let (hour, minute, second) = (now.hour(), now.minute(), now.second());
        let tz = now.format("%Z");
        let date_str =
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} {tz}");

        format!("[CURRENT DATE & TIME: {date_str}]\n\n{user_message}")
    }

    pub fn set_channel_name(&mut self, name: String) {
        self.channel_name = name;
    }

    /// Set the host-resolved, descriptive interaction context for this live
    /// session. This does not alter tools, policy, routing, memory, or storage.
    pub fn set_interaction_context(&mut self, interaction: Option<InteractionContext>) {
        self.interaction_context = interaction;
        self.refresh_system_prompt();
    }

    fn new_turn_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn observer_agent_alias(&self) -> Option<String> {
        if self.agent_alias.is_empty() {
            None
        } else {
            Some(self.agent_alias.clone())
        }
    }

    pub fn history(&self) -> &[ConversationMessage] {
        &self.history
    }

    /// Remove the trailing assistant interruption marker
    /// (`turn-interrupted-by-user`) the tool loop appends to live history when a
    /// turn is cancelled, returning whether one was removed. When cancellation
    /// folds the marker into a partial assistant response, preserve the partial
    /// response and remove only the runtime-owned marker suffix.
    ///
    /// External surfaces that project cancellation differently on their durable
    /// transcript (the ACP channel records a structured, replay-only cancellation
    /// event instead) call this so the generic marker is not re-sent to the
    /// provider on the next turn of the same still-active session. The marker
    /// string stays owned by the runtime here rather than being re-derived and
    /// content-matched at the call site.
    pub fn strip_trailing_interruption_marker(&mut self) -> bool {
        let marker = crate::i18n::get_required_cli_string("turn-interrupted-by-user");
        let is_marker = matches!(
            self.history.last(),
            Some(ConversationMessage::Chat(message))
                if message.role == "assistant" && message.content == marker
        );
        if is_marker {
            self.history.pop();
            return true;
        }

        let folded_suffix = format!("\n\n{marker}");
        let Some(ConversationMessage::Chat(message)) = self.history.last_mut() else {
            return false;
        };
        if message.role != "assistant" || !message.content.ends_with(&folded_suffix) {
            return false;
        }

        message
            .content
            .truncate(message.content.len() - folded_suffix.len());
        true
    }

    /// Degrade image references in the trailing turn of live history.
    ///
    /// A turn that just ended in a non-retryable failure is the newest whole
    /// turn: its opening user prompt is the last turn-opening user message and
    /// nothing was appended after the failure. Prompt-mode `[Tool results]`
    /// carriers inside the turn are not turn openings, so the span reaches
    /// back past them to the prompt that actually opened the turn. Any
    /// attachment in that span was already rejected (or already defeated the
    /// request), so leaving it in place resends it on the next prompt of the
    /// same still-active session and reproduces the failure. The span is
    /// projected in place with the shared failed-turn media degradation (see
    /// `media_degrade::degrade_media_in_messages`); the durable transcript
    /// keeps the original content for client replay.
    ///
    /// Returns the number of image references degraded.
    pub fn degrade_trailing_turn_media(&mut self) -> usize {
        let Some(start) = self
            .history
            .iter()
            .rposition(crate::agent::turn::media_degrade::is_turn_opening_user_message)
        else {
            return 0;
        };
        crate::agent::turn::media_degrade::degrade_media_in_messages(&mut self.history[start..])
    }

    pub fn channel_handles(&self) -> &AgentChannelHandles {
        &self.channel_handles
    }

    pub fn populate_channels(
        &self,
        channel_map: &std::collections::HashMap<String, Arc<dyn clawcrew_api::channel::Channel>>,
    ) -> Vec<String> {
        let mut names = Vec::new();
        for (name, ch) in channel_map {
            self.channel_handles.register_channel(name, Arc::clone(ch));
            names.push(name.clone());
        }
        names
    }

    /// Attribution fields for opening a turn span at external call sites
    /// (ACP, gateway WS) so every record inside a streamed turn carries the
    /// same `agent_alias`/`model_provider`/`model` the RPC dispatch path sets.
    /// Returns `(agent_alias, model_provider, model)`.
    pub fn attribution_fields(&self) -> (String, String, String) {
        (
            self.agent_alias.clone(),
            self.model_provider_name.clone(),
            self.model_name.clone(),
        )
    }

    /// Capacity and proactive-trim budget for the currently selected route.
    /// This is resolved on demand so a model/provider switch cannot leave a
    /// stale snapshot in a long-lived session.
    pub fn context_limits(&self) -> clawcrew_config::schema::ResolvedContextLimits {
        self.context_limits_for_route(&self.model_provider_name, &self.model_name)
    }

    /// Build a limits resolver bound to a config generation CELL. Kept next to
    /// `sync_config_generation` because the two form the single-generation
    /// contract: this resolver reports capacity/budget from whatever generation
    /// that function last published, which is the same generation
    /// `try_apply_model_switch` rebuilds the provider and route resolver from.
    pub fn context_generation_limits_resolver(
        generation: ConfigGeneration,
        agent_alias: String,
    ) -> impl Fn(&str, &str) -> clawcrew_config::schema::ResolvedContextLimits + Send + Sync + 'static
    {
        move |provider_ref, model| {
            let config = Arc::clone(&generation.read());
            config.resolved_context_limits_for_route(&agent_alias, provider_ref, model)
        }
    }

    /// Republish the agent's config generation from its acknowledged live model
    /// config source, when one exists.
    ///
    /// Called at a turn boundary, before any route is resolved. Provider
    /// rebuilding (`try_apply_model_switch`, via `provider_switch_config`) and
    /// limit resolution (`context_limits_for_route`, via `config_generation`)
    /// then read one identical `Arc<Config>`, so dispatch, route identity, and
    /// reported limits cannot straddle a mid-session `config/set`. Within a turn
    /// the generation is stable. Direct ACP/WS agents intentionally have no
    /// live model source here: they remain wholly on their construction
    /// generation until reconnect instead of partially adopting a reload.
    pub fn sync_config_generation(&mut self) {
        let Some(live) = self
            .provider_switch_config
            .as_ref()
            .and_then(|cfg| cfg.live.as_ref())
            .map(Arc::clone)
        else {
            return;
        };
        let latest = Arc::new(live.read().clone());
        if let Some(generation) = self.config_generation.as_ref() {
            *generation.write() = Arc::clone(&latest);
        }
        if let Some(switch_config) = self.provider_switch_config.as_mut() {
            switch_config.config = Some(latest);
        }
    }

    /// Resolve capacity and proactive budget for a route selected for the
    /// current turn, including values supplied by a live-config resolver.
    pub fn context_limits_for_route(
        &self,
        provider_name: &str,
        model: &str,
    ) -> clawcrew_config::schema::ResolvedContextLimits {
        self.context_limits_resolver.as_ref().map_or_else(
            || self.config.resolved.context_limits(),
            |resolve| resolve(provider_name, model),
        )
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.history_has_trim_breadcrumb = false;
    }

    pub fn set_history_has_trim_breadcrumb(&mut self, flag: bool) {
        self.history_has_trim_breadcrumb = flag;
    }

    pub fn history_has_trim_breadcrumb(&self) -> bool {
        self.history_has_trim_breadcrumb
    }

    pub fn history_trim_generation(&self) -> u64 {
        self.history_trim_generation
    }

    fn encode_response_cache_transcript(messages: &[ChatMessage]) -> String {
        let mut transcript = String::new();
        for message in messages {
            transcript.push_str("role=");
            transcript.push_str(&message.role.len().to_string());
            transcript.push(':');
            transcript.push_str(&message.role);
            transcript.push_str(";content=");
            transcript.push_str(&message.content.len().to_string());
            transcript.push(':');
            transcript.push_str(&message.content);
            transcript.push('\n');
        }
        transcript
    }

    fn memory_injection_active(&self) -> bool {
        if self.memory.name() == "none" {
            return false;
        }
        matches!(
            crate::agent::memory_inject::resolve_inject_policy(
                clawcrew_api::ingress::TurnOrigin::AgentDirect,
                self.memory_session_id.is_some(),
                false,
            ),
            crate::agent::memory_inject::InjectPolicy::Inject { .. }
        )
    }

    fn response_cache_key_for_messages(
        &self,
        messages: &[ChatMessage],
        effective_model: &str,
    ) -> Option<String> {
        // Bypass the cache when a per-turn memory preamble the key cannot see
        // will be injected downstream (see `memory_injection_active`), or when
        // hooks/tools can change the final provider request after this point.
        if self.temperature != Some(0.0)
            || self.response_cache.is_none()
            || self.memory_injection_active()
            || clawcrew_api::NATIVE_THINKING_OVERRIDE
                .try_with(Option::is_some)
                .unwrap_or(false)
            || self
                .hook_runner
                .as_ref()
                .is_some_and(|runner| !runner.is_empty())
            || !self.tools.is_empty()
            || self.activated_tools.is_some()
            || !self
                .model_provider
                .has_stable_request_identity(effective_model)
        {
            return None;
        }

        if messages
            .iter()
            .filter(|message| message.role != "system")
            .any(|message| message.content.contains("[IMAGE:"))
        {
            return None;
        }

        let transcript = Self::encode_response_cache_transcript(messages);
        let provider_model_identity = format!(
            "provider={}:{};alias={}:{};model={}:{}",
            self.model_provider_name.len(),
            self.model_provider_name,
            self.model_provider.alias().len(),
            self.model_provider.alias(),
            effective_model.len(),
            effective_model,
        );

        Some(clawcrew_memory::response_cache::ResponseCache::cache_key(
            &provider_model_identity,
            None,
            &transcript,
        ))
    }

    async fn append_streamed_user_message_to_history(
        &mut self,
        user_message: &str,
        new_msgs: &mut Vec<ConversationMessage>,
        turn_id: &str,
    ) {
        // Memory context is injected once in the engine, keyed on the
        // ingress origin (agent::memory_inject).
        if self.auto_save {
            let store_start = std::time::Instant::now();
            let store_result = self
                .memory
                .store(
                    "user_msg",
                    user_message,
                    MemoryCategory::Conversation,
                    self.memory_session_id.as_deref(),
                )
                .await;
            self.observer.record_event(&ObserverEvent::MemoryStore {
                category: MemoryCategory::Conversation.to_string(),
                backend: self.memory.name().to_string(),
                duration: store_start.elapsed(),
                success: store_result.is_ok(),
                channel: Some(self.channel_name.clone()),
                agent_alias: self.observer_agent_alias(),
                turn_id: Some(turn_id.to_string()),
            });
        }

        let enriched = self.enrich_user_message(user_message);

        let user_msg = ConversationMessage::Chat(ChatMessage::user(enriched));
        new_msgs.push(user_msg.clone());
        self.history.push(user_msg);
    }

    pub fn set_memory_session_id(&mut self, session_id: Option<String>) {
        self.memory_session_id = session_id;
    }

    pub fn set_temperature(&mut self, temperature: Option<f64>) {
        self.temperature = temperature;
    }

    pub fn refresh_memory_embedder(
        &self,
        model_provider: &str,
        api_key: Option<&str>,
        model: &str,
        dimensions: usize,
    ) {
        self.memory
            .refresh_embedder(model_provider, api_key, model, dimensions);
    }

    #[cfg(test)]
    pub fn temperature_for_test(&self) -> Option<f64> {
        self.temperature
    }

    pub fn set_model_name(&mut self, model_name: String) {
        self.model_name = model_name;
    }

    pub fn set_model_provider(&mut self, model_provider: Box<dyn ModelProvider>) {
        self.model_provider = model_provider;
    }

    pub fn set_model_provider_name(&mut self, model_provider_name: String) {
        self.model_provider_name = model_provider_name;
    }

    /// Install the route resolver that belongs to a newly swapped provider.
    /// The resolver holds the hint→provider/model route table bound to a
    /// specific provider set, so it MUST be replaced together with the provider
    /// box (see `set_model_provider`); otherwise a routed hint resolves through
    /// the previous provider's table while the new provider serves the call.
    pub fn set_model_route_resolver(
        &mut self,
        model_route_resolver: Arc<clawcrew_providers::router::ModelRouteResolver>,
    ) {
        self.model_route_resolver = model_route_resolver;
    }

    /// Publish the config generation a freshly swapped provider box was built
    /// from. MUST be called in the same state transition as
    /// `set_model_provider` / `set_model_route_resolver` (see
    /// `SessionStore::apply_model_provider`): it moves the provider-rebuild
    /// snapshot and the limits generation together, so a later explicit
    /// `model_switch` cannot rebuild dispatch from stale profiles and routes
    /// while `context_limits_for_route` reports the new config's capacity.
    pub fn set_config_generation(
        &mut self,
        config_generation: Arc<clawcrew_config::schema::Config>,
    ) {
        if let Some(generation) = self.config_generation.as_ref() {
            *generation.write() = Arc::clone(&config_generation);
        }
        match self.provider_switch_config.as_mut() {
            Some(switch_config) => {
                switch_config.config = Some(config_generation);
            }
            None => {
                self.provider_switch_config = Some(ProviderSwitchConfig {
                    config: Some(config_generation),
                    live: None,
                });
            }
        }
    }

    /// Resolve a selector through the agent's CURRENT route resolver. Test-only
    /// accessor so cross-module tests (e.g. RPC session refresh) can assert the
    /// resolver was replaced together with the provider box.
    #[cfg(test)]
    pub(crate) fn resolved_route_for_test(
        &self,
        selector: &str,
    ) -> clawcrew_providers::router::ResolvedModelRoute {
        self.model_route_resolver.resolve(selector)
    }

    pub fn set_tool_dispatcher(&mut self, tool_dispatcher: Box<dyn ToolDispatcher>) {
        self.tool_dispatcher = tool_dispatcher;
        self.refresh_system_prompt();
    }

    fn refresh_system_prompt(&mut self) {
        let Some(ConversationMessage::Chat(first)) = self.history.first() else {
            return;
        };
        if first.role != "system" {
            return;
        }
        if let Ok(sys) = self.build_system_prompt() {
            self.history[0] = ConversationMessage::Chat(ChatMessage::system(sys));
        }
    }

    #[cfg(test)]
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    #[cfg(test)]
    pub fn system_prompt_for_test(&self) -> Result<String> {
        self.build_system_prompt()
    }

    #[cfg(test)]
    pub async fn execute_tool_for_test(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Option<anyhow::Result<clawcrew_api::tool::ToolResult>> {
        let tool = crate::agent::tool_execution::find_tool(&self.tools, name)?;
        Some(tool.execute(args).await)
    }

    pub fn seed_history(&mut self, messages: &[ChatMessage]) {
        let _ = self.seed_history_with_event(messages);
    }

    /// Hydrate prior chat messages and return a transport event when restoring
    /// the history enforces the structured message cap.
    pub fn seed_history_with_event(&mut self, messages: &[ChatMessage]) -> Option<TurnEvent> {
        if self.history.is_empty()
            && let Ok(sys) = self.build_system_prompt()
        {
            self.history
                .push(ConversationMessage::Chat(ChatMessage::system(sys)));
        }
        for msg in messages {
            if msg.role != "system" {
                self.history.push(ConversationMessage::Chat(msg.clone()));
            }
        }
        self.trim_history(None)
            .map(HistoryTrimNotice::into_turn_event)
    }

    /// Hydrate the agent with a full `ConversationMessage` history (e.g. restored
    /// from an ACP session store). Preserves all variants including `AssistantToolCalls`
    /// and `ToolResults` — use this for ACP restore; use `seed_history` for flat
    /// channel session hydration.
    pub fn seed_conversation_history(&mut self, messages: Vec<ConversationMessage>) {
        let _ = self.seed_conversation_history_with_event(messages);
    }

    /// Hydrate structured conversation history and return a transport event
    /// when restoring the history enforces the structured message cap.
    pub fn seed_conversation_history_with_event(
        &mut self,
        messages: Vec<ConversationMessage>,
    ) -> Option<TurnEvent> {
        if self.history.is_empty()
            && let Ok(sys) = self.build_system_prompt()
        {
            self.history
                .push(ConversationMessage::Chat(ChatMessage::system(sys)));
        }
        for msg in messages {
            // Skip system messages from the seed — the system prompt is already prepended above.
            if matches!(&msg, ConversationMessage::Chat(m) if m.role == "system") {
                continue;
            }
            self.history.push(msg);
        }
        // Trim immediately so pre_len snapshots (taken before the first turn)
        // are always within the configured limit; otherwise a long restored
        // history would cause history[pre_len..] to panic after trim_history
        // shrinks the vec below pre_len during the turn.
        self.trim_history(None)
            .map(HistoryTrimNotice::into_turn_event)
    }

    pub async fn from_config(config: &Config, agent_alias: &str) -> Result<Self> {
        Self::from_config_with_session_cwd(config, agent_alias, None).await
    }

    pub async fn from_config_with_session_cwd(
        config: &Config,
        agent_alias: &str,
        session_cwd: Option<&Path>,
    ) -> Result<Self> {
        Self::from_config_with_session_cwd_and_mcp(config, agent_alias, session_cwd, true).await
    }

    pub async fn from_config_with_session_cwd_and_mcp(
        config: &Config,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
    ) -> Result<Self> {
        Self::from_config_with_session_cwd_and_mcp_approval_mode(
            config,
            agent_alias,
            session_cwd,
            initialize_mcp,
            false,
            false,
            // Non-ACP construction path: `deliver_file` has no transport here.
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    pub async fn from_config_with_session_cwd_and_mcp_backchannel(
        config: &Config,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        exclude_memory: bool,
        acp_delivery: bool,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
        canvas_store: Option<tools::CanvasStore>,
    ) -> Result<Self> {
        Self::from_config_with_session_cwd_and_mcp_approval_mode(
            config,
            agent_alias,
            session_cwd,
            initialize_mcp,
            true,
            exclude_memory,
            acp_delivery,
            None,
            sop_engine,
            sop_audit,
            canvas_store,
            None,
            None,
            None,
        )
        .await
    }

    pub async fn from_config_with_session_cwd_and_mcp_backchannel_and_acp_sessions(
        config: &Config,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        exclude_memory: bool,
        acp_delivery: bool,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
        canvas_store: Option<tools::CanvasStore>,
        acp_session_store: Arc<clawcrew_infra::acp_session_store::AcpSessionStore>,
    ) -> Result<Self> {
        Self::from_config_with_session_cwd_and_mcp_approval_mode(
            config,
            agent_alias,
            session_cwd,
            initialize_mcp,
            true,
            exclude_memory,
            acp_delivery,
            None,
            sop_engine,
            sop_audit,
            canvas_store,
            Some(acp_session_store),
            None,
            None,
        )
        .await
    }

    /// Build a daemon-backed ACP/WS Agent whose model route generation is pinned
    /// until reconnect while independently live tool/history policy continues
    /// to follow the shared config.
    pub async fn from_pinned_live_config_with_session_cwd_and_mcp_backchannel(
        live_config: Arc<parking_lot::RwLock<Config>>,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        exclude_memory: bool,
        acp_delivery: bool,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
        canvas_store: Option<tools::CanvasStore>,
    ) -> Result<Self> {
        let config = live_config.read().clone();
        Self::from_config_with_session_cwd_and_mcp_approval_mode(
            &config,
            agent_alias,
            session_cwd,
            initialize_mcp,
            true,
            exclude_memory,
            acp_delivery,
            None,
            sop_engine,
            sop_audit,
            canvas_store,
            None,
            Some(live_config),
            None,
        )
        .await
    }

    /// Build a daemon-backed ACP/WS Agent from live tool and history policy
    /// while keeping its model route generation pinned until reconnect.
    pub async fn from_live_config_with_session_cwd_and_mcp_backchannel(
        live_config: Arc<parking_lot::RwLock<Config>>,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        exclude_memory: bool,
        acp_delivery: bool,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
        canvas_store: Option<tools::CanvasStore>,
    ) -> Result<Self> {
        Self::from_pinned_live_config_with_session_cwd_and_mcp_backchannel(
            live_config,
            agent_alias,
            session_cwd,
            initialize_mcp,
            exclude_memory,
            acp_delivery,
            sop_engine,
            sop_audit,
            canvas_store,
        )
        .await
    }

    pub async fn from_live_config_with_session_cwd_and_mcp_backchannel_and_acp_sessions(
        live_config: Arc<parking_lot::RwLock<Config>>,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        exclude_memory: bool,
        acp_delivery: bool,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
        canvas_store: Option<tools::CanvasStore>,
        acp_session_store: Arc<clawcrew_infra::acp_session_store::AcpSessionStore>,
    ) -> Result<Self> {
        let config = live_config.read().clone();
        Self::from_config_with_session_cwd_and_mcp_approval_mode(
            &config,
            agent_alias,
            session_cwd,
            initialize_mcp,
            true,
            exclude_memory,
            acp_delivery,
            None,
            sop_engine,
            sop_audit,
            canvas_store,
            Some(acp_session_store),
            Some(live_config),
            None,
        )
        .await
    }

    /// Like [`Self::from_config_with_session_cwd_and_mcp_backchannel`] but also
    /// injects the TUI's captured shell environment so that tools like
    /// `ShellTool` inherit the user's real `PATH`, `SSH_AUTH_SOCK`, etc.
    /// rather than the daemon's stripped-down process environment.
    pub async fn from_config_with_tui_env(
        config: &Config,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        exclude_memory: bool,
        tui_env: Option<std::collections::HashMap<String, String>>,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
    ) -> Result<Self> {
        Self::from_config_with_session_cwd_and_mcp_approval_mode(
            config,
            agent_alias,
            session_cwd,
            initialize_mcp,
            true,
            exclude_memory,
            // TUI turns never transport an ACP file attachment.
            false,
            tui_env,
            sop_engine,
            sop_audit,
            None,
            None,
            None,
            None,
        )
        .await
    }

    /// Build a daemon-backed TUI Agent whose structured-history cap follows
    /// the shared config after reloads.
    pub async fn from_live_config_with_tui_env(
        live_config: Arc<parking_lot::RwLock<Config>>,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        exclude_memory: bool,
        tui_env: Option<std::collections::HashMap<String, String>>,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
    ) -> Result<Self> {
        // Stack-budget boundary for the daemon-backed construction paths
        // (`session/new`, rehydration, plugin agents). The whole incarnation
        // build — config snapshot, security policy, provider + route
        // resolver, memory backends, MCP, tool registry — is a deep
        // debug-build call chain that must not consume the RPC caller's
        // stack budget (the 2 MiB `session/new` regression contract): it
        // runs on a blocking-pool thread and the caller's stack pays only
        // the dispatch layers. The construction's own awaits (fs, memory
        // backends, MCP init) are driven through the captured runtime
        // handle, so their timing semantics are unchanged. Callers that
        // hold the config writer gate (`session/new`, rehydration) keep
        // holding it across this boundary, so the snapshot read below
        // still observes the same committed config generation.
        let handle = tokio::runtime::Handle::current();
        let agent_alias = agent_alias.to_string();
        let session_cwd = session_cwd.map(|p| p.to_path_buf());
        tokio::task::spawn_blocking(move || {
            let config = live_config.read().clone();
            handle.block_on(Self::from_config_with_session_cwd_and_mcp_approval_mode(
                &config,
                &agent_alias,
                session_cwd.as_deref(),
                initialize_mcp,
                true,
                exclude_memory,
                // TUI turns never transport an ACP file attachment.
                false,
                tui_env,
                sop_engine,
                sop_audit,
                None,
                None,
                Some(Arc::clone(&live_config)),
                Some(live_config),
            ))
        })
        .await
        .map_err(|join| anyhow::Error::msg(format!("agent construction task failed: {join}")))?
    }

    /// Build a daemon-backed ACP TUI Agent with access to the shared durable
    /// session store. The store is a read view for session tools; TUI turns do
    /// not gain ACP file-delivery authority.
    pub(crate) async fn from_live_config_with_tui_env_and_acp_sessions(
        live_config: Arc<parking_lot::RwLock<Config>>,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        exclude_memory: bool,
        tui_env: Option<std::collections::HashMap<String, String>>,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
        acp_session_store: Arc<clawcrew_infra::acp_session_store::AcpSessionStore>,
    ) -> Result<Self> {
        let handle = tokio::runtime::Handle::current();
        let agent_alias = agent_alias.to_string();
        let session_cwd = session_cwd.map(|p| p.to_path_buf());
        tokio::task::spawn_blocking(move || {
            let config = live_config.read().clone();
            handle.block_on(Self::from_config_with_session_cwd_and_mcp_approval_mode(
                &config,
                &agent_alias,
                session_cwd.as_deref(),
                initialize_mcp,
                true,
                exclude_memory,
                false,
                tui_env,
                sop_engine,
                sop_audit,
                None,
                Some(acp_session_store),
                Some(Arc::clone(&live_config)),
                Some(live_config),
            ))
        })
        .await
        .map_err(|join| anyhow::Error::msg(format!("agent construction task failed: {join}")))?
    }

    #[allow(clippy::too_many_arguments)]
    async fn from_config_with_session_cwd_and_mcp_approval_mode(
        config: &Config,
        agent_alias: &str,
        session_cwd: Option<&Path>,
        initialize_mcp: bool,
        approval_backchannel: bool,
        exclude_memory: bool,
        acp_delivery: bool,
        tui_env: Option<std::collections::HashMap<String, String>>,
        sop_engine: Option<Arc<std::sync::Mutex<SopEngine>>>,
        sop_audit: Option<Arc<SopAuditLogger>>,
        canvas_store: Option<tools::CanvasStore>,
        acp_session_store: Option<Arc<clawcrew_infra::acp_session_store::AcpSessionStore>>,
        live_config: Option<Arc<parking_lot::RwLock<Config>>>,
        live_model_config: Option<Arc<parking_lot::RwLock<Config>>>,
    ) -> Result<Self> {
        let agent_cfg = config
            .agent(agent_alias)
            .with_context(|| format!("agents.{agent_alias} is not configured"))?;
        let risk_profile = config
            .risk_profile_for_agent(agent_alias)
            .with_context(|| {
                format!(
                    "agents.{agent_alias}.risk_profile does not name a configured risk_profiles entry"
                )
            })?;

        let observer: Arc<dyn Observer> =
            Arc::from(observability::create_observer(&config.observability));
        let runtime: Arc<dyn platform::RuntimeAdapter> =
            Arc::from(platform::create_runtime(&config.runtime)?);
        // Per-agent workspace becomes the SecurityPolicy boundary
        // (file_read/write/edit + shell tool jail to the agent's own
        // dir). The session-cwd override still wins so ACP sessions
        // can pin tool path resolution to an IDE-provided cwd.
        let agent_workspace = config.agent_workspace_dir(agent_alias);
        // Create the per-agent workspace dir on demand so bootstrap
        // file writes (and downstream markdown-memory backends) don't
        // hit ENOENT on a fresh install.
        if let Err(e) = tokio::fs::create_dir_all(&agent_workspace).await {
            ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"agent": agent_alias, "workspace": agent_workspace.display().to_string(), "e": e.to_string()})), "Failed to create per-agent workspace dir (continuing): ");
        }
        if let Err(e) = crate::agent::personality::seed_default_personality(
            config,
            agent_alias,
            &agent_workspace,
        )
        .await
        {
            ::clawcrew_log::record!(WARN, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_outcome(::clawcrew_log::EventOutcome::Unknown).with_attrs(::serde_json::json!({"agent": agent_alias, "workspace": agent_workspace.display().to_string(), "e": e.to_string()})), "Failed to ensure per-agent bootstrap files (continuing with whatever exists): ");
        }
        let security = Arc::new({
            // Use for_agent so the runtime profile (max_actions_per_hour,
            // shell_timeout_secs, etc.) is applied — from_risk_profile passes
            // None for the runtime profile and silently falls back to the
            // schema default of 20 actions/hour regardless of config.
            let mut policy = SecurityPolicy::for_agent(config, agent_alias).with_context(|| {
                format!("agents.{agent_alias}: failed to build security policy")
            })?;
            if let Some(cwd) = session_cwd {
                policy.workspace_dir = cwd.to_path_buf();
                policy.allowed_roots.push(agent_workspace.clone());
            }
            policy
        });

        let (provider_name, provider_alias, agent_model_provider) =
            match config.resolved_model_provider_for_agent(agent_alias) {
                Some(resolved) => (resolved.0, resolved.1, Some(resolved.2)),
                None => {
                    let agent_ref = agent_cfg.model_provider.as_str();
                    if !agent_ref.is_empty() {
                        anyhow::bail!(
                            "agents.{agent_alias}.model_provider = \"{agent_ref}\" does not \
                             resolve to a configured [providers.models.<type>.<alias>] entry"
                        );
                    }
                    // V3 schema requires every agent to set model_provider.
                    // Empty is a config error rather than a silent fallback.
                    anyhow::bail!(
                        "agents.{agent_alias}.model_provider is empty — set it to a \
                         configured \"<type>.<alias>\" (e.g. \"anthropic.{agent_alias}\")"
                    );
                }
            };
        let memory: Arc<dyn Memory> = clawcrew_memory::create_memory_for_agent(
            config,
            agent_alias,
            agent_model_provider.and_then(|e| e.api_key.as_deref()),
        )
        .await?;

        let composio_key = if config.composio.enabled {
            config.composio.api_key.as_deref()
        } else {
            None
        };
        let composio_entity_id = if config.composio.enabled {
            Some(config.composio.entity_id.as_str())
        } else {
            None
        };

        // SOP loading is gated on `runtime_enabled()`: `sops_dir` is unset (or
        // empty) by default, so SOP runtime behavior is off until an operator
        // opts in by setting a directory.
        // If caller provided an engine (daemon path), use it; otherwise
        // build our own (CLI/standalone path) only when the gate is set.
        let (sop_engine, sop_audit) = match (sop_engine, sop_audit) {
            (Some(engine), Some(audit)) => (Some(engine), Some(audit)),
            (None, None) if config.sop.runtime_enabled() => {
                let mem: Arc<dyn clawcrew_memory::Memory> =
                    clawcrew_memory::create_memory_for_agent(config, agent_alias, None).await?;
                // CLI / standalone path: no channel map is wired here, so the route
                // adapter is the no-op (log-only). The daemon path builds the SOP
                // engine with a real channel-delivering adapter instead.
                let (engine, audit) = crate::sop::build_sop_engine(
                    config.sop.clone(),
                    &config.data_dir,
                    &config.install_root_dir(),
                    mem,
                    Default::default(),
                );
                (Some(engine), Some(audit))
            }
            _ => (None, None),
        };

        let acp_sessions =
            acp_session_store.map(|store| tools::AcpSessionReadView::new(store, agent_alias));
        let all_tools_result = tools::all_tools_with_runtime_and_acp_sessions(
            Arc::new(config.clone()),
            &security,
            risk_profile,
            agent_alias,
            runtime.clone(),
            memory.clone(),
            composio_key,
            composio_entity_id,
            &config.browser,
            &config.http_request,
            &config.web_fetch,
            &security.workspace_dir,
            &config.agents,
            agent_model_provider.and_then(|e| e.api_key.as_deref()),
            config,
            canvas_store,
            false,
            tui_env,
            sop_engine,
            sop_audit,
            // Daemon-backed constructors supply the shared handle; tools that
            // resolve config per call (plugin tools, `send_via` authority, and
            // the A2A outbound client) must follow reloads rather than this
            // call's `config` snapshot. `None` here would silently pin them to
            // startup state for the Agent's whole lifetime. One-shot callers
            // pass `None` and keep the documented snapshot fallback.
            live_config.clone(),
            acp_sessions,
        )?;
        // Skills are loaded here and handed to `assemble`, which owns skill
        // registration and resolves builtin/MCP elevation against the pre-filter
        // arcs internally. Bundle-aware via `[agents.<alias>].skill_bundles`.
        let skills = crate::skills::load_skills_for_agent_from_config(config, agent_alias);
        // Captured before `assemble` consumes the result: the concrete delegate
        // instance this registry built, so live-config regressions can drive its
        // nested-registry construction instead of re-deriving the wiring.
        #[cfg(test)]
        let built_delegate_tool = all_tools_result.delegate_tool.clone();
        // Capture before `runtime` is moved into `ScopedAssembly`.
        let shell_profile = runtime.shell_profile();
        let assembled = crate::tools::scoped::ScopedToolRegistry::assemble(
            crate::tools::scoped::ScopedAssembly {
                config,
                agent_alias,
                security: &security,
                built: all_tools_result,
                skills: &skills,
                runtime,
                caller_allowed: None,
                connect_mcp: initialize_mcp,
                connect_peripherals: false,
                exclude_memory,
                acp_delivery,
                list_deferred_mcp_specs: false,
                emit_assembly_logs: true,
                // `from_config` is the Agent (gateway / library) construction
                // path: no cross-turn reuse contract, so the per-call
                // `connect_all` is the correct choice. The daemon heartbeat
                // worker is the only `mcp_registry` supplier.
                mcp_registry: None,
            },
        )
        .await;
        // The Agent injects two distinct MCP prompt slots: `mcp_deferred_section` (the
        // deferred tool-search listing) and `mcp_pinned_section` (pinned resources).
        // `assemble` surfaces the two atomically, so from_config threads each into its
        // own slot below - no duplication, and the deferred advertisement the
        // regression suite asserts is preserved.
        let deferred_section = assembled.deferred_section().to_string();
        let pinned_section = assembled.pinned_section().to_string();
        let crate::tools::scoped::ScopedAssembled {
            registry,
            delegate_handle: _,
            ask_user_handle,
            reaction_handle,
            poll_handle,
            escalate_handle,
            channel_room_handle,
            activated_handle,
            // from_config performs no per-turn tool_filter_groups filtering
            // itself, so mcp_tool_names is dropped here along with `registry`'s
            // already-consumed sibling fields via `..`.
            ..
        } = assembled;
        // Thread the sealed registry straight to the builder - `.tools(...)` now
        // takes a `ScopedToolRegistry`, so no `into_inner()` unwrap here.
        let tools = registry;

        let model_name = match agent_model_provider
            .and_then(|e| e.model.as_deref())
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            Some(m) => m.to_string(),
            None => anyhow::bail!(
                "agents.{agent_alias}.model_provider resolves to a model_provider entry \
                 with no `model` set. Configure [providers.models.{provider_name}.<alias>] \
                 model = \"...\".",
            ),
        };

        let provider_ref = format!("{provider_name}.{provider_alias}");
        let provider_runtime_options = clawcrew_providers::provider_runtime_options_for_alias(
            config,
            provider_name,
            provider_alias,
        );

        let (model_provider, model_route_resolver) =
            clawcrew_providers::create_routed_model_provider_with_options_and_resolver(
                config,
                &provider_ref,
                agent_model_provider.and_then(|e| e.api_key.as_deref()),
                agent_model_provider.and_then(|e| e.uri.as_deref()),
                &config.reliability,
                &config.model_routes,
                &model_name,
                &provider_runtime_options,
            )?;

        let tool_dispatcher =
            tool_dispatcher_for_provider(agent_cfg, model_provider.as_ref(), &model_name);

        let response_cache = if config.memory.response_cache_enabled {
            clawcrew_memory::response_cache::ResponseCache::with_hot_cache(
                &config.data_dir,
                config.memory.response_cache_ttl_minutes,
                config.memory.response_cache_max_entries,
                config.memory.response_cache_hot_entries,
            )
            .ok()
            .map(Arc::new)
        } else {
            None
        };

        let approval_manager = if approval_backchannel {
            ApprovalManager::for_non_interactive_backchannel(risk_profile)
        } else {
            ApprovalManager::for_non_interactive(risk_profile)
        };

        // Daemon-backed agents resolve limits from a generation CELL rather than
        // directly from `live_config`. Callers with an acknowledged model
        // refresh transaction may supply `live_model_config`; direct ACP/WS
        // callers omit it so provider, route resolver, and limits all remain on
        // the construction generation until reconnect. The separate
        // `live_config` handle remains available to tools and history policy.
        // `config` is the immutable snapshot captured by the live constructor
        // before async setup. Seed the generation cell from that same snapshot
        // so provider/resolver/limits cannot be split across two commits.
        let config_generation: Option<ConfigGeneration> = live_config
            .as_ref()
            .map(|_| Arc::new(parking_lot::RwLock::new(Arc::new(config.clone()))));
        let live_config_for_generation = live_model_config;

        let context_limits_resolver: ContextLimitsResolver =
            if let Some(generation) = config_generation.as_ref().map(Arc::clone) {
                Arc::new(Agent::context_generation_limits_resolver(
                    generation,
                    agent_alias.to_string(),
                ))
            } else {
                let limit_config = config.clone();
                let limit_agent_alias = agent_alias.to_string();
                Arc::new(move |provider_ref, model| {
                    limit_config.resolved_context_limits_for_route(
                        &limit_agent_alias,
                        provider_ref,
                        model,
                    )
                })
            };

        let structured_history_cap_resolver: Arc<dyn Fn() -> usize + Send + Sync> =
            if let Some(cap_config) = live_config {
                let cap_agent_alias = agent_alias.to_string();
                Arc::new(move || {
                    cap_config
                        .read()
                        .effective_structured_max_history_messages(&cap_agent_alias)
                })
            } else {
                let max = config.effective_structured_max_history_messages(agent_alias);
                Arc::new(move || max)
            };

        let builder = Agent::builder();
        #[cfg(test)]
        let builder = builder.delegate_tool(built_delegate_tool);
        let mut builder = builder
            .model_provider(model_provider)
            .tools(tools)
            .memory(memory.clone())
            .observer(observer)
            .response_cache(response_cache)
            .tool_dispatcher(tool_dispatcher)
            .memory_inject_cfg(
                crate::agent::memory_inject::MemoryInjectConfig::from_memory_config(
                    &config.memory,
                    config.effective_memory_recall_limit(agent_alias),
                ),
            )
            .prompt_builder(SystemPromptBuilder::with_defaults())
            .shell_profile(shell_profile)
            .config(
                config
                    .resolved_agent_config(agent_alias)
                    .unwrap_or_else(|| agent_cfg.clone()),
            )
            .structured_history_cap_resolver(structured_history_cap_resolver)
            .context_limits_resolver(context_limits_resolver)
            .multimodal_config(config.multimodal.clone())
            .agent_alias(agent_alias.to_string())
            .model_name(model_name)
            // Store the full "type.alias" ref so the live provider identity
            // (attribution_fields().1) carries the same key the config
            // provider registry is keyed by, and wire-emission paths can
            // resolve model_context_window / cost pricing for the provider
            // that actually served the call.
            .model_provider_name(provider_ref.clone())
            .temperature(agent_model_provider.and_then(|e| e.temperature))
            .workspace_dir(security.workspace_dir.clone())
            .agent_workspace_dir(agent_workspace.clone())
            .classification_config(config.query_classification.clone())
            .model_route_resolver(model_route_resolver)
            .identity_config(agent_cfg.identity.clone())
            .skills(skills)
            .skills_prompt_mode(config.effective_skills_prompt_mode(agent_alias))
            .auto_save(config.memory.auto_save)
            .exclude_memory(exclude_memory)
            .security_summary(Some(security.prompt_summary()))
            .autonomy_level(risk_profile.level)
            .approval_route(risk_profile.approval_route.clone())
            .activated_tools(activated_handle)
            .mcp_deferred_section(Some(deferred_section))
            .mcp_pinned_section(Some(pinned_section))
            .hook_runner(if config.hooks.enabled {
                Some(Arc::new(crate::hooks::HookRunner::from_config(
                    &config.hooks,
                )))
            } else {
                None
            })
            .approval_manager(Some(Arc::new(approval_manager)))
            // The switch snapshot is seeded from the SAME generation the limits
            // resolver above reads, and both are republished together by
            // `sync_config_generation`.
            .provider_switch_config(ProviderSwitchConfig {
                config: Some(
                    config_generation
                        .as_ref()
                        .map_or_else(|| Arc::new(config.clone()), |cell| Arc::clone(&cell.read())),
                ),
                live: live_config_for_generation,
            });
        if let Some(generation) = config_generation {
            builder = builder.config_generation(generation);
        }
        let mut agent = builder.build()?;

        // Wire per-tool channel-map handles into the agent so callers (e.g.
        // the ACP server) can register back-channels after construction.
        agent.channel_handles = AgentChannelHandles {
            ask_user: ask_user_handle,
            channel_room: channel_room_handle,
            reaction: reaction_handle,
            poll: poll_handle,
            escalate: escalate_handle,
        };

        Ok(agent)
    }

    fn trim_history(&mut self, turn_id: Option<&str>) -> Option<HistoryTrimNotice> {
        let max = self
            .structured_history_cap_resolver
            .as_ref()
            .map_or(self.config.resolved.max_history_messages, |resolve| {
                resolve()
            });
        if self.history.len() <= max {
            return None;
        }
        let result = crate::agent::history_trim::trim_conversation_to_recent_turns(
            std::mem::take(&mut self.history),
            max,
            self.history_has_trim_breadcrumb,
        );
        self.history = result.history;
        if !result.trimmed {
            return None;
        }

        crate::agent::history_trim::insert_conversation_breadcrumb(&mut self.history);
        self.history_has_trim_breadcrumb = true;
        self.history_trim_generation = self.history_trim_generation.wrapping_add(1);
        let reason = crate::i18n::get_required_cli_string("history-trim-reason-message-cap");
        let channel = self.channel_name.clone();
        let agent_alias = self.observer_agent_alias();
        let turn_id = turn_id.map(str::to_owned);

        {
            let scope_span = ::clawcrew_log::info_span!(
                target: "clawcrew_log_internal_scope",
                "clawcrew_scope",
                agent_alias = ::clawcrew_log::field::Empty,
                channel = %channel,
                trace_id = ::clawcrew_log::field::Empty,
            );
            if let Some(agent_alias) = agent_alias.as_deref() {
                scope_span.record("agent_alias", agent_alias);
            }
            if let Some(turn_id) = turn_id.as_deref() {
                scope_span.record("trace_id", turn_id);
            }
            let _scope_guard = scope_span.enter();
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Complete)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_outcome(::clawcrew_log::EventOutcome::Success)
                    .with_attrs(::serde_json::json!({
                        "max_history_messages": max,
                        "dropped_messages": result.dropped_messages,
                        "dropped_turns": result.dropped_turns,
                        "kept_turns": result.kept_turns,
                        "remaining_messages": self.history.len(),
                    })),
                "trim_history: dropped oldest whole turns"
            );
        }

        self.observer.record_event(&ObserverEvent::HistoryTrimmed {
            dropped_messages: result.dropped_messages,
            kept_turns: result.kept_turns,
            reason: reason.clone(),
            channel: Some(channel),
            agent_alias,
            turn_id,
            // Message-limit trims carry no token accounting.
            token_budget: None,
            tokens_before: None,
            tokens_after: None,
            tokens_before_source: None,
            tokens_after_source: None,
            unsatisfiable_floor: None,
        });

        Some(HistoryTrimNotice {
            dropped_messages: result.dropped_messages,
            kept_turns: result.kept_turns,
            reason,
        })
    }

    fn append_receipts_block(
        &self,
        response: String,
        scope: Option<&crate::agent::tool_receipts::ReceiptScope>,
    ) -> String {
        if !self.config.resolved.tool_receipts.show_in_response {
            return response;
        }
        let Some(scope) = scope else {
            return response;
        };
        let block = {
            let receipts = scope.collector().lock().unwrap_or_else(|e| e.into_inner());
            crate::agent::tool_receipts::render_receipts_block(&receipts)
        };
        match block {
            Some(block) => {
                if response.is_empty() {
                    block
                } else {
                    format!("{response}\n\n{block}")
                }
            }
            None => response,
        }
    }

    /// Append a user-visible notice when the resilient provider wrapper served
    /// this turn with a different model or provider than requested (silent
    /// model downgrade, e.g. a `fallback_models` entry kicking in). The record
    /// is consumed from the `clawcrew_providers::reliable` task-local (single
    /// source of truth); nothing is stored.
    ///
    /// The notice is BOTH appended to the returned response (rendered by
    /// consumers of the final text, e.g. the gateway web UI's `done` frame)
    /// and streamed as a trailing [`TurnEvent::Chunk`] (rendered by streaming
    /// consumers that discard the final text on a clean finish, e.g. the
    /// ZeroCode TUI).
    ///
    /// A safeguard notice composed from the original request replaces this
    /// generic notice; a server-side safeguard notice does not, because the
    /// generic record is then the only presentation of the ordinary leg that
    /// preceded it. The safeguard notice itself is rendered by the caller.
    async fn append_model_fallback_notice(
        response: String,
        fallback: Option<&clawcrew_providers::reliable::ProviderFallbackInfo>,
        safeguard: Option<&clawcrew_providers::SafeguardFallbackNotice>,
        event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> String {
        let fallback = clawcrew_providers::visible_provider_fallback(fallback, safeguard);
        let with_notice = Self::format_model_fallback_notice(response.clone(), fallback);
        if with_notice == response {
            return response;
        }
        let Some(delta) = with_notice.strip_prefix(&response) else {
            return response;
        };
        let delta = delta.to_string();
        let _ = event_tx.send(TurnEvent::Chunk { delta }).await;
        with_notice
    }

    fn format_model_fallback_notice(
        response: String,
        fallback: Option<&clawcrew_providers::reliable::ProviderFallbackInfo>,
    ) -> String {
        let Some(fallback) = fallback else {
            return response;
        };
        // The wrapper also records plain retries (attempt > 0 on the primary
        // entry); an identical requested/served pair is not a downgrade.
        if fallback.actual_provider == fallback.requested_provider
            && fallback.actual_model == fallback.requested_model
        {
            return response;
        }
        let notice = crate::i18n::get_required_cli_string_with_args(
            "turn-model-fallback-notice",
            &[
                ("requested_model", fallback.requested_model.as_str()),
                ("requested_provider", fallback.requested_provider.as_str()),
                ("actual_model", fallback.actual_model.as_str()),
                ("actual_provider", fallback.actual_provider.as_str()),
            ],
        );
        if response.is_empty() {
            notice
        } else {
            format!("{response}\n\n{notice}")
        }
    }

    fn build_system_prompt(&self) -> Result<String> {
        self.build_system_prompt_with_dispatcher(self.tool_dispatcher.as_ref())
    }

    fn tool_protocol_prompts(&self) -> Result<Arc<crate::agent::turn::ToolProtocolPrompts>> {
        Ok(Arc::new(crate::agent::turn::ToolProtocolPrompts::new(
            self.build_system_prompt_with_dispatcher(&NativeToolDispatcher)?,
            self.build_system_prompt_with_dispatcher(&XmlToolDispatcher)?,
        )))
    }

    fn build_system_prompt_with_dispatcher(
        &self,
        dispatcher: &dyn ToolDispatcher,
    ) -> Result<String> {
        let expose_text_tool_protocol =
            !self.config.resolved.strict_tool_parsing || dispatcher.should_send_tool_specs();
        let no_tools: Vec<Box<dyn Tool>> = Vec::new();
        // Both arms resolve to `&[Box<dyn Tool>]`: the sealed registry derefs to
        // the same slice the raw `Vec` used to expose, so downstream prompt
        // construction is unchanged.
        let prompt_tools: &[Box<dyn Tool>] = if expose_text_tool_protocol {
            &self.tools
        } else {
            &no_tools
        };
        let instructions = dispatcher.prompt_instructions(prompt_tools);
        // Prompt policy facts come from the same ApprovalManager the
        // execution gate consults (borrowed, render-time). A builder without
        // a manager retains its legacy autonomy fallback but cannot name
        // `always_ask` exceptions it does not own.
        let (prompt_autonomy_level, prompt_always_ask) = match self.approval_manager.as_deref() {
            Some(mgr) => (mgr.autonomy_level(), mgr.always_ask_tools()),
            None => (self.autonomy_level, Vec::new()),
        };
        let ctx = PromptContext {
            workspace_dir: &self.workspace_dir,
            agent_workspace_dir: &self.agent_workspace_dir,
            model_name: &self.model_name,
            tools: prompt_tools,
            skills: &self.skills,
            skills_prompt_mode: self.skills_prompt_mode,
            identity_config: Some(&self.identity_config),
            interaction: self.interaction_context.as_ref(),
            dispatcher_instructions: &instructions,
            sends_native_tool_specs: dispatcher.should_send_tool_specs()
                && !prompt_tools.is_empty(),
            security_summary: self.security_summary.clone(),
            autonomy_level: prompt_autonomy_level,
            inject_memory: self.inject_memory,
            shell_profile: self.shell_profile.clone(),
        };
        let mut prompt = self
            .prompt_builder
            .build_with_approval_policy(&ctx, &prompt_always_ask)?;
        append_timestamp_orientation(&mut prompt);
        let receipts = &self.config.resolved.tool_receipts;
        if receipts.enabled && receipts.inject_system_prompt {
            prompt.push_str(crate::agent::tool_receipts::SYSTEM_PROMPT_ADDENDUM);
        }
        if !self.mcp_deferred_section.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&self.mcp_deferred_section);
        }
        if !self.mcp_pinned_section.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&self.mcp_pinned_section);
        }
        Ok(prompt)
    }

    fn rebuild_system_prompt_for_dispatcher(
        &mut self,
        dispatcher: &dyn ToolDispatcher,
    ) -> Result<()> {
        let new_prompt = self.build_system_prompt_with_dispatcher(dispatcher)?;
        let Some(ConversationMessage::Chat(first)) = self.history.first_mut() else {
            return Ok(());
        };
        if first.role != "system" {
            return Ok(());
        }
        first.content = new_prompt;
        Ok(())
    }

    fn rebuild_streamed_system_prompt_for_active_provider(
        &mut self,
        loop_history: &mut [ChatMessage],
    ) -> Result<()> {
        let dispatcher = tool_dispatcher_for_provider(
            &self.config,
            self.model_provider.as_ref(),
            &self.model_name,
        );
        self.rebuild_system_prompt_for_dispatcher(dispatcher.as_ref())?;

        let Some(ConversationMessage::Chat(persisted)) = self.history.first() else {
            return Ok(());
        };
        let Some(active) = loop_history
            .first_mut()
            .filter(|message| message.role == "system")
        else {
            return Ok(());
        };
        active.content.clone_from(&persisted.content);
        Ok(())
    }

    fn try_apply_model_switch(
        &mut self,
        current_effective_model: &str,
        new_model_provider: String,
        new_model: String,
    ) -> Option<String> {
        // Same-provider, same-model: nothing to do. The request is owned by
        // the completed tool-loop scope, so there is no persistent slot to clear.
        if new_model_provider == self.model_provider_name && new_model == current_effective_model {
            return None;
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            &format!(
                "Model switch detected in turn_streamed: {} {} -> {} {}",
                self.model_provider_name, current_effective_model, new_model_provider, new_model
            )
        );

        let switch_outcome: anyhow::Result<(
            Box<dyn ModelProvider>,
            Arc<clawcrew_providers::router::ModelRouteResolver>,
        )> = match self
            .provider_switch_config
            .as_ref()
            .and_then(|cfg| cfg.config.as_ref())
        {
            Some(full_config) => {
                let agent_entry = full_config
                    .resolved_model_provider_for_agent(&self.agent_alias)
                    .map(|(_ty, _alias, entry)| entry);
                let default_api_key = agent_entry.and_then(|e| e.api_key.as_deref());
                let default_base_url = agent_entry.and_then(|e| e.uri.as_deref());

                // Prefer a route-specific api_key when the switched
                // provider/model matches a configured model_route entry.
                let route_api_key = full_config
                    .model_routes
                    .iter()
                    .find(|r| {
                        r.model_provider.eq_ignore_ascii_case(&new_model_provider)
                            && (r.model.eq_ignore_ascii_case(&new_model)
                                || r.hint.eq_ignore_ascii_case(&new_model))
                    })
                    .and_then(|r| r.api_key.as_deref());
                let api_key = route_api_key.or(default_api_key);

                let runtime_options = new_model_provider
                    .split_once('.')
                    .map(|(family, alias)| {
                        clawcrew_providers::provider_runtime_options_for_alias(
                            full_config.as_ref(),
                            family,
                            alias,
                        )
                    })
                    .unwrap_or_default();

                clawcrew_providers::create_routed_model_provider_with_options_and_resolver(
                    full_config.as_ref(),
                    &new_model_provider,
                    api_key,
                    default_base_url,
                    &full_config.reliability,
                    &full_config.model_routes,
                    &new_model,
                    &runtime_options,
                )
            }
            None => Err(anyhow::Error::msg(
                "model_switch requested but agent has no provider_switch_config; \
                 cannot rebuild provider safely",
            )),
        };

        match switch_outcome {
            Ok((new_prov, new_route_resolver)) => {
                // Commit state only after the provider was built
                // successfully.
                self.model_provider = new_prov;
                self.model_route_resolver = new_route_resolver;
                self.model_provider_name = new_model_provider;
                self.model_name = new_model.clone();
                Some(new_model)
            }
            Err(e) => {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"err": e.to_string()})),
                    &format!(
                        "Failed to apply model_switch in turn_streamed; staying on {} {}",
                        self.model_provider_name, current_effective_model
                    )
                );
                None
            }
        }
    }

    fn classify_model(&self, user_message: &str) -> String {
        if let Some(decision) =
            super::classifier::classify_with_decision(&self.classification_config, user_message)
            && self.model_route_resolver.has_hint(&decision.hint)
        {
            let resolved_model = self
                .model_route_resolver
                .configured_model_for_hint(&decision.hint)
                .unwrap_or("unknown");
            ::clawcrew_log::record!(INFO, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"hint": decision.hint.as_str(), "model": resolved_model, "rule_priority": decision.priority, "message_length": user_message.len()})), "Classified message route");
            return format!("hint:{}", decision.hint);
        }

        // Fallback: auto-classify by complexity when no rule matched.
        if let Some(ref ac) = self.config.resolved.auto_classify {
            let tier = super::eval::estimate_complexity(user_message);
            if let Some(hint) = ac.hint_for(tier)
                && self.model_route_resolver.has_hint(hint)
            {
                ::clawcrew_log::record!(INFO, ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(::serde_json::json!({"hint": hint, "complexity": format!("{:?}", tier), "message_length": user_message.len()})), "Auto-classified by complexity");
                return format!("hint:{hint}");
            }
        }

        self.model_name.clone()
    }

    fn replay_loop_messages(
        loop_messages: &[ChatMessage],
        injected: Option<MemoryPreambleTarget<'_>>,
    ) -> Vec<ConversationMessage> {
        // The turn engine injects the recalled-memory preamble onto the last
        // user message (`turn::mod.rs`'s `memory` handling, which records the
        // exact rendered block) for this turn's provider request only; it
        // must never land in durable/canonical history, which every call
        // site of this function feeds.
        //
        // The strip is positional, never content-discovered: only
        // `injected.index` is considered, and only when that message is
        // still a user-role message starting with the recorded preamble.
        // Inferring the target from text instead — scanning every message
        // for the preamble, even from the end — silently changes genuine
        // history in two reachable cases. First, the no-trim and streamed
        // callers replay pre-injection canonical clones alongside the
        // recorded preamble; when the user's original text starts with that
        // exact block, any content match strips genuine content from a
        // buffer the injector never touched (those callers now pass `None`).
        // Second, steering input appends newer user messages after the
        // injected one, so a reverse scan can select the steering message,
        // damaging it while leaving the injected memory in place. An older
        // genuine message equal to the block is likewise never considered.
        //
        // The content confirmation is belt-and-braces, not discovery: it
        // covers the trim dropping the injected message itself (the preamble
        // leaves with it, so there is nothing to clean) without touching an
        // unrelated message that shifted into the recorded position.
        let strip_at = injected.as_ref().and_then(|target| {
            loop_messages
                .get(target.index)
                .filter(|msg| msg.role == "user" && msg.content.starts_with(target.preamble))
                .map(|_| target.index)
        });
        let mut replayed: Vec<ConversationMessage> = Vec::with_capacity(loop_messages.len());
        let push_tool_results = |replayed: &mut Vec<ConversationMessage>,
                                 results: Vec<ToolResultMessage>| {
            if let Some(ConversationMessage::ToolResults(previous)) = replayed.last_mut() {
                previous.extend(results);
            } else {
                replayed.push(ConversationMessage::ToolResults(results));
            }
        };
        for (index, msg) in loop_messages.iter().enumerate() {
            if msg.role == "assistant"
                && let Ok(serde_json::Value::Object(obj)) =
                    serde_json::from_str::<serde_json::Value>(&msg.content)
                && let Some(calls) = obj.get("tool_calls").and_then(|c| c.as_array())
                && !calls.is_empty()
                && calls.iter().all(|c| {
                    c.get("id").is_some_and(serde_json::Value::is_string)
                        && c.get("name").is_some_and(serde_json::Value::is_string)
                })
            {
                let tool_calls = calls
                    .iter()
                    .map(|c| clawcrew_providers::ToolCall {
                        id: c
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        name: c
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        arguments: c
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        extra_content: None,
                    })
                    .collect();
                replayed.push(ConversationMessage::AssistantToolCalls {
                    text: obj
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    tool_calls,
                    reasoning_content: obj
                        .get("reasoning_content")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                });
                continue;
            }
            if msg.role == "tool" {
                if let Ok(vals) = serde_json::from_str::<Vec<serde_json::Value>>(&msg.content) {
                    let results: Vec<ToolResultMessage> = vals
                        .into_iter()
                        .filter_map(|v| {
                            Some(ToolResultMessage {
                                tool_call_id: v.get("tool_call_id")?.as_str()?.to_string(),
                                content: v
                                    .get("content")
                                    .and_then(|c| c.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                // Provider-wire tool messages do not carry the
                                // producing tool name; replayed results fall back
                                // to blind canonicalization
                                tool_name: String::new(),
                            })
                        })
                        .collect();
                    if !results.is_empty() {
                        push_tool_results(&mut replayed, results);
                        continue;
                    }
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg.content) {
                    let result = ToolResultMessage {
                        tool_call_id: v
                            .get("tool_call_id")
                            .and_then(|id| id.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        content: v
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        // No provenance on the provider-wire shape; blind canon
                        // applies as before
                        tool_name: String::new(),
                    };
                    push_tool_results(&mut replayed, vec![result]);
                    continue;
                }
            }
            let stripped = if strip_at == Some(index) {
                crate::agent::memory_inject::strip_memory_context_preamble(
                    &msg.content,
                    injected.as_ref().map(|target| target.preamble),
                )
            } else {
                msg.content.as_str()
            };
            if stripped.len() == msg.content.len() {
                replayed.push(ConversationMessage::Chat(msg.clone()));
            } else {
                let mut msg = msg.clone();
                msg.content = stripped.to_string();
                replayed.push(ConversationMessage::Chat(msg));
            }
        }
        replayed
    }

    pub async fn turn(&mut self, user_message: &str) -> Result<String> {
        if user_message.trim().is_empty() {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "reason": "empty_user_message",
                        "entry_point": "Agent::turn",
                        "raw_len": user_message.len(),
                    })),
                "Refusing blank user turn (would emit timestamp-only message and risk prompt-template bleed-through)"
            );
            return Err(anyhow::Error::msg(
                "empty user message: refusing to dispatch a blank turn",
            ));
        }

        if self.history.is_empty() {
            let system_prompt = self.build_system_prompt()?;
            self.history
                .push(ConversationMessage::Chat(ChatMessage::system(
                    system_prompt,
                )));
        }

        // Pin one config generation for this whole turn BEFORE resolving a
        // route, so the provider this turn may rebuild and the limits it
        // reports come from the same generation.
        self.sync_config_generation();

        let effective_model = self.classify_model(user_message);
        let selected_route = self.model_route_resolver.resolve(&effective_model);
        let context_limits =
            self.context_limits_for_route(&selected_route.provider_name, &selected_route.model);

        let turn_id = Self::new_turn_id();
        let turn_observer = Arc::clone(&self.observer);
        let mut guard = crate::observability::AgentTurnGuard::start(
            turn_observer.as_ref(),
            selected_route.provider_name.clone(),
            selected_route.model.clone(),
            Some(self.channel_name.clone()),
            self.observer_agent_alias(),
            Some(turn_id.clone()),
        );

        // Memory context is injected once in the engine, keyed on the
        // ingress origin (agent::memory_inject).
        if self.auto_save {
            let store_start = std::time::Instant::now();
            let store_result = self
                .memory
                .store(
                    "user_msg",
                    user_message,
                    MemoryCategory::Conversation,
                    self.memory_session_id.as_deref(),
                )
                .await;
            self.observer.record_event(&ObserverEvent::MemoryStore {
                category: MemoryCategory::Conversation.to_string(),
                backend: self.memory.name().to_string(),
                duration: store_start.elapsed(),
                success: store_result.is_ok(),
                channel: Some(self.channel_name.clone()),
                agent_alias: self.observer_agent_alias(),
                turn_id: Some(turn_id.clone()),
            });
        }

        let enriched = self.enrich_user_message(user_message);

        self.history
            .push(ConversationMessage::Chat(ChatMessage::user(enriched)));

        let active_dispatcher = {
            let base_provider_messages = self.tool_dispatcher.to_provider_messages(&self.history);
            let (vision_provider_box, _degrade_strip_images) =
                match crate::agent::turn::resolve_vision_provider(
                    self.full_config(),
                    self.model_provider.as_ref(),
                    &base_provider_messages,
                    &self.multimodal_config,
                    &selected_route.provider_name,
                    &selected_route.model,
                    &effective_model,
                ) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        let _ = self.trim_history(Some(&turn_id));
                        return Err(error);
                    }
                };
            let (active_provider, active_model): (&dyn ModelProvider, &str) =
                vision_provider_box.as_ref().map_or(
                    (self.model_provider.as_ref(), effective_model.as_str()),
                    |resolved| (resolved.provider.as_ref(), resolved.model.as_str()),
                );
            tool_dispatcher_for_provider(&self.config, active_provider, active_model)
        };

        if let Err(error) = self.rebuild_system_prompt_for_dispatcher(active_dispatcher.as_ref()) {
            let _ = self.trim_history(Some(&turn_id));
            return Err(error);
        }
        let tool_protocol_prompts = match self.tool_protocol_prompts() {
            Ok(prompts) => prompts,
            Err(error) => {
                let _ = self.trim_history(Some(&turn_id));
                return Err(error);
            }
        };

        let provider_messages = active_dispatcher.to_provider_messages(&self.history);
        let cache_key = self.response_cache_key_for_messages(&provider_messages, &effective_model);

        if let (Some(cache), Some(key)) = (&self.response_cache, &cache_key) {
            if let Ok(Some(cached)) = cache.get(key) {
                self.observer.record_event(&ObserverEvent::CacheHit {
                    cache_type: "response".into(),
                    tokens_saved: 0,
                });
                self.history
                    .push(ConversationMessage::Chat(ChatMessage::assistant(
                        cached.clone(),
                    )));
                let _ = self.trim_history(Some(&turn_id));
                return Ok(cached);
            }
            self.observer.record_event(&ObserverEvent::CacheMiss {
                cache_type: "response".into(),
            });
        }

        // Split provider_messages: loop_history gets past turns only, while
        // loop_new_messages carries the canonical current turn for replay.
        // Request hooks mutate only the per-iteration provider snapshot.
        let split_idx = provider_messages
            .iter()
            .rposition(|m| m.role == "user")
            .unwrap_or(provider_messages.len());
        let mut loop_history = provider_messages[..split_idx].to_vec();
        let original_loop_history_len = loop_history.len();
        let original_loop_history_crumb = self.history_has_trim_breadcrumb;
        // Seed raw-transcript crumb provenance from the structured history's
        // owner-tracked state (the conversion preserves the crumb position).
        let mut loop_history_crumb_present = self.history_has_trim_breadcrumb;
        let mut loop_injected_memory_preamble: Option<String> = None;
        let mut loop_new_messages: Vec<ChatMessage> = provider_messages[split_idx..].to_vec();
        let knobs = crate::agent::loop_::LoopKnobs {
            dedup_enabled: false,
            max_iteration_behavior: crate::agent::loop_::MaxIterationBehavior::ErrorAtCap,
            detect_protocol_without_tools: false,
            draft_reasoning: clawcrew_config::schema::StreamReasoningMode::Status,
        };
        // E3 never had pattern-based loop detection; default pacing turns it
        // on. Keep the embedder contract (an N-step identical-args tool chain
        // completes) until the Agent surface grows a pacing config of its own.
        let pacing = clawcrew_config::schema::PacingConfig {
            loop_detection_enabled: false,
            ..clawcrew_config::schema::PacingConfig::default()
        };

        // Keep the loop call as a plain `.await` on this task. Caller-scoped
        // task-locals (session key, cost tracking, tool choice / thinking
        // overrides) silently vanish across a spawn.
        let cost_context = self.tool_loop_cost_tracking_context();
        let receipt_scope = crate::agent::tool_receipts::ReceiptScope::from_config(
            &self.config.resolved.tool_receipts,
        );
        let agent_alias_for_loop = self.observer_agent_alias();
        let turn_loop = crate::agent::loop_::TOOL_LOOP_COST_TRACKING_CONTEXT.scope(
            Some(cost_context.clone()),
            crate::agent::tool_receipts::scope_receipts(
                receipt_scope.clone(),
                Box::pin(crate::agent::loop_::run_tool_call_loop(
                    crate::agent::loop_::ToolLoop {
                        exec: crate::agent::loop_::ResolvedAgentExecution::resolve(
                            crate::agent::loop_::ResolvedModelAccess {
                                model_provider: self.model_provider.as_ref(),
                                provider_name: &selected_route.provider_name,
                                model: &selected_route.model,
                                dispatch_model: &effective_model,
                                temperature: self.temperature,
                            },
                            crate::agent::loop_::ResolvedIo {
                                tools_registry: &self.tools,
                                observer: self.observer.as_ref(),
                                silent: false,
                                approval: self.approval_manager.as_deref(),
                                multimodal_config: &self.multimodal_config,
                                // Inlined `full_config()` (per-field borrow) so it coexists with
                                // the `&mut self.image_cache` in this same ToolLoop expression.
                                config: self
                                    .provider_switch_config
                                    .as_ref()
                                    .and_then(|c| c.config.as_deref()),
                                hooks: self.hook_runner.as_deref(),
                                activated_tools: self.activated_tools.as_ref(),
                                app_registry: self.app_registry.as_ref(),
                                model_switch_callback: None,
                                receipt_generator: receipt_scope
                                    .as_ref()
                                    .map(crate::agent::tool_receipts::ReceiptScope::generator),
                            },
                            crate::agent::loop_::ResolvedRuntimeKnobs {
                                max_tool_iterations: self.config.resolved.max_tool_iterations,
                                excluded_tools: &[],
                                dedup_exempt_tools: &self.config.resolved.tool_call_dedup_exempt,
                                pacing: &pacing,
                                strict_tool_parsing: self.config.resolved.strict_tool_parsing,
                                parallel_tools: self.config.resolved.parallel_tools,
                                max_tool_result_chars: self.config.resolved.max_tool_result_chars,
                                context_limits,
                                context_limits_resolver: self.context_limits_resolver.clone(),
                                knobs: &knobs,
                            },
                        ),
                        history: &mut loop_history,
                        history_has_trim_breadcrumb: &mut loop_history_crumb_present,
                        injected_memory_preamble: &mut loop_injected_memory_preamble,
                        channel_name: &self.channel_name,
                        channel_reply_target: None,
                        cancellation_token: None,
                        on_delta: None,
                        shared_budget: None,
                        channel: None,
                        collected_receipts: receipt_scope
                            .as_ref()
                            .map(crate::agent::tool_receipts::ReceiptScope::collector),
                        event_tx: None,
                        steering: None,
                        new_messages_out: Some(&mut loop_new_messages),
                        image_cache: Some(&mut self.image_cache),
                        // Direct embedded Agent::turn call; source/transport/
                        // trust stay placeholders, not yet stamped at the edge.
                        memory: Some(crate::agent::memory_inject::TurnMemory {
                            handle: self.memory.as_ref(),
                            query: user_message.to_string(),
                            sessions: vec![self.memory_session_id.clone()],
                            suppress: false,
                            cfg: self.memory_inject_cfg,
                        }),
                        ingress: clawcrew_api::ingress::IngressContext::agent_direct(),
                        agent_alias: agent_alias_for_loop.as_deref(),
                        parent_agent_alias: None,
                        turn_id: &turn_id,
                        // Non-streamed `Agent::turn` returns text, not a
                        // terminal `StreamedTurnSuccess`, so it publishes no
                        // route snapshot.
                        served_route_sink: None,
                        // Live-daemon SOP path: re-assemble a nested step's agent
                        // when it delegates elsewhere. Config survives only via
                        // `provider_switch_config`; with `None` (test builder) a
                        // cross-agent step FAILS CLOSED rather than inheriting
                        // this turn's context.
                        sop_reassembly: self
                            .provider_switch_config
                            .as_ref()
                            .and_then(|c| c.config.as_deref())
                            .map(|config| crate::agent::turn::SopStepReassembly { config }),
                    },
                )),
            ),
        );
        // Context-window recovery can change the provider-visible transcript
        // without changing provider/model identity. Capture that fact
        // independently from final-response fallback attribution. Box before
        // entering either task-local scope: boxing inside a nested async block
        // still captures the large turn-loop future on the worker stack.
        let turn_loop = Box::pin(turn_loop);
        let (
            loop_result,
            turn_provider_recovery,
            turn_provider_context_truncated,
            turn_safeguard_fallback,
        ) = clawcrew_providers::scope_safeguard_fallback(async {
            let (result, recovery, context_truncated) =
                clawcrew_providers::reliable::scope_provider_fallback(async {
                    let result = crate::agent::turn::scope_tool_protocol_prompts(
                        Arc::clone(&tool_protocol_prompts),
                        turn_loop,
                    )
                    .await;
                    (
                        result,
                        clawcrew_providers::reliable::take_last_provider_fallback(),
                        clawcrew_providers::reliable::take_last_provider_context_truncation(),
                    )
                })
                .await;
            (
                result,
                recovery,
                context_truncated,
                clawcrew_providers::take_last_safeguard_fallback(),
            )
        })
        .await;

        // Feed the accumulated per-call usage into the AgentEnd guard before
        // any return below drops it — including the error path, which must
        // still report usage from calls that succeeded earlier in the turn.
        let usage = cost_context.snapshot_turn_usage();
        if usage.input_tokens > 0 || usage.output_tokens > 0 {
            guard.set_usage(
                Some(clawcrew_api::observability_traits::TurnTokenUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                }),
                None,
            );
        }
        // Write back any token-budget trim that happened inside the loop to
        // durable history. `loop_history` is the TurnState's history which
        // after `sync_pending` already contains the canonical current turn
        // (user+assistant...), so `loop_history.len()` includes both the
        // prefix and the canonical. To detect a trim we must compare only
        // the prefix part, not the full length which always grows via
        // `sync_pending` and tool appends.
        let new_prefix_len = loop_history.len().saturating_sub(loop_new_messages.len());
        let history_trimmed_in_loop = new_prefix_len != original_loop_history_len
            || loop_history_crumb_present != original_loop_history_crumb;
        if history_trimmed_in_loop {
            // The loop's history is already the authoritative full transcript
            // (trimmed prefix + canonical). It already contains the user and
            // assistant messages, so we can replay it directly without
            // appending `loop_new_messages` a second time — doing so duplicated
            // the current turn (5 messages instead of 3).
            //
            // This is the mutated history buffer, the only one that can
            // carry the injected preamble: the current turn's user message
            // opens the canonical tail, which starts at `new_prefix_len`.
            // The positional confirmation inside replay still verifies the
            // message before stripping it.
            let injected =
                loop_injected_memory_preamble
                    .as_deref()
                    .map(|preamble| MemoryPreambleTarget {
                        preamble,
                        index: new_prefix_len,
                    });
            self.history.clear();
            self.history
                .extend(Self::replay_loop_messages(&loop_history, injected));
            self.history_has_trim_breadcrumb = loop_history_crumb_present;
            self.history_trim_generation = self.history_trim_generation.wrapping_add(1);
        } else {
            // No trim: the loop did not change the prefix. Pop the pre-loop
            // enriched user message and replay the canonical (which may be the
            // request-enriched form, not the raw `enriched` we pushed).
            // `loop_new_messages` is a pre-injection clone the loop's memory
            // injection never touches (it mutates `loop_history` in place,
            // and only ever pushes to this buffer, never replaces it), so
            // no strip target is passed: an uninjected clone must replay
            // byte-for-byte even when the user's original text starts with
            // the recorded preamble.
            self.history.pop();
            for replayed in Self::replay_loop_messages(&loop_new_messages, None) {
                self.history.push(replayed);
            }
        }
        let response = match loop_result {
            Ok(response) => response,
            Err(error) => {
                let _ = self.trim_history(Some(&turn_id));
                return Err(error);
            }
        };

        let response = self.append_receipts_block(response, receipt_scope.as_ref());
        // The ordinary recovery leg is rendered first so the route reads in
        // order: an ordinary provider fallback, then any safeguard switch the
        // accepted attempt itself went through.
        let response = Self::format_model_fallback_notice(
            response,
            clawcrew_providers::visible_provider_fallback(
                turn_provider_recovery.as_ref(),
                turn_safeguard_fallback.as_ref(),
            ),
        );
        let response = crate::agent::append_safeguard_fallback_notice(
            response,
            turn_safeguard_fallback.as_ref(),
        );

        // Store in the response cache only when the turn was a single
        // tool-free exchange (exactly one assistant message), mirroring the
        // old "no tool calls" put condition.
        if let (Some(cache), Some(key)) = (&self.response_cache, &cache_key)
            && turn_provider_recovery.is_none()
            && turn_safeguard_fallback.is_none()
            && !turn_provider_context_truncated
            && loop_new_messages.len() == 2
            && loop_new_messages
                .last()
                .is_some_and(|m| m.role == "assistant")
        {
            #[allow(clippy::cast_possible_truncation)]
            let _ = cache.put(key, &effective_model, &response, usage.output_tokens as u32);
        }

        let _ = self.trim_history(Some(&turn_id));

        Ok(response)
    }

    pub async fn turn_streamed(
        &mut self,
        user_message: &str,
        event_tx: tokio::sync::mpsc::Sender<TurnEvent>,
        cancel_token: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<(String, Vec<ConversationMessage>)> {
        // See `Agent::turn` for the rationale. Same guard: blank input would
        // push a timestamp-only user message into history and the model would
        // narrate the trailing prompt-template sentinel instead of replying.
        if user_message.trim().is_empty() {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "reason": "empty_user_message",
                        "entry_point": "Agent::turn_streamed",
                        "raw_len": user_message.len(),
                    })),
                "Refusing blank user turn (would emit timestamp-only message and risk prompt-template bleed-through)"
            );
            return Err(anyhow::Error::msg(
                "empty user message: refusing to dispatch a blank turn",
            ));
        }

        let display_event_tx = event_tx.clone();
        let StreamedTurnSuccess {
            response: raw_response,
            new_messages,
            safeguard_fallback,
            ..
        } = self
            .turn_streamed_with_steering_state(user_message, event_tx, cancel_token, None)
            .await
            .map_err(|err| err.error)?;
        let response = crate::agent::append_safeguard_fallback_notice(
            raw_response.clone(),
            safeguard_fallback.as_ref(),
        );
        if safeguard_fallback.is_some()
            && let Some(delta) = response.strip_prefix(&raw_response)
        {
            let _ = display_event_tx
                .send(TurnEvent::Chunk {
                    delta: delta.to_string(),
                })
                .await;
        }
        Ok((response, new_messages))
    }

    pub async fn turn_streamed_with_steering_state(
        &mut self,
        user_message: &str,
        event_tx: tokio::sync::mpsc::Sender<TurnEvent>,
        cancel_token: Option<tokio_util::sync::CancellationToken>,
        mut steering_rx: Option<&mut tokio::sync::mpsc::Receiver<String>>,
    ) -> std::result::Result<StreamedTurnSuccess, StreamedTurnError> {
        // See `Agent::turn` for the rationale. Same guard: blank input would
        // push a timestamp-only user message into history and the model would
        // narrate the trailing prompt-template sentinel instead of replying.
        if user_message.trim().is_empty() {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "reason": "empty_user_message",
                        "entry_point": "Agent::turn_streamed_with_steering_state",
                        "raw_len": user_message.len(),
                    })),
                "Refusing blank user turn (would emit timestamp-only message and risk prompt-template bleed-through)"
            );
            return Err(StreamedTurnError {
                error: anyhow::Error::msg("empty user message: refusing to dispatch a blank turn"),
                committed_response: String::new(),
                new_messages: Vec::new(),
            });
        }

        // ── Preamble (identical to turn) ───────────────────────────────
        if self.history.is_empty() {
            let system_prompt = self
                .build_system_prompt()
                .map_err(|error| StreamedTurnError {
                    error,
                    committed_response: String::new(),
                    new_messages: Vec::new(),
                })?;
            self.history
                .push(ConversationMessage::Chat(ChatMessage::system(
                    system_prompt,
                )));
        }

        let mut new_msgs: Vec<ConversationMessage> = Vec::new();
        // Pin one config generation for this whole turn BEFORE resolving a
        // route, so a mid-turn `model_switch` rebuilds the provider from the
        // same generation the limits below are resolved from.
        self.sync_config_generation();
        // `effective_model` is `mut` so a `model_switch` requested mid-turn
        // (handled in the round loop's `ModelSwitchRequested` arm via
        // `try_apply_model_switch`) can rebind it for later rounds
        let mut effective_model = self.classify_model(user_message);
        let mut selected_route = self.model_route_resolver.resolve(&effective_model);
        let turn_id = Self::new_turn_id();
        let mut committed_response = String::new();
        // Requested-vs-served divergence for THIS turn. Source of truth is the
        // task-local record inside `clawcrew_providers::reliable`, consumed
        // once per round below; this is a per-turn transient resolved at
        // use-time, never stored on the agent.
        let mut turn_provider_recovery: Option<clawcrew_providers::reliable::ProviderFallbackInfo>;
        let mut turn_safeguard_fallback: Option<clawcrew_providers::SafeguardFallbackNotice>;
        let mut turn_provider_context_truncated = false;
        let turn_observer = Arc::clone(&self.observer);
        let mut guard = crate::observability::AgentTurnGuard::start(
            turn_observer.as_ref(),
            selected_route.provider_name.clone(),
            selected_route.model.clone(),
            Some(self.channel_name.clone()),
            self.observer_agent_alias(),
            Some(turn_id.clone()),
        );
        self.append_streamed_user_message_to_history(user_message, &mut new_msgs, &turn_id)
            .await;

        let active_dispatcher = {
            let base_provider_messages = self.tool_dispatcher.to_provider_messages(&self.history);
            let (vision_provider_box, _degrade_strip_images) =
                match crate::agent::turn::resolve_vision_provider(
                    self.full_config(),
                    self.model_provider.as_ref(),
                    &base_provider_messages,
                    &self.multimodal_config,
                    &selected_route.provider_name,
                    &selected_route.model,
                    &effective_model,
                ) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        let notice = self.trim_history(Some(&turn_id));
                        forward_history_trim_notice(&event_tx, notice).await;
                        return Err(StreamedTurnError {
                            error,
                            committed_response: String::new(),
                            new_messages: new_msgs,
                        });
                    }
                };
            let (active_provider, active_model): (&dyn ModelProvider, &str) =
                vision_provider_box.as_ref().map_or(
                    (self.model_provider.as_ref(), effective_model.as_str()),
                    |resolved| (resolved.provider.as_ref(), resolved.model.as_str()),
                );
            tool_dispatcher_for_provider(&self.config, active_provider, active_model)
        };

        if let Err(error) = self.rebuild_system_prompt_for_dispatcher(active_dispatcher.as_ref()) {
            let notice = self.trim_history(Some(&turn_id));
            forward_history_trim_notice(&event_tx, notice).await;
            return Err(StreamedTurnError {
                error,
                committed_response: String::new(),
                new_messages: new_msgs,
            });
        }
        let tool_protocol_prompts = match self.tool_protocol_prompts() {
            Ok(prompts) => prompts,
            Err(error) => {
                let notice = self.trim_history(Some(&turn_id));
                forward_history_trim_notice(&event_tx, notice).await;
                return Err(StreamedTurnError {
                    error,
                    committed_response: String::new(),
                    new_messages: new_msgs,
                });
            }
        };

        let provider_messages = active_dispatcher.to_provider_messages(&self.history);
        let cache_key = self.response_cache_key_for_messages(&provider_messages, &effective_model);

        if let (Some(cache), Some(key)) = (&self.response_cache, &cache_key) {
            if let Ok(Some(cached)) = cache.get(key) {
                self.observer.record_event(&ObserverEvent::CacheHit {
                    cache_type: "response".into(),
                    tokens_saved: 0,
                });
                let cached_msg = ConversationMessage::Chat(ChatMessage::assistant(cached.clone()));
                new_msgs.push(cached_msg.clone());
                self.history.push(cached_msg);
                let notice = self.trim_history(Some(&turn_id));
                forward_history_trim_notice(&event_tx, notice).await;
                self.observer.record_event(&ObserverEvent::TurnComplete);
                committed_response.push_str(&cached);
                return Ok(StreamedTurnSuccess {
                    response: committed_response,
                    new_messages: new_msgs,
                    provider_name: selected_route.provider_name.clone(),
                    model: selected_route.model.clone(),
                    // Cache hit: no LLM call was served, so there is no
                    // per-call route snapshot. The gateway falls back to
                    // resolving limits from the selected route.
                    final_context_limits: None,
                    safeguard_fallback: None,
                });
            }
            self.observer.record_event(&ObserverEvent::CacheMiss {
                cache_type: "response".into(),
            });
        }

        // Split provider_messages: loop_history gets past turns, while
        // user_msg_for_loop seeds round 0's canonical replay buffer. Request
        // hooks mutate only the per-iteration provider snapshot.
        let split_idx = provider_messages
            .iter()
            .rposition(|m| m.role == "user")
            .unwrap_or(provider_messages.len());
        let mut loop_history = provider_messages[..split_idx].to_vec();
        let mut streamed_original_loop_history_len = loop_history.len();
        let mut streamed_original_crumb = self.history_has_trim_breadcrumb;
        // Seed raw-transcript crumb provenance from the structured history's
        // owner-tracked state (the conversion preserves the crumb position).
        let mut loop_history_crumb_present = self.history_has_trim_breadcrumb;
        let mut loop_injected_memory_preamble: Option<String> = None;
        let user_msg_for_loop: Vec<ChatMessage> = provider_messages[split_idx..].to_vec();
        // Track total canonical ChatMessage length so prefix detection is not
        // confused by `sync_pending` which always grows `loop_history` via the
        // canonical. After each round, prefix_len = loop_history.len() - total_canonical_len.
        let mut total_canonical_len = 0usize;
        let approval_bridge: Option<Box<dyn clawcrew_api::channel::Channel>> =
            self.channel_handles.ask_user.as_ref().map(|handles| {
                Box::new(crate::agent::approval_bridge::AskUserApprovalBridge::new(
                    Arc::clone(handles),
                    self.approval_route.clone(),
                )) as Box<dyn clawcrew_api::channel::Channel>
            });

        let knobs = crate::agent::loop_::LoopKnobs {
            dedup_enabled: false,
            max_iteration_behavior: crate::agent::loop_::MaxIterationBehavior::GracefulSummary,
            detect_protocol_without_tools: false,
            draft_reasoning: clawcrew_config::schema::StreamReasoningMode::Status,
        };
        // The streaming engine never had pattern-based loop detection; default
        // pacing turns it on. Keep the embedder contract until this surface
        // grows a pacing config of its own (matches `Agent::turn`).
        let pacing = clawcrew_config::schema::PacingConfig {
            loop_detection_enabled: false,
            ..clawcrew_config::schema::PacingConfig::default()
        };

        let cost_context = self.tool_loop_cost_tracking_context();
        let agent_alias_for_loop = self.observer_agent_alias();

        // Built once per turn so the HMAC key is stable across steering rounds
        // and the same collector accumulates every round's receipts. `None`
        // when receipts are disabled, gated by the one shared seam.
        let receipt_scope = crate::agent::tool_receipts::ReceiptScope::from_config(
            &self.config.resolved.tool_receipts,
        );

        // ── Round loop: one tool-call-loop run per steering round ──────────
        // Sink the loop writes the final serving route into each round, so the
        // terminal `StreamedTurnSuccess` carries the route/limits that actually
        // served the last call — including a per-call vision switch — even when
        // the provider returned no usage. Carried across rounds; the last write
        // wins.
        let served_route_sink: crate::agent::loop_::ServedRouteSink =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        for round in 0..self.config.resolved.max_tool_iterations {
            // Early exit if the caller cancelled this turn (e.g. user abort)
            if cancel_token
                .as_ref()
                .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
            {
                let marker = crate::i18n::get_required_cli_string("turn-interrupted-by-user");
                let interruption =
                    ConversationMessage::Chat(ChatMessage::assistant(marker.clone()));
                new_msgs.push(interruption.clone());
                self.history.push(interruption);
                committed_response.push_str(&marker);
                let notice = self.trim_history(Some(&turn_id));
                forward_history_trim_notice(&event_tx, notice).await;
                return Err(StreamedTurnError {
                    error: crate::agent::loop_::ToolLoopCancelled.into(),
                    committed_response,
                    new_messages: new_msgs,
                });
            }

            let mut round_added: Vec<ChatMessage> = if round == 0 {
                user_msg_for_loop.clone()
            } else {
                Vec::new()
            };

            // Steering drain: each accepted mid-turn message becomes its own
            // enriched user turn in both transcripts before the next round.
            for steering_message in crate::agent::loop_::drain_steering_messages(&mut steering_rx) {
                // Mirror the enrichment logic from append_streamed_user_message_to_history
                // but route through round_added instead of self.history/new_msgs.
                if self.auto_save {
                    let store_start = std::time::Instant::now();
                    let store_result = self
                        .memory
                        .store(
                            "user_msg",
                            &steering_message,
                            MemoryCategory::Conversation,
                            self.memory_session_id.as_deref(),
                        )
                        .await;
                    self.observer.record_event(&ObserverEvent::MemoryStore {
                        category: MemoryCategory::Conversation.to_string(),
                        backend: self.memory.name().to_string(),
                        duration: store_start.elapsed(),
                        success: store_result.is_ok(),
                        channel: Some(self.channel_name.clone()),
                        agent_alias: self.observer_agent_alias(),
                        turn_id: Some(turn_id.clone()),
                    });
                }
                let enriched = self.enrich_user_message(&steering_message);
                round_added.push(ChatMessage::user(enriched));
            }
            let round_loop = crate::agent::loop_::TOOL_LOOP_COST_TRACKING_CONTEXT.scope(
                Some(cost_context.clone()),
                crate::agent::tool_receipts::scope_receipts(
                    receipt_scope.clone(),
                    Box::pin(crate::agent::loop_::run_tool_call_loop(
                        crate::agent::loop_::ToolLoop {
                            exec: crate::agent::loop_::ResolvedAgentExecution::resolve(
                                crate::agent::loop_::ResolvedModelAccess {
                                    model_provider: self.model_provider.as_ref(),
                                    provider_name: &selected_route.provider_name,
                                    model: &selected_route.model,
                                    dispatch_model: &effective_model,
                                    temperature: self.temperature,
                                },
                                crate::agent::loop_::ResolvedIo {
    app_registry: None,
                                    tools_registry: &self.tools,
                                    observer: self.observer.as_ref(),
                                    silent: true,
                                    approval: self.approval_manager.as_deref(),
                                    multimodal_config: &self.multimodal_config,
                                    // Inlined `full_config()` (per-field borrow) so it coexists with
                                    // the `&mut self.image_cache` in this same ToolLoop expression.
                                    config: self
                                        .provider_switch_config
                                        .as_ref()
                                        .and_then(|c| c.config.as_deref()),
                                    hooks: self.hook_runner.as_deref(),
                                    activated_tools: self.activated_tools.as_ref(),
                                    // `None` here (rather than a shared global) is
                                    // deliberate: `run_tool_call_loop` mints a fresh,
                                    // task-local switch state for this round when it
                                    // sees `None`, so a `model_switch` requested this
                                    // round can never leak into a sibling round or a
                                    // concurrently running turn/agent.
                                    model_switch_callback: None,
                                    receipt_generator: receipt_scope
                                        .as_ref()
                                        .map(crate::agent::tool_receipts::ReceiptScope::generator),
                                },
                                crate::agent::loop_::ResolvedRuntimeKnobs {
                                    max_tool_iterations: self.config.resolved.max_tool_iterations,
                                    excluded_tools: &[],
                                    dedup_exempt_tools: &self
                                        .config
                                        .resolved
                                        .tool_call_dedup_exempt,
                                    pacing: &pacing,
                                    strict_tool_parsing: self.config.resolved.strict_tool_parsing,
                                    parallel_tools: self.config.resolved.parallel_tools,
                                    max_tool_result_chars: self
                                        .config
                                        .resolved
                                        .max_tool_result_chars,
                                    // Fallback pair for the loop when no resolver is
                                    // wired; when `context_limits_resolver` is set
                                    // the loop re-resolves per call, so seed with the
                                    // resolver-free config limits instead of invoking
                                    // the resolver a second time here.
                                    context_limits: self.config.resolved.context_limits(),
                                    context_limits_resolver: self.context_limits_resolver.clone(),
                                    knobs: &knobs,
                                },
                            ),
                            history: &mut loop_history,
                            history_has_trim_breadcrumb: &mut loop_history_crumb_present,
                            injected_memory_preamble: &mut loop_injected_memory_preamble,
                            channel_name: &self.channel_name,
                            channel_reply_target: None,
                            cancellation_token: cancel_token.clone(),
                            on_delta: None,
                            shared_budget: None,
                            channel: approval_bridge.as_deref(),
                            collected_receipts: receipt_scope
                                .as_ref()
                                .map(crate::agent::tool_receipts::ReceiptScope::collector),
                            event_tx: Some(event_tx.clone()),
                            steering: None,
                            new_messages_out: Some(&mut round_added),
                            image_cache: Some(&mut self.image_cache),
                            // Direct embedded Agent::turn call; source/transport/
                            // trust stay placeholders, not yet stamped at the edge.
                            memory: Some(crate::agent::memory_inject::TurnMemory {
                                handle: self.memory.as_ref(),
                                query: user_message.to_string(),
                                sessions: vec![self.memory_session_id.clone()],
                                suppress: false,
                                cfg: self.memory_inject_cfg,
                            }),
                            ingress: clawcrew_api::ingress::IngressContext::agent_direct(),
                            agent_alias: agent_alias_for_loop.as_deref(),
                            parent_agent_alias: None,
                            turn_id: &turn_id,
                            served_route_sink: Some(served_route_sink.clone()),
                            // Live-daemon SOP path: re-assemble a nested step's
                            // agent when it delegates elsewhere. Config survives
                            // only via `provider_switch_config`; with `None`
                            // (test builder) a cross-agent step FAILS CLOSED
                            // rather than inheriting this turn's context.
                            sop_reassembly: self
                                .provider_switch_config
                                .as_ref()
                                .and_then(|c| c.config.as_deref())
                                .map(|config| crate::agent::turn::SopStepReassembly { config }),
                        },
                    )),
                ),
            );
            // Scope the provider-fallback task-local around the round so the
            // resilient wrapper's requested-vs-served record is visible here,
            // then read it immediately. Box before adding the prompt scope so
            // the nested task-locals do not capture the full round future on
            // the worker stack in debug builds.
            let round_loop = Box::pin(round_loop);
            let (loop_result, round_fallback, round_context_truncated, round_safeguard) =
                clawcrew_providers::scope_safeguard_fallback(async {
                    let (result, fallback, context_truncated) =
                        clawcrew_providers::reliable::scope_provider_fallback(async {
                            let result = crate::agent::turn::scope_tool_protocol_prompts(
                                Arc::clone(&tool_protocol_prompts),
                                round_loop,
                            )
                            .await;
                            (
                                result,
                                clawcrew_providers::reliable::take_last_provider_fallback(),
                                clawcrew_providers::reliable::take_last_provider_context_truncation(
                                ),
                            )
                        })
                        .await;
                    (
                        result,
                        fallback,
                        context_truncated,
                        clawcrew_providers::take_last_safeguard_fallback(),
                    )
                })
                .await;
            // Each accepted round owns the recovery presentation state. A
            // later primary/direct response must clear an earlier fallback,
            // rather than leaving its notice attached to the final answer.
            turn_provider_recovery = round_fallback;
            turn_safeguard_fallback = round_safeguard;
            turn_provider_context_truncated |= round_context_truncated;

            // Feed cumulative usage into the AgentEnd guard before any return
            // below drops it — the error paths must still report usage from
            // calls that succeeded earlier in the turn.
            let usage = cost_context.snapshot_turn_usage();
            if usage.input_tokens > 0 || usage.output_tokens > 0 {
                guard.set_usage(
                    Some(clawcrew_api::observability_traits::TurnTokenUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                    }),
                    None,
                );
            }

            // round_added now contains the user message for round 0;
            // a single tool-free exchange is [user, assistant].
            let single_text_exchange = round == 0
                && round_added.len() == 2
                && round_added.first().is_some_and(|m| m.role == "user")
                && round_added.last().is_some_and(|m| m.role == "assistant");

            if round == 0 {
                self.history.pop();
                new_msgs.pop();
            }
            // `round_added` is a pre-injection clone the loop's memory
            // injection never touches (it mutates `loop_history` in place,
            // once, before round 0, and only ever pushes to the canonical
            // buffer), so no strip target is passed here either: even the
            // round-0 user message it carries for a single tool-free
            // exchange is the clean clone, and must replay byte-for-byte.
            for replayed in Self::replay_loop_messages(&round_added, None) {
                new_msgs.push(replayed.clone());
                self.history.push(replayed);
            }
            total_canonical_len += round_added.len();
            // Write back durable token-budget trim from loop_history.
            // `loop_history` after this round is [trimmed_prefix + all canonical ChatMessages so far]
            // `total_canonical_len` tracks the ChatMessage length of all canonical so far,
            // so prefix_len = loop_history.len() - total_canonical_len.
            let new_prefix_len = loop_history.len().saturating_sub(total_canonical_len);
            if new_prefix_len != streamed_original_loop_history_len
                || loop_history_crumb_present != streamed_original_crumb
            {
                // The prefix was trimmed (old turns dropped or crumb inserted).
                // Rebuild durable history from the authoritative loop_history
                // which already contains the trimmed prefix + canonical. As
                // above, this is the mutated buffer: the injected message,
                // when retained, opens the canonical tail at `new_prefix_len`.
                let injected =
                    loop_injected_memory_preamble
                        .as_deref()
                        .map(|preamble| MemoryPreambleTarget {
                            preamble,
                            index: new_prefix_len,
                        });
                self.history.clear();
                self.history
                    .extend(Self::replay_loop_messages(&loop_history, injected));
                self.history_has_trim_breadcrumb = loop_history_crumb_present;
                self.history_trim_generation = self.history_trim_generation.wrapping_add(1);
                streamed_original_loop_history_len = new_prefix_len;
                streamed_original_crumb = loop_history_crumb_present;
            }

            match loop_result {
                Ok(response) => {
                    // Commit-before-drain: this round's assistant output is in
                    // history/new_msgs (replay above) and committed_response
                    // before any steering continuation is folded in.
                    committed_response.push_str(&response);
                    let notice = self.trim_history(Some(&turn_id));
                    forward_history_trim_notice(&event_tx, notice).await;

                    let has_more_steering =
                        steering_rx.as_deref_mut().is_some_and(|rx| !rx.is_empty());
                    if has_more_steering {
                        continue;
                    }

                    // Cache put only when the turn was a single tool-free
                    // exchange, mirroring the old "no tool calls" condition.
                    if single_text_exchange
                        && turn_provider_recovery.is_none()
                        && turn_safeguard_fallback.is_none()
                        && !turn_provider_context_truncated
                        && let (Some(cache), Some(key)) = (&self.response_cache, &cache_key)
                    {
                        #[allow(clippy::cast_possible_truncation)]
                        let _ =
                            cache.put(key, &effective_model, &response, usage.output_tokens as u32);
                    }

                    self.observer.record_event(&ObserverEvent::TurnComplete);
                    let committed_response =
                        self.append_receipts_block(committed_response, receipt_scope.as_ref());
                    let committed_response = Self::append_model_fallback_notice(
                        committed_response,
                        turn_provider_recovery.as_ref(),
                        turn_safeguard_fallback.as_ref(),
                        &event_tx,
                    )
                    .await;
                    // Prefer the route that actually served the final call
                    // (a per-call vision switch differs from the selected text
                    // route); fall back to the selected route when no call was
                    // served this turn.
                    let served = served_route_sink
                        .lock()
                        .expect("served-route sink lock")
                        .clone();
                    let (final_provider, final_model, final_limits) = match served {
                        Some(route) => {
                            (route.provider_name, route.model, Some(route.context_limits))
                        }
                        None => (
                            selected_route.provider_name.clone(),
                            selected_route.model.clone(),
                            None,
                        ),
                    };
                    return Ok(StreamedTurnSuccess {
                        response: committed_response,
                        new_messages: new_msgs,
                        provider_name: final_provider,
                        model: final_model,
                        final_context_limits: final_limits,
                        safeguard_fallback: turn_safeguard_fallback,
                    });
                }
                Err(error) => {
                    // Model switch requested mid-turn: the unified loop
                    // signals a pending `model_switch` by returning
                    // `ModelSwitchRequested`. The
                    // round's tool call + result are already replayed into
                    // history/new_msgs above; rebuild the provider from the
                    // captured `ProviderSwitchConfig` and continue the round
                    // loop so the next provider call uses the switched
                    // provider/model. A failed rebuild (no switch config / build
                    // error) falls through to the normal error handling below.
                    if let Some((new_model_provider, new_model)) =
                        crate::agent::loop_::is_model_switch_requested(&error)
                        && let Some(new_effective_model) = self.try_apply_model_switch(
                            &effective_model,
                            new_model_provider,
                            new_model,
                        )
                    {
                        if let Err(error) = self
                            .rebuild_streamed_system_prompt_for_active_provider(&mut loop_history)
                        {
                            let notice = self.trim_history(Some(&turn_id));
                            forward_history_trim_notice(&event_tx, notice).await;
                            return Err(StreamedTurnError {
                                error,
                                committed_response,
                                new_messages: new_msgs,
                            });
                        }
                        let notice = self.trim_history(Some(&turn_id));
                        forward_history_trim_notice(&event_tx, notice).await;
                        effective_model = new_effective_model;
                        selected_route = self.model_route_resolver.resolve(&effective_model);
                        continue;
                    }
                    // Rebuild the committed text from the failed round's plain
                    // assistant output (e.g. a persisted stream partial) when
                    // no prior round committed anything.
                    if committed_response.is_empty() {
                        for replayed in Self::replay_loop_messages(&round_added, None) {
                            if let ConversationMessage::Chat(message) = &replayed
                                && message.role == "assistant"
                            {
                                committed_response.push_str(&message.content);
                            }
                        }
                    }
                    let error = if crate::agent::loop_::is_tool_loop_cancelled(&error) {
                        // When the cancel arrived after event-visible
                        // streamed text, the error itself carries the
                        // partial the loop persisted (replayed into
                        // history/new_msgs above, and into
                        // committed_response by the empty-committed
                        // rebuild). Provenance, not content sniffing:
                        // model-authored text can end with the marker
                        // literal, so suffix-matching round_added would
                        // misfire. Synthesize the bare marker only when no
                        // interruption text was persisted this round.
                        let marker =
                            crate::i18n::get_required_cli_string("turn-interrupted-by-user");
                        let persisted_interruption = error
                            .downcast_ref::<crate::agent::loop_::StreamCancelledAfterOutput>()
                            .map(|cancelled| format!("{}\n\n{marker}", cancelled.partial_text));
                        match persisted_interruption {
                            Some(text) => {
                                if !committed_response.ends_with(&marker) {
                                    if !committed_response.is_empty() {
                                        committed_response.push_str("\n\n");
                                    }
                                    committed_response.push_str(&text);
                                }
                            }
                            None => {
                                committed_response.push_str(&marker);
                                let interruption = ConversationMessage::Chat(
                                    ChatMessage::assistant(marker.clone()),
                                );
                                new_msgs.push(interruption.clone());
                                self.history.push(interruption);
                            }
                        }
                        crate::agent::loop_::ToolLoopCancelled.into()
                    } else {
                        // Mark the interruption only when nothing was committed —
                        // prior-round text must round-trip unmodified.
                        if committed_response.is_empty() {
                            committed_response.push_str(&crate::i18n::get_required_cli_string(
                                "turn-stream-interrupted",
                            ));
                        }
                        error
                    };
                    let notice = self.trim_history(Some(&turn_id));
                    forward_history_trim_notice(&event_tx, notice).await;
                    return Err(StreamedTurnError {
                        error,
                        committed_response,
                        new_messages: new_msgs,
                    });
                }
            }
        }

        let notice = self.trim_history(Some(&turn_id));
        forward_history_trim_notice(&event_tx, notice).await;
        Err(StreamedTurnError {
            error: anyhow::Error::msg(format!(
                "Agent exceeded maximum tool iterations ({})",
                self.config.resolved.max_tool_iterations
            )),
            committed_response,
            new_messages: new_msgs,
        })
    }

    pub async fn run_single(&mut self, message: &str) -> Result<String> {
        self.turn(message).await
    }

    pub async fn run_interactive(&mut self) -> Result<()> {
        println!("🦀 ClawCrew Interactive Mode");
        println!("Type /quit to exit.\n");

        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let cli = crate::agent::loop_::CLI_CHANNEL_FN
            .get()
            .expect("CLI channel factory not registered — call register_cli_channel_fn at startup")(
        );

        let listen_handle = clawcrew_spawn::spawn!(async move {
            let _ = clawcrew_api::channel::Channel::listen(&*cli, tx).await;
        });

        while let Some(msg) = rx.recv().await {
            let response = match self.turn(&msg.content).await {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("\nError: {e}\n");
                    continue;
                }
            };
            println!("\n{response}\n");
        }

        listen_handle.abort();
        Ok(())
    }
}

pub async fn run(
    config: Config,
    agent_alias: &str,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: Option<f64>,
) -> Result<()> {
    let mut effective_config = config;
    if let Some(ref p) = provider_override {
        // When a model_provider override is specified, ensure that model_provider type exists
        // in models and update the agent's model_provider to reference it.
        let (type_key, alias_key) = p.split_once('.').unwrap_or((p.as_str(), agent_alias));
        effective_config
            .providers
            .models
            .ensure(type_key, alias_key);
        if let Some(agent_cfg) = effective_config.agents.get_mut(agent_alias) {
            agent_cfg.model_provider = format!("{type_key}.{alias_key}").into();
        }
    }
    // Apply model/temperature overrides to the agent's resolved provider entry.
    if let Some(agent_cfg) = effective_config.agents.get(agent_alias)
        && let Some((fam, ali)) = agent_cfg.model_provider.split_once('.')
        && let Some(entry) = effective_config.providers.models.ensure(fam, ali)
    {
        if let Some(m) = model_override {
            entry.model = Some(m);
        }
        entry.temperature = temperature;
    }

    let mut agent = Agent::from_config(&effective_config, agent_alias).await?;

    if let Some(msg) = message {
        let response = agent.run_single(&msg).await?;
        println!("{response}");
    } else {
        agent.run_interactive().await?;
    }

    Ok(())
}

// safety net (child module so fixtures can reach Agent internals the
// same way `mod tests` does).
#[cfg(test)]
#[path = "safety_net.rs"]
mod safety_net;

#[cfg(test)]
#[path = "parity.rs"]
mod parity;

// Live-config plugin regression (child module so it can read the constructed
// Agent's tool registry the same way `mod tests` does). Needs a WASM compiler
// on the host: `WasmTool::from_wasm` refuses to register a tool it cannot load,
// so a runtime-only plugin backend has no plugin tool to execute.
#[cfg(all(test, feature = "plugins-wasm-cranelift"))]
#[path = "plugin_live_config.rs"]
mod plugin_live_config;

#[cfg(test)]
mod tests;

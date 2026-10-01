use crate::agent::dispatcher::{ToolDispatcher, XmlToolDispatcher};
use crate::agent::loop_::{
    LoopKnobs, ResolvedAgentExecution, ResolvedIo, ResolvedModelAccess, ResolvedRuntimeKnobs,
    TOOL_LOOP_SESSION_KEY, TOOL_LOOP_THREAD_ID, ToolLoop, apply_text_tool_prompt_policy,
    run_tool_call_loop,
};
use crate::agent::prompt::{PromptContext, SystemPromptBuilder};
use crate::approval::{ApprovalManager, ApprovalRequirement};
use crate::observability::traits::{Observer, ObserverEvent, ObserverMetric};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use clawcrew_api::tool::{Tool, ToolOutput, ToolResult};
use clawcrew_config::schema::{
    AliasedAgentConfig, Config, DelegateExecutionMode, DelegateToolConfig, ModelProviderConfig,
    ResolvedRuntime, RiskProfileConfig, RuntimeProfileConfig, SkillBundleConfig,
};
use clawcrew_log::Instrument as _;
use clawcrew_memory::Memory;
use clawcrew_providers::{self, ChatMessage, ModelProvider, ProviderDispatch};
use clawcrew_tools::memory_export::MemoryExportTool;
use clawcrew_tools::memory_forget::MemoryForgetTool;
use clawcrew_tools::memory_purge::MemoryPurgeTool;
use clawcrew_tools::memory_recall::MemoryRecallTool;
use clawcrew_tools::memory_store::MemoryStoreTool;

fn current_tool_loop_session_key() -> Option<String> {
    TOOL_LOOP_SESSION_KEY.try_with(Clone::clone).ok().flatten()
}

fn invalid_semantic_completion_error(agent_name: &str) -> String {
    crate::agent::turn::outcome::semantic_empty_terminal_completion_message(Some(agent_name))
}

fn delegate_failure_error(agent_name: &str, error: &anyhow::Error) -> String {
    if error
        .chain()
        .any(|source| source.is::<clawcrew_providers::ReliableProviderTerminalFailure>())
    {
        // Reliable's aggregate is the durable retry diagnostic for delegated
        // task records; other typed terminal failures use the delivery projection.
        return format!("Agent '{agent_name}' failed: {error}");
    }

    crate::agent::turn::outcome::terminal_completion_error_message(error, Some(agent_name))
        .unwrap_or_else(|| format!("Agent '{agent_name}' failed: {error}"))
}

async fn scope_delegate_session_key<F>(session_key: Option<String>, future: F) -> F::Output
where
    F: std::future::Future,
{
    TOOL_LOOP_SESSION_KEY.scope(session_key, future).await
}

tokio::task_local! {
    /// Ambient delegate task id for a background worker, so a provider fallback
    /// observed mid-run is recorded as task metadata (P1.5) and surfaces in the
    /// TaskBoard activity feed.
    static DELEGATE_TASK_ID: String;
}

/// Serializable result of a background delegate task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackgroundDelegateResult {
    pub task_id: String,
    pub agent: String,
    pub status: BackgroundTaskStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// Output artifact written by current background delegates.
///
/// Lifecycle status intentionally lives only in the control-plane task row.
/// `BackgroundDelegateResult` remains the compatibility shape for legacy files
/// that embedded their own status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BackgroundDelegateOutput {
    task_id: String,
    output: Option<String>,
}

struct StoredBackgroundOutput {
    output: BackgroundDelegateOutput,
    legacy_agent: Option<String>,
    legacy_status: Option<BackgroundTaskStatus>,
    legacy_error: Option<String>,
    legacy_started_at: Option<String>,
    legacy_finished_at: Option<String>,
}

/// Status of a background delegate task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackgroundResultState {
    Running,
    Completed,
    Failed,
    Cancelled,
    Lost,
    TimedOut,
}

impl BackgroundResultState {
    fn from_file_status(status: &BackgroundTaskStatus) -> Self {
        match status {
            BackgroundTaskStatus::Running => Self::Running,
            BackgroundTaskStatus::Completed => Self::Completed,
            BackgroundTaskStatus::Failed => Self::Failed,
            BackgroundTaskStatus::Cancelled => Self::Cancelled,
        }
    }

    fn from_task_status(status: crate::control_plane::TaskStatus) -> Self {
        match status {
            crate::control_plane::TaskStatus::Queued
            | crate::control_plane::TaskStatus::Waiting
            | crate::control_plane::TaskStatus::Validating
            | crate::control_plane::TaskStatus::Retrying
            | crate::control_plane::TaskStatus::NeedsReview
            | crate::control_plane::TaskStatus::Running
            | crate::control_plane::TaskStatus::Paused => Self::Running,
            crate::control_plane::TaskStatus::Completed => Self::Completed,
            crate::control_plane::TaskStatus::Failed => Self::Failed,
            crate::control_plane::TaskStatus::Cancelled => Self::Cancelled,
            crate::control_plane::TaskStatus::Lost => Self::Lost,
            crate::control_plane::TaskStatus::TimedOut => Self::TimedOut,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Lost => "lost",
            Self::TimedOut => "timed_out",
        }
    }

    fn is_success(self) -> bool {
        self == Self::Completed
    }

    fn is_pending(self) -> bool {
        self == Self::Running
    }

    fn is_failure(self) -> bool {
        !matches!(self, Self::Running | Self::Completed)
    }
}

pub struct DelegateTool {
    agents: Arc<HashMap<String, AliasedAgentConfig>>,
    security: Arc<SecurityPolicy>,
    /// Global credential (from config.api_key) used when an agent has none set.
    global_credential: Option<String>,
    /// ModelProvider runtime options inherited from root config.
    provider_runtime_options: clawcrew_providers::ModelProviderRuntimeOptions,
    /// Depth at which this tool instance lives in the delegation chain.
    depth: u32,
    /// Binding delegation-depth ceiling for this tool's owner and its whole
    /// subtree. Source of truth: the owning agent's runtime profile
    /// `max_delegation_depth`. Each constructed sub-delegate tool carries the
    /// owner's effective ceiling tightened (min) with the next target's own
    /// profile cap, so a parent's cap binds its entire subtree and a chain
    /// can only tighten. `None` resolves per use in `effective_max_depth`.
    max_delegation_depth: Option<u32>,
    /// Whether this instance may manage background delegate tasks
    /// (`check_result`, `list_results`, `cancel_task`, `await_sessions`).
    /// Background records live in a workspace-wide namespace without owner
    /// identity, and a bounded sub-agent shares the delegating parent's
    /// workspace, so handing it the management surface would let it read and
    /// cancel tasks owned by other identities. Bounded sub-delegate tools
    /// therefore carry delegate-only instances; every other construction
    /// keeps the full surface.
    background_task_management: bool,
    /// Whether the loop calling this instance has an operator approval
    /// route. Top-level and wrapper constructions run inside loops that own
    /// an `ApprovalManager` (or deliberately run unguarded, for legacy test
    /// constructors); bounded sub-agent loops do not - `delegate` sub-agent
    /// loops never thread an approval manager, so a prompt-required call
    /// would silently bypass the target's approval policy. When `false`, the
    /// tool itself refuses delegation for callers whose risk profile would
    /// prompt (supervised default or `always_ask`), failing closed.
    operator_approval_available: bool,
    /// Parent tool registry for agentic sub-agents.
    parent_tools: Arc<RwLock<Vec<Arc<dyn Tool>>>>,
    /// Runtime adapter used to build target-owned registries for independent
    /// agentic delegation.
    runtime: Option<Arc<dyn crate::platform::RuntimeAdapter>>,
    /// Inherited multimodal handling config for sub-agent loops.
    multimodal_config: clawcrew_config::schema::MultimodalConfig,
    /// Global delegate tool config providing default timeout values.
    delegate_config: DelegateToolConfig,
    /// Workspace directory inherited from the root agent context.
    workspace_dir: PathBuf,
    /// Cancellation token for cascade control of background tasks.
    cancellation_token: CancellationToken,
    /// Optional memory instance for namespace isolation on delegate agents.
    memory: Option<Arc<dyn Memory>>,
    /// nested model provider map for brain resolution.
    providers_models: Arc<HashMap<String, HashMap<String, ModelProviderConfig>>>,
    /// named risk profiles for delegation depth and timeout resolution.
    risk_profiles: Arc<HashMap<String, RiskProfileConfig>>,
    /// named runtime profiles for agentic/tools/iteration resolution.
    runtime_profiles: Arc<HashMap<String, RuntimeProfileConfig>>,
    /// named skill bundles for skills-directory resolution.
    skill_bundles: Arc<HashMap<String, SkillBundleConfig>>,
    /// Optional handle to the loaded root config used to resolve delegate
    /// reachability, target mode, and per-target `SecurityPolicy` at delegate
    /// time. When unset (legacy unit-test constructors), DelegateTool falls
    /// back to using `self.security` for the spawned inner DelegateTool.
    root_config: Option<Arc<Config>>,
    /// The daemon's shared live-config handle, when the registry that built
    /// this tool had one. `root_config` above is a *snapshot* taken at
    /// construction; every nested registry this tool builds must additionally
    /// receive this handle so the target's per-execution resolvers (plugin
    /// `[plugins.entries.config]`, `send_via` peer-group authority) follow
    /// config reloads and credential rotation instead of the startup snapshot.
    /// `None` for one-shot / non-daemon callers, which keep the documented
    /// snapshot fallback.
    live_config: Option<Arc<RwLock<Config>>>,
    /// Alias of the agent that owns this DelegateTool. Excluded from the
    /// advertised roster so an agent is never offered itself as a
    /// delegation target. Empty when unset (legacy unit-test constructors).
    caller_alias: String,
    /// Optional per-tree override for background task lifecycle storage. A
    /// daemon-provided control plane wins; non-daemon surfaces share a
    /// process-local handle keyed by `root_config.data_dir`.
    task_control_plane: Arc<tokio::sync::OnceCell<crate::control_plane::ControlPlaneHandle>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelegateAdmission {
    /// This call entered through the user-visible `delegate` tool and must run
    /// caller-side tool authorization plus target reachability checks.
    Required,
    Prevalidated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelegateAction {
    Delegate,
    CheckResult,
    ListResults,
    CancelTask,
    AwaitSessions,
}

impl DelegateAction {
    const ALL: [Self; 5] = [
        Self::Delegate,
        Self::CheckResult,
        Self::ListResults,
        Self::CancelTask,
        Self::AwaitSessions,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Delegate => "delegate",
            Self::CheckResult => "check_result",
            Self::ListResults => "list_results",
            Self::CancelTask => "cancel_task",
            Self::AwaitSessions => "await_sessions",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.as_str() == value)
    }

    fn schema_values() -> Vec<&'static str> {
        Self::ALL.into_iter().map(Self::as_str).collect()
    }

    fn usage() -> String {
        Self::schema_values().join("/")
    }
}

pub(crate) struct IndependentTargetTools {
    pub(crate) tools: crate::tools::scoped::ScopedToolRegistry,
    /// The deferred-MCP + pinned-resources system-prompt section (empty unless
    /// the target has granted MCP bundles under deferred loading).
    deferred_section: String,
    /// Live handle to the deferred-MCP activated set (Some only when a deferred
    /// `tool_search` tool was registered), threaded into the sub-agent turn loop.
    activated_handle: Option<Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>>,
    workspace_dir: PathBuf,
    skills: Vec<crate::skills::Skill>,
}

impl DelegateTool {
    /// Canonical tool name. Referenced by `REENTRANT_AGENT_TOOLS` so a
    /// rename cannot desync the two.
    pub const NAME: &'static str = "delegate";
    const MAX_AWAIT_SESSIONS_TIMEOUT: Duration = Duration::from_secs(120);
    const MAX_AWAIT_SESSION_TASK_IDS: usize = 128;
    const TERMINAL_TRANSITION_ATTEMPTS: usize = 3;
    const TERMINAL_TRANSITION_RETRY_DELAY: Duration = Duration::from_millis(25);
    const TERMINAL_SETTLEMENT_MAX_RETRY_DELAY: Duration = Duration::from_secs(5);
    const OUTPUT_ARTIFACT_PREFIX: &'static str = "artifact:";
    const AGENTIC_ALWAYS_ASK_DOC_REF: &'static str =
        "ClawCrew docs, \"Delegation & SubAgents\" > \"What's not supported\"";

    pub fn new(
        agents: HashMap<String, AliasedAgentConfig>,
        global_credential: Option<String>,
        security: Arc<SecurityPolicy>,
    ) -> Self {
        Self::new_with_options(
            agents,
            global_credential,
            security,
            clawcrew_providers::ModelProviderRuntimeOptions::default(),
        )
    }

    pub fn new_with_options(
        agents: HashMap<String, AliasedAgentConfig>,
        global_credential: Option<String>,
        security: Arc<SecurityPolicy>,
        provider_runtime_options: clawcrew_providers::ModelProviderRuntimeOptions,
    ) -> Self {
        Self {
            agents: Arc::new(agents),
            security,
            global_credential,
            provider_runtime_options,
            depth: 0,
            max_delegation_depth: None,
            background_task_management: true,
            operator_approval_available: true,
            parent_tools: Arc::new(RwLock::new(Vec::new())),
            runtime: None,
            multimodal_config: clawcrew_config::schema::MultimodalConfig::default(),
            delegate_config: DelegateToolConfig::default(),
            workspace_dir: PathBuf::new(),
            cancellation_token: CancellationToken::new(),
            memory: None,
            providers_models: Arc::new(HashMap::new()),
            risk_profiles: Arc::new(HashMap::new()),
            runtime_profiles: Arc::new(HashMap::new()),
            skill_bundles: Arc::new(HashMap::new()),
            root_config: None,
            live_config: None,
            caller_alias: String::new(),
            task_control_plane: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Create a DelegateTool for a sub-agent (with incremented depth).
    /// When sub-agents eventually get their own tool registry, construct
    /// their DelegateTool via this method with `depth: parent.depth + 1`.
    pub fn with_depth(
        agents: HashMap<String, AliasedAgentConfig>,
        global_credential: Option<String>,
        security: Arc<SecurityPolicy>,
        depth: u32,
    ) -> Self {
        Self::with_depth_and_options(
            agents,
            global_credential,
            security,
            depth,
            clawcrew_providers::ModelProviderRuntimeOptions::default(),
        )
    }

    pub fn with_depth_and_options(
        agents: HashMap<String, AliasedAgentConfig>,
        global_credential: Option<String>,
        security: Arc<SecurityPolicy>,
        depth: u32,
        provider_runtime_options: clawcrew_providers::ModelProviderRuntimeOptions,
    ) -> Self {
        Self {
            agents: Arc::new(agents),
            security,
            global_credential,
            provider_runtime_options,
            depth,
            max_delegation_depth: None,
            background_task_management: true,
            operator_approval_available: true,
            parent_tools: Arc::new(RwLock::new(Vec::new())),
            runtime: None,
            multimodal_config: clawcrew_config::schema::MultimodalConfig::default(),
            delegate_config: DelegateToolConfig::default(),
            workspace_dir: PathBuf::new(),
            cancellation_token: CancellationToken::new(),
            memory: None,
            providers_models: Arc::new(HashMap::new()),
            risk_profiles: Arc::new(HashMap::new()),
            runtime_profiles: Arc::new(HashMap::new()),
            skill_bundles: Arc::new(HashMap::new()),
            root_config: None,
            live_config: None,
            caller_alias: String::new(),
            task_control_plane: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Attach parent tools used to build sub-agent allowlist registries.
    pub fn with_parent_tools(mut self, parent_tools: Arc<RwLock<Vec<Arc<dyn Tool>>>>) -> Self {
        self.parent_tools = parent_tools;
        self
    }

    /// Attach the runtime adapter used to build target-owned tools for
    /// independent agentic delegation.
    pub fn with_runtime(mut self, runtime: Arc<dyn crate::platform::RuntimeAdapter>) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Attach multimodal configuration for sub-agent tool loops.
    pub fn with_multimodal_config(
        mut self,
        config: clawcrew_config::schema::MultimodalConfig,
    ) -> Self {
        self.multimodal_config = config;
        self
    }

    /// Attach global delegate tool configuration for default timeout values.
    pub fn with_delegate_config(mut self, config: DelegateToolConfig) -> Self {
        self.delegate_config = config;
        self
    }

    /// Return a shared handle to the parent tools list.
    /// Callers can push additional tools (e.g. MCP wrappers) after construction.
    pub fn parent_tools_handle(&self) -> Arc<RwLock<Vec<Arc<dyn Tool>>>> {
        Arc::clone(&self.parent_tools)
    }

    /// Attach the workspace directory for system prompt enrichment.
    pub fn with_workspace_dir(mut self, workspace_dir: PathBuf) -> Self {
        self.workspace_dir = workspace_dir;
        self
    }

    fn agent_workspace(&self, agent_alias: &str) -> Option<PathBuf> {
        self.root_config
            .as_ref()
            .map(|cfg| cfg.agent_workspace_dir(agent_alias))
    }

    /// Attach a cancellation token for cascade control of background tasks.
    /// When the token is cancelled, all background sub-agents are aborted.
    pub fn with_cancellation_token(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = token;
        self
    }

    /// Return the cancellation token for external cascade control.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Attach memory for namespace isolation on delegate agents.
    pub fn with_memory(mut self, memory: Arc<dyn Memory>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Attach nested model provider map for brain resolution.
    pub fn with_providers_models(
        mut self,
        m: HashMap<String, HashMap<String, ModelProviderConfig>>,
    ) -> Self {
        self.providers_models = Arc::new(m);
        self
    }

    /// Attach risk profiles for depth/timeout resolution.
    pub fn with_risk_profiles(mut self, m: HashMap<String, RiskProfileConfig>) -> Self {
        self.risk_profiles = Arc::new(m);
        self
    }

    /// Attach runtime profiles for agentic/tools/iteration resolution.
    pub fn with_runtime_profiles(mut self, m: HashMap<String, RuntimeProfileConfig>) -> Self {
        self.runtime_profiles = Arc::new(m);
        self
    }

    /// Attach skill bundles for skills-directory resolution.
    pub fn with_skill_bundles(mut self, m: HashMap<String, SkillBundleConfig>) -> Self {
        self.skill_bundles = Arc::new(m);
        self
    }

    /// Attach the loaded root config so DelegateTool can resolve delegate
    /// reachability, target mode, and per-target `SecurityPolicy` from the
    /// canonical agent config at delegate time.
    pub fn with_root_config(mut self, config: Arc<Config>) -> Self {
        self.root_config = Some(config);
        self
    }

    #[cfg(test)]
    fn with_task_control_plane(self, handle: crate::control_plane::ControlPlaneHandle) -> Self {
        assert!(
            self.task_control_plane.set(handle).is_ok(),
            "task control plane is set only once in tests"
        );
        self
    }

    /// Attach the daemon's shared live-config handle alongside
    /// [`Self::with_root_config`].
    ///
    /// `with_root_config` supplies the snapshot this tool reads synchronously
    /// (reachability, target mode, per-target policy). This supplies the handle
    /// that every *nested* registry built for a delegated target needs so its
    /// per-execution resolvers follow reloads. Passing `None` is the documented
    /// one-shot behavior and keeps the snapshot fallback; dropping the handle
    /// when the caller has one silently pins delegated plugin tools to startup
    /// config for the parent's whole lifetime.
    pub fn with_live_config(mut self, live_config: Option<Arc<RwLock<Config>>>) -> Self {
        self.live_config = live_config;
        self
    }

    /// Set the owning agent's alias so it can be excluded from the
    /// advertised delegation roster (an agent must never delegate to
    /// itself).
    pub fn with_caller_alias(mut self, alias: impl Into<String>) -> Self {
        self.caller_alias = alias.into();
        self
    }

    pub(crate) fn policy_for_target(
        &self,
        target_alias: &str,
    ) -> anyhow::Result<Arc<SecurityPolicy>> {
        let Some(config) = self.root_config.as_ref() else {
            return Ok(Arc::clone(&self.security));
        };
        if !self.security.delegation_policy.permits() {
            let remediation = if self.security.risk_profile_name.trim().is_empty() {
                "set the caller risk profile's delegation_policy mode = \"allow\"".to_string()
            } else {
                format!(
                    "set [risk_profiles.{}].delegation_policy mode = \"allow\"",
                    self.security.risk_profile_name
                )
            };
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "target_agent": target_alias,
                        "caller_alias": self.caller_alias,
                        "caller_risk_profile": self.security.risk_profile_name,
                    })),
                "delegate refused: caller delegation_policy forbids delegation"
            );
            return Err(anyhow::Error::msg(format!(
                "delegation is forbidden for caller {:?} by risk profile {:?} \
                 delegation_policy; {remediation}",
                self.caller_alias, self.security.risk_profile_name
            )));
        }

        // Resolve reachability and execution mode through `Config` so
        // admission follows the same canonical roster advertised to callers.
        let Some(target_mode) = config.delegate_target_mode(&self.caller_alias, target_alias)
        else {
            let error = self.unreachable_target_error(config, target_alias);
            let caller_profile = config
                .agents
                .get(&self.caller_alias)
                .map(|agent| agent.risk_profile.trim())
                .unwrap_or_default();
            let target_profile = config
                .agents
                .get(target_alias)
                .map(|agent| agent.risk_profile.trim())
                .unwrap_or_default();
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "target_agent": target_alias,
                        "caller_alias": self.caller_alias,
                        "caller_risk_profile": caller_profile,
                        "target_risk_profile": target_profile,
                    })),
                "delegate refused: target not in caller's reachable set"
            );
            return Err(anyhow::Error::msg(error));
        };

        let mut target_policy = SecurityPolicy::for_agent(config, target_alias).map_err(|e| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "target_agent": target_alias,
                        "caller_alias": self.caller_alias,
                        "error": format!("{}", e),
                    })),
                "delegate: could not resolve target's security policy"
            );
            anyhow::Error::msg(format!(
                "could not resolve security policy for delegate target {target_alias:?}: {e}"
            ))
        })?;

        if target_mode == DelegateExecutionMode::Bounded {
            target_policy.tracker = self.security.tracker.clone();
            // Budget binding: the tracker counter is shared, but enforcement
            // compares it against the invoking policy's own ceiling, so a
            // target with a looser profile ceiling would outspend the
            // caller's budget - the escalation `ensure_no_escalation_beyond`
            // already rejects for child ceilings. The caller's ceiling is
            // itself the tightened chain value, so the bound holds for the
            // whole subtree.
            //
            // The two fields have DIFFERENT zero semantics in the schema.
            // `max_actions_per_hour`: `0` is a hard zero budget, so a numeric
            // min is exact. `max_cost_per_day_cents`: `0` inherits the global
            // limit, so a raw min could turn an unset target value into an
            // "inherit-global" child cap looser than the caller's explicit
            // cap. Combine cost so the result is never looser than the
            // caller's effective ceiling: both explicit -> min, one unset ->
            // carry the explicit one, both unset -> inherit (0).
            //
            // Enforcement status: the ACTION ceiling is enforced at every
            // admission through the shared tracker. The COST ceiling is
            // carried faithfully and enforced by `cost_budget_refusal` below
            // against the target agent's recorded daily spend.
            target_policy.max_actions_per_hour = target_policy
                .max_actions_per_hour
                .min(self.security.max_actions_per_hour);
            target_policy.max_cost_per_day_cents = match (
                self.security.max_cost_per_day_cents,
                target_policy.max_cost_per_day_cents,
            ) {
                (0, target) => target,
                (caller, 0) => caller,
                (caller, target) => caller.min(target),
            };

            if self.security.risk_profile_name == target_policy.risk_profile_name {
                target_policy.workspace_dir = self.security.workspace_dir.clone();
            }
        }

        // Cost governor: refuse a delegated run when the target agent's recorded
        // daily spend already exceeds its effective per-day cap. `0` means
        // "inherit the global limit" (no per-agent cap here).
        if let Some(refusal) =
            self.cost_budget_refusal(target_alias, target_policy.max_cost_per_day_cents)
        {
            return Err(anyhow::Error::msg(refusal));
        }

        // Token governor: refuse a delegated run when the target agent's
        // recorded daily token spend already exceeds the effective cap
        // (caller ∩ target). `0` means no per-agent cap.
        if let Some(refusal) =
            self.token_budget_refusal(target_alias, self.token_cap_for(target_alias))
        {
            return Err(anyhow::Error::msg(refusal));
        }

        Ok(Arc::new(target_policy))
    }

    /// The per-day token cap for a target agent, clamped by the caller's own cap
    /// (parent-subset). `0` means no cap; a `0` on either side inherits the other.
    fn token_cap_for(&self, target_alias: &str) -> u64 {
        let Some(config) = self.root_config.as_ref() else {
            return 0;
        };
        let cap_for = |alias: &str| {
            config
                .agents
                .get(alias)
                .map(|agent| self.resolve_max_tokens_per_day(&agent.runtime_profile))
                .unwrap_or(0)
        };
        match (cap_for(&self.caller_alias), cap_for(target_alias)) {
            (0, target) => target,
            (caller, 0) => caller,
            (caller, target) => caller.min(target),
        }
    }

    fn resolve_max_tokens_per_day(&self, runtime_profile: &str) -> u64 {
        self.runtime_profiles
            .get(runtime_profile)
            .map(|profile| profile.max_tokens_per_day)
            .unwrap_or(0)
    }

    /// `0` cap means unbounded.
    fn exceeds_token_cap(tokens: u64, cap: u64) -> bool {
        cap > 0 && tokens > cap
    }

    /// Refuse delegation when the target agent's recorded daily token spend
    /// already exceeds its effective per-day token cap. Returns `None` when no
    /// cap applies, no tracker/config is available, or the budget is within it.
    fn token_budget_refusal(&self, agent_name: &str, cap: u64) -> Option<String> {
        if cap == 0 {
            return None;
        }
        let config = self.root_config.as_ref()?;
        let tracker = crate::cost::CostTracker::get_or_init_global(
            config.cost.clone(),
            &config.data_dir,
        )?;
        let summary = tracker.get_summary_for_agent(agent_name).ok()?;
        if Self::exceeds_token_cap(summary.total_tokens, cap) {
            return Some(format!(
                "delegation refused: agent {agent_name:?} daily tokens {} exceed its cap {cap}",
                summary.total_tokens
            ));
        }
        None
    }

    /// `0` means "inherit the global limit" (never a per-agent cap).
    fn exceeds_cost_cap(daily_cost_usd: f64, max_cents: u32) -> bool {
        max_cents > 0 && daily_cost_usd > max_cents as f64 / 100.0
    }

    /// Refuse delegation when the target agent's recorded daily spend already
    /// exceeds its effective per-day cost cap. Returns `None` when no cap
    /// applies, no tracker/config is available, or the budget is within limits.
    fn cost_budget_refusal(
        &self,
        agent_name: &str,
        max_cost_per_day_cents: u32,
    ) -> Option<String> {
        if max_cost_per_day_cents == 0 {
            return None;
        }
        let config = self.root_config.as_ref()?;
        let tracker = crate::cost::CostTracker::get_or_init_global(
            config.cost.clone(),
            &config.data_dir,
        )?;
        let summary = tracker.get_summary_for_agent(agent_name).ok()?;
        if Self::exceeds_cost_cap(summary.daily_cost_usd, max_cost_per_day_cents) {
            return Some(format!(
                "delegation refused: agent {agent_name:?} daily cost ${:.2} exceeds its cap ${:.2}",
                summary.daily_cost_usd,
                max_cost_per_day_cents as f64 / 100.0,
            ));
        }
        None
    }

    fn unreachable_target_error(&self, config: &Config, target_alias: &str) -> String {
        let Some(caller) = config.agents.get(&self.caller_alias) else {
            return format!(
                "delegate target {target_alias:?} is not reachable because caller {:?} \
                 is not present in the loaded agents config",
                self.caller_alias
            );
        };

        let Some(target) = config.agents.get(target_alias) else {
            return format!(
                "delegate target {target_alias:?} is not reachable from {:?}: \
                 no agent with that alias exists in the loaded config",
                self.caller_alias
            );
        };

        let explicitly_configured = caller
            .delegates
            .iter()
            .any(|target| target.agent().trim() == target_alias);

        if !target.enabled {
            return format!(
                "delegate target {target_alias:?} is not reachable from {:?}: \
                 the target agent is disabled",
                self.caller_alias
            );
        }

        let caller_profile = caller.risk_profile.trim();
        let target_profile = target.risk_profile.trim();
        if caller.delegate_same_risk_profile
            && !explicitly_configured
            && !caller_profile.is_empty()
            && !target_profile.is_empty()
            && caller_profile != target_profile
        {
            return format!(
                "delegate target {target_alias:?} is not reachable from {:?}: \
                 different risk profile (caller uses {caller_profile:?}, target uses \
                 {target_profile:?}). delegate_same_risk_profile only reaches agents \
                 with the same risk profile; add an explicit [agents.{}].delegates \
                 entry with the intended mode, or change one agent's risk_profile.",
                self.caller_alias, self.caller_alias
            );
        }

        if !caller.delegate_same_risk_profile && !explicitly_configured {
            return format!(
                "delegate target {target_alias:?} is not reachable from {:?}: \
                 delegate_same_risk_profile is disabled and the target is not listed \
                 in [agents.{}].delegates",
                self.caller_alias, self.caller_alias
            );
        }

        format!(
            "delegate target {target_alias:?} is not reachable from {:?}; \
             add it to [agents.{}].delegates or share a risk profile with \
             delegate_same_risk_profile enabled",
            self.caller_alias, self.caller_alias
        )
    }

    fn mode_for_target(&self, target_alias: &str) -> DelegateExecutionMode {
        self.root_config
            .as_ref()
            .and_then(|config| config.delegate_target_mode(&self.caller_alias, target_alias))
            .unwrap_or(DelegateExecutionMode::Bounded)
    }

    fn unsupported_agentic_always_ask_refusal(&self, target_alias: &str) -> Option<ToolResult> {
        let config = self.root_config.as_ref()?;
        let target_mode = config.delegate_target_mode(&self.caller_alias, target_alias)?;
        let target_agent = config.agents.get(target_alias)?;
        // Independent targets have no approval backchannel at all. Bounded
        // one-shot targets do not execute tools, so `always_ask` is irrelevant;
        // bounded agentic loops must fail closed until approval forwarding is
        // implemented.
        if target_mode == DelegateExecutionMode::Bounded {
            let target_is_agentic = config
                .runtime_profiles
                .get(target_agent.runtime_profile.as_str())
                .map(|profile| profile.agentic)
                .unwrap_or(false);
            if !target_is_agentic {
                return None;
            }
        }
        let target_risk_profile = target_agent.risk_profile.trim();
        if target_risk_profile.is_empty() {
            return None;
        }

        let profile = config.risk_profiles.get(target_risk_profile)?;
        let always_ask_entries: Vec<String> = profile
            .always_ask
            .iter()
            .map(|entry| entry.trim())
            .filter(|entry| !entry.is_empty())
            .map(str::to_string)
            .collect();
        if always_ask_entries.is_empty() {
            return None;
        }
        let always_ask_label = always_ask_entries.join(", ");
        let mode_label = match target_mode {
            DelegateExecutionMode::Independent => "independent",
            DelegateExecutionMode::Bounded => "bounded agentic",
        };

        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                .with_attrs(::serde_json::json!({
                    "error_key": "delegate.agentic_always_ask_unsupported",
                    "caller_alias": self.caller_alias,
                    "target_agent": target_alias,
                    "target_mode": mode_label,
                    "target_risk_profile": target_risk_profile,
                    "always_ask": &always_ask_entries,
                })),
            "delegate refused: target cannot honor always_ask entries"
        );

        Some(ToolResult {
            success: false,
            output: ToolOutput::default(),
            error: Some(format!(
                "delegate target {target_alias:?} cannot run in {mode_label} mode from {:?}: \
                 risk profile {target_risk_profile:?} has always_ask entries ({}). \
                 See {}.",
                self.caller_alias,
                always_ask_label,
                Self::AGENTIC_ALWAYS_ASK_DOC_REF
            )),
        })
    }

    fn build_target_provider(
        &self,
        model_provider: &str,
        provider_type: &str,
        credential: Option<&str>,
    ) -> anyhow::Result<(Box<dyn ModelProvider>, String, String)> {
        if let Some(config) = self.root_config.as_deref() {
            let (provider, provider_name, model_name, _resolver) =
                crate::agent::agent::build_session_model_provider(config, model_provider, None)?;
            return Ok((provider, provider_name, model_name));
        }
        let provider = clawcrew_providers::create_model_provider_with_options(
            provider_type,
            credential,
            &self.provider_runtime_options,
        )?;
        let (_, _, model, _) = self.resolve_brain(model_provider);
        Ok((provider, provider_type.to_string(), model))
    }

    async fn memory_for_target_agent(
        &self,
        agent_name: &str,
    ) -> anyhow::Result<Option<Arc<dyn Memory>>> {
        let Some(config) = self.root_config.as_deref() else {
            return Ok(self.memory.clone());
        };

        let api_key = config
            .resolved_model_provider_for_agent(agent_name)
            .and_then(|(_, _, cfg)| cfg.api_key.as_deref());
        clawcrew_memory::create_memory_for_agent(config, agent_name, api_key)
            .await
            .map(Some)
    }

    fn memory_tools_for_target(
        memory: Arc<dyn Memory>,
        security: Arc<SecurityPolicy>,
    ) -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(MemoryStoreTool::new(memory.clone(), security.clone())),
            Box::new(MemoryRecallTool::new(memory.clone())),
            Box::new(MemoryForgetTool::new(memory.clone(), security.clone())),
            Box::new(MemoryExportTool::new(memory.clone())),
            Box::new(MemoryPurgeTool::new(memory, security)),
        ]
    }

    pub(crate) async fn independent_agentic_tools_for_target(
        &self,
        agent_name: &str,
        target_policy: Arc<SecurityPolicy>,
    ) -> anyhow::Result<IndependentTargetTools> {
        let config = self
            .root_config
            .as_ref()
            .ok_or_else(|| anyhow::Error::msg("independent delegation requires root config"))?;
        let runtime =
            self.runtime.as_ref().cloned().ok_or_else(|| {
                anyhow::Error::msg("independent delegation requires runtime adapter")
            })?;
        let risk_profile = config
            .risk_profile_for_agent(agent_name)
            .cloned()
            .ok_or_else(|| {
                anyhow::Error::msg(format!(
                    "Agent '{agent_name}' is agentic but its risk profile is not configured"
                ))
            })?;
        let memory = self
            .memory_for_target_agent(agent_name)
            .await?
            .ok_or_else(|| {
                anyhow::Error::msg(format!(
                    "Failed to initialize memory for independent delegate target '{agent_name}'"
                ))
            })?;
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
        let target_api_key = config
            .resolved_model_provider_for_agent(agent_name)
            .and_then(|(_, _, provider)| provider.api_key.as_deref());

        let all_tools_result = crate::tools::all_tools_with_runtime(
            Arc::clone(config),
            &target_policy,
            &risk_profile,
            agent_name,
            runtime.clone(),
            memory,
            composio_key,
            composio_entity_id,
            &config.browser,
            &config.http_request,
            &config.web_fetch,
            &target_policy.workspace_dir,
            &config.agents,
            target_api_key,
            config,
            None,
            false,
            None,
            None,
            None,
            // The delegated target's registry is built once, here, but its
            // plugin tools and `send_via` authority resolve per execution. They
            // must resolve against the daemon's shared handle, not the
            // `root_config` snapshot this DelegateTool captured at
            // construction - otherwise a reload or credential rotation is
            // invisible to every delegated plugin tool for the parent's whole
            // lifetime. `None` only when the parent registry itself had no live
            // handle (one-shot callers), which keeps the snapshot fallback.
            self.live_config.clone(),
        )?;

        let target_workspace = config.agent_workspace_dir(agent_name);
        let skills = crate::skills::load_skills_for_agent_from_config(config, agent_name);

        let assembled = crate::tools::scoped::ScopedToolRegistry::assemble(
            crate::tools::scoped::ScopedAssembly {
                config,
                agent_alias: agent_name,
                security: &target_policy,
                built: all_tools_result,
                skills: &skills,
                runtime,
                caller_allowed: None,
                connect_mcp: true,
                connect_peripherals: false,
                exclude_memory: false,
                acp_delivery: false,
                list_deferred_mcp_specs: false,
                emit_assembly_logs: true,
                // Delegate: targets are short-lived independent chat
                // sessions with no cross-turn reuse contract, so the
                // per-call `connect_all` is the correct choice. The
                // daemon heartbeat worker is the only `mcp_registry`
                // supplier.
                mcp_registry: None,
            },
        )
        .await;
        // Independent delegation injects one combined MCP prompt block: the harness
        // composes the deferred tool-search listing with any pinned MCP resources, so
        // this can no longer silently lose pinned resources the way a raw-field
        // destructure could (see `ScopedAssembled::combined_mcp_prompt_section`).
        let deferred_section = assembled.combined_mcp_prompt_section();
        let crate::tools::scoped::ScopedAssembled {
            mut registry,
            activated_handle,
            ..
        } = assembled;
        // Strip the delegate tool from the ALREADY-sealed registry via the
        // `retain` mutator - no unseal/reseal round-trip through a raw `Vec`.
        // Same set removed as before (`tool.name() != Self::NAME`).
        registry.retain(|tool| tool.name() != Self::NAME);
        Ok(IndependentTargetTools {
            tools: registry,
            deferred_section,
            activated_handle,
            workspace_dir: target_workspace,
            skills,
        })
    }

    /// Resolve `model_provider` ("type.alias") → (provider_type, credential, model, temperature).
    fn resolve_brain(&self, model_provider: &str) -> (String, Option<String>, String, Option<f64>) {
        if let Some((type_key, alias_key)) = model_provider.split_once('.')
            && let Some(alias_map) = self.providers_models.get(type_key)
            && let Some(cfg) = alias_map.get(alias_key)
        {
            return (
                type_key.to_string(),
                if cfg.requires_openai_auth {
                    cfg.api_key.clone()
                } else {
                    cfg.api_key
                        .clone()
                        .or_else(|| self.global_credential.clone())
                },
                cfg.model.clone().unwrap_or_default(),
                cfg.temperature,
            );
        }
        let type_key = model_provider
            .split_once('.')
            .map_or(model_provider, |(t, _)| t);
        (
            type_key.to_string(),
            self.global_credential.clone(),
            String::new(),
            None,
        )
    }

    /// Resolve max delegation depth from the named runtime profile (default: 3).
    fn resolve_max_depth(&self, runtime_profile: &str) -> u32 {
        if runtime_profile.is_empty() {
            return 3;
        }
        self.runtime_profiles
            .get(runtime_profile)
            .map(|p| p.max_delegation_depth)
            .filter(|&d| d > 0)
            .unwrap_or(3)
    }

    /// The binding delegation-depth ceiling for this tool's owner.
    ///
    /// Source of truth: the owning agent's runtime profile
    /// `max_delegation_depth`. Sub-delegate tools carry that ceiling tightened
    /// with each target's own profile cap (`tightened_max_depth`), so a
    /// parent's cap binds its entire subtree and a chain can only tighten.
    /// When no ceiling was carried (root tools and bare test constructors),
    /// the owner's own profile is resolved here; without a resolvable owner,
    /// the pre-chain fallback applies (the target's profile cap, default 3),
    /// which keeps legacy `with_depth` constructors on their historical
    /// semantics.
    fn effective_max_depth(&self, target_runtime_profile: &str) -> u32 {
        if let Some(cap) = self.max_delegation_depth {
            return cap;
        }
        if let Some(config) = self.root_config.as_deref()
            && !self.caller_alias.is_empty()
        {
            return config
                .runtime_profile_for_agent(&self.caller_alias)
                .map(|profile| profile.max_delegation_depth)
                .filter(|&cap| cap > 0)
                .unwrap_or(3);
        }
        self.resolve_max_depth(target_runtime_profile)
    }

    /// The ceiling a constructed sub-delegate tool carries: this tool's
    /// effective ceiling tightened (min) by the next target's own profile
    /// cap. A parent's cap therefore binds its whole subtree, while a target
    /// with a stricter profile tightens the chain from its level down.
    fn tightened_max_depth(&self, target_runtime_profile: &str) -> u32 {
        u32::min(
            self.effective_max_depth(target_runtime_profile),
            self.resolve_max_depth(target_runtime_profile),
        )
    }

    /// Fail-closed approval check for instances whose calling loop has no
    /// operator approval route (bounded sub-agent loops). The caller's own
    /// risk profile decides: when it would prompt for `delegate` (supervised
    /// default, or the tool named in `always_ask`), delegation is refused -
    /// the identical call in a loop with an approval manager would surface
    /// an operator prompt instead. Explicitly auto-approved profiles and
    /// full autonomy keep the grant. Unresolvable profiles fail closed;
    /// legacy constructors without any profile name keep their historical
    /// unguarded behavior.
    fn operator_approval_refusal(&self) -> Option<String> {
        if self.operator_approval_available {
            return None;
        }
        let profile_name = self.security.risk_profile_name.trim();
        if profile_name.is_empty() {
            return None;
        }
        let Some(profile) = self.risk_profiles.get(profile_name) else {
            return Some(format!(
                "delegation refused: risk profile {profile_name:?} could not be resolved for \
                 the delegation approval check"
            ));
        };
        let requirement =
            ApprovalManager::from_risk_profile(profile).approval_requirement(Self::NAME);
        if requirement == ApprovalRequirement::Prompt {
            return Some(format!(
                "delegation refused: risk profile {profile_name:?} requires approval for \
                 'delegate' and bounded sub-agents have no operator approval route; add \
                 'delegate' to the profile's auto_approve list or run the profile at full \
                 autonomy"
            ));
        }
        None
    }

    /// Resolve per-call delegation timeout from the named runtime profile.
    fn resolve_delegation_timeout(&self, runtime_profile: &str) -> Option<u64> {
        if runtime_profile.is_empty() {
            return None;
        }
        self.runtime_profiles
            .get(runtime_profile)
            .and_then(|p| p.delegation_timeout_secs)
    }

    /// Resolve agentic run timeout from the named runtime profile.
    fn resolve_agentic_timeout_secs(&self, runtime_profile: &str) -> Option<u64> {
        if runtime_profile.is_empty() {
            return None;
        }
        self.runtime_profiles
            .get(runtime_profile)
            .and_then(|p| p.agentic_timeout_secs)
    }

    /// Resolve agentic mode flag from the named runtime profile (default: false).
    fn resolve_agentic(&self, runtime_profile: &str) -> bool {
        if runtime_profile.is_empty() {
            return false;
        }
        self.runtime_profiles
            .get(runtime_profile)
            .map(|p| p.agentic)
            .unwrap_or(false)
    }

    fn resolve_loop_runtime(
        &self,
        agent_alias: &str,
        agent_config: &AliasedAgentConfig,
    ) -> ResolvedRuntime {
        if let Some(root_config) = self.root_config.as_ref()
            && let Some(resolved_config) = root_config.resolved_agent_config(agent_alias)
        {
            return resolved_config.resolved;
        }

        let mut resolved = agent_config.resolved.clone();

        if let Some(profile) = self
            .runtime_profiles
            .get(agent_config.runtime_profile.as_str())
        {
            if profile.max_tool_iterations > 0 {
                resolved.max_tool_iterations = profile.max_tool_iterations;
            }
            if profile.max_context_tokens.is_some() {
                resolved.max_context_tokens = profile.max_context_tokens;
            }
            if let Some(ratio) = profile
                .context_compact_ratio
                .filter(|r| *r > 0.0 && *r <= 1.0)
            {
                resolved.context_compact_ratio = Some(ratio);
            }
            if let Some(parallel_tools) = profile.parallel_tools {
                resolved.parallel_tools = parallel_tools;
            }
            if let Some(max_tool_result_chars) = profile.max_tool_result_chars {
                resolved.max_tool_result_chars = max_tool_result_chars;
            }
            resolved.strict_tool_parsing = profile.strict_tool_parsing;
        }

        resolved
    }

    fn resolve_tool_policy(&self, risk_profile: &str) -> Option<SecurityPolicy> {
        if risk_profile.is_empty() {
            return None;
        }

        let profile = self.risk_profiles.get(risk_profile)?;
        Some(SecurityPolicy {
            allowed_tools: profile.effective_allowed_tools(),
            excluded_tools: if profile.excluded_tools.is_empty() {
                None
            } else {
                Some(profile.excluded_tools.clone())
            },
            ..SecurityPolicy::default()
        })
    }

    fn delegate_admits_with_mcp(policy: &SecurityPolicy, name: &str) -> bool {
        let denied = policy
            .excluded_tools
            .as_ref()
            .is_some_and(|list| list.iter().any(|t| t == name));
        if denied {
            return false;
        }
        match policy.allowed_tools.as_ref() {
            None => true,
            Some(list) if list.is_empty() => false,
            Some(list) => list.iter().any(|t| t == name) || name.contains("__"),
        }
    }

    /// Resolve every configured skill bundle alias to its directory.
    /// Empty list / no matches → caller falls back to the workspace default.
    fn resolve_skill_bundle_dirs(&self, bundle_aliases: &[String]) -> Vec<String> {
        bundle_aliases
            .iter()
            .filter(|a| !a.is_empty())
            .filter_map(|a| self.skill_bundles.get(a).and_then(|b| b.directory.clone()))
            .collect()
    }

    /// Directory where background delegate results are stored.
    fn results_dir(&self) -> PathBuf {
        self.workspace_dir.join("delegate_results")
    }

    async fn background_control_plane(
        &self,
    ) -> anyhow::Result<crate::control_plane::ControlPlaneHandle> {
        #[cfg(test)]
        if let Some(handle) = self.task_control_plane.get() {
            return Ok(handle.clone());
        }
        if let Some(handle) = crate::control_plane::control_plane() {
            return Ok(handle.clone());
        }
        #[cfg(not(test))]
        if let Some(handle) = self.task_control_plane.get() {
            return Ok(handle.clone());
        }

        let Some(data_dir) = self
            .root_config
            .as_ref()
            .map(|config| config.data_dir.clone())
        else {
            ::clawcrew_log::record!(
                ERROR,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure),
                "background delegation rejected because the durable task store is unavailable"
            );
            return Err(anyhow::Error::msg(
                "background delegation requires a durable task store; root config is unavailable",
            ));
        };
        type ControlPlaneCell =
            tokio::sync::OnceCell<crate::control_plane::ControlPlaneRecoveryOwner>;
        static CONTROL_PLANES: std::sync::OnceLock<
            parking_lot::Mutex<HashMap<PathBuf, Arc<ControlPlaneCell>>>,
        > = std::sync::OnceLock::new();
        let cell = CONTROL_PLANES
            .get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
            .lock()
            .entry(data_dir.clone())
            .or_insert_with(|| Arc::new(ControlPlaneCell::new()))
            .clone();
        cell.get_or_try_init(|| async {
            let owner = crate::control_plane::ControlPlaneRecoveryOwner::start(&data_dir).await?;
            std::mem::drop(owner.spawn_reaper(
                crate::control_plane::reaper::DEFAULT_MAX_RUNTIME_SECS,
                CancellationToken::new(),
            ));
            Ok::<_, anyhow::Error>(owner)
        })
        .await
        .map(|owner| owner.handle().clone())
    }

    fn serialize_result<T: serde::Serialize>(result: &T) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(result)?)
    }

    async fn write_bytes_atomic(result_path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
        let tmp_path = result_path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
        let write_result = async {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)
                .await?;
            file.write_all(bytes).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&tmp_path, result_path).await?;
            Self::sync_parent_directory(result_path).await
        }
        .await;
        if let Err(error) = write_result {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(error);
        }
        Ok(())
    }

    async fn write_result_atomic<T: serde::Serialize>(
        result_path: &Path,
        result: &T,
    ) -> anyhow::Result<()> {
        let bytes = Self::serialize_result(result)?;
        Self::write_bytes_atomic(result_path, &bytes).await
    }

    fn durable_artifact_path(result_path: &Path) -> anyhow::Result<PathBuf> {
        if result_path.is_absolute() {
            return Ok(result_path.to_path_buf());
        }
        Ok(std::env::current_dir()?.join(result_path))
    }

    #[allow(clippy::unused_async)]
    async fn sync_parent_directory(path: &Path) -> anyhow::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            anyhow::Error::msg(format!(
                "delegate output path has no parent directory: {}",
                path.display()
            ))
        })?;

        #[cfg(unix)]
        {
            let directory = tokio::fs::File::open(parent).await?;
            directory.sync_all().await?;
        }

        #[cfg(not(unix))]
        {
            // std does not expose a portable directory-sync primitive here.
            let _ = parent;
        }

        Ok(())
    }

    async fn settle_background_task(
        store: &dyn crate::control_plane::TaskRegistry,
        result_path: &Path,
        result: BackgroundDelegateOutput,
        terminal_status: crate::control_plane::TaskStatus,
        terminal_error: Option<String>,
        owner_pid: u32,
        owner_boot_id: String,
    ) -> anyhow::Result<bool> {
        let task_id = result.task_id.clone();
        let bytes = match Self::serialize_result(&result) {
            Ok(bytes) => bytes,
            Err(error) => {
                let error = format!("failed to prepare delegate settlement: {error:#}");
                return Ok(Self::supervise_pre_intent_failure(
                    store,
                    &task_id,
                    owner_pid,
                    &owner_boot_id,
                    error,
                )
                .await);
            }
        };
        if !result_path.is_absolute() {
            let error = format!(
                "failed to prepare delegate settlement: output path is not absolute: {}",
                result_path.display()
            );
            return Ok(Self::supervise_pre_intent_failure(
                store,
                &task_id,
                owner_pid,
                &owner_boot_id,
                error,
            )
            .await);
        }
        let file_name = match result_path.file_name() {
            Some(file_name) => file_name.to_string_lossy(),
            None => {
                let error = format!(
                    "failed to prepare delegate settlement: delegate output path has no file \
                     name: {}",
                    result_path.display()
                );
                return Ok(Self::supervise_pre_intent_failure(
                    store,
                    &task_id,
                    owner_pid,
                    &owner_boot_id,
                    error,
                )
                .await);
            }
        };
        let artifact_path = result_path.to_path_buf();
        let output_ref = (terminal_status == crate::control_plane::TaskStatus::Completed)
            .then(|| format!("{}{}", Self::OUTPUT_ARTIFACT_PREFIX, file_name));
        let intent = crate::control_plane::task_registry::TerminalSettlementIntent {
            task_id: task_id.clone(),
            owner_pid,
            owner_boot_id,
            desired_status: terminal_status,
            artifact_path: artifact_path.to_string_lossy().into_owned(),
            artifact_ref: output_ref,
            artifact_sha256: hex::encode(Sha256::digest(&bytes)),
            terminal_error,
        };

        if !Self::supervise_settlement_intent(store, &task_id, &intent).await {
            return Ok(false);
        }

        if let Err(error) = Self::write_bytes_atomic(&artifact_path, &bytes).await {
            let persistence_error = format!("failed to persist delegate output: {error:#}");
            return Ok(Self::supervise_terminal_transition(&task_id, || {
                store.promote_terminal_settlement(
                    &intent,
                    crate::control_plane::TaskStatus::Failed,
                    None,
                    Some(persistence_error.clone()),
                )
            })
            .await);
        }

        let settled_error = intent.terminal_error.clone();
        let output_ref = intent.artifact_ref.clone();
        Ok(Self::supervise_terminal_transition(&task_id, || {
            store.promote_terminal_settlement(
                &intent,
                intent.desired_status,
                output_ref.clone(),
                settled_error.clone(),
            )
        })
        .await)
    }

    async fn supervise_pre_intent_failure(
        store: &dyn crate::control_plane::TaskRegistry,
        task_id: &str,
        owner_pid: u32,
        owner_boot_id: &str,
        error: String,
    ) -> bool {
        Self::supervise_terminal_transition(task_id, || {
            store.transition_terminal_if_owner(
                task_id,
                owner_pid,
                owner_boot_id,
                crate::control_plane::TaskStatus::Failed,
                None,
                Some(error.clone()),
            )
        })
        .await
    }

    async fn complete_background_task(
        store: &dyn crate::control_plane::TaskRegistry,
        result_path: &Path,
        result: BackgroundDelegateOutput,
        terminal_status: crate::control_plane::TaskStatus,
        terminal_error: Option<String>,
        owner_pid: u32,
        owner_boot_id: String,
    ) -> bool {
        let task_id = result.task_id.clone();
        let fallback_owner_boot_id = owner_boot_id.clone();
        let settled = Self::settle_background_task(
            store,
            result_path,
            result,
            terminal_status,
            terminal_error,
            owner_pid,
            owner_boot_id,
        )
        .await;
        let won = match settled {
            Ok(won) => won,
            Err(error) => {
                let error = format!("failed to settle background delegate task: {error:#}");
                Self::supervise_pre_intent_failure(
                    store,
                    &task_id,
                    owner_pid,
                    &fallback_owner_boot_id,
                    error,
                )
                .await
            }
        };

        Self::background_task_cancels().lock().remove(&task_id);
        won
    }

    async fn supervise_settlement_intent(
        store: &dyn crate::control_plane::TaskRegistry,
        task_id: &str,
        intent: &crate::control_plane::task_registry::TerminalSettlementIntent,
    ) -> bool {
        let mut failures = 0_u32;
        loop {
            match store
                .persist_terminal_settlement_intent(intent.clone())
                .await
            {
                Ok(ready) => {
                    if failures > 0 {
                        ::clawcrew_log::record!(
                            INFO,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Write
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Success)
                            .with_attrs(::serde_json::json!({
                                "task_id": task_id,
                                "attempts": failures + 1,
                                "intent_persisted": ready,
                            })),
                            "background delegate settlement intent recovered"
                        );
                    }
                    return ready;
                }
                Err(error) => {
                    failures = failures.saturating_add(1);
                    if failures == 1 || failures.is_power_of_two() {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Write
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "task_id": task_id,
                                "attempt": failures,
                                "error": format!("{error:#}"),
                            })),
                            "background delegate settlement intent will retry"
                        );
                    }
                }
            }

            let multiplier = 1_u32 << failures.saturating_sub(1).min(8);
            let delay = Self::TERMINAL_TRANSITION_RETRY_DELAY
                .saturating_mul(multiplier)
                .min(Self::TERMINAL_SETTLEMENT_MAX_RETRY_DELAY);
            tokio::time::sleep(delay).await;
        }
    }

    async fn supervise_terminal_transition<F, Fut>(task_id: &str, mut transition: F) -> bool
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = anyhow::Result<bool>>,
    {
        let mut failures = 0_u32;
        loop {
            match transition().await {
                Ok(won) => {
                    if failures > 0 {
                        ::clawcrew_log::record!(
                            INFO,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Write
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Success)
                            .with_attrs(::serde_json::json!({
                                "task_id": task_id,
                                "attempts": failures + 1,
                                "transition_won": won,
                            })),
                            "background delegate terminal transition recovered"
                        );
                    }
                    return won;
                }
                Err(error) => {
                    failures = failures.saturating_add(1);
                    if failures == 1 || failures.is_power_of_two() {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Write
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "task_id": task_id,
                                "attempt": failures,
                                "error": format!("{error:#}"),
                            })),
                            "background delegate terminal transition will retry"
                        );
                    }
                }
            }

            let multiplier = 1_u32 << failures.saturating_sub(1).min(8);
            let delay = Self::TERMINAL_TRANSITION_RETRY_DELAY
                .saturating_mul(multiplier)
                .min(Self::TERMINAL_SETTLEMENT_MAX_RETRY_DELAY);
            tokio::time::sleep(delay).await;
        }
    }

    async fn retry_terminal_transition<F, Fut>(mut transition: F) -> anyhow::Result<bool>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = anyhow::Result<bool>>,
    {
        let mut last_error = None;
        for attempt in 1..=Self::TERMINAL_TRANSITION_ATTEMPTS {
            match transition().await {
                Ok(won) => return Ok(won),
                Err(error) => last_error = Some(error),
            }
            if attempt < Self::TERMINAL_TRANSITION_ATTEMPTS {
                tokio::time::sleep(Self::TERMINAL_TRANSITION_RETRY_DELAY * attempt as u32).await;
            }
        }
        Err(last_error
            .expect("terminal transition loop always records an error")
            .context("terminal task transition failed after bounded retries"))
    }

    async fn settle_background_cancellation<F, Fut>(
        task_id: &str,
        transition: F,
    ) -> anyhow::Result<(bool, bool)>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = anyhow::Result<bool>>,
    {
        let won = Self::retry_terminal_transition(transition).await?;
        if !won {
            return Ok((false, false));
        }

        let aborted = Self::background_task_cancels()
            .lock()
            .remove(task_id)
            .inspect(CancellationToken::cancel)
            .is_some();
        Ok((true, aborted))
    }

    fn owns_delegate_task(&self, task: &crate::control_plane::TaskRecord) -> bool {
        task.kind == crate::control_plane::TaskKind::Delegate
            && self
                .caller_identity()
                .is_some_and(|caller| task.originator_route.as_deref() == Some(caller))
    }

    fn can_read_delegate_task(&self, task: &crate::control_plane::TaskRecord) -> bool {
        self.owns_delegate_task(task)
            || (task.kind == crate::control_plane::TaskKind::Delegate
                && task.originator_route.is_none()
                && task.status.is_terminal())
    }

    fn caller_identity(&self) -> Option<&str> {
        let alias = self.caller_alias.trim();
        (!alias.is_empty()).then_some(alias)
    }

    /// Validate that a user-provided task_id is a valid UUID to prevent
    /// path traversal attacks (e.g. `../../etc/passwd`).
    fn validate_task_id(task_id: &str) -> Result<(), String> {
        if uuid::Uuid::parse_str(task_id).is_err() {
            return Err(format!("Invalid task_id '{task_id}': must be a valid UUID"));
        }
        Ok(())
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn description(&self) -> &str {
        "Delegate a subtask to a specialized agent. Use when: a task benefits from a different model \
         (e.g. fast summarization, deep reasoning, code generation). The sub-agent runs a single \
         prompt by default; with agentic=true it can iterate with a filtered tool-call loop. \
         Supports background execution (returns a task_id immediately), batched background waits \
         (await_sessions), and parallel execution (runs multiple agents concurrently). Bounded \
         sub-agents receive the delegate tool only when their risk profile's delegation_policy \
         allows it; independent targets assemble their own tools."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let delegation_permitted = self.security.delegation_policy.permits();
        let caller_profile = self.security.risk_profile_name.as_str();
        // Delegate-only instances (bounded sub-agents) must not advertise the
        // task-management actions they will refuse.
        let (action_values, action_description) = if self.background_task_management {
            (
                DelegateAction::schema_values(),
                "Action to perform. Default: 'delegate'. Use 'check_result' to \
                 retrieve a background task result, 'await_sessions' to wait for \
                 multiple background results, 'list_results' to list all background \
                 tasks, 'cancel_task' to cancel a running background task."
                    .to_string(),
            )
        } else {
            (
                vec![DelegateAction::Delegate.as_str()],
                "Action to perform. Only 'delegate' is available: bounded sub-agents \
                 cannot manage background tasks."
                    .to_string(),
            )
        };
        let mut agent_names: Vec<String> = if !delegation_permitted {
            Vec::new()
        } else if let Some(config) = self.root_config.as_ref() {
            config.reachable_delegate_targets(&self.caller_alias)
        } else {
            let mut names: Vec<String> = self
                .agents
                .iter()
                .filter(|(name, _)| name.as_str() != self.caller_alias.as_str())
                .filter(|(_, cfg)| cfg.risk_profile.trim() == caller_profile)
                .map(|(name, _)| name.clone())
                .collect();
            names.sort_unstable();
            names
        };
        agent_names.sort_unstable();
        agent_names.dedup();
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "action": {
                    "type": "string",
                    "enum": action_values,
                    "description": action_description,
                    "default": DelegateAction::Delegate.as_str()
                },
                "agent": {
                    "type": "string",
                    "minLength": 1,
                    "description": format!(
                        "Name of the agent to delegate to. Available: {}",
                        if agent_names.is_empty() {
                            "(none configured)".to_string()
                        } else {
                            agent_names.join(", ")
                        }
                    )
                },
                "prompt": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The task/prompt to send to the sub-agent"
                },
                "context": {
                    "type": "string",
                    "description": "Optional context to prepend (e.g. relevant code, prior findings)"
                },
                "background": {
                    "type": "boolean",
                    "description": "When true, the sub-agent runs in a background tokio task and \
                                    returns a task_id immediately. Results are stored to \
                                    workspace/delegate_results/{task_id}.json.",
                    "default": false
                },
                "parallel": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Array of agent names to run concurrently with the same prompt. \
                                    Returns all results when all agents complete. Cannot be combined \
                                    with 'background'."
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID for check_result/cancel_task actions (returned by \
                                    background delegation)."
                },
                "task_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "maxItems": Self::MAX_AWAIT_SESSION_TASK_IDS,
                    "description": "Task IDs for await_sessions."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": Self::MAX_AWAIT_SESSIONS_TIMEOUT.as_millis(),
                    "description": "Maximum milliseconds for await_sessions to wait before returning partial results. Capped at 120000."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action_value = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| DelegateAction::Delegate.as_str());
        let Some(action) = DelegateAction::parse(action_value) else {
            return Ok(ToolResult {
                success: false,
                output: String::new().into(),
                error: Some(format!(
                    "Unknown action '{action_value}'. Use {}.",
                    DelegateAction::usage()
                )),
            });
        };

        // Bounded sub-agents carry delegate-only instances: background
        // records live in a workspace-wide namespace without owner identity,
        // so the management surface must never reach a distinct identity
        // sharing that workspace.
        if !self.background_task_management && action != DelegateAction::Delegate {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(
                    "task management actions (check_result, list_results, cancel_task, \
                     await_sessions) are not available to bounded sub-agents"
                        .to_string(),
                ),
            });
        }

        match action {
            DelegateAction::CheckResult => return self.handle_check_result(&args).await,
            DelegateAction::ListResults => return self.handle_list_results().await,
            DelegateAction::CancelTask => return self.handle_cancel_task(&args).await,
            DelegateAction::AwaitSessions => return self.handle_await_sessions(&args).await,
            DelegateAction::Delegate => {}
        }

        // --- Parallel mode ---
        if let Some(parallel_agents) = args.get("parallel").and_then(|v| v.as_array()) {
            return self.execute_parallel(parallel_agents, &args).await;
        }

        // --- Single-agent delegation (synchronous or background) ---
        let agent_name = args
            .get("agent")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"param": "agent"})),
                    "tool argument validation failed"
                );

                anyhow::Error::msg("Missing 'agent' parameter")
            })?;

        if agent_name.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some("'agent' parameter must not be empty".into()),
            });
        }

        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"param": "prompt"})),
                    "tool argument validation failed"
                );

                anyhow::Error::msg("Missing 'prompt' parameter")
            })?;

        if prompt.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some("'prompt' parameter must not be empty".into()),
            });
        }

        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if background {
            return self.execute_background(agent_name, prompt, &args).await;
        }

        // --- Synchronous delegation (original path) ---
        self.execute_sync(agent_name, prompt, &args).await
    }
}

impl DelegateTool {
    /// Original synchronous delegation path (extracted for reuse).
    async fn execute_sync(
        &self,
        agent_name: &str,
        prompt: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        self.execute_sync_with_admission(agent_name, prompt, args, DelegateAdmission::Required)
            .await
    }

    async fn execute_sync_with_admission(
        &self,
        agent_name: &str,
        prompt: &str,
        args: &serde_json::Value,
        admission: DelegateAdmission,
    ) -> anyhow::Result<ToolResult> {
        // Keep target recovery metadata local: the parent channel scope belongs to its own model call.
        let (result, fallback) = clawcrew_providers::reliable::scope_provider_fallback(async {
            let result = self
                .execute_sync_with_admission_inner(agent_name, prompt, args, admission)
                .await;
            let fallback = clawcrew_providers::reliable::take_last_provider_fallback_attribution();
            (result, fallback)
        })
        .await;

        let mut result = result?;
        if result.success
            && let Some(fallback) = fallback
        {
            let agentic = self
                .agents
                .get(agent_name)
                .is_some_and(|config| self.resolve_agentic(&config.runtime_profile));

            // Log fallback routing decision to observability layer
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Write)
                    .with_attrs(::serde_json::json!({
                        "agent": agent_name,
                        "requested_candidate": fallback.requested_candidate,
                        "actual_candidate": fallback.actual_candidate,
                        "agentic": agentic
                    })),
                "provider routing fallback selected"
            );

            // Surface the fallback as task metadata when this run is a
            // background delegate task (P1.5), so it appears in the TaskBoard
            // activity feed.
            self.record_provider_fallback_metadata(&fallback).await;

            let warning =
                crate::i18n::get_required_cli_string("delegate-provider-fallback-warning");
            let header_key = if agentic {
                "delegate-provider-fallback-header-agentic"
            } else {
                "delegate-provider-fallback-header"
            };
            let header = crate::i18n::get_required_cli_string_with_args(
                header_key,
                &[
                    ("agent", agent_name),
                    ("requested_provider", &fallback.requested_candidate),
                    ("requested_model", &fallback.fallback.requested_model),
                    ("actual_provider", &fallback.actual_candidate),
                    ("actual_model", &fallback.fallback.actual_model),
                ],
            );
            // Successful delegate results are always headed by one generated line. Re-rendering
            // it here keeps the caller-visible provenance accurate without exposing rejected
            // provider diagnostics, endpoints, or credentials.
            let rendered = result
                .output
                .split_once('\n')
                .map_or(result.output.as_str(), |(_, rendered)| rendered);
            result.output = format!("{header}\n{rendered}\n\n{warning}").into();
        }
        Ok(result)
    }

    /// Record a provider fallback as task metadata on the ambient delegate
    /// task, when one exists. No-op for the synchronous path (no task row).
    async fn record_provider_fallback_metadata(
        &self,
        fallback: &clawcrew_providers::reliable::ProviderFallbackAttribution,
    ) {
        // Session metadata projection (P1.5): independent of any task row.
        if let Some(session_key) = current_tool_loop_session_key() {
            crate::session::metadata::record_provider_fallback(
                &session_key,
                crate::session::metadata::SessionProviderFallback {
                    requested_provider: fallback.requested_candidate.clone(),
                    actual_provider: fallback.actual_candidate.clone(),
                    requested_model: fallback.fallback.requested_model.clone(),
                    actual_model: fallback.fallback.actual_model.clone(),
                    at: chrono::Utc::now().to_rfc3339(),
                },
            );
        }

        let Ok(task_id) = DELEGATE_TASK_ID.try_with(|id| id.clone()) else {
            return;
        };
        let Some(control_plane) = crate::control_plane::control_plane() else {
            return;
        };
        let _ = control_plane
            .store
            .record_task_event(
                &task_id,
                "provider_fallback",
                &serde_json::json!({
                    "requested_provider": fallback.requested_candidate,
                    "requested_model": fallback.fallback.requested_model,
                    "actual_provider": fallback.actual_candidate,
                    "actual_model": fallback.fallback.actual_model,
                }),
            )
            .await;
    }

    async fn execute_sync_with_admission_inner(
        &self,
        agent_name: &str,
        prompt: &str,
        args: &serde_json::Value,
        admission: DelegateAdmission,
    ) -> anyhow::Result<ToolResult> {
        let context = args
            .get("context")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");

        // Look up agent config
        let agent_config = match self.agents.get(agent_name) {
            Some(cfg) => cfg,
            None => {
                let available: Vec<&str> =
                    self.agents.keys().map(|s: &String| s.as_str()).collect();
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!(
                        "Unknown agent '{agent_name}'. Available agents: {}",
                        if available.is_empty() {
                            "(none configured)".to_string()
                        } else {
                            available.join(", ")
                        }
                    )),
                });
            }
        };

        // Resolve profile references
        let max_depth = self.effective_max_depth(&agent_config.runtime_profile);
        let (legacy_provider_type, credential, _, temperature) =
            self.resolve_brain(&agent_config.model_provider);
        let agentic = self.resolve_agentic(&agent_config.runtime_profile);

        // Check recursion depth (immutable — set at construction, incremented for sub-agents)
        if self.depth >= max_depth {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(format!(
                    "Delegation depth limit reached ({depth}/{max}). \
                     Cannot delegate further to prevent infinite loops.",
                    depth = self.depth,
                    max = max_depth
                )),
            });
        }

        if admission == DelegateAdmission::Required {
            if let Some(refusal) = self.operator_approval_refusal() {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(refusal),
                });
            }

            if let Err(error) = self
                .security
                .enforce_tool_operation(ToolOperation::Act, "delegate")
            {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(error),
                });
            }

            if let Err(e) = self.policy_for_target(agent_name) {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!("{e:#}")),
                });
            }
            if let Some(refusal) = self.unsupported_agentic_always_ask_refusal(agent_name) {
                return Ok(refusal);
            }
        }

        // Create model_provider for this agent
        let (model_provider, provider_type, model) = match self.build_target_provider(
            &agent_config.model_provider,
            &legacy_provider_type,
            credential.as_deref(),
        ) {
            Ok(provider) => provider,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!(
                        "Failed to create model_provider '{legacy_provider_type}' for agent '{agent_name}': {e}"
                    )),
                });
            }
        };

        // Build the message
        let full_prompt = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("[Context]\n{context}\n\n[Task]\n{prompt}")
        };

        // Agentic mode: run full tool-call loop with allowlisted tools.
        if agentic {
            return self
                .execute_agentic_with_admission(
                    agent_name,
                    agent_config,
                    &provider_type,
                    &model,
                    &*model_provider,
                    &full_prompt,
                    temperature,
                    admission,
                )
                .await;
        }

        // Build enriched system prompt for non-agentic sub-agent.
        let enriched_system_prompt = self.build_enriched_system_prompt(
            agent_name,
            agent_config,
            &model,
            &[],
            &self.workspace_dir,
            false,
            None,
            None,
        );
        let system_prompt_ref = enriched_system_prompt.as_deref();

        // Wrap the model_provider call in a timeout to prevent indefinite blocking
        let timeout_secs = self
            .resolve_delegation_timeout(&agent_config.runtime_profile)
            .unwrap_or(self.delegate_config.timeout_secs);
        let dispatcher = ProviderDispatch::from_ref(&*model_provider);
        let result = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            dispatcher.chat_with_system(system_prompt_ref, &full_prompt, &model, temperature),
        )
        .await;

        let result = match result {
            Ok(inner) => inner,
            Err(_elapsed) => {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!(
                        "Agent '{agent_name}' timed out after {timeout_secs}s"
                    )),
                });
            }
        };

        Ok(Self::render_non_agentic_result(
            agent_name,
            &provider_type,
            &model,
            result,
        ))
    }

    fn render_non_agentic_result(
        agent_name: &str,
        provider_type: &str,
        model: &str,
        result: anyhow::Result<String>,
    ) -> ToolResult {
        match result {
            Ok(response)
                if clawcrew_api::model_provider::strip_think_tags(&response).is_empty() =>
            {
                ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(invalid_semantic_completion_error(agent_name)),
                }
            }
            Ok(response) => ToolResult {
                success: true,
                output: format!("[Agent '{agent_name}' ({provider_type}/{model})]\n{response}",)
                    .into(),
                error: None,
            },
            Err(e) => ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(delegate_failure_error(agent_name, &e)),
            },
        }
    }
}

impl DelegateTool {
    // ── Background Execution ────────────────────────────────────────

    /// Spawn a sub-agent in a background tokio task. Returns a task_id immediately.
    /// The result is persisted to `workspace/delegate_results/{task_id}.json`.
    async fn execute_background(
        &self,
        agent_name: &str,
        prompt: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        // Validate agent exists and check depth/security before spawning
        let agent_config = match self.agents.get(agent_name) {
            Some(cfg) => cfg.clone(),
            None => {
                let available: Vec<&str> =
                    self.agents.keys().map(|s: &String| s.as_str()).collect();
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!(
                        "Unknown agent '{agent_name}'. Available agents: {}",
                        if available.is_empty() {
                            "(none configured)".to_string()
                        } else {
                            available.join(", ")
                        }
                    )),
                });
            }
        };

        let max_depth = self.effective_max_depth(&agent_config.runtime_profile);
        if self.depth >= max_depth {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(format!(
                    "Delegation depth limit reached ({depth}/{max}).",
                    depth = self.depth,
                    max = max_depth
                )),
            });
        }

        if let Some(refusal) = self.operator_approval_refusal() {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(refusal),
            });
        }

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "delegate")
        {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(error),
            });
        }

        let target_policy = match self.policy_for_target(agent_name) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!("{e:#}")),
                });
            }
        };
        if let Some(refusal) = self.unsupported_agentic_always_ask_refusal(agent_name) {
            return Ok(refusal);
        }

        // Runaway backstop: refuse a new background delegation once too many are already in
        // flight (each is a full agent loop). The in-flight set is the live cancel-token map.
        if Self::at_background_capacity(
            Self::background_task_cancels().lock().len(),
            Self::MAX_CONCURRENT_BACKGROUND_DELEGATIONS,
        ) {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(format!(
                    "Too many background delegations in flight (limit {}). Wait for some to \
                     finish (check_result) or cancel one (cancel_task) before starting more.",
                    Self::MAX_CONCURRENT_BACKGROUND_DELEGATIONS
                )),
            });
        }

        let Some(caller_identity) = self.caller_identity().map(str::to_owned) else {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(
                    "Cannot start background delegation: caller identity is unavailable".into(),
                ),
            });
        };

        let task_control_plane = match self.background_control_plane().await {
            Ok(handle) => handle,
            Err(error) => {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!("Cannot start background delegation: {error:#}")),
                });
            }
        };

        let task_id = uuid::Uuid::new_v4().to_string();
        let results_dir = self.results_dir();
        tokio::fs::create_dir_all(&results_dir).await?;

        let context = args
            .get("context")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        let full_prompt = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("[Context]\n{context}\n\n[Task]\n{prompt}")
        };

        let started_at = chrono::Utc::now().to_rfc3339();
        let agent_name_owned = agent_name.to_string();

        // The workspace artifact scopes listing and stores output, but never owns
        // lifecycle status. Status is created atomically in the task store below.
        let initial_result = BackgroundDelegateOutput {
            task_id: task_id.clone(),
            output: None,
        };
        let result_path =
            Self::durable_artifact_path(&results_dir.join(format!("{task_id}.json")))?;
        Self::write_result_atomic(&result_path, &initial_result).await?;

        if let Err(error) = task_control_plane
            .store
            .create(crate::control_plane::TaskRecord {
                id: task_id.clone(),
                kind: crate::control_plane::TaskKind::Delegate,
                agent: agent_name_owned.clone(),
                status: crate::control_plane::TaskStatus::Running,
                owner_pid: std::process::id(),
                owner_boot_id: task_control_plane.boot_id.clone(),
                heartbeat_at: None,
                depth: self.depth,
                parent_id: None,
                originator_route: Some(caller_identity),
                delivered: false,
                idem_key: None,
                principal_id: None,
                session_key: None,
                workspace: None,
                cancellation_state: crate::control_plane::task_registry::CancellationState::None,
                checkpoint_id: None,
                recovery_outcome: Default::default(),
                started_at: started_at.clone(),
                finished_at: None,
            })
            .await
        {
            let _ = tokio::fs::remove_file(&result_path).await;
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(format!(
                    "Cannot start background delegation: task registration failed: {error:#}"
                )),
            });
        }

        let agents = Arc::clone(&self.agents);
        let security = target_policy;
        let global_credential = self.global_credential.clone();
        let provider_runtime_options = self.provider_runtime_options.clone();
        // Depth ownership: a logical hop increments depth exactly once, at the
        // target-bound sub-delegate tool built in the bounded assembly. This
        // wrapper re-executes the SAME hop, so it inherits `depth` and the
        // carried depth ceiling verbatim; incrementing here double-counted
        // background hops and refused second hops one level early.
        let depth = self.depth;
        let max_delegation_depth = self.max_delegation_depth;
        let background_task_management = self.background_task_management;
        let operator_approval_available = self.operator_approval_available;
        let parent_tools = Arc::clone(&self.parent_tools);
        let runtime = self.runtime.clone();
        let multimodal_config = self.multimodal_config.clone();
        let delegate_config = self.delegate_config.clone();
        let workspace_dir = self.workspace_dir.clone();
        let child_token = self.cancellation_token.child_token();
        // Register the live token so `cancel_task` can actually abort THIS task (removed
        // when it settles, in the spawned closure below).
        Self::background_task_cancels()
            .lock()
            .insert(task_id.clone(), child_token.clone());
        let task_id_clone = task_id.clone();
        let providers_models = Arc::clone(&self.providers_models);
        let risk_profiles = Arc::clone(&self.risk_profiles);
        let runtime_profiles = Arc::clone(&self.runtime_profiles);
        let skill_bundles = Arc::clone(&self.skill_bundles);
        let root_config = self.root_config.clone();
        // Carried, not dropped: the background task rebuilds a DelegateTool that
        // will construct its own nested registries.
        let live_config = self.live_config.clone();
        let caller_alias = self.caller_alias.clone();
        let nested_task_control_plane = Arc::clone(&self.task_control_plane);
        let terminal_store = Arc::clone(&task_control_plane.store);
        let terminal_owner_pid = std::process::id();
        let terminal_owner_boot_id = task_control_plane.boot_id.clone();
        let memory = self.memory.clone();
        let parent_session_key = current_tool_loop_session_key();
        // Sender-bucket continuity (same rationale as the parallel spawn):
        // capture the originating sender scope so every admission inside the
        // detached task charges the caller's bucket, not the fallback
        // __global__ budget.
        let parent_thread_id = TOOL_LOOP_THREAD_ID.try_with(|v| v.clone()).ok().flatten();
        let __zc_delegate_alias = agent_name_owned.clone();
        let ambient_task_id = task_id.clone();

        clawcrew_spawn::spawn!(
            TOOL_LOOP_THREAD_ID.scope(
                parent_thread_id,
                DELEGATE_TASK_ID.scope(ambient_task_id, scope_delegate_session_key(parent_session_key, async move {
                let inner = DelegateTool {
                    agents,
                    security,
                    global_credential,
                    provider_runtime_options,
                    depth,
                    max_delegation_depth,
                    background_task_management,
                    operator_approval_available,
                    parent_tools,
                    runtime,
                    multimodal_config,
                    delegate_config,
                    workspace_dir: workspace_dir.clone(),
                    cancellation_token: child_token.clone(),
                    memory,
                    providers_models,
                    risk_profiles,
                    runtime_profiles,
                    skill_bundles,
                    root_config,
                    live_config,
                    caller_alias,
                    task_control_plane: nested_task_control_plane,
                };

                let args_inner = json!({
                    "agent": agent_name_owned,
                    "prompt": full_prompt,
                });

                // Race the delegation against cancellation
                let outcome = tokio::select! {
                    () = child_token.cancelled() => {
                        Err("Cancelled by parent session".to_string())
                    }
                    result = Box::pin(inner.execute_sync_with_admission(
                        &agent_name_owned,
                        &full_prompt,
                        &args_inner,
                        DelegateAdmission::Prevalidated,
                    )) => {
                        match result {
                            Ok(tool_result) => {
                                if tool_result.success {
                                    Ok(tool_result.output.into_string())
                                } else {
                                    Err(tool_result.error.unwrap_or_else(|| "Unknown error".into()))
                                }
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    }
                };

                drop(inner);
                drop(args_inner);
                drop(agent_name_owned);
                drop(full_prompt);
                drop(workspace_dir);
                drop(child_token);
                let (terminal_status, terminal_error, final_result) = match outcome {
                    Ok(output) => (
                        crate::control_plane::TaskStatus::Completed,
                        None,
                        BackgroundDelegateOutput {
                            task_id: task_id_clone.clone(),
                            output: Some(output),
                        },
                    ),
                    Err(err) => {
                        let status = if err.contains("Cancelled") {
                            crate::control_plane::TaskStatus::Cancelled
                        } else {
                            crate::control_plane::TaskStatus::Failed
                        };
                        (
                            status,
                            Some(err),
                            BackgroundDelegateOutput {
                                task_id: task_id_clone.clone(),
                                output: None,
                            },
                        )
                    }
                };

                let _won = DelegateTool::complete_background_task(
                    terminal_store.as_ref(),
                    &result_path,
                    final_result,
                    terminal_status,
                    terminal_error,
                    terminal_owner_pid,
                    terminal_owner_boot_id,
                )
                .await;
                })),
            )
            .instrument(::clawcrew_log::attribution_span!(
                &crate::agent::AgentAttribution(__zc_delegate_alias.as_str())
            ))
        );

        Ok(ToolResult {
            success: true,
            output: if self.background_task_management {
                format!(
                    "Background task started for agent '{agent_name}'.\n\
                     task_id: {task_id}\n\
                     Use action='check_result' with task_id='{task_id}' to retrieve the result."
                )
            } else {
                format!(
                    "Background task started for agent '{agent_name}'.\n\
                     task_id: {task_id}\n\
                     This tool cannot check task results, and the task record is owned by this \
                     tool's configured caller identity; retrieval through the delegate API is not \
                     available to it."
                )
            }
            .into(),
            error: None,
        })
    }

    // ── Parallel Execution ──────────────────────────────────────────

    /// Run multiple agents concurrently with the same prompt.
    async fn execute_parallel(
        &self,
        parallel_agents: &[serde_json::Value],
        args: &serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"param": "prompt"})),
                    "tool argument validation failed"
                );

                anyhow::Error::msg("Missing 'prompt' parameter for parallel execution")
            })?;

        if prompt.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some("'prompt' parameter must not be empty".into()),
            });
        }

        let agent_names: Vec<String> = parallel_agents
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();

        if agent_names.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some("'parallel' array must contain at least one agent name".into()),
            });
        }

        // Validate all agents exist before starting any
        for name in &agent_names {
            if !self.agents.contains_key(name) {
                let available: Vec<&str> =
                    self.agents.keys().map(|s: &String| s.as_str()).collect();
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!(
                        "Unknown agent '{name}' in parallel list. Available: {}",
                        if available.is_empty() {
                            "(none configured)".to_string()
                        } else {
                            available.join(", ")
                        }
                    )),
                });
            }
        }

        for name in &agent_names {
            // Validate the whole fan-out before any spawn. A single blocked
            // target should fail the entire parallel request rather than
            // launching a partial set of child agents and then reporting mixed
            // results.
            if let Err(e) = self.policy_for_target(name) {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!("{e:#}")),
                });
            }
            if let Some(refusal) = self.unsupported_agentic_always_ask_refusal(name) {
                return Ok(refusal);
            }
        }

        let parent_receipt_scope = crate::agent::tool_receipts::TOOL_LOOP_RECEIPT_CONTEXT
            .try_with(Clone::clone)
            .ok()
            .flatten();
        let parent_session_key = current_tool_loop_session_key();

        // Spawn all agents concurrently
        let mut handles = Vec::with_capacity(agent_names.len());
        // Sender-bucket continuity: spawned tasks start with empty
        // task-locals, so a worker that restores only the session key would
        // charge the fallback __global__ bucket and escape the originating
        // sender's action budget. Capture the sender scope before spawning
        // and restore it around each worker's entire execution.
        let parent_thread_id = TOOL_LOOP_THREAD_ID.try_with(|v| v.clone()).ok().flatten();
        for agent_name in &agent_names {
            let agents = Arc::clone(&self.agents);
            let security = Arc::clone(&self.security);
            let global_credential = self.global_credential.clone();
            let provider_runtime_options = self.provider_runtime_options.clone();
            // Depth ownership on the parallel path mirrors the background
            // wrapper: the fan-out task re-executes the SAME hop, so it
            // inherits `depth` and the carried ceiling verbatim; the single
            // increment lives in the bounded sub-delegate construction.
            let depth = self.depth;
            let max_delegation_depth = self.max_delegation_depth;
            let background_task_management = self.background_task_management;
            let operator_approval_available = self.operator_approval_available;
            let parent_tools = Arc::clone(&self.parent_tools);
            let runtime = self.runtime.clone();
            let multimodal_config = self.multimodal_config.clone();
            let delegate_config = self.delegate_config.clone();
            let workspace_dir = self.workspace_dir.clone();
            let cancellation_token = self.cancellation_token.child_token();
            let agent_name = agent_name.clone();
            let prompt = prompt.to_string();
            let args_clone = args.clone();
            let providers_models = Arc::clone(&self.providers_models);
            let risk_profiles = Arc::clone(&self.risk_profiles);
            let runtime_profiles = Arc::clone(&self.runtime_profiles);
            let skill_bundles = Arc::clone(&self.skill_bundles);
            let receipt_scope = parent_receipt_scope.clone();
            let root_config = self.root_config.clone();
            // Carried, not dropped: each fan-out task rebuilds a DelegateTool
            // that will construct its own nested registries.
            let live_config = self.live_config.clone();
            let caller_alias = self.caller_alias.clone();
            let session_key = parent_session_key.clone();
            let thread_scope = parent_thread_id.clone();
            let memory = self.memory.clone();
            let task_control_plane = Arc::clone(&self.task_control_plane);
            let __zc_delegate_alias = agent_name.clone();

            handles.push(clawcrew_spawn::spawn!(
                async move {
                    let inner = DelegateTool {
                        agents,
                        security,
                        global_credential,
                        provider_runtime_options,
                        depth,
                        max_delegation_depth,
                        background_task_management,
                        operator_approval_available,
                        parent_tools,
                        runtime,
                        multimodal_config,
                        delegate_config,
                        workspace_dir,
                        cancellation_token,
                        memory,
                        providers_models,
                        risk_profiles,
                        runtime_profiles,
                        skill_bundles,
                        root_config,
                        live_config,
                        caller_alias,
                        task_control_plane,
                    };
                    let agent_name_for_return = agent_name.clone();
                    let result = TOOL_LOOP_THREAD_ID
                        .scope(
                            thread_scope,
                            scope_delegate_session_key(session_key, async move {
                                crate::agent::tool_receipts::TOOL_LOOP_RECEIPT_CONTEXT
                                    .scope(receipt_scope, async move {
                                        Box::pin(inner.execute_sync(
                                            &agent_name,
                                            &prompt,
                                            &args_clone,
                                        ))
                                        .await
                                    })
                                    .await
                            }),
                        )
                        .await;
                    (agent_name_for_return, result)
                }
                .instrument(::clawcrew_log::attribution_span!(
                    &crate::agent::AgentAttribution(__zc_delegate_alias.as_str())
                ))
            ));
        }

        // Collect all results
        let mut outputs = Vec::with_capacity(handles.len());
        let mut all_success = true;

        for handle in handles {
            match handle.await {
                Ok((agent_name, Ok(tool_result))) => {
                    if !tool_result.success {
                        all_success = false;
                    }
                    outputs.push(format!(
                        "--- {agent_name} (success={}) ---\n{}{}",
                        tool_result.success,
                        tool_result.output,
                        tool_result
                            .error
                            .map(|e| format!("\nError: {e}"))
                            .unwrap_or_default()
                    ));
                }
                Ok((agent_name, Err(e))) => {
                    all_success = false;
                    outputs.push(format!("--- {agent_name} (success=false) ---\nError: {e}"));
                }
                Err(e) => {
                    all_success = false;
                    outputs.push(format!("--- [join error] ---\n{e}"));
                }
            }
        }

        Ok(ToolResult {
            success: all_success,
            output: format!(
                "[Parallel delegation: {} agents]\n\n{}",
                agent_names.len(),
                outputs.join("\n\n")
            )
            .into(),
            error: if all_success {
                None
            } else {
                Some("One or more parallel agents failed".into())
            },
        })
    }

    // ── Result Retrieval ────────────────────────────────────────────

    async fn read_stored_background_output(
        &self,
        task_id: &str,
    ) -> anyhow::Result<Option<StoredBackgroundOutput>> {
        let result_path = self.results_dir().join(format!("{task_id}.json"));
        let content = match tokio::fs::read_to_string(&result_path).await {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let value: serde_json::Value = serde_json::from_str(&content)?;
        if value.get("status").is_some() {
            let legacy: BackgroundDelegateResult = serde_json::from_value(value)?;
            anyhow::ensure!(
                legacy.task_id == task_id,
                "delegate output task id does not match its filename"
            );
            return Ok(Some(StoredBackgroundOutput {
                output: BackgroundDelegateOutput {
                    task_id: legacy.task_id,
                    output: legacy.output,
                },
                legacy_agent: Some(legacy.agent),
                legacy_status: Some(legacy.status),
                legacy_error: legacy.error,
                legacy_started_at: Some(legacy.started_at),
                legacy_finished_at: legacy.finished_at,
            }));
        }
        let output: BackgroundDelegateOutput = serde_json::from_value(value)?;
        anyhow::ensure!(
            output.task_id == task_id,
            "delegate output task id does not match its filename"
        );
        Ok(Some(StoredBackgroundOutput {
            output,
            legacy_agent: None,
            legacy_status: None,
            legacy_error: None,
            legacy_started_at: None,
            legacy_finished_at: None,
        }))
    }

    async fn read_background_view(
        &self,
        task_id: &str,
        allow_legacy_terminal: bool,
    ) -> anyhow::Result<Option<(BackgroundResultState, serde_json::Value, Option<String>)>> {
        let control_plane = self.background_control_plane().await?;
        if let Some(snapshot) = control_plane.store.get_snapshot(task_id).await? {
            if !(self.owns_delegate_task(&snapshot.task)
                || allow_legacy_terminal && self.can_read_delegate_task(&snapshot.task))
            {
                return Ok(None);
            }
            let state = BackgroundResultState::from_task_status(snapshot.task.status);
            let mut task_error = snapshot.error;
            let output = if state == BackgroundResultState::Completed {
                match snapshot.output {
                    Some(output_ref) => {
                        if let Some(filename) =
                            output_ref.strip_prefix(Self::OUTPUT_ARTIFACT_PREFIX)
                        {
                            let expected = format!("{task_id}.json");
                            if filename != expected {
                                task_error.get_or_insert_with(|| {
                                    format!(
                                        "delegate output reference '{filename}' does not match task '{task_id}'"
                                    )
                                });
                                None
                            } else {
                                match self.read_stored_background_output(task_id).await {
                                    Ok(Some(stored)) => stored.output.output,
                                    Ok(None) => {
                                        task_error.get_or_insert_with(|| {
                                            format!(
                                                "delegate output artifact '{filename}' is missing"
                                            )
                                        });
                                        None
                                    }
                                    Err(error) => {
                                        task_error.get_or_insert_with(|| {
                                            format!(
                                                "delegate output artifact '{filename}' is unreadable: {error:#}"
                                            )
                                        });
                                        None
                                    }
                                }
                            }
                        } else {
                            Some(output_ref)
                        }
                    }
                    None => None,
                }
            } else {
                None
            };
            let note = matches!(state, BackgroundResultState::Lost | BackgroundResultState::TimedOut)
                .then_some(
                    "the owning daemon exited or the task exceeded its max runtime; reconciled by the supervision reaper",
                );
            return Ok(Some((
                state,
                json!({
                    "task_id": task_id,
                    "agent": snapshot.task.agent,
                    "status": state.as_str(),
                    "output": output,
                    "error": task_error.clone(),
                    "started_at": snapshot.task.started_at,
                    "finished_at": snapshot.task.finished_at,
                    "note": note,
                }),
                task_error,
            )));
        }

        let stored = self.read_stored_background_output(task_id).await?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        let Some(status) = stored.legacy_status else {
            return Ok(None);
        };
        let state = BackgroundResultState::from_file_status(&status);
        Ok(Some((
            state,
            json!({
                "task_id": stored.output.task_id,
                "agent": stored.legacy_agent,
                "status": state.as_str(),
                "output": stored.output.output,
                "error": stored.legacy_error,
                "started_at": stored.legacy_started_at,
                "finished_at": stored.legacy_finished_at,
            }),
            stored.legacy_error,
        )))
    }

    fn task_ids_from_args(args: &serde_json::Value) -> anyhow::Result<Vec<String>> {
        let values = args
            .get("task_ids")
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow::Error::msg("Missing 'task_ids' parameter for await_sessions"))?;
        if values.len() > Self::MAX_AWAIT_SESSION_TASK_IDS {
            return Err(anyhow::Error::msg(format!(
                "'task_ids' must contain no more than {} task ids",
                Self::MAX_AWAIT_SESSION_TASK_IDS
            )));
        }
        let mut task_ids = Vec::with_capacity(values.len());
        let mut seen = HashSet::with_capacity(values.len());
        for value in values {
            let Some(task_id) = value.as_str() else {
                return Err(anyhow::Error::msg("'task_ids' must contain only strings"));
            };
            Self::validate_task_id(task_id).map_err(anyhow::Error::msg)?;
            if !seen.insert(task_id) {
                return Err(anyhow::Error::msg(format!(
                    "Duplicate task_id '{task_id}' in task_ids"
                )));
            }
            task_ids.push(task_id.to_string());
        }
        if task_ids.is_empty() {
            return Err(anyhow::Error::msg(
                "'task_ids' must contain at least one task id",
            ));
        }
        Ok(task_ids)
    }

    fn await_timeout(args: &serde_json::Value) -> anyhow::Result<Duration> {
        let Some(value) = args.get("timeout_ms") else {
            return Ok(Duration::from_millis(30_000));
        };
        let Some(timeout_ms) = value.as_u64() else {
            return Err(anyhow::Error::msg("'timeout_ms' must be an integer"));
        };
        let timeout = Duration::from_millis(timeout_ms);
        if timeout > Self::MAX_AWAIT_SESSIONS_TIMEOUT {
            return Err(anyhow::Error::msg(format!(
                "'timeout_ms' must be no more than {}",
                Self::MAX_AWAIT_SESSIONS_TIMEOUT.as_millis()
            )));
        }
        Ok(timeout)
    }

    /// Retrieve the result of a background delegate task by task_id.
    async fn handle_check_result(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"param": "task_id"})),
                    "tool argument validation failed"
                );

                anyhow::Error::msg("Missing 'task_id' parameter for check_result")
            })?;

        if let Err(e) = Self::validate_task_id(task_id) {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(e),
            });
        }

        let Some((state, value, task_error)) = self.read_background_view(task_id, true).await?
        else {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(format!("No result found for task_id '{task_id}'")),
            });
        };
        let success = state.is_success() && task_error.is_none();

        Ok(ToolResult {
            success,
            output: serde_json::to_string_pretty(&value)?.into(),
            error: if let Some(error) = task_error {
                Some(error)
            } else if success {
                None
            } else if state.is_failure() {
                Some(format!(
                    "background task is {} and will not complete",
                    state.as_str()
                ))
            } else {
                None
            },
        })
    }

    async fn handle_await_sessions(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let task_ids = match Self::task_ids_from_args(args) {
            Ok(task_ids) => task_ids,
            Err(error) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new().into(),
                    error: Some(error.to_string()),
                });
            }
        };
        let timeout = match Self::await_timeout(args) {
            Ok(timeout) => timeout,
            Err(error) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new().into(),
                    error: Some(error.to_string()),
                });
            }
        };
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let mut results = Vec::new();
            let mut pending = Vec::new();
            let mut missing = Vec::new();
            let mut failed = Vec::new();

            for task_id in &task_ids {
                let Some((state, value, task_error)) =
                    self.read_background_view(task_id, true).await?
                else {
                    missing.push(task_id.clone());
                    continue;
                };
                if state.is_pending() {
                    pending.push(task_id.clone());
                } else if state.is_failure() || task_error.is_some() {
                    failed.push(task_id.clone());
                }
                results.push(value);
            }

            let waiting = !pending.is_empty() || !missing.is_empty();
            let timed_out = waiting && tokio::time::Instant::now() >= deadline;
            if !waiting || timed_out {
                let completed = results
                    .iter()
                    .filter(|result| result.get("status") == Some(&json!("completed")))
                    .count();
                let success = missing.is_empty() && pending.is_empty() && failed.is_empty();
                let error = if success {
                    None
                } else if timed_out {
                    Some("one or more background tasks are still pending or missing".into())
                } else {
                    Some(
                        "one or more background tasks failed, were cancelled, or have unreadable output"
                            .into(),
                    )
                };
                return Ok(ToolResult {
                    success,
                    output: serde_json::to_string_pretty(&json!({
                        "status": if timed_out { "timeout" } else { "complete" },
                        "completed": completed,
                        "pending": pending,
                        "missing": missing,
                        "failed": failed,
                        "results": results,
                    }))?
                    .into(),
                    error,
                });
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// List all background delegate task results.
    async fn handle_list_results(&self) -> anyhow::Result<ToolResult> {
        let results_dir = self.results_dir();
        if !results_dir.exists() {
            return Ok(ToolResult {
                success: true,
                output: "No background delegate results found.".into(),
                error: None,
            });
        }

        let mut entries = tokio::fs::read_dir(&results_dir).await?;
        let mut results = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Some(task_id) = path.file_stem().and_then(|value| value.to_str())
                && Self::validate_task_id(task_id).is_ok()
                && let Some((_, result, _)) = self.read_background_view(task_id, false).await?
            {
                results.push(json!({
                    "task_id": result.get("task_id"),
                    "agent": result.get("agent"),
                    "status": result.get("status"),
                    "error": result.get("error"),
                    "started_at": result.get("started_at"),
                    "finished_at": result.get("finished_at"),
                }));
            }
        }

        if results.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: "No background delegate results found.".into(),
                error: None,
            });
        }

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&results)?.into(),
            error: None,
        })
    }

    fn background_task_cancels() -> &'static parking_lot::Mutex<HashMap<String, CancellationToken>>
    {
        static M: std::sync::OnceLock<parking_lot::Mutex<HashMap<String, CancellationToken>>> =
            std::sync::OnceLock::new();
        M.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
    }

    /// Runaway backstop: the maximum number of background delegations allowed in flight at
    /// once across the process. Each is a full agent loop, so this guards against a model
    /// (or a runaway loop) spawning unbounded background agent runs; normal use stays well
    /// under it.
    const MAX_CONCURRENT_BACKGROUND_DELEGATIONS: usize = 128;

    /// Pure predicate for the runaway backstop — separated from the live token-map read so
    /// it is unit-testable. `cap == 0` disables the backstop.
    fn at_background_capacity(in_flight: usize, cap: usize) -> bool {
        cap != 0 && in_flight >= cap
    }

    /// Cancel a running background task by task_id.
    async fn handle_cancel_task(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"param": "task_id"})),
                    "tool argument validation failed"
                );

                anyhow::Error::msg("Missing 'task_id' parameter for cancel_task")
            })?;

        if let Err(e) = Self::validate_task_id(task_id) {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(e),
            });
        }

        let control_plane = self.background_control_plane().await?;
        if let Some(snapshot) = control_plane.store.get_snapshot(task_id).await? {
            if !self.owns_delegate_task(&snapshot.task) {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!("No task found for task_id '{task_id}'")),
                });
            }
            if snapshot.task.status != crate::control_plane::TaskStatus::Running {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!(
                        "Task '{task_id}' is not running (status: {:?})",
                        snapshot.task.status
                    )),
                });
            }

            let (won, aborted) = Self::settle_background_cancellation(task_id, || {
                control_plane.store.transition_terminal(
                    task_id,
                    crate::control_plane::TaskStatus::Cancelled,
                    None,
                    Some("cancelled by user request".into()),
                )
            })
            .await?;
            if !won {
                let status = control_plane
                    .store
                    .get(task_id)
                    .await?
                    .map(|task| format!("{:?}", task.status))
                    .unwrap_or_else(|| "missing".into());
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!(
                        "Task '{task_id}' is not running (status: {status})"
                    )),
                });
            }
            return Ok(ToolResult {
                success: true,
                output: if aborted {
                    format!("Task '{task_id}' cancelled: the running task was aborted.").into()
                } else {
                    format!("Task '{task_id}' marked cancelled (it had already settled).").into()
                },
                error: None,
            });
        }

        // Compatibility path for legacy result files without a task row.
        let result_path = self.results_dir().join(format!("{task_id}.json"));
        let content = match tokio::fs::read_to_string(&result_path).await {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!("No task found for task_id '{task_id}'")),
                });
            }
            Err(error) => return Err(error.into()),
        };
        let mut result: BackgroundDelegateResult = match serde_json::from_str(&content) {
            Ok(result) => result,
            Err(_) => {
                return Ok(ToolResult {
                    success: false,
                    output: ToolOutput::default(),
                    error: Some(format!("No task found for task_id '{task_id}'")),
                });
            }
        };
        if result.status != BackgroundTaskStatus::Running {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(format!(
                    "Task '{task_id}' is not running (status: {:?})",
                    result.status
                )),
            });
        }
        let aborted = Self::background_task_cancels()
            .lock()
            .remove(task_id)
            .inspect(CancellationToken::cancel)
            .is_some();

        result.status = BackgroundTaskStatus::Cancelled;
        result.error = Some("Cancelled by user request".into());
        result.finished_at = Some(chrono::Utc::now().to_rfc3339());
        Self::write_result_atomic(&result_path, &result).await?;

        Ok(ToolResult {
            success: true,
            output: if aborted {
                format!("Task '{task_id}' cancelled: the running task was aborted.").into()
            } else {
                format!("Task '{task_id}' marked cancelled (it had already settled).").into()
            },
            error: None,
        })
    }

    /// Cancel all background tasks (cascade control).
    /// Call this when the parent session ends.
    pub fn cancel_all_background_tasks(&self) {
        self.cancellation_token.cancel();
    }

    fn compose_independent_system_prompt(
        base: Option<String>,
        mut deferred_section: String,
        native_tools: bool,
        strict_tool_parsing: bool,
    ) -> Option<String> {
        let mut ignored_tool_descs: Vec<(&str, &str)> = Vec::new();
        apply_text_tool_prompt_policy(
            native_tools,
            strict_tool_parsing,
            &mut ignored_tool_descs,
            &mut deferred_section,
        );
        if deferred_section.is_empty() {
            return base;
        }
        match base {
            Some(mut p) => {
                p.push_str("\n\n");
                p.push_str(&deferred_section);
                Some(p)
            }
            None => Some(deferred_section),
        }
    }

    fn build_enriched_system_prompt(
        &self,
        agent_alias: &str,
        agent_config: &AliasedAgentConfig,
        model_name: &str,
        sub_tools: &[Box<dyn Tool>],
        workspace_dir: &Path,
        sends_native_tool_specs: bool,
        skills_override: Option<&[crate::skills::Skill]>,
        approval_policy: Option<&ApprovalManager>,
    ) -> Option<String> {
        let mut resolved_agent_config = agent_config.clone();
        resolved_agent_config.resolved = self.resolve_loop_runtime(agent_alias, agent_config);
        let agent_config = &resolved_agent_config;

        let resolved_skills: Vec<crate::skills::Skill>;
        let skills: &[crate::skills::Skill] = match skills_override {
            Some(s) => s,
            None => {
                let bundle_dirs = self.resolve_skill_bundle_dirs(&agent_config.skill_bundles);
                resolved_skills = if bundle_dirs.is_empty() {
                    let default_dir = crate::skills::skills_dir(workspace_dir);
                    crate::skills::load_skills_from_directory(&default_dir, false).0
                } else {
                    bundle_dirs
                        .into_iter()
                        .flat_map(|dir| {
                            crate::skills::load_skills_from_directory(
                                &workspace_dir.join(dir),
                                false,
                            )
                            .0
                        })
                        .collect()
                };
                &resolved_skills
            }
        };

        let empty_tools: &[Box<dyn Tool>] = &[];
        let expose_text_tools =
            sends_native_tool_specs || !agent_config.resolved.strict_tool_parsing;
        let prompt_tools = if expose_text_tools {
            sub_tools
        } else {
            empty_tools
        };

        let shell_profile = self.runtime.as_ref().and_then(|r| r.shell_profile());

        // Build structured operational context using SystemPromptBuilder sections.
        let dispatcher_instructions = if sends_native_tool_specs || prompt_tools.is_empty() {
            String::new()
        } else {
            XmlToolDispatcher.prompt_instructions(prompt_tools)
        };
        // Independent delegates run under the target's own ApprovalManager, so
        // the prompt must state that exact policy — Full contracts and named
        // `always_ask` exceptions come from the same manager the nested loop's
        // gate consults. Callers without a manager (bounded delegation,
        // non-agentic one-shot) keep the generic default guidance.
        let (autonomy_level, always_ask_values) = match approval_policy {
            Some(mgr) => (mgr.autonomy_level(), mgr.always_ask_tools()),
            None => (crate::security::AutonomyLevel::default(), Vec::new()),
        };
        let ctx = PromptContext {
            workspace_dir,
            agent_workspace_dir: workspace_dir,
            model_name,
            tools: prompt_tools,
            skills,
            skills_prompt_mode: agent_config.resolved.prompt_injection_mode,
            identity_config: None,
            interaction: None,
            dispatcher_instructions: &dispatcher_instructions,
            sends_native_tool_specs: sends_native_tool_specs && !prompt_tools.is_empty(),
            security_summary: None,
            autonomy_level,
            inject_memory: true,
            shell_profile,
        };

        let builder = SystemPromptBuilder::default()
            .add_section(Box::new(crate::agent::prompt::ToolsSection))
            .add_section(Box::new(crate::agent::prompt::SafetySection))
            .add_section(Box::new(crate::agent::prompt::ShellSection))
            .add_section(Box::new(crate::agent::prompt::SkillsSection))
            .add_section(Box::new(crate::agent::prompt::WorkspaceSection))
            .add_section(Box::new(crate::agent::prompt::RuntimeSection))
            .add_section(Box::new(crate::agent::prompt::DateTimeSection));

        let mut enriched = builder
            .build_with_approval_policy(&ctx, &always_ask_values)
            .unwrap_or_default();

        if let Some(target_workspace) = self.agent_workspace(agent_alias) {
            let identity_files = [
                "AGENTS.md",
                "SOUL.md",
                "IDENTITY.md",
                "USER.md",
                "BOOTSTRAP.md",
            ];
            for filename in identity_files {
                let path = target_workspace.join(filename);
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    let trimmed = contents.trim();
                    if !trimmed.is_empty() {
                        enriched.push_str(trimmed);
                        enriched.push_str("\n\n");
                    }
                }
            }
        }

        let trimmed = enriched.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    #[cfg(test)]
    async fn execute_agentic(
        &self,
        agent_name: &str,
        agent_config: &AliasedAgentConfig,
        provider_type: &str,
        model: &str,
        model_provider: &dyn ModelProvider,
        full_prompt: &str,
        temperature: Option<f64>,
    ) -> anyhow::Result<ToolResult> {
        self.execute_agentic_with_admission(
            agent_name,
            agent_config,
            provider_type,
            model,
            model_provider,
            full_prompt,
            temperature,
            DelegateAdmission::Required,
        )
        .await
    }

    async fn execute_agentic_with_admission(
        &self,
        agent_name: &str,
        agent_config: &AliasedAgentConfig,
        provider_type: &str,
        model: &str,
        model_provider: &dyn ModelProvider,
        full_prompt: &str,
        temperature: Option<f64>,
        admission: DelegateAdmission,
    ) -> anyhow::Result<ToolResult> {
        let Some(tool_policy) = self.resolve_tool_policy(&agent_config.risk_profile) else {
            return Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(format!(
                    "Agent '{agent_name}' is agentic but risk_profile '{}' is not configured",
                    agent_config.risk_profile
                )),
            });
        };

        let target_policy = match admission {
            DelegateAdmission::Required => match self.policy_for_target(agent_name) {
                Ok(policy) => policy,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: ToolOutput::default(),
                        error: Some(format!("{e:#}")),
                    });
                }
            },
            DelegateAdmission::Prevalidated => Arc::clone(&self.security),
        };
        let target_mode = self.mode_for_target(agent_name);
        // Independent delegates are fresh, non-interactive target turns. Give the
        // nested loop a fresh manager from the target profile so prompt-required
        // tools fail closed before dispatch; built-in shell remains ungated here
        // and receives approved=false for its own command-policy enforcement.
        let approval_manager = if target_mode == DelegateExecutionMode::Independent {
            self.root_config
                .as_ref()
                .and_then(|config| config.risk_profile_for_agent(agent_name))
                .map(ApprovalManager::for_non_interactive)
        } else {
            None
        };
        // Deferred-MCP side-channels for an INDEPENDENT target: its sub-agent turn must
        // inject the deferred-tools prompt section and thread the activated set, exactly as
        // a fresh target turn does. Bounded delegation leaves these empty (it starts from
        // the parent's already-built registry, not the target's assembled one).
        let mut sub_deferred_section = String::new();
        let mut sub_activated: Option<Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>> = None;
        // For an INDEPENDENT target, build the sub-agent's system prompt (skills, identity)
        // from the TARGET's workspace, not the caller's - so skill *prompt* content matches
        // the skill *tools* assembled above. `None` for bounded delegation, which keeps the
        // caller's `self.workspace_dir`.
        let mut sub_workspace: Option<PathBuf> = None;
        // The target's canonical skills (Some for independent), so the prompt's SkillsSection
        // describes exactly the assembled skill tools rather than the local bundle resolver's
        // narrower view. None for bounded delegation (local resolution).
        let mut sub_skills: Option<Vec<crate::skills::Skill>> = None;
        let sub_tools: crate::tools::scoped::ScopedToolRegistry = match target_mode {
            DelegateExecutionMode::Independent => {
                match self
                    .independent_agentic_tools_for_target(agent_name, Arc::clone(&target_policy))
                    .await
                {
                    Ok(independent) => {
                        sub_deferred_section = independent.deferred_section;
                        sub_activated = independent.activated_handle;
                        sub_workspace = Some(independent.workspace_dir);
                        sub_skills = Some(independent.skills);
                        independent.tools
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: ToolOutput::default(),
                            error: Some(format!(
                                "Failed to initialize independent delegate tools for target '{agent_name}': {e:#}"
                            )),
                        });
                    }
                }
            }
            DelegateExecutionMode::Bounded => {
                let needs_memory_tools = {
                    let parent_tools = self.parent_tools.read();
                    parent_tools.iter().any(|tool| {
                        self.security.is_tool_allowed(tool.name())
                            && clawcrew_tools::MEMORY_TOOL_NAMES.contains(&tool.name())
                            && Self::delegate_admits_with_mcp(&tool_policy, tool.name())
                    })
                };
                let mut target_memory_tools: HashMap<String, Box<dyn Tool>> = if needs_memory_tools
                {
                    match self.memory_for_target_agent(agent_name).await {
                        Ok(Some(memory)) => {
                            Self::memory_tools_for_target(memory, Arc::clone(&target_policy))
                                .into_iter()
                                .map(|tool| (tool.name().to_string(), tool))
                                .collect()
                        }
                        Ok(None) => HashMap::new(),
                        Err(e) => {
                            return Ok(ToolResult {
                                success: false,
                                output: ToolOutput::default(),
                                error: Some(format!(
                                    "Failed to initialize memory for delegate target '{agent_name}': {e:#}"
                                )),
                            });
                        }
                    }
                } else {
                    HashMap::new()
                };

                // The delegate tool is retained in a bounded set only when the
                // TARGET's own risk-profile `delegation_policy` permits
                // delegation - the same gate the top-level delegate tool
                // applies to its callers (L470). The caller-side filters below
                // (`self.security.is_tool_allowed` + `delegate_admits_with_mcp`)
                // still apply to the retained name.
                let target_may_subdelegate = target_policy.delegation_policy.permits();

                // Base bounded set (Arc form): the parent's tools minus the
                // parent's own delegate instance. That instance carries the
                // PARENT's `caller_alias` and security, so handing it to the
                // sub-agent would resolve its delegation calls (reachability,
                // per-target policy, advertised roster) as the parent - a
                // confused-deputy shape. It is replaced below by a target-bound
                // instance. This same list becomes that instance's
                // `parent_tools`, so a further bounded hop inherits exactly the
                // set its delegating parent ran with (that hop re-substitutes
                // its own memory tools by name). The read guard is scoped to
                // this block so it drops BEFORE the `assemble().await` below -
                // a parking_lot guard held across an await would make the
                // delegate future `!Send`.
                let bounded_base_tools: Vec<Arc<dyn Tool>> = {
                    let parent_tools = self.parent_tools.read();
                    parent_tools
                        .iter()
                        .filter(|tool| tool.name() != Self::NAME)
                        .filter(|tool| self.security.is_tool_allowed(tool.name()))
                        .filter(|tool| Self::delegate_admits_with_mcp(&tool_policy, tool.name()))
                        .cloned()
                        .collect()
                };

                // Target-bound delegate tool: its delegation calls must resolve
                // `delegation_policy`, reachability, and the advertised roster
                // from the SUB-agent's identity (alias + policy), never the
                // delegating parent's. Depth ownership: this construction is
                // the single site that increments depth for a logical hop
                // (`self.depth + 1`); the background/parallel wrappers inherit
                // their depth verbatim. The carried ceiling is this tool's
                // effective ceiling tightened (min) by the target's own
                // profile cap, so a parent's cap binds the whole subtree
                // (source of truth: `effective_max_depth`).
                let sub_delegate_tool = (target_may_subdelegate
                    && self.security.is_tool_allowed(Self::NAME)
                    && Self::delegate_admits_with_mcp(&tool_policy, Self::NAME))
                .then(|| {
                    let nested_task_control_plane = Arc::clone(&self.task_control_plane);
                    Box::new(DelegateTool {
                        agents: Arc::clone(&self.agents),
                        security: Arc::clone(&target_policy),
                        global_credential: self.global_credential.clone(),
                        provider_runtime_options: self.provider_runtime_options.clone(),
                        depth: self.depth + 1,
                        max_delegation_depth: Some(
                            self.tightened_max_depth(&agent_config.runtime_profile),
                        ),
                        // Delegate-only: background records live in a
                        // workspace-wide namespace without owner identity,
                        // and this tool shares the delegating parent's
                        // workspace, so the management surface would expose
                        // foreign identities' tasks.
                        background_task_management: false,
                        // The bounded child loop has no operator approval
                        // route, so this tool enforces the target profile's
                        // own delegation-approval decision itself.
                        operator_approval_available: false,
                        parent_tools: Arc::new(RwLock::new(bounded_base_tools.clone())),
                        runtime: self.runtime.clone(),
                        multimodal_config: self.multimodal_config.clone(),
                        delegate_config: self.delegate_config.clone(),
                        workspace_dir: self.workspace_dir.clone(),
                        cancellation_token: self.cancellation_token.child_token(),
                        memory: self.memory.clone(),
                        providers_models: Arc::clone(&self.providers_models),
                        risk_profiles: Arc::clone(&self.risk_profiles),
                        runtime_profiles: Arc::clone(&self.runtime_profiles),
                        skill_bundles: Arc::clone(&self.skill_bundles),
                        root_config: self.root_config.clone(),
                        live_config: self.live_config.clone(),
                        caller_alias: agent_name.to_string(),
                        task_control_plane: nested_task_control_plane,
                    }) as Box<dyn Tool>
                });

                let mut filtered: Vec<Box<dyn Tool>> = bounded_base_tools
                    .iter()
                    .map(|tool| {
                        target_memory_tools.remove(tool.name()).unwrap_or_else(|| {
                            Box::new(ToolArcRef::new(Arc::clone(tool))) as Box<dyn Tool>
                        })
                    })
                    .collect();
                // Appended after the inherited set; the target-bound instance
                // takes the retained delegation slot the parent's instance used
                // to fill implicitly.
                filtered.extend(sub_delegate_tool);
                // Seal the already-filtered set through the one assembly seam.
                // The policy is `SecurityPolicy::default()` (no allow/deny
                // lists), so `assemble`'s built-in filter is a provable identity
                // over `filtered`: it drops nothing the delegate filter kept.
                // Re-applying `self.security` here would double-filter and could
                // REGRESS delegate scoping, so it is deliberately NOT reused. No
                // peripherals / MCP / skills / memory-strip. A default config is
                // load-bearing here: the caller's config could synthesize pipeline
                // tools and violate the bounded parent-registry ceiling.
                let bounded_default_config = Config::default();
                let bounded_security = Arc::new(SecurityPolicy::default());
                let assembled_bounded = crate::tools::scoped::ScopedToolRegistry::assemble(
                    crate::tools::scoped::ScopedAssembly {
                        config: &bounded_default_config,
                        agent_alias: agent_name,
                        security: &bounded_security,
                        built: crate::tools::AllToolsResult::from_prebuilt_tools(filtered),
                        // Empty is load-bearing: bounded children inherit no target skill
                        // tools, and a non-empty list would make this default policy active.
                        skills: &[],
                        runtime: Arc::new(crate::platform::NativeRuntime::new()),
                        caller_allowed: None,
                        connect_mcp: false,
                        connect_peripherals: false,
                        exclude_memory: false,
                        acp_delivery: false,
                        list_deferred_mcp_specs: false,
                        emit_assembly_logs: false,
                        mcp_registry: None,
                    },
                )
                .await;
                assembled_bounded.registry
            }
        };

        let loop_runtime = self.resolve_loop_runtime(agent_name, agent_config);
        let native_tools = model_provider
            .capabilities_for_model(model)
            .native_tool_calling;

        // Independent delegates execute as target-owned turns, so their thinking policy
        // must override the parent task-local scope for the child loop. Bounded delegates
        // deliberately retain the caller's turn context.
        let thinking_params = (target_mode == DelegateExecutionMode::Independent).then(|| {
            crate::agent::thinking::apply_thinking_level_with_config(
                loop_runtime.thinking.default_level,
                &loop_runtime.thinking,
            )
        });
        let effective_temperature = thinking_params.as_ref().map_or(temperature, |params| {
            temperature.map(|value| {
                crate::agent::thinking::clamp_temperature(value + params.temperature_adjustment)
            })
        });

        // Build enriched system prompt with tools, skills, workspace, datetime context.
        // Independent delegation builds it from the TARGET's workspace (`sub_workspace`), so
        // the skill prompt content matches the target's skill tools; bounded delegation
        // keeps the caller's `self.workspace_dir`.
        let prompt_workspace = sub_workspace.as_deref().unwrap_or(&self.workspace_dir);
        let enriched_system_prompt = self.build_enriched_system_prompt(
            agent_name,
            agent_config,
            model,
            &sub_tools,
            prompt_workspace,
            native_tools,
            sub_skills.as_deref(),
            // Independent targets run under their own manager: state that
            // exact policy in the prompt. Bounded delegates have none here.
            approval_manager.as_ref(),
        );
        // Independent delegates surface the target's deferred MCP tools the way a fresh
        // target turn does. See `compose_independent_system_prompt`: it applies the turn
        // engine's text-tool prompt policy to the deferred section (so a non-native strict
        // target suppresses it, exactly as a fresh turn would) and then appends it.
        let enriched_system_prompt = Self::compose_independent_system_prompt(
            enriched_system_prompt,
            sub_deferred_section,
            native_tools,
            loop_runtime.strict_tool_parsing,
        );
        let enriched_system_prompt = match (
            enriched_system_prompt,
            thinking_params
                .as_ref()
                .and_then(|params| params.system_prompt_prefix.as_deref()),
        ) {
            (Some(prompt), Some(prefix)) => Some(format!("{prefix}\n\n{prompt}")),
            (None, Some(prefix)) => Some(prefix.to_string()),
            (prompt, None) => prompt,
        };

        let mut history = Vec::new();
        // Delegate subagents start a fresh transcript: no prior trim, so no
        // crumb exists and none outlives this scoped loop.
        let mut subagent_crumb_present = false;
        let mut subagent_injected_memory_preamble: Option<String> = None;
        if let Some(system_prompt) = enriched_system_prompt.as_ref() {
            history.push(ChatMessage::system(system_prompt.clone()));
        }
        history.push(ChatMessage::user(full_prompt.to_string()));

        let noop_observer = NoopObserver;

        let agentic_timeout_secs = self
            .resolve_agentic_timeout_secs(&agent_config.runtime_profile)
            .unwrap_or(self.delegate_config.agentic_timeout_secs);
        let receipt_scope = crate::agent::tool_receipts::TOOL_LOOP_RECEIPT_CONTEXT
            .try_with(Clone::clone)
            .ok()
            .flatten();
        let receipt_generator = receipt_scope.as_ref().map(|s| &s.generator);
        let collected_receipts = receipt_scope.as_ref().map(|s| s.collector.as_ref());
        let turn_id = uuid::Uuid::new_v4().to_string();
        let pacing = clawcrew_config::schema::PacingConfig::default();
        let loop_knobs = LoopKnobs::default();
        let execution = tokio::time::timeout(
            Duration::from_secs(agentic_timeout_secs),
            run_tool_call_loop(ToolLoop {
                served_route_sink: None,
                sop_reassembly: None,
                exec: ResolvedAgentExecution::resolve(
                    ResolvedModelAccess {
                        model_provider,
                        provider_name: agent_config.model_provider.as_str(),
                        model,
                        dispatch_model: model,
                        temperature: effective_temperature,
                    },
                    ResolvedIo {
    app_registry: None,
                        tools_registry: &sub_tools,
                        observer: &noop_observer,
                        silent: true,
                        approval: approval_manager.as_ref(),
                        multimodal_config: &self.multimodal_config,
                        // Full config so the delegated sub-agent's vision route
                        // resolves the configured `vision_model_provider`'s alias
                        // options (the `vision` override, endpoint URI, credentials),
                        // exactly as the parent turn does. `None` only on the
                        // configless test builder (`root_config` unset).
                        config: self.root_config.as_deref(),
                        hooks: None,
                        // Thread the target's deferred-MCP activated set so `tool_search`
                        // can activate the target's deferred tools mid-turn (Some only for
                        // an independent target with granted deferred-MCP bundles).
                        activated_tools: sub_activated.as_ref(),
                        model_switch_callback: None,
                        receipt_generator,
                    },
                    ResolvedRuntimeKnobs {
                        max_tool_iterations: loop_runtime.max_tool_iterations,
                        excluded_tools: &[],
                        dedup_exempt_tools: tool_policy.excluded_tools.as_deref().unwrap_or(&[]),
                        pacing: &pacing,
                        strict_tool_parsing: loop_runtime.strict_tool_parsing,
                        parallel_tools: loop_runtime.parallel_tools,
                        max_tool_result_chars: loop_runtime.max_tool_result_chars,
                        // Resolve from the target's provider alias and model, not the
                        // delegating agent's route.
                        context_limits: self.root_config.as_deref().map_or_else(
                            || loop_runtime.context_limits(),
                            |config| {
                                config.resolved_context_limits_for_route(
                                    agent_name,
                                    &agent_config.model_provider,
                                    model,
                                )
                            },
                        ),
                        context_limits_resolver: None,
                        knobs: &loop_knobs,
                    },
                ),
                history: &mut history,
                // Delegate subagents start a fresh transcript: no prior trim,
                // so no crumb exists and none outlives this scoped loop.
                history_has_trim_breadcrumb: &mut subagent_crumb_present,
                injected_memory_preamble: &mut subagent_injected_memory_preamble,
                channel_name: "delegate",
                channel_reply_target: None,
                cancellation_token: Some(self.cancellation_token.child_token()),
                on_delta: None,
                shared_budget: None,
                // TODO thread from parent in future
                channel: None,
                collected_receipts,
                event_tx: None,
                steering: None,
                new_messages_out: None,
                image_cache: None,
                // Phase 1: stamp Internal/Trusted. Per-transport
                // stamping lands in a later phase.
                memory: None,
                ingress: clawcrew_api::ingress::IngressContext::sub_turn(),
                agent_alias: Some(agent_name),
                parent_agent_alias: None,
                turn_id: &turn_id,
            })
            .instrument(::clawcrew_log::attribution_span!(
                &crate::agent::AgentAttribution(agent_name)
            )),
        );
        let result = match thinking_params {
            Some(params) => {
                clawcrew_api::NATIVE_THINKING_OVERRIDE
                    .scope(params.native_thinking, execution)
                    .await
            }
            None => execution.await,
        };

        match result {
            Ok(Ok(response)) if response.trim().is_empty() => Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(invalid_semantic_completion_error(agent_name)),
            }),
            Ok(Ok(response)) => Ok(ToolResult {
                success: true,
                output: format!(
                    "[Agent '{agent_name}' ({provider_type}/{model}, agentic)]\n{response}",
                )
                .into(),
                error: None,
            }),
            Ok(Err(e)) => Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(delegate_failure_error(agent_name, &e)),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: ToolOutput::default(),
                error: Some(format!(
                    "Agent '{agent_name}' timed out after {agentic_timeout_secs}s"
                )),
            }),
        }
    }
}

struct ToolArcRef {
    inner: Arc<dyn Tool>,
}

impl ToolArcRef {
    fn new(inner: Arc<dyn Tool>) -> Self {
        Self { inner }
    }
}

impl ::clawcrew_api::attribution::Attributable for ToolArcRef {
    fn role(&self) -> ::clawcrew_api::attribution::Role {
        self.inner.role()
    }
    fn alias(&self) -> &str {
        self.inner.alias()
    }
    fn tool_provenance(&self) -> ::clawcrew_api::attribution::ToolProvenance {
        self.inner.tool_provenance()
    }
}

#[async_trait]
impl Tool for ToolArcRef {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.inner.parameters_schema()
    }

    fn output_schema(&self) -> Option<serde_json::Value> {
        self.inner.output_schema()
    }

    fn param_domains(&self) -> Vec<(&'static str, ::clawcrew_api::tool::OptionDomain)> {
        self.inner.param_domains()
    }

    // Forward `spec()` so inner overrides keep their `Arc`-shared parameter
    // schemas; the trait default would rebuild the spec from
    // `parameters_schema()`, deep-cloning MCP schemas every loop iteration.
    fn spec(&self) -> clawcrew_api::tool::ToolSpec {
        self.inner.spec()
    }

    fn invocation_triggers(&self) -> Vec<String> {
        self.inner.invocation_triggers()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.inner.execute(args).await
    }
}

struct NoopObserver;

impl Observer for NoopObserver {
    fn record_event(&self, _event: &ObserverEvent) {}

    fn record_metric(&self, _metric: &ObserverMetric) {}

    fn name(&self) -> &str {
        "noop"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests;

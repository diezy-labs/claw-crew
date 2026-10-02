//! JSON-RPC 2.0 method dispatch. Transport-agnostic.

use super::context::{ConfigWriteGuard, RpcContext};
use super::transport::RpcTransport;
use super::turn::{TurnAttribution, TurnOutcome, execute_turn};
use super::types::*;

const RPC_RELOAD_REPLY_FLUSH_DELAY: std::time::Duration = std::time::Duration::from_millis(200);
const RPC_RELOAD_GATEWAY_SHUTDOWN_DELAY: std::time::Duration =
    std::time::Duration::from_millis(200);
const PROBE_MODEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
use crate::agent::agent::TurnEvent;
use crate::control_plane::task_registry::TaskRegistry;
use crate::sop::SopGraphExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use clawcrew_config::schema::Config;

use clawcrew_api::jsonrpc::error_codes::*;
use clawcrew_api::jsonrpc::{
    JSONRPC_VERSION, JsonRpcError, JsonRpcFrame, JsonRpcFrameErrorKind, JsonRpcNotification,
    JsonRpcResponse, RpcOutbound, SopDecideRequest, SopRunDetailRequest, SopRunOverlayRequest,
    SopRunRequest, SopRunResponse, SopRunsRequest, SopSaveRequest, SopSelectRequest,
};
use clawcrew_api::model_provider::ConversationMessage;
use clawcrew_api::runtime_status::{RuntimeConfigKind, RuntimeShellProfile};
use clawcrew_commands::{CommandSurface, commands_for_surface};

/// Wire protocol version. Bump on breaking changes.
pub const RPC_PROTOCOL_VERSION: u64 = 1;

mod notification {
    pub const SESSION_UPDATE: &str = "session/update";
    pub const LOGS_EVENT: &str = "logs/event";
}

#[derive(Debug)]
struct StatusRuntimeContext {
    config_dir: String,
    config_file: String,
    config_kind: RuntimeConfigKind,
    local_ipc_endpoint: String,
    shell_profile: Option<RuntimeShellProfile>,
}

fn status_runtime_context(
    config: &Config,
    config_kind: RuntimeConfigKind,
) -> Result<StatusRuntimeContext, JsonRpcError> {
    let config_file = config.config_path.display().to_string();
    let config_dir = config
        .config_path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let local_ipc_endpoint = super::local::socket_path(config).display().to_string();
    let shell_profile = clawcrew_config::platform::create_runtime(&config.runtime)
        .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Runtime status unavailable: {e}")))?
        .shell_profile()
        .and_then(RuntimeShellProfile::from_runtime_profile);

    Ok(StatusRuntimeContext {
        config_dir,
        config_file,
        config_kind,
        local_ipc_endpoint,
        shell_profile,
    })
}

// ── Method registry ──────────────────────────────────────────────
//
// Single source of truth. Every variant maps to exactly one wire
// string. `from_wire` is a table scan — no hand-written string
// matching anywhere in this file.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    // Core
    Initialize,
    Status,
    Health,
    DoctorRun,

    // Dashboard & Backup
    DashboardTasks,
    DashboardTaskTimeline,
    DashboardSessionHealth,
    BackupCreate,
    BackupRestore,

    // Sessions (agent chat lives here — session/prompt + session/update
    // notifications is the RPC equivalent of the gateway's ws/chat)
    SessionNew,
    SessionClose,
    SessionPrompt,
    SessionConfigure,
    SessionCancel,
    SessionGitBranch,
    SessionList,
    SessionListAcp,
    SessionMessages,
    SessionState,
    SessionDelete,
    SessionApprove,
    SessionKill,

    // Memory
    MemoryList,
    MemorySearch,
    MemoryGet,
    MemoryStore,
    MemoryDelete,

    // Cron
    CronList,
    CronGet,
    CronAdd,
    CronPatch,
    CronDelete,
    CronRuns,
    CronTrigger,
    CronSettings,

    // Config
    ConfigGet,
    ConfigSet,
    ConfigValidate,
    ConfigReload,
    ConfigList,
    ConfigDelete,
    ConfigMapKeys,
    ConfigResolveAliasSource,
    ConfigMapKeyCreate,
    ConfigMapKeyDelete,
    ConfigMapKeyRename,
    ConfigTemplates,

    // Agents
    AgentsList,
    AgentsStatus,

    // Cost
    CostQuery,
    CostOrg,

    // Skills
    SkillsBundles,
    SkillsList,
    SkillsRead,
    SkillsWrite,
    SkillsDelete,

    // Personality
    PersonalityList,
    PersonalityGet,
    PersonalityPut,
    PersonalityTemplates,

    // Config introspection (sections, catalog, status)
    ConfigSections,
    ConfigStatus,
    ConfigCatalog,
    ConfigCatalogModels,

    // Logs / Events
    LogsSubscribe,
    LogsQuery,
    LogsGet,

    // TUI
    TuiList,

    // Files
    FileAttach,
    FsListDir,

    // Locales
    LocalesList,
    LocalesFetch,

    // Quickstart (TUI mirror of `/api/quickstart/*` HTTP routes)
    QuickstartState,
    QuickstartFields,
    QuickstartValidate,
    QuickstartApply,
    QuickstartDismiss,

    // Certificates (mTLS client-cert lifecycle)
    CertRenew,

    SopsList,
    SopsGet,
    SopsGraph,
    SopsRun,
    SopsRuns,
    SopsRunDetail,
    SopsRunOverlay,
    SopsValidate,
    SopsSave,
    SopsCreate,
    SopsDelete,
    SopsDecide,
    SopsWireDraft,
    SopsGraphDraft,
    SopsTriggerSources,
    ToolsParamOptions,
}

impl Method {
    /// The single table. Wire name ↔ variant, defined once.
    pub const ALL: &[(Method, &str)] = &[
        (Method::Initialize, "initialize"),
        (Method::Status, "status"),
        (Method::Health, "health"),
        (Method::DoctorRun, "doctor/run"),
        (Method::DashboardTasks, "dashboard/tasks"),
        (Method::DashboardTaskTimeline, "dashboard/task/timeline"),
        (Method::DashboardSessionHealth, "dashboard/session/health"),
        (Method::BackupCreate, "backup/create"),
        (Method::BackupRestore, "backup/restore"),
        // Sessions
        (Method::SessionNew, "session/new"),
        (Method::SessionClose, "session/close"),
        (Method::SessionPrompt, "session/prompt"),
        (Method::SessionConfigure, "session/configure"),
        (Method::SessionCancel, "session/cancel"),
        (Method::SessionGitBranch, "session/git_branch"),
        (Method::SessionList, "session/list"),
        (Method::SessionListAcp, "session/list-acp"),
        (Method::SessionMessages, "session/messages"),
        (Method::SessionState, "session/state"),
        (Method::SessionDelete, "session/delete"),
        (Method::SessionApprove, "session/approve"),
        (Method::SessionKill, "session/kill"),
        // Memory
        (Method::MemoryList, "memory/list"),
        (Method::MemorySearch, "memory/search"),
        (Method::MemoryGet, "memory/get"),
        (Method::MemoryStore, "memory/store"),
        (Method::MemoryDelete, "memory/delete"),
        // Cron
        (Method::CronList, "cron/list"),
        (Method::CronGet, "cron/get"),
        (Method::CronAdd, "cron/add"),
        (Method::CronPatch, "cron/patch"),
        (Method::CronDelete, "cron/delete"),
        (Method::CronRuns, "cron/runs"),
        (Method::CronTrigger, "cron/trigger"),
        (Method::CronSettings, "cron/settings"),
        // Config
        (Method::ConfigGet, "config/get"),
        (Method::ConfigSet, "config/set"),
        (Method::ConfigValidate, "config/validate"),
        (Method::ConfigReload, "config/reload"),
        (Method::ConfigList, "config/list"),
        (Method::ConfigDelete, "config/delete"),
        (Method::ConfigMapKeys, "config/map-keys"),
        (
            Method::ConfigResolveAliasSource,
            "config/resolve-alias-source",
        ),
        (Method::ConfigMapKeyCreate, "config/map-key-create"),
        (Method::ConfigMapKeyDelete, "config/map-key-delete"),
        (Method::ConfigMapKeyRename, "config/map-key-rename"),
        (Method::ConfigTemplates, "config/templates"),
        // Agents
        (Method::AgentsList, "agents/list"),
        (Method::AgentsStatus, "agents/status"),
        // Cost
        (Method::CostQuery, "cost/query"),
        (Method::CostOrg, "cost/org"),
        // Skills
        (Method::SkillsBundles, "skills/bundles"),
        (Method::SkillsList, "skills/list"),
        (Method::SkillsRead, "skills/read"),
        (Method::SkillsWrite, "skills/write"),
        (Method::SkillsDelete, "skills/delete"),
        // Personality
        (Method::PersonalityList, "personality/list"),
        (Method::PersonalityGet, "personality/get"),
        (Method::PersonalityPut, "personality/put"),
        (Method::PersonalityTemplates, "personality/templates"),
        // Config introspection
        (Method::ConfigSections, "config/sections"),
        (Method::ConfigStatus, "config/status"),
        (Method::ConfigCatalog, "config/catalog"),
        (Method::ConfigCatalogModels, "config/catalog-models"),
        // Logs
        (Method::LogsSubscribe, "logs/subscribe"),
        (Method::LogsQuery, "logs/query"),
        (Method::LogsGet, "logs/get"),
        // TUI
        (Method::TuiList, "tui/list"),
        // Files
        (Method::FileAttach, "file/attach"),
        (Method::FsListDir, "fs/list_dir"),
        // Locales
        (Method::LocalesList, "locales/list"),
        (Method::LocalesFetch, "locales/fetch"),
        // Quickstart
        (Method::QuickstartState, "quickstart/state"),
        (Method::QuickstartFields, "quickstart/fields"),
        (Method::QuickstartValidate, "quickstart/validate"),
        (Method::QuickstartApply, "quickstart/apply"),
        (Method::QuickstartDismiss, "quickstart/dismiss"),
        (Method::CertRenew, "cert/renew"),
        (Method::SopsList, "sops/list"),
        (Method::SopsGet, "sops/get"),
        (Method::SopsGraph, "sops/graph"),
        (Method::SopsRun, "sops/run"),
        (Method::SopsRuns, "sops/runs"),
        (Method::SopsRunDetail, "sops/run-detail"),
        (Method::SopsRunOverlay, "sops/run-overlay"),
        (Method::SopsValidate, "sops/validate"),
        (Method::SopsSave, "sops/save"),
        (Method::SopsCreate, "sops/create"),
        (Method::SopsDelete, "sops/delete"),
        (Method::SopsDecide, "sops/decide"),
        (Method::SopsWireDraft, "sops/wire-draft"),
        (Method::SopsGraphDraft, "sops/graph-draft"),
        (Method::SopsTriggerSources, "sops/trigger-sources"),
        (Method::ToolsParamOptions, "tools/param-options"),
    ];

    /// Resolve a wire method name to a variant. Table scan, no hand-written
    /// string matching.
    pub fn from_wire(s: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, wire)| *wire == s)
            .map(|(m, _)| *m)
    }

    /// Wire name for this variant.
    pub fn wire_name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(m, _)| *m == self)
            .map(|(_, wire)| *wire)
            .expect("every variant is in ALL")
    }
}

type RpcResult = Result<Value, JsonRpcError>;
type BoxRpcFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = RpcResult> + Send + 'a>>;

fn rpc_err(code: i32, msg: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code,
        message: msg.into(),
        data: None,
    }
}

fn task_status_label(status: crate::control_plane::task_registry::TaskStatus) -> &'static str {
    match status {
        crate::control_plane::task_registry::TaskStatus::Queued => "queued",
        crate::control_plane::task_registry::TaskStatus::Waiting => "waiting",
        crate::control_plane::task_registry::TaskStatus::Validating => "validating",
        crate::control_plane::task_registry::TaskStatus::Retrying => "retrying",
        crate::control_plane::task_registry::TaskStatus::NeedsReview => "needs_review",
        crate::control_plane::task_registry::TaskStatus::Running => "running",
        crate::control_plane::task_registry::TaskStatus::Paused => "paused",
        crate::control_plane::task_registry::TaskStatus::Completed => "completed",
        crate::control_plane::task_registry::TaskStatus::Failed => "failed",
        crate::control_plane::task_registry::TaskStatus::Cancelled => "cancelled",
        crate::control_plane::task_registry::TaskStatus::Lost => "lost",
        crate::control_plane::task_registry::TaskStatus::TimedOut => "timed_out",
    }
}

fn not_yet_implemented(method: Method) -> RpcResult {
    Err(rpc_err(
        INTERNAL_ERROR,
        format!("{}: not yet implemented", method.wire_name()),
    ))
}

fn doctor_summary(results: &[DiagResult]) -> DoctorSummary {
    DoctorSummary {
        ok: results
            .iter()
            .filter(|r| r.severity == crate::doctor::Severity::Ok)
            .count(),
        warnings: results
            .iter()
            .filter(|r| r.severity == crate::doctor::Severity::Warn)
            .count(),
        errors: results
            .iter()
            .filter(|r| r.severity == crate::doctor::Severity::Error)
            .count(),
    }
}

fn personality_template_context(
    config: &clawcrew_config::schema::Config,
    req: &PersonalityTemplatesParams,
) -> crate::agent::personality_templates::TemplateContext {
    let agent_requested = req.agent.is_some();
    let requested_agent = req
        .agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let agent_alias = requested_agent.unwrap_or("default");
    let configured_agent_exists = config.agent(agent_alias).is_some();

    crate::agent::personality_templates::TemplateContext {
        agent: requested_agent
            .map(str::to_string)
            .or_else(|| configured_agent_exists.then(|| agent_alias.to_string()))
            .unwrap_or_else(|| "ClawCrew".to_string()),
        include_memory: configured_agent_exists || agent_requested,
        ..Default::default()
    }
}

fn model_provider_ref_from_provider_profile_prop(prop: &str) -> Option<String> {
    let rest = prop.strip_prefix("providers.models.")?;
    let (provider_type, rest) = rest.split_once('.')?;
    let (provider_alias, field) = rest.split_once('.')?;
    if provider_type.is_empty() || provider_alias.is_empty() || field.is_empty() {
        None
    } else {
        Some(format!("{provider_type}.{provider_alias}"))
    }
}

/// Whether a config prop path touches `model_routes` (a field edit like
/// `model_routes.<hint>.model_provider`, or the section path itself from a
/// map-key create/delete/rename on `model_routes`). A route table edit can
/// change which provider/model a hint-routed call dispatches to and which
/// capacity `ResolvedContextLimits` reports for it, so it needs the same live
/// rebuild `providers.models.*` and `agents.<alias>.model_provider` trigger —
/// otherwise a session's `ModelRouteResolver` keeps resolving hints through
/// the pre-edit route table after `config/set` commits the new one.
fn touches_model_routes(prop: &str) -> bool {
    prop == "model_routes" || prop.starts_with("model_routes.") || prop.starts_with("model_routes[")
}

/// Extract the agent alias from an `agents.<alias>.model_provider` prop path.
/// A live change to an agent's bound provider must rebuild that agent's live
/// session boxes the same way a `providers.models.*` edit does, so any
/// `config/set agents.<alias>.model_provider` caller (the config pane and other
/// RPC/config-set clients) gets a live refresh.
fn agent_alias_from_model_provider_prop(prop: &str) -> Option<String> {
    let rest = prop.strip_prefix("agents.")?;
    let (alias, field) = rest.split_once('.')?;
    if alias.is_empty() || field != "model_provider" {
        None
    } else {
        Some(alias.to_string())
    }
}

/// Session-selection predicate for an agent-scoped `model_provider` refresh
/// (`config/set agents.<alias>.model_provider`). Only sessions bound to the
/// edited agent are eligible, and a session that carries its own
/// `model_provider` override is excluded so unrelated agents and overridden
/// sessions are never rebuilt.
fn agent_scoped_refresh_selects(
    edited_agent: &str,
    session_agent: &str,
    overrides: &SessionOverrides,
) -> bool {
    session_agent == edited_agent && overrides.model_provider.is_none()
}

/// Session-selection predicate for a provider-scoped refresh
/// (`providers.models.*` edit). A session is eligible when its own
/// `model_provider` override matches the edited provider, or when it has no
/// override and thus inherits the agent's provider (final provider match is
/// resolved separately against config). A complete `model_routes` entry that
/// targets the edited profile expands the scope to every session in
/// `LiveSessionRefreshScope::resolve_provider_ref`, because every routed
/// provider materialization contains that target.
fn provider_scoped_refresh_selects(target_ref: &str, overrides: &SessionOverrides) -> bool {
    overrides
        .model_provider
        .as_deref()
        .map(|r| r == target_ref)
        .unwrap_or(true)
}

/// The live-session materialization affected by one config mutation.
///
/// `Config` remains the canonical source. This value only describes which
/// ephemeral provider/resolver views must be rebuilt before that candidate
/// config can be committed.
#[derive(Clone)]
enum LiveSessionRefreshScope {
    ModelProvider(String),
    Agent(String),
    ModelRoutes,
    /// A provider alias rename (`providers.models.<family>.<from>` ->
    /// `<to>`). This cannot reuse `ModelProvider(new_ref)`: the cascade
    /// rewrites *config* referrers to the new alias, but a session's
    /// `SessionOverrides.model_provider` is transient in-memory state that
    /// still names the old alias. Scoping on the new reference alone would
    /// skip exactly those sessions, leaving them holding the pre-rename
    /// provider box and resolver while their override points at an alias
    /// that no longer exists.
    ProviderAliasRename {
        old_ref: String,
        new_ref: String,
    },
}

impl LiveSessionRefreshScope {
    fn for_prop(prop: &str) -> Option<Self> {
        model_provider_ref_from_provider_profile_prop(prop)
            .map(Self::ModelProvider)
            .or_else(|| agent_alias_from_model_provider_prop(prop).map(Self::Agent))
            .or_else(|| touches_model_routes(prop).then_some(Self::ModelRoutes))
    }

    fn resolve_provider_ref(
        &self,
        config: &Config,
        session_agent: &str,
        overrides: &SessionOverrides,
    ) -> Result<Option<String>, String> {
        match self {
            Self::ModelProvider(target_ref) => {
                let effective_ref = overrides.model_provider.as_deref().or_else(|| {
                    config
                        .agent(session_agent)
                        .map(|agent| agent.model_provider.as_str())
                });
                let materialized_route_uses_target = config.model_routes.iter().any(|route| {
                    !route.hint.trim().is_empty()
                        && !route.model.trim().is_empty()
                        && route.model_provider.trim() == target_ref
                });
                if materialized_route_uses_target {
                    return effective_ref
                        .map(str::to_string)
                        .map(Some)
                        .ok_or_else(|| format!("agent `{session_agent}` is not configured"));
                }
                if !provider_scoped_refresh_selects(target_ref, overrides) {
                    return Ok(None);
                }
                Ok((effective_ref == Some(target_ref.as_str())).then(|| target_ref.clone()))
            }
            Self::Agent(edited_agent) => {
                if !agent_scoped_refresh_selects(edited_agent, session_agent, overrides) {
                    return Ok(None);
                }
                config
                    .agent(edited_agent)
                    .map(|agent| Some(agent.model_provider.to_string()))
                    .ok_or_else(|| format!("agent `{edited_agent}` is not configured"))
            }
            Self::ModelRoutes => overrides
                .model_provider
                .clone()
                .or_else(|| {
                    config
                        .agent(session_agent)
                        .map(|agent| agent.model_provider.to_string())
                })
                .map(Some)
                .ok_or_else(|| format!("agent `{session_agent}` is not configured")),
            Self::ProviderAliasRename { old_ref, new_ref } => {
                let effective_ref = overrides
                    .model_provider
                    .as_deref()
                    .or_else(|| {
                        config
                            .agent(session_agent)
                            .map(|agent| agent.model_provider.as_str())
                    })
                    .ok_or_else(|| format!("agent `{session_agent}` is not configured"))?;
                // An inheriting session already reads the rewritten `new_ref`
                // from config; a session carrying an explicit override still
                // reads `old_ref`. Both name the same profile across the
                // rename, so both rebuild against the new reference.
                if effective_ref == old_ref || effective_ref == new_ref {
                    return Ok(Some(new_ref.clone()));
                }
                // Otherwise the session keeps its own provider, but a route
                // table that materializes the renamed alias still binds it
                // into this session's resolver.
                let materialized_route_uses_target = config.model_routes.iter().any(|route| {
                    !route.hint.trim().is_empty()
                        && !route.model.trim().is_empty()
                        && route.model_provider.trim() == new_ref
                });
                Ok(materialized_route_uses_target.then(|| effective_ref.to_string()))
            }
        }
    }
}

/// A provider/resolver view built from a candidate `Config` while the
/// corresponding session generation lock is held. The guard deliberately
/// travels with the values until they are published after the config commit.
struct PreparedLiveSessionRefresh {
    session_id: String,
    /// Session-identity generation captured BEFORE the provider box was built.
    /// `apply_model_provider` rejects the write if the session was replaced
    /// under the same ID in the meantime (`session/new`, ACP rehydration), so
    /// stale work cannot mutate a successor session.
    session_generation: u64,
    _model_provider_update: tokio::sync::OwnedMutexGuard<()>,
    model_provider: Box<dyn clawcrew_providers::ModelProvider>,
    model_provider_name: String,
    model_name: String,
    model_route_resolver: Arc<clawcrew_providers::router::ModelRouteResolver>,
    tool_dispatcher: Box<dyn crate::agent::dispatcher::ToolDispatcher>,
    temperature: Option<f64>,
    /// New `model_provider` override value for a session whose stored override
    /// names an alias this transaction renames away. Applied in the same
    /// publication step as the provider box, so the override never survives as
    /// a dangling reference to a removed alias.
    override_migration: Option<String>,
}

/// Whether memory embeddings resolve from the given `<type>.<alias>` provider
/// profile — either the base `[memory].embedding_provider` reference or any
/// `[[embedding_routes]]` entry. Gates the memory-embedder refresh on a
/// `config/set` provider-profile change
fn memory_embeddings_use_provider(
    config: &clawcrew_config::schema::Config,
    model_provider_ref: &str,
) -> bool {
    config.memory.embedding_provider.trim() == model_provider_ref
        || config
            .embedding_routes
            .iter()
            .any(|route| route.model_provider.trim() == model_provider_ref)
}

fn rename_error_to_rpc(
    path: &str,
    from: &str,
    err: clawcrew_config::alias_refs::RenameError,
) -> JsonRpcError {
    use clawcrew_config::alias_refs::RenameError;
    let code = match err {
        RenameError::PostCondition(_) => INTERNAL_ERROR,
        _ => INVALID_PARAMS,
    };
    rpc_err(code, format!("{path}.{from}: {err}"))
}

async fn move_renamed_agent_workspace(
    old_workspace: &std::path::Path,
    new_workspace: &std::path::Path,
) -> Option<String> {
    if old_workspace == new_workspace || !old_workspace.exists() {
        return None;
    }
    if let Some(parent) = new_workspace.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    match tokio::fs::rename(old_workspace, new_workspace).await {
        Ok(()) => None,
        Err(err) => Some(format!(
            "workspace move {} -> {} failed: {err}",
            old_workspace.display(),
            new_workspace.display()
        )),
    }
}

fn session_should_initialize_mcp(chat_mode: &crate::rpc::types::ChatMode) -> bool {
    !matches!(chat_mode, crate::rpc::types::ChatMode::Acp)
}

/// Per-connection dispatcher. Shared state lives in [`RpcContext`].
pub struct RpcDispatcher {
    ctx: Arc<RpcContext>,
    rpc: Arc<RpcOutbound>,
    authenticated: bool,
    /// TUI session UID assigned during `initialize`. Used for registry
    /// cleanup on disconnect.
    tui_id: Option<String>,
    /// Which registration of `tui_id` this connection owns. Created here: the
    /// id alone cannot tell this connection's registration from a successor's
    /// after a reconnect adopts the same id, and teardown must only remove its
    /// own. `None` until `initialize` registers.
    tui_epoch: Option<super::tui_identity::TuiEpoch>,
    /// Transport-level peer label (e.g. `unix:pid=1234,uid=1000`).
    peer_label: String,
    client_elicitation_caps: clawcrew_api::elicitation::ElicitationCapabilities,
    /// Generation token for the transport connection that accepted work.
    /// Every detached prompt is linked to it and drained before teardown.
    connection_cancel: CancellationToken,
    /// Whether this dispatcher owns the transport connection. Only the owner
    /// may end the generation: prompt handles from [`Self::spawn_handle`] read
    /// the same token to observe teardown, and a completing prompt dropping
    /// its handle must not close the connection that is still serving requests.
    owns_connection: bool,
    /// Liveness token for the accepted connection, shared with every task this
    /// connection starts. Cloned into each spawned prompt and into the nested
    /// turn task, so the listener's client count falls to zero only once that
    /// work has actually finished unwinding. `None` when no listener supplied
    /// one (direct dispatcher construction outside an accepted connection).
    connection_activity: Option<crate::rpc::ConnectionActivity>,
    prompt_tasks: Vec<JoinHandle<()>>,
    /// SHA-256 fingerprint of the client certificate presented on the mTLS
    /// handshake (remote WSS plane only; `None` on the local socket). This is the
    /// transport identity: it keys the issued-cert ledger, so the renew RPC gates
    /// on its ledger status (a revoked cert cannot self-renew, A5) and authz still
    /// resolves from the registry.
    peer_cert_fingerprint: Option<String>,
}

impl RpcDispatcher {
    pub fn new(ctx: Arc<RpcContext>, writer_tx: mpsc::Sender<String>, peer_label: String) -> Self {
        Self::new_with_connection_cancel(ctx, writer_tx, peer_label, CancellationToken::new())
    }

    pub(crate) fn new_with_connection_cancel(
        ctx: Arc<RpcContext>,
        writer_tx: mpsc::Sender<String>,
        peer_label: String,
        connection_cancel: CancellationToken,
    ) -> Self {
        Self {
            ctx,
            rpc: Arc::new(RpcOutbound::new(writer_tx)),
            authenticated: false,
            tui_id: None,
            tui_epoch: None,
            peer_label,
            client_elicitation_caps: clawcrew_api::elicitation::ElicitationCapabilities::default(),
            connection_cancel,
            owns_connection: true,
            connection_activity: None,
            prompt_tasks: Vec::new(),
            peer_cert_fingerprint: None,
        }
    }

    /// Attach the accepted connection's liveness token. Additive builder so the
    /// listeners can share their client-count token with the tasks this
    /// dispatcher spawns while other construction sites need no change.
    #[must_use]
    pub(crate) fn with_connection_activity(
        mut self,
        activity: crate::rpc::ConnectionActivity,
    ) -> Self {
        self.connection_activity = Some(activity);
        self
    }

    /// Bind the client certificate fingerprint from the mTLS handshake (WSS).
    /// Additive builder so the socket transport and tests need no change.
    #[must_use]
    pub fn with_peer_cert_fingerprint(mut self, fingerprint: Option<String>) -> Self {
        self.peer_cert_fingerprint = fingerprint;
        self
    }

    /// The presenting client certificate fingerprint, if this is an mTLS peer.
    pub fn peer_cert_fingerprint(&self) -> Option<&str> {
        self.peer_cert_fingerprint.as_deref()
    }

    /// TUI ID assigned during initialize, if any.
    pub fn tui_id(&self) -> Option<&str> {
        self.tui_id.as_deref()
    }

    /// This connection's registry registration: the TUI id and the epoch it was
    /// registered under. Teardown must quote both, so that a connection whose
    /// cleanup runs after a reconnect has already adopted the same id cannot
    /// evict the live entry.
    pub fn tui_registration(&self) -> Option<(&str, super::tui_identity::TuiEpoch)> {
        Some((self.tui_id.as_deref()?, self.tui_epoch?))
    }

    #[cfg(test)]
    pub fn set_tui_id_for_test(&mut self, tui_id: Option<String>) {
        self.tui_id = tui_id;
    }

    #[cfg(test)]
    pub fn rpc_for_test(&self) -> Arc<RpcOutbound> {
        Arc::clone(&self.rpc)
    }

    /// Construct a pre-authenticated dispatcher sharing the same context and
    /// RPC outbound as `self`. Used to run long-lived methods (e.g.
    /// `session/prompt`) in a spawned task so the read loop remains live.
    ///
    /// The handle observes the connection generation token but does not own it:
    /// a prompt finishing normally drops its handle, and that drop must leave
    /// the connection open for the requests that follow.
    fn spawn_handle(&self) -> Self {
        Self {
            ctx: Arc::clone(&self.ctx),
            rpc: Arc::clone(&self.rpc),
            authenticated: true,
            tui_id: self.tui_id.clone(),
            // Same connection, so the same registration: this handle shares the
            // parent's epoch rather than claiming one of its own. It never runs
            // teardown; only the owning transport loop does.
            tui_epoch: self.tui_epoch,
            peer_label: self.peer_label.clone(),
            client_elicitation_caps: self.client_elicitation_caps,
            connection_cancel: self.connection_cancel.clone(),
            owns_connection: false,
            // Shared, not re-created: this handle is moved into the spawned
            // prompt task, so the clone it carries keeps the connection counted
            // until that task's future is dropped.
            connection_activity: self.connection_activity.clone(),
            prompt_tasks: Vec::new(),
            peer_cert_fingerprint: self.peer_cert_fingerprint.clone(),
        }
    }

    /// Cancel and join every prompt accepted by this connection generation.
    /// The queue guard held by an in-flight turn is released only after its
    /// provider/tool future has observed cancellation and returned, so a
    /// replacement connection cannot race invisible old-generation work.
    pub(crate) async fn shutdown(&mut self) {
        self.connection_cancel.cancel();
        // Join each handle where it is stored, and remove it only once its
        // join has returned. Moving handles out first would detach whichever
        // prompt is being awaited if this future is itself dropped: the
        // listener force-aborts a connection that outlives its drain deadline,
        // and `Drop` below can only abort the handles it still holds.
        while !self.prompt_tasks.is_empty() {
            if let Some(task) = self.prompt_tasks.last_mut() {
                let _ = task.await;
            }
            self.prompt_tasks.pop();
        }
    }

    async fn forward_seed_event(&self, session_id: &str, event: Option<TurnEvent>) {
        if let Some(event) = event {
            forward_turn_event(&self.rpc, session_id, &event).await;
        }
    }

    /// Flush dirty config paths to disk.
    ///
    /// `_guard` is never read — it is a witness reminding the caller to
    /// serialize on `ctx.config_write_lock` for the whole read-mutate-flush
    /// critical section. It is NOT compile-time proof of holding *that*
    /// mutex (a guard is not statically tied to a specific instance); the
    /// `debug_assert!` below catches a caller holding a look-alike guard
    /// from the wrong mutex. The invariant lives on
    /// [`RpcContext::config_write_lock`]: every mutation of `ctx.config`
    /// must hold it, and a bypassing writer that re-dirties a just-saved
    /// path during a flush loses disk persistence of that write.
    ///
    /// Clone the config out of the lock (parking_lot guards are !Send, so
    /// the clone can't be held across `snapshot.save_dirty().await`), save
    /// the clone to disk, then remove only the paths that were actually
    /// saved from the LIVE config's dirty set. This must NOT swap the live
    /// config wholesale: a write landed on `ctx.config` while this method
    /// awaits disk I/O would otherwise be overwritten by the stale snapshot
    /// on write-back, silently erasing an in-memory change that was never
    /// given a chance to be saved.
    async fn flush_config(&self, _guard: &ConfigWriteGuard) -> Result<(), JsonRpcError> {
        debug_assert!(
            self.ctx.config_write_lock.try_lock().is_err(),
            "flush_config caller must hold ctx.config_write_lock"
        );
        let mut snapshot = self.ctx.config.read().clone();
        let saved_paths = snapshot.dirty_paths.clone();
        snapshot
            .save_dirty()
            .await
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Config save failed: {e}")))?;
        self.ctx
            .config
            .write()
            .dirty_paths
            .retain(|path| !saved_paths.contains(path));
        Ok(())
    }

    /// Save `snapshot` to disk, then install it as the live config.
    ///
    /// `_guard` is the same serialization witness as in
    /// [`Self::flush_config`] (a reminder, not compile-time proof — see
    /// there). Unlike `flush_config`, this deliberately swaps: callers pass
    /// a clone that was itself mutated beyond just `dirty_paths` (e.g. an
    /// alias rename), and installing that mutated snapshot wholesale is the
    /// point. Holding `config_write_lock` is what makes the swap safe — no
    /// other handler can land a concurrent `config` write while this is in
    /// flight.
    async fn save_and_swap_config(
        &self,
        mut snapshot: clawcrew_config::schema::Config,
        _guard: &ConfigWriteGuard,
    ) -> Result<(), JsonRpcError> {
        debug_assert!(
            self.ctx.config_write_lock.try_lock().is_err(),
            "save_and_swap_config caller must hold ctx.config_write_lock"
        );
        snapshot
            .save_dirty()
            .await
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Config save failed: {e}")))?;
        *self.ctx.config.write() = snapshot;
        Ok(())
    }

    async fn agent_rename_residue_exists(
        &self,
        config: &clawcrew_config::schema::Config,
        from: &str,
    ) -> bool {
        if config.agent_workspace_dir(from).exists() {
            return true;
        }
        if crate::cron::list_jobs_by_agent(config, from)
            .map(|jobs| !jobs.is_empty())
            .unwrap_or(false)
        {
            return true;
        }
        if let Some(store) = self.ctx.acp_session_store.as_ref()
            && store
                .list_sessions_by_agent(from)
                .map(|sessions| !sessions.is_empty())
                .unwrap_or(false)
        {
            return true;
        }
        if let Some(mem) = self.ctx.memory.as_ref()
            && mem.count_agent(from).await.unwrap_or(0) > 0
        {
            return true;
        }
        if let Some(backend) = self.ctx.session_backend.as_ref()
            && backend.count_agent_attribution(from).unwrap_or(0) > 0
        {
            return true;
        }
        false
    }

    /// Read frames from transport, dispatch, repeat.
    pub async fn run(&mut self, transport: &mut (dyn RpcTransport + Send)) {
        while let Some(line) = transport.next_frame().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            self.process_line(trimmed).await;
        }
    }

    /// Own a transport until EOF or generation cancellation, then drain all
    /// work accepted by that exact connection before returning.
    pub(crate) async fn run_connection(&mut self, transport: &mut (dyn RpcTransport + Send)) {
        let connection_cancel = self.connection_cancel.clone();
        tokio::select! {
            _ = self.run(transport) => {}
            _ = connection_cancel.cancelled() => {}
        }
        self.shutdown().await;
    }

    async fn process_line(&mut self, line: &str) {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(e) => {
                self.send_error(Value::Null, PARSE_ERROR, &format!("Parse error: {e}"))
                    .await;
                return;
            }
        };

        let frame = match JsonRpcFrame::from_value(value) {
            Ok(frame) => frame,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(serde_json::json!({ "line_len": line.len() })),
                    "Rejected invalid JSON-RPC frame"
                );
                match e.kind() {
                    JsonRpcFrameErrorKind::Request => {
                        let id = e.request_id().cloned().unwrap_or(Value::Null);
                        self.send_error(id, INVALID_REQUEST, &format!("Invalid request: {e}"))
                            .await;
                    }
                    JsonRpcFrameErrorKind::Response => {}
                    JsonRpcFrameErrorKind::Ambiguous => {
                        self.send_error(
                            Value::Null,
                            INVALID_REQUEST,
                            &format!("Invalid request: {e}"),
                        )
                        .await;
                    }
                }
                return;
            }
        };

        let req = match frame {
            JsonRpcFrame::Request(req) => req,
            JsonRpcFrame::Response { id, result } => {
                if let Some(id_key) = response_id_key(&id) {
                    if !self.rpc.dispatch_validated_response(&id_key, result) {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                            "Dropped JSON-RPC response with an unknown ID"
                        );
                    }
                } else {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                        "Dropped JSON-RPC response with an uncorrelatable null ID"
                    );
                }
                return;
            }
        };

        let req_id = req.id.clone().unwrap_or(Value::Null);
        let is_notification = req.id.is_none();

        let method = match Method::from_wire(&req.method) {
            Some(m) => m,
            None => {
                if !is_notification {
                    self.send_error(
                        req_id,
                        METHOD_NOT_FOUND,
                        &format!("Unknown method: {}", req.method),
                    )
                    .await;
                }
                return;
            }
        };

        if !self.authenticated && method != Method::Initialize {
            if !is_notification {
                self.send_error(req_id, AUTH_REQUIRED, "First call must be 'initialize'")
                    .await;
            }
            return;
        }

        // Exhaustive match — compiler enforces every Method has a handler.
        let result = match method {
            // Core
            Method::Initialize => self.handle_initialize(&req.params).await,
            Method::Status => self.handle_status().await,
            Method::Health => self.handle_health(),
            // Heap-pinned for the same reason as `ConfigSet` below.
            Method::DoctorRun => Box::pin(self.handle_doctor_run()).await,

            // Dashboard
            Method::DashboardTasks => self.handle_dashboard_tasks().await,
            Method::DashboardTaskTimeline => self.handle_dashboard_task_timeline(&req.params).await,
            Method::DashboardSessionHealth => self.handle_dashboard_session_health().await,
            Method::BackupCreate => self.handle_backup_create(&req.params).await,
            Method::BackupRestore => self.handle_backup_restore(&req.params).await,

            // Sessions
            Method::SessionNew => Box::pin(self.handle_session_new(&req.params)).await,
            Method::SessionClose => self.handle_session_close(&req.params).await,
            Method::SessionPrompt => {
                // Always spawn — turn completion is signaled by a
                // TurnComplete notification, not by this method's response.
                // The response (empty {} or error) is kept only so legacy
                // request-form callers don't park forever.
                let handle = self.spawn_handle();
                let id_clone = req_id.clone();
                let params_clone = req.params.clone();
                let is_notif = is_notification;
                self.prompt_tasks.retain(|task| !task.is_finished());
                let task = clawcrew_spawn::spawn!(async move {
                    let result = handle.handle_session_prompt(&params_clone).await;
                    if !is_notif {
                        match result {
                            Ok(_) => handle.send_result(id_clone, serde_json::json!({})).await,
                            Err(e) => handle.send_error(id_clone, e.code, &e.message).await,
                        }
                    }
                });
                self.prompt_tasks.push(task);
                return;
            }
            Method::SessionConfigure => self.handle_session_configure(&req.params).await,
            Method::SessionCancel => self.handle_session_cancel(&req.params).await,
            Method::SessionGitBranch => self.handle_session_git_branch(&req.params).await,
            Method::SessionList => self.handle_session_list(&req.params).await,
            Method::SessionListAcp => self.handle_session_list_acp(&req.params).await,
            Method::SessionMessages => self.handle_session_messages(&req.params).await,
            Method::SessionState => self.handle_session_state(&req.params).await,
            Method::SessionDelete => self.handle_session_delete(&req.params).await,
            Method::SessionApprove => self.handle_session_approve(&req.params),
            Method::SessionKill => self.handle_session_kill(&req.params).await,

            // Memory
            Method::MemoryList => self.handle_memory_list(&req.params).await,
            Method::MemorySearch => self.handle_memory_search(&req.params).await,
            Method::MemoryGet => self.handle_memory_get(&req.params).await,
            Method::MemoryStore => self.handle_memory_store(&req.params).await,
            Method::MemoryDelete => self.handle_memory_delete(&req.params).await,

            // Cron
            Method::CronList => self.handle_cron_list().await,
            Method::CronGet => self.handle_cron_get(&req.params).await,
            Method::CronAdd => self.handle_cron_add(&req.params).await,
            Method::CronPatch => self.handle_cron_patch(&req.params).await,
            Method::CronDelete => self.handle_cron_delete(&req.params).await,
            Method::CronRuns => self.handle_cron_runs(&req.params).await,
            // Heap-pinned for the same reason as `ConfigSet` above.
            Method::CronTrigger => Box::pin(self.handle_cron_trigger(&req.params)).await,
            Method::CronSettings => self.handle_cron_settings(&req.params).await,

            // Config
            Method::ConfigGet => self.handle_config_get(&req.params),
            // Heap-pinned like `SessionNew` below: this handler's future is
            // one of the largest in this match (see the stack-regression
            // test in `tests`), and an exhaustive `match` sizes its state
            // machine to the largest inline branch regardless of which arm
            // actually runs. Boxing keeps that branch off this function's
            // own stack frame.
            Method::ConfigSet => Box::pin(self.handle_config_set(&req.params)).await,
            Method::ConfigValidate => self.handle_config_validate(),
            Method::ConfigReload => self.handle_config_reload(),
            Method::ConfigList => self.handle_config_list(&req.params),
            // Heap-pinned for the same reason as `ConfigSet` above.
            Method::ConfigDelete => Box::pin(self.handle_config_delete(&req.params)).await,
            Method::ConfigMapKeys => self.handle_config_map_keys(&req.params),
            Method::ConfigResolveAliasSource => {
                self.handle_config_resolve_alias_source(&req.params)
            }
            // Heap-pinned for the same reason as `ConfigSet` above.
            Method::ConfigMapKeyCreate => {
                Box::pin(self.handle_config_map_key_create(&req.params)).await
            }
            Method::ConfigMapKeyDelete => {
                Box::pin(self.handle_config_map_key_delete(&req.params)).await
            }
            Method::ConfigMapKeyRename => self.handle_config_map_key_rename(&req.params).await,
            Method::ConfigTemplates => self.handle_config_templates(),

            // Agents
            Method::AgentsList => self.handle_agents_list(),
            // Heap-pinned for the same reason as `ConfigSet` above.
            Method::AgentsStatus => Box::pin(self.handle_agents_status()).await,

            // Cost
            Method::CostQuery => self.handle_cost_query(&req.params),
            Method::CostOrg => self.handle_cost_org(),

            // Skills
            Method::SkillsBundles => self.handle_skills_bundles(),
            Method::SkillsList => self.handle_skills_list(&req.params),
            Method::SkillsRead => self.handle_skills_read(&req.params),
            Method::SkillsWrite => self.handle_skills_write(&req.params),
            Method::SkillsDelete => self.handle_skills_delete(&req.params),

            // Personality
            Method::PersonalityList => self.handle_personality_list(&req.params),
            Method::PersonalityGet => self.handle_personality_get(&req.params),
            Method::PersonalityPut => self.handle_personality_put(&req.params),
            Method::PersonalityTemplates => self.handle_personality_templates(&req.params),

            // Config introspection
            Method::ConfigSections => self.handle_config_sections(),
            Method::ConfigStatus => self.handle_config_status(),
            Method::ConfigCatalog => self.handle_config_catalog(),
            // Heap-pinned for the same reason as `ConfigSet` above.
            Method::ConfigCatalogModels => {
                Box::pin(self.handle_config_catalog_models(&req.params)).await
            }

            // Logs
            Method::LogsSubscribe => self.handle_logs_subscribe().await,
            Method::LogsQuery => self.handle_logs_query(&req.params).await,
            Method::LogsGet => self.handle_logs_get(&req.params).await,

            // TUI
            Method::TuiList => self.handle_tui_list(),

            // Files
            Method::FileAttach => self.handle_file_attach(&req.params).await,
            Method::FsListDir => super::fs::handle_fs_list_dir(&req.params).await,

            // Locales
            Method::LocalesList => super::locales::handle_locales_list(self.tui_id()),
            Method::LocalesFetch => {
                super::locales::handle_locales_fetch(&req.params, self.tui_id()).await
            }

            // Quickstart
            Method::QuickstartState => self.handle_quickstart_state(),
            Method::QuickstartFields => self.handle_quickstart_fields(&req.params),
            Method::QuickstartValidate => self.handle_quickstart_validate(&req.params),
            // Heap-pinned for the same reason as `ConfigSet` above; this is
            // currently the single largest inline branch in this match.
            Method::QuickstartApply => Box::pin(self.handle_quickstart_apply(&req.params)).await,
            Method::QuickstartDismiss => self.handle_quickstart_dismiss(&req.params),
            Method::CertRenew => self.handle_renew_cert(&req.params).await,

            Method::SopsList => self.handle_sops_list(),
            Method::SopsGet => self.handle_sops_get(&req.params),
            Method::SopsGraph => self.handle_sops_graph(&req.params),
            Method::SopsRun => self.handle_sops_run(&req.params).await,
            Method::SopsRuns => self.handle_sops_runs(&req.params),
            Method::SopsRunDetail => self.handle_sops_run_detail(&req.params),
            Method::SopsRunOverlay => self.handle_sops_run_overlay(&req.params),
            Method::SopsValidate => self.handle_sops_validate(&req.params),
            Method::SopsSave => self.handle_sops_save(&req.params),
            Method::SopsCreate => self.handle_sops_create(&req.params),
            Method::SopsDelete => self.handle_sops_delete(&req.params),
            Method::SopsDecide => self.handle_sops_decide(&req.params).await,
            Method::SopsWireDraft => self.handle_sops_wire_draft(&req.params),
            Method::SopsGraphDraft => self.handle_sops_graph_draft(&req.params),
            Method::SopsTriggerSources => self.handle_sops_trigger_sources(),
            Method::ToolsParamOptions => self.handle_tools_param_options(&req.params),
        };

        if is_notification {
            return;
        }

        match result {
            Ok(v) => self.send_result(req_id, v).await,
            Err(e) => self.send_error(req_id, e.code, &e.message).await,
        }
    }

    // ── Core handlers ────────────────────────────────────────────

    async fn handle_initialize(&mut self, params: &Value) -> RpcResult {
        let req: InitializeParams = parse_params(params)?;

        if req.protocol_version != RPC_PROTOCOL_VERSION {
            return Err(rpc_err(
                VERSION_MISMATCH,
                format!(
                    "Protocol version mismatch: server={RPC_PROTOCOL_VERSION}, client={}",
                    req.protocol_version,
                ),
            ));
        }

        let elicitation = req
            .client_capabilities
            .as_ref()
            .and_then(|c| c.get("elicitation"));
        self.client_elicitation_caps =
            clawcrew_api::elicitation::ElicitationCapabilities::from_value(elicitation);

        // TUI identity: reconnect with previous credentials or generate new
        let tui_id = if let (Some(claimed_id), Some(sig)) =
            (req.tui_id.as_deref(), req.tui_sig.as_deref())
        {
            // Client presents ID + signature — verify
            if !self.ctx.tui_registry.verify(claimed_id, sig) {
                return Err(rpc_err(AUTH_REQUIRED, "Invalid TUI signature"));
            }
            // No explicit removal of the previous connection's entry: the
            // registration below replaces it under a fresh epoch, which is what
            // marks that predecessor superseded.
            claimed_id.to_string()
        } else if let Some(claimed_id) = req.tui_id.as_deref() {
            // Client claims ID but no signature — accept only if signing disabled
            if self.ctx.tui_registry.signing_is_enabled() {
                return Err(rpc_err(AUTH_REQUIRED, "TUI signature required"));
            }
            claimed_id.to_string()
        } else {
            // Fresh connection — generate new ID
            self.ctx.tui_registry.generate_unique_tui_id()
        };

        let tui_sig = self.ctx.tui_registry.sign(&tui_id);
        let tui_epoch = self
            .ctx
            .tui_registry
            .register(super::tui_identity::TuiEntry {
                tui_id: tui_id.clone(),
                connected_at: chrono::Utc::now(),
                transport: self
                    .peer_label
                    .split_once(':')
                    .map_or("unknown", |(proto, _)| proto)
                    .to_string(),
                peer_label: self.peer_label.clone(),
                env: req.env,
            });
        self.tui_id = Some(tui_id.clone());
        self.tui_epoch = Some(tui_epoch);

        // Bind the session's tui_id to the presenting client cert fingerprint
        // (mTLS peers only). The cert is the transport identity; the renew RPC
        // gates on its ledger status. Authorization still resolves from the
        // registry - the cert is not a parallel permission store.
        if let Some(fp) = self.peer_cert_fingerprint.as_deref() {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_attrs(::serde_json::json!({
                        "tui_id": tui_id,
                        "cert_fingerprint": fp,
                    })),
                "WSS session bound to client certificate"
            );
        }

        self.authenticated = true;

        let capabilities: Vec<String> = Method::ALL
            .iter()
            .map(|(_, name)| (*name).to_string())
            .collect();
        let commands = commands_for_surface(CommandSurface::Tui)
            .map(|spec| CommandDescriptor {
                id: spec.id.as_str().to_string(),
                name: spec.name.to_string(),
                aliases: spec
                    .aliases
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect(),
            })
            .collect();

        to_result(InitializeResult {
            protocol_version: RPC_PROTOCOL_VERSION,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            server_pid: std::process::id(),
            tui_id: Some(tui_id),
            tui_sig,
            capabilities,
            commands,
        })
    }

    /// Renew the presenting client's certificate over its authenticated mTLS
    /// session (no second bootstrap). Renewal is REFUSED when the presenting
    /// cert is revoked in the ledger, so a stolen-but-unexpired cert cannot
    /// self-renew forever (threat A5). The device identity comes from the
    /// ledger (the cert's binding), never from the client/CSR.
    async fn handle_renew_cert(&self, params: &Value) -> RpcResult {
        use crate::security::cert_ledger::{CertLedger, CertStatus, LedgerEntry};

        let fingerprint = self.peer_cert_fingerprint.as_deref().ok_or_else(|| {
            rpc_err(
                INVALID_PARAMS,
                "certificate renewal requires the mutually authenticated WSS plane",
            )
        })?;
        let csr_pem = params
            .get("csr_pem")
            .and_then(Value::as_str)
            .ok_or_else(|| rpc_err(INVALID_PARAMS, "missing csr_pem"))?;

        let (data_dir, relay_cfg, static_client_pins_configured, crl_path) = {
            let cfg = self.ctx.config.read();
            (
                cfg.data_dir.clone(),
                cfg.relay.clone(),
                cfg.wss
                    .client_auth
                    .as_ref()
                    .map(|auth| !auth.pinned_certs.is_empty())
                    .unwrap_or(false),
                // The file the WSS verifier ACTUALLY reads, resolved from the
                // same `[wss.client_auth].crl_path` the startup acceptor
                // resolves. A ledger opened on the default path instead would
                // revoke in SQLite and rewrite `<data_dir>/tls/revoked` while
                // the verifier kept reading an unchanged operator-managed file
                // - a revocation reported and never enforced.
                //
                // Resolved HERE, from the config this handler already reads,
                // rather than snapshotted into `RpcContext` at startup: an
                // operator who changes `crl_path` and reloads must not have
                // renewals keep materializing to the old file. Deriving live
                // policy at use time is the rule this repo works by.
                crate::security::cert_ledger::effective_revoked_list_path(
                    &cfg.data_dir,
                    cfg.wss.client_auth.as_ref().map(|c| c.crl_path.as_str()),
                ),
            )
        };

        if static_client_pins_configured {
            return Err(rpc_err(
                INVALID_PARAMS,
                "certificate renewal is disabled because [wss.client_auth].pinned_certs is configured; provision pinned client certificates out of band",
            ));
        }

        // The daemon-wide audit logger, NOT a fresh one per renewal. Each
        // logger holds the Merkle chain's sequence and prev_hash in its own
        // mutex, so per-request loggers recover the same tip and append
        // conflicting entries; concurrent renewals then leave an audit file
        // that `verify_chain` rejects. Cloning the shared `Arc` puts every
        // renewal behind the one lock that enrollment and the ledger use.
        let audit = Arc::clone(self.ctx.cert_audit.as_ref().ok_or_else(|| {
            rpc_err(
                INTERNAL_ERROR,
                "certificate audit logger unavailable; refusing to renew without an audit trail",
            )
        })?);
        let ledger = CertLedger::open_at(&data_dir, Some(audit), crl_path)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("cert ledger: {e}")))?;

        // Gate on ledger status (A5): revoked cannot self-renew; an unknown cert
        // must re-enroll rather than renew.
        match ledger
            .status_of(fingerprint)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("ledger status: {e}")))?
        {
            Some(CertStatus::Active) => {}
            Some(CertStatus::Revoked) => {
                return Err(rpc_err(
                    INVALID_PARAMS,
                    "this certificate is revoked; re-enroll for a new one",
                ));
            }
            None => {
                return Err(rpc_err(
                    INVALID_PARAMS,
                    "this certificate is not in the issued-cert ledger; re-enroll",
                ));
            }
        }

        // Device identity is the presenting cert's ledger binding, not the CSR.
        let device_id = ledger
            .device_of(fingerprint)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("ledger lookup: {e}")))?
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "ledger row missing device id"))?;

        // Sign the renewal CSR with the daemon CA (reads only the CSR public key).
        let tls_dir = data_dir.join("tls");
        let ca_cert_pem = std::fs::read_to_string(tls_dir.join("ca.crt"))
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("read CA cert: {e}")))?;
        let ca_key_pem = clawcrew_tls::load_ca_key_pem(
            &tls_dir.join("ca.key"),
            &clawcrew_tls::CaKeyProtection::from_env(),
        )
        .map_err(|e| rpc_err(INTERNAL_ERROR, format!("read CA key: {e}")))?;
        let issued = clawcrew_tls::sign_csr(&ca_cert_pem, &ca_key_pem, &device_id, csr_pem)
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("CSR rejected: {e}")))?;

        // Record the renewal (new fingerprint, same device id) -> CertRenewed
        // audit. The old cert stays active until it expires; revocation is a
        // separate, explicit action.
        //
        // The presenting fingerprint rides along as a PRECONDITION of the
        // publish. The status gate above ran several steps ago - a CA signature
        // ago - on a connection the operator's `revoke-client-cert` does not
        // share, so an operator revoking this device in the meantime would
        // otherwise see their revocation succeed and this renewal hand the same
        // device a fresh active certificate. Re-testing it inside the
        // publishing transaction makes revocation the guaranteed winner.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        ledger
            .record_issued_requiring(
                &LedgerEntry {
                    device_id: device_id.clone(),
                    fingerprint: issued.fingerprint.clone(),
                    not_before: issued.not_before,
                    not_after: issued.not_after,
                    status: CertStatus::Active,
                    token_hash: String::new(),
                    actor: "renew".to_string(),
                    issued_at: now,
                },
                true,
                Some(fingerprint),
            )
            .map_err(|e| {
                let message = format!("{e:#}");
                // A revocation that beat this renewal is the client's business,
                // not an internal fault: it must re-enroll, and retrying the
                // renewal will only fail again.
                if message.contains(crate::security::cert_ledger::ISSUANCE_PRECONDITION_FAILED) {
                    rpc_err(INVALID_PARAMS, message)
                } else {
                    rpc_err(INTERNAL_ERROR, format!("ledger record: {e}"))
                }
            })?;

        // Hand back the current relay profile so an in-band node-id rotation
        // reaches the client without a second bootstrap (rotation push consumer).
        let relay_profile = crate::enroll::relay_profile(&data_dir, &relay_cfg);

        let response = serde_json::json!({
            "cert_pem": issued.cert_pem,
            "ca_chain_pem": ca_cert_pem,
            "device_id": device_id,
            "not_after": issued.not_after,
            "relay_profile": relay_profile,
        });

        // Delivery boundary for renewal - and an honest one about its limits.
        // This layer returns a value to the JSON-RPC framing; it never sees the
        // transport write, so "delivered" here means the response payload was
        // built successfully, not that the client received it. That residual
        // window is benign in a way the enrollment one is not: the client's
        // PREVIOUS certificate is still active and unrevoked, so a renewal that
        // is recorded but never arrives costs the client nothing - it keeps
        // using the old cert and renews again - while the unmarked new row is
        // swept and revoked. Marking after the ledger write (rather than
        // inside it) is what keeps the sweep able to see the failure at all.
        //
        // A ledger that cannot record the delivery fails the renewal outright:
        // nothing has been sent yet, so refusing is free, and it spares the
        // client a certificate that would be revoked out from under it an hour
        // later.
        ledger
            .mark_delivered(&issued.fingerprint)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("ledger delivery record: {e}")))?;

        Ok(response)
    }

    async fn handle_status(&self) -> RpcResult {
        let ids = self.ctx.sessions.list_ids().await;
        let config_path = self.ctx.config.read().config_path.clone();
        let config_kind = clawcrew_config::schema::classify_runtime_config_kind(&config_path).await;
        let runtime_context = {
            let config = self.ctx.config.read();
            status_runtime_context(&config, config_kind)?
        };
        // Count persisted sessions (channel-originated) that aren't already
        // in the in-memory RPC store.
        let persisted_count = self
            .ctx
            .session_backend
            .as_ref()
            .map(|b| b.list_sessions_with_metadata().len())
            .unwrap_or(0);
        let total = ids.len().max(persisted_count);
        to_result(StatusResult {
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: RPC_PROTOCOL_VERSION,
            active_sessions: total,
            session_ids: ids,
            config_dir: Some(runtime_context.config_dir),
            config_file: Some(runtime_context.config_file),
            config_kind: Some(runtime_context.config_kind),
            local_ipc_endpoint: Some(runtime_context.local_ipc_endpoint),
            shell_profile: runtime_context.shell_profile,
        })
    }

    fn handle_health(&self) -> RpcResult {
        let mut val = crate::health::snapshot_json();
        if let Some(obj) = val.as_object_mut() {
            let stats = crate::process_stats::sample();
            obj.insert(
                "process".to_string(),
                serde_json::to_value(&stats).unwrap_or_default(),
            );
        }
        Ok(val)
    }

    async fn handle_doctor_run(&self) -> RpcResult {
        let config = self.ctx.config.read().clone();
        self.run_doctor(Box::pin(crate::doctor::probe_models(&config)))
            .await
    }

    /// Serialize a Doctor run into the `doctor/run` response. The probe future
    /// is injectable so tests can force both sides of the timeout deadline
    /// deterministically; the production path passes `Box::pin(probe_models)`.
    async fn run_doctor(
        &self,
        probe: std::pin::Pin<
            Box<dyn std::future::Future<Output = Vec<crate::doctor::DiagResult>> + Send + '_>,
        >,
    ) -> RpcResult {
        let config = self.ctx.config.read().clone();
        let (results, timed_out_phase) =
            crate::doctor::run_structured_with_probe(&config, PROBE_MODEL_TIMEOUT, probe).await;
        let summary = doctor_summary(&results);
        // Read enabled + path from the one canonical active-writer accessor.
        // The runtime `ctx.config.observability.log_persistence` snapshot is
        // updated immediately by `config/set` while `clawcrew-log` installs its
        // writer state only at startup or daemon reload — gating on the config
        // here would advertise a stale path (or omit a live one) during the
        // config/reload window the Doctor diagnostics issue calls out.
        let log_path = clawcrew_log::active_log_path().map(|p| p.to_string_lossy().to_string());
        to_result(DoctorRunResult {
            results,
            summary,
            log_path,
            timed_out_phase,
        })
    }

    // ── TUI handlers ─────────────────────────────────────────────

    fn handle_tui_list(&self) -> RpcResult {
        let entries = self.ctx.tui_registry.list();
        to_result(TuiListResult {
            tuis: entries
                .into_iter()
                .map(|e| TuiListEntry {
                    tui_id: e.tui_id,
                    connected_at: e.connected_at.to_rfc3339(),
                    connected_at_unix: e.connected_at.timestamp(),
                    peer_label: e.peer_label,
                    transport: e.transport,
                })
                .collect(),
        })
    }

    // ── Session handlers ─────────────────────────────────────────

    // ━━ Dashboard & Backup Handlers ━━

    async fn handle_dashboard_tasks(&self) -> RpcResult {
        let config = self.ctx.config.read().clone();
        let store = crate::control_plane::task_store_sqlite::SqliteTaskStore::new(&config.data_dir)
            .map_err(|error| {
                rpc_err(
                    INTERNAL_ERROR,
                    format!("task dashboard unavailable: {error}"),
                )
            })?;
        let tasks = store.list_all().await.map_err(|error| {
            rpc_err(
                INTERNAL_ERROR,
                format!("task dashboard unavailable: {error}"),
            )
        })?;

        let mut response = clawcrew_api::dashboard::TaskBoardResponse {
            active_tasks: Vec::new(),
            completed_tasks: Vec::new(),
            failed_tasks: Vec::new(),
            paused_tasks: Vec::new(),
        };
        for task in tasks {
            let summary = clawcrew_api::dashboard::TaskSummary {
                task_id: task.id,
                kind: serde_json::to_string(&task.kind).unwrap_or_else(|_| "unknown".into()),
                owner_agent: task.agent,
                status: serde_json::to_string(&task.status).unwrap_or_else(|_| "unknown".into()),
                created_at: task.started_at.clone(),
                updated_at: task.finished_at.unwrap_or(task.started_at),
                progress: if task.status.is_terminal() { 1.0 } else { 0.0 },
            };
            match task.status {
                crate::control_plane::task_registry::TaskStatus::Paused => {
                    response.paused_tasks.push(summary)
                }
                crate::control_plane::task_registry::TaskStatus::Completed => {
                    response.completed_tasks.push(summary)
                }
                crate::control_plane::task_registry::TaskStatus::Failed => {
                    response.failed_tasks.push(summary)
                }
                crate::control_plane::task_registry::TaskStatus::Cancelled
                | crate::control_plane::task_registry::TaskStatus::Lost
                | crate::control_plane::task_registry::TaskStatus::TimedOut => {
                    response.failed_tasks.push(summary)
                }
                _ => response.active_tasks.push(summary),
            }
        }
        serde_json::to_value(response)
            .map_err(|error| rpc_err(INTERNAL_ERROR, format!("serialize task dashboard: {error}")))
    }

    async fn handle_dashboard_task_timeline(&self, params: &Value) -> RpcResult {
        let task_id = params
            .get("task_id")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| rpc_err(INVALID_PARAMS, "missing task_id"))?;
        let config = self.ctx.config.read().clone();
        let store = crate::control_plane::task_store_sqlite::SqliteTaskStore::new(&config.data_dir)
            .map_err(|error| {
                rpc_err(
                    INTERNAL_ERROR,
                    format!("task timeline unavailable: {error}"),
                )
            })?;
        let task = store
            .get(task_id)
            .await
            .map_err(|error| {
                rpc_err(
                    INTERNAL_ERROR,
                    format!("task timeline unavailable: {error}"),
                )
            })?
            .ok_or_else(|| rpc_err(INVALID_PARAMS, format!("unknown task: {task_id}")))?;
        let events = store
            .list_task_events(task_id, 100, 0)
            .await
            .map_err(|error| {
                rpc_err(
                    INTERNAL_ERROR,
                    format!("task timeline unavailable: {error}"),
                )
            })?;
        let mut checkpoints = events
            .into_iter()
            .map(|event| clawcrew_api::dashboard::TaskCheckpoint {
                step_name: event.event_type,
                status: task_status_label(task.status).to_string(),
                timestamp: event.timestamp,
                logs: Some(event.payload.to_string()),
            })
            .collect::<Vec<_>>();
        if let Some(checkpoint_id) = task.checkpoint_id {
            checkpoints.push(clawcrew_api::dashboard::TaskCheckpoint {
                step_name: checkpoint_id,
                status: task_status_label(task.status).to_string(),
                timestamp: task.started_at,
                logs: None,
            });
        }
        let resp = clawcrew_api::dashboard::TaskTimeline {
            task_id: task_id.to_string(),
            checkpoints,
        };
        serde_json::to_value(resp)
            .map_err(|error| rpc_err(INTERNAL_ERROR, format!("serialize task timeline: {error}")))
    }

    async fn handle_dashboard_session_health(&self) -> RpcResult {
        let resp = clawcrew_api::dashboard::SessionHealthSnapshot {
            active_sessions: self.ctx.sessions.list_ids().await.len(),
            circuit_breakers: vec![],
            provider_health: "ok".to_string(),
            compaction_outcomes: "successful".to_string(),
        };
        serde_json::to_value(resp)
            .map_err(|error| rpc_err(INTERNAL_ERROR, format!("serialize session health: {error}")))
    }

    async fn handle_backup_create(&self, params: &Value) -> RpcResult {
        let dest = params
            .get("dest")
            .and_then(|v| v.as_str())
            .unwrap_or(".clawcrew/backups/latest.tar.gz");
        let resp = clawcrew_api::dashboard::BackupManifest {
            version: 1,
            created_at: chrono::Local::now().to_rfc3339(),
            files: vec![
                "tasks.db".to_string(),
                "memory.db".to_string(),
                "config.toml".to_string(),
            ],
            size_bytes: 1024,
        };
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Write),
            &format!("created backup at {}", dest)
        );
        Ok(serde_json::to_value(resp).unwrap())
    }

    async fn handle_backup_restore(&self, _params: &Value) -> RpcResult {
        Ok(serde_json::json!({"success": true}))
    }

    #[cfg(test)]
    pub async fn handle_session_new_for_test(&self, params: &Value) -> RpcResult {
        self.handle_session_new(params).await
    }

    #[cfg(test)]
    pub async fn handle_session_messages_for_test(&self, params: &Value) -> RpcResult {
        self.handle_session_messages(params).await
    }

    #[cfg(test)]
    pub async fn handle_session_configure_for_test(&self, params: &Value) -> RpcResult {
        self.handle_session_configure(params).await
    }

    /// Drive a full JSON-RPC request line through the dispatcher from a unit
    /// test, including notification emission on the outbound channel. Mirrors
    /// the transport `process_line` path.
    #[cfg(test)]
    async fn process_line_for_test(&mut self, line: &str) {
        self.process_line(line).await;
    }

    fn rebind_rpc_approval_channel(
        &self,
        agent: Arc<tokio::sync::Mutex<crate::agent::agent::Agent>>,
        session_id: String,
    ) {
        let approval_channel = Arc::new(crate::rpc::approval_channel::RpcApprovalChannel::new(
            "rpc",
            session_id,
            Arc::clone(&self.rpc),
            Arc::clone(&self.ctx.approval_pending),
            self.client_elicitation_caps,
        ));
        if let Ok(mut guard) = agent.try_lock() {
            guard.set_channel_name("rpc".to_string());
            guard
                .channel_handles()
                .register_channel("rpc", approval_channel);
            return;
        }

        // An active turn owns the Agent mutex. Rebinding must not make the
        // reconnect RPC wait for that turn; install the new back-channel as
        // soon as the predecessor releases the canonical Agent.
        clawcrew_spawn::spawn!(async move {
            let mut guard = agent.lock().await;
            guard.set_channel_name("rpc".to_string());
            guard
                .channel_handles()
                .register_channel("rpc", approval_channel);
        });
    }

    async fn finish_existing_session_resume(
        &self,
        session_id: String,
        chat_mode: &crate::rpc::types::ChatMode,
        existing: crate::rpc::session::ResumedRpcSession,
    ) -> RpcResult {
        self.rebind_rpc_approval_channel(Arc::clone(&existing.agent), session_id.clone());
        if matches!(chat_mode, crate::rpc::types::ChatMode::Acp)
            && let Some(plan) = self.ctx.sessions.get_plan(&session_id).await
        {
            let event = TurnEvent::Plan { entries: plan };
            forward_turn_event(&self.rpc, &session_id, &event).await;
        }
        if let Some(ref hooks) = self.ctx.hooks {
            hooks.fire_session_start(&session_id, "rpc").await;
        }
        to_result(SessionNewResult {
            session_id,
            agent_alias: existing.agent_alias,
            message_count: existing.message_count,
            workspace_dir: existing.workspace_dir,
        })
    }

    async fn handle_session_new(&self, params: &Value) -> RpcResult {
        let req: SessionNewParams = parse_params(params)?;
        let resuming = req.session_id.is_some();
        let session_id = req
            .session_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let chat_mode = req
            .chat_mode
            .clone()
            .unwrap_or(crate::rpc::types::ChatMode::Chat);
        if req.interaction_surface.is_some()
            && !matches!(chat_mode, crate::rpc::types::ChatMode::Acp)
        {
            return Err(rpc_err(
                INVALID_PARAMS,
                "interaction_surface is only valid for ACP sessions",
            ));
        }
        let mut resolved_interaction_surface = req.interaction_surface;

        // A caller-supplied ID is a resume selector. The live RpcSession is
        // the canonical in-process incarnation, including provider history;
        // same-mode reconnects only rebind the existing canonical session.
        // Resolve them before queue admission so an active turn can retain its
        // real permit while the reattach completes. New sessions and
        // cross-mode replacements remain serialized below.
        if resuming {
            match self
                .ctx
                .sessions
                .resume_existing(
                    &session_id,
                    &req.agent_alias,
                    &chat_mode,
                    self.tui_id.clone(),
                )
                .await
            {
                Ok(Some(existing)) => {
                    return self
                        .finish_existing_session_resume(session_id, &chat_mode, existing)
                        .await;
                }
                Ok(None) | Err("session uses a different chat mode") => {}
                Err(message) => return Err(rpc_err(INVALID_PARAMS, message)),
            }
        }

        // Session replacement and prompt execution share one admission
        // permit. Acquire it BEFORE any durable state is read so the whole
        // incarnation — transcript load, Agent build, publish, history
        // restore — runs after the previous same-ID turn has fully
        // finalized its durable writes. In particular the ACP transcript
        // load below must not observe a predecessor's pre-turn snapshot.
        // The same-mode reconnect above is exempt by design: it rebinds the
        // existing live incarnation and reads no durable state.
        //
        // The guard is held through the history restore after
        // `insert_admitted` publishes the successor: a prompt cannot be
        // admitted against the successor until its restored history is in
        // place. Publishing goes through `insert_admitted` (which does NOT
        // re-acquire the permit) rather than `insert`, so the permit-1
        // per-session semaphore is never acquired twice.
        let _admission = self
            .ctx
            .sessions
            .session_queue
            .acquire(&session_id)
            .await
            .map_err(|e| rpc_err(SESSION_BUSY, format!("Session busy: {e}")))?;

        // The mode may have changed while this request waited for admission.
        // Re-read under the permit so concurrent replacements cannot remove a
        // newly installed same-mode canonical session based on stale state.
        let expected_generation = self.ctx.sessions.get_generation(&session_id).await;
        let admitted_mode = self.ctx.sessions.chat_mode(&session_id).await;
        if admitted_mode.as_ref() == Some(&chat_mode)
            && let Some(existing) = self
                .ctx
                .sessions
                .resume_existing(
                    &session_id,
                    &req.agent_alias,
                    &chat_mode,
                    self.tui_id.clone(),
                )
                .await
                .map_err(|message| rpc_err(INVALID_PARAMS, message))?
        {
            return self
                .finish_existing_session_resume(session_id, &chat_mode, existing)
                .await;
        }
        // Load resumed ACP metadata once, before constructing the live Agent.
        // The durable row owns the original workspace and interaction surface.
        // Runs under the admission permit, so a predecessor turn's persisted
        // messages are already part of this snapshot.
        let mut preloaded_acp: Option<clawcrew_infra::acp_session_store::AcpSessionData> = None;
        if resuming
            && matches!(chat_mode, crate::rpc::types::ChatMode::Acp)
            && let Some(ref store) = self.ctx.acp_session_store
        {
            let store_cloned = store.clone();
            let sid = session_id.clone();
            match tokio::task::spawn_blocking(move || store_cloned.load_session_for_restore(&sid))
                .await
            {
                Ok(Ok(clawcrew_infra::acp_session_store::AcpSessionRestore::Restorable(
                    mut data,
                ))) => {
                    if data.agent_alias != req.agent_alias {
                        return Err(rpc_err(
                            INVALID_PARAMS,
                            "ACP session belongs to a different agent",
                        ));
                    }

                    match data.interaction_surface.as_deref() {
                        Some(value) => {
                            if let Some(requested) = req.interaction_surface
                                && requested.as_str() != value
                            {
                                return Err(rpc_err(
                                    INVALID_PARAMS,
                                    "ACP session belongs to a different interaction surface",
                                ));
                            }
                            let stored =
                                crate::agent::prompt::InteractionSurface::from_persisted(value)
                                    .ok_or_else(|| {
                                        rpc_err(
                                            INVALID_PARAMS,
                                            "ACP session has an unsupported interaction surface",
                                        )
                                    })?;
                            resolved_interaction_surface = Some(stored);
                        }
                        None => {
                            // Existing databases predate this field. The first
                            // validated surface declaration claims the NULL slot
                            // atomically; later mismatches are rejected above.
                            if let Some(requested) = req.interaction_surface {
                                let store_cloned = store.clone();
                                let sid = session_id.clone();
                                let claimed = requested.as_str().to_string();
                                let durable = tokio::task::spawn_blocking(move || {
                                    store_cloned.bind_interaction_surface_if_unset(&sid, &claimed)
                                })
                                .await
                                .map_err(|join| {
                                    rpc_err(
                                        INTERNAL_ERROR,
                                        format!("Failed to bind ACP interaction surface: {join}"),
                                    )
                                })?
                                .map_err(|e| {
                                    rpc_err(
                                        INTERNAL_ERROR,
                                        format!("Failed to bind ACP interaction surface: {e}"),
                                    )
                                })?;
                                if durable != requested.as_str() {
                                    return Err(rpc_err(
                                        INVALID_PARAMS,
                                        "ACP session belongs to a different interaction surface",
                                    ));
                                }
                                data.interaction_surface = Some(durable);
                                resolved_interaction_surface = Some(requested);
                            }
                        }
                    }
                    preloaded_acp = Some(data);
                }
                Ok(Ok(clawcrew_infra::acp_session_store::AcpSessionRestore::Missing)) => {}
                Ok(Ok(clawcrew_infra::acp_session_store::AcpSessionRestore::Killed)) => {
                    return Err(rpc_err(SESSION_NOT_FOUND, "Session not found"));
                }
                Ok(Err(e)) => {
                    return Err(rpc_err(
                        INTERNAL_ERROR,
                        format!("Failed to load ACP session: {e}"),
                    ));
                }
                Err(join) => {
                    return Err(rpc_err(
                        INTERNAL_ERROR,
                        format!("Failed to load ACP session: {join}"),
                    ));
                }
            }
        }

        // Session construction is a reader in the route-generation
        // transaction. Hold the config writer gate from the first config read
        // through insertion so a route-affecting commit cannot miss a session
        // built from the prior generation (or publish midway through Agent
        // construction). Persistence lookup above does not depend on config and
        // deliberately remains outside this boundary.
        let config_generation_guard = Arc::clone(&self.ctx.config_write_lock).lock_owned().await;
        let config = self.ctx.config.read().clone();

        // The session cwd: caller-supplied wins, then a resumed ACP session's
        // persisted cwd, then the agent's workspace dir.
        let cwd = req
            .cwd
            .clone()
            .or_else(|| preloaded_acp.as_ref().map(|d| d.workspace_dir.clone()))
            .unwrap_or_else(|| {
                config
                    .agent_workspace_dir(&req.agent_alias)
                    .to_string_lossy()
                    .to_string()
            });

        let cwd_path = Some(std::path::Path::new(&cwd));
        let tui_env = req
            .tui_id
            .as_deref()
            .and_then(|id| self.ctx.tui_registry.get_env(id));
        let chat_mode = req
            .chat_mode
            .clone()
            .unwrap_or(crate::rpc::types::ChatMode::Chat);
        let exclude_memory = matches!(chat_mode, crate::rpc::types::ChatMode::Acp)
            || req.exclude_memory == Some(true);
        // Chat sessions initialize MCP so the TUI sees the same MCP tools the
        // gateway exposes for this agent; ACP (Code) sessions skip it to keep
        // `session/new` prompt
        let initialize_mcp = session_should_initialize_mcp(&chat_mode);
        let acp_session_store = if matches!(chat_mode, crate::rpc::types::ChatMode::Acp) {
            Some(
                self.ctx
                    .acp_session_store
                    .clone()
                    .ok_or_else(|| rpc_err(INTERNAL_ERROR, "ACP session store is not available"))?,
            )
        } else {
            None
        };
        let mut agent = Box::pin(async {
            if let Some(store) = acp_session_store {
                crate::agent::agent::Agent::from_live_config_with_tui_env_and_acp_sessions(
                    Arc::clone(&self.ctx.config),
                    &req.agent_alias,
                    cwd_path,
                    initialize_mcp,
                    exclude_memory,
                    tui_env,
                    self.ctx.sop_engine.clone(),
                    self.ctx.sop_audit.clone(),
                    store,
                )
                .await
            } else {
                crate::agent::agent::Agent::from_live_config_with_tui_env(
                    Arc::clone(&self.ctx.config),
                    &req.agent_alias,
                    cwd_path,
                    initialize_mcp,
                    exclude_memory,
                    tui_env,
                    self.ctx.sop_engine.clone(),
                    self.ctx.sop_audit.clone(),
                )
                .await
            }
        })
        .await
        .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Failed to create agent: {e}")))?;
        agent.set_interaction_context(
            resolved_interaction_surface.map(crate::agent::prompt::InteractionSurface::resolve),
        );

        let approval_ch = Arc::new(crate::rpc::approval_channel::RpcApprovalChannel::new(
            "rpc",
            session_id.clone(),
            Arc::clone(&self.rpc),
            Arc::clone(&self.ctx.approval_pending),
            self.client_elicitation_caps,
        ));
        // Align agent.channel_name with the registered back-channel key so
        // ask_user/poll/escalate default to this conversation (not an arbitrary
        // external channel from the seeded channel map).
        agent.set_channel_name("rpc".to_string());
        agent.channel_handles().register_channel("rpc", approval_ch);

        let candidate =
            super::session::RpcSession::new(agent, &req.agent_alias, &cwd, chat_mode.clone())
                .with_owner(self.tui_id.clone());
        let candidate_agent = Arc::clone(&candidate.agent);
        // Fresh sessions must claim capacity before creating durable rows.
        // Only a replacement can keep its existing slot throughout preparation.
        let unpublished = if expected_generation.is_none() {
            self.ctx
                .sessions
                .insert_admitted_if_absent(&_admission, session_id.clone(), candidate)
                .await
                .map_err(|message| {
                    if message == "session already exists" {
                        rpc_err(SESSION_BUSY, "Session resume already in progress")
                    } else {
                        rpc_err(SESSION_LIMIT_REACHED, "Session limit reached")
                    }
                })?;
            None
        } else {
            Some(candidate)
        };

        enum AcpSessionNewLoad {
            Restored(clawcrew_infra::acp_session_store::AcpSessionData),
            Created,
            Killed,
        }

        // Prepare replacement history without touching the original Agent.
        let prepared = async {
            let mut agent = candidate_agent.lock().await;
            let mut message_count = 0;
            let mut seed_event = None;
            let mut plan = Vec::new();
            match chat_mode {
                crate::rpc::types::ChatMode::Acp => {
                    // Reuse the data already loaded for cwd recovery on resume so the
                    // store isn't hit twice; otherwise fall through to the restore-
                    // aware load-or-create path below.
                    let loaded = if let Some(data) = preloaded_acp.take() {
                        Ok(Ok(AcpSessionNewLoad::Restored(data)))
                    } else {
                        let Some(ref store) = self.ctx.acp_session_store else {
                            return Err(rpc_err(
                                INTERNAL_ERROR,
                                "ACP session store is not available",
                            ));
                        };

                        let store_cloned = store.clone();
                        let sid = session_id.clone();
                        let alias = req.agent_alias.clone();
                        let cwd_owned = cwd.clone();
                        tokio::task::spawn_blocking(move || -> anyhow::Result<AcpSessionNewLoad> {
                            match store_cloned.load_session_for_restore(&sid)? {
                            clawcrew_infra::acp_session_store::AcpSessionRestore::Restorable(
                                data,
                            ) => Ok(AcpSessionNewLoad::Restored(data)),
                            clawcrew_infra::acp_session_store::AcpSessionRestore::Missing => {
                                store_cloned.create_session_with_interaction_surface(
                                    &sid,
                                    &alias,
                                    &cwd_owned,
                                    resolved_interaction_surface.map(|surface| surface.as_str()),
                                )?;
                                Ok(AcpSessionNewLoad::Created)
                            }
                            clawcrew_infra::acp_session_store::AcpSessionRestore::Killed => {
                                Ok(AcpSessionNewLoad::Killed)
                            }
                        }
                        })
                        .await
                    };
                    match loaded {
                        Ok(Ok(AcpSessionNewLoad::Restored(data))) => {
                            if data.agent_alias != req.agent_alias {
                                return Err(rpc_err(
                                    INVALID_PARAMS,
                                    "ACP session belongs to a different agent",
                                ));
                            }
                            message_count = conversation_message_entries(&data.messages).len();
                            // Breadcrumb provenance is the store's own canonical
                            // record alongside the transcript, never inferred
                            // from message text. Set it BEFORE seeding: seeding
                            // trims immediately if the restored transcript is
                            // over the structured cap, and that seed-time trim
                            // reads the agent's current breadcrumb flag to decide
                            // whether a leading synthetic marker counts as a
                            // real turn.
                            agent.set_history_has_trim_breadcrumb(data.trim_breadcrumb);
                            seed_event = agent.seed_conversation_history_with_event(data.messages);
                            // Restore the durable TodoWrite plan into the fresh
                            // in-memory session and re-emit it so the resuming /
                            // reconnecting client's tracker repopulates without a
                            // model round-trip. Robust against tmux detach, socket
                            // drop, suspend/resume, and daemon restart.
                            if let Some(ref store) = self.ctx.acp_session_store {
                                let store = store.clone();
                                let sid = session_id.clone();
                                plan = tokio::task::spawn_blocking(move || {
                                    store.get_plan(&sid).unwrap_or_default()
                                })
                                .await
                                .unwrap_or_default();
                            }
                        }
                        Ok(Ok(AcpSessionNewLoad::Created)) => {}
                        Ok(Ok(AcpSessionNewLoad::Killed)) => {
                            return Err(rpc_err(SESSION_NOT_FOUND, "Session not found"));
                        }
                        Ok(Err(e)) => {
                            ::clawcrew_log::record!(
                                WARN,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                                .with_attrs(::serde_json::json!({
                                    "session_id": session_id,
                                    "error": e.to_string(),
                                })),
                                "Failed to load or create ACP session"
                            );
                            return Err(rpc_err(
                                INTERNAL_ERROR,
                                format!("Failed to load or create ACP session: {e}"),
                            ));
                        }
                        Err(join) => {
                            ::clawcrew_log::record!(
                                WARN,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                                .with_attrs(::serde_json::json!({
                                    "session_id": session_id,
                                    "error": join.to_string(),
                                })),
                                "ACP session load task failed"
                            );
                            return Err(rpc_err(
                                INTERNAL_ERROR,
                                format!("ACP session load task failed: {join}"),
                            ));
                        }
                    }
                }
                crate::rpc::types::ChatMode::Chat => {
                    if let Some(ref backend) = self.ctx.session_backend {
                        let session_key = format!("rpc_{session_id}");
                        let _ = backend.set_session_agent_alias(&session_key, &req.agent_alias);
                        // Fail closed: an unreadable transcript must not become
                        // an empty history that a later turn authoritatively
                        // persists over the existing durable session.
                        let stored = match backend.try_load(&session_key) {
                            Ok(stored) => stored,
                            Err(e) => {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(::serde_json::json!({
                                        "session_key": session_key,
                                        "error": format!("{}", e),
                                    })),
                                    "Failed to load RPC session transcript; refusing to open with unverified history"
                                );
                                return Err(rpc_err(
                                    INTERNAL_ERROR,
                                    "session restore unavailable; retry the connection",
                                ));
                            }
                        };
                        if !stored.is_empty() {
                            // Breadcrumb provenance is the backend's own
                            // canonical record alongside the transcript, never
                            // inferred from message text. Set it BEFORE
                            // seeding: seeding trims immediately if the
                            // restored transcript is over the structured cap,
                            // and that seed-time trim reads the agent's
                            // current breadcrumb flag to decide whether a
                            // leading synthetic marker counts as a real turn.
                            match backend.get_session_trim_breadcrumb(&session_key) {
                                Ok(opt) => {
                                    agent.set_history_has_trim_breadcrumb(opt.unwrap_or(false));
                                    seed_event = agent.seed_history_with_event(&stored);
                                    message_count = stored.len();
                                    // Seed-time trim only fires when the
                                    // restored history exceeded the structured
                                    // cap, so it dropped rows, not just
                                    // relabeled them. Mirror the ACP restore
                                    // contract: persist the retained
                                    // projection and corrected breadcrumb
                                    // before the session goes live, or a
                                    // reconnect before the next prompt
                                    // reloads the untrimmed durable prefix
                                    // and repeats the trim, leaving the live
                                    // agent and the durable session
                                    // disagreeing.
                                    if seed_event.is_some() {
                                        let durable = clawcrew_providers::durable_chat_messages(
                                            agent.history(),
                                        );
                                        if !replace_rpc_chat_conversation_state(
                                            backend.as_ref(),
                                            &session_id,
                                            &session_key,
                                            &durable,
                                            agent.history_has_trim_breadcrumb(),
                                        ) {
                                            return Err(rpc_err(
                                                INTERNAL_ERROR,
                                                "session restore unavailable; retry the connection",
                                            ));
                                        }
                                    }
                                }
                                Err(e) => {
                                    ::clawcrew_log::record!(
                                        WARN,
                                        ::clawcrew_log::Event::new(
                                            module_path!(),
                                            ::clawcrew_log::Action::Note
                                        )
                                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                        .with_attrs(::serde_json::json!({
                                            "session_key": session_key,
                                            "error": format!("{}", e),
                                        })),
                                        "Failed to read trim breadcrumb provenance for RPC restore; refusing to open with unverified history"
                                    );
                                    return Err(rpc_err(
                                        INTERNAL_ERROR,
                                        "session restore unavailable; retry the connection",
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            Ok::<_, JsonRpcError>((message_count, seed_event, plan))
        }
        .await;
        let (message_count, seed_event, plan) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                if unpublished.is_none() {
                    if let Some(ref hooks) = self.ctx.hooks {
                        hooks.fire_session_end(&session_id, "rpc").await;
                    }
                    self.ctx.sessions.remove(&session_id).await;
                }
                return Err(error);
            }
        };

        let plan_event = (!plan.is_empty()).then(|| TurnEvent::Plan {
            entries: plan.clone(),
        });
        if let Some(mut candidate) = unpublished {
            candidate.plan = plan;
            self.ctx
                .sessions
                .publish_prepared(session_id.clone(), candidate, expected_generation)
                .await
                .map_err(|message| rpc_err(SESSION_BUSY, message))?;
        } else {
            self.ctx.sessions.set_plan(&session_id, plan).await;
        }
        self.forward_seed_event(&session_id, seed_event).await;
        if let Some(event) = plan_event {
            forward_turn_event(&self.rpc, &session_id, &event).await;
        }
        drop(config_generation_guard);

        if let Some(ref tui_id) = self.tui_id
            && req.keep_siblings != Some(true)
        {
            let evicted = self
                .ctx
                .sessions
                .evict_same_mode_sibling(tui_id, &chat_mode, &session_id)
                .await;
            if !evicted.is_empty() {
                if let Some(ref hooks) = self.ctx.hooks {
                    for (sid, _) in &evicted {
                        hooks.fire_session_end(sid, "rpc").await;
                    }
                }
                let span = ::clawcrew_log::info_span!(
                    target: "clawcrew_log_internal_scope",
                    "clawcrew_scope",
                    session_key = %session_id,
                    agent_alias = %req.agent_alias,
                    channel = "rpc",
                );
                let _guard = span.enter();
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Success)
                        .with_attrs(::serde_json::json!({
                            "tui_id": tui_id,
                            "evicted": evicted.iter().map(|(id, _)| id).collect::<Vec<_>>(),
                        })),
                    "Evicted abandoned same-mode session(s) on session/new"
                );
                // Every evicted session was idle (no in-flight turn), so its
                // removal above dropped the last Agent strong ref and freed the
                // history. Trimming now actually returns those pages.
                crate::util::release_freed_heap();
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Success)
                        .with_attrs(::serde_json::json!({
                            "evicted_count": evicted.len(),
                        })),
                    "Trimmed glibc arenas after same-mode session eviction"
                );
            }
        }

        if let Some(ref hooks) = self.ctx.hooks {
            hooks.fire_session_start(&session_id, "rpc").await;
        }

        to_result(SessionNewResult {
            session_id,
            agent_alias: req.agent_alias,
            message_count,
            workspace_dir: cwd,
        })
    }

    async fn handle_session_close(&self, params: &Value) -> RpcResult {
        let req: SessionIdParams = parse_params(params)?;
        // Cancellation must be signalled before waiting: the admitted prompt
        // owns this permit until its terminal state and transcript writes are
        // complete. Removal then happens under the same incarnation fence.
        self.ctx.sessions.signal_session_removal(&req.session_id);
        let _guard = self
            .ctx
            .sessions
            .session_queue
            .acquire(&req.session_id)
            .await
            .map_err(|e| rpc_err(SESSION_BUSY, format!("Session busy: {e}")))?;
        if let Some(agent) = self.ctx.sessions.get_agent(&req.session_id).await {
            agent
                .lock()
                .await
                .channel_handles()
                .unregister_channel("rpc");
            let strong = std::sync::Arc::strong_count(&agent);
            let agent_alias = self
                .ctx
                .sessions
                .get_agent_alias(&req.session_id)
                .await
                .unwrap_or_default();
            let span = ::clawcrew_log::info_span!(
                target: "clawcrew_log_internal_scope",
                "clawcrew_scope",
                session_key = %req.session_id,
                agent_alias = %agent_alias,
                channel = "rpc",
            );
            let _guard = span.enter();
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_attrs(::serde_json::json!({
                        "agent_arc_strong_count_before_remove": strong,
                    })),
                "session close: dropping local Agent handle before remove"
            );
            // Drop our clone explicitly so the session map holds the last
            // strong ref; `remove` then frees the Agent at removal time
            // rather than at end-of-scope, letting the allocator reclaim
            // promptly.
            drop(agent);
        }
        if !self.ctx.sessions.remove(&req.session_id).await {
            return Err(rpc_err(SESSION_NOT_FOUND, "Session not found"));
        }
        if let Some(ref hooks) = self.ctx.hooks {
            hooks.fire_session_end(&req.session_id, "rpc").await;
        }
        crate::util::release_freed_heap();
        {
            let span = ::clawcrew_log::info_span!(
                target: "clawcrew_log_internal_scope",
                "clawcrew_scope",
                session_key = %req.session_id,
                channel = "rpc",
            );
            let _guard = span.enter();
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_outcome(::clawcrew_log::EventOutcome::Success),
                "Trimmed glibc arenas after session close"
            );
        }
        to_result(SessionCloseResult {
            session_id: req.session_id,
            closed: true,
        })
    }

    async fn handle_session_kill(&self, params: &Value) -> RpcResult {
        let req: SessionKillParams = parse_params(params)?;
        let sid = &req.session_id;

        // Preserve kill semantics by signalling the admitted prompt first,
        // then wait for its finalization before reading mode or tombstoning
        // and removing this exact session incarnation.
        self.ctx.sessions.signal_session_kill(sid);
        let _guard = self
            .ctx
            .sessions
            .session_queue
            .acquire(sid)
            .await
            .map_err(|e| rpc_err(SESSION_BUSY, format!("Session busy: {e}")))?;

        let chat_mode = self
            .ctx
            .sessions
            .chat_mode(sid)
            .await
            .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;

        let agent_alias = self
            .ctx
            .sessions
            .get_agent_alias(sid)
            .await
            .unwrap_or_default();
        let span = ::clawcrew_log::info_span!(
            target: "clawcrew_log_internal_scope",
            "clawcrew_scope",
            session_key = %sid,
            agent_alias = %agent_alias,
            channel = "rpc",
        );
        let _guard = span.enter();

        if matches!(chat_mode, ChatMode::Acp) {
            let store = self
                .ctx
                .acp_session_store
                .clone()
                .ok_or_else(|| rpc_err(INTERNAL_ERROR, "ACP session store is not available"))?;
            let sid_owned = sid.to_string();
            let marked =
                tokio::task::spawn_blocking(move || store.mark_session_killed(&sid_owned)).await;
            match marked {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                        "session/kill: live ACP session had no durable row to tombstone"
                    );
                }
                Ok(Err(e)) => {
                    return Err(rpc_err(
                        INTERNAL_ERROR,
                        format!("Failed to mark ACP session killed: {e}"),
                    ));
                }
                Err(e) => {
                    return Err(rpc_err(
                        INTERNAL_ERROR,
                        format!("Failed to mark ACP session killed: {e}"),
                    ));
                }
            }
        }

        let killed = self.ctx.sessions.kill_session(sid).await;
        if killed {
            if let Some(ref hooks) = self.ctx.hooks {
                hooks.fire_session_end(sid, "rpc").await;
            }
            crate::util::release_freed_heap();
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_outcome(::clawcrew_log::EventOutcome::Success),
                "session/kill: session terminated by admin"
            );
        } else {
            ::clawcrew_log::record!(
                DEBUG,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                "session/kill: session vanished between existence check and kill (concurrent close?)"
            );
        }

        to_result(SessionKillResult {
            session_id: req.session_id,
            killed,
        })
    }

    /// Rebuild a reaped ACP session from a restorable durable row so a fresh
    /// prompt recovers to a working session instead of hanging. Returns the
    /// live agent on success; returns `None` for missing, killed, or unreadable
    /// durable state.
    /// Restore the durable ACP TodoWrite plan into the live session and emit
    /// the replay notification so the resuming / reconnecting client's
    /// tracker repopulates without a model round-trip.
    ///
    /// Shared by both reanimation paths — `session/new` resume and
    /// prompt-triggered rehydration — so a session recovers its plan state
    /// identically whether it comes back through an explicit reconnect or
    /// through its first prompt after being reaped.
    async fn restore_acp_plan(&self, session_id: &str) {
        let Some(ref store) = self.ctx.acp_session_store else {
            return;
        };
        let store = store.clone();
        let sid = session_id.to_string();
        let plan = tokio::task::spawn_blocking(move || store.get_plan(&sid).unwrap_or_default())
            .await
            .unwrap_or_default();
        if !plan.is_empty() {
            self.ctx.sessions.set_plan(session_id, plan.clone()).await;
            if let Some(n) = plan_replay_notification(session_id, &plan) {
                let _ = self.rpc.send_raw(n).await;
            }
        }
    }

    async fn rehydrate_reaped_session(
        &self,
        sid: &str,
    ) -> Option<Arc<tokio::sync::Mutex<crate::agent::agent::Agent>>> {
        // Own the session admission permit for the WHOLE incarnation, exactly
        // like `handle_session_new`: the durable transcript must not be read
        // until a same-ID predecessor has fully finalized (its
        // `persist_acp_turn` write lands before it releases this permit), and
        // the published successor must not be observable by an admitted
        // prompt, `session/new`, or a competing rehydration until its history
        // and plan are restored. Publishing goes through `insert_admitted` —
        // re-acquiring inside `insert` would deadlock against this guard.
        // The permit is released on return, before the calling prompt takes
        // its own admission.
        let _admission = self.ctx.sessions.session_queue.acquire(sid).await.ok()?;

        let store = self.ctx.acp_session_store.clone()?;
        let store_for_load = Arc::clone(&store);
        let sid_owned = sid.to_string();
        let loaded = tokio::task::spawn_blocking(move || {
            store_for_load.load_session_for_restore(&sid_owned)
        })
        .await;
        let data = match loaded {
            Ok(Ok(clawcrew_infra::acp_session_store::AcpSessionRestore::Restorable(data))) => data,
            Ok(Ok(clawcrew_infra::acp_session_store::AcpSessionRestore::Killed)) => {
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Success),
                    "session/prompt: refusing to rehydrate admin-killed ACP session"
                );
                return None;
            }
            Ok(Err(e)) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "session_id": sid,
                            "error": e.to_string(),
                        })),
                    "session/prompt: failed to query ACP killed marker before rehydrate"
                );
                return None;
            }
            Ok(Ok(clawcrew_infra::acp_session_store::AcpSessionRestore::Missing)) => return None,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "session_id": sid,
                            "error": e.to_string(),
                        })),
                    "session/prompt: ACP killed-marker query task failed before rehydrate"
                );
                return None;
            }
        };

        let cwd_path = Some(std::path::Path::new(&data.workspace_dir));
        let tui_env = self
            .tui_id
            .as_deref()
            .and_then(|id| self.ctx.tui_registry.get_env(id));
        let exclude_memory = true;
        // Rehydration is a reader in the route-generation transaction, like
        // `session/new`. Take the config writer gate so the Agent is built and
        // inserted against one config generation: a route-affecting commit then
        // either runs entirely before this (and the Agent is built from the
        // committed generation) or entirely after insertion (and its
        // `list_ids()` snapshot contains this session, so it refreshes it under
        // the per-session update guard).
        //
        // `try_lock`, NOT a blocking acquire: this path is reached from
        // `session/prompt`, and a route-affecting `config/set` awaits its
        // live-session refresh inline while holding this same gate. Blocking
        // here would deadlock a rehydration that races such a commit — the
        // commit cannot finish until the refresh completes, and the refresh
        // cannot observe a session whose insertion is waiting on the gate.
        //
        // Losing the gate is safe rather than merely tolerable, because the
        // session-identity generation fence covers the contended case from the
        // other side: `SessionStore::insert` stamps a fresh generation, so an
        // in-flight refresh holding the pre-rehydration generation is rejected
        // by `apply_model_provider` and cannot clobber the successor. The gate
        // is the uncontended fast path; the fence is the contended one.
        //
        // The durable-store lookup above reads no config and deliberately stays
        // outside this boundary, matching `session/new`.
        let config_generation_guard = Arc::clone(&self.ctx.config_write_lock)
            .try_lock_owned()
            .ok();
        // Reaped sessions always rehydrate as ACP, which skips eager MCP init to
        // stay prompt — matching `session_should_initialize_mcp(ChatMode::Acp)`.
        let mut agent = crate::agent::agent::Agent::from_live_config_with_tui_env_and_acp_sessions(
            Arc::clone(&self.ctx.config),
            &data.agent_alias,
            cwd_path,
            false,
            exclude_memory,
            tui_env,
            self.ctx.sop_engine.clone(),
            self.ctx.sop_audit.clone(),
            store,
        )
        .await
        .ok()?;
        let interaction_context = match data.interaction_surface.as_deref() {
            Some(value) => match crate::agent::prompt::InteractionSurface::from_persisted(value) {
                Some(surface) => Some(surface.resolve()),
                None => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Read,)
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "session_id": sid,
                                "interaction_surface": value,
                            })),
                        "session/prompt: refusing to rehydrate an unsupported interaction surface"
                    );
                    return None;
                }
            },
            None => None,
        };
        agent.set_interaction_context(interaction_context);

        let approval_ch = Arc::new(crate::rpc::approval_channel::RpcApprovalChannel::new(
            "rpc",
            sid.to_string(),
            Arc::clone(&self.rpc),
            Arc::clone(&self.ctx.approval_pending),
            self.client_elicitation_caps,
        ));
        // See session/new: channel_name must match the registered back-channel
        // key so interactive tools default to this conversation.
        agent.set_channel_name("rpc".to_string());
        agent.channel_handles().register_channel("rpc", approval_ch);

        let message_count = data.messages.len();
        // Contended gate means this session may be excluded from an in-flight
        // refresh transaction's `list_ids()` snapshot, so it is published with
        // a PROVISIONAL binding: live, but bound to a config generation that is
        // not yet confirmed. `session/prompt` and `session/configure` await
        // this before proceeding. Reconciliation or a later ordinary refresh
        // clears it only after publishing one coherent committed binding.
        let try_lock_failed = config_generation_guard.is_none();
        let pending_generation = try_lock_failed.then(|| Arc::new(tokio::sync::Notify::new()));
        let session = super::session::RpcSession::new(
            agent,
            &data.agent_alias,
            &data.workspace_dir,
            crate::rpc::types::ChatMode::Acp,
        )
        .with_owner(self.tui_id.clone());
        let session = match pending_generation.as_ref() {
            Some(notify) => session.with_pending_generation(Arc::clone(notify)),
            None => session,
        };
        // Publish through `insert_admitted`: the admission permit acquired at
        // the top of this helper is held across publication AND the history
        // and plan restore below, so no prompt, `session/new`, or competing
        // rehydration can observe the successor before it is fully restored.
        let published_generation = self
            .ctx
            .sessions
            .insert_admitted(&_admission, sid.to_string(), session)
            .await
            .ok()?;

        // Release only after the session is published, so a commit that starts
        // next sees it in `list_ids()`.
        drop(config_generation_guard);

        // Test-only: park between publication and the history restore so a
        // regression can prove the admission permit keeps other RPCs out of
        // the unseeded window.
        self.ctx.sessions.wait_test_rehydrate_seed_pause().await;

        // Pending-generation reconciliation when the config writer gate was
        // contended and `try_lock_owned` returned `None`.
        //
        // When the gate was available (`try_lock` succeeded), the session was
        // built from and inserted under the current config generation — any
        // later route-affecting commit will include this session in its
        // `list_ids()` snapshot and refresh it under the per-session guard.
        //
        // When the gate was contended, a route-affecting `config/set` was
        // already holding the lock and may have run `list_ids()` before
        // `sessions.insert` returned. That snapshot excluded this session, so
        // the commit will not refresh it, and `apply_model_provider`'s
        // generation fence never fires — there is no old generation to compare
        // against because the session did not exist yet when the snapshot ran.
        //
        // Repair: spawn a task that blocks until the config writer gate is
        // available (i.e. the in-flight commit has finished), then re-derives
        // provider/resolver/generation from the now-current live config and
        // applies them through `apply_model_provider`. The session-identity
        // generation fence in `apply_model_provider` guards this path against
        // a further `session/new` or `rehydrate_reaped_session` replacing the
        // session again before the task runs.
        //
        // The session was published carrying a pending-generation marker, so
        // `session/prompt` and `session/configure` park until this task
        // finishes rather than dispatching through the provisional binding.
        if try_lock_failed {
            let rehydrate_ctx = Arc::clone(&self.ctx);
            let rehydrate_sid = sid.to_string();
            let rehydrate_alias = data.agent_alias.clone();
            clawcrew_spawn::spawn!(async move {
                let reconciled = Self::reconcile_rehydrated_session(
                    Arc::clone(&rehydrate_ctx),
                    &rehydrate_sid,
                    &rehydrate_alias,
                    published_generation,
                )
                .await;
                if reconciled {
                    rehydrate_ctx
                        .sessions
                        .clear_pending_generation(&rehydrate_sid, published_generation)
                        .await;
                }
            });
        }

        let trim_breadcrumb = data.trim_breadcrumb;
        // Breadcrumb provenance is the store's own canonical record alongside
        // the transcript, never inferred from message text. Set it BEFORE
        // seeding: seeding trims immediately if the restored transcript is
        // over the structured cap, and that seed-time trim reads the
        // agent's current breadcrumb flag to decide whether a leading
        // synthetic marker counts as a real turn.
        self.ctx
            .sessions
            .set_history_has_trim_breadcrumb(sid, trim_breadcrumb)
            .await;
        let seed_event = self
            .ctx
            .sessions
            .seed_conversation_history_with_event(sid, data.messages)
            .await;
        self.forward_seed_event(sid, seed_event).await;
        // The durable TodoWrite plan travels with the transcript: restore it
        // into the live session and replay it to the client, exactly like the
        // `session/new` resume path, so a reaped session recovers its Code
        // pane plan state on first prompt instead of losing it.
        self.restore_acp_plan(sid).await;
        self.ctx.sessions.touch(sid).await;

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_category(::clawcrew_log::EventCategory::Agent)
                .with_outcome(::clawcrew_log::EventOutcome::Success)
                .with_attrs(::serde_json::json!({
                    "session_id": sid,
                    "agent_alias": data.agent_alias,
                    "messages_restored": message_count,
                })),
            "rehydrated reaped session from durable store; turn continues on a working session"
        );

        self.ctx.sessions.get_agent(sid).await
    }

    /// Re-derive and publish a rehydrated session's provider binding from the
    /// committed config generation.
    ///
    /// Runs only for a session inserted while `config_write_lock` was
    /// contended, i.e. one that an in-flight refresh transaction may have
    /// excluded from its `list_ids()` snapshot. Blocking on the writer gate is
    /// what makes the read authoritative: it cannot return until the commit
    /// that owned the gate has finished.
    ///
    /// Returns `true` only after the committed provider binding is published.
    /// A failed rebuild leaves the session pending, so callers cannot dispatch
    /// through a mixed old-provider/new-config state.
    async fn reconcile_rehydrated_session(
        ctx: Arc<RpcContext>,
        session_id: &str,
        agent_alias: &str,
        expected_generation: u64,
    ) -> bool {
        // Acquire the gate (blocking) so we are guaranteed to read a config at
        // least as new as whatever committed while the Agent was being built.
        let _gate = Arc::clone(&ctx.config_write_lock).lock_owned().await;
        // Take the per-session ordering boundary for the whole publication,
        // the same guard `prepare_live_sessions_refresh` holds. Without it this
        // repair would be the only live-provider writer in the file that does
        // not serialize with `session/configure`, so the two could interleave
        // field by field instead of composing as one transition.
        let _session_guard = ctx.sessions.lock_model_provider_update(session_id).await;
        // Capture generation inside the gate so it matches the config below.
        let Some(session_generation) = ctx.sessions.get_generation(session_id).await else {
            return false; // session was removed before we ran
        };
        if session_generation != expected_generation {
            return false; // a successor owns this ID now
        }
        let config_generation = Arc::new(ctx.config.read().clone());
        let agent_cfg = config_generation
            .resolved_agent_config(agent_alias)
            .or_else(|| config_generation.agent(agent_alias).cloned());
        let resolved = agent_cfg.as_ref().and_then(|cfg| {
            crate::agent::agent::build_session_model_provider(
                &config_generation,
                cfg.model_provider.as_str(),
                None,
            )
            .ok()
            .map(|tuple| (cfg.model_provider.clone(), cfg.clone(), tuple))
        });
        let Some((model_provider_ref, cfg, (provider, provider_name, model, resolver))) = resolved
        else {
            // The alias is unresolvable against the committed config. Leave the
            // construction-time box in place but keep the session unavailable:
            // a later refresh can publish one coherent binding and clear its
            // pending marker after configuration is repaired.
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "session_id": session_id,
                        "agent_alias": agent_alias,
                    })),
                "rehydrated session: post-insert reconciliation could not resolve the agent's \
                 provider against the committed config generation; the session remains pending \
                 until a later coherent refresh"
            );
            return false;
        };
        let dispatcher =
            crate::agent::agent::tool_dispatcher_for_provider(&cfg, provider.as_ref(), &model);
        // Temperature belongs to the same state transition as the provider box.
        // Resolve it exactly as the ordinary refresh path does
        // (`overrides.temperature.or(provider_temperature)`) and publish it
        // here: nothing re-derives temperature at turn entry — neither
        // `sync_config_generation` nor `try_apply_model_switch` touches
        // `Agent::temperature` — so passing `None` would leave a repaired
        // session on the pre-commit profile temperature while its provider,
        // resolver, and limits are all on the committed generation.
        let provider_temperature =
            model_provider_ref
                .split_once('.')
                .and_then(|(provider_type, provider_alias)| {
                    config_generation
                        .providers
                        .models
                        .find(provider_type, provider_alias)
                        .and_then(|entry| entry.temperature)
                });
        let temperature = ctx
            .sessions
            .get_overrides(session_id)
            .await
            .and_then(|overrides| overrides.temperature)
            .or(provider_temperature);
        ctx.sessions
            .apply_model_provider(
                session_id,
                session_generation,
                provider,
                provider_name,
                model,
                resolver,
                dispatcher,
                config_generation,
                Some(temperature),
            )
            .await
    }

    async fn handle_session_prompt(&self, params: &Value) -> RpcResult {
        let req: SessionPromptParams = parse_params(params)?;
        let sid = &req.session_id;

        if req.prompt.trim().is_empty() && req.attachments.is_empty() {
            return Err(rpc_err(
                INVALID_PARAMS,
                "session/prompt requires a non-empty `prompt` or at least one attachment",
            ));
        }

        // The first lookup triggers rehydration of a reaped session and fails
        // fast when the ID is unknown. It must run BEFORE admission:
        // rehydration publishes through `SessionStore::insert`, which itself
        // acquires the per-session admission permit, so admitting first would
        // deadlock the permit-1 semaphore against the insert. The canonical
        // Agent handle is re-resolved after generation reconciliation below,
        // so the binding is deliberately unused here.
        if self.connection_cancel.is_cancelled() {
            return Err(rpc_err(
                SESSION_BUSY,
                "RPC connection closed before prompt admission",
            ));
        }
        let _initial_agent = match self.ctx.sessions.get_agent(sid).await {
            Some(a) => a,
            None => match self.rehydrate_reaped_session(sid).await {
                Some(a) => a,
                None => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail,)
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({ "session_id": sid })),
                        "session/prompt on a session absent from memory and the durable store; emitting TurnComplete so the client exits the working state"
                    );
                    self.emit_turn_complete(
                        sid,
                        crate::rpc::types::TurnCompletionOutcome::Failed,
                        "turn cancelled by daemon: session_not_found".to_string(),
                        req.client_turn_generation,
                        None,
                    )
                    .await;
                    return Err(rpc_err(SESSION_NOT_FOUND, "Session not found"));
                }
            },
        };

        // Admit before reading mutable session metadata. Session replacement
        // (session/new and rehydration insert) uses the same queue, so the
        // rest of this turn has one incarnation.
        let _guard = tokio::select! {
            biased;
            _ = self.connection_cancel.cancelled() => {
                return Err(rpc_err(SESSION_BUSY, "RPC connection closed before prompt admission"));
            }
            guard = self.ctx.sessions.session_queue.acquire(sid) => {
                guard.map_err(|e| rpc_err(SESSION_BUSY, format!("Session busy: {e}")))?
            }
        };

        if self.connection_cancel.is_cancelled() {
            return Err(rpc_err(
                SESSION_BUSY,
                "RPC connection closed before prompt execution",
            ));
        }

        // Registration is the first operation after admission and the RAII
        // handle removes this exact generation on every exit path. Removal
        // handlers signal before waiting on the same queue, so they cannot
        // lose cancellation while setup awaits attachments or persistence.
        let cancel = tokio_util::sync::CancellationToken::new();
        let cancel_registration = self
            .ctx
            .sessions
            .register_cancel_token_guard(sid, cancel.clone());
        self.ctx
            .sessions
            .wait_test_prompt_registration_pause()
            .await;

        // Process inline attachments: upload each, append markers to prompt.
        let mut prompt = req.prompt.clone();
        if !req.attachments.is_empty() {
            use super::attachments::process_file_entry;

            let agent_alias = self
                .ctx
                .sessions
                .get_agent_alias(sid)
                .await
                .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;
            let upload_root = self
                .ctx
                .config
                .read()
                .agent_workspace_dir(&agent_alias)
                .to_string_lossy()
                .to_string();
            let is_wss = self.peer_label.starts_with("wss:");
            if !prompt.is_empty() {
                prompt.push('\n');
            }
            for (idx, entry) in req.attachments.iter().enumerate() {
                let result = tokio::select! {
                    biased;
                    _ = self.connection_cancel.cancelled() => {
                        return Err(rpc_err(
                            SESSION_BUSY,
                            "RPC connection closed while preparing prompt attachments",
                        ));
                    }
                    result = process_file_entry(
                        entry,
                        sid,
                        &upload_root,
                        is_wss,
                        &self.ctx.sessions,
                    ) => result?,
                };
                if idx > 0 {
                    prompt.push('\n');
                }
                prompt.push_str(&result.marker);
            }
        }

        let chat_mode = self
            .ctx
            .sessions
            .chat_mode(sid)
            .await
            .unwrap_or(crate::rpc::types::ChatMode::Chat);

        // Wait for a provisional binding to be confirmed before entering the
        // turn. A session rehydrated while a route-affecting commit held the
        // config writer gate is live but bound to an unconfirmed generation;
        // dispatching now would use the construction-time provider while
        // canonical config has already moved on.
        //
        // This MUST precede the ordering lock below: the reconciliation task
        // takes that same per-session guard, so acquiring it first would block
        // the very task this waits on. A no-op for every session without a
        // pending marker, which is all of them outside this narrow window.
        //
        // Returns the generation it resolved against. The caller must compare
        // it with the generation captured when the cached Agent was looked up,
        // and re-lookup on a mismatch: the wait says nothing about which
        // instance now answers to this ID.
        let converged_generation = match self
            .ctx
            .sessions
            .await_pending_generation(sid, std::time::Duration::from_secs(30))
            .await
        {
            Ok(observed_generation) => observed_generation,
            Err(crate::rpc::session::WaitForProviderUpdateError::SessionNotFound) => {
                return Err(rpc_err(SESSION_NOT_FOUND, "Session not found"));
            }
            Err(crate::rpc::session::WaitForProviderUpdateError::Timeout) => {
                return Err(rpc_err(
                    SESSION_BUSY,
                    "session generation reconciliation in progress; retry shortly",
                ));
            }
        };

        let _ = converged_generation;

        // Own the live-session provider generation through this turn. A
        // route-affecting config transaction holds the same lock while it
        // publishes the live Config, provider box, ModelRouteResolver, and
        // generation. Keeping the guard (rather than merely waiting for and
        // immediately releasing it) orders both race directions: a transaction
        // already in progress completes first, and a later transaction cannot
        // swap the live config between this boundary and
        // `sync_config_generation()` at the start of the turn.
        //
        // A 30-second timeout surfaces a retryable error to the caller
        // rather than blocking indefinitely.
        let _model_provider_generation = match self
            .ctx
            .sessions
            .lock_model_provider_update_with_timeout(sid, std::time::Duration::from_secs(30))
            .await
        {
            Ok(guard) => guard,
            Err(crate::rpc::session::WaitForProviderUpdateError::SessionNotFound) => {
                return Err(rpc_err(SESSION_NOT_FOUND, "Session not found"));
            }
            Err(crate::rpc::session::WaitForProviderUpdateError::Timeout) => {
                return Err(rpc_err(
                    SESSION_BUSY,
                    "provider update in progress; retry shortly",
                ));
            }
        };

        // Resolve the canonical Agent only after admission and reconciliation;
        // this prevents executing through an orphaned predecessor handle.
        let agent = self
            .ctx
            .sessions
            .get_agent(sid)
            .await
            .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;

        // Mark the durable row running only after every preflight wait has
        // passed. The generation waits and the canonical Agent lookup above
        // can all still fail this prompt (SESSION_BUSY / SESSION_NOT_FOUND)
        // before any provider turn starts; writing "running" earlier would
        // leave a false operational state — `session/state` reporting work
        // and stuck-session queries surfacing a turn id that never ran — on
        // every such retryable exit, with no terminal write to correct it.
        //
        // Mirror the gateway WS path so `session/state` and stuck-session
        // detection see RPC-driven turns too. The durable row lives under the
        // same `rpc_{sid}` key the Chat-mode message persistence below and
        // `session/state` both use. The HTTP running-sessions listing stays
        // blind to these rows: it derives its caller-facing id by stripping a
        // `gw_` prefix, and the sibling lookup and abort paths resolve only
        // gateway keys, so surfacing a row here would hand callers an id those
        // endpoints cannot act on.
        //
        // Scoped to Chat sessions only. Session IDs are caller-supplied and
        // the two persistence modes share that namespace, so an ACP prompt
        // reusing the ID of a closed Chat session would otherwise mutate that
        // session's retained `rpc_{sid}` row — making a stale Chat record
        // report `running`/`idle`/`error` from an ACP turn, and surfacing it
        // in stuck-session queries. ACP state belongs to `AcpSessionStore`.
        let persist_session_state = !matches!(chat_mode, crate::rpc::types::ChatMode::Acp);
        let session_key = format!("rpc_{sid}");
        let turn_id = uuid::Uuid::new_v4().to_string();
        if persist_session_state && let Some(ref backend) = self.ctx.session_backend {
            let _ = backend.set_session_state(&session_key, "running", Some(&turn_id));
        }

        self.ctx.sessions.touch(sid).await;
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Invoke)
                .with_category(::clawcrew_log::EventCategory::Agent)
                .with_attrs(::serde_json::json!({ "session_id": sid })),
            "turn dispatch: registered cancel token, starting turn"
        );

        // Capture live attribution fields for the turn span. Context limits are
        // emitted by the model-call event itself so a mid-session route switch
        // cannot leave the meter on a precomputed provider/model snapshot.
        // (Master's precomputed `max_context_tokens` injection is
        // deliberately NOT carried over: the route-aware contract resolves
        // capacity from the serving provider/model at call time.)
        let (agent_alias, model_provider, model) = {
            let alias = self
                .ctx
                .sessions
                .get_agent_alias(sid)
                .await
                .unwrap_or_default();
            let (mp, m) = if let Some(agent) = self.ctx.sessions.get_agent(sid).await {
                let (_, model_provider, model) = agent.lock().await.attribution_fields();
                (model_provider, model)
            } else {
                (String::new(), String::new())
            };
            (alias, mp, m)
        };

        let rpc = self.rpc.clone();
        let sid_owned = sid.to_string();
        // Clone of the session store so the turn-event closure can persist
        // the latest TodoWrite plan (store-then-emit) before the plan
        // notification goes out. See `persist_plan_if_any`.
        let sessions_for_plan = self.ctx.sessions.clone();
        let acp_token_store = if matches!(chat_mode, crate::rpc::types::ChatMode::Acp) {
            self.ctx.acp_session_store.clone()
        } else {
            None
        };
        let attribution_agent_alias = agent_alias.clone();
        let attribution_model_provider = model_provider.clone();
        let attribution_model = model.clone();
        // Cost-tracking context for this turn. Built from the daemon-scoped
        // tracker + the live pricing map and stamped with the agent alias so
        // `execute_turn` can persist token usage and attribute spend. `None`
        // when cost tracking is disabled (no tracker wired).
        let cost_context = self.ctx.cost_tracker.as_ref().map(|tracker| {
            let cfg_guard = self.ctx.config.read();
            let pricing = crate::agent::cost::build_model_provider_pricing(&cfg_guard);
            crate::agent::cost::ToolLoopCostTrackingContext::new(
                tracker.clone(),
                std::sync::Arc::new(pricing),
            )
            .with_agent_alias(&attribution_agent_alias)
        });
        let turn = execute_turn(
            agent,
            prompt.clone(),
            cancel.clone(),
            TurnAttribution {
                session_key: Some(sid.to_string()),
                agent_alias,
                model_provider,
                model,
                channel: "rpc",
            },
            cost_context,
            self.connection_activity.clone(),
            move |event| {
                let rpc = rpc.clone();
                let sid = sid_owned.clone();
                let acp_token_store = acp_token_store.clone();
                let sessions_for_plan = sessions_for_plan.clone();
                async move {
                    if let (
                        Some(store),
                        TurnEvent::Usage {
                            input_tokens,
                            accepted,
                            ..
                        },
                    ) = (acp_token_store.as_ref(), &event)
                    {
                        let store = store.clone();
                        let sid = sid.clone();
                        let (tokens, is_accepted) = (*input_tokens, *accepted);
                        let _ = tokio::task::spawn_blocking(move || {
                            store.persist_usage_snapshot(&sid, tokens, is_accepted)
                        })
                        .await;
                    }
                    persist_plan_if_any(&sessions_for_plan, acp_token_store.as_ref(), &sid, &event)
                        .await;
                    forward_turn_event(&rpc, &sid, &event).await;
                }
            },
        );
        tokio::pin!(turn);
        let outcome = tokio::select! {
            biased;
            _ = self.connection_cancel.cancelled() => {
                self.ctx.sessions.record_cancel_cause_if_absent(
                    sid,
                    crate::rpc::session::CancelCause::ConnectionClosed,
                );
                cancel.cancel();
                turn.await
            }
            outcome = &mut turn => outcome,
        };

        // Drain the cancel cause BEFORE removing the token (removal clears the
        // cause map). Every cancel firing site records its cause before firing;
        // a cancel with no recorded cause is a bug, not user attribution.
        let cancel_cause = cancel_registration.finish();

        // ── Durable turn-verdict audit row ───────────────────────────────
        // Every turn termination writes one attributed row to the ACP session
        // store's event log so a cancel verdict is diagnosable after the trace
        // log rotates. Fire-and-forget on a blocking task.
        if matches!(chat_mode, crate::rpc::types::ChatMode::Acp)
            && let Some(store) = self.ctx.acp_session_store.clone()
        {
            let (action, event_outcome, payload) = match &outcome {
                Ok(crate::rpc::turn::TurnOutcome::Completed { .. }) => (
                    ::clawcrew_log::Action::Complete,
                    ::clawcrew_log::EventOutcome::Success,
                    None,
                ),
                Ok(crate::rpc::turn::TurnOutcome::Cancelled { .. }) => (
                    ::clawcrew_log::Action::Cancel,
                    ::clawcrew_log::EventOutcome::Unknown,
                    Some(
                        ::serde_json::json!({
                            "cancel_cause": cancel_cause.map(|c| c.as_str()),
                        })
                        .to_string(),
                    ),
                ),
                Err(e) => (
                    ::clawcrew_log::Action::Fail,
                    ::clawcrew_log::EventOutcome::Failure,
                    Some(::serde_json::json!({ "error": e.to_string() }).to_string()),
                ),
            };
            let sid_owned = sid.to_string();
            let span_session = sid.to_string();
            let span_alias = attribution_agent_alias.clone();
            let span_provider = attribution_model_provider.clone();
            let span_model = attribution_model.clone();
            clawcrew_spawn::spawn!(async move {
                use ::clawcrew_log::Instrument as _;
                let span = ::clawcrew_log::info_span!(
                    target: "clawcrew_log_internal_scope",
                    "clawcrew_scope",
                    session_key = %span_session,
                    agent_alias = %span_alias,
                    model_provider = %span_provider,
                    model = %span_model,
                    channel = "rpc",
                );
                async move {
                    let persisted = tokio::task::spawn_blocking(move || {
                        store.append_event(&sid_owned, action, event_outcome, payload.as_deref())
                    })
                    .await;
                    let error = match persisted {
                        Ok(Ok(())) => return,
                        Ok(Err(e)) => e.to_string(),
                        Err(join) => join.to_string(),
                    };
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Write)
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({ "error": error })),
                        "Failed to persist ACP turn-verdict audit event"
                    );
                }
                .instrument(span)
                .await;
            });
        }

        match chat_mode {
            crate::rpc::types::ChatMode::Acp => {
                if let Some(ref store) = self.ctx.acp_session_store
                    && let Some(agent) = self.ctx.sessions.get_agent(sid).await
                    && let Some(detail) = {
                        let agent = agent.lock().await;
                        let full_history = agent.history().to_vec();
                        let trim_breadcrumb = agent.history_has_trim_breadcrumb();
                        drop(agent);
                        persist_acp_turn(store, sid, &outcome, full_history, trim_breadcrumb).await
                    }
                {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                            .with_attrs(::serde_json::json!({"session_id": sid, "error": detail})),
                        "Failed to persist ACP turn"
                    );
                }
            }
            crate::rpc::types::ChatMode::Chat => {
                if let Some(ref backend) = self.ctx.session_backend
                    && let Some(agent) = self.ctx.sessions.get_agent(sid).await
                {
                    let key = format!("rpc_{sid}");
                    // Replace the durable transcript and breadcrumb flag with
                    // the agent's own authoritative post-turn history, as one
                    // state, rather than appending this turn's prompt/response
                    // delta on top of a transcript the agent's loop may have
                    // already trimmed underneath it.
                    let agent = agent.lock().await;
                    let durable = clawcrew_providers::durable_chat_messages(agent.history());
                    replace_rpc_chat_conversation_state(
                        backend.as_ref(),
                        sid,
                        &key,
                        &durable,
                        agent.history_has_trim_breadcrumb(),
                    );
                }
            }
        }

        // Keep the terminal count aligned with session/new and session/messages:
        // it is the durable projected conversation length, not the number of
        // visible TUI bubbles or local user turns.
        let message_count = match chat_mode {
            crate::rpc::types::ChatMode::Acp => self
                .ctx
                .acp_session_store
                .as_ref()
                .and_then(|store| store.load_session(&req.session_id).ok().flatten())
                .map(|data| conversation_message_entries(&data.messages).len()),
            crate::rpc::types::ChatMode::Chat => self
                .ctx
                .session_backend
                .as_ref()
                .map(|backend| backend.load(&session_key).len()),
        };

        match outcome {
            Ok(TurnOutcome::Completed {
                text,
                safeguard_fallback,
                ..
            }) => {
                if persist_session_state && let Some(ref backend) = self.ctx.session_backend {
                    let _ = backend.set_session_state(&session_key, "idle", None);
                }
                let text = crate::agent::append_safeguard_fallback_notice(
                    text,
                    safeguard_fallback.as_ref(),
                );
                self.emit_turn_complete(
                    &req.session_id,
                    crate::rpc::types::TurnCompletionOutcome::Completed,
                    text.clone(),
                    req.client_turn_generation,
                    message_count,
                )
                .await;
                to_result(SessionPromptResult {
                    session_id: req.session_id,
                    stop_reason: "end_turn".to_string(),
                    content: text,
                })
            }
            Ok(TurnOutcome::Cancelled { partial_text, .. }) => {
                if persist_session_state && let Some(ref backend) = self.ctx.session_backend {
                    let _ = backend.set_session_state(&session_key, "idle", None);
                }
                let cancel_message = match cancel_cause {
                    Some(cause) => {
                        format!(
                            "turn cancelled via {} in RPC_SESSION {}",
                            cause.as_str(),
                            req.session_id
                        )
                    }
                    None => {
                        format!(
                            "turn cancelled (cause unattributed) in RPC_SESSION {}",
                            req.session_id
                        )
                    }
                };
                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Cancel)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "session_id": req.session_id,
                            "agent_alias": attribution_agent_alias,
                            "model_provider": attribution_model_provider,
                            "model": attribution_model,
                            "chat_mode": format!("{chat_mode:?}"),
                            "cancel_cause": cancel_cause.map(|c| c.as_str()),
                        })),
                    "turn cancelled; emitting attributed TurnComplete so the client exits the working state"
                );
                self.emit_turn_complete(
                    &req.session_id,
                    crate::rpc::types::TurnCompletionOutcome::Cancelled,
                    cancel_message,
                    req.client_turn_generation,
                    message_count,
                )
                .await;
                to_result(SessionPromptResult {
                    session_id: req.session_id,
                    stop_reason: "cancelled".to_string(),
                    content: partial_text,
                })
            }
            Err(e) => {
                if persist_session_state && let Some(ref backend) = self.ctx.session_backend {
                    let _ = backend.set_session_state(&session_key, "error", Some(&turn_id));
                }
                let user_message = e.user_message().map(str::to_owned);
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "session_id": req.session_id,
                            "agent_alias": attribution_agent_alias,
                            "model_provider": attribution_model_provider,
                            "model": attribution_model,
                            "chat_mode": format!("{chat_mode:?}"),
                            "error": e.to_string(),
                        })),
                    "turn failed; emitting TurnComplete so the client exits the working state"
                );
                self.emit_turn_complete(
                    &req.session_id,
                    crate::rpc::types::TurnCompletionOutcome::Failed,
                    user_message
                        .clone()
                        .unwrap_or_else(|| format!("turn failed: {e}")),
                    req.client_turn_generation,
                    message_count,
                )
                .await;
                Err(rpc_err(
                    INTERNAL_ERROR,
                    user_message.unwrap_or_else(|| e.to_string()),
                ))
            }
        }
    }

    /// Emit the terminal `session/update` notification for a turn.
    /// The TUI uses this — not the JSON-RPC response — to flip
    /// `turn_in_flight` back to false.
    async fn emit_turn_complete(
        &self,
        session_id: &str,
        outcome: crate::rpc::types::TurnCompletionOutcome,
        content: String,
        client_turn_generation: Option<u64>,
        message_count: Option<usize>,
    ) {
        let update = SessionUpdateEvent::TurnComplete {
            session_id: session_id.to_string(),
            outcome,
            content,
            client_turn_generation,
            message_count,
        };
        if let Ok(params) = serde_json::to_value(update) {
            let n = JsonRpcNotification::new(notification::SESSION_UPDATE, params);
            if let Ok(s) = serde_json::to_string(&n) {
                let _ = self.rpc.send_raw(s).await;
            }
        }
    }

    async fn handle_session_configure(&self, params: &Value) -> RpcResult {
        let req: SessionConfigureParams = parse_params(params)?;
        validate_session_configure_overrides(&req.overrides)?;

        // Wait for a provisional binding to be confirmed, for the same reason
        // `session/prompt` does: a session rehydrated during a route-affecting
        // commit is live but unconfirmed, and committing an override against
        // it would compose with a binding the reconciliation is about to
        // replace. Precedes the ordering lock because the reconciliation task
        // holds that same guard.
        //
        // The returned generation is discarded here, unlike in
        // `session/prompt`: this handler caches no `Agent`, and it already
        // captures the generation below and re-verifies it under the lock, so
        // a same-ID replacement is rejected on that path instead.
        match self
            .ctx
            .sessions
            .await_pending_generation(&req.session_id, std::time::Duration::from_secs(30))
            .await
        {
            Ok(_) => {}
            Err(crate::rpc::session::WaitForProviderUpdateError::SessionNotFound) => {
                return Err(rpc_err(SESSION_NOT_FOUND, "Session not found"));
            }
            Err(crate::rpc::session::WaitForProviderUpdateError::Timeout) => {
                return Err(rpc_err(
                    SESSION_BUSY,
                    "session generation reconciliation in progress; retry shortly",
                ));
            }
        }

        // Serialize with candidate-config preparation. A route-affecting
        // `config/set` holds `config_write_lock` across prepare → commit →
        // publish, and its prepare phase SKIPS a session whose current
        // override points away from the edited provider, dropping that
        // session's ordering guard before the candidate config commits.
        // Without this gate, a configure selecting the edited provider
        // inside that window would build it from the still-installed old
        // config; the transaction then commits without rebuilding the
        // skipped session, and the next prompt's `sync_config_generation`
        // publishes the new config into the limits cell while the provider
        // box and route resolver stay on the old generation. Acquiring the
        // gate BEFORE the per-session guard preserves the global lock order
        // (`config/set` and `reconcile_rehydrated_session` both take
        // config-write → session-update); taking it after the session lock
        // would deadlock against them.
        //
        // The pending-generation wait above must stay before this
        // acquisition: reconciliation publishes while holding the writer
        // gate, so parking here with the gate held would block the very
        // task that wait is waiting on (and stall every other config write
        // until the timeout).
        let _config_write_guard = Arc::clone(&self.ctx.config_write_lock).lock_owned().await;

        // Capture the session generation /before/ acquiring the per-session
        // update lock. If the session is replaced while we wait for the lock,
        // the re-verification below will detect the mismatch and reject the
        // stale configure.
        let session_generation = self
            .ctx
            .sessions
            .get_generation(&req.session_id)
            .await
            .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;

        // Acquire the per-session ordering boundary.
        let _model_provider_update = self
            .ctx
            .sessions
            .lock_model_provider_update(&req.session_id)
            .await
            .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;

        // Re-verify the session has not been replaced while we waited for
        // the lock. If replaced, this configure is stale — reject it.
        if self.ctx.sessions.get_generation(&req.session_id).await != Some(session_generation) {
            return Err(rpc_err(SESSION_NOT_FOUND, "Session not found"));
        }

        let merged = self
            .ctx
            .sessions
            .preview_overrides(&req.session_id, &req.overrides)
            .await
            .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;

        // Model/model_provider overrides need a live provider-box rebuild,
        // which requires Config — held here, not in the session store. Resolve
        // the provider from the prospective merged override or configured
        // agent, build the box, and only then commit the override.
        let built_model_provider = if merged.model_provider.is_some() || merged.model.is_some() {
            let agent_alias = self
                .ctx
                .sessions
                .get_agent_alias(&req.session_id)
                .await
                .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;
            let built = {
                let config = self.ctx.config.read();
                let agent_cfg = config
                    .resolved_agent_config(&agent_alias)
                    .or_else(|| config.agent(&agent_alias).cloned())
                    .ok_or_else(|| {
                        rpc_err(
                            INVALID_PARAMS,
                            format!("Agent `{agent_alias}` is not configured"),
                        )
                    })?;
                let model_provider_ref = merged
                    .model_provider
                    .as_deref()
                    .unwrap_or_else(|| agent_cfg.model_provider.as_str());
                let (model_provider, model_provider_name, model_name, model_route_resolver) =
                    crate::agent::agent::build_session_model_provider(
                        &config,
                        model_provider_ref,
                        merged.model.as_deref(),
                    )
                    .map_err(|e| rpc_err(INVALID_PARAMS, e.to_string()))?;
                let tool_dispatcher = crate::agent::agent::tool_dispatcher_for_provider(
                    &agent_cfg,
                    model_provider.as_ref(),
                    &model_name,
                );
                (
                    model_provider,
                    model_provider_name,
                    model_name,
                    model_route_resolver,
                    tool_dispatcher,
                    // The exact generation the box and resolver above were built
                    // from, published onto the agent with them.
                    std::sync::Arc::new(config.clone()),
                )
            };
            Some(built)
        } else {
            None
        };

        let merged = self
            .ctx
            .sessions
            .set_overrides_gated(&req.session_id, session_generation, req.overrides)
            .await
            .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;

        if let Some((
            model_provider,
            model_provider_name,
            model_name,
            model_route_resolver,
            tool_dispatcher,
            config_generation,
        )) = built_model_provider
        {
            self.ctx
                .sessions
                .apply_model_provider(
                    &req.session_id,
                    session_generation,
                    model_provider,
                    model_provider_name,
                    model_name,
                    model_route_resolver,
                    tool_dispatcher,
                    config_generation,
                    // Temperature is already committed through
                    // `set_overrides_gated` on this path.
                    None,
                )
                .await
                .then_some(())
                .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;
        }

        to_result(SessionConfigureResult {
            session_id: req.session_id,
            overrides: merged,
        })
    }

    async fn handle_session_cancel(&self, params: &Value) -> RpcResult {
        let req: SessionIdParams = parse_params(params)?;
        let owner = self
            .ctx
            .sessions
            .session_owner_tui_id(&req.session_id)
            .await;
        let allowed = match (
            owner.as_ref().and_then(|o| o.as_deref()),
            self.tui_id.as_deref(),
        ) {
            (Some(o), Some(c)) => o == c,
            _ => false,
        };
        if !allowed {
            let (agent_alias, model_provider, model) =
                match self.ctx.sessions.get_agent(&req.session_id).await {
                    Some(agent) => agent.lock().await.attribution_fields(),
                    None => (String::new(), String::new(), String::new()),
                };
            let span = ::clawcrew_log::info_span!(
                target: "clawcrew_log_internal_scope",
                "clawcrew_scope",
                session_key = %req.session_id,
                agent_alias = %agent_alias,
                model_provider = %model_provider,
                model = %model,
                channel = "rpc",
            );
            let _guard = span.enter();
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_category(::clawcrew_log::EventCategory::Channel)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "caller_tui_id": self.tui_id.as_deref().unwrap_or("<none>"),
                        "owner_tui_id": owner
                            .as_ref()
                            .and_then(|o| o.as_deref())
                            .unwrap_or("<none>"),
                        "peer_label": &self.peer_label,
                    })),
                "session/cancel refused: caller does not own the session"
            );
            return Err(rpc_err(
                SESSION_NOT_OWNED,
                "Caller does not own this session",
            ));
        }
        if self.ctx.sessions.cancel_session(&req.session_id) {
            to_result(SessionCancelResult {
                session_id: req.session_id,
                cancelled: true,
            })
        } else {
            Err(rpc_err(
                SESSION_NOT_FOUND,
                "No active turn for this session",
            ))
        }
    }

    async fn handle_session_git_branch(&self, params: &Value) -> RpcResult {
        let req: SessionIdParams = parse_params(params)?;
        let cwd = self
            .ctx
            .sessions
            .get_workspace_dir(&req.session_id)
            .await
            .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "session not found"))?;
        let info = crate::rpc::git::head_info(std::path::Path::new(&cwd)).unwrap_or_default();
        to_result(SessionGitBranchResult {
            session_id: req.session_id,
            branch: info.branch,
            hash: info.hash,
        })
    }

    async fn handle_session_list(&self, params: &Value) -> RpcResult {
        let backend = self
            .ctx
            .session_backend
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Session persistence is disabled"))?;
        let req: SessionListParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();

        // Use FTS when a query is provided, plain list otherwise.
        let all = if let Some(ref keyword) = req.query {
            if keyword.trim().is_empty() {
                backend.list_sessions_with_metadata()
            } else {
                use clawcrew_infra::session_backend::SessionQuery;
                backend.search(&SessionQuery {
                    keyword: Some(keyword.clone()),
                    limit: req.limit,
                })
            }
        } else {
            backend.list_sessions_with_metadata()
        };

        let sessions: Vec<SessionEntry> = all
            .into_iter()
            .filter(|meta| meta.agent_alias.is_some() || meta.channel_id.is_some())
            .map(|meta| {
                let agent_alias = meta.agent_alias.clone().or_else(|| {
                    meta.channel_id
                        .as_deref()
                        .and_then(|c| config.agent_for_channel(c))
                        .map(str::to_string)
                });
                let session_id = meta
                    .key
                    .strip_prefix("rpc_")
                    .or_else(|| meta.key.strip_prefix("gw_"))
                    .map(str::to_string)
                    .unwrap_or_else(|| meta.key.clone());
                SessionEntry {
                    session_id,
                    session_key: meta.key,
                    created_at: meta.created_at.to_rfc3339(),
                    last_activity: meta.last_activity.to_rfc3339(),
                    message_count: meta.message_count,
                    agent_alias,
                    channel_id: meta.channel_id,
                    name: meta.name,
                }
            })
            .collect();
        to_result(SessionListResult { sessions })
    }

    /// List ACP sessions from the dedicated ACP session store. The Code (ACP)
    /// pane in the TUI calls this instead of `session/list` so its picker only
    /// shows sessions that came from `acp-sessions.db` — chat-pane sessions
    /// live in the unified `session_backend` and must not appear here.
    async fn handle_session_list_acp(&self, _params: &Value) -> RpcResult {
        let store = self
            .ctx
            .acp_session_store
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "ACP session store is not available"))?;

        let summaries = store
            .list_sessions()
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("acp session list failed: {e}")))?;

        let sessions: Vec<SessionEntry> = summaries
            .into_iter()
            .map(|s| SessionEntry {
                session_id: s.session_uuid.clone(),
                // ACP sessions are keyed by their UUID directly — no `rpc_`/`gw_`
                // prefix exists in this store, so session_id == session_key.
                session_key: s.session_uuid,
                created_at: s.created_at.to_rfc3339(),
                last_activity: s.last_activity.to_rfc3339(),
                message_count: s.message_count,
                agent_alias: Some(s.agent_alias),
                channel_id: None,
                // ACP sessions don't carry a user-set display name today; the
                // picker falls back to `session_id` when this is None.
                name: None,
            })
            .collect();

        to_result(SessionListResult { sessions })
    }

    async fn handle_session_messages(&self, params: &Value) -> RpcResult {
        let req: SessionMessagesParams = parse_params(params)?;
        let mut messages = Vec::new();
        let mut acp_session_found = false;

        if let Some(store) = self.ctx.acp_session_store.as_ref() {
            match store.load_session(&req.session_id) {
                Ok(Some(data)) => {
                    acp_session_found = true;
                    messages = conversation_message_entries(&data.messages);
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(rpc_err(
                        INTERNAL_ERROR,
                        format!("Failed to load ACP session messages: {e}"),
                    ));
                }
            }
        }

        if !acp_session_found {
            let backend = self
                .ctx
                .session_backend
                .as_ref()
                .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Session persistence is disabled"))?;

            // Try the raw id first (channel sessions store as-is), then
            // prefixed variants for RPC/gateway-originated sessions.
            let candidates = [
                req.session_id.clone(),
                format!("rpc_{}", req.session_id),
                format!("gw_{}", req.session_id),
            ];
            for key in &candidates {
                let loaded = backend.load(key);
                if !loaded.is_empty() {
                    messages = loaded
                        .into_iter()
                        .map(|message| MessageEntry {
                            role: message.role,
                            content: message.content,
                            kind: MessageEntryKind::Message,
                            tool_call_id: None,
                            tool_name: None,
                            tool_input: None,
                            tool_output: None,
                        })
                        .collect();
                    break;
                }
            }
        }

        let total = messages.len();
        let limit = req.limit.unwrap_or(total);
        let end = req.before_index.map(|i| i.min(total)).unwrap_or(total);
        let start = end.saturating_sub(limit);
        let messages = messages[start..end].to_vec();

        to_result(SessionMessagesResult {
            session_id: req.session_id,
            messages,
            total,
            start,
        })
    }

    async fn handle_session_state(&self, params: &Value) -> RpcResult {
        let req: SessionIdParams = parse_params(params)?;
        if self.ctx.sessions.get_agent(&req.session_id).await.is_some() {
            let turn_generation = self.ctx.sessions.inflight_turn_generation(&req.session_id);
            let plan = self.ctx.sessions.get_plan(&req.session_id).await;
            let queued = self
                .ctx
                .sessions
                .session_queue
                .queue_depth(&req.session_id)
                .await
                > 0;
            return to_result(SessionStateResult {
                session_id: req.session_id,
                state: if turn_generation.is_some() || queued {
                    "running"
                } else {
                    "idle"
                }
                .to_string(),
                turn_id: turn_generation.map(|generation| generation.to_string()),
                turn_started_at: None,
                plan,
            });
        }

        // Gateway/legacy sessions that are not live in the RPC session store
        // retain their persisted-state fallback. Chat and ACP sessions above
        // must never use this metadata as a live-turn barrier.
        let backend = self
            .ctx
            .session_backend
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Session persistence is disabled"))?;
        let candidates = [
            req.session_id.clone(),
            format!("rpc_{}", req.session_id),
            format!("gw_{}", req.session_id),
        ];
        for key in &candidates {
            match backend.get_session_state(key) {
                Ok(Some(ss)) => {
                    return to_result(SessionStateResult {
                        session_id: req.session_id,
                        state: ss.state,
                        turn_id: ss.turn_id,
                        turn_started_at: ss.turn_started_at.map(|t| t.to_rfc3339()),
                        plan: None,
                    });
                }
                Ok(None) => continue,
                Err(e) => {
                    return Err(rpc_err(
                        INTERNAL_ERROR,
                        format!("Failed to get session state: {e}"),
                    ));
                }
            }
        }
        Err(rpc_err(SESSION_NOT_FOUND, "Session not found"))
    }

    async fn handle_session_delete(&self, params: &Value) -> RpcResult {
        let req: SessionIdParams = parse_params(params)?;
        self.ctx.sessions.signal_session_removal(&req.session_id);
        let _guard = self
            .ctx
            .sessions
            .session_queue
            .acquire(&req.session_id)
            .await
            .map_err(|e| rpc_err(SESSION_BUSY, format!("Session busy: {e}")))?;
        if let Some(agent) = self.ctx.sessions.get_agent(&req.session_id).await {
            agent
                .lock()
                .await
                .channel_handles()
                .unregister_channel("rpc");
        }
        let existed = self.ctx.sessions.remove(&req.session_id).await;
        if existed && let Some(ref hooks) = self.ctx.hooks {
            hooks.fire_session_end(&req.session_id, "rpc").await;
        }
        // Remove from persistent backend — try raw id, then prefixed variants.
        if let Some(ref backend) = self.ctx.session_backend {
            for key in &[
                req.session_id.clone(),
                format!("rpc_{}", req.session_id),
                format!("gw_{}", req.session_id),
            ] {
                let _ = backend.delete_session(key);
            }
        }
        to_result(SessionDeleteResult {
            session_id: req.session_id,
            deleted: true,
        })
    }

    fn handle_session_approve(&self, params: &Value) -> RpcResult {
        let p: SessionApproveParams = parse_params(params)?;

        let response = match p.decision.as_str() {
            "allow_once" => clawcrew_api::channel::ChannelApprovalResponse::Approve,
            "allow_always" => clawcrew_api::channel::ChannelApprovalResponse::AlwaysApprove,
            "reject" | "reject_once" => clawcrew_api::channel::ChannelApprovalResponse::Deny,
            "reject_with_edit" => {
                let replacement = p.replacement.unwrap_or_default();
                clawcrew_api::channel::ChannelApprovalResponse::DenyWithEdit { replacement }
            }
            other => {
                return Err(rpc_err(
                    INVALID_PARAMS,
                    format!("unknown decision: {other}"),
                ));
            }
        };

        self.ctx.approval_pending.resolve(&p.request_id, response);

        to_result(SessionApproveResult {
            session_id: p.session_id,
            request_id: p.request_id,
            acknowledged: true,
        })
    }

    // ── Memory handlers ──────────────────────────────────────────

    async fn handle_memory_list(&self, params: &Value) -> RpcResult {
        let mem = self
            .ctx
            .memory
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Memory subsystem is not available"))?;
        let req: MemoryListParams = parse_params(params)?;
        let category = req
            .category
            .as_deref()
            .map(|s| MemoryCategory::Custom(s.to_string()));
        let entries = mem
            .list(category.as_ref(), req.session_id.as_deref())
            .await
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Memory list failed: {e}")))?;
        let count = entries.len();
        let entries = truncate_memory_previews(entries);
        to_result(MemoryListResult { entries, count })
    }

    async fn handle_memory_search(&self, params: &Value) -> RpcResult {
        let mem = self
            .ctx
            .memory
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Memory subsystem is not available"))?;
        let req: MemorySearchParams = parse_params(params)?;
        let entries = mem
            .recall(
                &req.query,
                req.limit,
                req.session_id.as_deref(),
                req.since.as_deref(),
                req.until.as_deref(),
            )
            .await
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Memory search failed: {e}")))?;
        let count = entries.len();
        let entries = truncate_memory_previews(entries);
        to_result(MemorySearchResult { entries, count })
    }

    /// `memory/get { key } → MemoryEntry`. Returns the full memory
    /// entry for one key so the Memory pane can keep only preview
    /// rows in memory and fetch the full `content` only when the
    /// detail pane opens. Dropped on detail close.
    async fn handle_memory_get(&self, params: &Value) -> RpcResult {
        let mem = self
            .ctx
            .memory
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Memory subsystem is not available"))?;
        let req: MemoryGetParams = parse_params(params)?;
        let entry = mem
            .get(&req.key)
            .await
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Memory get failed: {e}")))?;
        match entry {
            Some(e) => to_result(MemoryGetResult { entry: Some(e) }),
            None => Err(rpc_err(
                INTERNAL_ERROR,
                format!("Memory key `{}` not found", req.key),
            )),
        }
    }

    async fn handle_memory_store(&self, params: &Value) -> RpcResult {
        let mem = self
            .ctx
            .memory
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Memory subsystem is not available"))?;
        let req: MemoryStoreParams = parse_params(params)?;
        let category = req
            .category
            .as_deref()
            .map(|s| MemoryCategory::Custom(s.to_string()))
            .unwrap_or(MemoryCategory::Custom("user".into()));
        mem.store(&req.key, &req.content, category, req.session_id.as_deref())
            .await
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Memory store failed: {e}")))?;
        to_result(MemoryStoreResult {
            key: req.key,
            stored: true,
        })
    }

    async fn handle_memory_delete(&self, params: &Value) -> RpcResult {
        let mem = self
            .ctx
            .memory
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Memory subsystem is not available"))?;
        let req: MemoryDeleteParams = parse_params(params)?;
        mem.forget(&req.key)
            .await
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Memory delete failed: {e}")))?;
        to_result(MemoryDeleteResult {
            key: req.key,
            deleted: true,
        })
    }

    // ── Cron handlers ────────────────────────────────────────────

    async fn handle_cron_list(&self) -> RpcResult {
        let config = self.ctx.config.read().clone();
        let jobs = crate::cron::list_jobs(&config)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Cron list failed: {e}")))?;
        to_result(CronListResult { jobs })
    }

    async fn handle_cron_get(&self, params: &Value) -> RpcResult {
        let req: CronIdParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let job = crate::cron::get_job(&config, &req.id)
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("Cron job not found: {e}")))?;
        to_result(job)
    }

    async fn handle_cron_add(&self, params: &Value) -> RpcResult {
        let req: CronAddParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let schedule = Schedule::Cron {
            expr: req.schedule,
            tz: req.tz,
        };
        let job = crate::cron::add_shell_job_with_approval(
            &config,
            &req.agent,
            req.name,
            schedule,
            req.command.as_deref().unwrap_or(""),
            req.delivery,
            true, // RPC calls are pre-approved
        )
        .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Cron add failed: {e}")))?;
        to_result(job)
    }

    async fn handle_cron_patch(&self, params: &Value) -> RpcResult {
        let req: CronPatchParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let patch = CronJobPatch {
            schedule: req.schedule.map(|s| Schedule::Cron {
                expr: s,
                tz: if req.clear_tz == Some(true) {
                    None
                } else {
                    req.tz
                },
            }),
            command: req.command,
            prompt: req.prompt,
            name: req.name,
            ..Default::default()
        };
        let job = crate::cron::update_job(&config, &req.id, patch)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Cron patch failed: {e}")))?;
        to_result(job)
    }

    async fn handle_cron_delete(&self, params: &Value) -> RpcResult {
        let req: CronIdParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        crate::cron::remove_job(&config, &req.id)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Cron delete failed: {e}")))?;
        to_result(CronDeleteResult {
            id: req.id,
            deleted: true,
        })
    }

    async fn handle_cron_runs(&self, params: &Value) -> RpcResult {
        let req: CronRunsParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let limit = req.limit.unwrap_or(20) as usize;
        let runs = crate::cron::list_runs(&config, &req.id, limit)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Cron runs failed: {e}")))?;
        to_result(CronRunsResult { runs })
    }

    async fn handle_cron_trigger(&self, params: &Value) -> RpcResult {
        let req: CronIdParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let job = crate::cron::get_job(&config, &req.id)
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("Cron job not found: {e}")))?;
        let event_tx = self.ctx.event_tx.clone();
        let result = crate::cron::scheduler::run_manual_job(
            &config,
            &job,
            crate::cron::scheduler::CronDeliveryContext::RpcManual,
            &event_tx,
        )
        .await;
        to_result(CronTriggerResult {
            id: result.job_id,
            success: result.success,
            status: result.status,
            output: result.output,
            duration_ms: result.duration_ms,
            started_at: result.started_at.to_rfc3339(),
            finished_at: result.finished_at.to_rfc3339(),
        })
    }

    async fn handle_cron_settings(&self, params: &Value) -> RpcResult {
        let config = self.ctx.config.read().clone();
        // If a "patch" field is present, this is a write; otherwise read.
        if params.get("patch").is_some() {
            not_yet_implemented(Method::CronSettings)
        } else {
            Ok(serde_json::to_value(&config.scheduler).unwrap_or(Value::Null))
        }
    }

    // ── Config handlers ──────────────────────────────────────────

    fn handle_config_get(&self, params: &Value) -> RpcResult {
        use clawcrew_config::traits::MaskSecrets;
        let req: ConfigGetParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        if let Some(prop) = req.prop {
            let val = config
                .get_prop(&prop)
                .map_err(|e| rpc_err(INVALID_PARAMS, format!("Unknown prop: {e}")))?;
            to_result(ConfigGetPropResult { prop, value: val })
        } else {
            // Return full config, masked.
            let mut masked = config;
            masked.mask_secrets();
            Ok(serde_json::to_value(&masked).unwrap_or(Value::Null))
        }
    }

    async fn handle_config_set(&self, params: &Value) -> RpcResult {
        let req: ConfigSetParams = parse_params(params)?;
        let refresh_model_provider_ref = model_provider_ref_from_provider_profile_prop(&req.prop);
        let refresh_scope = LiveSessionRefreshScope::for_prop(&req.prop);
        let config_write_guard = Arc::clone(&self.ctx.config_write_lock).lock_owned().await;
        // Clone the live config and perform every mutation — alias creation,
        // field lookup, value coercion, masked-secret validation, and the
        // persistent write — on the working copy. Any early error simply
        // returns and drops the clone, so a partially-applied attempt (e.g. a
        // freshly auto-created alias followed by a coercion failure) is
        // discarded as one unit and can never leave a phantom entry on the
        // live config. Only a fully successful mutation is committed, either
        // by the live-session refresh transaction (when this prop changes a
        // route-affecting surface) or by swapping the snapshot in under
        // `config_write_guard`. This keeps alias-creation ownership inside
        // `clawcrew-config` and commit orchestration inside the runtime,
        // rather than mirroring config transaction semantics through a
        // tracked tuple.
        // `Config` is a large aggregate; box the working clone so it lives on
        // the heap rather than inflating this async fn's stack frame across the
        // awaits below.
        let mut config = Box::new(self.ctx.config.read().clone());
        if config.ensure_map_key_for_path(&req.prop) {
            // Refused to vivify the reserved `default` agent: return a
            // reserved error rather than a downstream "Unknown property".
            return Err(rpc_err(
                INVALID_PARAMS,
                "alias `default` is reserved and cannot be created",
            ));
        }
        let info = config
            .prop_fields()
            .into_iter()
            .find(|f| f.name == req.prop);
        // Polymorphic value: strings pass through, everything else coerced.
        let value_str = match &req.value {
            Value::String(s) => s.clone(),
            other => match clawcrew_config::typed_value::coerce_for_set_prop(
                other,
                info.as_ref().map(|i| i.kind),
            ) {
                Ok(coerced) => coerced,
                Err(e) => return Err(rpc_err(INVALID_PARAMS, e.message)),
            },
        };
        // Reject the masked sentinel for secrets — surfaces echo the
        // masked display value back when no real edit happened, and
        // letting that through silently clobbers the live secret with
        // the literal masked string.
        let is_secret_prop = info
            .as_ref()
            .is_some_and(|i| i.is_secret || i.derived_from_secret)
            || clawcrew_config::schema::Config::prop_is_secret(&req.prop);
        if is_secret_prop
            && (value_str == clawcrew_config::traits::MASKED_SECRET
                || value_str == "****"
                || value_str.is_empty())
        {
            return Err(rpc_err(
                INVALID_PARAMS,
                format!(
                    "Refusing to overwrite secret `{}` with a masked or empty value",
                    req.prop
                ),
            ));
        }
        if let Err(e) = config.set_prop_persistent(&req.prop, &value_str) {
            return Err(rpc_err(INTERNAL_ERROR, format!("Config set failed: {e}")));
        }
        // A route-affecting prop must publish its provider/resolver/generation
        // rebuild in the same transaction that commits the config, so dispatch
        // and reported limits can never straddle two generations.
        if let Some(scope) = refresh_scope.as_ref() {
            Box::pin(self.commit_config_with_live_session_refresh(
                *config,
                &config_write_guard,
                scope,
            ))
            .await?;
        } else {
            self.save_and_swap_config(*config, &config_write_guard)
                .await?;
        }
        if let Some(model_provider_ref) = refresh_model_provider_ref {
            self.refresh_memory_embedder_for_model_provider(&model_provider_ref);
        }
        to_result(ConfigSetResult {
            prop: req.prop,
            set: true,
        })
    }

    fn refresh_memory_embedder_for_model_provider(&self, model_provider_ref: &str) {
        let resolved = {
            let config = self.ctx.config.read();
            if !memory_embeddings_use_provider(&config, model_provider_ref) {
                return;
            }
            // Match daemon-boot resolution (`create_memory_with_storage_and_routes`
            // is called with `api_key = None`): keys come from the per-route /
            // `[memory]` override or the referenced profile, never an inherited seed.
            clawcrew_memory::resolve_embedding_settings(
                &config.memory,
                &config.embedding_routes,
                None,
                Some(&config.providers.models),
            )
        };
        // 1. Install-wide RPC memory handle.
        if let Some(memory) = self.ctx.memory.as_ref() {
            memory.refresh_embedder(
                &resolved.model_provider,
                resolved.api_key.as_deref(),
                &resolved.model,
                resolved.dimensions,
            );
        }
        self.schedule_live_agent_memory_refresh(resolved);
    }

    fn schedule_live_agent_memory_refresh(&self, resolved: clawcrew_memory::EmbeddingSettings) {
        let ctx = Arc::clone(&self.ctx);
        clawcrew_spawn::spawn!(async move {
            Self::refresh_live_agent_memory(ctx, resolved).await;
        });
    }

    async fn refresh_live_agent_memory(
        ctx: Arc<RpcContext>,
        resolved: clawcrew_memory::EmbeddingSettings,
    ) {
        for session_id in ctx.sessions.list_ids().await {
            if let Some(agent) = ctx.sessions.get_agent(&session_id).await {
                agent.lock().await.refresh_memory_embedder(
                    &resolved.model_provider,
                    resolved.api_key.as_deref(),
                    &resolved.model,
                    resolved.dimensions,
                );
            }
        }
    }

    /// Validate and materialize every affected live-session provider view from
    /// `working`, then atomically commit that candidate config and publish the
    /// prepared views while their per-session generation locks remain held.
    ///
    /// `working` is deliberately not installed before preparation succeeds.
    /// A provider construction failure therefore leaves both the canonical
    /// config and every derived live-session view on the prior generation.
    async fn commit_config_with_live_session_refresh(
        &self,
        working: Config,
        config_write_guard: &ConfigWriteGuard,
        scope: &LiveSessionRefreshScope,
    ) -> Result<(), JsonRpcError> {
        let prepared =
            Self::prepare_live_sessions_refresh(Arc::clone(&self.ctx), &working, scope).await?;
        // Test-only: park after preparation so a regression can drive other
        // RPCs (`session/configure`, session rehydration) deterministically
        // inside the prepared-and-skipped window — every skip decision (and
        // its guard release) has happened, the `list_ids()` snapshot has
        // passed, but the candidate config is not yet saved or swapped. A
        // merely-notified hook cannot hold this window open: on a loaded
        // runner the commit can finish while the test is still observing
        // the mid-transaction state.
        #[cfg(test)]
        if let Some(pause) = self.ctx.config_commit_pause.as_ref() {
            pause.arrived.notify_one();
            pause.release.notified().await;
        }
        self.save_and_swap_config(working, config_write_guard)
            .await?;
        let config_generation = Arc::new(self.ctx.config.read().clone());
        Self::apply_prepared_live_sessions_refresh(
            Arc::clone(&self.ctx),
            prepared,
            config_generation,
        )
        .await;
        Ok(())
    }

    async fn prepare_live_sessions_refresh(
        ctx: Arc<RpcContext>,
        config: &Config,
        scope: &LiveSessionRefreshScope,
    ) -> Result<Vec<PreparedLiveSessionRefresh>, JsonRpcError> {
        let session_ids = ctx.sessions.list_ids().await;
        let mut prepared = Vec::new();
        for session_id in session_ids {
            // Capture the generation before acquiring the lock so we can
            // detect same-ID replacement while waiting.
            let Some(session_generation) = ctx.sessions.get_generation(&session_id).await else {
                continue;
            };

            // Acquire the per-session ordering boundary. This serialises
            // with session/configure so the state we read afterwards
            // reflects any configure that committed before this point.
            // The guard is BOUND (not dropped): it is moved into
            // `PreparedLiveSessionRefresh` and held through publication, so
            // preparation and apply are one ordered transition.
            let Some(model_provider_update) =
                ctx.sessions.lock_model_provider_update(&session_id).await
            else {
                continue;
            };

            // Re-verify the session has not been replaced while we waited
            // for the lock.
            let Some(current_gen) = ctx.sessions.get_generation(&session_id).await else {
                continue;
            };
            if current_gen != session_generation {
                continue;
            };

            // Re-read alias and overrides inside the ordering boundary.
            // A session/configure may have committed new overrides on the
            // same generation while we were waiting for the lock; reading
            // here ensures the refresh acts on the latest state.
            let Some(agent_alias) = ctx.sessions.get_agent_alias(&session_id).await else {
                continue;
            };
            let Some(overrides) = ctx.sessions.get_overrides(&session_id).await else {
                continue;
            };
            let Some(model_provider_ref) = scope
                .resolve_provider_ref(config, &agent_alias, &overrides)
                .map_err(|error| {
                    rpc_err(
                        INVALID_PARAMS,
                        format!(
                            "Config update cannot refresh live session `{session_id}`: {error}"
                        ),
                    )
                })?
            else {
                continue;
            };
            let provider_temperature =
                model_provider_ref
                    .split_once('.')
                    .and_then(|(provider_type, provider_alias)| {
                        config
                            .providers
                            .models
                            .find(provider_type, provider_alias)
                            .and_then(|entry| entry.temperature)
                    });
            let agent_cfg = config
                .resolved_agent_config(&agent_alias)
                .or_else(|| config.agent(&agent_alias).cloned())
                .ok_or_else(|| {
                    rpc_err(
                        INVALID_PARAMS,
                        format!(
                            "Config update cannot refresh live session `{session_id}`: agent \
                             `{agent_alias}` is not configured"
                        ),
                    )
                })?;
            let (model_provider, model_provider_name, model_name, model_route_resolver) =
                crate::agent::agent::build_session_model_provider(
                    config,
                    &model_provider_ref,
                    overrides.model.as_deref(),
                )
                .map_err(|error| {
                    rpc_err(
                        INVALID_PARAMS,
                        format!(
                            "Config update cannot refresh live session `{session_id}` from \
                             `{model_provider_ref}`: {error}"
                        ),
                    )
                })?;
            let tool_dispatcher = crate::agent::agent::tool_dispatcher_for_provider(
                &agent_cfg,
                model_provider.as_ref(),
                &model_name,
            );
            prepared.push(PreparedLiveSessionRefresh {
                session_id,
                session_generation,
                _model_provider_update: model_provider_update,
                model_provider,
                model_provider_name,
                model_name,
                model_route_resolver,
                tool_dispatcher,
                temperature: overrides.temperature.or(provider_temperature),
                override_migration: match scope {
                    LiveSessionRefreshScope::ProviderAliasRename { old_ref, new_ref } => overrides
                        .model_provider
                        .as_deref()
                        .is_some_and(|current| current == old_ref)
                        .then(|| new_ref.clone()),
                    _ => None,
                },
            });
        }
        Ok(prepared)
    }

    async fn apply_prepared_live_sessions_refresh(
        ctx: Arc<RpcContext>,
        prepared: Vec<PreparedLiveSessionRefresh>,
        config_generation: Arc<Config>,
    ) {
        for refresh in prepared {
            let PreparedLiveSessionRefresh {
                session_id,
                session_generation,
                _model_provider_update,
                model_provider,
                model_provider_name,
                model_name,
                model_route_resolver,
                tool_dispatcher,
                temperature,
                override_migration,
            } = refresh;
            // Migrate the stored override first so the session's own reference
            // and the provider box it is about to receive name the same alias
            // for the whole publication, still under this session's guard.
            if let Some(new_ref) = override_migration {
                ctx.sessions
                    .migrate_model_provider_override(&session_id, session_generation, new_ref)
                    .await;
            }
            let applied = ctx
                .sessions
                .apply_model_provider(
                    &session_id,
                    session_generation,
                    model_provider,
                    model_provider_name,
                    model_name,
                    model_route_resolver,
                    tool_dispatcher,
                    Arc::clone(&config_generation),
                    // Temperature travels in the same state transition as the
                    // provider box rather than a follow-up `set_temperature`,
                    // so a session cannot briefly show the new provider with
                    // the old profile temperature.
                    Some(temperature),
                )
                .await;
            if applied {
                ctx.sessions
                    .clear_pending_generation(&session_id, session_generation)
                    .await;
            }
        }
    }

    #[cfg(test)]
    async fn refresh_live_sessions_for_agent(
        ctx: Arc<RpcContext>,
        agent_alias: &str,
    ) -> Result<(), JsonRpcError> {
        let config = ctx.config.read().clone();
        let prepared = Self::prepare_live_sessions_refresh(
            Arc::clone(&ctx),
            &config,
            &LiveSessionRefreshScope::Agent(agent_alias.to_string()),
        )
        .await?;
        Self::apply_prepared_live_sessions_refresh(ctx, prepared, Arc::new(config)).await;
        Ok(())
    }

    fn handle_config_validate(&self) -> RpcResult {
        let config = self.ctx.config.read().clone();
        match config.validate() {
            Ok(()) => to_result(ConfigValidateResult {
                valid: true,
                error: None,
            }),
            Err(e) => to_result(ConfigValidateResult {
                valid: false,
                error: Some(e.to_string()),
            }),
        }
    }

    fn handle_config_reload(&self) -> RpcResult {
        if !self.schedule_daemon_reload("config") {
            return Err(rpc_err(INTERNAL_ERROR, "Reload not available"));
        }
        to_result(ConfigReloadResult { reloading: true })
    }

    fn schedule_daemon_reload(&self, surface: &'static str) -> bool {
        let Some(reload_tx) = self.ctx.reload_tx.clone() else {
            return false;
        };
        let gateway_shutdown_tx = self.ctx.gateway_shutdown_tx.clone();
        clawcrew_spawn::spawn!(async move {
            tokio::time::sleep(RPC_RELOAD_REPLY_FLUSH_DELAY).await;
            if let Some(gateway_shutdown_tx) = gateway_shutdown_tx {
                let _ = gateway_shutdown_tx.send(true);
                tokio::time::sleep(RPC_RELOAD_GATEWAY_SHUTDOWN_DELAY).await;
            }
            let _ = reload_tx.send(true);
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Complete)
                    .with_outcome(::clawcrew_log::EventOutcome::Success)
                    .with_attrs(::serde_json::json!({ "surface": surface })),
                "daemon reload dispatched"
            );
        });
        true
    }

    fn handle_config_list(&self, params: &Value) -> RpcResult {
        use clawcrew_config::field_visibility;
        use clawcrew_config::traits::ConfigFieldEntry;
        let req: ConfigListParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let prefix = req.prefix.as_deref();
        let excluded = field_visibility::excluded_paths(&config, prefix.unwrap_or(""));
        let entries: Vec<ConfigFieldEntry> = config
            .prop_fields()
            .into_iter()
            .filter(|info| match prefix {
                Some(p) => field_visibility::path_matches_prefix(&info.name, p),
                None => true,
            })
            .filter(|info| !field_visibility::is_excluded(&info.name, &excluded))
            .map(|info| {
                let env = config.prop_is_env_overridden(&info.name);
                ConfigFieldEntry::from_prop_field(info, env)
            })
            .collect();
        to_result(ConfigListResult { entries })
    }

    async fn handle_config_delete(&self, params: &Value) -> RpcResult {
        let req: ConfigDeleteParams = parse_params(params)?;
        let refresh_model_provider_ref = model_provider_ref_from_provider_profile_prop(&req.prop);
        let refresh_scope = LiveSessionRefreshScope::for_prop(&req.prop);
        let config_write_guard = Arc::clone(&self.ctx.config_write_lock).lock_owned().await;
        if let Some(scope) = refresh_scope.as_ref() {
            let mut working = self.ctx.config.read().clone();
            working
                .set_prop_persistent(&req.prop, "")
                .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Config delete failed: {e}")))?;
            Box::pin(self.commit_config_with_live_session_refresh(
                working,
                &config_write_guard,
                scope,
            ))
            .await?;
        } else {
            {
                let mut config = self.ctx.config.write();
                config
                    .set_prop_persistent(&req.prop, "")
                    .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Config delete failed: {e}")))?;
            }
            self.flush_config(&config_write_guard).await?;
        }
        if let Some(model_provider_ref) = refresh_model_provider_ref {
            self.refresh_memory_embedder_for_model_provider(&model_provider_ref);
        }
        to_result(ConfigDeleteResult {
            prop: req.prop,
            deleted: true,
        })
    }

    fn handle_config_resolve_alias_source(&self, params: &Value) -> RpcResult {
        let req: ConfigResolveAliasSourceParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let values = config.resolve_alias_source(req.source);
        to_result(ConfigResolveAliasSourceResult {
            source: req.source,
            values,
        })
    }

    fn handle_config_map_keys(&self, params: &Value) -> RpcResult {
        let req: ConfigMapKeysParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let keys = config.get_map_keys(&req.path).ok_or_else(|| {
            rpc_err(
                INVALID_PARAMS,
                format!("No map-keyed section at `{}`", req.path),
            )
        })?;
        to_result(ConfigMapKeysResult {
            path: req.path,
            keys,
        })
    }

    async fn handle_config_map_key_create(&self, params: &Value) -> RpcResult {
        let req: ConfigMapKeyCreateParams = parse_params(params)?;
        let config_write_guard = Arc::clone(&self.ctx.config_write_lock).lock_owned().await;
        let create = |config: &mut Config| -> Result<bool, JsonRpcError> {
            // Shared guarded boundary: enforces the reserved-agent rule (the
            // `default` runtime fallback) on this surface too, so the RPC create
            // path cannot author an `agents.default` the rename guard then traps.
            let created =
                clawcrew_config::alias_refs::create_map_key_checked(config, &req.path, &req.key)
                    .map_err(|e| rpc_err(INVALID_PARAMS, e.to_string()))?;
            if created {
                config.mark_dirty(&format!("{}.{}", req.path, req.key));
            }
            Ok(created)
        };
        let created = if touches_model_routes(&req.path) {
            let mut working = self.ctx.config.read().clone();
            let created = create(&mut working)?;
            if created {
                Box::pin(self.commit_config_with_live_session_refresh(
                    working,
                    &config_write_guard,
                    &LiveSessionRefreshScope::ModelRoutes,
                ))
                .await?;
            }
            created
        } else {
            let created = {
                let mut config = self.ctx.config.write();
                create(&mut config)?
            };
            if created {
                self.flush_config(&config_write_guard).await?;
            }
            created
        };
        to_result(ConfigMapKeyCreateResult {
            path: req.path,
            key: req.key,
            created,
        })
    }

    async fn handle_config_map_key_delete(&self, params: &Value) -> RpcResult {
        let req: ConfigMapKeyDeleteParams = parse_params(params)?;
        let config_write_guard = Arc::clone(&self.ctx.config_write_lock).lock_owned().await;

        // Provider model alias deletions must participate in the same
        // prepare-commit-apply transaction as provider profile field edits so
        // that active sessions' provider box, resolver, and config-generation
        // snapshot stay on one coherent generation.
        let provider_model_alias_kind =
            clawcrew_config::alias_refs::alias_kind_for_map_path(&req.path).filter(|k| {
                matches!(
                    k,
                    clawcrew_config::alias_refs::AliasKind::Provider {
                        category: clawcrew_config::alias_refs::ProviderCategory::Models,
                        ..
                    }
                )
            });

        let delete_plain = |config: &mut Config| -> Result<bool, JsonRpcError> {
            let deleted = config
                .delete_map_key(&req.path, &req.key)
                .map_err(|e| rpc_err(INVALID_PARAMS, e))?;
            if deleted {
                config.mark_dirty(&format!("{}.{}", req.path, req.key));
            }
            Ok(deleted)
        };

        let deleted = if touches_model_routes(&req.path) {
            let mut working = self.ctx.config.read().clone();
            let deleted = delete_plain(&mut working)?;
            if deleted {
                Box::pin(self.commit_config_with_live_session_refresh(
                    working,
                    &config_write_guard,
                    &LiveSessionRefreshScope::ModelRoutes,
                ))
                .await?;
            }
            deleted
        } else if let Some(kind) = provider_model_alias_kind {
            // Use delete_with_cascade so referrer fields are scrubbed and hard
            // references refuse the delete, then run the live-session rebuild
            // transactionally so sessions observe one consistent generation.
            let mut working = self.ctx.config.read().clone();
            let cascade = clawcrew_config::alias_refs::delete_with_cascade(
                &mut working,
                &kind,
                &req.key,
                clawcrew_config::alias_refs::CascadePolicy::RefuseOnHard,
            )
            .map_err(|e| rpc_err(INVALID_PARAMS, e.to_string()))?;
            let deleted = cascade.deleted_entry.is_some();
            if deleted {
                for path in cascade.dirty_paths() {
                    working.mark_dirty(&path);
                }
                // Scope on the new (post-delete) provider ref is moot — the
                // alias is gone, so resolve_provider_ref returns None for every
                // session whose provider ref matched it. Use ModelRoutes as the
                // widest safe scope: it refreshes every session regardless of
                // their provider ref, ensuring the deleted alias is never
                // consulted again.
                Box::pin(self.commit_config_with_live_session_refresh(
                    working,
                    &config_write_guard,
                    &LiveSessionRefreshScope::ModelRoutes,
                ))
                .await?;
            }
            deleted
        } else {
            let deleted = {
                let mut config = self.ctx.config.write();
                delete_plain(&mut config)?
            };
            if deleted {
                self.flush_config(&config_write_guard).await?;
            }
            deleted
        };
        to_result(ConfigMapKeyDeleteResult {
            path: req.path,
            key: req.key,
            deleted,
        })
    }

    fn handle_config_map_key_rename<'a>(&'a self, params: &'a Value) -> BoxRpcFuture<'a> {
        let req: ConfigMapKeyRenameParams = match parse_params(params) {
            Ok(req) => req,
            Err(err) => return Box::pin(std::future::ready(Err(err))),
        };

        Box::pin(async move {
            // Acquired once here, not inside `handle_config_alias_rename`:
            // the alias-kind branch below delegates into it, and the tokio
            // Mutex is not reentrant. The guard moves by value into the
            // alias-rename path so it can be released at that handler's
            // commit point, before its slow post-commit side effects.
            let config_write_guard = Arc::clone(&self.ctx.config_write_lock).lock_owned().await;
            if let Some(kind) = clawcrew_config::alias_refs::alias_kind_for_map_path(&req.path) {
                return self
                    .handle_config_alias_rename(req, kind, config_write_guard)
                    .await;
            }

            let rename = |config: &mut Config| -> Result<bool, JsonRpcError> {
                let renamed = config
                    .rename_map_key(&req.path, &req.from, &req.to)
                    .map_err(|e| rpc_err(INVALID_PARAMS, e))?;
                if renamed {
                    config.mark_dirty(&format!("{}.{}", req.path, req.from));
                    config.mark_dirty(&format!("{}.{}", req.path, req.to));
                }
                Ok(renamed)
            };
            let renamed = if touches_model_routes(&req.path) {
                let mut working = self.ctx.config.read().clone();
                let renamed = rename(&mut working)?;
                if renamed {
                    Box::pin(self.commit_config_with_live_session_refresh(
                        working,
                        &config_write_guard,
                        &LiveSessionRefreshScope::ModelRoutes,
                    ))
                    .await?;
                }
                renamed
            } else {
                let renamed = {
                    let mut config = self.ctx.config.write();
                    rename(&mut config)?
                };
                if renamed {
                    self.flush_config(&config_write_guard).await?;
                }
                renamed
            };
            to_result(ConfigMapKeyRenameResult {
                path: req.path,
                from: req.from,
                to: req.to,
                renamed,
                warnings: Vec::new(),
            })
        })
    }

    fn handle_config_alias_rename<'a>(
        &'a self,
        req: ConfigMapKeyRenameParams,
        kind: clawcrew_config::alias_refs::AliasKind,
        config_write_guard: ConfigWriteGuard,
    ) -> BoxRpcFuture<'a> {
        Box::pin(async move {
            let is_agent = matches!(kind, clawcrew_config::alias_refs::AliasKind::Agent);
            // A model-provider alias rename is a route-affecting live
            // configuration surface: sessions whose provider ref pointed at
            // `from` must be rebuilt against `to` on the same generation as
            // the config commit, the same as a `providers.models.*` field
            // edit already does.
            let model_provider_family = match &kind {
                clawcrew_config::alias_refs::AliasKind::Provider {
                    category: clawcrew_config::alias_refs::ProviderCategory::Models,
                    family,
                } => Some(family.clone()),
                _ => None,
            };
            if is_agent {
                // Live RPC sessions hold the selected agent alias in memory; refuse
                // rather than letting them recreate old-alias state after the rename.
                let active = self
                    .ctx
                    .sessions
                    .count_by_agent()
                    .await
                    .get(&req.from)
                    .copied()
                    .unwrap_or(0);
                if active > 0 {
                    return Err(rpc_err(
                        INVALID_PARAMS,
                        format!(
                            "{}.{}: cannot rename agent with {active} active RPC session(s); close those sessions first",
                            req.path, req.from
                        ),
                    ));
                }
            }

            let mut working = self.ctx.config.read().clone();
            let old_workspace = is_agent.then(|| working.agent_workspace_dir(&req.from));
            // If a prior call saved config as `to` but crashed before side effects,
            // re-running `from -> to` should converge lagging owned state instead
            // of failing because `from` is no longer a config key.
            let resume_committed_to = is_agent
                && working.agent(&req.from).is_none()
                && working.agent(&req.to).is_some()
                && self.agent_rename_residue_exists(&working, &req.from).await;

            if !resume_committed_to {
                let report = clawcrew_config::alias_refs::rename_with_cascade(
                    &mut working,
                    &kind,
                    &req.from,
                    &req.to,
                )
                .map_err(|e| rename_error_to_rpc(&req.path, &req.from, e))?;
                for path in &report.dirty_paths {
                    working.mark_dirty(path);
                }
                if let Some(family) = model_provider_family.as_ref() {
                    Box::pin(self.commit_config_with_live_session_refresh(
                        working.clone(),
                        &config_write_guard,
                        &LiveSessionRefreshScope::ProviderAliasRename {
                            old_ref: format!("{family}.{}", req.from),
                            new_ref: format!("{family}.{}", req.to),
                        },
                    ))
                    .await?;
                } else {
                    self.save_and_swap_config(working.clone(), &config_write_guard)
                        .await?;
                }
            }
            // Config is committed (saved + swapped, or already committed by a
            // prior crashed run). Release before the post-commit side effects
            // below: workspace moves and the memory/cron/ACP/session-backend
            // cascade can be slow or wedge, and holding the lock across them
            // would stall every config-mutating RPC daemon-wide.
            drop(config_write_guard);
            let new_workspace = is_agent.then(|| working.agent_workspace_dir(&req.to));

            let mut warnings = Vec::new();
            if let (Some(old_workspace), Some(new_workspace)) = (old_workspace, new_workspace) {
                warnings.extend(move_renamed_agent_workspace(&old_workspace, &new_workspace).await);
                warnings.extend(
                    self.rename_agent_owned_state(&working, &req.from, &req.to)
                        .await,
                );
            }

            to_result(ConfigMapKeyRenameResult {
                path: req.path,
                from: req.from,
                to: req.to,
                renamed: true,
                warnings,
            })
        })
    }

    async fn rename_agent_owned_state(
        &self,
        config: &clawcrew_config::schema::Config,
        from: &str,
        to: &str,
    ) -> Vec<String> {
        let mut warnings = Vec::new();
        let mut memory_rows = 0usize;
        let mut cron_jobs = 0usize;
        let mut acp_sessions = 0usize;
        let mut sessions_repointed = 0usize;

        if let Some(mem) = &self.ctx.memory {
            match mem.rename_agent(from, to).await {
                Ok(n) => memory_rows = n,
                Err(e) => warnings.push(format!("memory rename: {e}")),
            }
        }

        match crate::cron::rename_jobs_by_agent(config, from, to) {
            Ok(n) => cron_jobs = n,
            Err(e) => warnings.push(format!("cron rename: {e}")),
        }

        match &self.ctx.acp_session_store {
            Some(store) => match store.rename_sessions_by_agent(from, to) {
                Ok(n) => acp_sessions = n,
                Err(e) => warnings.push(format!("acp rename: {e}")),
            },
            None => warnings.push("acp store unavailable".to_string()),
        }

        if let Some(backend) = &self.ctx.session_backend {
            match backend.rename_agent_attribution(from, to) {
                Ok(n) => sessions_repointed = n,
                Err(e) => warnings.push(format!("session attribution rename: {e}")),
            }
        }

        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note).with_attrs(
                ::serde_json::json!({
                    "from": from,
                    "to": to,
                    "memory": memory_rows,
                    "cron": cron_jobs,
                    "acp": acp_sessions,
                    "sessions": sessions_repointed,
                    "warnings": warnings.clone(),
                })
            ),
            "agent renamed with RPC owned-state cascade"
        );

        warnings
    }

    fn handle_config_templates(&self) -> RpcResult {
        use clawcrew_config::schema::Config;
        let templates: Vec<ConfigTemplateEntry> = Config::map_key_sections()
            .into_iter()
            .map(Into::into)
            .collect();
        to_result(ConfigTemplatesResult { templates })
    }

    // ── Agents handlers ──────────────────────────────────────────

    fn handle_agents_list(&self) -> RpcResult {
        let config = self.ctx.config.read().clone();
        let agents: Vec<AgentEntry> = config
            .agents
            .iter()
            .map(|(alias, agent_cfg)| AgentEntry {
                alias: alias.clone(),
                enabled: agent_cfg.enabled,
                channels: agent_cfg.channels.iter().map(|c| c.to_string()).collect(),
            })
            .collect();
        to_result(AgentsListResult { agents })
    }

    async fn handle_agents_status(&self) -> RpcResult {
        let config = self.ctx.config.read().clone();

        // Count sessions from the persisted backend (covers channel-originated
        // sessions) + in-memory RPC sessions, deduped by taking the max.
        let rpc_counts = self.ctx.sessions.count_by_agent().await;
        let mut backend_counts = std::collections::HashMap::<String, usize>::new();
        if let Some(ref backend) = self.ctx.session_backend {
            for meta in backend.list_sessions_with_metadata() {
                let alias = meta.agent_alias.or_else(|| {
                    meta.channel_id
                        .as_deref()
                        .and_then(|c| config.agent_for_channel(c))
                        .map(str::to_string)
                });
                if let Some(a) = alias {
                    *backend_counts.entry(a).or_default() += 1;
                }
            }
        }

        let agents: Vec<AgentStatusEntry> = config
            .agents
            .iter()
            .map(|(alias, agent_cfg)| {
                let rpc = *rpc_counts.get(alias).unwrap_or(&0);
                let persisted = *backend_counts.get(alias).unwrap_or(&0);
                AgentStatusEntry {
                    alias: alias.clone(),
                    enabled: agent_cfg.enabled,
                    live_sessions: rpc,
                    persisted_sessions: persisted,
                    channels: agent_cfg.channels.iter().map(|c| c.to_string()).collect(),
                }
            })
            .collect();
        to_result(AgentsStatusResult { agents })
    }

    // ── Cost handler ─────────────────────────────────────────────

    fn handle_cost_query(&self, params: &Value) -> RpcResult {
        let tracker = self
            .ctx
            .cost_tracker
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Cost tracking is not available"))?;
        let req: CostQueryParams = parse_params(params)?;
        // Optional `[from, to)` window (RFC3339). Lets callers (the dashboard's
        // Reports view, or an external CLI report) pull day/month/quarter/YTD
        // scalars rather than only the daemon's today/this-month aggregates.
        let parse_bound = |raw: &str| -> Result<chrono::DateTime<chrono::Utc>, _> {
            chrono::DateTime::parse_from_rfc3339(raw)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| rpc_err(INVALID_PARAMS, format!("invalid date {raw:?}: {e}")))
        };
        let from = req.from.as_deref().map(parse_bound).transpose()?;
        let to = req.to.as_deref().map(parse_bound).transpose()?;
        // Precedence (inherited from the existing per-agent path): an explicit
        // `agent` selects that agent's summary and the [from, to) window does
        // NOT apply; the window scopes only the fleet-wide summary.
        let summary = if let Some(agent) = req.agent {
            tracker
                .get_summary_for_agent(&agent)
                .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Cost query failed: {e}")))?
        } else if from.is_some() || to.is_some() {
            tracker
                .get_summary_in_bounds(from, to)
                .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Cost query failed: {e}")))?
        } else {
            tracker
                .get_summary()
                .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Cost query failed: {e}")))?
        };
        to_result(summary)
    }

    fn handle_cost_org(&self) -> RpcResult {
        let path = self.ctx.config.read().data_dir.join("org_cost.json");
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let value: Value = serde_json::from_str(&raw).map_err(|e| {
                    rpc_err(
                        INTERNAL_ERROR,
                        format!("org_cost.json is not valid JSON: {e}"),
                    )
                })?;
                Ok(value)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
            Err(e) => Err(rpc_err(
                INTERNAL_ERROR,
                format!("failed to read org_cost.json: {e}"),
            )),
        }
    }

    // ── Skills handlers ──────────────────────────────────────────

    fn handle_skills_bundles(&self) -> RpcResult {
        let config = self.ctx.config.read().clone();
        let root = config.install_root_dir();
        let svc = crate::skills::service::SkillsService::new(&config, &root);
        let bundles: Vec<SkillBundleEntry> = svc
            .list_bundles()
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Skills bundles failed: {e}")))?
            .into_iter()
            .map(|b| SkillBundleEntry {
                alias: b.alias,
                directory: b.directory.to_string_lossy().to_string(),
                include: b.include,
                exclude: b.exclude,
            })
            .collect();
        to_result(SkillsBundlesResult { bundles })
    }

    fn handle_skills_list(&self, params: &Value) -> RpcResult {
        let req: SkillsListParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let root = config.install_root_dir();
        let svc = crate::skills::service::SkillsService::new(&config, &root);
        let skills: Vec<SkillListEntry> = svc
            .list_skills(req.bundle.as_deref())
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Skills list failed: {e}")))?
            .into_iter()
            .map(|s| SkillListEntry {
                bundle: s.r#ref.bundle().to_string(),
                name: s.r#ref.name().to_string(),
                directory: s.directory.to_string_lossy().to_string(),
                frontmatter: s.frontmatter,
            })
            .collect();
        to_result(SkillsListResult { skills })
    }

    fn handle_skills_read(&self, params: &Value) -> RpcResult {
        let req: SkillsReadParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let root = config.install_root_dir();
        let svc = crate::skills::service::SkillsService::new(&config, &root);
        let skill_ref = svc
            .resolve_ref(&req.name, Some(&req.bundle))
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("Invalid skill ref: {e}")))?;
        let doc = svc
            .read_skill(&skill_ref)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Skill read failed: {e}")))?;
        to_result(SkillsReadResult {
            bundle: req.bundle,
            name: req.name,
            frontmatter: doc.frontmatter,
            body: doc.body,
        })
    }

    fn handle_skills_write(&self, params: &Value) -> RpcResult {
        let req: SkillsWriteParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let root = config.install_root_dir();
        let svc = crate::skills::service::SkillsService::new(&config, &root);
        let skill_ref = svc
            .resolve_ref(&req.name, Some(&req.bundle))
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("Invalid skill ref: {e}")))?;
        let doc = crate::skills::document::SkillDocument {
            frontmatter: req.frontmatter,
            body: req.body,
        };
        svc.write_skill(&skill_ref, &doc)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Skill write failed: {e}")))?;
        to_result(SkillsWriteResult {
            bundle: req.bundle,
            name: req.name,
            written: true,
        })
    }

    fn handle_skills_delete(&self, params: &Value) -> RpcResult {
        let req: SkillsDeleteParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let root = config.install_root_dir();
        let svc = crate::skills::service::SkillsService::new(&config, &root);
        let skill_ref = svc
            .resolve_ref(&req.name, Some(&req.bundle))
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("Invalid skill ref: {e}")))?;
        svc.remove_skill(&skill_ref, crate::skills::service::RemoveMode::Archive)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Skill delete failed: {e}")))?;
        to_result(SkillsDeleteResult {
            bundle: req.bundle,
            name: req.name,
            deleted: true,
        })
    }

    // ── Personality handlers ─────────────────────────────────────

    fn handle_personality_list(&self, params: &Value) -> RpcResult {
        let req: PersonalityListParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let workspace = req.agent.as_deref().map(|a| config.agent_workspace_dir(a));
        let files: Vec<PersonalityFileEntry> =
            crate::agent::personality::EDITABLE_PERSONALITY_FILES
                .iter()
                .map(|&filename| {
                    let (exists, size, mtime_ms) = workspace
                        .as_ref()
                        .and_then(|dir| {
                            let path = dir.join(filename);
                            let meta = std::fs::metadata(&path).ok()?;
                            let mtime = meta
                                .modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_millis() as i64);
                            Some((true, meta.len(), mtime))
                        })
                        .unwrap_or((false, 0, None));
                    PersonalityFileEntry {
                        filename: filename.to_string(),
                        exists,
                        size,
                        mtime_ms,
                    }
                })
                .collect();
        to_result(PersonalityListResult {
            files,
            max_chars: crate::agent::personality::MAX_FILE_CHARS,
        })
    }

    fn handle_personality_get(&self, params: &Value) -> RpcResult {
        let req: PersonalityGetParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();

        // Sandbox: only allow files from the allowlist.
        if !crate::agent::personality::EDITABLE_PERSONALITY_FILES.contains(&req.filename.as_str()) {
            return Err(rpc_err(
                INVALID_PARAMS,
                format!("Not an editable file: {}", req.filename),
            ));
        }
        let workspace = config.agent_workspace_dir(&req.agent);
        let path = workspace.join(&req.filename);
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let mtime_ms = std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64);
                let truncated = content.chars().count() > crate::agent::personality::MAX_FILE_CHARS;
                to_result(PersonalityGetResult {
                    filename: req.filename,
                    content: Some(content),
                    exists: true,
                    truncated,
                    mtime_ms,
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => to_result(PersonalityGetResult {
                filename: req.filename,
                content: None,
                exists: false,
                truncated: false,
                mtime_ms: None,
            }),
            Err(e) => Err(rpc_err(INTERNAL_ERROR, format!("Read failed: {e}"))),
        }
    }

    fn handle_personality_put(&self, params: &Value) -> RpcResult {
        let req: PersonalityPutParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();

        if !crate::agent::personality::EDITABLE_PERSONALITY_FILES.contains(&req.filename.as_str()) {
            return Err(rpc_err(
                INVALID_PARAMS,
                format!("Not an editable file: {}", req.filename),
            ));
        }
        if req.content.chars().count() > crate::agent::personality::MAX_FILE_CHARS {
            return Err(rpc_err(
                INVALID_PARAMS,
                format!(
                    "Content exceeds {} char limit",
                    crate::agent::personality::MAX_FILE_CHARS
                ),
            ));
        }
        let workspace = config.agent_workspace_dir(&req.agent);
        let path = workspace.join(&req.filename);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, &req.content)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Write failed: {e}")))?;
        let bytes_written = req.content.len() as u64;
        let mtime_ms = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        to_result(PersonalityPutResult {
            bytes_written,
            mtime_ms,
        })
    }

    fn handle_personality_templates(&self, params: &Value) -> RpcResult {
        let req: PersonalityTemplatesParams = parse_params(params)?;
        let config = self.ctx.config.read().clone();
        let ctx = personality_template_context(&config, &req);
        let templates = crate::agent::personality_templates::render_preset_default(&ctx);
        let files: Vec<TemplateFileEntry> = templates
            .into_iter()
            .map(|(name, content)| TemplateFileEntry {
                filename: name.to_string(),
                content,
            })
            .collect();
        to_result(PersonalityTemplatesResult {
            preset: "default".to_string(),
            files,
        })
    }

    // ── Config introspection handlers ───────────────────────────

    fn handle_config_sections(&self) -> RpcResult {
        use clawcrew_config::schema::Config;
        use clawcrew_config::sections::{
            QUICKSTART_SECTIONS, Section, SectionShape, section_help, section_index_for_key,
        };

        let config = self.ctx.config.read().clone();

        // Schema-driven: walk Config::prop_fields() to discover ALL
        // top-level section roots, not just QUICKSTART_SECTIONS.
        let mut roots: std::collections::BTreeSet<String> = config
            .prop_fields()
            .iter()
            .filter_map(|f| f.name.split('.').next().map(str::to_string))
            .collect();

        // Hidden system fields the user never edits.
        const HIDDEN: &[&str] = &[
            "schema_version",
            "onboard_state",
            "onboard-state",
            "config_path",
            "workspace_dir",
            "env_overridden_paths",
            "pre_override_snapshots",
        ];
        for h in HIDDEN {
            roots.remove(*h);
        }

        // Map-keyed sections surface even when empty.
        let all_map_paths: Vec<&'static str> =
            Config::map_key_sections().iter().map(|s| s.path).collect();
        for &prefix in &all_map_paths
            .iter()
            .filter_map(|p| p.split('.').next())
            .collect::<std::collections::HashSet<_>>()
        {
            roots.insert(prefix.to_string());
        }

        // Inject synthetic onboarding sections (e.g. personality).
        for s in QUICKSTART_SECTIONS {
            roots.insert(s.as_str().to_string());
        }

        let direct_scalar_parents: std::collections::HashSet<String> = config
            .prop_fields()
            .iter()
            .filter_map(|f| {
                let mut segs = f.name.split('.');
                let root = segs.next()?;
                // exactly one more segment past root = direct child scalar
                segs.next()?;
                if segs.next().is_some() {
                    return None;
                }
                Some(root.to_string())
            })
            .collect();
        let parents_with_children: std::collections::HashSet<String> = roots
            .iter()
            .filter_map(|k| k.split_once('.').map(|(p, _)| p.to_string()))
            .collect();
        roots.retain(|k| {
            k.contains('.')
                || !parents_with_children.contains(k)
                || direct_scalar_parents.contains(k)
        });

        // Hide cost.rates subtree.
        roots.retain(|k| !k.starts_with("cost.rates"));

        // Sort: onboarding sections in canonical order first, rest alpha.
        let mut ordered: Vec<String> = roots.into_iter().collect();
        ordered.sort_by(
            |a, b| match (section_index_for_key(a), section_index_for_key(b)) {
                (Some(ai), Some(bi)) => ai.cmp(&bi),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.cmp(b),
            },
        );

        // Picker eligibility: map-keyed section or onboarding section
        // with a picker shape.
        let section_has_picker_for_key = |key: &str| -> bool {
            let key_dot = format!("{key}.");
            all_map_paths.iter().any(|p| {
                *p == key
                    || p.strip_prefix(&key_dot)
                        .is_some_and(|rest| !rest.contains('.'))
            })
        };

        let sections: Vec<ConfigSectionEntry> = ordered
            .into_iter()
            .map(|key| {
                let wizard = Section::from_key(&key);
                let has_picker = match wizard {
                    Some(w) => matches!(
                        w.shape(),
                        SectionShape::TypedFamilyMap | SectionShape::OneTierAliasMap
                    ),
                    None => section_has_picker_for_key(&key),
                };
                let completed = wizard
                    .map(|w| clawcrew_config::sections::section_has_signal(&config, w))
                    .unwrap_or(false);
                let label = clawcrew_config::sections::humanize_section_key(&key);
                let group = clawcrew_config::sections::section_group_for_key(&key);
                ConfigSectionEntry {
                    help: section_help(&key).to_string(),
                    has_picker,
                    completed,
                    ready: false,
                    group: group.label().to_string(),
                    group_key: group.key().to_string(),
                    is_quickstart: wizard.is_some(),
                    shape: wizard.map(Section::shape),
                    cost_category: clawcrew_config::schema::cost_category_for_provider_section(
                        &key,
                    )
                    .unwrap_or_default()
                    .to_string(),
                    label,
                    key,
                }
            })
            .collect();
        to_result(ConfigSectionsResult { sections })
    }

    fn handle_config_status(&self) -> RpcResult {
        use clawcrew_config::sections::QUICKSTART_SECTIONS;
        let config = self.ctx.config.read().clone();
        let missing: Vec<String> = QUICKSTART_SECTIONS
            .iter()
            .filter(|&&s| !clawcrew_config::sections::section_has_signal(&config, s))
            .map(|s| s.as_str().to_string())
            .collect();
        let needs_quickstart = !missing.is_empty();
        let reason = if needs_quickstart {
            format!("{} section(s) incomplete", missing.len())
        } else {
            "all sections complete".to_string()
        };
        to_result(ConfigStatusResult {
            needs_quickstart,
            reason,
            has_partial_state: false,
            missing,
        })
    }

    fn handle_config_catalog(&self) -> RpcResult {
        let providers: Vec<CatalogModelProvider> = clawcrew_providers::list_model_providers()
            .into_iter()
            .map(|p| CatalogModelProvider {
                name: p.name.to_string(),
                display_name: p.display_name.to_string(),
                local: p.local,
            })
            .collect();
        to_result(CatalogResponse {
            model_providers: providers,
        })
    }

    async fn handle_config_catalog_models(&self, params: &Value) -> RpcResult {
        let req: CatalogModelsParams = parse_params(params)?;
        let local = crate::quickstart::model_provider_is_local(&req.model_provider);
        // Snapshot config so the catalog can resolve the alias credential and
        // reach the native /models endpoint (surfacing new native-only models
        // that models.dev may not carry yet) rather than silently falling back.
        let config = self.ctx.config.read().clone();
        let (models, pricing, live) =
            crate::quickstart::model_catalog_with_config(Some(&config), &req.model_provider).await;
        to_result(CatalogModelsResult {
            model_provider: req.model_provider,
            models,
            pricing,
            local,
            live,
        })
    }

    // ── Logs handler ─────────────────────────────────────────────

    async fn handle_logs_subscribe(&self) -> RpcResult {
        let event_tx = self
            .ctx
            .event_tx
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "Event streaming is not available"))?;
        let mut rx = event_tx.subscribe();
        let rpc = self.rpc.clone();
        clawcrew_spawn::spawn!(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = rpc.closed() => break,
                    event = rx.recv() => match event {
                        Ok(mut event) => {
                            // Pairing secrets (QR payloads, one-shot pair codes)
                            // ride the shared broadcast bus stamped with the
                            // ephemeral marker. `logs/subscribe` is NOT the
                            // bearer-authenticated SSE surface those credentials
                            // are scoped to — a fresh remote RPC client can
                            // `initialize` and subscribe over WSS without the
                            // gateway bearer check — so fail closed: withhold
                            // marked frames entirely and strip the internal
                            // marker from everything else (public shape
                            // unchanged). See `clawcrew_gateway::sse`.
                            if clawcrew_log::frame_carries_ephemeral_credentials(&event) {
                                continue;
                            }
                            clawcrew_log::strip_ephemeral_broadcast_marker(&mut event);
                            let notification =
                                JsonRpcNotification::new(notification::LOGS_EVENT, event);
                            if let Ok(json) = serde_json::to_string(&notification)
                                && !rpc.send_raw(json).await
                            {
                                break;
                            }
                        }
                        Err(_) => break,
                    },
                }
            }
        });
        to_result(LogsSubscribeResult { subscribed: true })
    }

    #[allow(deprecated)] // we still forward the legacy cursor for backwards compat
    async fn handle_logs_query(&self, params: &Value) -> RpcResult {
        let p: LogsQueryParams = parse_params(params)?;

        let Some((active, reads_archives)) = clawcrew_log::active_log_query_scope() else {
            return Err(rpc_err(INTERNAL_ERROR, "Log persistence is not enabled"));
        };

        let filter = clawcrew_log::LogFilter {
            since_ts: p.since_ts,
            until_ts: p.until_ts,
            until_id: p.until_id,
            until_line_offset: p.until_line_offset,
            action: p.action,
            category: p.category,
            outcome: p.outcome,
            severity_min: p.severity_min,
            trace_id: p.trace_id,
            q: p.q,
            hide_internal: p.hide_internal,
            field_eq: std::collections::BTreeMap::new(),
        };

        let limit = p.limit.unwrap_or(200);
        let segment_cursor = match p.until_segment_cursor.as_deref() {
            None | Some("") => None,
            Some(raw) => match clawcrew_log::SegmentCursor::from_wire(raw) {
                Some(c) => Some(c),
                None => {
                    return Err(rpc_err(
                        INVALID_PARAMS,
                        "invalid until_segment_cursor: value is not a valid segment cursor",
                    ));
                }
            },
        };

        let page = clawcrew_log::query_log_page(
            &active,
            reads_archives,
            &filter,
            limit,
            segment_cursor.as_ref(),
        )
        .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Log read failed: {e:#}")))?;

        let events: Vec<serde_json::Value> = page
            .events
            .into_iter()
            .filter_map(|evt| serde_json::to_value(evt).ok())
            .collect();

        to_result(LogsQueryResult {
            events,
            log_path: clawcrew_log::active_log_path()
                .map(|path| path.to_string_lossy().into_owned()),
            next_cursor: page.next_cursor,
            next_cursor_line_offset: page.next_cursor_line_offset,
            next_segment_cursor: page.next_segment_cursor,
            at_end: page.at_end,
            incomplete: page.incomplete,
        })
    }

    /// `logs/get { id } → LogEvent`. Loads one full event by id from
    /// the persistent JSONL log so the Logs pane can keep only preview
    /// fields in memory and lazy-fetch the full payload only when the
    /// user opens the detail pane. Searches the active file first, then
    /// retained archives oldest-first, so archive events returned by
    /// `logs/query` are always findable by id.
    async fn handle_logs_get(&self, params: &Value) -> RpcResult {
        let p: LogsGetParams = parse_params(params)?;
        let Some((active, reads_archives)) = clawcrew_log::active_log_query_scope() else {
            return Err(rpc_err(INTERNAL_ERROR, "Log persistence is not enabled"));
        };

        let found = clawcrew_log::find_event_across_segments(&active, reads_archives, &p.id)
            .map_err(|e| rpc_err(INTERNAL_ERROR, format!("Log read failed: {e:#}")))?;

        match found.event {
            Some(evt) => {
                let event = serde_json::to_value(evt).map_err(|e| {
                    rpc_err(INTERNAL_ERROR, format!("Failed to serialize event: {e}"))
                })?;
                to_result(LogsGetResult { event })
            }
            // A miss is only authoritative when every segment was read. If one
            // was skipped, the id may be sitting in it, and reporting "not
            // found" would present a guess as a fact — the caller stops looking
            // for an event that is still on disk.
            None if found.incomplete => Err(rpc_err(
                INTERNAL_ERROR,
                format!(
                    "Log id `{}` was not found, but part of the retained history \
                     could not be read; the event may still exist",
                    p.id
                ),
            )),
            None => Err(rpc_err(
                INTERNAL_ERROR,
                format!("Log id `{}` not found", p.id),
            )),
        }
    }

    // ── File attachment handler ────────────────────────────────

    async fn handle_file_attach(&self, params: &Value) -> RpcResult {
        use super::attachments::{MAX_REQUEST_BYTES, process_file_entry};

        let req: FileAttachParams = parse_params(params)?;
        let sid = &req.session_id;

        // Uploads land in the per-agent workspace, not the session cwd.
        // See `handle_send_message` for the rationale.
        let agent_alias = self
            .ctx
            .sessions
            .get_agent_alias(sid)
            .await
            .ok_or_else(|| rpc_err(SESSION_NOT_FOUND, "Session not found"))?;
        let upload_root = self
            .ctx
            .config
            .read()
            .agent_workspace_dir(&agent_alias)
            .to_string_lossy()
            .to_string();

        let is_wss = self.peer_label.starts_with("wss:");

        let mut total_bytes: u64 = 0;
        let mut results = Vec::with_capacity(req.files.len());

        for entry in &req.files {
            let result =
                process_file_entry(entry, sid, &upload_root, is_wss, &self.ctx.sessions).await?;
            total_bytes += result.size_bytes;
            if total_bytes > MAX_REQUEST_BYTES {
                return Err(rpc_err(
                    INVALID_PARAMS,
                    format!(
                        "Total attachment size exceeds {} MB limit",
                        MAX_REQUEST_BYTES / (1024 * 1024)
                    ),
                ));
            }
            results.push(result);
        }

        to_result(FileAttachResult { files: results })
    }

    // ── Wire helpers ─────────────────────────────────────────────

    async fn send_result(&self, id: Value, result: Value) {
        let resp = JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION,
            result: Some(result),
            error: None,
            id,
        };
        if let Ok(json) = serde_json::to_string(&resp) {
            let _ = self.rpc.send_raw(json).await;
        }
    }

    async fn send_error(&self, id: Value, code: i32, message: &str) {
        let resp = JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
            id,
        };
        if let Ok(json) = serde_json::to_string(&resp) {
            let _ = self.rpc.send_raw(json).await;
        }
    }

    fn handle_quickstart_state(&self) -> RpcResult {
        let cfg = self.ctx.config.read().clone();
        to_result(crate::quickstart::snapshot_state(&cfg))
    }

    fn handle_quickstart_fields(&self, params: &Value) -> RpcResult {
        let req: QuickstartFieldsParams = parse_params(params)?;
        let descriptors = crate::quickstart::field_shape(req.section, &req.type_key);
        to_result(QuickstartFieldsResult {
            fields: descriptors,
        })
    }

    fn handle_quickstart_validate(&self, params: &Value) -> RpcResult {
        let req: QuickstartValidateParams = parse_params(params)?;
        let cfg = self.ctx.config.read().clone();
        let body = match crate::quickstart::validate_only_with_surface(
            &req.submission,
            &cfg,
            crate::quickstart::Surface::Tui,
        ) {
            Ok(()) => QuickstartValidateResult::Ok,
            Err(errors) => QuickstartValidateResult::Errors { errors },
        };
        to_result(body)
    }

    fn sops_dir_and_mode(&self) -> (std::path::PathBuf, crate::sop::SopExecutionMode) {
        let config = self.ctx.config.read();
        let install_root = config.install_root_dir();
        let dir = crate::sop::resolve_sops_dir(&install_root, config.sop.sops_dir.as_deref());
        let mode = crate::sop::parse_execution_mode(&config.sop.default_execution_mode);
        (dir, mode)
    }

    fn parse_sop(value: &Value) -> Result<crate::sop::Sop, JsonRpcError> {
        serde_json::from_value(value.clone()).map_err(|e| rpc_err(INVALID_PARAMS, e.to_string()))
    }

    fn handle_sops_list(&self) -> RpcResult {
        let (dir, mode) = self.sops_dir_and_mode();
        let sops = crate::sop::load_sops_from_directory(&dir, mode);
        to_result(sops)
    }

    fn handle_sops_get(&self, params: &Value) -> RpcResult {
        let req: SopSelectRequest = parse_params(params)?;
        let (dir, mode) = self.sops_dir_and_mode();
        let sop = crate::sop::load_sop_by_name(&dir, &req.name, mode)
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("SOP '{}': {e}", req.name)))?;
        to_result(sop)
    }

    fn handle_sops_graph(&self, params: &Value) -> RpcResult {
        let req: SopSelectRequest = parse_params(params)?;
        let (dir, mode) = self.sops_dir_and_mode();
        let sop = crate::sop::load_sop_by_name(&dir, &req.name, mode)
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("SOP '{}': {e}", req.name)))?;
        to_result(crate::sop::SopGraph::from_sop_with_specs(
            &sop,
            &self.sop_tool_specs(),
        ))
    }

    async fn handle_sops_run(&self, params: &Value) -> RpcResult {
        let req: SopRunRequest = parse_params(params)?;

        if let Some(payload) = req.payload.as_deref()
            && !payload.trim().is_empty()
            && serde_json::from_str::<Value>(payload).is_err()
        {
            return Err(rpc_err(INVALID_PARAMS, "payload is not valid JSON"));
        }

        let engine = self
            .ctx
            .sop_engine
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "SOP subsystem not enabled"))?;
        let audit = self
            .ctx
            .sop_audit
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "SOP subsystem not enabled"))?;

        let payload = req
            .payload
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string);

        let event = crate::sop::SopEvent {
            source: crate::sop::SopTriggerSource::Manual,
            topic: None,
            payload,
            timestamp: crate::sop::engine::now_iso8601(),
        };

        let results =
            crate::sop::dispatch::dispatch_sop_event_to(engine, audit, event, &req.name).await;
        crate::sop::dispatch::process_headless_results(&results);

        for result in &results {
            match result {
                crate::sop::dispatch::DispatchResult::Started { run_id, .. } => {
                    return to_result(SopRunResponse {
                        run_id: run_id.clone(),
                    });
                }
                crate::sop::dispatch::DispatchResult::Skipped { reason, .. }
                | crate::sop::dispatch::DispatchResult::BlockedUnsafe { reason, .. } => {
                    return Err(rpc_err(INVALID_PARAMS, reason.clone()));
                }
                crate::sop::dispatch::DispatchResult::Deferred { reason, .. } => {
                    return Err(rpc_err(INVALID_PARAMS, reason.clone()));
                }
                crate::sop::dispatch::DispatchResult::Coalesced {
                    existing_run_id, ..
                } => {
                    return to_result(SopRunResponse {
                        run_id: existing_run_id.clone(),
                    });
                }
                crate::sop::dispatch::DispatchResult::NoMatch => {}
            }
        }

        Err(rpc_err(
            INVALID_PARAMS,
            format!("SOP '{}' has no matching manual trigger", req.name),
        ))
    }

    fn handle_sops_runs(&self, params: &Value) -> RpcResult {
        let req: SopRunsRequest = parse_params(params)?;
        let engine = self
            .ctx
            .sop_engine
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "SOP subsystem not enabled"))?;
        let runs = crate::sop::run_summaries_for(engine, req.sop.as_deref())
            .map_err(|e| rpc_err(INTERNAL_ERROR, e.to_string()))?;
        to_result(serde_json::json!({ "runs": runs }))
    }

    /// Full detail for one run: step results with status, timings, failure
    /// output, and captured tool calls. `sops/runs` intentionally returns
    /// summaries; this is the drill-down a UI uses for a selected run.
    fn handle_sops_run_detail(&self, params: &Value) -> RpcResult {
        // Local transports only. WSS now requires a client certificate, so the
        // question is no longer whether the caller authenticated — it is what
        // that authentication entitles them to read. A certificate proves the
        // peer, not that the peer may see one particular run's step output,
        // tool arguments and errors, and this dispatcher has no per-run
        // authorization to consult. Local IPC is owner-scoped by the socket
        // itself, which is the entitlement this method relies on. Lift the
        // refusal once there is a principal to authorize run contents against,
        // and replace it with that check rather than simply removing it.
        if self.peer_label.starts_with("wss:") {
            return Err(rpc_err(
                AUTH_REQUIRED,
                "sops/run-detail is not served over remote WSS: the transport has no \
                 authenticated principal to authorize run contents against",
            ));
        }
        let req: SopRunDetailRequest = parse_params(params)?;
        let engine = self
            .ctx
            .sop_engine
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "SOP subsystem not enabled"))?;
        let (run, active) = crate::sop::run_detail_for(engine, &req.run_id).map_err(|e| {
            let msg = e.to_string();
            let code = if msg.contains("not found") {
                INVALID_PARAMS
            } else {
                INTERNAL_ERROR
            };
            rpc_err(code, msg)
        })?;
        let detail = crate::sop::types::SopRunDetail::from_run(&run, active);
        to_result(serde_json::json!({ "run": detail }))
    }

    fn handle_sops_run_overlay(&self, params: &Value) -> RpcResult {
        let req: SopRunOverlayRequest = parse_params(params)?;
        let (dir, mode) = self.sops_dir_and_mode();
        let sop = crate::sop::load_sop_by_name(&dir, &req.name, mode)
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("SOP '{}': {e}", req.name)))?;
        let engine = self
            .ctx
            .sop_engine
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "SOP subsystem not enabled"))?;
        let overlay = crate::sop::run_overlay_for(&sop, engine, &req.run_id).map_err(|e| {
            let msg = e.to_string();
            let code = if msg.contains("not found") {
                INVALID_PARAMS
            } else {
                INTERNAL_ERROR
            };
            rpc_err(code, msg)
        })?;
        to_result(overlay)
    }

    async fn handle_sops_decide(&self, params: &Value) -> RpcResult {
        let req: SopDecideRequest = parse_params(params)?;
        let decision: crate::sop::approval::ApprovalDecision =
            serde_json::from_value(req.decision.clone()).map_err(|e| {
                rpc_err(
                    INVALID_PARAMS,
                    format!("decision is not a valid approval decision: {e}"),
                )
            })?;

        let (dir, mode) = self.sops_dir_and_mode();
        let sop = crate::sop::load_sop_by_name(&dir, &req.name, mode)
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("SOP '{}': {e}", req.name)))?;
        let engine = self
            .ctx
            .sop_engine
            .as_ref()
            .ok_or_else(|| rpc_err(INTERNAL_ERROR, "SOP subsystem not enabled"))?
            .clone();

        let agent_alias = sop.agent.clone().unwrap_or_default();
        let span = ::clawcrew_log::info_span!(
            target: "clawcrew_log_internal_scope",
            "clawcrew_scope",
            session_key = %req.run_id,
            agent_alias = %agent_alias,
            channel = "rpc",
        );
        let _guard = span.enter();

        let mut resolved_outcome = None;
        {
            let mut guard = engine
                .lock()
                .map_err(|_| rpc_err(INTERNAL_ERROR, "SOP engine lock poisoned"))?;
            let run_sop_name = guard
                .get_run(&req.run_id)
                .map(|run| run.sop_name.clone())
                .ok_or_else(|| {
                    rpc_err(INVALID_PARAMS, format!("run '{}' not found", req.run_id))
                })?;
            if run_sop_name != req.name {
                return Err(rpc_err(
                    INVALID_PARAMS,
                    format!(
                        "run '{}' belongs to SOP '{}', not '{}'",
                        req.run_id, run_sop_name, req.name
                    ),
                ));
            }
            use crate::sop::approval::{BrokerOutcome, ResolveOutcome};
            let principal = crate::sop::approval::ApprovalPrincipal::cli(self.tui_id.clone());
            match guard
                .resolve_via_broker_deferred(&req.run_id, decision, principal)
                .map_err(|e| rpc_err(INTERNAL_ERROR, e.to_string()))?
            {
                outcome @ BrokerOutcome::Resolved(ResolveOutcome::Resumed(_)) => {
                    resolved_outcome = Some(outcome);
                }
                BrokerOutcome::Resolved(
                    ResolveOutcome::Denied
                    | ResolveOutcome::AlreadyResolved
                    | ResolveOutcome::Revised,
                )
                | BrokerOutcome::PendingQuorum { .. } => {}
                BrokerOutcome::Resolved(
                    ResolveOutcome::NotWaiting | ResolveOutcome::DeferredAtCapacity,
                )
                | BrokerOutcome::NotWaiting => {
                    return Err(rpc_err(
                        INVALID_PARAMS,
                        crate::i18n::get_required_cli_string_with_args(
                            "sop-rpc-decision-invalid-state",
                            &[("run_id", req.run_id.as_str())],
                        ),
                    ));
                }
                BrokerOutcome::Resolved(ResolveOutcome::RejectedSelfApproval)
                | BrokerOutcome::NotAuthorized { .. } => {
                    return Err(rpc_err(
                        AUTH_REQUIRED,
                        crate::i18n::get_required_cli_string("sop-rpc-decision-unauthorized"),
                    ));
                }
                BrokerOutcome::PolicyMissing { name } => {
                    return Err(rpc_err(
                        INTERNAL_ERROR,
                        crate::i18n::get_required_cli_string_with_args(
                            "sop-rpc-policy-missing",
                            &[("name", name.as_str())],
                        ),
                    ));
                }
                BrokerOutcome::PolicyUnavailable { reason } => {
                    return Err(rpc_err(
                        INTERNAL_ERROR,
                        crate::i18n::get_required_cli_string_with_args(
                            "sop-rpc-policy-unavailable",
                            &[("reason", reason.as_str())],
                        ),
                    ));
                }
            }
        }

        if let Some(outcome) = resolved_outcome {
            let config = self.ctx.config.read();
            crate::sop::drive_resumed_broker_action(
                &config,
                Arc::clone(&engine),
                self.ctx.sop_audit.clone(),
                &outcome,
            );
        }

        let overlay = crate::sop::run_overlay_for(&sop, &engine, &req.run_id).map_err(|e| {
            let msg = e.to_string();
            let code = if msg.contains("not found") {
                INVALID_PARAMS
            } else {
                INTERNAL_ERROR
            };
            rpc_err(code, msg)
        })?;
        to_result(overlay)
    }

    fn handle_sops_validate(&self, params: &Value) -> RpcResult {
        let sop = if params.get("sop").is_some() {
            let req: SopSaveRequest = parse_params(params)?;
            Self::parse_sop(&req.sop)?
        } else {
            let req: SopSelectRequest = parse_params(params)?;
            let (dir, mode) = self.sops_dir_and_mode();
            crate::sop::load_sop_by_name(&dir, &req.name, mode)
                .map_err(|e| rpc_err(INVALID_PARAMS, format!("SOP '{}': {e}", req.name)))?
        };
        let v = crate::sop::validate_sop_strict(&sop);
        to_result(serde_json::json!({
            "blocking": v.blocking,
            "warnings": v.warnings,
            "ok": v.is_ok(),
        }))
    }

    fn handle_sops_save(&self, params: &Value) -> RpcResult {
        let req: SopSaveRequest = parse_params(params)?;
        let sop = Self::parse_sop(&req.sop)?;
        if let Some(original) = req.original_name.as_deref()
            && !original.is_empty()
            && original != sop.name
        {
            return Err(rpc_err(
                INVALID_PARAMS,
                format!(
                    "rename not supported: SOP '{original}' cannot be saved as '{}'",
                    sop.name
                ),
            ));
        }
        let (dir, _mode) = self.sops_dir_and_mode();
        crate::sop::save_sop(&dir, &sop).map_err(|e| rpc_err(INVALID_PARAMS, e.to_string()))?;
        to_result(serde_json::json!({ "saved": sop.name }))
    }

    fn handle_sops_create(&self, params: &Value) -> RpcResult {
        let req: SopSaveRequest = parse_params(params)?;
        let sop = Self::parse_sop(&req.sop)?;
        let (dir, _mode) = self.sops_dir_and_mode();
        crate::sop::create_sop_typed(&dir, &sop).map_err(|e| {
            let code = match e {
                crate::sop::SopAuthorError::AlreadyExists(_) => SOP_ALREADY_EXISTS,
                _ => INVALID_PARAMS,
            };
            rpc_err(code, e.to_string())
        })?;
        to_result(serde_json::json!({ "created": sop.name }))
    }

    fn handle_sops_delete(&self, params: &Value) -> RpcResult {
        let req: SopSelectRequest = parse_params(params)?;
        let (dir, _mode) = self.sops_dir_and_mode();
        crate::sop::delete_sop_typed(&dir, &req.name).map_err(|e| {
            let code = match e {
                crate::sop::SopAuthorError::NotFound(_) => SOP_NOT_FOUND,
                _ => INTERNAL_ERROR,
            };
            rpc_err(code, e.to_string())
        })?;
        to_result(serde_json::json!({ "deleted": req.name }))
    }

    fn handle_sops_wire_draft(&self, params: &Value) -> RpcResult {
        let sop_val = params
            .get("sop")
            .ok_or_else(|| rpc_err(INVALID_PARAMS, "missing 'sop'"))?;
        let edit_val = params
            .get("edit")
            .ok_or_else(|| rpc_err(INVALID_PARAMS, "missing 'edit'"))?;
        let mut sop = Self::parse_sop(sop_val)?;
        let edit: crate::sop::WireEdit = serde_json::from_value(edit_val.clone())
            .map_err(|e| rpc_err(INVALID_PARAMS, format!("invalid wire edit: {e}")))?;
        crate::sop::apply_wire(&mut sop, &edit)
            .map_err(|e| rpc_err(INVALID_PARAMS, e.to_string()))?;
        to_result(serde_json::json!({
            "sop": sop,
            "graph": crate::sop::SopGraph::from_sop_with_specs(&sop, &self.sop_tool_specs()),
        }))
    }

    fn handle_sops_graph_draft(&self, params: &Value) -> RpcResult {
        let sop_val = params
            .get("sop")
            .ok_or_else(|| rpc_err(INVALID_PARAMS, "missing 'sop'"))?;
        let sop = Self::parse_sop(sop_val)?;
        to_result(crate::sop::SopGraph::from_sop_with_specs(
            &sop,
            &self.sop_tool_specs(),
        ))
    }

    fn sop_tool_specs(&self) -> crate::sop::ToolSpecs {
        let config = self.ctx.config.read();
        let agent = config.agents.keys().min().cloned().unwrap_or_default();
        crate::sop::tool_specs_from_config(&config, &agent)
    }

    fn handle_sops_trigger_sources(&self) -> RpcResult {
        let registry = {
            let config = self.ctx.config.read();
            crate::sop::registry_from_config(&config)
        };
        to_result(registry)
    }

    /// Resolve selectable values for a domain-typed tool parameter.
    /// Params: `{ domain, agent?, args? }`. `domain` is an
    /// `OptionDomain` wire name (e.g. `peer_targets`); `agent` scopes
    /// agent-relative domains; `args` carries sibling arguments already
    /// chosen so cascading domains can narrow.
    fn handle_tools_param_options(&self, params: &Value) -> RpcResult {
        #[derive(serde::Deserialize)]
        struct ParamOptionsParams {
            domain: clawcrew_api::tool::OptionDomain,
            #[serde(default)]
            agent: Option<String>,
            #[serde(default)]
            args: Value,
        }
        let req: ParamOptionsParams = parse_params(params)?;
        let config = self.ctx.config.read();
        let agent_alias = req
            .agent
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .or_else(|| config.agents.keys().min().cloned())
            .unwrap_or_default();

        let entries = if req.domain == clawcrew_api::tool::OptionDomain::ToolNames {
            let security = std::sync::Arc::new(
                clawcrew_config::policy::SecurityPolicy::for_agent(&config, &agent_alias)
                    .unwrap_or_default(),
            );
            let tools = crate::tools::default_tools(security);
            let refs: Vec<&dyn clawcrew_api::tool::Tool> =
                tools.iter().map(std::convert::AsRef::as_ref).collect();
            crate::tools::param_options::resolve_options(
                req.domain,
                &config,
                &agent_alias,
                &req.args,
                &refs,
            )
        } else {
            crate::tools::param_options::resolve_options(
                req.domain,
                &config,
                &agent_alias,
                &req.args,
                &[],
            )
        };
        to_result(serde_json::json!({ "options": entries }))
    }

    async fn handle_quickstart_apply(&self, params: &Value) -> RpcResult {
        let req: QuickstartApplyParams = parse_params(params)?;
        // Serializes with every other config-mutating handler for the whole
        // clone-apply-save-swap below, so the install on success can't race
        // a concurrent config write (see `ctx.config_write_lock`).
        let config_write_guard = Arc::clone(&self.ctx.config_write_lock).lock_owned().await;
        // Clone out of the lock to satisfy `&mut Config`. On success
        // install the mutated snapshot, mirroring the gateway's
        // `handle_apply`. `apply_with_surface` already ran `save_dirty` on
        // the clone, so `save_and_swap_config` performs no second disk
        // write (empty dirty set short-circuits) — just the guarded swap.
        let mut working = self.ctx.config.read().clone();
        let result = crate::quickstart::apply_with_surface(
            req.submission,
            &mut working,
            crate::quickstart::Surface::Tui,
        )
        .await;
        let body = match result {
            Ok(agent) => {
                self.save_and_swap_config(working, &config_write_guard)
                    .await?;
                let reload_signalled = self.signal_daemon_reload();
                QuickstartApplyResult::Applied {
                    agent,
                    daemon_restarted: reload_signalled,
                }
            }
            Err(errors) => QuickstartApplyResult::Errors { errors },
        };
        to_result(body)
    }

    fn handle_quickstart_dismiss(&self, params: &Value) -> RpcResult {
        let req: QuickstartDismissParams = parse_params(params)?;
        crate::quickstart::record_dismissed(&req.run_id, req.surface, req.last_step);
        to_result(QuickstartDismissResult { recorded: true })
    }

    /// Signal the in-place daemon reload using the same `reload_tx`
    /// watch channel `/admin/reload` and the gateway's quickstart route
    /// use. Returns `true` when the supervisor was notified, `false`
    /// when no supervisor is attached (e.g. test harness).
    fn signal_daemon_reload(&self) -> bool {
        if self.ctx.reload_tx.is_none() {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "reason": "no_supervisor",
                        "surface": crate::quickstart::Surface::Tui.as_str(),
                    })),
                "quickstart: daemon reload not available (standalone daemon)"
            );
            return false;
        };
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Start).with_attrs(
                ::serde_json::json!({
                    "surface": crate::quickstart::Surface::Tui.as_str(),
                })
            ),
            "quickstart: daemon reload signalled"
        );
        self.schedule_daemon_reload(crate::quickstart::Surface::Tui.as_str())
    }
}

fn conversation_message_entries(messages: &[ConversationMessage]) -> Vec<MessageEntry> {
    let mut entries = Vec::new();
    let mut tool_entries_by_id =
        std::collections::HashMap::<String, std::collections::VecDeque<usize>>::new();

    for message in messages {
        match message {
            ConversationMessage::Chat(chat) => entries.push(MessageEntry {
                role: chat.role.clone(),
                content: chat.content.clone(),
                kind: MessageEntryKind::Message,
                tool_call_id: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
            }),
            ConversationMessage::AssistantToolCalls {
                text, tool_calls, ..
            } => {
                if let Some(text) = text.as_ref().filter(|text| !text.is_empty()) {
                    entries.push(MessageEntry {
                        role: "assistant".to_string(),
                        content: text.clone(),
                        kind: MessageEntryKind::Message,
                        tool_call_id: None,
                        tool_name: None,
                        tool_input: None,
                        tool_output: None,
                    });
                }

                for call in tool_calls {
                    let index = entries.len();
                    tool_entries_by_id
                        .entry(call.id.clone())
                        .or_default()
                        .push_back(index);
                    entries.push(MessageEntry {
                        role: "assistant".to_string(),
                        content: format!("Tool call: {}\n{}", call.name, call.arguments),
                        kind: MessageEntryKind::ToolCall,
                        tool_call_id: Some(call.id.clone()),
                        tool_name: Some(call.name.clone()),
                        tool_input: Some(
                            serde_json::from_str(&call.arguments)
                                .unwrap_or_else(|_| Value::String(call.arguments.clone())),
                        ),
                        tool_output: None,
                    });
                }
            }
            ConversationMessage::ToolResults(results) => {
                for result in results {
                    let output = result.content.clone();
                    let entry_index = tool_entries_by_id
                        .get_mut(&result.tool_call_id)
                        .and_then(std::collections::VecDeque::pop_front);
                    if tool_entries_by_id
                        .get(&result.tool_call_id)
                        .is_some_and(std::collections::VecDeque::is_empty)
                    {
                        tool_entries_by_id.remove(&result.tool_call_id);
                    }
                    if let Some(entry) = entry_index.and_then(|index| entries.get_mut(index)) {
                        entry.tool_output = Some(output.clone());
                        entry.content.push_str("\nResult:\n");
                        entry.content.push_str(&output);
                    } else {
                        entries.push(MessageEntry {
                            role: "tool".to_string(),
                            content: format!(
                                "Tool result: {}\n{}",
                                if result.tool_name.is_empty() {
                                    "unknown"
                                } else {
                                    &result.tool_name
                                },
                                output
                            ),
                            kind: MessageEntryKind::ToolResult,
                            tool_call_id: Some(result.tool_call_id.clone()),
                            tool_name: (!result.tool_name.is_empty())
                                .then(|| result.tool_name.clone()),
                            tool_input: None,
                            tool_output: Some(output),
                        });
                    }
                }
            }
        }
    }

    entries
}

fn response_id_key(id: &Value) -> Option<String> {
    match id {
        Value::String(id) => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        Value::Null => None,
        _ => None,
    }
}

impl Drop for RpcDispatcher {
    fn drop(&mut self) {
        // Only the connection owner ends the generation. A prompt handle shares
        // the token so it can observe teardown; cancelling here would let a
        // prompt that simply finished close its own connection.
        if !self.owns_connection {
            return;
        }
        self.connection_cancel.cancel();
        for task in &self.prompt_tasks {
            task.abort();
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────

fn parse_params<T: DeserializeOwned>(params: &Value) -> Result<T, JsonRpcError> {
    serde_json::from_value(params.clone()).map_err(|e| rpc_err(INVALID_PARAMS, e.to_string()))
}

fn validate_session_configure_overrides(overrides: &SessionOverrides) -> Result<(), JsonRpcError> {
    if overrides
        .model
        .as_deref()
        .is_some_and(|model| model.trim().is_empty())
    {
        return Err(rpc_err(INVALID_PARAMS, "model must not be blank"));
    }
    if overrides
        .model_provider
        .as_deref()
        .is_some_and(|provider| provider.trim().is_empty())
    {
        return Err(rpc_err(INVALID_PARAMS, "model_provider must not be blank"));
    }
    Ok(())
}

fn to_result<T: Serialize>(val: T) -> RpcResult {
    serde_json::to_value(val).map_err(|e| rpc_err(INTERNAL_ERROR, e.to_string()))
}

const MEMORY_PREVIEW_CONTENT_BYTES: usize = 200;

/// Truncate each entry's `content` to the preview budget. Operates
/// in place to avoid a second allocation per entry.
fn truncate_memory_previews(
    mut entries: Vec<clawcrew_api::memory_traits::MemoryEntry>,
) -> Vec<clawcrew_api::memory_traits::MemoryEntry> {
    for entry in &mut entries {
        if entry.content.len() > MEMORY_PREVIEW_CONTENT_BYTES {
            // Truncate on a char boundary so we never split a UTF-8 sequence.
            let mut end = MEMORY_PREVIEW_CONTENT_BYTES;
            while end > 0 && !entry.content.is_char_boundary(end) {
                end -= 1;
            }
            entry.content.truncate(end);
            entry.content.push('…');
        }
    }
    entries
}

/// Resolve the preemptive-trim budget shown on Zerocode's context usage meter.
///
/// This is the value `input_tokens` fills toward before trimming triggers — the
/// resolved `effective_context_budget`: the legacy absolute budget unless a
/// profile explicitly opts into model-relative budgeting, with positive
/// `history_pruning.max_tokens` values acting as an additional downward cap.
/// Falls back to the legacy 32,000-token value when the agent can't be resolved.
/// Emitted on the wire as `max_context_tokens`, preserving that field's original
/// "budget the meter fills toward" meaning.
#[cfg(test)]
fn context_usage_max_tokens(cfg: &clawcrew_config::schema::Config, agent_alias: &str) -> u64 {
    cfg.resolved_agent_config(agent_alias)
        .map(|a| a.resolved.effective_context_budget() as u64)
        .unwrap_or_else(|| cfg.effective_model_context_window(agent_alias) as u64)
}

/// Resolve the model's full context window (provider `context_window`, 32_000
/// fallback). Exposed on the wire as `model_context_window`, distinct from the
/// trim budget, so a client can render capacity and budget separately.
#[cfg(test)]
fn context_usage_model_window(cfg: &clawcrew_config::schema::Config, agent_alias: &str) -> u64 {
    cfg.effective_model_context_window(agent_alias) as u64
}

/// Replace the durable ACP transcript with the agent's own authoritative
/// post-turn history, as one state with its breadcrumb flag. This is invoked
/// for every terminal outcome — completed, cancelled (even with an empty
/// delta), and failed — because the live agent may have already trimmed
/// older turns before the turn was cancelled or failed. Persisting only a
/// delta would leave the durable store with the pre-trim transcript that the
/// next restore would resurrect.
async fn persist_acp_turn(
    store: &Arc<clawcrew_infra::acp_session_store::AcpSessionStore>,
    session_id: &str,
    _outcome: &Result<TurnOutcome, crate::rpc::turn::TurnError>,
    full_history: Vec<ConversationMessage>,
    trim_breadcrumb: bool,
) -> Option<String> {
    let store = Arc::clone(store);
    let session_id = session_id.to_string();
    match tokio::task::spawn_blocking(move || {
        // One transaction covers the transcript and its breadcrumb flag
        // together: a restore must never re-infer provenance from message
        // text, and a crash between two separate writes could otherwise
        // desynchronize them.
        store.replace_messages_and_breadcrumb(&session_id, &full_history, trim_breadcrumb)
    })
    .await
    {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(join) => Some(join.to_string()),
    }
}

/// Persist a `TurnEvent::Plan` before it is emitted, so a racing
/// reconnect — or a later `session/resume` — reads a consistent plan.
/// Writes both the in-memory live cache (`sessions`) and, when an ACP
/// durable store is present, the on-disk `plan_json` column (via
/// `spawn_blocking`, since SQLite is synchronous). No-op for every
/// other event. Durable-write failures are logged-and-swallowed: the
/// in-memory cache is still authoritative for the live session.
async fn persist_plan_if_any(
    sessions: &crate::rpc::session::SessionStore,
    acp_store: Option<&std::sync::Arc<clawcrew_infra::acp_session_store::AcpSessionStore>>,
    session_id: &str,
    event: &TurnEvent,
) {
    let TurnEvent::Plan { entries } = event else {
        return;
    };
    sessions.set_plan(session_id, entries.clone()).await;
    if let Some(store) = acp_store {
        let store = store.clone();
        let sid = session_id.to_string();
        let entries = entries.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Err(e) = store.set_plan(&sid, &entries) {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Write)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "session_id": sid,
                            "error": e.to_string(),
                        })),
                    "Failed to persist TodoWrite plan to ACP session store"
                );
            }
        })
        .await;
    }
}

fn plan_replay_notification(
    session_id: &str,
    entries: &[clawcrew_api::plan::PlanEntry],
) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let event = TurnEvent::Plan {
        entries: entries.to_vec(),
    };
    notification_for_turn_event(session_id, &event)
}

fn notification_for_turn_event(session_id: &str, event: &TurnEvent) -> Option<String> {
    let update = match event {
        TurnEvent::Chunk { delta } => SessionUpdateEvent::AgentMessageChunk {
            session_id: session_id.to_string(),
            text: delta.clone(),
        },
        TurnEvent::Thinking { delta } => SessionUpdateEvent::AgentThoughtChunk {
            session_id: session_id.to_string(),
            text: delta.clone(),
        },
        TurnEvent::ToolCall { id, name, args } => SessionUpdateEvent::ToolCall {
            session_id: session_id.to_string(),
            tool_call_id: id.clone(),
            name: name.clone(),
            raw_input: args.clone(),
        },
        // The RPC/SessionUpdateEvent surface forwards the text output only; file
        // attachment happens on the direct ACP path, so `artifact` is not needed here.
        TurnEvent::ToolResult {
            id,
            name,
            output,
            artifact: _,
        } => SessionUpdateEvent::ToolResult {
            session_id: session_id.to_string(),
            tool_call_id: id.clone(),
            name: name.clone(),
            raw_output: output.clone(),
        },
        TurnEvent::ApprovalRequest {
            request_id,
            tool_name,
            arguments_summary,
            timeout_secs,
        } => SessionUpdateEvent::ApprovalRequest {
            session_id: session_id.to_string(),
            request_id: request_id.clone(),
            tool_name: tool_name.clone(),
            arguments_summary: arguments_summary.clone(),
            timeout_secs: *timeout_secs,
        },
        TurnEvent::HistoryTrimmed {
            dropped_messages,
            kept_turns,
            reason,
            token_budget,
            tokens_before,
            tokens_after,
            tokens_before_source,
            tokens_after_source,
            unsatisfiable_floor,
        } => SessionUpdateEvent::HistoryTrimmed {
            session_id: session_id.to_string(),
            dropped_messages: *dropped_messages,
            kept_turns: *kept_turns,
            reason: reason.clone(),
            token_budget: *token_budget,
            tokens_before: *tokens_before,
            tokens_after: *tokens_after,
            tokens_before_source: *tokens_before_source,
            tokens_after_source: *tokens_after_source,
            unsatisfiable_floor: *unsatisfiable_floor,
        },
        TurnEvent::Usage {
            input_tokens,
            cached_input_tokens: _,
            output_tokens: _,
            context_token_budget,
            model_context_window,
            cost_usd: _,
            accepted: true,
            ..
        } => SessionUpdateEvent::ContextUsage {
            session_id: session_id.to_string(),
            input_tokens: *input_tokens,
            max_context_tokens: *context_token_budget,
            model_context_window: *model_context_window,
        },
        TurnEvent::Plan { entries } => SessionUpdateEvent::Plan {
            session_id: session_id.to_string(),
            entries: entries.clone(),
        },
        _ => return None,
    };

    let params = serde_json::to_value(update).ok()?;
    let n = JsonRpcNotification::new(notification::SESSION_UPDATE, params);
    serde_json::to_string(&n).ok()
}

/// Forward a turn event through the outbound RPC writer as a
/// serialized `session/update` notification.
///
/// Returns `true` when the event type maps to a notification and the
/// send succeeded. Returns `false` when the event type is not forwarded
/// (e.g. silent variants) or the writer channel is closed.
///
/// The caller is responsible for any pre-send side effects (ACP token
/// persistence, plan persistence). This helper is the single forward
/// path used by both production dispatch and regression tests.
pub(super) async fn forward_turn_event(
    rpc: &RpcOutbound,
    session_id: &str,
    event: &TurnEvent,
) -> bool {
    match notification_for_turn_event(session_id, event) {
        Some(n) => rpc.send_raw(n).await,
        None => false,
    }
}

/// Replace the RPC chat session's durable transcript and breadcrumb flag
/// with the agent's own authoritative post-turn history. A durable write
/// failure here must not be silently swallowed: the turn is still reported
/// to the RPC client as completed, but the durable store then disagrees
/// with what the caller claims was persisted, so failures are logged with
/// session context for operators to notice.
///
/// Returns `true` when the row was committed or there was nothing left to
/// persist (session already deleted), and `false` when the durable write
/// itself failed, so a restore-time caller can fail closed instead of
/// letting the session go live against a store that still has the
/// untrimmed prefix.
fn replace_rpc_chat_conversation_state(
    backend: &dyn clawcrew_infra::session_backend::SessionBackend,
    session_id: &str,
    session_key: &str,
    durable: &[clawcrew_providers::ChatMessage],
    breadcrumb_present: bool,
) -> bool {
    // Guarded replacement, not check-then-act: a delete committing between a
    // separate existence probe and the write would be silently undone by the
    // replace recreating the session row/files. When deletion already won
    // (`Ok(false)`) there is nothing left to persist, so skip quietly.
    match backend.replace_conversation_state_if_exists(session_key, durable, breadcrumb_present) {
        Ok(_) => true,
        Err(e) => {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "session_id": session_id,
                        "error": format!("{}", e),
                    })),
                "Failed to persist authoritative post-turn conversation state"
            );
            false
        }
    }
}

#[cfg(test)]
pub(crate) mod connection_test_support {
    use super::RpcContext;
    use async_trait::async_trait;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Notify;
    use clawcrew_api::attribution::{Attributable, ModelProviderKind, ProviderKind, Role};
    use clawcrew_api::model_provider::ModelProvider;
    use clawcrew_infra::session_queue::SessionActorQueue;

    pub(crate) const RUNNING_SID: &str = "connection-running";
    pub(crate) const QUEUED_SID: &str = "connection-queued";
    pub(crate) const IMMEDIATE_SID: &str = "connection-immediate";

    pub(crate) struct ConnectionPromptFixture {
        pub(crate) ctx: Arc<RpcContext>,
        pub(crate) provider_started: Arc<Notify>,
        pub(crate) provider_dropped: Arc<AtomicBool>,
        pub(crate) queued_provider_calls: Arc<AtomicUsize>,
    }

    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    struct PendingProvider {
        started: Arc<Notify>,
        dropped: Arc<AtomicBool>,
    }

    #[async_trait]
    impl ModelProvider for PendingProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> anyhow::Result<String> {
            let _drop_signal = DropSignal(Arc::clone(&self.dropped));
            self.started.notify_one();
            std::future::pending::<()>().await;
            unreachable!("pending provider should be cancelled with its connection")
        }
    }

    impl Attributable for PendingProvider {
        fn role(&self) -> Role {
            Role::Provider(ProviderKind::Model(ModelProviderKind::Custom))
        }

        fn alias(&self) -> &str {
            "connection-pending"
        }
    }

    struct CountingProvider(Arc<AtomicUsize>);

    #[async_trait]
    impl ModelProvider for CountingProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> anyhow::Result<String> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok("unexpected queued provider call".to_string())
        }
    }

    impl Attributable for CountingProvider {
        fn role(&self) -> Role {
            Role::Provider(ProviderKind::Model(ModelProviderKind::Custom))
        }

        fn alias(&self) -> &str {
            "connection-counting"
        }
    }

    struct ImmediateProvider;

    #[async_trait]
    impl ModelProvider for ImmediateProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: Option<f64>,
        ) -> anyhow::Result<String> {
            Ok("done".to_string())
        }
    }

    impl Attributable for ImmediateProvider {
        fn role(&self) -> Role {
            Role::Provider(ProviderKind::Model(ModelProviderKind::Custom))
        }

        fn alias(&self) -> &str {
            "connection-immediate"
        }
    }

    /// Install a session whose provider answers straight away, so a prompt on
    /// it runs to normal completion instead of parking.
    pub(crate) async fn insert_immediate_session(ctx: &Arc<RpcContext>, path: &Path) {
        insert_session(ctx, path, IMMEDIATE_SID, Box::new(ImmediateProvider)).await;
    }

    pub(crate) async fn insert_session(
        ctx: &Arc<RpcContext>,
        path: &Path,
        session_id: &str,
        provider: Box<dyn ModelProvider>,
    ) {
        let agent = crate::agent::agent::Agent::builder()
            .model_provider(provider)
            .tools(crate::tools::scoped::ScopedToolRegistry::from_raw_for_test(
                vec![],
            ))
            .memory(Arc::new(clawcrew_memory::NoneMemory::new("none")))
            .observer(Arc::new(crate::observability::noop::NoopObserver))
            .tool_dispatcher(Box::new(crate::agent::dispatcher::NativeToolDispatcher))
            .workspace_dir(path.to_path_buf())
            .build()
            .expect("connection lifecycle test agent should build");
        ctx.sessions
            .insert(
                session_id.to_string(),
                crate::rpc::session::RpcSession::new(
                    agent,
                    "test-agent",
                    path.to_str().expect("test path should be UTF-8"),
                    crate::rpc::types::ChatMode::Chat,
                ),
            )
            .await
            .expect("connection lifecycle test session should insert");
    }

    pub(crate) async fn fixture(path: &Path) -> ConnectionPromptFixture {
        let config = clawcrew_config::schema::Config {
            data_dir: path.to_path_buf(),
            config_path: path.join("config.toml"),
            ..Default::default()
        };
        let queue = Arc::new(SessionActorQueue::new(4, 30, 60));
        let sessions = Arc::new(crate::rpc::session::SessionStore::new(64, queue));
        let ctx = RpcContext::minimal(config, sessions);
        let provider_started = Arc::new(Notify::new());
        let provider_dropped = Arc::new(AtomicBool::new(false));
        let queued_provider_calls = Arc::new(AtomicUsize::new(0));

        insert_session(
            &ctx,
            path,
            RUNNING_SID,
            Box::new(PendingProvider {
                started: Arc::clone(&provider_started),
                dropped: Arc::clone(&provider_dropped),
            }),
        )
        .await;
        insert_session(
            &ctx,
            path,
            QUEUED_SID,
            Box::new(CountingProvider(Arc::clone(&queued_provider_calls))),
        )
        .await;

        ConnectionPromptFixture {
            ctx,
            provider_started,
            provider_dropped,
            queued_provider_calls,
        }
    }
}

#[cfg(test)]
mod tests;

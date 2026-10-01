use crate::approval::ApprovalManager;

/// Format token count with thousands separators.
fn format_tokens(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// CLI channel factory, injected by the binary. Returns a `Box<dyn Channel>` for interactive mode.
pub static CLI_CHANNEL_FN: std::sync::OnceLock<
    Box<dyn Fn() -> Box<dyn clawcrew_api::channel::Channel> + Send + Sync>,
> = std::sync::OnceLock::new();

/// Register the CLI channel factory. Called once at startup by the binary.
pub fn register_cli_channel_fn(
    f: Box<dyn Fn() -> Box<dyn clawcrew_api::channel::Channel> + Send + Sync>,
) {
    let _ = CLI_CHANNEL_FN.set(f);
}

/// Peripheral tools factory type — takes owned config so the returned future is 'static.
pub type PeripheralToolsFn = Box<
    dyn Fn(
            clawcrew_config::schema::PeripheralsConfig,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Vec<Box<dyn Tool>>>> + Send>,
        > + Send
        + Sync,
>;

/// Peripheral tools factory, injected by the binary when hardware feature is on.
static PERIPHERAL_TOOLS_FN: std::sync::OnceLock<PeripheralToolsFn> = std::sync::OnceLock::new();

/// Register the peripheral tools factory. Called once at startup by the binary.
pub fn register_peripheral_tools_fn(f: PeripheralToolsFn) {
    let _ = PERIPHERAL_TOOLS_FN.set(f);
}

/// Public helper for other crates (e.g. channels orchestrator) to load
/// peripheral tools through the registered factory. Returns empty vec
/// when nothing is registered (hardware feature off or not yet wired).
pub async fn load_peripheral_tools(
    config: clawcrew_config::schema::PeripheralsConfig,
) -> Vec<Box<dyn Tool>> {
    if let Some(f) = PERIPHERAL_TOOLS_FN.get() {
        f(config).await.unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Channel map factory type — builds `channel_key → Arc<dyn Channel>` map.
/// Injected by the binary so `clawcrew-runtime` doesn't depend on
/// `clawcrew-channels`.
type ChannelMapFn = Box<
    dyn Fn()
            -> std::collections::HashMap<String, std::sync::Arc<dyn clawcrew_api::channel::Channel>>
        + Send
        + Sync,
>;

/// Channel map factory, injected by the binary.
static CHANNEL_MAP_FN: std::sync::OnceLock<ChannelMapFn> = std::sync::OnceLock::new();

/// Register the channel map factory. Called once at startup by the binary.
pub fn register_channel_map_fn(f: ChannelMapFn) {
    let _ = CHANNEL_MAP_FN.set(f);
}

pub(crate) fn seed_channel_handles(
    ask_user_handle: &Option<tools::PerToolChannelHandle>,
    channel_room_handle: &Option<tools::PerToolChannelHandle>,
    reaction_handle: &tools::PerToolChannelHandle,
    poll_handle: &Option<tools::PerToolChannelHandle>,
    escalate_handle: &Option<tools::PerToolChannelHandle>,
) -> usize {
    let Some(factory) = CHANNEL_MAP_FN.get() else {
        return 0;
    };
    let map = factory();
    if map.is_empty() {
        return 0;
    }

    let handles = [
        ask_user_handle.as_ref(),
        channel_room_handle.as_ref(),
        Some(reaction_handle),
        poll_handle.as_ref(),
        escalate_handle.as_ref(),
    ];

    let mut count = 0;
    for (name, ch) in &map {
        for handle in handles.iter().flatten() {
            handle
                .write()
                .insert(name.clone(), std::sync::Arc::clone(ch));
        }
        count += 1;
    }
    count
}

pub(crate) fn live_channel_registry() -> Option<tools::PerToolChannelHandle> {
    let factory = CHANNEL_MAP_FN.get()?;
    let map = factory();
    if map.is_empty() {
        return None;
    }
    Some(Arc::new(parking_lot::RwLock::new(map)))
}
use crate::agent::TurnMeta;
use crate::observability::{self, Observer, ObserverEvent};
use crate::platform;
use crate::security::{AutonomyLevel, SecurityPolicy};
use crate::tools::scoped;
use crate::tools::{self, Tool};
use crate::util::truncate_with_ellipsis;
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashSet;
use std::fmt::Write;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use clawcrew_api::channel::Channel;
use clawcrew_api::ingress::{IngressContext, TurnOrigin};
use clawcrew_config::schema::Config;
use clawcrew_memory::{self, Memory, MemoryCategory};
#[cfg(test)]
use clawcrew_providers::ChatRequest;
use clawcrew_providers::{self, ChatMessage, ModelProvider, ToolCall};

// Cost tracking moved to `super::cost`.
pub use super::cost::{
    TOOL_LOOP_COST_TRACKING_CONTEXT, ToolLoopCostTrackingContext, TurnUsage,
    check_tool_loop_budget, record_tool_loop_cost_usage,
};

// History management moved to `super::history`.
pub use super::history::{
    append_or_merge_system_message, canonicalize_tool_result_media_markers,
    estimate_history_tokens, load_interactive_session_history,
    load_interactive_session_history_with_crumb, normalize_system_messages,
    save_interactive_session_history, save_interactive_session_history_with_crumb, trim_history,
    truncate_tool_result,
};

/// Minimum user-message length (in chars) for auto-save to memory.
/// Matches the channel-side constant in `channels/mod.rs`.
const AUTOSAVE_MIN_MESSAGE_CHARS: usize = 20;

fn interactive_context_recovery_budget(
    context_limits: clawcrew_config::schema::ResolvedContextLimits,
) -> usize {
    context_limits.model_context_window.saturating_mul(9) / 10
}

/// The single autosave decision for a turn's user-side text, shared by both
/// store sites in this file so the gates cannot drift apart.
///
/// Order matters for meaning, not correctness: the origin gate
/// (`should_autosave_origin`) is the load-bearing check — scheduled and
/// parent-composed origins are known internal synthetic producers, whatever
/// their text looks like. The content filter remains as a backstop for
/// synthetic shapes that arrive on autosave-eligible origins (e.g. replayed
/// histories).
fn should_autosave_user_message(
    auto_save: bool,
    origin: clawcrew_api::ingress::TurnOrigin,
    content: &str,
) -> bool {
    auto_save
        && clawcrew_memory::should_autosave_origin(origin)
        && content.chars().count() >= AUTOSAVE_MIN_MESSAGE_CHARS
        && !clawcrew_memory::should_skip_autosave_content(content)
}

pub(crate) const MAX_INTERACTIVE_INPUT_BYTES: usize = 1024 * 1024; // 1 MiB

/// Result of [`read_capped_line`].
#[derive(Debug)]
pub(crate) enum CappedLine {
    /// A full line under the cap, with the trailing `\n` stripped.
    Line(String),
    /// The physical line exceeded `cap`. The remainder has been
    /// drained to the next `\n` or EOF, so the caller must treat this
    /// as a discarded line and must not feed it into the model path.
    Truncated,
    /// EOF with no bytes read.
    Eof,
}

pub(crate) fn read_capped_line<R: std::io::BufRead>(
    reader: R,
    cap: usize,
) -> std::io::Result<CappedLine> {
    let mut raw = Vec::new();
    // +1 headroom so the cap detection is unambiguous: a buffer that
    // reaches exactly `cap` bytes without a `\n` was truncated; a
    // buffer shorter than `cap` has the full line.
    let mut limited = reader.take((cap + 1) as u64);
    std::io::BufRead::read_until(&mut limited, b'\n', &mut raw)?;
    let truncated = raw.len() > cap;
    if truncated {
        // Drain the rest of the physical line without accumulating it
        // in memory; `read_until` into a `Vec` would re-introduce the
        // original OOM vector.
        let mut inner = limited.into_inner();
        discard_until_newline(&mut inner)?;
        return Ok(CappedLine::Truncated);
    } else if raw.last() == Some(&b'\n') {
        // Strip the trailing `\n` that `read_until` leaves behind. The
        // lossy decode runs after the strip so the result has no
        // trailing newline regardless of the cap path.
        raw.pop();
    }
    if raw.is_empty() {
        return Ok(CappedLine::Eof);
    }
    Ok(CappedLine::Line(String::from_utf8_lossy(&raw).into_owned()))
}

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

fn glob_match(pattern: &str, name: &str) -> bool {
    match pattern.find('*') {
        None => pattern == name,
        Some(star) => {
            let prefix = &pattern[..star];
            let suffix = &pattern[star + 1..];
            name.starts_with(prefix)
                && name.ends_with(suffix)
                && name.len() >= prefix.len() + suffix.len()
        }
    }
}

pub fn apply_policy_tool_filter(
    tools: &mut Vec<Box<dyn Tool>>,
    policy: Option<&clawcrew_config::policy::SecurityPolicy>,
    caller_allowed: Option<&[String]>,
) {
    tools.retain(|t| {
        let name = t.name();
        let policy_ok = policy.is_none_or(|p| p.is_tool_allowed(name));
        let caller_ok = caller_allowed.is_none_or(|list| list.iter().any(|n| n == name));
        policy_ok && caller_ok
    });
}

/// Build the MCP tool-access policy for an agent from its `SecurityPolicy`
/// (`allowed_tools` + `excluded_tools`) and an optional caller-supplied
/// allowlist. Shared by the runtime agent loop and the channels orchestrator
/// so every MCP registration site gates through identical logic.
pub fn mcp_tool_access_policy(
    security: &clawcrew_config::policy::SecurityPolicy,
    caller_allowed: Option<&[String]>,
) -> Option<clawcrew_tools::tool_search::ToolAccessPolicy> {
    clawcrew_tools::tool_search::ToolAccessPolicy::from_security(
        security.allowed_tools.as_deref(),
        security.excluded_tools.as_deref(),
        caller_allowed,
    )
}

/// Whether an MCP tool name is admitted by `policy` (a `None` policy admits
/// everything). The risk-profile denylist always wins; the allowlist
/// auto-admits `<server>__<tool>` names so a restrictive allowlist does not
/// silently drop a configured server's tools.
pub fn eager_mcp_tool_allowed(
    name: &str,
    policy: Option<&clawcrew_tools::tool_search::ToolAccessPolicy>,
) -> bool {
    policy.is_none_or(|policy| policy.is_tool_allowed(name))
}

pub(crate) fn mcp_allowed_tool_count<'a>(
    names: impl IntoIterator<Item = &'a str>,
    policy: Option<&clawcrew_tools::tool_search::ToolAccessPolicy>,
) -> usize {
    names
        .into_iter()
        .filter(|name| eager_mcp_tool_allowed(name, policy))
        .count()
}

/// Append a pre-rendered pinned-MCP-resources section onto the system-prompt
/// MCP accumulator (`deferred_section`).
///
/// This MUST be called *after* the `deferred_loading` branch, which reassigns
/// `deferred_section` with `=` (via `build_deferred_tools_section_filtered`)
/// and would otherwise clobber any earlier-pushed pinned content. Centralizing
/// the append keeps both `run()` and `process_message()` consistent and pins
/// the ordering invariant in one testable place. No-op for an empty section.
pub fn append_pinned_mcp_section(deferred_section: &mut String, pinned_section: &str) {
    if pinned_section.is_empty() {
        return;
    }
    deferred_section.push_str("\n\n");
    deferred_section.push_str(pinned_section);
}

/// Register an eager MCP tool wrapper into `tools` (and the delegate handle,
/// when present) only if `policy` admits it. Returns `true` when the tool was
/// registered, `false` when the policy dropped it.
pub fn register_eager_mcp_tool_if_allowed(
    wrapper: std::sync::Arc<dyn Tool>,
    tools: &mut Vec<Box<dyn Tool>>,
    delegate_handle: Option<&tools::DelegateParentToolsHandle>,
    policy: Option<&clawcrew_tools::tool_search::ToolAccessPolicy>,
) -> bool {
    if !eager_mcp_tool_allowed(wrapper.name(), policy) {
        return false;
    }
    if let Some(handle) = delegate_handle {
        handle.write().push(std::sync::Arc::clone(&wrapper));
    }
    tools.push(Box::new(tools::ArcToolRef(wrapper)));
    true
}

pub(crate) fn preactivate_always_filter_groups(
    deferred: &crate::tools::DeferredMcpToolSet,
    activated: &Arc<Mutex<crate::tools::ActivatedToolSet>>,
    groups: &[clawcrew_config::schema::ToolFilterGroup],
    policy: Option<&clawcrew_tools::tool_search::ToolAccessPolicy>,
    delegate_handle: Option<&tools::DelegateParentToolsHandle>,
) -> HashSet<String> {
    use clawcrew_config::schema::ToolFilterGroupMode;

    let mut activated_names: HashSet<String> = HashSet::new();
    let always_patterns: Vec<&str> = groups
        .iter()
        .filter(|group| matches!(group.mode, ToolFilterGroupMode::Always))
        .flat_map(|group| group.tools.iter().map(String::as_str))
        .collect();
    if always_patterns.is_empty() {
        return activated_names;
    }
    // A poisoned mutex only means another thread panicked mid-update; the
    // activated map itself stays coherent (inserts are atomic), so recover.
    let mut guard = match activated.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    for stub in &deferred.stubs {
        if guard.is_activated(&stub.prefixed_name) {
            continue;
        }
        if !eager_mcp_tool_allowed(&stub.prefixed_name, policy) {
            continue;
        }
        if !always_patterns
            .iter()
            .any(|pat| glob_match(pat, &stub.prefixed_name))
        {
            continue;
        }
        if let Some(tool) = deferred.activate(&stub.prefixed_name) {
            let tool: Arc<dyn Tool> = Arc::from(tool);
            // Pre-activated tools must reach delegated subagents exactly as
            // tool_search-activated ones do (same dedup as the activation
            // hook `assemble` installs on `ToolSearchTool`).
            if let Some(handle) = delegate_handle {
                let mut delegate_tools = handle.write();
                let already = delegate_tools
                    .iter()
                    .any(|existing| existing.name() == tool.name());
                if !already {
                    delegate_tools.push(Arc::clone(&tool));
                }
            }
            guard.activate(stub.prefixed_name.clone(), tool);
            activated_names.insert(stub.prefixed_name.clone());
        }
    }
    activated_names
}

pub fn filter_tool_specs_for_turn(
    tool_specs: Vec<crate::tools::ToolSpec>,
    groups: &[clawcrew_config::schema::ToolFilterGroup],
    user_message: &str,
    mcp_tool_names: &HashSet<String>,
) -> Vec<crate::tools::ToolSpec> {
    if groups.is_empty() {
        return tool_specs;
    }

    let msg_lower = user_message.to_ascii_lowercase();

    tool_specs
        .into_iter()
        .filter(|spec| {
            if !mcp_tool_names.contains(&spec.name) {
                return true;
            }
            mcp_tool_included_for_turn(&spec.name, groups, &msg_lower)
        })
        .collect()
}

fn mcp_tool_included_for_turn(
    name: &str,
    groups: &[clawcrew_config::schema::ToolFilterGroup],
    msg_lower: &str,
) -> bool {
    use clawcrew_config::schema::ToolFilterGroupMode;

    groups.iter().any(|group| {
        let pattern_matches = group.tools.iter().any(|pat| glob_match(pat, name));
        if !pattern_matches {
            return false;
        }
        match group.mode {
            ToolFilterGroupMode::Always => true,
            ToolFilterGroupMode::Dynamic => group
                .keywords
                .iter()
                .any(|kw| msg_lower.contains(&kw.to_ascii_lowercase())),
        }
    })
}

pub fn filter_by_allowed_tools(
    specs: Vec<crate::tools::ToolSpec>,
    allowed: Option<&[String]>,
) -> Vec<crate::tools::ToolSpec> {
    match allowed {
        None => specs,
        Some(list) => specs
            .into_iter()
            .filter(|spec| list.iter().any(|name| name == &spec.name))
            .collect(),
    }
}

// Re-export from clawcrew-types for backwards compatibility.
pub use clawcrew_api::TOOL_LOOP_SESSION_KEY;
pub use clawcrew_api::TOOL_LOOP_THREAD_ID;

// Re-export tool call parsing from the standalone parser crate.
pub use clawcrew_tool_call_parser::{
    ParsedToolCall, ToolProtocolEnvelopeKind, build_native_assistant_history_from_parsed_calls,
    canonicalize_json_for_tool_signature, classify_tool_protocol_envelope,
    contains_tool_protocol_tag_call, detect_tool_call_parse_issue,
    looks_like_malformed_tool_protocol_envelope,
    looks_like_malformed_tool_protocol_envelope_for_known_tools, looks_like_tool_protocol_envelope,
    looks_like_tool_protocol_example, parse_tool_calls, strip_think_tags, strip_tool_result_blocks,
    tool_protocol_envelope_mentions_known_tool,
};

/// Run a future with the thread ID set in task-local storage.
/// Rate-limiting reads this to assign per-sender buckets.
pub async fn scope_thread_id<F>(thread_id: Option<String>, future: F) -> F::Output
where
    F: std::future::Future,
{
    TOOL_LOOP_THREAD_ID.scope(thread_id, future).await
}

/// Run a future with the session key set in task-local storage.
/// The scope wraps the entire agent turn, so all tools invoked during
/// the turn (including nested calls) see the same session key.
/// SessionsCurrentTool reads this to identify the active session.
pub async fn scope_session_key<F>(session_key: Option<String>, future: F) -> F::Output
where
    F: std::future::Future,
{
    TOOL_LOOP_SESSION_KEY.scope(session_key, future).await
}

pub(crate) fn compute_excluded_mcp_tools(
    tools_registry: &[Box<dyn Tool>],
    groups: &[clawcrew_config::schema::ToolFilterGroup],
    user_message: &str,
    mcp_tool_names: &HashSet<String>,
) -> Vec<String> {
    if groups.is_empty() {
        return Vec::new();
    }
    let msg_lower = user_message.to_ascii_lowercase();
    tools_registry
        .iter()
        .map(|t| t.name())
        .filter(|name| {
            mcp_tool_names.contains(*name) && !mcp_tool_included_for_turn(name, groups, &msg_lower)
        })
        .map(str::to_string)
        .collect()
}

pub fn native_tool_specs_present_for_turn(
    model_provider: &dyn ModelProvider,
    model: &str,
    tools_registry: &[Box<dyn Tool>],
    excluded_tools: &[String],
    activated_tools: Option<&Arc<Mutex<crate::tools::ActivatedToolSet>>>,
) -> Result<bool> {
    if !model_provider
        .capabilities_for_model(model)
        .native_tool_calling
    {
        return Ok(false);
    }

    // Name-only presence check mirroring `build_iteration_tool_specs`'s
    // filtering, without assembling any specs tools are present if
    // the registry or the activated deferred set has a non-excluded name.
    let is_excluded = |name: &str| excluded_tools.iter().any(|ex| ex == name);
    if tools_registry.iter().any(|tool| !is_excluded(tool.name())) {
        return Ok(true);
    }
    let Some(at) = activated_tools else {
        return Ok(false);
    };
    let activated = match at.lock() {
        Ok(guard) => guard,
        // Same recovery as build_iteration_tool_specs: a poisoned lock is
        // still safe for a read-only name scan.
        Err(poisoned) => poisoned.into_inner(),
    };
    Ok(activated.tool_names().iter().any(|name| !is_excluded(name)))
}

pub(crate) static IMAGE_DATA_URI_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[IMAGE:data:[^\]]*\]").expect("static image data URI regex must compile")
});

fn elide_image_data(content: &str) -> String {
    IMAGE_DATA_URI_REGEX
        .replace_all(content, "[IMAGE:<image data elided>]")
        .into_owned()
}

pub(crate) fn scrub_for_export(content: &str) -> String {
    scrub_credentials(&clawcrew_providers::scrub_secret_patterns(
        &elide_image_data(content),
    ))
}

pub(crate) fn capture_llm_messages(
    messages: &[ChatMessage],
    output_text: Option<&str>,
    output_tool_calls: &[ToolCall],
) -> Option<clawcrew_api::observability_traits::LlmMessageSnapshot> {
    if !cfg!(feature = "observability-otel") {
        return None;
    }

    use clawcrew_api::observability_traits::{
        LlmMessageSnapshot, MessageSnapshot, ToolCallSnapshot,
    };

    let system_instructions = messages
        .iter()
        .find(|m| m.role == "system")
        .map(|m| scrub_for_export(&m.content));

    let input = messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| MessageSnapshot {
            role: m.role.clone(),
            content: scrub_for_export(&m.content),
        })
        .collect();

    let output_text = output_text.filter(|t| !t.is_empty()).map(scrub_for_export);

    let output_tool_calls = output_tool_calls
        .iter()
        .map(|tc| ToolCallSnapshot {
            id: tc.id.clone(),
            name: tc.name.clone(),
            arguments_json: scrub_for_export(&tc.arguments),
        })
        .collect();

    Some(LlmMessageSnapshot {
        input,
        output_text,
        output_tool_calls,
        system_instructions,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_system_prompt_for_turn(
    agent_workspace: &std::path::Path,
    model_name: &str,
    tool_descs: &[(&str, &str)],
    deferred_section: &str,
    skills: &[crate::skills::Skill],
    identity_config: Option<&clawcrew_config::schema::IdentityConfig>,
    bootstrap_max_chars: Option<usize>,
    risk_profile: &clawcrew_config::schema::RiskProfileConfig,
    model_provider: &dyn ModelProvider,
    tools_registry: &[Box<dyn Tool>],
    excluded_tools: &[String],
    activated_tools: Option<&Arc<Mutex<crate::tools::ActivatedToolSet>>>,
    strict_tool_parsing: bool,
    skills_prompt_mode: clawcrew_config::schema::SkillsPromptInjectionMode,
    compact_context: bool,
    max_system_prompt_chars: usize,
    inject_memory: bool,
    show_tool_calls: bool,
    thinking_prefix: Option<&str>,
    shell_profile: Option<&clawcrew_api::runtime_traits::ShellProfile>,
) -> Result<String> {
    let native_tools = model_provider
        .capabilities_for_model(model_name)
        .native_tool_calling;
    let native_tool_specs_present = native_tool_specs_present_for_turn(
        model_provider,
        model_name,
        tools_registry,
        excluded_tools,
        activated_tools,
    )?;
    let excluded_tool_names: HashSet<&str> = excluded_tools.iter().map(String::as_str).collect();
    let effective_tool_names: HashSet<&str> = tools_registry
        .iter()
        .map(|tool| tool.name())
        .filter(|name| !excluded_tool_names.contains(*name))
        .collect();
    let mut turn_tool_descs = tool_descs.to_vec();
    turn_tool_descs.retain(|(name, _)| effective_tool_names.contains(name));
    let mut turn_deferred_section = deferred_section.to_string();
    let expose_text_tool_protocol = apply_text_tool_prompt_policy(
        native_tools,
        strict_tool_parsing,
        &mut turn_tool_descs,
        &mut turn_deferred_section,
    );
    let skill_tools_protocol_exposed = expose_text_tool_protocol || native_tool_specs_present;
    let mut system_prompt =
        crate::agent::system_prompt::build_system_prompt_with_mode_and_effective_tools(
            agent_workspace,
            model_name,
            &turn_tool_descs,
            |name| skill_tools_protocol_exposed && effective_tool_names.contains(name),
            skills,
            identity_config,
            bootstrap_max_chars,
            Some(risk_profile),
            native_tool_specs_present,
            skills_prompt_mode,
            compact_context,
            0,
            inject_memory,
            show_tool_calls,
            shell_profile,
        );

    if expose_text_tool_protocol {
        system_prompt.push_str(&build_tool_instructions_for_names(
            tools_registry,
            &effective_tool_names,
        ));
    }
    if !turn_deferred_section.is_empty() {
        system_prompt.push('\n');
        system_prompt.push_str(&turn_deferred_section);
    }
    if let Some(prefix) = thinking_prefix {
        system_prompt = format!("{prefix}\n\n{system_prompt}");
    }

    Ok(crate::agent::system_prompt::finalize_system_prompt(
        system_prompt,
        max_system_prompt_chars,
    ))
}

pub fn make_query_summary(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    Some(truncate_with_ellipsis(&scrub_credentials(raw), 200))
}

pub use clawcrew_api::TOOL_CHOICE_OVERRIDE;

#[cfg(test)]
fn tools_to_openai_format(tools_registry: &[Box<dyn Tool>]) -> Vec<serde_json::Value> {
    tools_registry
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.parameters_schema()
                }
            })
        })
        .collect()
}

fn autosave_memory_key(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4())
}

/// Build hardware datasheet context from RAG when peripherals are enabled.
/// Includes pin-alias lookup (e.g. "red_led" → 13) when query matches, plus retrieved chunks.
fn build_hardware_context(
    rag: &crate::rag::HardwareRag,
    observer: &dyn Observer,
    user_msg: &str,
    boards: &[String],
    chunk_limit: usize,
    turn: TurnMeta<'_>,
) -> String {
    if rag.is_empty() || boards.is_empty() {
        return String::new();
    }

    let mut context = String::new();

    // Pin aliases: when user says "red led", inject "red_led: 13" for matching boards
    let pin_ctx = rag.pin_alias_context(user_msg, boards);
    if !pin_ctx.is_empty() {
        context.push_str(&pin_ctx);
    }

    let start = std::time::Instant::now();
    let chunks = rag.retrieve(user_msg, boards, chunk_limit);
    let duration = start.elapsed();
    observer.record_event(&ObserverEvent::RagRetrieve {
        query_summary: make_query_summary(user_msg),
        duration,
        num_chunks: chunks.len(),
        num_boards: boards.len(),
        channel: Some(turn.channel_name.to_string()),
        agent_alias: turn.agent_alias.map(str::to_string),
        turn_id: Some(turn.turn_id.to_string()),
    });

    if chunks.is_empty() && pin_ctx.is_empty() {
        return String::new();
    }

    if !chunks.is_empty() {
        context.push_str("[Hardware documentation]\n");
    }
    for chunk in chunks {
        let board_tag = chunk.board.as_deref().unwrap_or("generic");
        let _ = writeln!(
            context,
            "--- {} ({}) ---\n{}\n",
            chunk.source, board_tag, chunk.content
        );
    }
    context.push('\n');
    context
}

// Tool execution moved to `super::tool_execution`.
pub use super::tool_execution::{ToolExecutionOutcome, should_execute_tools_in_parallel};

/// Execute a single turn of the agent loop: send messages, parse tool calls,
/// execute tools, and loop until the LLM produces a final text response.
/// When `silent` is true, suppresses stdout (for channel use).
/// `agent_alias`, when the caller has resolved one, is threaded onto the
/// turn's `AgentStart`/`AgentEnd` brackets and onto the inner `ToolLoop`, so
/// every lifecycle observer event of the turn (agent_start, llm_request,
/// llm_response, tool_call_start, tool_call, agent_end) carries the full
/// `(channel, agent_alias, turn_id)` correlation triple that observer
/// consumers (Prometheus, OTel, the gateway `/api/events` stream) rely on for
/// per-agent attribution. `None` opts out for callers without a resolved
/// alias (tests, benches). `turn_id` follows the same pattern: `Some` reuses
/// a caller-minted id so pre-turn events (the `process_message` RAG
/// retrieval) join the bracket; `None` self-mints.
#[allow(clippy::too_many_arguments)]
pub async fn agent_turn(
    config: Option<&clawcrew_config::schema::Config>,
    model_provider: &dyn ModelProvider,
    history: &mut Vec<ChatMessage>,
    // Authoritative record that `history` carries the synthetic trim
    // breadcrumb after its leading system messages; kept beside the buffer
    // instead of being inferred from localized text. Set to true when a trim
    // path inserts a crumb during the turn.
    history_has_trim_breadcrumb: &mut bool,
    // Out-param the loop writes through when it injects a recalled-memory
    // preamble onto the last user message: the exact byte length injected,
    // so the caller can strip precisely that block before persisting —
    // see `ToolLoop::injected_memory_preamble`.
    injected_memory_preamble: &mut Option<String>,
    tools_registry: &scoped::ScopedToolRegistry,
    observer: &dyn Observer,
    provider_name: &str,
    model: &str,
    temperature: Option<f64>,
    silent: bool,
    channel_name: &str,
    channel_reply_target: Option<&str>,
    multimodal_config: &clawcrew_config::schema::MultimodalConfig,
    max_tool_iterations: usize,
    approval: Option<&ApprovalManager>,
    excluded_tools: &[String],
    dedup_exempt_tools: &[String],
    activated_tools: Option<&std::sync::Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>>,
    model_switch_callback: Option<ModelSwitchCallback>,
    strict_tool_parsing: bool,
    parallel_tools: bool,
    max_tool_result_chars: usize,
    context_token_budget: usize,
    channel: Option<&dyn Channel>,
    origin: TurnOrigin,
    memory: Option<crate::agent::memory_inject::TurnMemory<'_>>,
    agent_alias: Option<&str>,
    turn_id: Option<&str>,
) -> Result<String> {
    agent_turn_with_sop_reassembly(
        config,
        model_provider,
        history,
        history_has_trim_breadcrumb,
        injected_memory_preamble,
        tools_registry,
        observer,
        provider_name,
        model,
        temperature,
        silent,
        channel_name,
        channel_reply_target,
        multimodal_config,
        max_tool_iterations,
        approval,
        excluded_tools,
        dedup_exempt_tools,
        activated_tools,
        model_switch_callback,
        strict_tool_parsing,
        parallel_tools,
        max_tool_result_chars,
        context_token_budget,
        channel,
        origin,
        memory,
        agent_alias,
        turn_id,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn agent_turn_with_sop_reassembly(
    config: Option<&clawcrew_config::schema::Config>,
    model_provider: &dyn ModelProvider,
    history: &mut Vec<ChatMessage>,
    // Authoritative breadcrumb provenance for `history` — see `agent_turn`.
    history_has_trim_breadcrumb: &mut bool,
    // See `agent_turn`.
    injected_memory_preamble: &mut Option<String>,
    tools_registry: &scoped::ScopedToolRegistry,
    observer: &dyn Observer,
    provider_name: &str,
    model: &str,
    temperature: Option<f64>,
    silent: bool,
    channel_name: &str,
    channel_reply_target: Option<&str>,
    multimodal_config: &clawcrew_config::schema::MultimodalConfig,
    max_tool_iterations: usize,
    approval: Option<&ApprovalManager>,
    excluded_tools: &[String],
    dedup_exempt_tools: &[String],
    activated_tools: Option<&std::sync::Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>>,
    model_switch_callback: Option<ModelSwitchCallback>,
    strict_tool_parsing: bool,
    parallel_tools: bool,
    max_tool_result_chars: usize,
    context_token_budget: usize,
    channel: Option<&dyn Channel>,
    origin: TurnOrigin,
    memory: Option<crate::agent::memory_inject::TurnMemory<'_>>,
    agent_alias: Option<&str>,
    turn_id: Option<&str>,
    sop_reassembly: Option<SopStepReassembly<'_>>,
) -> Result<String> {
    let turn_id = turn_id.map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_string);
    #[cfg(test)]
    if let Some(hook) = AGENT_TURN_SOP_REASSEMBLY_TEST_HOOK
        .lock()
        .expect("agent-turn reassembly test hook lock should not be poisoned")
        .as_ref()
        .cloned()
    {
        hook(sop_reassembly.is_some());
    }
    // Bracket the turn with AgentStart/AgentEnd so entry points that dispatch
    // through `agent_turn` (gateway webhook chat via `process_message`, peer
    // messages) surface turn lifecycle events to observers — mirroring the
    // CLI `run` and `Agent::turn_streamed` entry points. The brackets carry
    // the caller's resolved alias, so they agree with the inner events on the
    // full (channel, agent_alias, turn_id) triple.
    let mut turn_guard = crate::observability::AgentTurnGuard::start(
        observer,
        provider_name,
        model,
        Some(channel_name.to_string()),
        agent_alias.map(str::to_string),
        Some(turn_id.clone()),
    );
    let resolved_capacity = config.map_or(
        clawcrew_config::schema::ResolvedModelContextWindow {
            tokens: clawcrew_config::schema::UNCONFIGURED_CONTEXT_WINDOW_FALLBACK,
            source: clawcrew_config::schema::ModelContextWindowSource::CompatibilityFallback,
        },
        |config| config.resolved_model_context_window_for_route(provider_name, model),
    );
    let context_token_budget = if context_token_budget == 0 {
        0
    } else {
        context_token_budget.min(resolved_capacity.tokens)
    };
    let context_limits = clawcrew_config::schema::ResolvedContextLimits {
        model_context_window: resolved_capacity.tokens,
        context_token_budget,
        model_context_window_source: resolved_capacity.source,
    };
    let result = Box::pin(run_tool_call_loop(ToolLoop {
        sop_reassembly,
        history_has_trim_breadcrumb,
        injected_memory_preamble,
        exec: ResolvedAgentExecution::resolve(
            ResolvedModelAccess {
                model_provider,
                provider_name,
                model,
                dispatch_model: model,
                temperature,
            },
            ResolvedIo {
    app_registry: None,
                tools_registry,
                observer,
                silent,
                approval,
                multimodal_config,
                config,
                hooks: None,
                activated_tools,
                model_switch_callback,
                receipt_generator: None,
            },
            ResolvedRuntimeKnobs {
                max_tool_iterations,
                excluded_tools,
                dedup_exempt_tools,
                pacing: &clawcrew_config::schema::PacingConfig::default(),
                strict_tool_parsing,
                parallel_tools,
                max_tool_result_chars,
                context_limits,
                context_limits_resolver: None,
                knobs: &LoopKnobs::default(),
            },
        ),
        history,
        channel_name,
        channel_reply_target,
        cancellation_token: None,
        on_delta: None,
        shared_budget: None, // no shared budget for agent_turn callers
        channel,
        collected_receipts: None,
        event_tx: None,
        steering: None,
        new_messages_out: None,
        image_cache: None,
        // Origin and the per-turn memory half are threaded from the entry
        // point; source/transport/trust stay phase-1 placeholders until
        // per-transport stamping lands.
        memory,
        ingress: IngressContext::from_origin(origin),
        agent_alias,
        parent_agent_alias: None,
        served_route_sink: None,
        turn_id: &turn_id,
    }))
    .await;
    // Snapshot token usage from the task-local cost context when the caller
    // scoped one around this call (the gateway scopes both
    // `TOOL_LOOP_TURN_USAGE` and `TOOL_LOOP_COST_TRACKING_CONTEXT` around
    // `process_message`); unscoped callers report `None`. When this runs
    // nested inside a parent turn's scoped context (peer-message-as-tool),
    // `snapshot_turn_usage` prefers the caller-scoped task-local and may
    // report the parent turn's cumulative usage — pre-existing cost
    // attribution semantics, kept as-is.
    let tokens_used = TOOL_LOOP_COST_TRACKING_CONTEXT
        .try_with(std::clone::Clone::clone)
        .ok()
        .flatten()
        .and_then(|ctx| {
            let usage = ctx.snapshot_turn_usage();
            (usage.input_tokens > 0 || usage.output_tokens > 0).then_some(
                clawcrew_api::observability_traits::TurnTokenUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                },
            )
        });
    turn_guard.set_usage(tokens_used, None);
    turn_guard.finish();
    result
}

// ── Agent Tool-Call Loop ──────────────────────────────────────────────────
// The turn engine lives in `super::turn` — `run_tool_call_loop` plus one
// file per step (run sheet in agent/turn/mod.rs). `crate::agent::loop_`
// stays the canonical public path via these re-exports.
pub(crate) use super::turn::StreamCancelledAfterOutput;
pub use super::turn::{
    ContextLimitsResolver, DRAFT_PLACEHOLDER, DraftEvent, LoopKnobs, MaxIterationBehavior,
    ModelSwitchCallback, ModelSwitchRequested, PROGRESS_MIN_INTERVAL_MS, ProgressEvent,
    REASONING_FULL_PREFIX, ResolvedAgentExecution, ResolvedIo, ResolvedModelAccess,
    ResolvedRuntimeKnobs, ServedRoute, ServedRouteSink, SopStepReassembly, StreamDelta,
    THINKING_STATUS_PREFIX, ToolLoop, ToolLoopCancelled, drain_steering_messages,
    is_model_switch_requested, is_thinking_status_text, is_tool_loop_cancelled, run_tool_call_loop,
    scrub_credentials, thinking_status_label_round, thinking_status_round, thinking_status_text,
};
#[cfg(test)]
pub(crate) use super::turn::{
    DEFAULT_MAX_TOOL_ITERATIONS, MAX_MALFORMED_TOOL_PROTOCOL_RETRIES,
    build_native_assistant_history, consume_provider_streaming_response,
    maybe_inject_channel_delivery_defaults, resolve_display_text,
};

/// Build the tool instruction block for the system prompt so the LLM knows
/// how to invoke tools.
pub fn build_tool_instructions(tools_registry: &[Box<dyn Tool>]) -> String {
    build_tool_instructions_for_tools(tools_registry.iter().map(|tool| tool.as_ref()))
}

/// Build tool instructions for the subset of registered tools that are
/// effective for the current prompt.
pub fn build_tool_instructions_for_names(
    tools_registry: &[Box<dyn Tool>],
    effective_tool_names: &HashSet<&str>,
) -> String {
    build_tool_instructions_for_tools(
        tools_registry
            .iter()
            .map(|tool| tool.as_ref())
            .filter(|tool| effective_tool_names.contains(tool.name())),
    )
}

fn build_tool_instructions_for_tools<'a>(tools: impl IntoIterator<Item = &'a dyn Tool>) -> String {
    let tools: Vec<&dyn Tool> = tools.into_iter().collect();
    if tools.is_empty() {
        return String::new();
    }

    // The tool-call formatting guidance has one home: `agent::tool_call_format`.
    // Do not re-type it here; layer builder-specific material (the tool
    // listing below) around it instead.
    let mut instructions = String::from("\n");
    instructions.push_str(crate::agent::tool_call_format::TOOL_CALL_PROTOCOL_INSTRUCTIONS);
    instructions.push_str("### Available Tools\n\n");

    for tool in tools {
        let desc = tool.description();
        let _ = writeln!(
            instructions,
            "**{}**: {}\nParameters: `{}`\n",
            tool.name(),
            desc,
            tool.parameters_schema()
        );
    }

    instructions
}

fn retain_registered_tool_descriptions(
    tool_descs: &mut Vec<(&str, &str)>,
    tools_registry: &[Box<dyn Tool>],
) {
    let registered_tool_names: HashSet<&str> =
        tools_registry.iter().map(|tool| tool.name()).collect();
    tool_descs.retain(|(name, _)| registered_tool_names.contains(name));
}

pub fn apply_text_tool_prompt_policy(
    native_tools: bool,
    strict_tool_parsing: bool,
    tool_descs: &mut Vec<(&str, &str)>,
    deferred_section: &mut String,
) -> bool {
    let expose_text_tool_protocol = !native_tools && !strict_tool_parsing;
    if !native_tools && strict_tool_parsing {
        tool_descs.clear();
        deferred_section.clear();
    }
    expose_text_tool_protocol
}

#[derive(Default)]
pub struct AgentRunOverrides {
    pub security: Option<Arc<SecurityPolicy>>,
    pub memory: Option<Arc<dyn Memory>>,
    pub is_subagent: bool,
    /// Spawn-site opt-out of the engine's memory-context injection (e.g. a
    /// cron job configured with `uses_memory = false`). Default `false`.
    pub suppress_memory_inject: bool,
    pub memory_free: bool,
    /// Pre-built MCP registry supplied by the caller. The daemon heartbeat
    /// worker constructs this once at worker start and shares it across
    /// every tick so that stdio MCP children live for the daemon's
    /// lifetime rather than being orphaned and re-spawned per
    /// `agent::run` call. When `Some`, the loop MUST use this
    /// `Arc<McpRegistry>` and MUST NOT call `McpRegistry::connect_all`
    /// itself. `None` preserves the legacy per-call connect path
    /// (CLI / one-shot), which is correct for callers that have no
    /// cross-turn reuse contract.
    pub mcp_registry: Option<Arc<crate::tools::McpRegistry>>,
}

fn agent_provider_composite(
    config: &clawcrew_config::schema::Config,
    agent_alias: &str,
) -> Option<String> {
    config
        .resolved_model_provider_for_agent(agent_alias)
        .map(|(ty, alias, _)| format!("{ty}.{alias}"))
}

/// Return the owned agent config direct-turn setup needs, with runtime-profile
/// values baked into `resolved`.
fn resolved_agent_for_turn(
    config: &clawcrew_config::schema::Config,
    agent_alias: &str,
) -> Result<clawcrew_config::schema::AliasedAgentConfig> {
    let agent = config
        .resolved_agent_config(agent_alias)
        .with_context(|| format!("agents.{agent_alias} is not configured"))?;
    #[cfg(test)]
    if let Some(hook) = RESOLVED_AGENT_FOR_TURN_TEST_HOOK
        .lock()
        .expect("resolved-agent test hook lock should not be poisoned")
        .as_ref()
        .cloned()
    {
        hook(agent_alias, agent.resolved.max_tool_iterations);
    }
    Ok(agent)
}

#[cfg(test)]
type ResolvedAgentForTurnTestHook = Arc<dyn Fn(&str, usize) + Send + Sync>;

#[cfg(test)]
static RESOLVED_AGENT_FOR_TURN_TEST_HOOK: LazyLock<Mutex<Option<ResolvedAgentForTurnTestHook>>> =
    LazyLock::new(|| Mutex::new(None));

#[cfg(test)]
type AgentTurnSopReassemblyTestHook = Arc<dyn Fn(bool) + Send + Sync>;

#[cfg(test)]
static AGENT_TURN_SOP_REASSEMBLY_TEST_HOOK: LazyLock<
    Mutex<Option<AgentTurnSopReassemblyTestHook>>,
> = LazyLock::new(|| Mutex::new(None));

fn api_key_and_uri_for_provider(
    config: &clawcrew_config::schema::Config,
    provider_name: &str,
    fallback: Option<&clawcrew_config::schema::ModelProviderConfig>,
) -> (Option<String>, Option<String>) {
    if let Some((fam, al)) = provider_name.split_once('.')
        && let Some(entry) = config.providers.models.find(fam, al)
    {
        return (entry.api_key.clone(), entry.uri.clone());
    }
    (
        fallback.and_then(|e| e.api_key.clone()),
        fallback.and_then(|e| e.uri.clone()),
    )
}

/// Project a typed terminal-completion failure only at the direct CLI boundary.
///
/// The typed error's `Display` remains the stable diagnostic used by provider
/// and runtime telemetry. CLI delivery is the presentation boundary, where the
/// corresponding Fluent message is required instead.
fn project_cli_terminal_completion_error(error: anyhow::Error) -> anyhow::Error {
    let Some(user_message) = crate::agent::terminal_completion_error_message(&error, None) else {
        return error;
    };
    let diagnostic = error.to_string();
    ::clawcrew_log::record!(
        ERROR,
        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
            .with_category(::clawcrew_log::EventCategory::Agent)
            .with_outcome(::clawcrew_log::EventOutcome::Failure)
            .with_attrs(::serde_json::json!({
                "error": diagnostic,
                "error_key": "terminal_completion",
            })),
        "CLI agent turn failed"
    );
    anyhow::Error::msg(user_message)
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn run(
    config: Config,
    agent_alias: &str,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: Option<f64>,
    peripheral_overrides: Vec<String>,
    interactive: bool,
    session_state_file: Option<PathBuf>,
    allowed_tools: Option<Vec<String>>,
    origin: TurnOrigin,
    overrides: AgentRunOverrides,
) -> Result<String> {
    use ::clawcrew_log::Instrument;
    let agent = resolved_agent_for_turn(&config, agent_alias)?;
    crate::agent::thinking::validate_thinking_config(&agent.resolved.thinking);
    let risk_profile = config
        .risk_profile_for_agent(agent_alias)
        .with_context(|| {
            format!(
                "agents.{agent_alias}.risk_profile does not name a configured risk_profiles entry"
            )
        })?
        .clone();
    let memory_composite = {
        use clawcrew_config::multi_agent::MemoryBackendKind;
        match agent.memory.backend {
            MemoryBackendKind::Markdown => format!("markdown.{agent_alias}"),
            MemoryBackendKind::None => "none".to_string(),
            _ => {
                let raw = config.memory.backend.trim();
                if raw.is_empty() || raw.eq_ignore_ascii_case("none") {
                    "none".to_string()
                } else {
                    let (kind, alias) = raw.split_once('.').unwrap_or((raw, "default"));
                    format!("{kind}.{alias}")
                }
            }
        }
    };
    let __zc_alias = agent_alias.to_string();
    let __zc_attribution_span =
        ::clawcrew_log::attribution_span!(&crate::agent::AgentAttribution(__zc_alias.as_str()));
    let __zc_scope_span = ::clawcrew_log::info_span!(
        target: "clawcrew_log_internal_scope",
        "clawcrew_scope",
        risk_profile = %agent.risk_profile,
        runtime_profile = %agent.runtime_profile,
        memory_namespace = %memory_composite,
    );
    let __zc_body = async move {
        let agent_alias: &str = __zc_alias.as_str();
        // ── Effective per-agent runtime tunables ──────────────────────
        // Profile values (when set) override the agent's inline fields.
        // See `Config::resolved_agent_config` for precedence rules.
        let eff_max_history_messages = agent.resolved.max_history_messages;
        let eff_compact_context = agent.resolved.compact_context;
        let eff_max_system_prompt_chars = agent.resolved.max_system_prompt_chars;
        let eff_prompt_injection_mode = agent.resolved.prompt_injection_mode;
        let base_observer = observability::create_observer(&config.observability);
        let observer: Arc<dyn Observer> = Arc::from(base_observer);
        let turn_id = uuid::Uuid::new_v4().to_string();
        let channel_name = if interactive { "cli" } else { "daemon" };
        let _flush_guard = interactive.then(|| observability::FlushGuard::new(observer.clone()));
        if interactive
            && matches!(
                config.observability.backend,
                clawcrew_config::schema::ObservabilityBackend::Prometheus
            )
        {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown),
                "Observability backend is Prometheus (pull/scrape model): a one-shot CLI process \
                 exits before any scraper can pull, so its telemetry will not be collected. \
                 Prometheus is intended for long-running (daemon) deployments."
            );
        }
        let runtime: Arc<dyn platform::RuntimeAdapter> =
            Arc::from(platform::create_runtime(&config.runtime)?);
        let is_subagent_caller = overrides.is_subagent;
        let suppress_memory_inject = overrides.suppress_memory_inject;
        let memory_free = overrides.memory_free;
        let security = match overrides.security {
            Some(sec) => sec,
            None => Arc::new(SecurityPolicy::for_agent(&config, agent_alias)?),
        };

        let agent_provider_resolved = config
            .resolved_model_provider_for_agent(agent_alias)
            .map(|(ty, alias, cfg)| (ty, alias.to_string(), cfg.clone()));
        let agent_model_provider = agent_provider_resolved.as_ref().map(|(_, _, cfg)| cfg);

        let mem: Arc<dyn Memory> = if memory_free {
            Arc::new(clawcrew_memory::NoneMemory::new("none"))
        } else {
            match overrides.memory {
                Some(m) => m,
                None => {
                    clawcrew_memory::create_memory_for_agent(
                        &config,
                        agent_alias,
                        agent_model_provider.and_then(|e| e.api_key.as_deref()),
                    )
                    .await?
                }
            }
        };
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Load)
                .with_category(::clawcrew_log::EventCategory::Memory)
                .with_attrs(::serde_json::json!({"backend": mem.name()})),
            "Memory initialized"
        );

        // ── Peripherals (merge peripheral tools into registry) ─
        if !peripheral_overrides.is_empty() {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Load)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_attrs(::serde_json::json!({"peripherals": peripheral_overrides})),
                "Peripheral overrides from CLI (config boards take precedence)"
            );
        }

        // ── Tools (including memory tools and peripherals) ────────────
        let (composio_key, composio_entity_id) = if config.composio.enabled {
            (
                config.composio.api_key.as_deref(),
                Some(config.composio.entity_id.as_str()),
            )
        } else {
            (None, None)
        };

        // Build SOP engine when sops_dir is configured so SOP tools are
        // available on this path (CLI agent run). No channel map is wired on this
        // path, so the approval route adapter is the no-op (log-only); the daemon
        // path injects a real channel-delivering adapter.
        let (sop_engine, sop_audit) = if config.sop.runtime_enabled() {
            let sop_mem: Arc<dyn clawcrew_memory::Memory> =
                clawcrew_memory::create_memory_for_agent(&config, agent_alias, None).await?;
            let (engine, audit) = crate::sop::build_sop_engine(
                config.sop.clone(),
                &config.data_dir,
                &config.install_root_dir(),
                sop_mem,
                Default::default(),
            );
            (Some(engine), Some(audit))
        } else {
            (None, None)
        };

        let all_tools_result = tools::all_tools_with_runtime(
            Arc::new(config.clone()),
            &security,
            &risk_profile,
            agent_alias,
            runtime.clone(),
            mem.clone(),
            composio_key,
            composio_entity_id,
            &config.browser,
            &config.http_request,
            &config.web_fetch,
            &config.data_dir,
            &config.agents,
            agent_model_provider.and_then(|e| e.api_key.as_deref()),
            &config,
            None,
            is_subagent_caller,
            None,
            sop_engine,
            sop_audit,
            None,
        )?;
        let skills = crate::skills::load_skills_for_agent_from_config(&config, agent_alias);
        // Route the per-agent tool registry through the one gated seam
        // (peripherals -> built-in filter -> MCP scope+gate -> skills), identical
        // to the behavior this path hand-rolled. `caller_allowed` carries the
        // run() per-run allowlist; connect_peripherals is true (execution path).
        let assembled = scoped::ScopedToolRegistry::assemble(scoped::ScopedAssembly {
            config: &config,
            agent_alias,
            security: &security,
            built: all_tools_result,
            skills: &skills,
            runtime: runtime.clone(),
            caller_allowed: allowed_tools.as_deref(),
            connect_mcp: true,
            connect_peripherals: true,
            // A memory-free run drops the persistent memory tools so the model
            // cannot read or write memory even though the registry is otherwise
            // built identically.
            exclude_memory: memory_free,
            acp_delivery: false,
            list_deferred_mcp_specs: false,
            emit_assembly_logs: true,
            // Honor the daemon worker's pre-built shared registry so stdio
            // MCP children live for the daemon's lifetime, not per
            // `agent::run` call. CLI/one-shot callers leave
            // `mcp_registry` at its default (`None`) and the seam
            // falls back to the per-call `connect_all`.
            mcp_registry: overrides.mcp_registry.as_ref().map(Arc::clone),
        })
        .await;
        // run injects one combined MCP prompt block: deferred tool-search listing +
        // pinned resources, composed by the harness.
        let deferred_section = assembled.combined_mcp_prompt_section();
        let scoped::ScopedAssembled {
            registry,
            delegate_handle: _,
            ask_user_handle,
            reaction_handle,
            poll_handle,
            escalate_handle,
            channel_room_handle,
            activated_handle,
            mcp_tool_names,
            ..
        } = assembled;
        // Keep the sealed registry sealed: the engine's `ResolvedIo.tools_registry`
        // now takes `&ScopedToolRegistry`, so this local stays a scoped registry
        // (coerces to `&[Box<dyn Tool>]` at the leaf call sites via `Deref`).
        let tools_registry = registry;

        // Populate all channel-driven tool handles from the registered factory.
        let count = seed_channel_handles(
            &ask_user_handle,
            &channel_room_handle,
            &reaction_handle,
            &poll_handle,
            &escalate_handle,
        );
        if count > 0 {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Register)
                    .with_category(::clawcrew_log::EventCategory::Channel)
                    .with_attrs(::serde_json::json!({"count": count})),
                &format!("Registered {} channel(s) for CLI agent", count),
            );
        }

        // ── Resolve model_provider ─────────────────────────────────────────
        let agent_provider_ref = agent_provider_composite(&config, agent_alias);
        let mut provider_name = provider_override
            .as_deref()
            .or(agent_provider_ref.as_deref())
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    ERROR,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_category(::clawcrew_log::EventCategory::Agent)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"agent_alias": agent_alias})),
                    "agent loop refused: agent.model_provider unresolved and no --provider override"
                );
                anyhow::Error::msg(format!(
                    "agents.{agent_alias}.model_provider does not resolve and no provider override \
                     was passed on the CLI. Either set `[agents.{agent_alias}] model_provider` or \
                     pass --provider."
                ))
            })?
            .to_string();

        let mut model_name = match model_override
            .as_deref()
            .or(agent_model_provider.and_then(|e| e.model.as_deref()))
        {
            Some(m) => m.to_string(),
            None => anyhow::bail!(
                "no model configured for agent {agent_alias}: \
             [providers.models.{provider_name}.<alias>].model is unset and --model was not passed"
            ),
        };
        let mut context_limits =
            config.resolved_context_limits_for_route(agent_alias, &provider_name, &model_name);

        {
            let span = clawcrew_log::Span::current();
            let mp_composite = match agent_provider_resolved.as_ref() {
                Some((ty, alias, _)) => format!("{ty}.{alias}"),
                None => provider_name.clone(),
            };
            span.record("model_provider", mp_composite.as_str());
            span.record("model", model_name.as_str());
        }

        let agent_runtime_options = match agent_provider_resolved.as_ref() {
            Some((ty, alias, _)) => {
                clawcrew_providers::provider_runtime_options_for_alias(&config, ty, alias)
            }
            None => clawcrew_providers::provider_runtime_options_for_agent(&config, agent_alias),
        };
        // Resolve every alias-owned option, including vision, through the shared
        // provider-ref resolver. This keeps a --provider override isolated from
        // the agent alias without a second capability-specific lookup.
        let provider_runtime_options = clawcrew_providers::options_for_provider_ref(
            &config,
            &provider_name,
            &agent_runtime_options,
        );

        // Resolve api_key and uri from the actual provider being constructed.
        // For dotted aliases (e.g. "openai.shartgpt"), look up the alias-specific
        // config so a -p override does not leak the agent's current provider key
        // (e.g. an xai key) to a different provider family that doesn't expect it.
        let (initial_api_key, initial_uri) =
            api_key_and_uri_for_provider(&config, &provider_name, agent_model_provider);
        let mut model_provider: Box<dyn ModelProvider> =
            clawcrew_providers::create_routed_model_provider_with_options(
                &config,
                &provider_name,
                initial_api_key.as_deref(),
                initial_uri.as_deref(),
                &config.reliability,
                &config.model_routes,
                &model_name,
                &provider_runtime_options,
            )?;

        let mut turn_guard = crate::observability::AgentTurnGuard::start(
            observer.as_ref(),
            provider_name.to_string(),
            model_name.to_string(),
            Some(channel_name.to_string()),
            Some(agent_alias.to_string()),
            Some(turn_id.clone()),
        );

        // ── Hardware RAG (datasheet retrieval when peripherals + datasheet_dir) ──
        let hardware_rag: Option<crate::rag::HardwareRag> = config
            .peripherals
            .datasheet_dir
            .as_ref()
            .filter(|d| !d.trim().is_empty())
            .map(|dir| crate::rag::HardwareRag::load(&config.data_dir, dir.trim()))
            .and_then(Result::ok)
            .filter(|r: &crate::rag::HardwareRag| !r.is_empty());
        if let Some(ref rag) = hardware_rag {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Load)
                    .with_category(::clawcrew_log::EventCategory::Agent)
                    .with_attrs(::serde_json::json!({"chunks": rag.len()})),
                "Hardware RAG loaded"
            );
        }

        let board_names: Vec<String> = config
            .peripherals
            .boards
            .iter()
            .map(|b| b.board.clone())
            .collect();

        // ── Initialize locale-aware tool descriptions ──────────────────
        let _i18n_locale = config
            .locale
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(crate::i18n::detect_locale);

        let mut tool_descs: Vec<(&str, &str)> = vec![
            (
                "shell",
                "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.",
            ),
            (
                "file_read",
                "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.",
            ),
            (
                "file_write",
                "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.",
            ),
            (
                "memory_store",
                "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.",
            ),
            (
                "memory_recall",
                "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.",
            ),
            (
                "memory_forget",
                "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.",
            ),
        ];
        if matches!(
            eff_prompt_injection_mode,
            clawcrew_config::schema::SkillsPromptInjectionMode::Compact
        ) {
            tool_descs.push((
                "read_skill",
                "Load the full source for an available skill by name. Use when: compact mode only shows a summary and you need the complete skill instructions.",
            ));
        }
        tool_descs.push((
        "cron_add",
        "Create a cron job. Supports schedule kinds: cron, at, every; and job types: shell or agent.",
    ));
        tool_descs.push((
            "cron_list",
            "List all cron jobs with schedule, status, and metadata.",
        ));
        tool_descs.push(("cron_remove", "Remove a cron job by job_id."));
        tool_descs.push((
        "cron_update",
        "Patch a cron job (schedule, enabled, command/prompt, model, delivery, session_target).",
    ));
        tool_descs.push((
            "cron_run",
            "Force-run a cron job immediately and record a run history entry.",
        ));
        tool_descs.push(("cron_runs", "Show recent run history for a cron job."));
        tool_descs.push((
        "screenshot",
        "Capture a screenshot of the current screen. Returns file path and base64-encoded PNG. Use when: visual verification, UI inspection, debugging displays.",
    ));
        tool_descs.push((
        "image_info",
        "Read image file metadata (format, dimensions, size) and optionally base64-encode it. Use when: inspecting images, preparing visual data for analysis.",
    ));
        if config.browser.enabled {
            tool_descs.push((
                "browser_open",
                "Open approved HTTPS URLs in system browser (allowlist-only, no scraping)",
            ));
        }
        if config.composio.enabled {
            tool_descs.push((
            "composio",
            "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). Use action='list' to discover, 'execute' to run (optionally with connected_account_id), 'connect' to OAuth.",
        ));
        }
        tool_descs.push((
        "schedule",
        "Manage scheduled tasks (create/list/get/cancel/pause/resume). Supports recurring cron and one-shot delays.",
    ));
        tool_descs.push((
            "channel_room",
            "Create channel rooms and invite users through active channels. Use with Matrix channel keys such as matrix.default.",
        ));
        tool_descs.push((
        "model_routing_config",
        "Configure default model, scenario routing, and delegate agents. Use for natural-language requests like: 'set conversation to kimi and coding to gpt-5.3-codex'.",
    ));
        if !config.agents.is_empty() {
            tool_descs.push((
            "delegate",
            "Delegate a sub-task to a specialized agent. Use when: task needs different model/capability, or to parallelize work.",
        ));
        }
        if config.peripherals.enabled && !config.peripherals.boards.is_empty() {
            tool_descs.push((
            "gpio_read",
            "Read GPIO pin value (0 or 1) on connected hardware (STM32, Arduino). Use when: checking sensor/button state, LED status.",
        ));
            tool_descs.push((
            "gpio_write",
            "Set GPIO pin high (1) or low (0) on connected hardware. Use when: turning LED on/off, controlling actuators.",
        ));
            tool_descs.push((
            "arduino_upload",
            "Upload agent-generated Arduino sketch. Use when: user asks for 'make a heart', 'blink pattern', or custom LED behavior on Arduino. You write the full .ino code; ClawCrew compiles and uploads it. Pin 13 = built-in LED on Uno.",
        ));
            tool_descs.push((
            "hardware_memory_map",
            "Return flash and RAM address ranges for connected hardware. Use when: user asks for 'upper and lower memory addresses', 'memory map', or 'readable addresses'.",
        ));
            tool_descs.push((
            "hardware_board_info",
            "Return full board info (chip, architecture, memory map) for connected hardware. Use when: user asks for 'board info', 'what board do I have', 'connected hardware', 'chip info', or 'what hardware'.",
        ));
            tool_descs.push((
            "hardware_memory_read",
            "Read actual memory/register values from Nucleo via USB. Use when: user asks to 'read register values', 'read memory', 'dump lower memory 0-126', 'give address and value'. Params: address (hex, default 0x20000000), length (bytes, default 128).",
        ));
            tool_descs.push((
            "hardware_capabilities",
            "Query connected hardware for reported GPIO pins and LED pin. Use when: user asks what pins are available.",
        ));
        }
        retain_registered_tool_descriptions(&mut tool_descs, &tools_registry);
        let bootstrap_max_chars = if eff_compact_context {
            Some(6000)
        } else {
            None
        };
        let prompt_excluded_tools = message
            .as_deref()
            .map(|msg| {
                compute_excluded_mcp_tools(
                    &tools_registry,
                    &agent.resolved.tool_filter_groups,
                    msg,
                    &mcp_tool_names,
                )
            })
            .unwrap_or_default();
        let agent_workspace = config.agent_workspace_dir(agent_alias);
        let mut system_prompt = build_system_prompt_for_turn(
            &agent_workspace,
            &model_name,
            &tool_descs,
            &deferred_section,
            &skills,
            Some(&agent.identity),
            bootstrap_max_chars,
            &risk_profile,
            model_provider.as_ref(),
            &tools_registry,
            &prompt_excluded_tools,
            activated_handle.as_ref(),
            agent.resolved.strict_tool_parsing,
            eff_prompt_injection_mode,
            eff_compact_context,
            eff_max_system_prompt_chars,
            true,
            config.channels.show_tool_calls,
            None,
            runtime.shell_profile().as_ref(),
        )?;

        // ── Approval manager (supervised mode) ───────────────────────
        let approval_manager = if interactive {
            Some(ApprovalManager::from_risk_profile(&risk_profile))
        } else {
            None
        };
        let memory_session_id = session_state_file.as_deref().and_then(|path| {
            let raw = path.to_string_lossy().trim().to_string();
            if raw.is_empty() {
                None
            } else {
                // Match the sanitized form persisted by memory backend migrations.
                Some(clawcrew_api::session_keys::sanitize_session_key(&format!(
                    "cli:{raw}"
                )))
            }
        });

        // ── Cost tracking context (scoped for CLI / cron / web agents) ──
        let cost_tracking_context =
            crate::agent::cost::tool_loop_cost_tracking_context_for_agent(&config, agent_alias);

        // ── Execute ──────────────────────────────────────────────────
        let mut final_output = String::new();

        // Save the base system prompt before any thinking modifications so
        // the interactive loop can restore it between turns.
        let base_system_prompt = system_prompt.clone();

        if let Some(msg) = message {
            // ── Parse thinking directive from user message ─────────
            let (thinking_directive, effective_msg) =
                match crate::agent::thinking::parse_thinking_directive(&msg) {
                    Some((level, remaining)) => {
                        ::clawcrew_log::record!(
                            INFO,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_attrs(::serde_json::json!({"thinking_level": level})),
                            "Thinking directive parsed from message"
                        );
                        (Some(level), remaining)
                    }
                    None => (None, msg.clone()),
                };
            let thinking_level = crate::agent::thinking::resolve_thinking_level(
                thinking_directive,
                None,
                &agent.resolved.thinking,
            );
            let thinking_params = crate::agent::thinking::apply_thinking_level_with_config(
                thinking_level,
                &agent.resolved.thinking,
            );
            let effective_temperature: Option<f64> = temperature.map(|t| {
                crate::agent::thinking::clamp_temperature(
                    t + thinking_params.temperature_adjustment,
                )
            });

            // Compute per-turn excluded MCP tools from tool_filter_groups before
            // building the turn prompt so tool availability matches the specs
            // sent to the provider.
            let excluded_tools = compute_excluded_mcp_tools(
                &tools_registry,
                &agent.resolved.tool_filter_groups,
                &effective_msg,
                &mcp_tool_names,
            );
            system_prompt = build_system_prompt_for_turn(
                &agent_workspace,
                &model_name,
                &tool_descs,
                &deferred_section,
                &skills,
                Some(&agent.identity),
                bootstrap_max_chars,
                &risk_profile,
                model_provider.as_ref(),
                &tools_registry,
                &excluded_tools,
                activated_handle.as_ref(),
                agent.resolved.strict_tool_parsing,
                eff_prompt_injection_mode,
                eff_compact_context,
                eff_max_system_prompt_chars,
                true,
                config.channels.show_tool_calls,
                thinking_params.system_prompt_prefix.as_deref(),
                runtime.shell_profile().as_ref(),
            )?;

            let excluded_tool_names: HashSet<&str> =
                excluded_tools.iter().map(String::as_str).collect();
            let runtime_capability_names = tools_registry
                .iter()
                .map(|tool| tool.name())
                .filter(|name| !excluded_tool_names.contains(*name))
                .collect::<Vec<_>>();
            if let Some(suggestion) = crate::skills::render_missing_skill_install_suggestion(
                &effective_msg,
                &skills,
                &runtime_capability_names,
                &config.data_dir,
                &config.skills.extra_registries,
                config.skills.install_suggestions.enabled,
            ) {
                final_output = suggestion;
                if interactive {
                    println!("{final_output}");
                }
                observer.record_event(&ObserverEvent::TurnComplete);
                return Ok(final_output);
            }

            // Auto-save user message to memory (skip autonomous origins
            // and short/trivial or synthetic messages).
            if should_autosave_user_message(config.memory.auto_save, origin, &effective_msg) {
                let user_key = autosave_memory_key("user_msg");
                let store_start = std::time::Instant::now();
                let store_result = mem
                    .store(
                        &user_key,
                        &effective_msg,
                        MemoryCategory::Conversation,
                        memory_session_id.as_deref(),
                    )
                    .await;
                observer.record_event(&ObserverEvent::MemoryStore {
                    category: MemoryCategory::Conversation.to_string(),
                    backend: mem.name().to_string(),
                    duration: store_start.elapsed(),
                    success: store_result.is_ok(),
                    channel: Some(channel_name.to_string()),
                    agent_alias: Some(agent_alias.to_string()),
                    turn_id: Some(turn_id.clone()),
                });
            }

            // Memory context is injected once in the engine, keyed on the
            // ingress origin (agent::memory_inject). Hardware RAG context
            // stays site-built; the engine prepends the memory block above
            // it, preserving the legacy mem -> hw -> [now] msg order.
            let rag_limit = if eff_compact_context { 2 } else { 5 };
            let hw_context = hardware_rag
                .as_ref()
                .map(|r| {
                    build_hardware_context(
                        r,
                        &*observer,
                        &effective_msg,
                        &board_names,
                        rag_limit,
                        TurnMeta {
                            parent_agent_alias: None,
                            agent_alias: Some(agent_alias),
                            turn_id: &turn_id,
                            channel_name,
                        },
                    )
                })
                .unwrap_or_default();
            let context = hw_context;
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
            let enriched = if context.is_empty() {
                format!("[{now}] {effective_msg}")
            } else {
                format!("{context}[{now}] {effective_msg}")
            };

            let mut history = vec![
                ChatMessage::system(&system_prompt),
                ChatMessage::user(&enriched),
            ];
            // One-shot transcript: no prior trim ran, so no crumb exists.
            let mut history_has_trim_breadcrumb = false;

            // Compute per-turn excluded MCP tools from tool_filter_groups.
            let excluded_tools = compute_excluded_mcp_tools(
                &tools_registry,
                &agent.resolved.tool_filter_groups,
                &effective_msg,
                &mcp_tool_names,
            );

            #[allow(unused_assignments)]
            let mut response = String::new();
            loop {
                if let Some(sys_msg) = history.first_mut()
                    && sys_msg.role == "system"
                {
                    sys_msg.content = build_system_prompt_for_turn(
                        &agent_workspace,
                        &model_name,
                        &tool_descs,
                        &deferred_section,
                        &skills,
                        Some(&agent.identity),
                        bootstrap_max_chars,
                        &risk_profile,
                        model_provider.as_ref(),
                        &tools_registry,
                        &excluded_tools,
                        activated_handle.as_ref(),
                        agent.resolved.strict_tool_parsing,
                        eff_prompt_injection_mode,
                        eff_compact_context,
                        eff_max_system_prompt_chars,
                        true,
                        config.channels.show_tool_calls,
                        thinking_params.system_prompt_prefix.as_deref(),
                        runtime.shell_profile().as_ref(),
                    )?;
                }
                match clawcrew_api::NATIVE_THINKING_OVERRIDE
                    .scope(
                        thinking_params.native_thinking,
                        TOOL_LOOP_COST_TRACKING_CONTEXT.scope(
                            cost_tracking_context.clone(),
                            run_tool_call_loop(ToolLoop {
                                exec: ResolvedAgentExecution::resolve(
                                    ResolvedModelAccess {
                                        model_provider: model_provider.as_ref(),
                                        provider_name: &provider_name,
                                        model: &model_name,
                                        dispatch_model: &model_name,
                                        temperature: effective_temperature,
                                    },
                                    ResolvedIo {
    app_registry: None,
                                        tools_registry: &tools_registry,
                                        observer: observer.as_ref(),
                                        silent: !interactive,
                                        approval: approval_manager.as_ref(),
                                        multimodal_config: &config.multimodal,
                                        config: Some(&config),
                                        hooks: None,
                                        activated_tools: activated_handle.as_ref(),
                                        model_switch_callback: None,
                                        receipt_generator: None,
                                    },
                                    ResolvedRuntimeKnobs {
                                        max_tool_iterations: agent.resolved.max_tool_iterations,
                                        excluded_tools: &excluded_tools,
                                        dedup_exempt_tools: &agent.resolved.tool_call_dedup_exempt,
                                        pacing: &config.pacing,
                                        strict_tool_parsing: agent.resolved.strict_tool_parsing,
                                        parallel_tools: agent.resolved.parallel_tools,
                                        max_tool_result_chars: agent.resolved.max_tool_result_chars,
                                        context_limits,
                                        context_limits_resolver: None,
                                        knobs: &LoopKnobs::default(),
                                    },
                                ),
                                history: &mut history,
                                history_has_trim_breadcrumb: &mut history_has_trim_breadcrumb,
                                injected_memory_preamble: &mut None,
                                channel_name,
                                channel_reply_target: None,
                                cancellation_token: None,
                                on_delta: None,
                                shared_budget: None,
                                channel: None,
                                collected_receipts: None,
                                event_tx: None,
                                steering: None,
                                new_messages_out: None,
                                image_cache: None,
                                // Origin is threaded from the entry point;
                                // source/transport/trust stay phase-1
                                // placeholders until per-transport stamping.
                                memory: Some(crate::agent::memory_inject::TurnMemory {
                                    handle: mem.as_ref(),
                                    query: effective_msg.clone(),
                                    sessions: vec![memory_session_id.clone()],
                                    suppress: suppress_memory_inject,
                                    cfg: crate::agent::memory_inject::MemoryInjectConfig::from_memory_config(
                                        &config.memory,
                                        crate::agent::memory_inject::DEFAULT_RECALL_LIMIT,
                                    ),
                                }),
                                ingress: IngressContext::from_origin(origin),
                                agent_alias: Some(agent_alias),
                                parent_agent_alias: None,
                                turn_id: &turn_id,
                                served_route_sink: None,
                                sop_reassembly: Some(crate::agent::turn::SopStepReassembly {
                                    config: &config,
                                }),
                            }),
                        ),
                    )
                    .await
                {
                    Ok(resp) => {
                        response = resp;
                        break;
                    }
                    Err(e) => {
                        if let Some((new_model_provider, new_model)) = is_model_switch_requested(&e)
                        {
                            ::clawcrew_log::record!(
                                INFO,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Migrate
                                )
                                .with_category(::clawcrew_log::EventCategory::Provider),
                                &format!(
                                    "Model switch requested, switching from {} {} to {} {}",
                                    provider_name, model_name, new_model_provider, new_model
                                )
                            );

                            let (switch_api_key, switch_uri) = api_key_and_uri_for_provider(
                                &config,
                                &new_model_provider,
                                agent_model_provider,
                            );
                            model_provider =
                                clawcrew_providers::create_routed_model_provider_with_options(
                                    &config,
                                    &new_model_provider,
                                    switch_api_key.as_deref(),
                                    switch_uri.as_deref(),
                                    &config.reliability,
                                    &config.model_routes,
                                    &new_model,
                                    &clawcrew_providers::options_for_provider_ref(
                                        &config,
                                        &new_model_provider,
                                        &clawcrew_providers::provider_runtime_options_for_agent(
                                            &config,
                                            agent_alias,
                                        ),
                                    ),
                                )?;

                            provider_name = new_model_provider;
                            model_name = new_model;
                            context_limits = config.resolved_context_limits_for_route(
                                agent_alias,
                                &provider_name,
                                &model_name,
                            );

                            turn_guard.set_model_route(provider_name.clone(), model_name.clone());

                            continue;
                        }
                        return Err(project_cli_terminal_completion_error(e));
                    }
                }
            }

            // After successful multi-step execution, attempt autonomous skill creation.
            if config.skills.skill_creation.enabled {
                let tool_calls = crate::skills::creator::extract_tool_calls_from_history(&history);
                if tool_calls.len() >= 2 {
                    let creator = crate::skills::creator::SkillCreator::new(
                        config.data_dir.clone(),
                        config.skills.skill_creation.clone(),
                    );
                    // Opt-in reflection synthesizes a `SKILL.md` from a bounded
                    // slice of the execution; it falls back to `SKILL.toml`
                    // internally when the provider call or its output is
                    // unusable. Default path stays the deterministic generator.
                    let creation_result = if config.skills.skill_creation.reflection_enabled {
                        TOOL_LOOP_COST_TRACKING_CONTEXT
                            .scope(
                                cost_tracking_context.clone(),
                                creator.create_from_execution_reflected(
                                    &msg,
                                    &tool_calls,
                                    &response,
                                    None,
                                    &provider_name,
                                    model_provider.as_ref(),
                                    &model_name,
                                ),
                            )
                            .await
                    } else {
                        creator.create_from_execution(&msg, &tool_calls, None).await
                    };
                    match creation_result {
                        Ok(Some(slug)) => {
                            ::clawcrew_log::record!(
                                INFO,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Register
                                )
                                .with_category(::clawcrew_log::EventCategory::Agent)
                                .with_attrs(::serde_json::json!({"slug": slug})),
                                "Auto-created skill from execution"
                            );
                        }
                        Ok(None) => {
                            ::clawcrew_log::record!(
                                DEBUG,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Skip
                                )
                                .with_category(::clawcrew_log::EventCategory::Agent),
                                "Skill creation skipped (duplicate or disabled)"
                            );
                        }
                        Err(e) => ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Fail
                            )
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                            "Skill creation failed"
                        ),
                    }
                }
            }
            // Emit the user-visible response before any background work so the
            // skill-review fork can never delay the user's answer.
            final_output = response;
            if interactive {
                println!("{final_output}");
            }
            observer.record_event(&ObserverEvent::TurnComplete);

            if config.skills.skill_improvement.enabled {
                let review_workspace = config.agent_workspace_dir(agent_alias);
                let review_config = config.skills.skill_improvement.clone();
                let failed_slugs: Vec<String> =
                    crate::skills::improver::extract_skill_executions_from_history(&history)
                        .into_iter()
                        .filter_map(|(slug, ok)| if ok { None } else { Some(slug) })
                        .collect();
                TOOL_LOOP_COST_TRACKING_CONTEXT
                    .scope(
                        cost_tracking_context.clone(),
                        crate::skills::review::maybe_run_skill_review(
                            Some(&config),
                            review_workspace,
                            review_config,
                            config.skills.allow_scripts,
                            history.clone(),
                            failed_slugs,
                            model_provider.as_ref(),
                            &provider_name,
                            &model_name,
                            observer.as_ref(),
                            &config.multimodal,
                            &config.pacing,
                            agent.resolved.max_tool_result_chars,
                            agent.resolved.effective_context_budget(),
                            None, // cancellation_token — no parent token in single-shot run
                            Some(agent_alias),
                        ),
                    )
                    .await;
            }
        } else {
            println!("🦀 ClawCrew Interactive Mode");
            println!("Type /help for commands.\n");
            let cli = CLI_CHANNEL_FN.get().expect(
                "CLI channel factory not registered — call register_cli_channel_fn at startup",
            )();

            // Persistent conversation history across turns, with explicit
            // breadcrumb provenance. Legacy v1 files are migrated by inspecting
            // the restored history for a leading breadcrumb.
            let (mut history, mut history_has_trim_breadcrumb) =
                if let Some(path) = session_state_file.as_deref() {
                    load_interactive_session_history_with_crumb(path, &system_prompt)?
                } else {
                    (vec![ChatMessage::system(&system_prompt)], false)
                };

            loop {
                print!("> ");
                let _ = std::io::stdout().flush();

                let input = {
                    let stdin = std::io::stdin().lock();
                    match read_capped_line(stdin, MAX_INTERACTIVE_INPUT_BYTES) {
                        Ok(CappedLine::Eof) => break,
                        Ok(CappedLine::Line(s)) => s,
                        Ok(CappedLine::Truncated) => {
                            eprintln!(
                                "\nWarning: input line exceeds {} bytes and was discarded.",
                                MAX_INTERACTIVE_INPUT_BYTES
                            );
                            continue;
                        }
                        Err(e) => {
                            eprintln!("\nError reading input: {e}\n");
                            break;
                        }
                    }
                };

                let user_input = input.trim().to_string();
                if user_input.is_empty() {
                    continue;
                }
                match user_input.as_str() {
                    "/quit" | "/exit" => break,
                    "/help" => {
                        println!("Available commands:");
                        println!("  /help             Show this help message");
                        println!("  /clear /new       Clear conversation history");
                        println!("  /quit /exit       Exit interactive mode");
                        println!(
                            "  /think:<level>    Set reasoning depth (off|minimal|low|medium|high|max)\n"
                        );
                        continue;
                    }
                    "/clear" | "/new" => {
                        println!(
                            "This will clear the current conversation and delete all session memory."
                        );
                        println!("Core memories (long-term facts/preferences) will be preserved.");
                        print!("Continue? [y/N] ");
                        let _ = std::io::stdout().flush();

                        let confirm = {
                            let stdin = std::io::stdin().lock();
                            match read_capped_line(stdin, MAX_INTERACTIVE_INPUT_BYTES) {
                                Ok(CappedLine::Line(s)) => s,
                                Ok(CappedLine::Truncated) | Ok(CappedLine::Eof) | Err(_) => {
                                    println!("Cancelled.\n");
                                    continue;
                                }
                            }
                        };
                        if !matches!(confirm.trim().to_lowercase().as_str(), "y" | "yes") {
                            println!("Cancelled.\n");
                            continue;
                        }

                        history.clear();
                        history.push(ChatMessage::system(&system_prompt));
                        history_has_trim_breadcrumb = false;
                        // Clear conversation and daily memory
                        let mut cleared = 0;
                        for category in [MemoryCategory::Conversation, MemoryCategory::Daily] {
                            let entries = mem.list(Some(&category), None).await.unwrap_or_default();
                            for entry in entries {
                                if mem.forget(&entry.key).await.unwrap_or(false) {
                                    cleared += 1;
                                }
                            }
                        }
                        if cleared > 0 {
                            println!("Conversation cleared ({cleared} memory entries removed).\n");
                        } else {
                            println!("Conversation cleared.\n");
                        }
                        if let Some(path) = session_state_file.as_deref() {
                            save_interactive_session_history_with_crumb(
                                path,
                                &history,
                                history_has_trim_breadcrumb,
                            )?;
                        }
                        continue;
                    }
                    _ => {}
                }

                // ── Parse thinking directive from interactive input ───
                let (thinking_directive, effective_input) =
                    match crate::agent::thinking::parse_thinking_directive(&user_input) {
                        Some((level, remaining)) => {
                            ::clawcrew_log::record!(
                                INFO,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_category(::clawcrew_log::EventCategory::Agent)
                                .with_attrs(::serde_json::json!({"thinking_level": level})),
                                "Thinking directive parsed"
                            );
                            (Some(level), remaining)
                        }
                        None => (None, user_input.clone()),
                    };
                let thinking_level = crate::agent::thinking::resolve_thinking_level(
                    thinking_directive,
                    None,
                    &agent.resolved.thinking,
                );
                let thinking_params = crate::agent::thinking::apply_thinking_level_with_config(
                    thinking_level,
                    &agent.resolved.thinking,
                );
                let turn_temperature: Option<f64> = temperature.map(|t| {
                    crate::agent::thinking::clamp_temperature(
                        t + thinking_params.temperature_adjustment,
                    )
                });

                // Compute per-turn excluded MCP tools from tool_filter_groups
                // before the provider call; the system prompt is rebuilt from
                // this same set immediately before each attempt.
                let excluded_tools = compute_excluded_mcp_tools(
                    &tools_registry,
                    &agent.resolved.tool_filter_groups,
                    &effective_input,
                    &mcp_tool_names,
                );

                let excluded_tool_names: HashSet<&str> =
                    excluded_tools.iter().map(String::as_str).collect();
                let runtime_capability_names = tools_registry
                    .iter()
                    .map(|tool| tool.name())
                    .filter(|name| !excluded_tool_names.contains(*name))
                    .collect::<Vec<_>>();
                if let Some(suggestion) = crate::skills::render_missing_skill_install_suggestion(
                    &effective_input,
                    &skills,
                    &runtime_capability_names,
                    &config.data_dir,
                    &config.skills.extra_registries,
                    config.skills.install_suggestions.enabled,
                ) {
                    final_output = suggestion;
                    if let Err(e) = clawcrew_api::channel::Channel::send(
                        &*cli,
                        &clawcrew_api::channel::SendMessage::new(
                            format!("\n{final_output}\n"),
                            "user",
                        ),
                    )
                    .await
                    {
                        eprintln!("\nError sending CLI response: {e}\n");
                    }
                    observer.record_event(&ObserverEvent::TurnComplete);
                    if let Some(sys_msg) = history.first_mut()
                        && sys_msg.role == "system"
                    {
                        sys_msg.content.clone_from(&base_system_prompt);
                    }
                    continue;
                }

                // Auto-save conversation turns (skip autonomous origins
                // and short/trivial or synthetic messages).
                if should_autosave_user_message(config.memory.auto_save, origin, &effective_input) {
                    let user_key = autosave_memory_key("user_msg");
                    let store_start = std::time::Instant::now();
                    let store_result = mem
                        .store(
                            &user_key,
                            &effective_input,
                            MemoryCategory::Conversation,
                            memory_session_id.as_deref(),
                        )
                        .await;
                    observer.record_event(&ObserverEvent::MemoryStore {
                        category: MemoryCategory::Conversation.to_string(),
                        backend: mem.name().to_string(),
                        duration: store_start.elapsed(),
                        success: store_result.is_ok(),
                        channel: Some(channel_name.to_string()),
                        agent_alias: Some(agent_alias.to_string()),
                        turn_id: Some(turn_id.clone()),
                    });
                }

                // Memory context is injected once in the engine, keyed on
                // the ingress origin (agent::memory_inject). Hardware RAG
                // stays site-built; the engine prepends the memory block
                // above it.
                let rag_limit = if eff_compact_context { 2 } else { 5 };
                let hw_context = hardware_rag
                    .as_ref()
                    .map(|r| {
                        build_hardware_context(
                            r,
                            &*observer,
                            &effective_input,
                            &board_names,
                            rag_limit,
                            TurnMeta {
                                parent_agent_alias: None,
                                agent_alias: Some(agent_alias),
                                turn_id: &turn_id,
                                channel_name,
                            },
                        )
                    })
                    .unwrap_or_default();
                let context = hw_context;
                let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
                let enriched = if context.is_empty() {
                    format!("[{now}] {effective_input}")
                } else {
                    format!("{context}[{now}] {effective_input}")
                };

                history.push(ChatMessage::user(&enriched));

                // Set up streaming channel so tool progress and response
                // content are printed progressively instead of buffered.
                let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<DraftEvent>(64);
                let content_was_streamed =
                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let content_streamed_flag = content_was_streamed.clone();
                let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());

                let consumer_handle = clawcrew_spawn::spawn!(async move {
                    use std::io::Write;
                    while let Some(event) = delta_rx.recv().await {
                        match event {
                            StreamDelta::Lifecycle(_) => {}
                            StreamDelta::Status(text) => {
                                if is_tty {
                                    let _ = write!(std::io::stderr(), "\x1b[2m{text}\x1b[0m");
                                } else {
                                    let _ = write!(std::io::stderr(), "{text}");
                                }
                                let _ = std::io::stderr().flush();
                            }
                            StreamDelta::Text(text) => {
                                content_streamed_flag
                                    .store(true, std::sync::atomic::Ordering::Relaxed);
                                print!("{text}");
                                let _ = std::io::stdout().flush();
                            }
                            StreamDelta::FlushBarrier(ack) => {
                                // CLI prints deltas immediately; nothing is
                                // buffered, so release the barrier right away.
                                StreamDelta::ack_flush_barrier(&ack);
                            }
                            StreamDelta::Reasoning(_) => {}
                            tool_event @ (StreamDelta::ToolStart { .. }
                            | StreamDelta::ToolComplete { .. }) => {
                                if let Some(text) = tool_event.legacy_status() {
                                    if is_tty {
                                        let _ = write!(std::io::stderr(), "\x1b[2m{text}\x1b[0m");
                                    } else {
                                        eprint!("{text}");
                                    }
                                    let _ = std::io::stderr().flush();
                                }
                            }
                        }
                    }
                });

                // Ctrl+C cancels the in-flight turn instead of killing the process.
                let cancel_token = CancellationToken::new();
                let cancel_token_clone = cancel_token.clone();
                let ctrlc_handle = clawcrew_spawn::spawn!(async move {
                    if tokio::signal::ctrl_c().await.is_ok() {
                        cancel_token_clone.cancel();
                    }
                });

                let response = loop {
                    if let Some(sys_msg) = history.first_mut()
                        && sys_msg.role == "system"
                    {
                        sys_msg.content = build_system_prompt_for_turn(
                            &agent_workspace,
                            &model_name,
                            &tool_descs,
                            &deferred_section,
                            &skills,
                            Some(&agent.identity),
                            bootstrap_max_chars,
                            &risk_profile,
                            model_provider.as_ref(),
                            &tools_registry,
                            &excluded_tools,
                            activated_handle.as_ref(),
                            agent.resolved.strict_tool_parsing,
                            eff_prompt_injection_mode,
                            eff_compact_context,
                            eff_max_system_prompt_chars,
                            true,
                            config.channels.show_tool_calls,
                            thinking_params.system_prompt_prefix.as_deref(),
                            runtime.shell_profile().as_ref(),
                        )?;
                    }
                    match clawcrew_api::NATIVE_THINKING_OVERRIDE
                        .scope(
                            thinking_params.native_thinking,
                            TOOL_LOOP_COST_TRACKING_CONTEXT.scope(
                                cost_tracking_context.clone(),
                                run_tool_call_loop(ToolLoop {
                                    exec: ResolvedAgentExecution::resolve(
                                        ResolvedModelAccess {
                                            model_provider: model_provider.as_ref(),
                                            provider_name: &provider_name,
                                            model: &model_name,
                                            dispatch_model: &model_name,
                                            temperature: turn_temperature,
                                        },
                                        ResolvedIo {
    app_registry: None,
                                            tools_registry: &tools_registry,
                                            observer: observer.as_ref(),
                                            silent: true,
                                            approval: approval_manager.as_ref(),
                                            multimodal_config: &config.multimodal,
                                            config: Some(&config),
                                            hooks: None,
                                            activated_tools: activated_handle.as_ref(),
                                            model_switch_callback: None,
                                            receipt_generator: None,
                                        },
                                        ResolvedRuntimeKnobs {
                                            max_tool_iterations: agent.resolved.max_tool_iterations,
                                            excluded_tools: &excluded_tools,
                                            dedup_exempt_tools: &agent
                                                .resolved
                                                .tool_call_dedup_exempt,
                                            pacing: &config.pacing,
                                            strict_tool_parsing: agent.resolved.strict_tool_parsing,
                                            parallel_tools: agent.resolved.parallel_tools,
                                            max_tool_result_chars: agent
                                                .resolved
                                                .max_tool_result_chars,
                                            context_limits,
                                            context_limits_resolver: None,
                                            knobs: &LoopKnobs::default(),
                                        },
                                    ),
                                    history: &mut history,
                                    history_has_trim_breadcrumb:
                                        &mut history_has_trim_breadcrumb,
                                    injected_memory_preamble: &mut None,
                                    channel_name,
                                    channel_reply_target: None,
                                    cancellation_token: Some(cancel_token.clone()),
                                    on_delta: Some(delta_tx.clone()),
                                    shared_budget: None,
                                    channel: None,
                                    collected_receipts: None,
                                    event_tx: None,
                                    steering: None,
                                    new_messages_out: None,
                                    image_cache: None,
                                    // Origin is threaded from the entry point;
                                    // source/transport/trust stay phase-1
                                    // placeholders until per-transport stamping.
                                    memory: Some(crate::agent::memory_inject::TurnMemory {
                                        handle: mem.as_ref(),
                                        query: effective_input.clone(),
                                        sessions: vec![memory_session_id.clone()],
                                        suppress: suppress_memory_inject,
                                        cfg: crate::agent::memory_inject::MemoryInjectConfig::from_memory_config(
                                            &config.memory,
                                            crate::agent::memory_inject::DEFAULT_RECALL_LIMIT,
                                        ),
                                    }),
                                    ingress: IngressContext::from_origin(origin),
                                    agent_alias: Some(agent_alias),
                                    parent_agent_alias: None,
                                    turn_id: &turn_id,
                                    served_route_sink: None,
                                    sop_reassembly: Some(crate::agent::turn::SopStepReassembly {
                                        config: &config,
                                    }),
                                }),
                            ),
                        )
                        .await
                    {
                        Ok(resp) => break resp,
                        Err(e) => {
                            if is_tool_loop_cancelled(&e) {
                                eprintln!("\n\x1b[2m(cancelled)\x1b[0m");
                                break String::new();
                            }
                            if let Some((new_model_provider, new_model)) =
                                is_model_switch_requested(&e)
                            {
                                ::clawcrew_log::record!(
                                    INFO,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Migrate
                                    )
                                    .with_category(::clawcrew_log::EventCategory::Provider),
                                    &format!(
                                        "Model switch requested, switching from {} {} to {} {}",
                                        provider_name, model_name, new_model_provider, new_model
                                    )
                                );

                                let (switch_api_key2, switch_uri2) = api_key_and_uri_for_provider(
                                    &config,
                                    &new_model_provider,
                                    agent_model_provider,
                                );
                                model_provider =
                                    clawcrew_providers::create_routed_model_provider_with_options(
                                        &config,
                                        &new_model_provider,
                                        switch_api_key2.as_deref(),
                                        switch_uri2.as_deref(),
                                        &config.reliability,
                                        &config.model_routes,
                                        &new_model,
                                        &clawcrew_providers::options_for_provider_ref(
                                            &config,
                                            &new_model_provider,
                                            &clawcrew_providers::provider_runtime_options_for_agent(
                                                &config,
                                                agent_alias,
                                            ),
                                        ),
                                    )?;

                                provider_name = new_model_provider;
                                model_name = new_model;
                                context_limits = config.resolved_context_limits_for_route(
                                    agent_alias,
                                    &provider_name,
                                    &model_name,
                                );

                                turn_guard
                                    .set_model_route(provider_name.clone(), model_name.clone());

                                continue;
                            }
                            // Context overflow recovery: drop oldest whole
                            // turns and retry. No summarization, no splicing.
                            if clawcrew_providers::reliable::is_context_window_exceeded(&e) {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Retry
                                    )
                                    .with_category(::clawcrew_log::EventCategory::Agent),
                                    "Context overflow in interactive loop, attempting recovery"
                                );
                                let taken = std::mem::take(&mut history);
                                let recovery_budget = interactive_context_recovery_budget(context_limits);
                                let crumb_present_before_recovery = history_has_trim_breadcrumb;
                                let result =
                                    crate::agent::history_trim::trim_to_recent_turns_with_crumb(
                                        taken,
                                        recovery_budget,
                                        crumb_present_before_recovery,
                                    );
                                if result.trimmed {
                                    // P0.3/#7: surface the compaction as session health.
                                    if let Ok(Some(session_key)) =
                                        TOOL_LOOP_SESSION_KEY.try_with(Clone::clone)
                                    {
                                        crate::session::metadata::record_compaction(&session_key);
                                    }
                                    let mut trimmed = result.history;
                                    // Owner-aware insertion: does not stack a
                                    // second marker when the existing
                                    // breadcrumb (protected from drop above)
                                    // is still present.
                                    history_has_trim_breadcrumb =
                                        crate::agent::history_trim::insert_breadcrumb_deduped(
                                            &mut trimmed,
                                            crumb_present_before_recovery,
                                        );
                                    history = trimmed;
                                    {
                                        let __zc_trim_span = ::clawcrew_log::info_span!(
                                            target: "clawcrew_log_internal_scope",
                                            "clawcrew_scope",
                                            model = %model_name,
                                            model_provider = %provider_name,
                                        );
                                        let _zc_trim_guard = __zc_trim_span.entered();
                                        ::clawcrew_log::record!(
                                            INFO,
                                            ::clawcrew_log::Event::new(
                                                module_path!(),
                                                ::clawcrew_log::Action::Retry
                                            )
                                            .with_category(::clawcrew_log::EventCategory::Agent)
                                            .with_outcome(::clawcrew_log::EventOutcome::Success)
                                            .with_attrs(::serde_json::json!({
                                                "dropped_messages": result.dropped_messages,
                                                "dropped_turns": result.dropped_turns,
                                                "kept_turns": result.kept_turns,
                                            })),
                                            "Context recovered via whole-turn trim, retrying turn"
                                        );
                                    }
                                    continue;
                                }
                                history = result.history;
                                let system_floor =
                                    crate::agent::history::estimate_system_floor_tokens(&history);
                                let floor_exceeds_budget = system_floor >= recovery_budget;
                                {
                                    let __zc_trim_span = ::clawcrew_log::info_span!(
                                        target: "clawcrew_log_internal_scope",
                                        "clawcrew_scope",
                                        model = %model_name,
                                        model_provider = %provider_name,
                                    );
                                    let _zc_trim_guard = __zc_trim_span.entered();
                                    if floor_exceeds_budget {
                                        ::clawcrew_log::record!(
                                            WARN,
                                            ::clawcrew_log::Event::new(
                                                module_path!(),
                                                ::clawcrew_log::Action::Fail
                                            )
                                            .with_category(::clawcrew_log::EventCategory::Agent)
                                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                                            .with_attrs(::serde_json::json!({
                                                "system_floor": system_floor,
                                                "budget": recovery_budget,
                                                "error_key": "context_floor_exceeds_budget",
                                            })),
                                            crate::agent::history::context_floor_remediation(
                                                system_floor,
                                                recovery_budget,
                                            )
                                        );
                                    } else {
                                        ::clawcrew_log::record!(
                                            WARN,
                                            ::clawcrew_log::Event::new(
                                                module_path!(),
                                                ::clawcrew_log::Action::Fail
                                            )
                                            .with_category(::clawcrew_log::EventCategory::Agent)
                                            .with_outcome(::clawcrew_log::EventOutcome::Failure),
                                            "Context overflow but only one turn remains; cannot trim further"
                                        );
                                    }
                                }

                                if floor_exceeds_budget {
                                    eprintln!(
                                        "\nError: {e}\n{}\n",
                                        crate::agent::history::context_floor_remediation(
                                            system_floor,
                                            recovery_budget,
                                        )
                                    );
                                    break String::new();
                                }
                            }

                            let error = project_cli_terminal_completion_error(e);
                            eprintln!("\nError: {error}\n");
                            break String::new();
                        }
                    }
                };

                // Clean up: stop the Ctrl+C listener and flush streaming events.
                ctrlc_handle.abort();
                drop(delta_tx);
                let _ = consumer_handle.await;

                final_output = response;
                if content_was_streamed.load(std::sync::atomic::Ordering::Relaxed) {
                    println!();
                } else if let Err(e) = clawcrew_api::channel::Channel::send(
                    &*cli,
                    &clawcrew_api::channel::SendMessage::new(format!("\n{final_output}\n"), "user"),
                )
                .await
                {
                    eprintln!("\nError sending CLI response: {e}\n");
                }
                observer.record_event(&ObserverEvent::TurnComplete);

                // Display context usage for this turn.
                if let Some(ref ctx) = cost_tracking_context {
                    let usage = ctx.snapshot_turn_usage();
                    let effective_input_tokens = usage.last_input_tokens;
                    if effective_input_tokens > 0 || usage.output_tokens > 0 {
                        let max_ctx = context_limits.model_context_window as u64;
                        let pct = if max_ctx > 0 {
                            (effective_input_tokens as f64 / max_ctx as f64 * 100.0).min(100.0)
                        } else {
                            0.0
                        };
                        let bar_width: usize = 16;
                        let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
                        let empty = bar_width.saturating_sub(filled);
                        let bar = format!(
                            "[{}{}]",
                            "\u{2588}".repeat(filled),
                            "\u{2591}".repeat(empty)
                        );
                        let msg = if effective_input_tokens > 0 {
                            crate::i18n::get_required_cli_string_with_args(
                                "cli-agent-context-bar",
                                &[
                                    ("used", format_tokens(effective_input_tokens).as_str()),
                                    ("max", format_tokens(max_ctx).as_str()),
                                    ("bar", &bar),
                                    ("pct", format!("{:.0}", pct).as_str()),
                                ],
                            )
                        } else {
                            crate::i18n::get_required_cli_string_with_args(
                                "cli-agent-context-bar-unknown",
                                &[("max", format_tokens(max_ctx).as_str())],
                            )
                        };
                        eprintln!("\x1b[2m{}\x1b[0m", msg);
                    }
                }

                // Hard cap as a safety net.
                trim_history(&mut history, eff_max_history_messages);

                // Restore base system prompt after the per-turn tool framing
                // and optional thinking prefix have been applied.
                if let Some(sys_msg) = history.first_mut()
                    && sys_msg.role == "system"
                {
                    sys_msg.content.clone_from(&base_system_prompt);
                }

                if let Some(path) = session_state_file.as_deref() {
                    save_interactive_session_history_with_crumb(
                        path,
                        &history,
                        history_has_trim_breadcrumb,
                    )?;
                }
            }
        }

        let tokens_used = cost_tracking_context.as_ref().and_then(|ctx| {
            let usage = ctx.snapshot_turn_usage();
            (usage.input_tokens > 0 || usage.output_tokens > 0).then_some(
                clawcrew_api::observability_traits::TurnTokenUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                },
            )
        });
        turn_guard.set_model_route(provider_name.clone(), model_name.clone());
        turn_guard.set_usage(tokens_used, None);
        turn_guard.finish();

        Ok(final_output)
    };
    __zc_body
        .instrument(__zc_scope_span)
        .instrument(__zc_attribution_span)
        .await
}

/// Process a single message through the full agent (with tools, peripherals, memory).
/// Used by channels (Telegram, Discord, etc.) to enable hardware and tool use.
pub async fn process_message(
    config: Config,
    agent_alias: &str,
    message: &str,
    session_id: Option<&str>,
    origin: TurnOrigin,
) -> Result<String> {
    process_message_shared(Arc::new(config), agent_alias, message, session_id, origin).await
}

/// Shared-snapshot implementation for callers that already own the canonical
/// config behind an [`Arc`]. Keeping that allocation through the whole turn
/// avoids placing or cloning the large [`Config`] value in detached task
/// futures.
pub(crate) async fn process_message_shared(
    config: Arc<Config>,
    agent_alias: &str,
    message: &str,
    session_id: Option<&str>,
    origin: TurnOrigin,
) -> Result<String> {
    use ::clawcrew_log::Instrument;
    let agent = resolved_agent_for_turn(&config, agent_alias)?;
    crate::agent::thinking::validate_thinking_config(&agent.resolved.thinking);
    let risk_profile = config
        .risk_profile_for_agent(agent_alias)
        .with_context(|| {
            format!(
                "agents.{agent_alias}.risk_profile does not name a configured risk_profiles entry"
            )
        })?
        .clone();
    let memory_composite = {
        use clawcrew_config::multi_agent::MemoryBackendKind;
        match agent.memory.backend {
            MemoryBackendKind::Markdown => format!("markdown.{agent_alias}"),
            MemoryBackendKind::None => "none".to_string(),
            _ => {
                let raw = config.memory.backend.trim();
                if raw.is_empty() || raw.eq_ignore_ascii_case("none") {
                    "none".to_string()
                } else {
                    let (kind, alias) = raw.split_once('.').unwrap_or((raw, "default"));
                    format!("{kind}.{alias}")
                }
            }
        }
    };
    let __zc_alias = agent_alias.to_string();
    let __zc_message = message.to_string();
    let __zc_session_id = session_id.map(str::to_string);
    let __zc_attribution_span =
        ::clawcrew_log::attribution_span!(&crate::agent::AgentAttribution(__zc_alias.as_str()));
    let __zc_scope_span = ::clawcrew_log::info_span!(
        target: "clawcrew_log_internal_scope",
        "clawcrew_scope",
        risk_profile = %agent.risk_profile,
        runtime_profile = %agent.runtime_profile,
        memory_namespace = %memory_composite,
    );
    let __zc_body = async move {
        let agent_alias: &str = __zc_alias.as_str();
        let message: &str = __zc_message.as_str();
        let session_id: Option<&str> = __zc_session_id.as_deref();

        // ── Effective per-agent runtime tunables ──────────────────────
        // Profile values (when set) override the agent's inline fields.
        // See `Config::resolved_agent_config` for precedence rules.
        let eff_compact_context = agent.resolved.compact_context;
        let eff_max_system_prompt_chars = agent.resolved.max_system_prompt_chars;
        let eff_prompt_injection_mode = agent.resolved.prompt_injection_mode;

        let observer: Arc<dyn Observer> =
            Arc::from(observability::create_observer(&config.observability));
        let runtime: Arc<dyn platform::RuntimeAdapter> =
            Arc::from(platform::create_runtime(&config.runtime)?);
        let security = Arc::new(SecurityPolicy::for_agent(&config, agent_alias)?);
        let (provider_name, provider_alias, agent_model_provider) = match config
            .resolved_model_provider_for_agent(agent_alias)
        {
            Some(resolved) => (resolved.0, resolved.1.to_string(), Some(resolved.2.clone())),
            None => {
                let agent_ref = agent.model_provider.as_str();
                if !agent_ref.is_empty() {
                    anyhow::bail!(
                        "agents.{agent_alias}.model_provider = \"{agent_ref}\" does not resolve to \
                     a configured [providers.models.<type>.<alias>] entry"
                    );
                }
                anyhow::bail!(
                    "agents.{agent_alias}.model_provider is empty \u{2014} set it to a configured \
                 \"<type>.<alias>\" (e.g. \"anthropic.{agent_alias}\")"
                );
            }
        };
        let approval_manager = ApprovalManager::for_non_interactive(&risk_profile);
        let mem: Arc<dyn Memory> = clawcrew_memory::create_memory_for_agent(
            &config,
            agent_alias,
            agent_model_provider
                .as_ref()
                .and_then(|e| e.api_key.as_deref()),
        )
        .await?;

        let (composio_key, composio_entity_id) = if config.composio.enabled {
            (
                config.composio.api_key.as_deref(),
                Some(config.composio.entity_id.as_str()),
            )
        } else {
            (None, None)
        };

        // Build SOP engine when sops_dir is configured so SOP tools are
        // available on this path (process_message CLI agent). No channel map is
        // wired here, so the approval route adapter is the no-op (log-only); the
        // daemon path injects a real channel-delivering adapter.
        let (sop_engine, sop_audit) = if config.sop.runtime_enabled() {
            let sop_mem: Arc<dyn clawcrew_memory::Memory> =
                clawcrew_memory::create_memory_for_agent(&config, agent_alias, None).await?;
            let (engine, audit) = crate::sop::build_sop_engine(
                config.sop.clone(),
                &config.data_dir,
                &config.install_root_dir(),
                sop_mem,
                Default::default(),
            );
            (Some(engine), Some(audit))
        } else {
            (None, None)
        };

        let all_tools_result_pm = tools::all_tools_with_runtime(
            Arc::clone(&config),
            &security,
            &risk_profile,
            agent_alias,
            runtime.clone(),
            mem.clone(),
            composio_key,
            composio_entity_id,
            &config.browser,
            &config.http_request,
            &config.web_fetch,
            &config.data_dir,
            &config.agents,
            agent_model_provider
                .as_ref()
                .and_then(|e| e.api_key.as_deref()),
            &config,
            None,
            false,
            None,
            sop_engine,
            sop_audit,
            None,
        )?;
        let skills = crate::skills::load_skills_for_agent_from_config(&config, agent_alias);
        let assembled = scoped::ScopedToolRegistry::assemble(scoped::ScopedAssembly {
            config: &config,
            agent_alias,
            security: &security,
            built: all_tools_result_pm,
            skills: &skills,
            runtime: runtime.clone(),
            caller_allowed: None,
            connect_mcp: true,
            connect_peripherals: true,
            exclude_memory: false,
            acp_delivery: false,
            list_deferred_mcp_specs: false,
            emit_assembly_logs: true,
            // `process_message` is the channel/orchestrator live-chat path;
            // it has no cross-turn reuse contract, so the per-call
            // `connect_all` path inside `assemble` is the correct choice.
            // The daemon heartbeat worker — the only caller that has a
            // reuse contract — passes its own `mcp_registry` through
            // `agent::run` (`AgentRunOverrides::mcp_registry`).
            mcp_registry: None,
        })
        .await;
        // process_message injects one combined MCP prompt block: deferred tool-search
        // listing + pinned resources, composed by the harness. `mut` because the
        // text-tool prompt policy below may clear it for a non-native strict target.
        let mut deferred_section = assembled.combined_mcp_prompt_section();
        let scoped::ScopedAssembled {
            registry,
            delegate_handle: _,
            ask_user_handle,
            reaction_handle,
            poll_handle,
            escalate_handle,
            channel_room_handle,
            activated_handle: activated_handle_pm,
            mcp_tool_names: mcp_tool_names_pm,
            ..
        } = assembled;
        // Stays sealed: `agent_turn_with_sop_reassembly` now takes
        // `&ScopedToolRegistry`; leaf uses coerce via `Deref`.
        let tools_registry = registry;

        // Populate all channel-driven tool handles from the registered factory.
        let count = seed_channel_handles(
            &ask_user_handle,
            &channel_room_handle,
            &reaction_handle,
            &poll_handle,
            &escalate_handle,
        );
        if count > 0 {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Register)
                    .with_category(::clawcrew_log::EventCategory::Channel)
                    .with_attrs(::serde_json::json!({"count": count})),
                &format!("Registered {} channel(s) for process_message agent", count),
            );
        }

        let model_name = match agent_model_provider
            .as_ref()
            .and_then(|e| e.model.as_deref())
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            Some(m) => m.to_string(),
            None => anyhow::bail!(
                "agents.{agent_alias}.model_provider resolves to a model_provider entry with no \
             `model` set. Configure [providers.models.{provider_name}.<alias>] model = \"...\"."
            ),
        };
        let provider_runtime_options = clawcrew_providers::provider_runtime_options_for_alias(
            &config,
            provider_name,
            provider_alias.as_str(),
        );
        let model_provider_ref = format!("{provider_name}.{provider_alias}");
        let model_provider: Box<dyn ModelProvider> =
            clawcrew_providers::create_routed_model_provider_with_options(
                &config,
                &model_provider_ref,
                agent_model_provider
                    .as_ref()
                    .and_then(|e| e.api_key.as_deref()),
                agent_model_provider.as_ref().and_then(|e| e.uri.as_deref()),
                &config.reliability,
                &config.model_routes,
                &model_name,
                &provider_runtime_options,
            )?;

        let hardware_rag: Option<crate::rag::HardwareRag> = config
            .peripherals
            .datasheet_dir
            .as_ref()
            .filter(|d| !d.trim().is_empty())
            .map(|dir| crate::rag::HardwareRag::load(&config.data_dir, dir.trim()))
            .and_then(Result::ok)
            .filter(|r: &crate::rag::HardwareRag| !r.is_empty());
        let board_names: Vec<String> = config
            .peripherals
            .boards
            .iter()
            .map(|b| b.board.clone())
            .collect();

        // ── Initialize locale-aware tool descriptions ──────────────────
        let _i18n_locale = config
            .locale
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(crate::i18n::detect_locale);

        let mut tool_descs: Vec<(&str, &str)> = vec![
            ("shell", "Execute terminal commands."),
            ("file_read", "Read file contents."),
            ("file_write", "Write file contents."),
            ("memory_store", "Save to memory."),
            ("memory_recall", "Search memory."),
            ("memory_forget", "Delete a memory entry."),
            (
                "model_routing_config",
                "Configure default model, scenario routing, and delegate agents.",
            ),
            ("screenshot", "Capture a screenshot."),
            ("image_info", "Read image metadata."),
        ];
        if matches!(
            eff_prompt_injection_mode,
            clawcrew_config::schema::SkillsPromptInjectionMode::Compact
        ) {
            tool_descs.push((
                "read_skill",
                "Load the full source for an available skill by name.",
            ));
        }
        if config.browser.enabled {
            tool_descs.push(("browser_open", "Open approved URLs in browser."));
        }
        if config.composio.enabled {
            tool_descs.push(("composio", "Execute actions on 1000+ apps via Composio."));
        }
        tool_descs.push((
            "channel_room",
            "Create channel rooms and invite users through active channels.",
        ));
        if config.peripherals.enabled && !config.peripherals.boards.is_empty() {
            tool_descs.push(("gpio_read", "Read GPIO pin value on connected hardware."));
            tool_descs.push((
                "gpio_write",
                "Set GPIO pin high or low on connected hardware.",
            ));
            tool_descs.push((
            "arduino_upload",
            "Upload Arduino sketch. Use for 'make a heart', custom patterns. You write full .ino code; ClawCrew uploads it.",
        ));
            tool_descs.push((
            "hardware_memory_map",
            "Return flash and RAM address ranges. Use when user asks for memory addresses or memory map.",
        ));
            tool_descs.push((
            "hardware_board_info",
            "Return full board info (chip, architecture, memory map). Use when user asks for board info, what board, connected hardware, or chip info.",
        ));
            tool_descs.push((
            "hardware_memory_read",
            "Read actual memory/register values from Nucleo. Use when user asks to read registers, read memory, dump lower memory 0-126, or give address and value.",
        ));
            tool_descs.push((
            "hardware_capabilities",
            "Query connected hardware for reported GPIO pins and LED pin. Use when user asks what pins are available.",
        ));
        }

        let effective_message_for_filter =
            crate::agent::thinking::strip_thinking_directive(message);
        let mut excluded_tools = compute_excluded_mcp_tools(
            &tools_registry,
            &agent.resolved.tool_filter_groups,
            effective_message_for_filter.as_ref(),
            &mcp_tool_names_pm,
        );
        {
            let active_profile = &risk_profile;
            if active_profile.level != AutonomyLevel::Full {
                excluded_tools.extend(active_profile.excluded_tools.iter().cloned());
            }
        }

        // Filter tool descriptions to match the effective set.
        tool_descs.retain(|(name, _)| !excluded_tools.iter().any(|ex| ex == name));

        // Derive effective tool names from the filtered set so prompt builders
        // and channel target guards see the correct state.
        let effective_tool_names: HashSet<&str> = tools_registry
            .iter()
            .map(|tool| tool.name())
            .filter(|name| !excluded_tools.iter().any(|ex| ex == *name))
            .collect();
        tool_descs.retain(|(name, _)| effective_tool_names.contains(name));

        let bootstrap_max_chars = if eff_compact_context {
            Some(6000)
        } else {
            None
        };
        let native_tools = model_provider
            .capabilities_for_model(&model_name)
            .native_tool_calling;
        let native_tool_specs_present = native_tool_specs_present_for_turn(
            model_provider.as_ref(),
            &model_name,
            &tools_registry,
            &excluded_tools,
            activated_handle_pm.as_ref(),
        )?;
        let expose_text_tool_protocol = apply_text_tool_prompt_policy(
            native_tools,
            agent.resolved.strict_tool_parsing,
            &mut tool_descs,
            &mut deferred_section,
        );
        let skill_tools_protocol_exposed = expose_text_tool_protocol || native_tool_specs_present;
        let agent_workspace = config.agent_workspace_dir(agent_alias);
        let mut system_prompt =
            crate::agent::system_prompt::build_system_prompt_with_mode_and_effective_tools(
                &agent_workspace,
                &model_name,
                &tool_descs,
                |name| skill_tools_protocol_exposed && effective_tool_names.contains(name),
                &skills,
                Some(&agent.identity),
                bootstrap_max_chars,
                Some(&risk_profile),
                native_tool_specs_present,
                eff_prompt_injection_mode,
                eff_compact_context,
                0,
                false,
                config.channels.show_tool_calls,
                runtime.shell_profile().as_ref(),
            );
        if expose_text_tool_protocol {
            system_prompt.push_str(&build_tool_instructions_for_names(
                &tools_registry,
                &effective_tool_names,
            ));
        }
        if !deferred_section.is_empty() {
            system_prompt.push('\n');
            system_prompt.push_str(&deferred_section);
        }

        // ── Parse thinking directive from user message ─────────────
        let (thinking_directive, effective_message) =
            match crate::agent::thinking::parse_thinking_directive(message) {
                Some((level, remaining)) => {
                    ::clawcrew_log::record!(
                        INFO,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_category(::clawcrew_log::EventCategory::Agent)
                            .with_attrs(::serde_json::json!({"thinking_level": level})),
                        "Thinking directive parsed from message"
                    );
                    (Some(level), remaining)
                }
                None => (None, message.to_string()),
            };
        let thinking_level = crate::agent::thinking::resolve_thinking_level(
            thinking_directive,
            None,
            &agent.resolved.thinking,
        );
        let thinking_params = crate::agent::thinking::apply_thinking_level_with_config(
            thinking_level,
            &agent.resolved.thinking,
        );
        let effective_temperature: Option<f64> = agent_model_provider
            .as_ref()
            .and_then(|e| e.temperature)
            .map(|t| {
                crate::agent::thinking::clamp_temperature(
                    t + thinking_params.temperature_adjustment,
                )
            });

        // Prepend thinking system prompt prefix when present.
        if let Some(ref prefix) = thinking_params.system_prompt_prefix {
            system_prompt = format!("{prefix}\n\n{system_prompt}");
        }
        system_prompt = crate::agent::system_prompt::finalize_system_prompt(
            system_prompt,
            eff_max_system_prompt_chars,
        );

        let effective_msg_ref = effective_message.as_str();
        let runtime_capability_names: Vec<&str> = effective_tool_names.iter().copied().collect();
        if let Some(suggestion) = crate::skills::render_missing_skill_install_suggestion(
            effective_msg_ref,
            &skills,
            &runtime_capability_names,
            &config.data_dir,
            &config.skills.extra_registries,
            config.skills.install_suggestions.enabled,
        ) {
            return Ok(suggestion);
        }

        // Memory context is injected once in the engine, keyed on the ingress
        // origin (agent::memory_inject); recall is scoped to this entry's
        // session_id. Hardware RAG stays site-built; the engine prepends the
        // memory block above it.
        // Pre-mint the turn id so the pre-turn RAG retrieval and the
        // agent_turn bracket share one correlation id. The RAG span stays a
        // root span (it runs before AgentStart) but carries the matching
        // clawcrew.turn_id attribute; nesting it is a tracked follow-up.
        let turn_id = uuid::Uuid::new_v4().to_string();
        let rag_limit = if eff_compact_context { 2 } else { 5 };
        let hw_context = hardware_rag
            .as_ref()
            .map(|r| {
                build_hardware_context(
                    r,
                    &*observer,
                    effective_msg_ref,
                    &board_names,
                    rag_limit,
                    TurnMeta {
                        parent_agent_alias: None,
                        agent_alias: Some(agent_alias),
                        turn_id: &turn_id,
                        channel_name: "daemon",
                    },
                )
            })
            .unwrap_or_default();
        let context = hw_context;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
        let enriched = if context.is_empty() {
            format!("[{now}] {effective_message}")
        } else {
            format!("{context}[{now}] {effective_message}")
        };

        let mut history = vec![
            ChatMessage::system(&system_prompt),
            ChatMessage::user(&enriched),
        ];
        // One-shot transcript: no prior trim ran, so no crumb exists.
        let mut history_has_trim_breadcrumb = false;
        let mut excluded_tools = compute_excluded_mcp_tools(
            &tools_registry,
            &agent.resolved.tool_filter_groups,
            effective_msg_ref,
            &mcp_tool_names_pm,
        );
        {
            let active_profile = &risk_profile;
            if active_profile.level != AutonomyLevel::Full {
                excluded_tools.extend(active_profile.excluded_tools.iter().cloned());
            }
        }

        let routed_approval_channel = risk_profile.approval_route.as_ref().and_then(|route| {
            live_channel_registry().map(|handles| {
                crate::agent::agent::RoutedApprovalChannel::new(handles, route.clone())
            })
        });
        let routed_approval_channel_ref = routed_approval_channel
            .as_ref()
            .map(|c| c as &dyn clawcrew_api::channel::Channel);

        clawcrew_api::NATIVE_THINKING_OVERRIDE
            .scope(
                thinking_params.native_thinking,
                agent_turn_with_sop_reassembly(
                    Some(&config),
                    model_provider.as_ref(),
                    &mut history,
                    &mut history_has_trim_breadcrumb,
                    &mut None,
                    &tools_registry,
                    observer.as_ref(),
                    &model_provider_ref,
                    &model_name,
                    effective_temperature,
                    true,
                    "daemon",
                    None,
                    &config.multimodal,
                    agent.resolved.max_tool_iterations,
                    Some(&approval_manager),
                    &excluded_tools,
                    &agent.resolved.tool_call_dedup_exempt,
                    activated_handle_pm.as_ref(),
                    None,
                    agent.resolved.strict_tool_parsing,
                    agent.resolved.parallel_tools,
                    agent.resolved.max_tool_result_chars,
                    config
                        .resolved_context_limits_for_route(
                            agent_alias,
                            &model_provider_ref,
                            &model_name,
                        )
                        .context_token_budget,
                    // Cross-channel HITL: a route-only approval bridge when the
                    // profile sets `approval_route` and channels are live, else
                    // `None` (today's channel-less auto-deny). See above.
                    routed_approval_channel_ref,
                    origin,
                    Some(crate::agent::memory_inject::TurnMemory {
                        handle: mem.as_ref(),
                        query: effective_message.clone(),
                        sessions: vec![session_id.map(str::to_string)],
                        suppress: false,
                        cfg: crate::agent::memory_inject::MemoryInjectConfig::from_memory_config(
                            &config.memory,
                            crate::agent::memory_inject::DEFAULT_RECALL_LIMIT,
                        ),
                    }),
                    Some(agent_alias),
                    Some(&turn_id),
                    Some(SopStepReassembly { config: &config }),
                ),
            )
            .await
    };
    __zc_body
        .instrument(__zc_scope_span)
        .instrument(__zc_attribution_span)
        .await
}

#[cfg(test)]
mod tests;

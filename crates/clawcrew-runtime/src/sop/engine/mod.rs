use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};

use super::capability::SopCapabilityRegistry;
use super::load_sops;
use super::metrics::SopMetricsCollector;
use super::route::{self, NextStep, RouteCtx};
use super::rundata::RunData;
use super::schema;
use super::store::{
    ClaimToken, InMemoryRunStore, PersistedRun, ProposalRecord, ProposalStatus, RetentionPolicy,
    SopEventRecord, SopRunStore, StoreError,
};
use super::types::{
    DeterministicRunState, DeterministicSavings, FilesystemEventKind, Sop, SopAdmission,
    SopAdmissionPolicy, SopEvent, SopExecutionMode, SopPriority, SopRun, SopRunAction,
    SopRunStatus, SopRunSummary, SopStep, SopStepKind, SopStepResult, SopStepStatus, SopTrigger,
    SopTriggerSource,
};
use crate::calendar::{CALENDAR_NO_SHOW_TOPIC, CalendarNoShowEvent};
use crate::security::{ContentSafety, new_marker_id};
use serde_json::Value;
use clawcrew_config::schema::SopConfig;

/// Central SOP orchestrator: loads SOPs, matches triggers, manages run lifecycle.
pub struct SopEngine {
    sops: Vec<Sop>,
    active_runs: HashMap<String, SopRun>,
    /// Completed/failed/cancelled runs (kept for status queries).
    finished_runs: Vec<SopRun>,
    config: SopConfig,
    run_counter: u64,
    /// Cumulative savings from deterministic execution.
    deterministic_savings: DeterministicSavings,
    /// Durable run-state store. Defaults to an ephemeral in-memory store
    /// (current behavior); `build_sop_engine` injects the configured backend.
    store: Arc<dyn SopRunStore>,
    /// Run-execution metrics collector. Per-engine fresh in `new()` (test
    /// isolation); `build_sop_engine` swaps in the process-shared collector.
    metrics: Arc<SopMetricsCollector>,
    /// Optional live run-change notifier. When present, every run mutation
    /// (admission, step advance, terminal finish) publishes the run's fresh
    /// summary so push surfaces (the Runs WebSocket) can forward it without
    /// polling. `None` in tests and any embedder that does not want a feed.
    run_notifier: Option<tokio::sync::broadcast::Sender<SopRunSummary>>,
    /// Deterministic capability registry for `kind = "capability"` SOP steps.
    capabilities: Arc<SopCapabilityRegistry>,
    /// Run IDs parked (`WaitingApproval`/`PausedCheckpoint`) whose exec claim was
    /// deliberately KEPT because the parked snapshot could not be durably
    /// persisted (`persist_parked_snapshot_then_release_claim`'s fail-closed
    /// branch). `retry_pending_park_persists` retries these each maintenance
    /// tick, which renews the kept claim's lease as a side effect even while the
    /// retry keeps failing, so the reaper's expired-claim sweep never reclaims a
    /// claim standing in for a park that still is not durable. Cleared (and the
    /// claim released) once a later retry persists successfully.
    claims_pending_persist: std::collections::HashSet<String>,
    /// Approval broker (EPIC G): membership + quorum authorization wrapping the
    /// `resolve_gate` chokepoint. Defaults to a pass-through (no policies) so
    /// behavior is unchanged until a `[sop.approval]` policy is configured.
    approval_broker: Arc<super::approval::ApprovalBroker>,
    /// A2: per-message dispatch idempotency for at-least-once transports. Maps a
    /// redelivery-stable `(sop_name, delivery key)` to the run that already started for
    /// it, so an AMQP broker redelivery of the same message (e.g. after a partial
    /// multi-SOP dispatch requeued the whole delivery) coalesces instead of starting a
    /// second run. Bounded FIFO (`DISPATCH_DEDUP_CAP`); the window need only outlast a
    /// broker redelivery, not persist forever, so it is in-memory like `finished_runs`.
    ///
    /// CONTRACT (best-effort): the delivery key derives from the AMQP `message-id`, so
    /// this is exactly-once ONLY when publishers set a UNIQUE `message-id` per logical
    /// message (the AMQP-recommended practice). That is the sole cross-redelivery-stable
    /// identity the broker exposes: `redelivered` is set for ANY requeue and the delivery
    /// tag changes across a redelivery, so neither can prove two deliveries are the same
    /// message. Under `message-id` REUSE (a publisher contract violation), a redelivery of
    /// a reused id can coalesce a genuinely distinct trigger into the wrong run and ACK it
    /// away: at-most-once, a dropped trigger. This is an accepted, documented limitation of
    /// keying on a publisher-controlled id; the safe direction elsewhere is always a
    /// duplicate run, never a silent drop, and a delivery with no `message-id` is never
    /// deduplicated. A requeue-free design (ACK every delivery, retry deferred SOPs
    /// in-process) would remove the redelivery and thus this dependency entirely - tracked
    /// as a follow-up, out of scope for the dedup window here.
    dispatch_dedup: std::collections::VecDeque<(String, String)>,
    /// Run IDs parked at a checkpoint whose denial tried to take the terminal
    /// path, but the terminal write failed after the run's exec claim was
    /// reacquired. The parked snapshot is already durable, so this set only
    /// renews the retained claim during maintenance; it must not release the
    /// claim until the operator retries to a durable outcome.
    claims_retained_after_terminal_rollback: std::collections::HashSet<String>,
    /// Process-local proof that the driver reached a supported cancellation
    /// boundary. The durable `CancelRequested` status remains the lifecycle
    /// source of truth; this set only distinguishes a still-running step from
    /// one whose driver exited at a safe boundary, so maintenance may retry a
    /// failed terminal write without cancelling work mid-step.
    cancellation_finalization_ready: std::collections::HashSet<String>,
    /// Process-local proof that a headless driver exhausted its bounded action
    /// budget and exited. The durable run remains `Running` with its claim held
    /// until maintenance can persist the terminal `Failed` transition. This set
    /// creates the safe-boundary fact; it does not duplicate durable run state.
    step_budget_finalization_ready: std::collections::HashSet<String>,
}

/// Cap on the in-memory per-message dispatch-dedup window (`SopEngine::dispatch_dedup`).
const DISPATCH_DEDUP_CAP: usize = 512;
/// Terminal writes retried while the driver is already at a safe cancellation boundary.
const CANCELLATION_FINALIZE_ATTEMPTS: usize = 3;

/// Composite dedup key: `sop_name` and the transport delivery key joined by a NUL, which
/// cannot appear in a SOP name, so distinct pairs never collide.
fn dispatch_dedup_composite(sop_name: &str, dedup_key: &str) -> String {
    format!("{sop_name}\u{0}{dedup_key}")
}

/// Outcome of one [`SopEngine::run_maintenance_tick`] pass (EPIC A1), for
/// observability. All counts are 0 on a quiet tick.
#[derive(Debug, Default, Clone)]
pub struct MaintenanceSummary {
    /// Approval gates that hit their timeout this pass.
    pub timed_out: usize,
    /// Expired concurrency-claim leases reclaimed.
    pub reaped_claims: usize,
    /// Terminal runs pruned past the retention policy.
    pub pruned_runs: usize,
    /// Requested cancellations terminalized after an earlier boundary write
    /// failed and the store recovered.
    pub finalized_cancellations: usize,
    /// Step-budget failures terminalized after an earlier store failure.
    pub finalized_step_budget_failures: usize,
    /// Timeout actions produced. Mostly self-applied (`Escalate` re-stamps,
    /// `Cancel` finalizes); an opt-in `AutoApprove` yields a resumed `ExecuteStep`
    /// the caller logs until EPIC A2's live executor exists.
    pub timeout_actions: Vec<SopRunAction>,
}

impl MaintenanceSummary {
    /// True when the pass did nothing (no timeouts, reaps, or prunes).
    pub fn is_empty(&self) -> bool {
        self.timed_out == 0
            && self.reaped_claims == 0
            && self.pruned_runs == 0
            && self.finalized_cancellations == 0
            && self.finalized_step_budget_failures == 0
    }
}

#[derive(Debug)]
struct TerminalPersistenceRetained {
    run_id: String,
    source: StoreError,
}

impl std::fmt::Display for TerminalPersistenceRetained {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "terminal persistence failed for run {}; active run and admission claim remain retained: {}",
            self.run_id, self.source
        )
    }
}

impl std::error::Error for TerminalPersistenceRetained {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// True when `err` is the typed `TerminalPersistenceRetained` marker (a
/// terminal write failed and the run stayed active with its claim intact), as
/// opposed to any other fault. Lets a caller in another module or crate (e.g.
/// the gateway cancel endpoint) render it as retryable backpressure rather
/// than reporting the run as cancelled, without depending on the private
/// struct.
pub fn err_is_terminal_persistence_retained(err: &anyhow::Error) -> bool {
    err.is::<TerminalPersistenceRetained>()
}

#[derive(Debug)]
struct CancellationRequestPersistenceRetained {
    run_id: String,
    source: anyhow::Error,
}

impl std::fmt::Display for CancellationRequestPersistenceRetained {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cancellation request for run {} was not durably persisted; the run remains active: {}",
            self.run_id, self.source
        )
    }
}

impl std::error::Error for CancellationRequestPersistenceRetained {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// True when cancellation could not be durably persisted and the run remains
/// active with its admission claim intact. Covers both the initial
/// `CancelRequested` snapshot and the terminal `Cancelled` transition.
pub fn err_is_cancellation_persistence_retained(err: &anyhow::Error) -> bool {
    err.is::<TerminalPersistenceRetained>() || err.is::<CancellationRequestPersistenceRetained>()
}

/// Typed marker: a resume could not re-acquire an exec slot because the SOP's
/// per-SOP `max_concurrent` or the global `max_concurrent_total` is already
/// saturated. This is routine BACKPRESSURE, not a fault - kept distinct from a
/// store error so callers surface it as "at capacity, retry" (leaving the run
/// parked and re-resolvable) instead of logging it as a failure. It is the
/// signal that enforces the documented concurrency caps on the resume path: a
/// resume that would exceed them is refused rather than oversubscribed.
#[derive(Debug)]
struct ResumeAtCapacity {
    run_id: String,
    sop_name: String,
}

impl std::fmt::Display for ResumeAtCapacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "run {} ({}) cannot resume yet: execution slots are full; it stays parked and re-resolvable once a slot frees",
            self.run_id, self.sop_name
        )
    }
}

impl std::error::Error for ResumeAtCapacity {}

/// True when `err` is the typed `ResumeAtCapacity` backpressure marker (an
/// over-cap resume was refused), as opposed to a store fault. Lets a caller in
/// another module or crate (e.g. `resolve_gate`, or the gateway resume endpoint)
/// render it as backpressure (HTTP 503) rather than a fault without depending on
/// the private struct.
pub fn err_is_resume_at_capacity(err: &anyhow::Error) -> bool {
    err.is::<ResumeAtCapacity>()
}

enum ActivePersistOutcome {
    Saved,
    CapacityFull,
    Failed,
}

enum ParkPersistOutcome {
    Released,
    CapacityFull,
    PersistFailed,
}

enum GateClearTransition {
    Active {
        // Boxed: `SopRunAction` is large; keeping it inline makes this the
        // dominant variant (clippy::large_enum_variant).
        action: Box<SopRunAction>,
        follow_up: Option<GateResolutionFollowUp>,
    },
    Terminal {
        status: SopRunStatus,
        reason: Option<String>,
        follow_up: Option<GateResolutionFollowUp>,
    },
}

enum GateResolutionFollowUp {
    StepSchemaReject {
        step: u32,
        phase: &'static str,
        reason: String,
    },
    StepSkipped {
        sop_name: String,
        step: u32,
        reason: String,
    },
}

/// A held execution-slot reservation from phase 1 of a start (`reserve_run_slot`),
/// awaiting phase 2 (`activate_reserved_run`) or release (`release_reservation`).
/// Carries the CAS claim that keeps the slot held so the AMQP multi-match batch path
/// can reserve every matched SOP before activating any of them.
pub(crate) struct StartReservation {
    run_id: String,
    claim: ClaimToken,
    sop: Sop,
    deterministic: bool,
}

impl StartReservation {
    /// The SOP this reservation holds a slot for.
    pub(crate) fn sop_name(&self) -> &str {
        &self.sop.name
    }
}

/// What an idempotent operator cancellation did. Returned by
/// `cancel_run_idempotent` so a caller (the gateway cancel endpoint) can
/// report success on both a fresh cancellation and a repeat request without
/// re-deriving the run's active/terminal state itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// The run was parked with no work in flight and is now durably
    /// `Cancelled`.
    Cancelled,
    /// Cancellation was durably requested while a step was in flight. The run
    /// remains active and claimed until the driver reaches the next boundary.
    Requested,
    /// A prior request already left the run in `CancelRequested`.
    AlreadyRequested,
    /// The run had already reached a terminal state - previously cancelled,
    /// or it raced to normal completion/failure first. Reported as-is; no
    /// second cancellation or audit event was recorded.
    AlreadyTerminal(SopRunStatus),
}

impl SopEngine {
    /// Create a new engine with the given config. Call `reload()` to load SOPs.
    pub fn new(config: SopConfig) -> Self {
        Self {
            sops: Vec::new(),
            active_runs: HashMap::new(),
            finished_runs: Vec::new(),
            config,
            run_counter: 0,
            deterministic_savings: DeterministicSavings::default(),
            store: Arc::new(InMemoryRunStore::new()),
            metrics: Arc::new(SopMetricsCollector::new()),
            run_notifier: None,
            capabilities: Arc::new(SopCapabilityRegistry::with_builtins()),
            claims_pending_persist: std::collections::HashSet::new(),
            approval_broker: Arc::new(super::approval::ApprovalBroker::disabled()),
            dispatch_dedup: std::collections::VecDeque::new(),
            claims_retained_after_terminal_rollback: std::collections::HashSet::new(),
            cancellation_finalization_ready: std::collections::HashSet::new(),
            step_budget_finalization_ready: std::collections::HashSet::new(),
        }
    }

    /// Inject a durable run-state store (used by `build_sop_engine`). Default is
    /// an ephemeral in-memory store, so callers that don't set one keep today's
    /// behavior exactly.
    pub fn with_store(mut self, store: Arc<dyn SopRunStore>) -> Self {
        self.store = store;
        self
    }

    /// Inject the metrics collector. `build_sop_engine` passes the process-shared
    /// collector so the engine's completion metrics and the SOP tools' reports
    /// observe one set; the default per-engine collector keeps tests isolated.
    pub fn with_metrics(mut self, metrics: Arc<SopMetricsCollector>) -> Self {
        self.metrics = metrics;
        self
    }

    /// Attach a live run-change notifier. `build_sop_engine` wires the gateway's
    /// sender here so run transitions push to the Runs WebSocket. Returns the
    /// engine unchanged when never called (tests, headless embedders).
    pub fn with_run_notifier(mut self, tx: tokio::sync::broadcast::Sender<SopRunSummary>) -> Self {
        self.run_notifier = Some(tx);
        self
    }

    /// Subscribe to the live run-change feed if a notifier is attached. Each
    /// item is a fresh [`SopRunSummary`] for the run that just transitioned.
    pub fn subscribe_run_changes(&self) -> Option<tokio::sync::broadcast::Receiver<SopRunSummary>> {
        self.run_notifier.as_ref().map(|tx| tx.subscribe())
    }

    /// Publish a run's current summary on the notifier, if attached. A send
    /// error means no live subscribers; that is not a failure, so it is
    /// dropped. Marked `active` per the caller's chokepoint.
    fn notify_run(&self, run: &SopRun, active: bool) {
        if let Some(tx) = self.run_notifier.as_ref() {
            let _ = tx.send(SopRunSummary::from_run(run, active));
        }
    }

    /// Inject a deterministic capability registry. Tests and future daemon
    /// wiring can replace the built-ins without adding another execution path.
    pub fn with_capabilities(mut self, capabilities: Arc<SopCapabilityRegistry>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Inject the approval broker (built from `[sop.approval]` config). Defaults to
    /// a pass-through; `build_sop_engine` replaces it with the configured broker.
    pub fn with_approval_broker(mut self, broker: Arc<super::approval::ApprovalBroker>) -> Self {
        self.approval_broker = broker;
        self
    }

    /// The approval broker (membership + quorum authorization). Callers that must
    /// deliver an escalation to a policy's second route read it here.
    pub fn approval_broker(&self) -> Arc<super::approval::ApprovalBroker> {
        Arc::clone(&self.approval_broker)
    }

    /// Resolve a gate or deterministic checkpoint THROUGH the broker (membership +
    /// quorum), then its single transition owner.
    /// This is the entry point out-of-band surfaces (gateway / CLI / tools) should
    /// call so a `[sop.approval]` policy is enforced; with no policy it is exactly
    /// `resolve_gate` for a `WaitingApproval` run or the historical checkpoint
    /// resolver for a `PausedCheckpoint` run. The broker is cloned out first so it
    /// does not borrow `self` while `self` is mutated by the chokepoint.
    ///
    /// This shared entry point does **not** consume a resumed deterministic
    /// capability tail inline: the caller receives the resumed action and hands it
    /// to the shared async executor, which reacquires the engine lock once per
    /// capability. That is what lets an operator cancellation persist
    /// `CancelRequested` *between* post-checkpoint capabilities instead of blocking
    /// on the engine mutex until the whole tail has already run.
    ///
    /// A caller that genuinely owns an exclusive engine handle and must observe the
    /// fully-driven tail before returning wants [`Self::resolve_via_broker_inline`]
    /// instead — but note that no shared transport qualifies.
    pub fn resolve_via_broker(
        &mut self,
        run_id: &str,
        decision: super::approval::ApprovalDecision,
        principal: super::approval::ApprovalPrincipal,
    ) -> Result<super::approval::BrokerOutcome> {
        self.resolve_via_broker_inner(run_id, decision, principal, false)
    }

    /// Resolve through the broker and synchronously drive the resumed
    /// deterministic capability tail while the engine mutex is still held.
    ///
    /// ⚠️ Cancellation cannot interleave with a tail driven this way. Reserved for
    /// genuinely exclusive callers (single-owner engine handles and tests that
    /// assert on the fully-driven tail). Shared transports — admin HTTP,
    /// WebSocket, RPC, channels, and the `sop_approve` tool — must use
    /// [`Self::resolve_via_broker`], which defers the tail.
    pub fn resolve_via_broker_inline(
        &mut self,
        run_id: &str,
        decision: super::approval::ApprovalDecision,
        principal: super::approval::ApprovalPrincipal,
    ) -> Result<super::approval::BrokerOutcome> {
        self.resolve_via_broker_inner(run_id, decision, principal, true)
    }

    /// Explicit alias for [`Self::resolve_via_broker`], kept for call sites that
    /// want the deferral to be obvious at the point of use.
    pub fn resolve_via_broker_deferred(
        &mut self,
        run_id: &str,
        decision: super::approval::ApprovalDecision,
        principal: super::approval::ApprovalPrincipal,
    ) -> Result<super::approval::BrokerOutcome> {
        self.resolve_via_broker_inner(run_id, decision, principal, false)
    }

    fn resolve_via_broker_inner(
        &mut self,
        run_id: &str,
        decision: super::approval::ApprovalDecision,
        principal: super::approval::ApprovalPrincipal,
        drive_inline_capability_tail: bool,
    ) -> Result<super::approval::BrokerOutcome> {
        let broker = Arc::clone(&self.approval_broker);
        if let Some(step) = self.active_runs.get(run_id).and_then(|run| {
            (run.status == SopRunStatus::PausedCheckpoint).then_some(run.current_step)
        }) {
            if let Some(outcome) =
                broker.authorize_checkpoint(self, run_id, step, &decision, &principal)?
            {
                return Ok(outcome);
            }
            if let super::approval::ApprovalDecision::Revise { guidance } = &decision {
                self.revise_checkpoint_with_principal(
                    run_id,
                    guidance,
                    decision.clone(),
                    principal,
                )?;
                return Ok(super::approval::BrokerOutcome::Resolved(
                    super::approval::ResolveOutcome::Revised,
                ));
            }
            let action = self.decide_checkpoint_with_principal(
                run_id,
                decision,
                principal,
                drive_inline_capability_tail,
            )?;
            return Ok(super::approval::BrokerOutcome::Resolved(
                super::approval::ResolveOutcome::Resumed(Box::new(action)),
            ));
        }
        broker.resolve(self, run_id, decision, principal)
    }
    /// Reconstruct in-flight runs from the store at startup (durable backends).
    /// No-op for the in-memory default. Does not overwrite already-present runs.
    pub fn restore_runs(&mut self) {
        match self.store.load_active_runs() {
            Ok(runs) => {
                let mut restored = 0usize;
                // Parking is durable before its out-of-band notice is attempted. A
                // daemon can therefore exit in the interval between those two
                // operations; replay the existing request seam after restore so a
                // parked gate cannot become invisible forever. Delivery is
                // intentionally at-least-once and keeps the canonical gate
                // reference, allowing adapters to de-duplicate it if needed.
                let mut replay_parked_requests = Vec::new();
                let mut finalize_cancel_requests = Vec::new();
                for pr in runs {
                    // A1: a run persisted while parked at a HITL approval / paused at
                    // a deterministic checkpoint normally holds NO exec claim - it
                    // released its slot on park. Restore it WITHOUT re-establishing a
                    // claim unless the live claim is explicitly marked as retained
                    // after a failed terminal checkpoint decision.
                    //
                    // An executing (Running/Pending) run DID hold a claim, so
                    // re-establish it WITHOUT admission caps: it was already admitted
                    // before the restart, so reconstruction is not new admission. This
                    // keeps `active_runs` and the live-claim count aligned 1:1 even for
                    // an over-cap restored set (the old capped `try_claim_run` silently
                    // dropped the claim over cap, leaving a locally active run with no
                    // store claim). On a renew error the run is left out of
                    // `active_runs` rather than cached orphaned, and the failure is
                    // logged loudly.
                    let parked = matches!(
                        pr.run.status,
                        SopRunStatus::WaitingApproval | SopRunStatus::PausedCheckpoint
                    );
                    if parked {
                        let retained = match self
                            .store
                            .has_retained_terminal_rollback_claim(&pr.run.run_id)
                        {
                            Ok(retained) => retained,
                            Err(e) => {
                                ::clawcrew_log::record!(
                                    WARN,
                                    ::clawcrew_log::Event::new(
                                        module_path!(),
                                        ::clawcrew_log::Action::Note
                                    )
                                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                                    .with_attrs(
                                        ::serde_json::json!({
                                            "run_id": pr.run.run_id.as_str(),
                                            "error": e.to_string(),
                                        })
                                    ),
                                    "SOP engine: failed to inspect parked claim retention marker; failing closed (assuming retained)"
                                );
                                // FAIL CLOSED: a transient inspection read error must NOT
                                // discard a claim the terminal-rollback marker may exist to
                                // preserve (mapping it to `false` here would route into the
                                // release branch and drop that claim). Assume retained: the
                                // run keeps its claim. `heartbeat_claim` is an UPDATE-only
                                // no-op when the claim row is in fact already gone, so this
                                // cannot resurrect a released claim; the lease reaper reclaims
                                // a genuine orphan later. Erring toward keeping is the safe
                                // direction - releasing here could strand a run a real failed
                                // terminal write left restorable.
                                true
                            }
                        };
                        if retained && Self::terminal_rollback_marker_is_stale(&pr.run) {
                            // Crash-window reconcile: a terminal-rollback retention
                            // marker is legitimate ONLY when a genuine TERMINAL write
                            // failed and left the run restorable in its PRE-terminal
                            // parked state — i.e. still awaiting the (retried) decision
                            // at its current checkpoint, with NO recorded result for that
                            // step. A marker on a run that ALREADY recorded a terminal
                            // result for its current step reached this parked gate through
                            // a COMPLETED failure-route continuation (e.g. a denied
                            // checkpoint that Retried and re-parked). Its marker is stale —
                            // release it now rather than renew it forever.
                            ::clawcrew_log::record!(
                                INFO,
                                ::clawcrew_log::Event::new(
                                    module_path!(),
                                    ::clawcrew_log::Action::Note
                                )
                                .with_attrs(::serde_json::json!({
                                    "run_id": pr.run.run_id.as_str(),
                                    "current_step": pr.run.current_step,
                                })),
                                "SOP engine: releasing stale terminal-rollback claim on a continued parked run"
                            );
                            self.release_claim_best_effort(&Self::claim_handle_for_run(&pr.run));
                        } else if retained {
                            self.claims_retained_after_terminal_rollback
                                .insert(pr.run.run_id.clone());
                            self.heartbeat_claim_for_run(&pr.run);
                        } else {
                            // A parked run normally holds no exec slot. A durable store
                            // written by OLD behavior can carry a stale `sop_claims` row
                            // for this run; RELEASE it now so the restored parked run is
                            // genuinely claim-less and does not block admission.
                            self.release_claim_best_effort(&Self::claim_handle_for_run(&pr.run));
                        }
                    } else if let Err(e) = self
                        .store
                        .renew_claim_for_restore(&pr.run.run_id, &pr.run.sop_name)
                    {
                        let span = ::clawcrew_log::attribution_span!(&pr.run);
                        let _guard = span.enter();
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Note
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "run_id": pr.run.run_id.as_str(),
                                "sop_name": pr.run.sop_name.as_str(),
                                "error": e.to_string(),
                            })),
                            "SOP engine: dropping restored run, could not re-establish its store claim"
                        );
                        continue;
                    }
                    let run_id = pr.run.run_id.clone();
                    let cancel_requested = pr.run.status == SopRunStatus::CancelRequested;
                    if self.active_runs.insert(run_id.clone(), pr.run).is_none() {
                        restored += 1;
                        if parked {
                            replay_parked_requests.push(run_id);
                        } else if cancel_requested {
                            finalize_cancel_requests.push(run_id);
                        }
                    }
                }
                // A restored cancellation request has no process-local work
                // left in flight, so startup itself is a safe execution
                // boundary. Finalize before any restored work can resume.
                for run_id in finalize_cancel_requests {
                    if let Err(e) = self.finish_requested_cancellation(&run_id) {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Fail
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "run_id": run_id,
                                "error": e.to_string(),
                            })),
                            "SOP engine: failed to finalize restored cancellation request"
                        );
                    }
                }
                // Reuse the same policy resolution and request construction used
                // by a newly parked run. Restored runs already released any claim,
                // so this is delivery recovery only, not another park transition.
                for run_id in replay_parked_requests {
                    self.notify_park_request(&run_id);
                }
                if restored > 0 {
                    let span = ::clawcrew_log::info_span!(
                        target: "clawcrew_log_internal_scope",
                        "clawcrew_scope",
                        sop_name = "*",
                    );
                    let _guard = span.enter();
                    ::clawcrew_log::record!(
                        INFO,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_attrs(::serde_json::json!({"restored": restored})),
                        &format!("SOP engine restored {restored} run(s) from store")
                    );
                }
            }
            Err(e) => {
                let span = ::clawcrew_log::info_span!(
                    target: "clawcrew_log_internal_scope",
                    "clawcrew_scope",
                    sop_name = "*",
                );
                let _guard = span.enter();
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": e.to_string()})),
                    "SOP engine: failed to restore runs from store"
                );
            }
        }
        self.restore_finished_runs();
    }

    /// Seed the display retention window (`finished_runs`) from the store's
    /// terminal records at boot, newest-first and capped at `max_finished_runs`.
    /// Terminal runs are durable but not part of the active-run rehydrate set, so
    /// without this the Runs surface drops all completed/failed/cancelled runs
    /// across a restart even though they remain on disk.
    fn restore_finished_runs(&mut self) {
        let limit = self.config.max_finished_runs;
        match self.store.load_terminal_runs(limit) {
            Ok(runs) => {
                let mut seeded = 0usize;
                for pr in runs {
                    let span = ::clawcrew_log::attribution_span!(&pr.run);
                    let _guard = span.enter();
                    ::clawcrew_log::record!(
                        DEBUG,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Success)
                            .with_attrs(::serde_json::json!({
                                "run_id": pr.run.run_id.as_str(),
                                "sop_name": pr.run.sop_name.as_str(),
                            })),
                        "SOP engine: seeded terminal run into the retention window"
                    );
                    self.finished_runs.push(pr.run);
                    seeded += 1;
                }
                self.finished_runs
                    .sort_by(|a, b| a.started_at.cmp(&b.started_at));
                if seeded > 0 {
                    let span = ::clawcrew_log::info_span!(
                        target: "clawcrew_log_internal_scope",
                        "clawcrew_scope",
                        sop_name = "*",
                    );
                    let _guard = span.enter();
                    ::clawcrew_log::record!(
                        INFO,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_attrs(::serde_json::json!({"seeded": seeded})),
                        &format!(
                            "SOP engine seeded {seeded} terminal run(s) into the retention window"
                        )
                    );
                }
            }
            Err(e) => {
                let span = ::clawcrew_log::info_span!(
                    target: "clawcrew_log_internal_scope",
                    "clawcrew_scope",
                    sop_name = "*",
                );
                let _guard = span.enter();
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": e.to_string()})),
                    "SOP engine: failed to seed terminal runs from store"
                );
            }
        }
    }

    /// Next monotonic revision for a run: one past whatever the store currently
    /// holds (0 if absent). Keeps every persist strictly newer so the store's
    /// revision guard accepts it; a cheap indexed lookup on either backend.
    fn next_run_revision(&self, run_id: &str) -> u64 {
        match self.store.load_run(run_id) {
            Ok(Some(existing)) => existing.revision.saturating_add(1),
            _ => 0,
        }
    }

    /// Persist a still-active run (best-effort; logs on failure). Cheap no-op
    /// effect for the in-memory default.
    fn persist_active(&self, run_id: &str) {
        let _ = self.persist_active_checked(run_id);
    }

    /// Persist a still-active run and REPORT whether the durable write succeeded.
    /// Returns `true` if there is no such active run (nothing to persist) or the
    /// snapshot was saved; `false` only if `save_run` errored. The park paths use
    /// this so they release the exec claim ONLY after the parked snapshot is
    /// durably written: a run parked in memory but NOT persisted must keep its
    /// slot, or a crash would leave the approval/checkpoint lost while newer
    /// triggers had already admitted into the "freed" slot.
    fn persist_active_checked(&self, run_id: &str) -> bool {
        matches!(
            self.persist_active_checked_with_capacity(run_id, None),
            ActivePersistOutcome::Saved
        )
    }

    fn persist_active_checked_with_capacity(
        &self,
        run_id: &str,
        max_pending: Option<usize>,
    ) -> ActivePersistOutcome {
        let Some(run) = self.active_runs.get(run_id) else {
            return ActivePersistOutcome::Saved;
        };
        self.heartbeat_claim_for_run(run);
        let mut pr = PersistedRun::new(run.clone(), now_iso8601(), run.trigger_event.source);
        // Each persist is a new state revision; the store rejects a
        // same-revision divergent write, so advance past what is stored.
        pr.revision = self.next_run_revision(run_id);
        let outcome = match max_pending {
            Some(max_pending) => {
                match self.store.save_run_with_pending_capacity(&pr, max_pending) {
                    Ok(true) => ActivePersistOutcome::Saved,
                    Ok(false) => ActivePersistOutcome::CapacityFull,
                    Err(e) => {
                        Self::log_persist_failure(run_id, e);
                        ActivePersistOutcome::Failed
                    }
                }
            }
            None => match self.store.save_run(&pr) {
                Ok(()) => ActivePersistOutcome::Saved,
                Err(e) => {
                    Self::log_persist_failure(run_id, e);
                    ActivePersistOutcome::Failed
                }
            },
        };
        if !matches!(outcome, ActivePersistOutcome::CapacityFull) {
            self.notify_run(run, true);
        }
        outcome
    }

    fn log_persist_failure(run_id: &str, e: crate::sop::store::StoreError) {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                .with_attrs(::serde_json::json!({"run_id": run_id, "error": e.to_string()})),
            "SOP engine: failed to persist run"
        );
    }

    fn pending_capacity_limit_for_run(&self, run_id: &str) -> Option<usize> {
        let run = self.active_runs.get(run_id)?;
        let sop = self.sops.iter().find(|sop| sop.name == run.sop_name)?;
        (sop.max_pending_approvals > 0).then_some(sop.max_pending_approvals as usize)
    }

    fn pending_pool_full_reason(&self, sop: &Sop) -> Option<String> {
        if sop.max_pending_approvals == 0 {
            return None;
        }
        let pending = self.pending_count_for_sop(&sop.name);
        if pending >= sop.max_pending_approvals as usize {
            Some(format!(
                "SOP '{}' pending-approval pool full ({pending}/{})",
                sop.name, sop.max_pending_approvals
            ))
        } else {
            None
        }
    }

    fn pending_pool_capacity_raced_reason(&self, sop: &Sop) -> String {
        let pending = self.pending_count_for_sop(&sop.name);
        format!(
            "SOP '{}' pending-approval pool full ({pending}/{})",
            sop.name, sop.max_pending_approvals
        )
    }

    fn log_pending_capacity_full(run_id: &str, reason: &str) {
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                .with_attrs(::serde_json::json!({"run_id": run_id, "reason": reason})),
            "SOP engine: pending-approval pool full at park transition; KEEPING the exec claim"
        );
    }
    fn persisted_active_snapshot(&self, run_id: &str) -> Result<(PersistedRun, SopRun)> {
        let run = self
            .active_runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        self.heartbeat_claim_for_run(&run);
        let mut persisted = PersistedRun::new(run.clone(), now_iso8601(), run.trigger_event.source);
        persisted.revision = self.next_run_revision(run_id);
        Ok((persisted, run))
    }

    /// Persist an active run transition and append its gate event as one store
    /// outcome. Used by `resolve_gate` so the durable gate ledger cannot get ahead
    /// of the run state transition it authorizes.
    pub(crate) fn persist_active_with_gate_event(
        &self,
        run_id: &str,
        event: &SopEventRecord,
    ) -> Result<()> {
        let (persisted, run) = self.persisted_active_snapshot(run_id)?;
        self.store.save_run_with_event(&persisted, event).map_err(|e| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(
                        ::serde_json::json!({"run_id": run_id, "error": e.to_string()})
                    ),
                "SOP engine: gate resolution persistence failed; run transition and ledger remain uncommitted"
            );
            anyhow::Error::new(e)
        })?;
        self.notify_run(&run, true);
        Ok(())
    }

    /// Park a run (WaitingApproval / PausedCheckpoint) and free its exec slot, but
    /// ONLY after the parked snapshot is durably persisted. If the persist fails,
    /// the claim is KEPT (fail closed): the run stays correctly counted against
    /// capacity, so it is never both claimless AND un-persisted (which a crash
    /// would turn into a lost park while newer triggers had already admitted into
    /// the "freed" slot). The slot is held until a later persist succeeds,
    /// trading a little concurrency for no lost park.
    fn persist_parked_snapshot_then_release_claim(&mut self, run_id: &str) -> ParkPersistOutcome {
        let max_pending = self.pending_capacity_limit_for_run(run_id);
        match self.persist_active_checked_with_capacity(run_id, max_pending) {
            ActivePersistOutcome::Saved => {
                self.claims_pending_persist.remove(run_id);
                self.release_claim_on_park(run_id);
                ParkPersistOutcome::Released
            }
            ActivePersistOutcome::CapacityFull => ParkPersistOutcome::CapacityFull,
            ActivePersistOutcome::Failed => {
                // Track this run so `heartbeat_active_claims` keeps renewing its KEPT
                // claim despite the park status (see `claims_pending_persist`'s doc):
                // otherwise the claim's lease goes un-renewed and the maintenance
                // reaper reclaims it once it expires, silently undoing the fail-closed
                // keep and over-admitting a newer trigger.
                self.claims_pending_persist.insert(run_id.to_string());
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"run_id": run_id})),
                    "SOP engine: parked snapshot not persisted; KEEPING the exec claim (fail closed) so the park is not lost"
                );
                ParkPersistOutcome::PersistFailed
            }
        }
    }

    /// Retry the durable persist for every run in `claims_pending_persist`. A
    /// retry that now succeeds completes the deferred park transition (releases
    /// the claim). A retry that still fails, or that now finds the pending pool
    /// full, leaves the run tracked - but the persist helper heartbeats the claim
    /// BEFORE attempting the write, unconditionally, so even an unsaved retry still
    /// renews the kept claim's lease. This is what keeps `reap_expired_claims`
    /// from reclaiming it: called every maintenance tick, a park that never
    /// manages to persist still gets its claim renewed once per tick for as long
    /// as it stays parked.
    fn retry_pending_park_persists(&mut self) {
        let pending: Vec<String> = self.claims_pending_persist.iter().cloned().collect();
        for run_id in pending {
            let Some(status) = self.active_runs.get(&run_id).map(|run| run.status) else {
                // The run left active_runs some other way (finished/evicted);
                // nothing left to retry or release.
                self.claims_pending_persist.remove(&run_id);
                continue;
            };
            let max_pending = self.pending_capacity_limit_for_run(&run_id);
            match self.persist_active_checked_with_capacity(&run_id, max_pending) {
                ActivePersistOutcome::Saved => {
                    self.claims_pending_persist.remove(&run_id);
                    // Only release the claim if the run is STILL parked. The entry
                    // guards in `resolve_gate`/`approve_step`/`resume_deterministic_run`
                    // (`is_park_persist_pending`) already refuse to resume a run while
                    // it is tracked here, so this should be unreachable in practice -
                    // but if a run somehow left the parked state without going through
                    // one of those guarded paths, its claim is now legitimately held
                    // by that transition and must NOT be released out from under it.
                    if !holds_exec_claim(status) {
                        self.release_claim_on_park(&run_id);
                        // The initial park deliberately withheld its route notice while
                        // the snapshot was not durable. This successful retry is the
                        // single point that makes the parked gate recoverable, so emit
                        // the deferred request now. Removing the pending marker first
                        // prevents later maintenance ticks from sending it again.
                        self.notify_park_request(&run_id);
                    }
                }
                ActivePersistOutcome::CapacityFull | ActivePersistOutcome::Failed => {}
            }
        }
    }

    fn retry_capacity_blocked_gated_pends(&mut self) {
        let candidates: Vec<String> = self
            .active_runs
            .values()
            .filter(|run| run.status == SopRunStatus::Pending)
            .map(|run| run.run_id.clone())
            .collect();

        for run_id in candidates {
            let Some((sop, step)) = self.active_runs.get(&run_id).and_then(|run| {
                let sop = self.sops.iter().find(|sop| sop.name == run.sop_name)?;
                // Resolve the gated step by NUMBER, not vector index: step numbers
                // are not required to be contiguous/1-based, so an index lookup
                // strands a non-contiguous pending step (it never re-promotes and
                // leaks its exec claim).
                let step = sop
                    .steps
                    .iter()
                    .find(|step| step.number == run.current_step)?;
                pending_step_blocks_direct_advance(sop, step).then(|| (sop.clone(), step.clone()))
            }) else {
                continue;
            };

            if self.pending_pool_full_reason(&sop).is_some() {
                continue;
            }

            if step.kind == SopStepKind::Checkpoint {
                if let Err(e) = self.persist_deterministic_state(&run_id, &sop, true) {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "run_id": run_id,
                                "error": e.to_string(),
                            })),
                        "SOP maintenance: checkpoint pending-cap retry could not persist state"
                    );
                    continue;
                }
                if let Some(run) = self.active_runs.get_mut(&run_id) {
                    run.status = SopRunStatus::PausedCheckpoint;
                    run.waiting_since = Some(now_iso8601());
                }
            } else if let Some(run) = self.active_runs.get_mut(&run_id) {
                run.status = SopRunStatus::WaitingApproval;
                run.waiting_since = Some(now_iso8601());
            }

            match self.persist_parked_snapshot_then_release_claim(&run_id) {
                // The park is now durable: deliver the deferred approval-request
                // notice withheld while the initial persist was failing. This is a
                // no-op when the step has no policy request route.
                ParkPersistOutcome::Released => self.notify_park_request(&run_id),
                ParkPersistOutcome::PersistFailed => {}
                ParkPersistOutcome::CapacityFull => {
                    let reason = self.pending_pool_capacity_raced_reason(&sop);
                    Self::log_pending_capacity_full(&run_id, &reason);
                    self.mark_step_pending(&run_id, &sop, step.number, reason);
                }
            }
        }
    }

    /// True if `run_id`'s exec claim is being kept pending a retried park persist
    /// (`claims_pending_persist`): its most recent park snapshot has not yet been
    /// durably written. The three resume paths (`resolve_gate` via
    /// `clear_waiting_gate`, `approve_step`, `resume_deterministic_run`) must
    /// refuse to proceed while this is true - the kept claim predates the resume
    /// attempt, so a later rollback (on a ledger/audit failure) or a maintenance
    /// retry's release would either drop a claim that must survive, or release a
    /// claim out from under a run that has since started executing. Fail closed
    /// here instead: the gate/checkpoint stays parked, re-resolvable once a
    /// maintenance tick's retry durably persists the park.
    pub(crate) fn is_park_persist_pending(&self, run_id: &str) -> bool {
        self.claims_pending_persist.contains(run_id)
    }

    /// A prompt becomes stale only after a replacement presentation is durable.
    /// A gate can update its in-memory revision before its parked snapshot saves;
    /// finalizing the old prompt in that window would leave operators without a
    /// recoverable replacement after a crash.
    pub fn is_gate_reference_superseded(&self, run_id: &str, reference_revision: u32) -> bool {
        self.active_runs.get(run_id).is_some_and(|run| {
            run.revision != reference_revision && !self.is_park_persist_pending(run_id)
        })
    }

    /// Admit a run through the store CAS claim before it becomes locally active.
    /// The durable store is the concurrency source of truth; `active_runs` is the
    /// execution cache/status surface.
    fn claim_admission(&self, run_id: &str, sop: &Sop) -> Result<ClaimToken> {
        match self.store.try_claim_run(
            run_id,
            &sop.name,
            sop.max_concurrent as usize,
            self.config.max_concurrent_total,
        ) {
            Ok(Some(token)) => Ok(token),
            Ok(None) => {
                bail!(
                    "Cannot start SOP '{}': cooldown or concurrency limit reached",
                    sop.name
                );
            }
            Err(e) => Err(anyhow::Error::new(e)),
        }
    }

    fn release_claim_best_effort(&self, token: &ClaimToken) {
        if let Err(e) = self.store.release_claim(token) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "run_id": token.run_id.as_str(),
                        "error": e.to_string(),
                    })),
                "SOP engine: failed to release run admission claim"
            );
        }
    }

    fn claim_handle_for_run(run: &SopRun) -> ClaimToken {
        ClaimToken {
            run_id: run.run_id.clone(),
            sop_name: run.sop_name.clone(),
            claimed_at: String::new(),
            lease_expires: String::new(),
            holder: "engine".to_string(),
        }
    }

    fn heartbeat_claim_for_run(&self, run: &SopRun) {
        let token = Self::claim_handle_for_run(run);
        if let Err(e) = self.store.heartbeat_claim(&token) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({
                        "run_id": run.run_id.as_str(),
                        "error": e.to_string(),
                    })),
                "SOP engine: failed to heartbeat run admission claim"
            );
        }
    }

    fn heartbeat_active_claims(&self) {
        // Only EXECUTING runs hold a claim; a parked run released its claim on park,
        // so heartbeating it would (on a durable store carrying a stale row from the
        // old behavior) extend a claim that should be gone. Skip parked runs. A run
        // in `claims_pending_persist` (a park whose snapshot failed to persist,
        // KEEPING its claim) is renewed by `retry_pending_park_persists` instead -
        // called just before this each tick - so its kept claim's lease never goes
        // un-renewed even while parked.
        for run in self.active_runs.values() {
            if holds_exec_claim(run.status) {
                self.heartbeat_claim_for_run(run);
            }
        }
        for run_id in &self.claims_retained_after_terminal_rollback {
            if let Some(run) = self.active_runs.get(run_id)
                && !holds_exec_claim(run.status)
            {
                self.heartbeat_claim_for_run(run);
            }
        }
    }

    /// A1: release a parked run's exec claim so its concurrency slot frees for
    /// other triggers. A run waiting on a human approval (or paused at a
    /// deterministic checkpoint) is not executing, so it must not hold an
    /// execution slot. The run stays in `active_runs` - every reader (gate_state,
    /// overdue_waiting_run_ids, resolve_gate, resume) and `finish_run` rely on it
    /// still being there; only the store CAS claim is dropped. Best-effort +
    /// logged. Persist the parked state BEFORE calling this so a crash in the
    /// window leaves a restorable parked run rather than a freed-but-unpersisted one.
    pub(crate) fn release_claim_on_park(&self, run_id: &str) {
        if let Some(run) = self.active_runs.get(run_id) {
            self.release_claim_best_effort(&Self::claim_handle_for_run(run));
        }
    }

    /// Checked counterpart to `release_claim_on_park`: release a parked run's exec
    /// claim and REPORT a store failure instead of swallowing it. Used on the
    /// checkpoint-denial CONTINUATION path, where the reacquired claim still carries
    /// the durable terminal-rollback retention marker. If that release is swallowed
    /// and fails, the marker survives on a run that actually CONTINUED (did not
    /// terminal-rollback), and `restore_runs` would then renew that stale claim
    /// forever, leaking the slot. Returning the error lets the caller fail closed
    /// (roll back + surface it) rather than report success with a live marker.
    /// `Ok(())` when there is no such active run (nothing to release).
    fn release_claim_checked(&self, run_id: &str) -> Result<(), crate::sop::store::StoreError> {
        match self.active_runs.get(run_id) {
            Some(run) => self.store.release_claim(&Self::claim_handle_for_run(run)),
            None => Ok(()),
        }
    }

    /// Whether a durable terminal-rollback retention marker on a restored parked
    /// run is STALE. A legitimate marker guards a run whose TERMINAL write failed and
    /// left it restorable in its pre-terminal parked state — still awaiting the
    /// retried decision at its current checkpoint, which therefore has NO recorded
    /// result yet. If the current step ALREADY has a recorded `step_result`, the run
    /// reached this parked gate through a COMPLETED failure-route continuation (e.g. a
    /// denied checkpoint that `Retry`-re-parked at the same step), so the marker is
    /// stale and must be released rather than renewed forever.
    ///
    /// This is a HEURISTIC, not an exact classifier, and it errs on the SAFE side.
    /// It has two disclosed residuals, both bounded and benign:
    /// - It does NOT catch a denial that routed via `Goto` to a DIFFERENT, fresh
    ///   checkpoint (new current step, no result yet): that durable footprint is
    ///   indistinguishable from a legitimate terminal rollback at that fresh checkpoint,
    ///   so a stale marker there survives. The checked continuation release plus the
    ///   lease reaper cover that path in the non-crash case (see `deny_checkpoint`).
    /// - Symmetrically, it CAN flag a legitimate marker: a `Retry` checkpoint denied
    ///   enough times to re-park at the same step (leaving a `Failed` result there) and
    ///   then routed to a terminal `Fail` whose terminal write fails takes
    ///   `deny_checkpoint`'s retain-and-restore branch while carrying a result for its
    ///   current step; a restart before re-resolution would release that legitimate
    ///   marker. That direction is safe: the run is still restored into `active_runs`
    ///   (never lost) and only loses its HELD slot, degrading to standard parked
    ///   semantics — it re-acquires its exec slot on its next decision, capped
    ///   (subject to `max_concurrent`/`max_concurrent_total`) via
    ///   `reacquire_claim_on_resume` for an approval or checkpoint-approve resume, or
    ///   uncapped via `reacquire_claim_uncapped` for a subsequent denial. No double
    ///   execution, no permanent leak, no hard-cap violation.
    fn terminal_rollback_marker_is_stale(run: &SopRun) -> bool {
        run.step_results
            .iter()
            .any(|result| result.step_number == run.current_step)
    }

    /// A1: re-establish a RESUMING run's exec claim, subject to the SOP's per-SOP
    /// `max_concurrent` AND the global `max_concurrent_total`. A run parked at a HITL
    /// approval / deterministic checkpoint released its exec slot on park; resuming
    /// it must re-admit through the SAME store CAS (`try_claim_run`) a fresh start
    /// uses, so a burst of simultaneous approvals can never push executing runs past
    /// the configured caps. (That burst is the reviewed defect: many runs park,
    /// releasing their slots, then all resume at once - the uncapped restore path
    /// let them oversubscribe.) Three outcomes:
    /// - `Ok(())`                 a slot was available; the run holds its claim and may resume.
    /// - `Err(ResumeAtCapacity)`  the cap is saturated. TYPED backpressure, NOT a fault:
    ///   the caller leaves the run parked and re-resolvable (`resolve_gate` reports
    ///   `DeferredAtCapacity`; the checkpoint paths surface it to the operator), and a
    ///   later approval attempt or the timeout tick's retry resumes it once a slot frees.
    /// - `Err(_)`                 a store fault (fail-closed, as before): abort the resume,
    ///   never execute uncounted.
    ///
    /// A missing run is a no-op `Ok` (the caller already validated it exists). The
    /// checkpoint-DENIAL path uses `reacquire_claim_uncapped` instead - a denial may
    /// TERMINATE the run, and gating a terminating run on a free slot would refuse to
    /// end it under load and strand it.
    pub(crate) fn reacquire_claim_on_resume(&self, run_id: &str) -> Result<()> {
        let Some((rid, sop_name)) = self
            .active_runs
            .get(run_id)
            .map(|run| (run.run_id.clone(), run.sop_name.clone()))
        else {
            return Ok(());
        };
        // Resolve the per-SOP cap exactly as the normal admit path does. The resume
        // pre-flights (`can_clear_waiting_gate` / `can_advance_deterministic_step`)
        // already proved the SOP is still loaded before we reach here; if it somehow
        // is not, fail closed rather than resume uncounted.
        let per_sop_cap = self
            .get_sop(&sop_name)
            .map(|sop| sop.max_concurrent as usize);
        let Some(per_sop_cap) = per_sop_cap else {
            return Err(anyhow::Error::msg(format!(
                "failed to re-acquire exec claim on resume for run {rid}: SOP '{sop_name}' no longer loaded"
            )));
        };
        match self.store.try_claim_run(
            &rid,
            &sop_name,
            per_sop_cap,
            self.config.max_concurrent_total,
        ) {
            Ok(Some(_token)) => Ok(()),
            Ok(None) => Err(anyhow::Error::new(ResumeAtCapacity {
                run_id: rid,
                sop_name,
            })),
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "run_id": rid.as_str(),
                            "error": e.to_string(),
                        })),
                    "SOP engine: resume aborted, could not re-acquire the run admission claim (fail-closed)"
                );
                Err(anyhow::Error::msg(format!(
                    "failed to re-acquire exec claim on resume for run {rid}: {e}"
                )))
            }
        }
    }

    /// UNCAPPED exec-claim re-establishment, for the checkpoint-DENIAL path only
    /// (`deny_checkpoint`). A denial may TERMINATE the run - it reacquires the claim
    /// to write terminal state and the terminal-rollback retention marker atomically,
    /// so this is rollback/atomicity machinery, not new admission, and must never be
    /// blocked by the concurrency cap (refusing to terminate a run under load would
    /// strand it, since it already released its slot at park). This is the ORIGINAL
    /// uncapped restore behavior; the capped `reacquire_claim_on_resume` above governs
    /// the three resume-to-continue paths (approval approve, checkpoint approve,
    /// deterministic resume). Fail-CLOSED on a store error, as before.
    pub(crate) fn reacquire_claim_uncapped(&self, run_id: &str) -> Result<()> {
        let Some(run) = self.active_runs.get(run_id) else {
            return Ok(());
        };
        self.store
            .renew_claim_for_restore(&run.run_id, &run.sop_name)
            .map(|_| ())
            .map_err(|e| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "run_id": run.run_id.as_str(),
                            "error": e.to_string(),
                        })),
                    "SOP engine: resume aborted, could not re-acquire the run admission claim (fail-closed)"
                );
                anyhow::Error::msg(format!(
                    "failed to re-acquire exec claim on resume for run {run_id}: {e}"
                ))
            })
    }

    /// Persist a run that has reached a terminal state and release its claim atomically.
    fn persist_terminal(&self, run: &SopRun) -> Result<()> {
        let mut pr = PersistedRun::new(run.clone(), now_iso8601(), run.trigger_event.source);
        // The terminal write is the run's final revision; advance past the last
        // active snapshot so the store's revision guard accepts it.
        pr.revision = self.next_run_revision(&run.run_id);
        self.store.finish_run(&run.run_id, &pr).map_err(|e| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(
                        ::serde_json::json!({"run_id": run.run_id, "error": e.to_string()})
                    ),
                "SOP engine: terminal persistence failed; run and admission claim remain active"
            );
            anyhow::Error::new(TerminalPersistenceRetained {
                run_id: run.run_id.clone(),
                source: e,
            })
        })?;
        self.notify_run(run, false);
        Ok(())
    }

    /// Terminal counterpart to `persist_active_with_gate_event`: persist the
    /// terminal run, release its claim, and append the gate-resolution ledger row
    /// in one store transaction.
    fn persist_terminal_with_gate_event(&self, run: &SopRun, event: &SopEventRecord) -> Result<()> {
        let mut pr = PersistedRun::new(run.clone(), now_iso8601(), run.trigger_event.source);
        pr.revision = self.next_run_revision(&run.run_id);
        self.store
            .finish_run_with_event(&run.run_id, &pr, event)
            .map_err(|e| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(
                            ::serde_json::json!({"run_id": run.run_id, "error": e.to_string()})
                        ),
                    "SOP engine: terminal gate resolution persistence failed; run and ledger remain uncommitted"
                );
                anyhow::Error::new(TerminalPersistenceRetained {
                    run_id: run.run_id.clone(),
                    source: e,
                })
            })?;
        self.notify_run(run, false);
        Ok(())
    }

    fn record_transition_event(
        &self,
        run_id: &str,
        kind: &str,
        reason: Option<String>,
        payload: serde_json::Value,
    ) {
        let ev = SopEventRecord {
            run_id: run_id.to_string(),
            seq: 0,
            ts: now_iso8601(),
            kind: kind.to_string(),
            actor: None,
            reason,
            payload,
        };
        if let Err(e) = self.store.append_event(&ev) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                    .with_attrs(
                        ::serde_json::json!({"run_id": run_id, "kind": kind, "error": e.to_string()})
                    ),
                "SOP engine: failed to append transition event"
            );
        }
    }

    /// Load/reload SOPs from the configured directory, resolved against
    /// `install_root` (the install root, `config_path`'s parent).
    pub fn reload(&mut self, install_root: &Path) {
        self.sops = load_sops(
            install_root,
            self.config.sops_dir.as_deref(),
            super::parse_execution_mode(&self.config.default_execution_mode),
        );
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            &format!("SOP engine loaded {} SOPs", self.sops.len())
        );
    }

    /// Return all loaded SOP definitions.
    pub fn sops(&self) -> &[Sop] {
        &self.sops
    }

    #[cfg(test)]
    pub(crate) fn replace_sops_for_test(&mut self, sops: Vec<Sop>) {
        self.sops = sops;
    }

    /// Return all active (in-flight) runs.
    pub fn active_runs(&self) -> &HashMap<String, SopRun> {
        &self.active_runs
    }

    /// Look up a run by ID (active or finished).
    pub fn get_run(&self, run_id: &str) -> Option<&SopRun> {
        self.active_runs
            .get(run_id)
            .or_else(|| self.finished_runs.iter().find(|r| r.run_id == run_id))
    }

    /// Look up an SOP by name.
    pub fn get_sop(&self, name: &str) -> Option<&Sop> {
        self.sops.iter().find(|s| s.name == name)
    }

    // ── Trigger matching ────────────────────────────────────────

    /// Match an incoming event against all loaded SOPs and return the names of
    /// SOPs whose triggers match.
    pub fn match_trigger(&self, event: &SopEvent) -> Vec<&Sop> {
        self.sops
            .iter()
            .filter(|sop| sop.triggers.iter().any(|t| trigger_matches(t, event)))
            .collect()
    }

    /// True when any loaded SOP has a trigger of this source. Fan-in
    /// callers use this as a cheap pre-filter before building and
    /// dispatching an event.
    pub fn wants_source(&self, source: SopTriggerSource) -> bool {
        self.sops
            .iter()
            .any(|sop| sop.triggers.iter().any(|t| t.source() == source))
    }

    // ── Run lifecycle ───────────────────────────────────────────

    /// Check whether a new run can be started for the given SOP
    /// (respects cooldown and concurrency limits).
    pub fn can_start(&self, sop_name: &str) -> bool {
        let sop = match self.get_sop(sop_name) {
            Some(s) => s,
            None => return false,
        };
        let (active_for_sop, active_total) = self.exec_counts(sop_name);
        if active_for_sop >= sop.max_concurrent as usize
            || active_total >= self.config.max_concurrent_total
        {
            return false;
        }
        !self.in_cooldown(sop)
    }

    /// Live *executing* run counts `(for_sop, total)`. The store's CAS claims are
    /// the authoritative concurrency source (shared across engine holders); parked
    /// runs release their claim (A1), so they are excluded. Falls back to the
    /// in-memory view (also parked-excluded) only if the store call errors.
    pub(crate) fn exec_counts(&self, sop_name: &str) -> (usize, usize) {
        match self.store.claim_counts(sop_name) {
            Ok(counts) => counts,
            Err(_) => (
                self.active_runs
                    .values()
                    .filter(|r| holds_exec_claim(r.status) && r.sop_name == sop_name)
                    .count(),
                self.active_runs
                    .values()
                    .filter(|r| holds_exec_claim(r.status))
                    .count(),
            ),
        }
    }

    /// Whether the SOP's cooldown window is still active (blocks a new start). Read
    /// from the shared store so every engine holder observes the same completion
    /// marker; falls back to the local finished list only on a store error.
    fn in_cooldown(&self, sop: &Sop) -> bool {
        if sop.cooldown_secs == 0 {
            return false;
        }
        let last_completed = match self.store.last_terminal_completed_at(&sop.name) {
            Ok(completed) => completed,
            Err(_) => self
                .last_finished_run(&sop.name)
                .and_then(|last| last.completed_at.clone()),
        };
        matches!(last_completed, Some(ts) if !cooldown_elapsed(&ts, sop.cooldown_secs))
    }

    /// Count runs of `sop_name` currently parked at a HITL approval / checkpoint
    /// (they hold no exec slot). This is the "pending-approval pool" A2 bounds.
    fn pending_count_for_sop(&self, sop_name: &str) -> usize {
        // Read the shared store's active-run surface so multiple engine holders see
        // one source of truth for the pending-approval pool (mirrors exec_counts,
        // which reads store claim_counts). A persisted `WaitingApproval` run parked
        // by a sibling engine is counted here, so `max_pending_approvals` is not
        // silently exceeded across processes. Fall back to this engine's local view
        // only when the store errors.
        match self.store.load_active_runs() {
            Ok(runs) => runs
                .iter()
                .filter(|pr| pr.run.sop_name == sop_name && !holds_exec_claim(pr.run.status))
                .count(),
            Err(_) => self
                .active_runs
                .values()
                .filter(|r| r.sop_name == sop_name && !holds_exec_claim(r.status))
                .count(),
        }
    }

    /// First active (executing or parked) run id for `sop_name`, if any - the
    /// `Coalesce` policy names the in-flight run a new trigger folds into. Resolved
    /// from the SHARED store's active-run surface (like exec/pending counts), so an
    /// engine whose local map is empty still finds a sibling engine's in-flight run
    /// and returns `Coalesce` rather than `Defer` (which on a durable transport would
    /// churn redeliveries instead of acknowledging the trigger as absorbed). Falls
    /// back to the local map only on a store error.
    fn first_active_run_for_sop(&self, sop_name: &str) -> Option<String> {
        match self.store.load_active_runs() {
            Ok(runs) => runs
                .into_iter()
                .find(|pr| pr.run.sop_name == sop_name)
                .map(|pr| pr.run.run_id),
            Err(_) => self
                .active_runs
                .values()
                .find(|r| r.sop_name == sop_name)
                .map(|r| r.run_id.clone()),
        }
    }

    /// A2: decide how to admit a matched trigger for `sop_name` under its
    /// `SopAdmissionPolicy`. `Admit` still passes through the authoritative CAS in
    /// `start_run`; the other outcomes are surfaced by the dispatch layer so a
    /// non-admitted trigger is never silently lost. A cooldown or unknown SOP drops
    /// regardless of policy (a cooldown is a deliberate rate limit, not backpressure).
    ///
    /// AUTHORITY: within a SINGLE daemon this decision is authoritative - the engine
    /// `Mutex` serializes `evaluate_admission` + the CAS claim, so two triggers cannot
    /// both admit past the policy. The exec-slot cap is additionally CAS-authoritative
    /// via the shared store even ACROSS engines. The pending-approval pool
    /// (`max_pending_approvals`), however, is only ADVISORY across engines: a run
    /// parks at approval only AFTER it has executed, so its pending slot cannot be
    /// atomically pre-reserved at admission time, and two engines sharing a store can
    /// each admit a run that later parks. Making the pending cap cross-engine-
    /// authoritative requires a store-level two-phase reservation (a follow-up); the
    /// single-daemon deployment - the common case - is fully authoritative today.
    pub fn evaluate_admission(&self, sop_name: &str) -> SopAdmission {
        let sop = match self.get_sop(sop_name) {
            Some(s) => s,
            None => {
                return SopAdmission::Drop {
                    reason: format!("SOP '{sop_name}' not loaded"),
                };
            }
        };
        if self.in_cooldown(sop) {
            return SopAdmission::Drop {
                reason: format!("SOP '{sop_name}' in cooldown"),
            };
        }

        let (exec_for_sop, exec_total) = self.exec_counts(sop_name);
        let pending_for_sop = self.pending_count_for_sop(sop_name);
        let exec_slot_free = exec_for_sop < sop.max_concurrent as usize
            && exec_total < self.config.max_concurrent_total;
        let policy = sop.admission_policy;

        // Pending-approval-pool backpressure (every policy but Drop, which drops).
        if sop.max_pending_approvals > 0 && pending_for_sop >= sop.max_pending_approvals as usize {
            let reason = format!("SOP '{sop_name}' pending-approval pool full ({pending_for_sop})");
            return match policy {
                SopAdmissionPolicy::Drop => SopAdmission::Drop { reason },
                _ => SopAdmission::Defer { reason },
            };
        }

        match policy {
            SopAdmissionPolicy::Parallel => {
                if exec_slot_free {
                    SopAdmission::Admit
                } else {
                    SopAdmission::Defer {
                        reason: format!("SOP '{sop_name}' execution slots full"),
                    }
                }
            }
            SopAdmissionPolicy::Hold => {
                if exec_for_sop + pending_for_sop == 0 && exec_slot_free {
                    SopAdmission::Admit
                } else {
                    SopAdmission::Defer {
                        reason: format!("SOP '{sop_name}' held (a run is already in flight)"),
                    }
                }
            }
            SopAdmissionPolicy::Coalesce => {
                if exec_for_sop + pending_for_sop == 0 && exec_slot_free {
                    SopAdmission::Admit
                } else if let Some(existing_run_id) = self.first_active_run_for_sop(sop_name) {
                    SopAdmission::Coalesce { existing_run_id }
                } else {
                    SopAdmission::Defer {
                        reason: format!("SOP '{sop_name}' execution slots full"),
                    }
                }
            }
            SopAdmissionPolicy::Drop => {
                if exec_slot_free {
                    SopAdmission::Admit
                } else {
                    SopAdmission::Drop {
                        reason: format!("SOP '{sop_name}' execution slots full (drop policy)"),
                    }
                }
            }
        }
    }

    /// A2 per-message idempotency: the run already started for `(sop_name, dedup_key)`, if
    /// one is in the bounded window AND the key is not ambiguous. Used by dispatch to
    /// coalesce a broker redelivery of the same message. Returns `None` for an AMBIGUOUS
    /// key (empty run - one a distinct fresh delivery reused): such a key must never
    /// coalesce, so its deliveries dispatch (a duplicate at worst, never a lost trigger).
    pub(crate) fn dispatch_dedup_lookup(&self, sop_name: &str, dedup_key: &str) -> Option<String> {
        let composite = dispatch_dedup_composite(sop_name, dedup_key);
        self.dispatch_dedup
            .iter()
            .find(|(k, _)| *k == composite)
            .and_then(|(_, run_id)| (!run_id.is_empty()).then(|| run_id.clone()))
    }

    /// A2: a FRESH (non-redelivery) delivery arrived for `(sop_name, dedup_key)`. If that
    /// key is ALREADY in the window a distinct delivery is REUSING a message-id (an AMQP
    /// contract violation); mark it AMBIGUOUS (empty run) so neither it nor a later
    /// redelivery ever coalesces - the safe direction is a duplicate run, never ACKing a
    /// distinct trigger away. Called BEFORE admission, so it also covers a reused-id
    /// delivery that then defers and is broker-redelivered.
    pub(crate) fn note_fresh_dispatch_key(&mut self, sop_name: &str, dedup_key: &str) {
        let composite = dispatch_dedup_composite(sop_name, dedup_key);
        if let Some(entry) = self
            .dispatch_dedup
            .iter_mut()
            .find(|(k, _)| *k == composite)
        {
            entry.1.clear();
        }
    }

    /// Record that a run started for `(sop_name, dedup_key)` so a later redelivery of the
    /// same message coalesces. A new key records its run; an existing key that maps to a
    /// DIFFERENT run (a reused message-id) is marked AMBIGUOUS (empty run - never
    /// coalesce). Bounded FIFO so the window self-trims.
    ///
    /// BEST-EFFORT and BOUNDED, by design: the window is in-memory and capped at
    /// `DISPATCH_DEDUP_CAP`. If a redelivery arrives after the process restarted or after
    /// more than the cap of other starts have pushed this key out, the dedup MISSES and
    /// the SOP may run again - this is the SAFE failure direction (an at-least-once
    /// duplicate, never a lost message). An eviction that drops a key whose run is still
    /// active is logged so the miss is observable rather than silent.
    pub(crate) fn record_dispatch_dedup(&mut self, sop_name: &str, dedup_key: &str, run_id: &str) {
        let composite = dispatch_dedup_composite(sop_name, dedup_key);
        if let Some(entry) = self
            .dispatch_dedup
            .iter_mut()
            .find(|(k, _)| *k == composite)
        {
            // Reused message-id (different run, or already ambiguous): mark ambiguous.
            if entry.1 != run_id {
                entry.1.clear();
            }
            return;
        }
        self.dispatch_dedup
            .push_back((composite, run_id.to_string()));
        while self.dispatch_dedup.len() > DISPATCH_DEDUP_CAP {
            if let Some((_, evicted_run)) = self.dispatch_dedup.pop_front()
                && self.active_runs.contains_key(&evicted_run)
            {
                ::clawcrew_log::record!(
                    DEBUG,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_attrs(::serde_json::json!({
                            "evicted_run_id": evicted_run,
                            "cap": DISPATCH_DEDUP_CAP,
                        })),
                    "SOP dispatch: per-message dedup window evicted a still-active run's \
                     key (window full); a later redelivery of that message may re-run it"
                );
            }
        }
    }

    /// Start a new SOP run. Returns the first action to take.
    /// Deterministic SOPs are automatically routed to `start_deterministic_run`.
    /// Enforce the SOP's admission policy at a start entrypoint. `Admit` proceeds;
    /// any other outcome declines the start with a descriptive error so a trigger is
    /// never run past its policy. dispatch pre-consults `evaluate_admission` and only
    /// reaches a start path on `Admit`, so re-checking here (under the same held lock)
    /// is idempotent; a DIRECT caller (`sop_execute`, or `start_deterministic_run`)
    /// would otherwise bypass Hold / Coalesce / the `max_pending_approvals` pool.
    fn enforce_admission(&self, sop_name: &str) -> Result<()> {
        match self.evaluate_admission(sop_name) {
            SopAdmission::Admit => Ok(()),
            SopAdmission::Coalesce { existing_run_id } => bail!(
                "SOP '{sop_name}' not started: coalesced into in-flight run {existing_run_id}"
            ),
            SopAdmission::Defer { reason } | SopAdmission::Drop { reason } => {
                bail!("SOP '{sop_name}' not started: {reason}")
            }
        }
    }

    fn rollback_failed_start(
        &mut self,
        run_id: &str,
        claim: &ClaimToken,
        err: anyhow::Error,
    ) -> anyhow::Error {
        if err.is::<TerminalPersistenceRetained>() {
            return err;
        }
        self.active_runs.remove(run_id);
        self.release_claim_best_effort(claim);
        err
    }

    /// Undo a SUCCESSFUL `activate_reserved_run` that must be reversed because a LATER
    /// sibling in the same all-or-nothing AMQP multi-match batch failed to activate.
    /// Activation runs no irreversible side effect (deterministic execution and the LLM
    /// agent loop both run LATER, in `record_started_run` / the driver), so the run is
    /// safe to reverse. Two cases:
    /// - A still-EXECUTING sibling (`holds_exec_claim` true) never durably persisted during
    ///   activation: drop it in-memory and release its exec claim.
    /// - A sibling that PARKED at a step-1 approval/checkpoint gate DID durably persist its
    ///   parked snapshot (and already released its claim). Dropping it only in-memory would
    ///   ORPHAN that durable row: after a restart, `restore_runs` would reconstruct it,
    ///   duplicating a run whose whole delivery was deferred + requeued. Durably supersede it
    ///   with a terminal `Cancelled` (a higher revision the store's guard accepts) so restore
    ///   skips it. Best-effort: a store failure here only leaves the bounded orphan back
    ///   (logged), never a double execution — the sibling never ran.
    pub(crate) fn rollback_activated_run(&mut self, run_id: &str) {
        let Some(mut run) = self.active_runs.remove(run_id) else {
            return;
        };
        if holds_exec_claim(run.status) {
            self.release_claim_best_effort(&Self::claim_handle_for_run(&run));
            return;
        }
        // Parked sibling: its durable snapshot must not survive the rollback.
        run.status = SopRunStatus::Cancelled;
        run.completed_at = Some(now_iso8601());
        if let Err(e) = self.persist_terminal(&run) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "run_id": run.run_id.as_str(),
                        "error": e.to_string(),
                    })),
                "SOP dispatch: could not durably cancel a rolled-back parked AMQP sibling; a stale parked row may be reconstructed on restart"
            );
        }
    }

    pub fn start_run(&mut self, sop_name: &str, event: SopEvent) -> Result<SopRunAction> {
        // A start is a two-phase operation: reserve the exec slot through the
        // authoritative store CAS (no side effect yet), then activate the reserved
        // slot into a live run and dispatch its first step. The phases are split so the
        // AMQP multi-match path can reserve the WHOLE matched batch before activating
        // any of it (see `dispatch`). A single start runs both phases back-to-back.
        let reservation = self.reserve_run_slot(sop_name)?;
        self.activate_reserved_run(reservation, event)
    }

    /// Phase 1 of a start: reserve `sop_name`'s exec slot through the authoritative
    /// store CAS WITHOUT creating an active run or dispatching any step — so no SOP
    /// side effect occurs yet. The returned `StartReservation` holds a live claim; the
    /// caller MUST either `activate_reserved_run` it or `release_reservation` it, or
    /// the slot leaks. This is the primitive behind the AMQP multi-match all-or-defer-
    /// all reservation: every matched SOP's capacity is held atomically before ANY of
    /// them produces a side effect, so a sibling engine grabbing a slot mid-batch can
    /// never leave a partial start (it makes one reservation fail → release-all +
    /// defer-all), only a safe requeue.
    pub(crate) fn reserve_run_slot(&mut self, sop_name: &str) -> Result<StartReservation> {
        self.enforce_admission(sop_name)?;

        let sop = self
            .get_sop(sop_name)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"sop_name": sop_name})),
                    "SOP engine: sop not found"
                );
                anyhow::Error::msg(format!("SOP not found: {sop_name}"))
            })?
            .clone();

        if !self.can_start(sop_name) {
            bail!(
                "Cannot start SOP '{}': cooldown or concurrency limit reached",
                sop_name
            );
        }

        if sop.steps.is_empty() {
            bail!("SOP '{}' has no steps defined", sop_name);
        }

        let deterministic = sop.execution_mode == SopExecutionMode::Deterministic;
        self.run_counter += 1;
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let epoch_ns = dur.as_nanos();
        let prefix = if deterministic { "det" } else { "run" };
        let run_id = format!("{prefix}-{epoch_ns}-{:04}", self.run_counter);
        let claim = self.claim_admission(&run_id, &sop)?;
        Ok(StartReservation {
            run_id,
            claim,
            sop,
            deterministic,
        })
    }

    /// Release a reservation that will NOT be activated (a batch that could not fully
    /// reserve), freeing its exec slot for admission. Best-effort + logged, exactly
    /// like a park release: a swallowed failure only lets the reaper collect the claim
    /// later — no run was ever created, so there is no side effect to unwind.
    pub(crate) fn release_reservation(&self, reservation: StartReservation) {
        self.release_claim_best_effort(&reservation.claim);
    }

    /// Phase 2 of a start: convert a held reservation into a live run — build the run
    /// record, insert it, and dispatch its first step, rolling the reservation back
    /// (release the claim, drop the run) if that dispatch fails.
    pub(crate) fn activate_reserved_run(
        &mut self,
        reservation: StartReservation,
        event: SopEvent,
    ) -> Result<SopRunAction> {
        let StartReservation {
            run_id,
            claim,
            sop,
            deterministic,
        } = reservation;

        let run = SopRun {
            run_id: run_id.clone(),
            sop_name: sop.name.clone(),
            trigger_event: event,
            frame_marker_id: new_marker_id(),
            status: SopRunStatus::Running,
            current_step: 1,
            total_steps: u32::try_from(sop.steps.len()).unwrap_or(u32::MAX),
            started_at: now_iso8601(),
            completed_at: None,
            failure_reason: None,
            step_results: Vec::new(),
            waiting_since: None,
            llm_calls_saved: 0,
            revision: 0,
            revision_base: 0,
        };
        let first_input = step_input_value(&run, 1);
        self.active_runs.insert(run_id.clone(), run);

        if deterministic {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                &format!(
                    "Deterministic SOP run {} started for '{}'",
                    run_id, sop.name
                )
            );
            match self.dispatch_deterministic_step(&run_id, &sop, 1, first_input) {
                Ok(action) => Ok(action),
                Err(e) => Err(self.rollback_failed_start(&run_id, &claim, e)),
            }
        } else {
            ::clawcrew_log::record!(
                INFO,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                &format!("SOP run {} started for '{}'", run_id, sop.name)
            );
            match self.dispatch_llm_step(&run_id, &sop, 1, None) {
                Ok(action) => Ok(action),
                Err(e) => Err(self.rollback_failed_start(&run_id, &claim, e)),
            }
        }
    }

    pub fn advance_step(&mut self, run_id: &str, result: SopStepResult) -> Result<SopRunAction> {
        let (sop_name, current_step_number) = {
            let run = self.active_runs.get(run_id).ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"run_id": run_id})),
                    "SOP engine: active run not found"
                );
                anyhow::Error::msg(format!("Active run not found: {run_id}"))
            })?;
            if matches!(
                run.status,
                SopRunStatus::WaitingApproval | SopRunStatus::PausedCheckpoint
            ) {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "run_id": run_id,
                            "status": run.status.to_string(),
                            "step": run.current_step,
                        })),
                    "SOP engine: advance_step rejected — run is paused at a gate"
                );
                bail!(
                    "Run {run_id} is paused at a {} gate; resolve the gate through \
                     `resolve_gate` (WaitingApproval) or `approve_step` (PausedCheckpoint) \
                     before advancing with sop_advance",
                    run.status
                );
            }
            (run.sop_name.clone(), run.current_step)
        };

        let sop = self
            .sops
            .iter()
            .find(|s| s.name == sop_name)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"sop_name": sop_name})),
                    "SOP engine: sop no longer loaded (definition removed mid-run)"
                );
                anyhow::Error::msg(format!("SOP '{sop_name}' no longer loaded"))
            })?
            .clone();

        let current_step = sop
            .steps
            .get((current_step_number.saturating_sub(1)) as usize)
            .cloned()
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(
                            ::serde_json::json!({"sop_name": sop_name, "step": current_step_number})
                        ),
                    "SOP engine: step no longer exists (definition changed mid-run)"
                );
                anyhow::Error::msg(format!(
                    "SOP '{sop_name}' step {current_step_number} no longer exists (definition changed mid-run)"
                ))
            })?;

        if self
            .active_runs
            .get(run_id)
            .is_some_and(|run| run.status == SopRunStatus::Pending)
            && pending_step_blocks_direct_advance(&sop, &current_step)
        {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "run_id": run_id,
                        "step": current_step.number,
                        "step_kind": current_step.kind.to_string(),
                    })),
                "SOP engine: advance_step rejected - pending run is blocked at a human gate"
            );
            bail!(
                "Run {run_id} is pending at gated step {}; wait for pending approval/checkpoint \
                 capacity and resolve the gate before advancing with sop_advance",
                current_step.number
            );
        }

        // Deterministic runs are driven through the dedicated piping path so the
        // same `sop_advance` tool advances every execution mode.
        if sop.execution_mode == SopExecutionMode::Deterministic {
            if result.status == SopStepStatus::Failed {
                self.record_step_result(run_id, result.clone())?;
                return self.route_recorded_step(
                    run_id,
                    &sop,
                    &current_step,
                    SopStepStatus::Failed,
                    true,
                    Some(retry_input_value(
                        self.active_runs.get(run_id).ok_or_else(|| {
                            anyhow::Error::msg(format!("Active run not found: {run_id}"))
                        })?,
                        current_step.number,
                    )),
                    Some(step_result_value(&result)),
                );
            }
            let piped = declared_step_output_value(&current_step, &result.output);
            return self.advance_deterministic_step(
                run_id,
                piped,
                Some((result.started_at.clone(), result.completed_at.clone())),
            );
        }

        let mut recorded = result.clone();
        if result.status == SopStepStatus::Completed {
            let output = declared_step_output_value(&current_step, &result.output);
            if let Err(reason) = self.validate_step_output(&current_step, &output) {
                let full_reason = format!(
                    "Step {} output schema validation failed: {reason}",
                    current_step.number
                );
                self.record_transition_event(
                    run_id,
                    "step_schema_reject",
                    Some(full_reason.clone()),
                    ::serde_json::json!({
                        "step": current_step.number,
                        "phase": "output",
                    }),
                );
                recorded.status = SopStepStatus::Failed;
                recorded.output = full_reason;
            } else if jsonish_value(&result.output) != output {
                // Canonicalize a schema-validated recovery at the model-output
                // boundary. Downstream piping, retry, replay, and persisted run
                // data re-parse the recorded text with the exact JSON-or-string
                // parser, so the record must hold whatever value validation
                // accepted — a fenced object recovered from prose and a
                // double-encoded object unwrapped from a JSON string both
                // differ from that re-parse until rewritten here.
                recorded.output = output.to_string();
            }
        }

        let retry_input = if recorded.status == SopStepStatus::Failed {
            Some(retry_input_value(
                self.active_runs
                    .get(run_id)
                    .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?,
                current_step.number,
            ))
        } else {
            None
        };

        self.record_step_result(run_id, recorded.clone())?;
        self.route_recorded_step(
            run_id,
            &sop,
            &current_step,
            recorded.status,
            false,
            retry_input,
            None,
        )
    }

    fn schema_input_failure_action(
        &mut self,
        run_id: &str,
        step: &SopStep,
        input: &Value,
    ) -> Result<Option<SopRunAction>> {
        self.schema_input_failure_reason(step, input)
            .map(|reason| self.fail_step_schema_validation(run_id, step.number, "input", reason))
            .transpose()
    }

    fn schema_input_failure_reason(&self, step: &SopStep, input: &Value) -> Option<String> {
        self.validate_step_input(step, input).err()
    }

    fn validate_step_input(&self, step: &SopStep, input: &Value) -> Result<(), String> {
        if !self.config.step_schema_enforce {
            return Ok(());
        }
        let Some(schema) = step
            .schema
            .as_ref()
            .and_then(|schema| schema.input.as_ref())
        else {
            return Ok(());
        };
        schema::validate_value(schema, input).map_err(|e| e.to_string())
    }

    fn validate_step_output(&self, step: &SopStep, output: &Value) -> Result<(), String> {
        if !self.config.step_schema_enforce {
            return Ok(());
        }
        let Some(schema) = step
            .schema
            .as_ref()
            .and_then(|schema| schema.output.as_ref())
        else {
            return Ok(());
        };
        schema::validate_value(schema, output).map_err(|e| e.to_string())
    }

    fn fail_step_schema_validation(
        &mut self,
        run_id: &str,
        step_number: u32,
        phase: &str,
        reason: String,
    ) -> Result<SopRunAction> {
        let reason = format!("Step {step_number} {phase} schema validation failed: {reason}");
        self.record_transition_event(
            run_id,
            "step_schema_reject",
            Some(reason.clone()),
            ::serde_json::json!({
                "step": step_number,
                "phase": phase,
            }),
        );
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                .with_outcome(::clawcrew_log::EventOutcome::Failure)
                .with_attrs(::serde_json::json!({
                    "run_id": run_id,
                    "step": step_number,
                    "phase": phase,
                    "reason": reason,
                })),
            "SOP step schema validation failed"
        );
        self.finish_run(run_id, SopRunStatus::Failed, Some(reason))
    }

    fn gate_schema_failure_transition(
        &self,
        run_id: &str,
        step_number: u32,
        phase: &'static str,
        reason: String,
    ) -> Result<GateClearTransition> {
        self.active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        let reason = format!("Step {step_number} {phase} schema validation failed: {reason}");
        Ok(GateClearTransition::Terminal {
            status: SopRunStatus::Failed,
            reason: Some(reason.clone()),
            follow_up: Some(GateResolutionFollowUp::StepSchemaReject {
                step: step_number,
                phase,
                reason,
            }),
        })
    }

    fn record_step_result(&mut self, run_id: &str, result: SopStepResult) -> Result<()> {
        let run = self.active_runs.get_mut(run_id).ok_or_else(|| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"run_id": run_id})),
                "SOP engine: active run not found"
            );
            anyhow::Error::msg(format!("Active run not found: {run_id}"))
        })?;
        run.step_results.push(result);
        Ok(())
    }

    fn route_recorded_step(
        &mut self,
        run_id: &str,
        sop: &Sop,
        current_step: &SopStep,
        last_status: SopStepStatus,
        deterministic: bool,
        retry_input: Option<Value>,
        routed_input: Option<Value>,
    ) -> Result<SopRunAction> {
        if let Some(action) = self.finish_requested_cancellation(run_id)? {
            return Ok(action);
        }
        let decision =
            self.route_decision_after_recorded_step(run_id, sop, current_step, last_status)?;
        self.apply_route_decision(
            run_id,
            sop,
            current_step.number,
            decision,
            deterministic,
            retry_input,
            routed_input,
        )
    }

    fn route_decision_after_recorded_step(
        &self,
        run_id: &str,
        sop: &Sop,
        current_step: &SopStep,
        last_status: SopStepStatus,
    ) -> Result<NextStep> {
        let run = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;

        if last_status == SopStepStatus::Failed {
            let failed_executions = run
                .step_results
                .iter()
                .filter(|result| {
                    result.step_number == current_step.number
                        && result.status == SopStepStatus::Failed
                })
                .count()
                .try_into()
                .unwrap_or(u32::MAX);
            let retries_consumed = failed_executions.saturating_sub(1);
            let decision = route::failure::route_failure(
                &current_step.on_failure,
                retries_consumed,
                self.config.max_step_retries,
            );
            return Ok(match decision {
                NextStep::Fail(reason) if reason == "step failed" => {
                    let detail = run
                        .step_results
                        .iter()
                        .rev()
                        .find(|result| {
                            result.step_number == current_step.number
                                && result.status == SopStepStatus::Failed
                        })
                        .map(|result| result.output.as_str())
                        .unwrap_or("step failed");
                    NextStep::Fail(format!("Step {} failed: {detail}", current_step.number))
                }
                other => other,
            });
        }

        let run_data = RunData::from_step_results(&run.step_results);
        Ok(route::resolve_next(&RouteCtx {
            sop,
            run,
            run_data: &run_data,
            last_status,
            max_step_visits: self.config.max_step_visits,
        }))
    }

    fn apply_route_decision(
        &mut self,
        run_id: &str,
        sop: &Sop,
        current_step_number: u32,
        decision: NextStep,
        deterministic: bool,
        retry_input: Option<Value>,
        routed_input: Option<Value>,
    ) -> Result<SopRunAction> {
        match decision {
            NextStep::Step(step_number) => {
                if let Some(action) = self.visit_bound_failure(run_id, step_number)? {
                    return Ok(action);
                }
                self.record_transition_event(
                    run_id,
                    "step_promoted",
                    None,
                    ::serde_json::json!({
                        "from_step": current_step_number,
                        "to_step": step_number,
                    }),
                );
                if deterministic {
                    let input = routed_input.unwrap_or_default();
                    self.dispatch_deterministic_step(run_id, sop, step_number, input)
                } else {
                    self.dispatch_llm_step(run_id, sop, step_number, None)
                }
            }
            NextStep::Retry => {
                if let Some(action) = self.visit_bound_failure(run_id, current_step_number)? {
                    return Ok(action);
                }
                self.record_transition_event(
                    run_id,
                    "step_retry",
                    None,
                    ::serde_json::json!({
                        "step": current_step_number,
                    }),
                );
                if deterministic {
                    self.dispatch_deterministic_step(
                        run_id,
                        sop,
                        current_step_number,
                        retry_input.unwrap_or_default(),
                    )
                } else {
                    self.dispatch_llm_step(run_id, sop, current_step_number, retry_input)
                }
            }
            NextStep::Complete => {
                if deterministic {
                    self.finish_deterministic_run(run_id)
                } else {
                    ::clawcrew_log::record!(
                        INFO,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                            .with_attrs(::serde_json::json!({"run_id": run_id})),
                        "SOP run completed successfully"
                    );
                    self.finish_run(run_id, SopRunStatus::Completed, None)
                }
            }
            NextStep::Fail(reason) => self.finish_run(run_id, SopRunStatus::Failed, Some(reason)),
            NextStep::Wait(step_number) => Ok(self.mark_step_pending(
                run_id,
                sop,
                step_number,
                format!("step {step_number} dependencies not satisfied"),
            )),
        }
    }

    fn visit_bound_failure(
        &mut self,
        run_id: &str,
        step_number: u32,
    ) -> Result<Option<SopRunAction>> {
        let run = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        if route::guard::within_visit_bound(run, step_number, self.config.max_step_visits) {
            return Ok(None);
        }

        Ok(Some(self.finish_run(
            run_id,
            SopRunStatus::Failed,
            Some(format!("step {step_number} visit limit reached")),
        )?))
    }

    fn dispatch_llm_step(
        &mut self,
        run_id: &str,
        sop: &Sop,
        step_number: u32,
        input_override: Option<Value>,
    ) -> Result<SopRunAction> {
        let step = self.resolve_sop_step(sop, step_number)?;
        if let Some(action) = self.visit_bound_failure(run_id, step_number)? {
            return Ok(action);
        }

        if let Some(run) = self.active_runs.get_mut(run_id) {
            run.current_step = step_number;
            run.status = SopRunStatus::Running;
            run.waiting_since = None;
        }

        let run_data = {
            let run = self
                .active_runs
                .get(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            RunData::from_step_results(&run.step_results)
        };
        if !route::eligible(&step, &run_data) {
            return Ok(self.mark_step_pending(
                run_id,
                sop,
                step.number,
                format!("step {} dependencies not satisfied", step.number),
            ));
        }

        let input = match input_override {
            Some(input) => input,
            None => {
                let run = self
                    .active_runs
                    .get(run_id)
                    .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
                step_input_value(run, step.number)
            }
        };
        if let Some(action) = self.schema_input_failure_action(run_id, &step, &input)? {
            return Ok(action);
        }

        let context = {
            let run = self
                .active_runs
                .get(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            format_step_context(sop, run, &step, &self.config)
        };
        // Upstream's resolve_step_action now forces approval whenever the
        // SOP-level mode needs it (strictly stronger than the old
        // approval_mode-conditional escalation), so the mode param is gone.
        let action = resolve_step_action(sop, &step, run_id.to_string(), context);
        let parked_for_approval = matches!(action, SopRunAction::WaitApproval { .. });
        let has_prior_gate_presentation = parked_for_approval
            && self.run_events(run_id).is_ok_and(|events| {
                events.iter().any(|event| {
                    matches!(
                        event.kind.as_str(),
                        "gate_vote" | "gate_resolved" | "gate_escalated" | "gate_timed_out"
                    )
                })
            });

        // A1: free the exec slot while the run waits on a human - but only AFTER
        // the parked snapshot is durably persisted (else keep the claim, fail
        // closed).
        if parked_for_approval {
            if let Some(reason) = self.pending_pool_full_reason(sop) {
                Self::log_pending_capacity_full(run_id, &reason);
                return Ok(self.mark_step_pending(run_id, sop, step.number, reason));
            }
            if let Some(run) = self.active_runs.get_mut(run_id) {
                run.status = SopRunStatus::WaitingApproval;
                run.waiting_since = Some(now_iso8601());
                if run.revision > 0 || has_prior_gate_presentation {
                    run.revision += 1;
                }
            }
            match self.persist_parked_snapshot_then_release_claim(run_id) {
                // Deliver only after the parked snapshot is durable. A failed persist
                // keeps the claim and the maintenance retry issues the notice later.
                ParkPersistOutcome::Released => self.notify_park_request(run_id),
                ParkPersistOutcome::CapacityFull => {
                    let reason = self.pending_pool_capacity_raced_reason(sop);
                    Self::log_pending_capacity_full(run_id, &reason);
                    return Ok(self.mark_step_pending(run_id, sop, step.number, reason));
                }
                ParkPersistOutcome::PersistFailed => {
                    let reason =
                        format!("SOP '{}' park snapshot not yet durably persisted", sop.name);
                    return Ok(SopRunAction::Pending {
                        run_id: run_id.to_string(),
                        sop_name: sop.name.clone(),
                        step: step.number,
                        reason,
                    });
                }
            }
        } else {
            self.persist_active(run_id);
        }
        Ok(action)
    }

    /// Deliver the initial approval-request notice for a run that just parked at a
    /// policied gate, if that policy names a `request_route`. Best-effort: a run
    /// with no policy, a policy with no request route, or a delivery error all leave
    /// the (already-parked, already-durable) gate untouched.
    fn notify_park_request(&self, run_id: &str) {
        let Some(run) = self.get_run(run_id) else {
            return;
        };
        let (sop_name, step, revision) = (run.sop_name.clone(), run.current_step, run.revision);
        // Edit/Revise resolve ONLY through the deterministic-checkpoint path
        // (`resolve_checkpoint`); a broker-owned approval gate refuses them
        // fail-closed. Offering the choices on a non-checkpoint park would
        // render buttons whose submissions are always rejected — the operator's
        // typed text silently lost behind a success-looking ack.
        let is_checkpoint = run.status == SopRunStatus::PausedCheckpoint;
        // The notice carries WHAT is being approved: the parked step's piped
        // input (trigger payload at step 1, previous step's output later) plus
        // the step's authored `- prompt:` template when it has one.
        let context = step_input_value(run, step);
        let step_def = self
            .resolve_active_run_sop(run_id)
            .ok()
            .and_then(|(_, sop)| self.resolve_sop_step(&sop, step).ok());
        let gate_prompt = step_def.as_ref().and_then(|s| s.gate_prompt.clone());
        // Input-bearing choices: Edit needs the step's `- edit:` declaration;
        // Revise needs an llm.generate predecessor and headroom under the cap.
        let edit_field = step_def
            .as_ref()
            .filter(|_| is_checkpoint)
            .and_then(|s| s.edit.as_deref())
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(str::to_string);
        let can_revise = is_checkpoint
            && revision.saturating_sub(run.revision_base) < MAX_GATE_REVISIONS
            && self.revisable_predecessor(run_id).is_some();
        let Some(policy_name) = self.current_step_policy_name(run_id) else {
            return;
        };
        let broker = self.approval_broker();
        if let Some(route) = broker.request_route(self.approval_config(), &policy_name) {
            broker.deliver_request(
                &route,
                &super::approval::GateNotice {
                    run_id,
                    sop_name: &sop_name,
                    step,
                    context: &context,
                    gate_prompt: gate_prompt.as_deref(),
                    revision,
                    edit_field: edit_field.as_deref(),
                    can_revise,
                },
            );
        }
    }

    fn dispatch_deterministic_step(
        &mut self,
        run_id: &str,
        sop: &Sop,
        step_number: u32,
        input: Value,
    ) -> Result<SopRunAction> {
        let step = self.resolve_sop_step(sop, step_number)?;
        if let Some(action) = self.visit_bound_failure(run_id, step_number)? {
            return Ok(action);
        }

        if let Some(run) = self.active_runs.get_mut(run_id) {
            run.current_step = step_number;
            run.status = SopRunStatus::Running;
            run.waiting_since = None;
        }

        self.resolve_deterministic_action(sop, run_id, &step, input)
    }

    fn resolve_sop_step(&self, sop: &Sop, step_number: u32) -> Result<SopStep> {
        sop.steps
            .iter()
            .find(|step| step.number == step_number)
            .cloned()
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(
                            ::serde_json::json!({"sop_name": sop.name, "step": step_number})
                        ),
                    "SOP engine: step no longer exists (definition changed mid-run)"
                );
                anyhow::Error::msg(format!(
                    "SOP '{}' step {step_number} no longer exists (definition changed mid-run)",
                    sop.name
                ))
            })
    }

    fn mark_step_pending(
        &mut self,
        run_id: &str,
        sop: &Sop,
        step_number: u32,
        reason: String,
    ) -> SopRunAction {
        self.mark_step_pending_with_persist(run_id, sop, step_number, reason, true)
    }

    fn mark_step_pending_with_persist(
        &mut self,
        run_id: &str,
        sop: &Sop,
        step_number: u32,
        reason: String,
        persist: bool,
    ) -> SopRunAction {
        let now = now_iso8601();
        if let Some(run) = self.active_runs.get_mut(run_id) {
            run.current_step = step_number;
            run.status = SopRunStatus::Pending;
            run.waiting_since = Some(now.clone());
            let last_is_same_skip = run.step_results.last().is_some_and(|result| {
                result.step_number == step_number && result.status == SopStepStatus::Skipped
            });
            if !last_is_same_skip {
                run.step_results.push(SopStepResult {
                    step_number,
                    status: SopStepStatus::Skipped,
                    output: reason.clone(),
                    started_at: now.clone(),
                    completed_at: Some(now.clone()),
                    effective_agent: None,
                    tool_calls: Vec::new(),
                });
            }
        }
        ::clawcrew_log::record!(
            WARN,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                .with_attrs(::serde_json::json!({
                    "run_id": run_id,
                    "sop_name": sop.name,
                    "step": step_number,
                    "reason": reason,
                })),
            "SOP run pending on step dependencies"
        );
        self.record_transition_event(
            run_id,
            "step_skipped",
            Some(reason.clone()),
            ::serde_json::json!({
                "step": step_number,
                "status": "pending",
            }),
        );
        if persist {
            self.persist_active(run_id);
        }
        SopRunAction::Pending {
            run_id: run_id.to_string(),
            sop_name: sop.name.clone(),
            step: step_number,
            reason,
        }
    }

    fn gate_step_pending_transition(
        &mut self,
        run_id: &str,
        sop: &Sop,
        step_number: u32,
        reason: String,
    ) -> Result<GateClearTransition> {
        let now = now_iso8601();
        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        run.current_step = step_number;
        run.status = SopRunStatus::Pending;
        run.waiting_since = Some(now.clone());
        let last_is_same_skip = run.step_results.last().is_some_and(|result| {
            result.step_number == step_number && result.status == SopStepStatus::Skipped
        });
        if !last_is_same_skip {
            run.step_results.push(SopStepResult {
                step_number,
                status: SopStepStatus::Skipped,
                output: reason.clone(),
                started_at: now.clone(),
                completed_at: Some(now),
                effective_agent: None,
                tool_calls: Vec::new(),
            });
        }

        Ok(GateClearTransition::Active {
            action: Box::new(SopRunAction::Pending {
                run_id: run_id.to_string(),
                sop_name: sop.name.clone(),
                step: step_number,
                reason: reason.clone(),
            }),
            follow_up: Some(GateResolutionFollowUp::StepSkipped {
                sop_name: sop.name.clone(),
                step: step_number,
                reason,
            }),
        })
    }

    fn record_gate_resolution_follow_up(&self, run_id: &str, follow_up: GateResolutionFollowUp) {
        match follow_up {
            GateResolutionFollowUp::StepSchemaReject {
                step,
                phase,
                reason,
            } => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "run_id": run_id,
                            "step": step,
                            "phase": phase,
                            "reason": reason.as_str(),
                        })),
                    "SOP step schema validation failed"
                );
                self.record_transition_event(
                    run_id,
                    "step_schema_reject",
                    Some(reason),
                    ::serde_json::json!({
                        "step": step,
                        "phase": phase,
                    }),
                );
            }
            GateResolutionFollowUp::StepSkipped {
                sop_name,
                step,
                reason,
            } => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({
                            "run_id": run_id,
                            "sop_name": sop_name,
                            "step": step,
                            "reason": reason.as_str(),
                        })),
                    "SOP run pending on step dependencies"
                );
                self.record_transition_event(
                    run_id,
                    "step_skipped",
                    Some(reason),
                    ::serde_json::json!({
                        "step": step,
                        "status": "pending",
                    }),
                );
            }
        }
    }

    fn finish_deterministic_run(&mut self, run_id: &str) -> Result<SopRunAction> {
        let saved = self
            .active_runs
            .get(run_id)
            .map(|run| run.llm_calls_saved)
            .unwrap_or(0);
        let action = self.finish_run(run_id, SopRunStatus::Completed, None)?;
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
            &format!("Deterministic SOP run {run_id} completed ({saved} LLM calls saved)")
        );
        self.deterministic_savings.total_llm_calls_saved += saved;
        self.deterministic_savings.total_runs += 1;
        Ok(action)
    }

    /// Cancel an active run immediately without an operator-supplied reason.
    ///
    /// Internal callers use this only when no step is executing concurrently.
    /// Remotely initiated cancellation must use `cancel_run_idempotent`, which
    /// requests cancellation for a running step and preserves its claim until
    /// the driver reaches a boundary.
    pub fn cancel_run(&mut self, run_id: &str) -> Result<()> {
        self.cancel_run_with_reason_and_actor(run_id, None, None)
    }

    fn cancel_run_with_reason_and_actor(
        &mut self,
        run_id: &str,
        reason: Option<String>,
        actor: Option<String>,
    ) -> Result<()> {
        let current_step = self
            .active_runs
            .get(run_id)
            .map(|run| run.current_step)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        let event = SopEventRecord {
            run_id: run_id.to_string(),
            seq: 0,
            ts: now_iso8601(),
            kind: "run_cancelled".to_string(),
            actor,
            reason: reason.clone(),
            payload: ::serde_json::json!({ "step": current_step }),
        };
        self.finish_run_with_gate_event(run_id, SopRunStatus::Cancelled, reason, &event)?;
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"run_id": run_id})),
            "SOP run cancelled"
        );
        Ok(())
    }

    fn request_run_cancellation(
        &mut self,
        run_id: &str,
        reason: Option<String>,
        actor: Option<String>,
    ) -> Result<()> {
        let prior_run = self
            .active_runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        let current_step = prior_run.current_step;
        let event = SopEventRecord {
            run_id: run_id.to_string(),
            seq: 0,
            ts: now_iso8601(),
            kind: "run_cancel_requested".to_string(),
            actor,
            reason,
            payload: ::serde_json::json!({ "step": current_step }),
        };
        if let Some(run) = self.active_runs.get_mut(run_id) {
            run.status = SopRunStatus::CancelRequested;
        }
        if let Err(source) = self.persist_active_with_gate_event(run_id, &event) {
            self.active_runs.insert(run_id.to_string(), prior_run);
            return Err(anyhow::Error::new(CancellationRequestPersistenceRetained {
                run_id: run_id.to_string(),
                source,
            }));
        }
        ::clawcrew_log::record!(
            INFO,
            ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                .with_attrs(::serde_json::json!({"run_id": run_id})),
            "SOP run cancellation requested"
        );
        Ok(())
    }

    /// Finish a previously requested cancellation at a driver-owned execution
    /// boundary. Actor and reason are resolved from the durable request event,
    /// avoiding a second in-memory source of truth.
    pub fn finish_requested_cancellation(&mut self, run_id: &str) -> Result<Option<SopRunAction>> {
        if self
            .active_runs
            .get(run_id)
            .is_none_or(|run| run.status != SopRunStatus::CancelRequested)
        {
            self.cancellation_finalization_ready.remove(run_id);
            return Ok(None);
        }
        self.cancellation_finalization_ready
            .insert(run_id.to_string());
        let request = self
            .run_events(run_id)?
            .into_iter()
            .rev()
            .find(|event| event.kind == "run_cancel_requested")
            .ok_or_else(|| {
                anyhow::Error::msg(format!(
                    "run {run_id} is cancel_requested but has no durable request event"
                ))
            })?;
        let mut last_error = None;
        for _ in 0..CANCELLATION_FINALIZE_ATTEMPTS {
            match self.cancel_run_with_reason_and_actor(
                run_id,
                request.reason.clone(),
                request.actor.clone(),
            ) {
                Ok(()) => {
                    last_error = None;
                    break;
                }
                Err(e) => last_error = Some(e),
            }
        }
        if let Some(e) = last_error {
            return Err(e);
        }
        Ok(Some(
            self.get_run(run_id)
                .map(|run| SopRunAction::Cancelled {
                    run_id: run.run_id.clone(),
                    sop_name: run.sop_name.clone(),
                })
                .ok_or_else(|| {
                    anyhow::Error::msg(format!("Run {run_id} not found after cancel"))
                })?,
        ))
    }

    /// Cancel a run by id, idempotently. Classifies the run under a single
    /// borrow of engine state (no intervening lookup that a concurrent
    /// completion could race) and reports:
    /// - `Ok(Some(Cancelled))` - the run was active and is now `Cancelled`.
    /// - `Ok(Some(AlreadyTerminal(status)))` - the run had already reached a
    ///   terminal status (a repeat cancel, or it finished normally first);
    ///   no second cancellation or audit event is recorded.
    /// - `Ok(None)` - no run with this id is known to the engine.
    /// - `Err(_)` - the terminal persist failed; see
    ///   `err_is_terminal_persistence_retained` to tell a retryable store
    ///   fault (the run stays active and claimed) from any other error.
    pub fn cancel_run_idempotent(
        &mut self,
        run_id: &str,
        reason: Option<String>,
        actor: Option<String>,
    ) -> Result<Option<CancelOutcome>> {
        let Some(status) = self.active_runs.get(run_id).map(|run| run.status) else {
            return Ok(self
                .get_run(run_id)
                .map(|run| CancelOutcome::AlreadyTerminal(run.status)));
        };
        match status {
            SopRunStatus::CancelRequested => Ok(Some(CancelOutcome::AlreadyRequested)),
            SopRunStatus::Running => {
                self.request_run_cancellation(run_id, reason, actor)?;
                Ok(Some(CancelOutcome::Requested))
            }
            SopRunStatus::Pending
            | SopRunStatus::WaitingApproval
            | SopRunStatus::PausedCheckpoint => {
                self.cancel_run_with_reason_and_actor(run_id, reason, actor)?;
                Ok(Some(CancelOutcome::Cancelled))
            }
            SopRunStatus::Completed | SopRunStatus::Failed | SopRunStatus::Cancelled => {
                Ok(Some(CancelOutcome::AlreadyTerminal(status)))
            }
        }
    }

    pub fn approve_step(&mut self, run_id: &str) -> Result<SopRunAction> {
        self.resume_checkpoint(run_id, None)
    }

    /// Resume a run paused at a deterministic checkpoint, optionally amending one
    /// field of the piped value first (`amend = (field, text)`, the operator-edited
    /// draft). The amended value becomes the checkpoint's recorded output, so the
    /// human-approved text flows downstream while the predecessor step keeps the
    /// model's original.
    fn resume_checkpoint(
        &mut self,
        run_id: &str,
        amend: Option<(String, String)>,
    ) -> Result<SopRunAction> {
        self.resume_checkpoint_inner(run_id, amend, false)
    }

    fn resume_checkpoint_inner(
        &mut self,
        run_id: &str,
        amend: Option<(String, String)>,
        claim_already_reacquired: bool,
    ) -> Result<SopRunAction> {
        let status = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"run_id": run_id})),
                    "SOP engine: active run not found"
                );
                anyhow::Error::msg(format!("Active run not found: {run_id}"))
            })?
            .status;

        if status != SopRunStatus::PausedCheckpoint {
            bail!("Run {run_id} is not paused at a checkpoint (status: {status})");
        }

        // Refuse to resume while the checkpoint's parked snapshot has not yet
        // been durably persisted (see `is_park_persist_pending`'s doc): the kept
        // claim predates this attempt, and reacquiring on top of it would give a
        // later rollback or a maintenance retry no way to distinguish "freshly
        // reacquired" from "pre-existing, must survive."
        if self.is_park_persist_pending(run_id) {
            bail!(
                "Run {run_id} cannot resume: its parked checkpoint snapshot is not yet durably persisted (retrying)"
            );
        }

        // Pre-flight the same SOP/step lookups `advance_deterministic_step` performs
        // BEFORE reacquiring the claim or mutating the run: a definition removed or
        // shrunk while parked must fail closed with the run left at
        // `PausedCheckpoint` (re-resolvable), not stranded in `Running` holding a
        // claim it can never advance.
        self.can_advance_deterministic_step(run_id)?;

        // A1: fail-closed - re-acquire the exec claim released when this run parked
        // BEFORE flipping it to Running; if it cannot, abort and leave the run paused
        // (re-resolvable) rather than execute uncounted.
        if !claim_already_reacquired {
            self.reacquire_claim_on_resume(run_id)?;
        }
        // A deterministic run paused at a checkpoint resumes through the
        // deterministic piping path: the checkpoint step is recorded as
        // completed and its input (the previous step's output — or, for a
        // checkpoint parked at step 1, the trigger payload) is piped forward.
        // Same step-1 mapping as `step_input_value`; `.last()` alone starved an
        // intake-gate pipeline (checkpoint BEFORE the first work step) of its
        // trigger payload.
        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        let mut piped = step_input_value(run, run.current_step);
        // Operator amendment: replace the declared editable field BEFORE any run
        // mutation, so a non-amendable input (pre-flighted by
        // `can_amend_checkpoint`, so defensive here) leaves the run parked.
        if let Some((field, text)) = amend {
            match piped.as_object_mut() {
                Some(map) => {
                    map.insert(field, serde_json::Value::String(text));
                }
                None => {
                    self.release_claim_on_park(run_id);
                    bail!(
                        "Run {run_id} checkpoint input is not a JSON object; \
                         cannot amend field '{field}'"
                    );
                }
            }
        }
        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        let prior_waiting_since = run.waiting_since.clone();
        run.status = SopRunStatus::Running;
        run.waiting_since = None;
        match self.advance_deterministic_step(run_id, piped, None) {
            Ok(action) => self.drive_inline_capability_tail(run_id, action),
            Err(e) => {
                // Defensive: the pre-flight above validated the same lookups under
                // this lock, so this is unreachable in practice. If the advance
                // still fails, roll the run back to `PausedCheckpoint` and release
                // the just-reacquired claim so a run that made no progress does not
                // get stuck in `Running` holding a leaked exec slot.
                if let Some(run) = self.active_runs.get_mut(run_id) {
                    run.status = SopRunStatus::PausedCheckpoint;
                    run.waiting_since = prior_waiting_since;
                }
                self.release_claim_on_park(run_id);
                Err(e)
            }
        }
    }

    /// The `- edit:` field the run's current checkpoint step declares, or why an
    /// amend cannot apply. Resolved under the engine lock at resolution time, so
    /// the field an operator edits is always the step's live declaration.
    fn checkpoint_edit_field(&self, run_id: &str) -> Result<String> {
        let (_, sop) = self.resolve_active_run_sop(run_id)?;
        let current_step = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?
            .current_step;
        let step = self.resolve_sop_step(&sop, current_step)?;
        step.edit
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow::Error::msg(format!(
                    "SOP '{}' step {current_step} does not declare an editable field \
                     (`- edit:`); an amend cannot apply",
                    sop.name
                ))
            })
    }

    /// Pre-flight an `Amend` WITHOUT mutating anything: the step must declare an
    /// editable field and the checkpoint's piped value must be a JSON object the
    /// field can replace into.
    fn can_amend_checkpoint(&self, run_id: &str) -> Result<()> {
        self.checkpoint_edit_field(run_id)?;
        let run = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        if !step_input_value(run, run.current_step).is_object() {
            bail!(
                "Run {run_id} checkpoint input is not a JSON object; \
                 there is no field an amend could replace"
            );
        }
        Ok(())
    }

    /// The step a `Revise` would re-run: the last COMPLETED step before the
    /// checkpoint, but only when it is an `llm.generate` capability (the only
    /// step kind a re-draft is meaningful for). `None` = this gate is not
    /// revisable.
    fn revisable_predecessor(&self, run_id: &str) -> Option<u32> {
        let run = self.get_run(run_id)?;
        let pred = run
            .step_results
            .iter()
            .rev()
            .find(|r| r.status == SopStepStatus::Completed && r.step_number < run.current_step)?
            .step_number;
        let (_, sop) = self.resolve_active_run_sop(run_id).ok()?;
        let step = self.resolve_sop_step(&sop, pred).ok()?;
        (step.kind == SopStepKind::Capability && step.capability_id() == Some("llm.generate"))
            .then_some(pred)
    }

    /// Pre-flight a `Revise` WITHOUT mutating anything: the revision cap has not
    /// been reached and the gate has an `llm.generate` predecessor to re-run.
    fn can_revise_checkpoint(&self, run_id: &str) -> Result<()> {
        let run = self
            .get_run(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        // Per-GATE budget: presentations spent at THIS gate, not run-wide
        // (`revision` also advances when a later gate first parks).
        if run.revision.saturating_sub(run.revision_base) >= MAX_GATE_REVISIONS {
            bail!(
                "Run {run_id} has reached this gate's revision limit ({MAX_GATE_REVISIONS}); \
                 approve, edit, or deny the current draft"
            );
        }
        if self.revisable_predecessor(run_id).is_none() {
            bail!(
                "Run {run_id} has no llm.generate predecessor step to re-run; \
                 this gate is not revisable"
            );
        }
        Ok(())
    }

    /// Re-run the checkpoint's predecessor `llm.generate` step with the operator's
    /// guidance framed as reviewer feedback, replace the recorded draft, bump the
    /// gate revision, and re-present the gate. The run never leaves
    /// `PausedCheckpoint`: a failed re-draft keeps the OLD draft parked and
    /// answerable. The caller commits the new snapshot and ledger event together.
    /// The model call blocks under the engine lock — the same tradeoff as a normal
    /// `llm.generate` step.
    fn revise_checkpoint_draft(&mut self, run_id: &str, guidance: &str) -> Result<()> {
        let (_, sop) = self.resolve_active_run_sop(run_id)?;
        let pred_number = self.revisable_predecessor(run_id).ok_or_else(|| {
            anyhow::Error::msg(format!(
                "Run {run_id} has no llm.generate predecessor step to re-run"
            ))
        })?;
        let pred_step = self.resolve_sop_step(&sop, pred_number)?;
        let piped = {
            let run = self
                .get_run(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            replay_input_for_step(run, pred_number)
        };

        // The guidance rides in the step's STATIC config plane (alongside the
        // authored instruction), NOT the untrusted payload frame: it comes from
        // an authenticated approver, and it must be able to steer the redraft.
        let mut step = pred_step.clone();
        let mut configured = step
            .capability_input
            .take()
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(object) = configured.as_object_mut() {
            object.insert(
                "revision_feedback".to_string(),
                serde_json::Value::String(guidance.to_string()),
            );
        }
        step.capability_input = Some(configured);

        // The re-draft is real work: hold an exec slot for its duration (the run
        // released its slot when it parked).
        self.reacquire_claim_on_resume(run_id)?;
        let ctx = super::capability::CapabilityContext {
            run_id: run_id.to_string(),
            sop_name: sop.name.clone(),
            step_number: pred_number,
            sop_location: sop.location.clone(),
        };
        let result = self.capabilities.execute_step(ctx, &step, piped);
        self.metrics.record_capability_executed(&sop.name);

        let output = match result {
            Ok(r) if r.success => match self.validate_step_output(&pred_step, &r.output) {
                Ok(()) => r.output,
                Err(reason) => {
                    self.release_claim_on_park(run_id);
                    bail!(
                        "Run {run_id} revised draft failed step {pred_number}'s output \
                         schema (previous draft kept): {reason}"
                    );
                }
            },
            Ok(r) => {
                self.release_claim_on_park(run_id);
                bail!(
                    "Run {run_id} re-draft failed (previous draft kept): {}",
                    r.error
                        .unwrap_or_else(|| "capability returned failure".to_string())
                );
            }
            Err(e) => {
                self.release_claim_on_park(run_id);
                bail!("Run {run_id} re-draft failed (previous draft kept): {e}");
            }
        };

        {
            let run = self
                .active_runs
                .get_mut(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            if let Some(recorded) = run
                .step_results
                .iter_mut()
                .rev()
                .find(|r| r.step_number == pred_number && r.status == SopStepStatus::Completed)
            {
                recorded.output = output.to_string();
                recorded.completed_at = Some(now_iso8601());
            }
            run.revision += 1;
            run.waiting_since = Some(now_iso8601());
        }

        Ok(())
    }

    /// Apply a revise decision while preserving the current store contract: the
    /// new parked draft and its gate-resolution event commit together, or the
    /// in-memory run rolls back to the previous answerable draft.
    fn revise_checkpoint_with_principal(
        &mut self,
        run_id: &str,
        guidance: &str,
        decision: super::approval::ApprovalDecision,
        principal: super::approval::ApprovalPrincipal,
    ) -> Result<()> {
        let prior_run = self
            .active_runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        if prior_run.status != SopRunStatus::PausedCheckpoint {
            bail!(
                "Run {run_id} is not paused at a checkpoint (status: {})",
                prior_run.status
            );
        }
        if self.is_park_persist_pending(run_id) {
            bail!(
                "Run {run_id} cannot re-draft: its parked checkpoint snapshot is not yet \
                 durably persisted (retrying)"
            );
        }
        self.can_revise_checkpoint(run_id)?;
        let (_, sop) = self.resolve_active_run_sop(run_id)?;
        self.revise_checkpoint_draft(run_id, guidance)?;

        let event = super::approval::GateLedgerEntry {
            run_id: run_id.to_string(),
            step: prior_run.current_step,
            gate_revision: Some(prior_run.revision),
            checkpoint_revision: Some(prior_run.revision),
            decision_identity: super::approval::broker::checkpoint_decision_identity(&decision)
                .map(|(_, identity)| identity),
            kind: super::approval::GateEventKind::Resolved,
            decision: Some(decision),
            principal,
            ts: now_iso8601(),
        }
        .into_event_record();
        if let Err(e) = self.persist_active_with_gate_event(run_id, &event) {
            self.active_runs.insert(run_id.to_string(), prior_run);
            self.release_claim_on_park(run_id);
            return Err(e);
        }

        // The run store is authoritative. Refresh the rehydration artifact after
        // the atomic store write, then release the temporary execution claim and
        // present the versioned replacement prompt.
        if let Err(e) = self.persist_deterministic_state(run_id, &sop, true) {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"run_id": run_id, "error": e.to_string()})),
                "SOP engine: revised state-file refresh failed (run store remains authoritative)"
            );
        }
        self.release_claim_on_park(run_id);
        self.notify_park_request(run_id);
        Ok(())
    }

    /// Pre-flight ONLY the fallible SOP/step lookups that
    /// `advance_deterministic_step` performs for `run_id`'s current step, WITHOUT
    /// reacquiring a claim, mutating the run, or persisting anything.
    ///
    /// `approve_step` calls this BEFORE it reacquires the exec claim and flips the
    /// run to `Running`, so a checkpoint resume whose SOP was removed or shrunk
    /// while parked fails closed here - with the run left untouched at
    /// `PausedCheckpoint` - instead of after the mutation, which would otherwise
    /// strand the run in `Running`, holding a claim, with no way to make progress.
    pub(crate) fn can_advance_deterministic_step(&self, run_id: &str) -> Result<()> {
        let (_, sop) = self.resolve_active_run_sop(run_id)?;
        let current_step = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?
            .current_step;
        self.resolve_sop_step(&sop, current_step)?;
        Ok(())
    }

    /// Pre-flight ONLY the fallible lookups that `clear_waiting_gate` performs
    /// (the SOP is still loaded and the waiting step still resolves by number),
    /// WITHOUT reacquiring a claim, mutating the run, or persisting anything.
    ///
    /// `resolve_gate` calls this BEFORE it reacquires the exec claim and appends
    /// the immutable `gate_resolved` ledger row, so a run whose SOP was removed or
    /// shrunk while it sat parked fails closed here - with no claim reacquired and
    /// no false "resolved" audit row - instead of after the ledger append, which
    /// would otherwise leave a durable `gate_resolved` row for a still-waiting gate
    /// AND leak the reacquired exec slot. Runs under the engine mutex, so the
    /// lookups it validates cannot change before `clear_waiting_gate` re-runs them.
    pub(crate) fn can_clear_waiting_gate(&self, run_id: &str) -> Result<()> {
        let (sop_name, current_step) = {
            let run = self
                .active_runs
                .get(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            (run.sop_name.clone(), run.current_step)
        };
        let sop = self
            .sops
            .iter()
            .find(|s| s.name == sop_name)
            .ok_or_else(|| anyhow::Error::msg(format!("SOP '{sop_name}' no longer loaded")))?;
        self.resolve_sop_step(sop, current_step)?;
        Ok(())
    }

    /// Resolve a checkpoint decision (`PausedCheckpoint`). `Approve` resumes the
    /// success path (records the checkpoint `Completed`, pipes forward down
    /// `routing.next`); `Deny` takes the failure path (records the checkpoint
    /// `Failed` and routes through the step's `on_failure`, exactly like a step
    /// that failed execution). This is the single entry point for both outcomes;
    /// callers never branch on status. `approve_step` is the `Approve`-only alias.
    pub fn decide_checkpoint(
        &mut self,
        run_id: &str,
        decision: super::approval::ApprovalDecision,
    ) -> Result<SopRunAction> {
        match decision {
            super::approval::ApprovalDecision::Approve => self.approve_step(run_id),
            super::approval::ApprovalDecision::Deny { reason } => {
                self.deny_checkpoint(run_id, reason)
            }
            super::approval::ApprovalDecision::Amend { .. }
            | super::approval::ApprovalDecision::Revise { .. } => {
                bail!(
                    "checkpoint edit and revise decisions must resolve through the approval broker"
                )
            }
        }
    }

    /// Apply a broker-authorized checkpoint decision and persist the resulting run
    /// state together with the approver audit row. The run store is the durable
    /// source of truth for both surfaces, so a failed combined write leaves the
    /// checkpoint parked with no false resolution event.
    fn decide_checkpoint_with_principal(
        &mut self,
        run_id: &str,
        decision: super::approval::ApprovalDecision,
        principal: super::approval::ApprovalPrincipal,
        drive_inline_capability_tail: bool,
    ) -> Result<SopRunAction> {
        let prior_run = self
            .active_runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        if prior_run.status != SopRunStatus::PausedCheckpoint {
            bail!(
                "Run {run_id} is not paused at a checkpoint (status: {})",
                prior_run.status
            );
        }
        if self.is_park_persist_pending(run_id) {
            bail!(
                "Run {run_id} cannot resolve: its parked checkpoint snapshot is not yet durably persisted (retrying)"
            );
        }

        if matches!(decision, super::approval::ApprovalDecision::Revise { .. }) {
            bail!("checkpoint revise decisions use the revision persistence path")
        }
        if matches!(decision, super::approval::ApprovalDecision::Amend { .. }) {
            self.can_amend_checkpoint(run_id)?;
        }

        let (_, sop) = self.resolve_active_run_sop(run_id)?;
        let current_step = self.resolve_sop_step(&sop, prior_run.current_step)?;
        let mut piped = step_input_value(&prior_run, current_step.number);
        if let super::approval::ApprovalDecision::Amend { text } = &decision {
            let field = self.checkpoint_edit_field(run_id)?;
            let Some(object) = piped.as_object_mut() else {
                bail!(
                    "Run {run_id} checkpoint input is not a JSON object; cannot amend field '{field}'"
                );
            };
            object.insert(field, serde_json::Value::String(text.clone()));
        }
        let (status, recorded_output, routed_output, started_at, completed_at) = match &decision {
            super::approval::ApprovalDecision::Approve
            | super::approval::ApprovalDecision::Amend { .. } => (
                SopStepStatus::Completed,
                piped.to_string(),
                piped,
                prior_run.started_at.clone(),
                Some(now_iso8601()),
            ),
            super::approval::ApprovalDecision::Deny { reason } => {
                if let super::step_contract::StepFailure::Goto { step } = &current_step.on_failure {
                    self.resolve_sop_step(&sop, *step)?;
                }
                let detail = reason
                    .clone()
                    .unwrap_or_else(|| "checkpoint denied by operator".to_string());
                let now = now_iso8601();
                (
                    SopStepStatus::Failed,
                    detail.clone(),
                    serde_json::Value::String(detail),
                    now.clone(),
                    Some(now),
                )
            }
            super::approval::ApprovalDecision::Revise { .. } => {
                bail!("checkpoint revise decisions use the revision persistence path")
            }
        };

        let retries_consumed = prior_run
            .step_results
            .iter()
            .filter(|result| {
                result.step_number == current_step.number && result.status == SopStepStatus::Failed
            })
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        let denial_terminates = matches!(decision, super::approval::ApprovalDecision::Deny { .. })
            && matches!(
                route::failure::route_failure(
                    &current_step.on_failure,
                    retries_consumed,
                    self.config.max_step_retries,
                ),
                NextStep::Fail(_)
            );
        if denial_terminates {
            self.reacquire_claim_uncapped(run_id)?;
            if let Err(e) = self
                .store
                .mark_claim_retained_after_terminal_rollback(run_id)
            {
                self.release_claim_on_park(run_id);
                return Err(anyhow::Error::msg(format!(
                    "failed to persist terminal-rollback claim marker for run {run_id}: {e}"
                )));
            }
        } else {
            self.reacquire_claim_on_resume(run_id)?;
        }

        if let Some(run) = self.active_runs.get_mut(run_id) {
            run.status = SopRunStatus::Running;
            run.waiting_since = None;
            run.step_results.push(SopStepResult {
                step_number: current_step.number,
                status,
                output: recorded_output,
                started_at,
                completed_at,
                effective_agent: None,
                tool_calls: Vec::new(),
            });
        }

        let mut routed_status = status;
        if status == SopStepStatus::Completed {
            if let Err(reason) = self.validate_step_output(&current_step, &routed_output) {
                routed_status = SopStepStatus::Failed;
                let full_reason = format!(
                    "Step {} output schema validation failed: {reason}",
                    current_step.number
                );
                if let Some(recorded) = self
                    .active_runs
                    .get_mut(run_id)
                    .and_then(|run| run.step_results.last_mut())
                {
                    recorded.status = SopStepStatus::Failed;
                    recorded.output = full_reason;
                }
            } else if let Some(run) = self.active_runs.get_mut(run_id) {
                run.llm_calls_saved += 1;
            }
        }

        let route = match self.route_decision_after_recorded_step(
            run_id,
            &sop,
            &current_step,
            routed_status,
        ) {
            Ok(route) => route,
            Err(e) => {
                self.active_runs.insert(run_id.to_string(), prior_run);
                if !denial_terminates {
                    self.release_claim_on_park(run_id);
                }
                return Err(e);
            }
        };
        let event = super::approval::GateLedgerEntry {
            run_id: run_id.to_string(),
            step: current_step.number,
            gate_revision: Some(prior_run.revision),
            checkpoint_revision: Some(prior_run.revision),
            decision_identity: super::approval::broker::checkpoint_decision_identity(&decision)
                .map(|(_, identity)| identity),
            kind: super::approval::GateEventKind::Resolved,
            decision: Some(decision),
            principal,
            ts: now_iso8601(),
        }
        .into_event_record();

        match route {
            NextStep::Complete => {
                let saved = self
                    .active_runs
                    .get(run_id)
                    .map(|run| run.llm_calls_saved)
                    .unwrap_or(0);
                match self.finish_run_with_gate_event(run_id, SopRunStatus::Completed, None, &event)
                {
                    Ok(action) => {
                        self.deterministic_savings.total_llm_calls_saved += saved;
                        self.deterministic_savings.total_runs += 1;
                        Ok(action)
                    }
                    Err(e) => {
                        self.active_runs.insert(run_id.to_string(), prior_run);
                        if !denial_terminates {
                            self.release_claim_on_park(run_id);
                        }
                        Err(e)
                    }
                }
            }
            NextStep::Fail(reason) => match self.finish_run_with_gate_event(
                run_id,
                SopRunStatus::Failed,
                Some(reason),
                &event,
            ) {
                Ok(action) => Ok(action),
                Err(e) => {
                    self.active_runs.insert(run_id.to_string(), prior_run);
                    if !denial_terminates {
                        self.release_claim_on_park(run_id);
                    }
                    Err(e)
                }
            },
            next => {
                if let Err(e) = self.persist_active_with_gate_event(run_id, &event) {
                    self.active_runs.insert(run_id.to_string(), prior_run);
                    self.release_claim_on_park(run_id);
                    return Err(e);
                }
                let action = self.apply_route_decision(
                    run_id,
                    &sop,
                    current_step.number,
                    next,
                    true,
                    Some(retry_input_value(&prior_run, current_step.number)),
                    Some(routed_output),
                )?;
                // Checkpoint decisions are an established synchronous engine API:
                // callers and durable-effect tests expect an authorized capability
                // tail to finish before the broker outcome is returned. Shared
                // dashboard/manual execution instead uses the async executor, which
                // advances one action per lock acquisition.
                if drive_inline_capability_tail {
                    self.drive_inline_capability_tail(run_id, action)
                } else {
                    Ok(action)
                }
            }
        }
    }

    /// Failure path for a denied checkpoint: record the checkpoint step `Failed`
    /// and route through its `on_failure` policy via the shared deterministic
    /// record-and-route chokepoint. `Goto` reaches the authored failure step;
    /// the default `Fail` terminates the run `Failed`. Mirrors `approve_step`'s
    /// guard so a wrong-status or missing run fails closed with the gate intact.
    fn deny_checkpoint(&mut self, run_id: &str, reason: Option<String>) -> Result<SopRunAction> {
        let status = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"run_id": run_id})),
                    "SOP engine: active run not found"
                );
                anyhow::Error::msg(format!("Active run not found: {run_id}"))
            })?
            .status;

        if status != SopRunStatus::PausedCheckpoint {
            bail!("Run {run_id} is not paused at a checkpoint (status: {status})");
        }

        if self.is_park_persist_pending(run_id) {
            bail!(
                "Run {run_id} cannot resolve: its parked checkpoint snapshot is not yet durably persisted (retrying)"
            );
        }

        let (_, sop) = self.resolve_active_run_sop(run_id)?;
        let current_step_number = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?
            .current_step;
        let current_step = self.resolve_sop_step(&sop, current_step_number)?;

        // Resolve a failure-route target before mutating the parked run. A stale
        // `Goto` must leave the checkpoint untouched and re-resolvable.
        if let super::step_contract::StepFailure::Goto { step } = &current_step.on_failure {
            self.resolve_sop_step(&sop, *step)?;
        }

        let prior_run = self
            .active_runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        // Classify the denial's routing outcome BEFORE any mutation, using the
        // AUTHORITATIVE failure router (not a second copy of its logic). A denial
        // records the checkpoint step `Failed`; the router computes `retries_consumed`
        // as (Failed count - 1) after that record, so before it the current Failed
        // count for this step is exactly that value.
        let retries_consumed = self
            .active_runs
            .get(run_id)
            .map(|run| {
                run.step_results
                    .iter()
                    .filter(|r| {
                        r.step_number == current_step.number && r.status == SopStepStatus::Failed
                    })
                    .count() as u32
            })
            .unwrap_or(0);
        let terminates = matches!(
            route::failure::route_failure(
                &current_step.on_failure,
                retries_consumed,
                self.config.max_step_retries,
            ),
            NextStep::Fail(_)
        );
        if terminates {
            // TERMINAL denial (default `Fail`, or a `Retry` whose budget is spent):
            // it must reacquire to complete atomically even under saturation - gating
            // a run that is ENDING on a free slot would strand it. This is the
            // terminal-rollback atomicity path; it stays UNCAPPED by design.
            self.reacquire_claim_uncapped(run_id)?;
        } else {
            // CONTINUING denial (`Goto`, or a `Retry` with budget remaining): it
            // resumes execution, so it must pass the SAME capped store CAS every other
            // resume-to-continue path uses, honoring the per-SOP and global limits. At
            // capacity this returns `ResumeAtCapacity`; the `?` early-returns with the
            // checkpoint still parked and re-resolvable (no mutation, no retention
            // marker yet) - typed backpressure, never an over-cap execution.
            self.reacquire_claim_on_resume(run_id)?;
        }
        if let Err(marker_err) = self
            .store
            .mark_claim_retained_after_terminal_rollback(run_id)
        {
            self.active_runs.insert(run_id.to_string(), prior_run);
            self.release_claim_on_park(run_id);
            return Err(anyhow::Error::msg(format!(
                "failed to persist terminal-rollback claim retention marker for run {run_id}: {marker_err}"
            )));
        }
        self.claims_retained_after_terminal_rollback
            .insert(run_id.to_string());

        let detail = reason.unwrap_or_else(|| "checkpoint denied by operator".to_string());
        let now = now_iso8601();

        if let Some(run) = self.active_runs.get_mut(run_id) {
            run.status = SopRunStatus::Running;
            run.waiting_since = None;
        }
        match self.record_deterministic_step_result(
            run_id,
            &sop,
            &current_step,
            SopStepStatus::Failed,
            detail.clone(),
            serde_json::Value::String(detail.clone()),
            now.clone(),
            Some(now),
        ) {
            Ok(action) => {
                if !self.persist_active_checked(run_id) {
                    self.active_runs.insert(run_id.to_string(), prior_run);
                    self.claims_pending_persist.remove(run_id);
                    self.claims_retained_after_terminal_rollback.remove(run_id);
                    self.release_claim_on_park(run_id);
                    return Err(anyhow::Error::msg(format!(
                        "failed to persist checkpoint denial transition for run {run_id}"
                    )));
                }
                if self.active_runs.get(run_id).is_some_and(|run| {
                    matches!(
                        run.status,
                        SopRunStatus::WaitingApproval | SopRunStatus::PausedCheckpoint
                    )
                }) {
                    // The denial ROUTED to another gate and the new parked snapshot
                    // is durably persisted, so this run continued — it did NOT terminal-
                    // rollback. The reacquired claim still carries the durable terminal-
                    // rollback retention marker, which is now stale. Clear it with a
                    // CHECKED release: a swallowed failure would leave a live durable
                    // marker on a continued run, which `restore_runs` would then renew
                    // forever (the slot leak this PR exists to prevent). If the release
                    // fails we must NOT report success with a live marker — roll back to
                    // the pre-decision park, drop the in-memory retention/pending
                    // tracking (so the stale claim is not heartbeated and the lease
                    // reaper frees it), and surface the error so the caller retries.
                    if let Err(e) = self.release_claim_checked(run_id) {
                        self.active_runs.insert(run_id.to_string(), prior_run);
                        self.claims_pending_persist.remove(run_id);
                        self.claims_retained_after_terminal_rollback.remove(run_id);
                        return Err(anyhow::Error::msg(format!(
                            "failed to release exec claim after routing checkpoint denial for run {run_id}: {e}"
                        )));
                    }
                    self.claims_pending_persist.remove(run_id);
                }
                self.claims_retained_after_terminal_rollback.remove(run_id);
                self.record_transition_event(
                    run_id,
                    "checkpoint_denied",
                    Some(detail),
                    ::serde_json::json!({
                        "step": current_step.number,
                        "kind": current_step.kind.to_string(),
                    }),
                );
                Ok(action)
            }
            Err(e) => {
                self.active_runs.insert(run_id.to_string(), prior_run);
                // The terminal write was rejected, so the durable store may still
                // restore this parked run. Keep the claim acquired for this decision
                // attempt to prevent another trigger from taking its execution slot.
                Err(e)
            }
        }
    }

    /// Prepare a `WaitingApproval` gate clear: mutate the in-memory run to the
    /// target state and describe how the wrapper must commit it with the gate
    /// ledger row. The wrapper owns persistence and post-commit secondary events.
    ///
    /// All-or-nothing: the SOP definition and current step are resolved (and
    /// bounds-checked) BEFORE any in-memory mutation, so a definition removed or
    /// shrunk mid-run returns `Err` with the gate left untouched (still
    /// `WaitingApproval`, re-resolvable) rather than half-transitioned or panicking
    /// on an out-of-range step index (which would poison the engine mutex). The
    /// pure prefix of these lookups is exposed as `can_clear_waiting_gate` so
    /// `resolve_gate` can fail closed before it touches the claim or the ledger.
    fn clear_waiting_gate(&mut self, run_id: &str) -> Result<GateClearTransition> {
        let (sop_name, current_step) = {
            let run = self
                .active_runs
                .get(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            (run.sop_name.clone(), run.current_step)
        };

        let sop = self
            .sops
            .iter()
            .find(|s| s.name == sop_name)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"sop_name": sop_name})),
                    "SOP engine: sop no longer loaded (definition removed mid-run)"
                );
                anyhow::Error::msg(format!("SOP '{sop_name}' no longer loaded"))
            })?
            .clone();

        // Resolve the waiting step by its NUMBER (not vec position): a routed SOP with
        // non-contiguous step numbers (e.g. 1, 5) means position != number, and a
        // positional lookup would resume the wrong step - and, worse, only AFTER
        // resolve_gate already reacquired the claim and wrote the gate_resolved row.
        let step = self.resolve_sop_step(&sop, current_step)?;

        let run_data = {
            let run = self
                .active_runs
                .get(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            RunData::from_step_results(&run.step_results)
        };
        if !route::eligible(&step, &run_data) {
            return self.gate_step_pending_transition(
                run_id,
                &sop,
                step.number,
                format!("step {} dependencies not satisfied", step.number),
            );
        }

        let input = {
            let run = self
                .active_runs
                .get(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            step_input_value(run, step.number)
        };
        if let Some(reason) = self.schema_input_failure_reason(&step, &input) {
            return self.gate_schema_failure_transition(run_id, step.number, "input", reason);
        }

        // The exec claim was already re-acquired by resolve_gate BEFORE the audit row
        // (so a claim failure never writes a false gate_resolved row, and the run
        // holds its claim before EITHER the Pending or the Running transition here).

        // The lookups succeeded; commit the transition.
        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        run.status = SopRunStatus::Running;
        run.waiting_since = None;
        let context = format_step_context(&sop, run, &step, &self.config);

        let mut step = step;
        step.agent = step
            .effective_agent(sop.agent.as_deref())
            .map(str::to_string);

        Ok(GateClearTransition::Active {
            action: Box::new(SopRunAction::ExecuteStep {
                run_id: run_id.to_string(),
                step,
                context,
            }),
            follow_up: None,
        })
    }

    /// List finished runs, optionally filtered by SOP name.
    pub fn finished_runs(&self, sop_name: Option<&str>) -> Vec<&SopRun> {
        self.finished_runs
            .iter()
            .filter(|r| sop_name.is_none_or(|name| r.sop_name == name))
            .collect()
    }

    /// Summaries of every run the engine currently holds: live runs from the
    /// active set plus retained terminal runs, newest first by start time.
    /// This is the enumeration the Runs surface polls; it never touches the
    /// durable store directly, so it reflects exactly what the running engine
    /// knows (active set + `max_finished_runs` retention window).
    pub fn run_summaries(&self, sop_name: Option<&str>) -> Vec<SopRunSummary> {
        let mut out: Vec<SopRunSummary> = self
            .active_runs
            .values()
            .filter(|r| sop_name.is_none_or(|name| r.sop_name == name))
            .map(|r| SopRunSummary::from_run(r, true))
            .chain(
                self.finished_runs
                    .iter()
                    .filter(|r| sop_name.is_none_or(|name| r.sop_name == name))
                    .map(|r| SopRunSummary::from_run(r, false)),
            )
            .collect();
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        out
    }

    /// Return cumulative deterministic execution savings.
    pub fn deterministic_savings(&self) -> &DeterministicSavings {
        &self.deterministic_savings
    }

    /// Save a procedural-memory proposal into the shared SOP store. This is the
    /// production-facing engine surface EPIC F consumes for approval/write-back.
    pub fn save_proposal(&self, proposal: &ProposalRecord) -> Result<(), StoreError> {
        self.store.save_proposal(proposal)
    }

    /// Load a procedural-memory proposal by id from the shared SOP store.
    pub fn load_proposal(&self, id: &str) -> Result<Option<ProposalRecord>, StoreError> {
        self.store.load_proposal(id)
    }

    /// List procedural-memory proposals, optionally filtered by lifecycle status.
    pub fn list_proposals(
        &self,
        status: Option<ProposalStatus>,
    ) -> Result<Vec<ProposalRecord>, StoreError> {
        self.store.list_proposals(status)
    }

    // ── Deterministic execution ─────────────────────────────────

    /// Start a deterministic SOP run. Steps execute sequentially without LLM
    /// round-trips. Returns the first action (DeterministicStep or CheckpointWait).
    pub fn start_deterministic_run(
        &mut self,
        sop_name: &str,
        event: SopEvent,
    ) -> Result<SopRunAction> {
        // A2: this is a PUBLIC start entrypoint, so it must enforce the admission
        // policy itself - a direct caller must not be able to bypass Hold / Coalesce
        // / the pending-approval pool that `start_run` enforces. (When reached via
        // `start_run` the re-check is idempotent under the same held lock.)
        self.enforce_admission(sop_name)?;

        let sop = self.get_sop(sop_name).ok_or_else(|| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"sop_name": sop_name})),
                "SOP engine: sop not found"
            );
            anyhow::Error::msg(format!("SOP not found: {sop_name}"))
        })?;

        // Reject a non-deterministic SOP BEFORE reserving a slot, so a wrong-mode direct
        // call cannot claim (and then have to roll back) an execution slot.
        if sop.execution_mode != SopExecutionMode::Deterministic {
            bail!(
                "SOP '{}' is not in deterministic mode (mode: {})",
                sop_name,
                sop.execution_mode
            );
        }

        // Reserve + activate through the shared two-phase start path (identical run_id
        // prefix, logging, and dispatch to the pre-refactor inline body).
        let reservation = self.reserve_run_slot(sop_name)?;
        self.activate_reserved_run(reservation, event)
    }

    pub fn drive_headless_deterministic(
        &mut self,
        run_id: &str,
        first_action: SopRunAction,
    ) -> Result<SopRunAction> {
        let mut action = first_action;
        loop {
            action = self.advance_headless_deterministic_step(run_id, action)?;
            if !matches!(action, SopRunAction::DeterministicStep { .. }) {
                return Ok(action);
            }
        }
    }

    /// Preserve the synchronous checkpoint contract for built-in capability
    /// tails without consuming an `Execute` action that belongs to an external
    /// agent driver.
    fn drive_inline_capability_tail(
        &mut self,
        run_id: &str,
        action: SopRunAction,
    ) -> Result<SopRunAction> {
        if matches!(
            action,
            SopRunAction::DeterministicStep {
                ref step,
                ..
            } if step.kind == SopStepKind::Capability
        ) {
            self.drive_headless_deterministic(run_id, action)
        } else {
            Ok(action)
        }
    }

    /// Execute at most one deterministic capability step. Async drivers call
    /// this once per engine-lock acquisition so an operator cancellation can
    /// acquire the lock and become visible before the next step dispatches.
    pub fn advance_headless_deterministic_step(
        &mut self,
        run_id: &str,
        action: SopRunAction,
    ) -> Result<SopRunAction> {
        if let Some(cancelled) = self.finish_requested_cancellation(run_id)? {
            return Ok(cancelled);
        }
        match action {
            SopRunAction::DeterministicStep {
                ref step,
                ref input,
                ..
            } if step.kind == SopStepKind::Capability => {
                let (_, sop) = self.resolve_active_run_sop(run_id)?;
                self.execute_capability_step(&sop, run_id, step, input.clone())
            }
            SopRunAction::DeterministicStep {
                ref step,
                ref run_id,
                ..
            } => {
                let sop_name = self
                    .active_runs
                    .get(run_id)
                    .map(|run| run.sop_name.clone())
                    .unwrap_or_default();
                self.fail_headless_driverless_step(run_id, &sop_name, step)
            }
            terminal => Ok(terminal),
        }
    }

    /// Fail a run whose headless driver consumed its bounded step budget.
    /// The engine owns the durable terminal transition so the active run and
    /// its concurrency claim are released together.
    pub(crate) fn fail_headless_step_budget(&mut self, run_id: &str) -> Result<SopRunAction> {
        self.step_budget_finalization_ready
            .insert(run_id.to_string());
        let result = self.finish_run(
            run_id,
            SopRunStatus::Failed,
            Some("SOP headless driver step budget exhausted".to_string()),
        );
        if result.is_ok() {
            self.step_budget_finalization_ready.remove(run_id);
        }
        result
    }

    /// Advance a deterministic run with the output of the current step.
    /// The output is piped as input to the next step.
    pub fn advance_deterministic_step(
        &mut self,
        run_id: &str,
        step_output: serde_json::Value,
        step_timestamps: Option<(String, Option<String>)>,
    ) -> Result<SopRunAction> {
        let (_, sop) = self.resolve_active_run_sop(run_id)?;
        let current_step_number = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?
            .current_step;
        let current_step = self.resolve_sop_step(&sop, current_step_number)?;
        let (started_at, completed_at) = match step_timestamps {
            Some((started, completed)) => (started, completed),
            None => {
                let run = self
                    .active_runs
                    .get(run_id)
                    .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
                (run.started_at.clone(), Some(now_iso8601()))
            }
        };

        self.record_deterministic_step_result(
            run_id,
            &sop,
            &current_step,
            SopStepStatus::Completed,
            step_output.to_string(),
            step_output,
            started_at,
            completed_at,
        )
    }

    fn forge_comment_authorized_by_prior_checkpoint(
        &self,
        sop: &Sop,
        run_id: &str,
        step_number: u32,
        input: &serde_json::Value,
    ) -> bool {
        let Some(run) = self.active_runs.get(run_id) else {
            return false;
        };
        let checkpoint_revision = run.revision;
        let Some(checkpoint_result) = run
            .step_results
            .iter()
            .rev()
            .find(|result| result.status == SopStepStatus::Completed)
        else {
            return false;
        };
        let checkpoint_step_number = checkpoint_result.step_number;
        if !sop.steps.iter().any(|step| {
            step.number == checkpoint_step_number && step.kind == SopStepKind::Checkpoint
        }) {
            return false;
        }
        if checkpoint_step_number >= step_number {
            return false;
        }
        if !forge_comment_input_matches_checkpoint_output(input, checkpoint_result) {
            return false;
        }

        self.run_events(run_id).is_ok_and(|events| {
            events.iter().any(|event| {
                event.kind.as_str() == "gate_resolved"
                    && event.payload.get("step").and_then(|value| value.as_u64())
                        == Some(u64::from(checkpoint_step_number))
                    && event
                        .payload
                        .get("checkpoint_revision")
                        .and_then(|value| value.as_u64())
                        == Some(u64::from(checkpoint_revision))
                    && event
                        .payload
                        .get("source")
                        .and_then(|value| value.as_str())
                        .is_some_and(|source| source != "agent" && source != "system")
                    && matches!(
                        event
                            .payload
                            .get("decision")
                            .and_then(|value| value.as_str()),
                        Some("approve") | Some("amend")
                    )
            })
        })
    }

    fn forge_comment_effect_payload(
        &self,
        sop: &Sop,
        step_number: u32,
        input: &Value,
    ) -> Result<Value> {
        let target =
            super::capability::resolve_forge_comment_target(input).map_err(anyhow::Error::msg)?;
        Ok(::serde_json::json!({
            "capability": "forge.comment",
            "sop_name": sop.name,
            "step": step_number,
            "channel": target.channel,
            "repo": target.repo,
            "number": target.number,
            "body": target.body,
        }))
    }

    fn forge_comment_success_output(&self, input: &Value) -> Result<Value> {
        let target =
            super::capability::resolve_forge_comment_target(input).map_err(anyhow::Error::msg)?;
        Ok(::serde_json::json!({
            "posted": true,
            "repo": target.repo,
            "number": target.number,
        }))
    }

    fn forge_comment_effect_state(
        &self,
        run_id: &str,
        effect_payload: &Value,
    ) -> Result<(bool, bool), StoreError> {
        let mut started = false;
        let mut completed = false;
        for event in self.store.list_events(run_id)? {
            if event.payload == *effect_payload {
                match event.kind.as_str() {
                    "capability_effect_started" => started = true,
                    "capability_effect_completed" => completed = true,
                    _ => {}
                }
            }
        }
        Ok((started, completed))
    }

    fn record_forge_comment_effect_marker(
        &self,
        run_id: &str,
        kind: &str,
        effect_payload: Value,
    ) -> Result<(), StoreError> {
        self.store
            .append_event(&SopEventRecord {
                run_id: run_id.to_string(),
                seq: 0,
                ts: now_iso8601(),
                kind: kind.to_string(),
                actor: None,
                reason: None,
                payload: effect_payload,
            })
            .map(|_| ())
    }

    fn record_forge_comment_failure(
        &mut self,
        run_id: &str,
        sop: &Sop,
        step: &SopStep,
        error: String,
        started_at: String,
    ) -> Result<SopRunAction> {
        self.metrics.record_capability_executed(&sop.name);
        let completed_at = Some(now_iso8601());
        self.record_deterministic_step_result(
            run_id,
            sop,
            step,
            SopStepStatus::Failed,
            error.clone(),
            serde_json::Value::String(error),
            started_at,
            completed_at,
        )
    }

    fn execute_forge_comment_step(
        &mut self,
        sop: &Sop,
        run_id: &str,
        step: &SopStep,
        input: Value,
        capability_input: Value,
        started_at: String,
    ) -> Result<SopRunAction> {
        if !self.forge_comment_authorized_by_prior_checkpoint(
            sop,
            run_id,
            step.number,
            &capability_input,
        ) {
            return self.record_forge_comment_failure(
                run_id,
                sop,
                step,
                "forge.comment requires the immediately preceding checkpoint to approve the exact repo, number, body, and channel"
                    .to_string(),
                started_at,
            );
        }

        let effect_payload =
            match self.forge_comment_effect_payload(sop, step.number, &capability_input) {
                Ok(payload) => payload,
                Err(e) => {
                    return self.record_forge_comment_failure(
                        run_id,
                        sop,
                        step,
                        format!("forge.comment: invalid target for effect ledger: {e}"),
                        started_at,
                    );
                }
            };
        let success_output = match self.forge_comment_success_output(&capability_input) {
            Ok(output) => output,
            Err(e) => {
                return self.record_forge_comment_failure(
                    run_id,
                    sop,
                    step,
                    format!("forge.comment: invalid target for success replay: {e}"),
                    started_at,
                );
            }
        };

        match self.forge_comment_effect_state(run_id, &effect_payload) {
            Ok((_started, true)) => {
                self.metrics.record_capability_executed(&sop.name);
                let completed_at = Some(now_iso8601());
                return self.record_deterministic_step_result(
                    run_id,
                    sop,
                    step,
                    SopStepStatus::Completed,
                    success_output.to_string(),
                    success_output,
                    started_at,
                    completed_at,
                );
            }
            Ok((true, false)) => {
                return self.record_forge_comment_failure(
                    run_id,
                    sop,
                    step,
                    "forge.comment has a prior unconfirmed public-send attempt for this run/step/target; refusing to replay automatically"
                        .to_string(),
                    started_at,
                );
            }
            Ok((false, false)) => {}
            Err(e) => {
                return self.record_forge_comment_failure(
                    run_id,
                    sop,
                    step,
                    format!(
                        "forge.comment cannot inspect durable effect ledger (fail-closed): {e}"
                    ),
                    started_at,
                );
            }
        }

        if let Err(e) = self.record_forge_comment_effect_marker(
            run_id,
            "capability_effect_started",
            effect_payload.clone(),
        ) {
            return self.record_forge_comment_failure(
                run_id,
                sop,
                step,
                format!(
                    "forge.comment cannot persist public-send attempt marker (fail-closed): {e}"
                ),
                started_at,
            );
        }

        let ctx = super::capability::CapabilityContext {
            run_id: run_id.to_string(),
            sop_name: sop.name.clone(),
            step_number: step.number,
            sop_location: sop.location.clone(),
        };
        let result = self.capabilities.execute_step(ctx, step, input);
        self.metrics.record_capability_executed(&sop.name);
        let completed_at = Some(now_iso8601());
        match result {
            Ok(result) if result.success => {
                if let Err(e) = self.record_forge_comment_effect_marker(
                    run_id,
                    "capability_effect_completed",
                    effect_payload,
                ) {
                    let error = format!(
                        "forge.comment posted but could not persist success marker (fail-closed; refusing replay): {e}"
                    );
                    return self.record_deterministic_step_result(
                        run_id,
                        sop,
                        step,
                        SopStepStatus::Failed,
                        error.clone(),
                        serde_json::Value::String(error),
                        started_at,
                        completed_at,
                    );
                }
                self.record_deterministic_step_result(
                    run_id,
                    sop,
                    step,
                    SopStepStatus::Completed,
                    result.output.to_string(),
                    result.output,
                    started_at,
                    completed_at,
                )
            }
            Ok(result) => {
                let error = result
                    .error
                    .unwrap_or_else(|| "capability returned failure".to_string());
                self.record_deterministic_step_result(
                    run_id,
                    sop,
                    step,
                    SopStepStatus::Failed,
                    error.clone(),
                    serde_json::Value::String(error),
                    started_at,
                    completed_at,
                )
            }
            Err(e) => {
                let error = e.to_string();
                self.record_deterministic_step_result(
                    run_id,
                    sop,
                    step,
                    SopStepStatus::Failed,
                    error.clone(),
                    serde_json::Value::String(error),
                    started_at,
                    completed_at,
                )
            }
        }
    }

    fn execute_capability_step(
        &mut self,
        sop: &Sop,
        run_id: &str,
        step: &SopStep,
        input: serde_json::Value,
    ) -> Result<SopRunAction> {
        let started_at = now_iso8601();
        let capability_input = step.capability_call_input(input.clone());
        if step.capability_id() == Some("forge.comment") {
            return self.execute_forge_comment_step(
                sop,
                run_id,
                step,
                input,
                capability_input,
                started_at,
            );
        }

        let ctx = super::capability::CapabilityContext {
            run_id: run_id.to_string(),
            sop_name: sop.name.clone(),
            step_number: step.number,
            sop_location: sop.location.clone(),
        };
        let result = self.capabilities.execute_step(ctx, step, input);
        self.metrics.record_capability_executed(&sop.name);
        let completed_at = Some(now_iso8601());
        match result {
            Ok(result) if result.success => self.record_deterministic_step_result(
                run_id,
                sop,
                step,
                SopStepStatus::Completed,
                result.output.to_string(),
                result.output,
                started_at,
                completed_at,
            ),
            Ok(result) => {
                let error = result
                    .error
                    .unwrap_or_else(|| "capability returned failure".to_string());
                self.record_deterministic_step_result(
                    run_id,
                    sop,
                    step,
                    SopStepStatus::Failed,
                    error.clone(),
                    serde_json::Value::String(error),
                    started_at,
                    completed_at,
                )
            }
            Err(e) => {
                let error = e.to_string();
                self.record_deterministic_step_result(
                    run_id,
                    sop,
                    step,
                    SopStepStatus::Failed,
                    error.clone(),
                    serde_json::Value::String(error),
                    started_at,
                    completed_at,
                )
            }
        }
    }

    fn record_deterministic_step_result(
        &mut self,
        run_id: &str,
        sop: &Sop,
        current_step: &SopStep,
        status: SopStepStatus,
        recorded_output: String,
        routed_output: serde_json::Value,
        started_at: String,
        completed_at: Option<String>,
    ) -> Result<SopRunAction> {
        let run = self.active_runs.get_mut(run_id).ok_or_else(|| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"run_id": run_id})),
                "SOP engine: active run not found"
            );
            anyhow::Error::msg(format!("Active run not found: {run_id}"))
        })?;
        let retry_input = retry_input_value(run, current_step.number);
        run.step_results.push(SopStepResult {
            step_number: run.current_step,
            status,
            output: recorded_output,
            started_at,
            completed_at,
            effective_agent: None,
            tool_calls: Vec::new(),
        });

        let mut last_status = status;
        if status == SopStepStatus::Completed {
            if let Err(reason) = self.validate_step_output(current_step, &routed_output) {
                last_status = SopStepStatus::Failed;
                let full_reason = format!(
                    "Step {} output schema validation failed: {reason}",
                    current_step.number
                );
                self.record_transition_event(
                    run_id,
                    "step_schema_reject",
                    Some(full_reason.clone()),
                    ::serde_json::json!({
                        "step": current_step.number,
                        "phase": "output",
                    }),
                );
                if let Some(recorded) = self
                    .active_runs
                    .get_mut(run_id)
                    .and_then(|run| run.step_results.last_mut())
                {
                    recorded.status = SopStepStatus::Failed;
                    recorded.output = full_reason;
                }
            } else if let Some(run) = self.active_runs.get_mut(run_id) {
                run.llm_calls_saved += 1;
            }
        }

        self.route_recorded_step(
            run_id,
            sop,
            current_step,
            last_status,
            true,
            Some(retry_input),
            Some(routed_output),
        )
    }

    fn resolve_active_run_sop(&self, run_id: &str) -> Result<(String, Sop)> {
        let sop_name = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?
            .sop_name
            .clone();
        let sop = self
            .sops
            .iter()
            .find(|s| s.name == sop_name)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("SOP '{sop_name}' no longer loaded")))?;
        Ok((sop_name, sop))
    }

    fn fail_headless_driverless_step(
        &mut self,
        run_id: &str,
        sop_name: &str,
        step: &SopStep,
    ) -> Result<SopRunAction> {
        let reason = format!(
            "Headless deterministic SOP step {} '{}' requires an external driver; it was not executed",
            step.number, step.title
        );
        let now = now_iso8601();
        if let Some(run) = self.active_runs.get_mut(run_id) {
            run.step_results.push(SopStepResult {
                step_number: step.number,
                status: SopStepStatus::Failed,
                output: reason.clone(),
                started_at: now.clone(),
                completed_at: Some(now),
                effective_agent: None,
                tool_calls: Vec::new(),
            });
        }
        self.record_transition_event(
            run_id,
            "headless_driver_missing",
            Some(reason.clone()),
            ::serde_json::json!({
                "sop_name": sop_name,
                "step": step.number,
                "kind": step.kind.to_string(),
            }),
        );
        self.finish_run(run_id, SopRunStatus::Failed, Some(reason))
    }

    /// Resume a deterministic run from persisted state.
    pub fn resume_deterministic_run(
        &mut self,
        state: DeterministicRunState,
    ) -> Result<SopRunAction> {
        // Validate the run exists and is paused (immutable read), capturing its SOP
        // name, before any mutation - so the fail-closed reacquire can run first.
        let sop_name = match self.active_runs.get(&state.run_id) {
            Some(run) if run.status == SopRunStatus::PausedCheckpoint => run.sop_name.clone(),
            Some(run) => {
                bail!(
                    "Run {} is not paused at checkpoint (status: {})",
                    state.run_id,
                    run.status
                );
            }
            None => {
                let run_id = state.run_id.clone();
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"run_id": run_id})),
                    "SOP engine: active run not found"
                );
                bail!("Active run not found: {}", state.run_id);
            }
        };

        // Refuse to resume while the checkpoint's parked snapshot has not yet
        // been durably persisted (see `is_park_persist_pending`'s doc): the kept
        // claim predates this attempt, and reacquiring on top of it would give a
        // later rollback or a maintenance retry no way to distinguish "freshly
        // reacquired" from "pre-existing, must survive."
        if self.is_park_persist_pending(&state.run_id) {
            bail!(
                "Run {} cannot resume: its parked checkpoint snapshot is not yet durably persisted (retrying)",
                state.run_id
            );
        }

        let sop = self
            .sops
            .iter()
            .find(|s| s.name == sop_name)
            .ok_or_else(|| {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({"sop_name": sop_name.as_str()})),
                    "SOP engine: sop no longer loaded (definition removed mid-run)"
                );
                anyhow::Error::msg(format!("SOP '{sop_name}' no longer loaded"))
            })?
            .clone();

        // Pre-flight the step this resume will advance to BEFORE reacquiring the
        // claim or mutating the run: a definition shrunk while parked must fail
        // closed here, with the run left untouched at `PausedCheckpoint`
        // (re-resolvable), instead of after the mutation below - which would
        // otherwise strand the run in `Running`, holding a claim, with no way to
        // make progress.
        let resume_step = if state.last_completed_step == 0 {
            1
        } else {
            state.last_completed_step
        };
        self.resolve_sop_step(&sop, resume_step)?;

        // A1: fail-closed - a restored parked run holds no exec claim; re-acquire it
        // BEFORE the transition and abort (leaving the run paused) if it fails.
        self.reacquire_claim_on_resume(&state.run_id)?;

        let run = self
            .active_runs
            .get_mut(&state.run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {}", state.run_id)))?;
        let prior_waiting_since = run.waiting_since.clone();
        let prior_llm_calls_saved = run.llm_calls_saved;
        let prior_current_step = run.current_step;
        run.status = SopRunStatus::Running;
        run.waiting_since = None;
        run.llm_calls_saved = state.llm_calls_saved;
        // Resuming past step 0 re-enters at the last completed step. Set it
        // here, under the lookup that just validated the run, so the dispatch
        // below never needs a second, fallible lookup of the same run.
        if state.last_completed_step != 0 {
            run.current_step = state.last_completed_step;
        }
        for (step_number, output) in &state.step_outputs {
            let already_recorded = run
                .step_results
                .iter()
                .any(|result| result.step_number == *step_number);
            if !already_recorded {
                run.step_results.push(SopStepResult {
                    step_number: *step_number,
                    status: SopStepStatus::Completed,
                    output: output.to_string(),
                    started_at: state.persisted_at.clone(),
                    completed_at: Some(state.persisted_at.clone()),
                    effective_agent: None,
                    tool_calls: Vec::new(),
                });
            }
        }

        let last_output = state
            .step_outputs
            .get(&state.last_completed_step)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let run_id = state.run_id.clone();

        let outcome = if state.last_completed_step == 0 {
            self.dispatch_deterministic_step(&run_id, &sop, 1, last_output)
        } else {
            self.resolve_sop_step(&sop, state.last_completed_step)
                .and_then(|current_step| {
                    self.route_recorded_step(
                        &run_id,
                        &sop,
                        &current_step,
                        SopStepStatus::Completed,
                        true,
                        None,
                        Some(last_output),
                    )
                })
        };

        match outcome {
            Ok(action) => Ok(action),
            Err(e) => {
                // Defensive: the pre-flight above validated the same step lookup
                // under this lock, so this is unreachable in practice. If it still
                // fails, roll the run back to `PausedCheckpoint` and release the
                // just-reacquired claim so it doesn't get stuck in `Running`
                // holding a leaked exec slot.
                if let Some(run) = self.active_runs.get_mut(&run_id) {
                    run.status = SopRunStatus::PausedCheckpoint;
                    run.waiting_since = prior_waiting_since;
                    run.llm_calls_saved = prior_llm_calls_saved;
                    run.current_step = prior_current_step;
                }
                self.release_claim_on_park(&run_id);
                Err(e)
            }
        }
    }

    /// Resolve the action for a deterministic step (execute or checkpoint).
    fn resolve_deterministic_action(
        &mut self,
        sop: &Sop,
        run_id: &str,
        step: &SopStep,
        input: serde_json::Value,
    ) -> Result<SopRunAction> {
        let run_data = {
            let run = self
                .active_runs
                .get(run_id)
                .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
            RunData::from_step_results(&run.step_results)
        };
        if !route::eligible(step, &run_data) {
            return Ok(self.mark_step_pending(
                run_id,
                sop,
                step.number,
                format!("step {} dependencies not satisfied", step.number),
            ));
        }

        if let Some(action) = self.schema_input_failure_action(run_id, step, &input)? {
            return Ok(action);
        }

        match step.kind {
            SopStepKind::Checkpoint => {
                if let Some(reason) = self.pending_pool_full_reason(sop) {
                    Self::log_pending_capacity_full(run_id, &reason);
                    return Ok(self.mark_step_pending(run_id, sop, step.number, reason));
                }

                // Persist the checkpoint state before flipping the run status. If
                // the state-file write fails, the run remains Running with its
                // execution claim still heartbeat-eligible.
                let state_file = self.persist_deterministic_state(run_id, sop, true)?;

                // A prior checkpoint's recorded result (it records on resolve)
                // means this run has presented a gate before.
                let has_prior_gate = self.active_runs.get(run_id).is_some_and(|run| {
                    run.step_results.iter().any(|r| {
                        sop.steps
                            .iter()
                            .any(|s| s.number == r.step_number && s.kind == SopStepKind::Checkpoint)
                    })
                });
                // Pause at checkpoint - persist state and wait for approval
                if let Some(run) = self.active_runs.get_mut(run_id) {
                    run.status = SopRunStatus::PausedCheckpoint;
                    run.waiting_since = Some(now_iso8601());
                    // A NEW gate presentation (not a revise re-park): after the
                    // run's first-ever park, bump the presentation counter so
                    // this gate's prompt reference can never collide with an
                    // earlier gate's leftover buttons, and rebase the per-gate
                    // revise budget (`revision - revision_base`).
                    if run.revision > 0 || has_prior_gate {
                        run.revision += 1;
                    }
                    run.revision_base = run.revision;
                }

                ::clawcrew_log::record!(
                    INFO,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note),
                    &format!(
                        "Deterministic SOP run {run_id}: checkpoint at step {} '{}', state persisted to {}",
                        step.number,
                        step.title,
                        state_file.display().to_string()
                    )
                );

                // Mirror the paused checkpoint into the shared run store (alongside
                // the deterministic state file) so a restart leaves a non-terminal
                // row for restore_runs() to rehydrate. A1: free the exec slot while
                // the run waits at the checkpoint - but only AFTER the parked
                // snapshot is durably persisted (else keep the claim).
                match self.persist_parked_snapshot_then_release_claim(run_id) {
                    // A policy-gated checkpoint is the same durable approval boundary
                    // as `WaitingApproval`: send its configured request notice only
                    // after the parked snapshot is recoverable. If this write failed,
                    // the maintenance retry owns the eventual single notification.
                    ParkPersistOutcome::Released => self.notify_park_request(run_id),
                    ParkPersistOutcome::CapacityFull => {
                        let reason = self.pending_pool_capacity_raced_reason(sop);
                        Self::log_pending_capacity_full(run_id, &reason);
                        return Ok(self.mark_step_pending(run_id, sop, step.number, reason));
                    }
                    ParkPersistOutcome::PersistFailed => {
                        let reason =
                            format!("SOP '{}' park snapshot not yet durably persisted", sop.name);
                        return Ok(SopRunAction::Pending {
                            run_id: run_id.to_string(),
                            sop_name: sop.name.clone(),
                            step: step.number,
                            reason,
                        });
                    }
                }

                Ok(SopRunAction::CheckpointWait {
                    run_id: run_id.to_string(),
                    step: step.clone(),
                    state_file,
                })
            }
            SopStepKind::Capability | SopStepKind::Execute => {
                // Persist the active (Running) deterministic run so a restart mid-run
                // leaves a non-terminal row for restore_runs() to rehydrate. This is
                // the single sink for start / advance / resume deterministic steps.
                //
                // Capability execution is deliberately returned as an action instead
                // of running inline. Shared-engine drivers can then drop the lock
                // between capabilities, allowing cancellation to become visible at
                // the next safe step boundary.
                self.persist_active(run_id);

                Ok(SopRunAction::DeterministicStep {
                    run_id: run_id.to_string(),
                    step: step.clone(),
                    input,
                })
            }
        }
    }

    /// Persist the current deterministic run state to a JSON file.
    fn persist_deterministic_state(
        &self,
        run_id: &str,
        sop: &Sop,
        paused_at_checkpoint: bool,
    ) -> Result<PathBuf> {
        let run = self.active_runs.get(run_id).ok_or_else(|| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Reject)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({"run_id": run_id})),
                "SOP engine: run not found in history"
            );
            anyhow::Error::msg(format!("Run not found: {run_id}"))
        })?;

        let mut step_outputs = HashMap::new();
        let mut last_completed_step = 0;
        for result in &run.step_results {
            if result.status == SopStepStatus::Completed {
                let value = jsonish_value(&result.output);
                step_outputs.insert(result.step_number, value);
                last_completed_step = result.step_number;
            }
        }

        let state = DeterministicRunState {
            run_id: run_id.to_string(),
            sop_name: run.sop_name.clone(),
            last_completed_step,
            total_steps: run.total_steps,
            step_outputs,
            persisted_at: now_iso8601(),
            llm_calls_saved: run.llm_calls_saved,
            paused_at_checkpoint,
        };

        // Write to SOP location directory, or system temp dir
        let temp_dir = std::env::temp_dir();
        let dir = sop.location.as_deref().unwrap_or(temp_dir.as_path());
        let state_file = dir.join(format!("{run_id}.state.json"));
        let json = serde_json::to_string_pretty(&state)?;
        std::fs::write(&state_file, json)?;

        Ok(state_file)
    }

    /// Best-effort removal of a run's park-snapshot file once the run is
    /// terminal. Mirrors `persist_deterministic_state`'s path resolution; a
    /// missing file (the run never parked) is not an error.
    fn remove_deterministic_state_file(&self, run: &SopRun) {
        let temp_dir = std::env::temp_dir();
        let dir = self
            .get_sop(&run.sop_name)
            .and_then(|sop| sop.location.clone())
            .unwrap_or(temp_dir);
        let path = dir.join(format!("{}.state.json", run.run_id));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Failure)
                        .with_attrs(::serde_json::json!({
                            "run_id": run.run_id,
                            "path": path.display().to_string(),
                            "error": e.to_string(),
                        })),
                    "SOP engine: terminal run's park snapshot could not be removed"
                );
            }
        }
    }

    /// Load a persisted deterministic run state from a JSON file.
    pub fn load_deterministic_state(path: &Path) -> Result<DeterministicRunState> {
        let content = std::fs::read_to_string(path)?;
        let state: DeterministicRunState = serde_json::from_str(&content)?;
        Ok(state)
    }

    // ── Approval timeout ──────────────────────────────────────────

    pub fn check_approval_timeouts(&mut self) -> Vec<SopRunAction> {
        let action_cfg = self.config.approval_timeout_action;
        let mut actions = Vec::new();
        for run_id in self.overdue_waiting_run_ids() {
            if let Some(a) =
                super::approval::timeout::apply_timeout_action(self, &run_id, action_cfg)
            {
                actions.push(a);
            }
        }
        actions
    }

    fn overdue_waiting_run_ids(&self) -> Vec<String> {
        let timeout_secs = self.config.approval_timeout_secs;
        if timeout_secs == 0 {
            return Vec::new();
        }
        // cooldown_elapsed(ts, secs) returns true when (now - ts) >= secs.
        self.active_runs
            .values()
            .filter(|r| r.status == SopRunStatus::WaitingApproval)
            .filter(|r| !self.is_park_persist_pending(&r.run_id))
            .filter(|r| {
                r.waiting_since
                    .as_deref()
                    .is_some_and(|ts| cooldown_elapsed(ts, timeout_secs))
            })
            .map(|r| r.run_id.clone())
            .collect()
    }

    pub fn run_maintenance_tick(&mut self) -> MaintenanceSummary {
        // Count overdue gates BEFORE applying the action: the fail-closed Escalate
        // default re-stamps in place and produces no action, so counting actions
        // alone would under-report the escalations.
        let timed_out = self.overdue_waiting_run_ids().len();
        let timeout_actions = self.check_approval_timeouts();
        self.retry_pending_park_persists();
        self.retry_capacity_blocked_gated_pends();
        let finalized_step_budget_failures = self.retry_ready_step_budget_finalizations();
        let finalized_cancellations = self.retry_ready_cancellation_finalizations();
        self.heartbeat_active_claims();
        let reaped_claims = self.reap_expired_claims();
        let pruned_runs = self.prune_terminal_runs();
        MaintenanceSummary {
            timed_out,
            reaped_claims,
            pruned_runs,
            finalized_cancellations,
            finalized_step_budget_failures,
            timeout_actions,
        }
    }

    fn retry_ready_step_budget_finalizations(&mut self) -> usize {
        let ready: Vec<String> = self
            .step_budget_finalization_ready
            .iter()
            .cloned()
            .collect();
        let mut finalized = 0;
        for run_id in ready {
            match self.active_runs.get(&run_id).map(|run| run.status) {
                Some(SopRunStatus::Running) => match self.fail_headless_step_budget(&run_id) {
                    Ok(_) => finalized += 1,
                    Err(error) => {
                        ::clawcrew_log::record!(
                            WARN,
                            ::clawcrew_log::Event::new(
                                module_path!(),
                                ::clawcrew_log::Action::Fail
                            )
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "run_id": run_id,
                                "error": error.to_string(),
                            })),
                            "SOP maintenance: step-budget finalization retry failed"
                        );
                    }
                },
                Some(SopRunStatus::CancelRequested) => {
                    // A durable operator request supersedes the pending failure.
                    // The same driver-exited proof now makes cancellation safe
                    // to finalize during this maintenance pass.
                    self.step_budget_finalization_ready.remove(&run_id);
                    self.cancellation_finalization_ready.insert(run_id);
                }
                Some(_) | None => {
                    self.step_budget_finalization_ready.remove(&run_id);
                }
            }
        }
        finalized
    }

    fn retry_ready_cancellation_finalizations(&mut self) -> usize {
        let ready: Vec<String> = self
            .cancellation_finalization_ready
            .iter()
            .cloned()
            .collect();
        let mut finalized = 0;
        for run_id in ready {
            match self.finish_requested_cancellation(&run_id) {
                Ok(Some(_)) => finalized += 1,
                Ok(None) => {
                    self.cancellation_finalization_ready.remove(&run_id);
                }
                Err(error) => {
                    ::clawcrew_log::record!(
                        WARN,
                        ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Fail)
                            .with_outcome(::clawcrew_log::EventOutcome::Failure)
                            .with_attrs(::serde_json::json!({
                                "run_id": run_id,
                                "error": error.to_string(),
                            })),
                        "SOP maintenance: cancellation finalization retry failed"
                    );
                }
            }
        }
        finalized
    }

    /// Reclaim concurrency-claim leases past their expiry (the holder died without
    /// releasing). Best-effort: a store error is logged and the pass continues.
    /// Returns the number reclaimed.
    fn reap_expired_claims(&self) -> usize {
        let now = now_iso8601();
        let expired = match self.store.expired_claims(&now) {
            Ok(claims) => claims,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": e.to_string()})),
                    "SOP maintenance: failed to read expired claims"
                );
                return 0;
            }
        };
        let mut reclaimed = 0;
        for token in &expired {
            match self.store.release_claim(token) {
                Ok(()) => reclaimed += 1,
                Err(e) => ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": e.to_string()})),
                    "SOP maintenance: failed to release expired claim"
                ),
            }
        }
        reclaimed
    }

    /// Drop terminal runs beyond the retention policy (`max_finished_runs`).
    /// Best-effort; returns the number pruned.
    fn prune_terminal_runs(&self) -> usize {
        let policy = RetentionPolicy {
            max_terminal: self.config.max_finished_runs,
            keep_secs: None,
        };
        match self.store.prune(&policy) {
            Ok(n) => n,
            Err(e) => {
                ::clawcrew_log::record!(
                    WARN,
                    ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                        .with_outcome(::clawcrew_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": e.to_string()})),
                    "SOP maintenance: failed to prune terminal runs"
                );
                0
            }
        }
    }

    /// Re-stamp a run's `waiting_since` to now (timeout escalation: the gate stays
    /// open but the clock resets so it re-surfaces, not self-approves).
    pub(crate) fn restamp_waiting_with_gate_event(
        &mut self,
        run_id: &str,
        event: &SopEventRecord,
    ) -> Result<()> {
        let previous = match self.active_runs.get_mut(run_id) {
            Some(run) => {
                let previous = run.waiting_since.clone();
                run.waiting_since = Some(now_iso8601());
                previous
            }
            None => return Ok(()),
        };
        // Persist the re-stamped clock with the escalation event as one durable
        // outcome; otherwise history could say the gate escalated while the
        // timeout clock still points at the old overdue instant.
        if let Err(e) = self.persist_active_with_gate_event(run_id, event) {
            if let Some(run) = self.active_runs.get_mut(run_id) {
                run.waiting_since = previous;
            }
            return Err(e);
        }
        Ok(())
    }

    /// The current step number of an active run (0 if absent). For ledger rows.
    pub(crate) fn run_current_step(&self, run_id: &str) -> u32 {
        self.active_runs
            .get(run_id)
            .map(|r| r.current_step)
            .unwrap_or(0)
    }

    // ── Test helpers ──────────────────────────────────────────────

    /// Replace loaded SOPs (for testing from other modules).
    // Available for cross-crate testing
    pub fn set_sops_for_test(&mut self, sops: Vec<Sop>) {
        self.sops = sops;
    }

    /// Replace the live `[sop.approval]` config (for testing a mid-flight reload from
    /// other modules) - so a test can revoke a group membership while a quorum gate is
    /// parked and assert the earlier voter stops counting.
    #[cfg(test)]
    pub(crate) fn set_approval_config_for_test(
        &mut self,
        approval: clawcrew_config::schema::SopApprovalConfig,
    ) {
        self.config.approval = approval;
    }

    // ── Internal helpers ────────────────────────────────────────

    pub fn last_finished_run(&self, sop_name: &str) -> Option<&SopRun> {
        self.finished_runs
            .iter()
            .rev()
            .find(|r| r.sop_name == sop_name)
    }

    /// Attach the terminal cause to the run record itself, so a failed run
    /// carries its own explanation wherever it is read back from (the store,
    /// the Runs surface, `sop_status`) rather than only in a transcript that
    /// may not exist for daemon-driven SOPs. Only `Failed` gets a cause; other
    /// terminal statuses have nothing to explain.
    fn stamp_failure_reason(run: &mut SopRun, status: SopRunStatus, reason: Option<&str>) {
        if status == SopRunStatus::Failed {
            run.failure_reason = reason.map(str::to_string);
        }
    }

    /// Append the terminal `run_failed` audit row, mirroring the way the event
    /// log already explains gates and promotions. Emitted only after the
    /// terminal write succeeded, so a retained (failed-to-persist) run never
    /// leaves a phantom failure row behind.
    fn record_run_failed_event(&self, run_id: &str, status: SopRunStatus, reason: Option<&str>) {
        if status != SopRunStatus::Failed {
            return;
        }
        self.record_transition_event(
            run_id,
            "run_failed",
            reason.map(str::to_string),
            ::serde_json::json!({}),
        );
    }

    pub fn finish_run(
        &mut self,
        run_id: &str,
        status: SopRunStatus,
        reason: Option<String>,
    ) -> Result<SopRunAction> {
        let mut run = self
            .active_runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        run.status = status;
        run.completed_at = Some(now_iso8601());
        Self::stamp_failure_reason(&mut run, status, reason.as_deref());
        let sop_name = run.sop_name.clone();
        let run_id_owned = run.run_id.clone();
        self.persist_terminal(&run)?;
        self.record_run_failed_event(run_id, status, reason.as_deref());
        self.claims_pending_persist.remove(run_id);
        self.claims_retained_after_terminal_rollback.remove(run_id);
        self.cancellation_finalization_ready.remove(run_id);
        self.step_budget_finalization_ready.remove(run_id);
        self.active_runs.remove(run_id);
        self.metrics.record_run_complete(&run);
        // The park snapshot is purely a rehydration artifact: a terminal run must
        // not leave one behind claiming `paused_at_checkpoint`. Decisions and the
        // final status live in the run store / approval ledger, not the snapshot.
        self.remove_deterministic_state_file(&run);
        self.finished_runs.push(run);

        // Evict oldest finished runs when over capacity
        let max = self.config.max_finished_runs;
        if max > 0 && self.finished_runs.len() > max {
            let excess = self.finished_runs.len() - max;
            self.finished_runs.drain(..excess);
        }

        Ok(match status {
            SopRunStatus::Failed => SopRunAction::Failed {
                run_id: run_id_owned,
                sop_name,
                reason: reason.unwrap_or_default(),
            },
            SopRunStatus::Cancelled => SopRunAction::Cancelled {
                run_id: run_id_owned,
                sop_name,
            },
            _ => SopRunAction::Completed {
                run_id: run_id_owned,
                sop_name,
            },
        })
    }

    pub(crate) fn finish_run_with_gate_event(
        &mut self,
        run_id: &str,
        status: SopRunStatus,
        reason: Option<String>,
        event: &SopEventRecord,
    ) -> Result<SopRunAction> {
        let mut run = self
            .active_runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        run.status = status;
        run.completed_at = Some(now_iso8601());
        Self::stamp_failure_reason(&mut run, status, reason.as_deref());
        let sop_name = run.sop_name.clone();
        let run_id_owned = run.run_id.clone();
        self.persist_terminal_with_gate_event(&run, event)?;
        self.record_run_failed_event(run_id, status, reason.as_deref());
        self.claims_pending_persist.remove(run_id);
        self.claims_retained_after_terminal_rollback.remove(run_id);
        self.cancellation_finalization_ready.remove(run_id);
        self.step_budget_finalization_ready.remove(run_id);
        self.active_runs.remove(run_id);
        self.metrics.record_run_complete(&run);
        self.remove_deterministic_state_file(&run);
        self.finished_runs.push(run);

        let max = self.config.max_finished_runs;
        if max > 0 && self.finished_runs.len() > max {
            let excess = self.finished_runs.len() - max;
            self.finished_runs.drain(..excess);
        }

        Ok(match status {
            SopRunStatus::Failed => SopRunAction::Failed {
                run_id: run_id_owned,
                sop_name,
                reason: reason.unwrap_or_default(),
            },
            _ => SopRunAction::Completed {
                run_id: run_id_owned,
                sop_name,
            },
        })
    }

    pub(crate) fn clear_waiting_gate_with_event(
        &mut self,
        run_id: &str,
        event: &SopEventRecord,
    ) -> Result<SopRunAction> {
        let prior_run = self
            .active_runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        let action = match self.clear_waiting_gate(run_id) {
            Ok(transition) => match transition {
                GateClearTransition::Active { action, follow_up } => {
                    if let Err(e) = self.persist_active_with_gate_event(run_id, event) {
                        self.active_runs.insert(run_id.to_string(), prior_run);
                        self.release_claim_on_park(run_id);
                        return Err(e);
                    }
                    if let Some(follow_up) = follow_up {
                        self.record_gate_resolution_follow_up(run_id, follow_up);
                    }
                    *action
                }
                GateClearTransition::Terminal {
                    status,
                    reason,
                    follow_up,
                } => {
                    let action =
                        match self.finish_run_with_gate_event(run_id, status, reason, event) {
                            Ok(action) => action,
                            Err(e) => {
                                self.active_runs.insert(run_id.to_string(), prior_run);
                                self.release_claim_on_park(run_id);
                                return Err(e);
                            }
                        };
                    if let Some(follow_up) = follow_up {
                        self.record_gate_resolution_follow_up(run_id, follow_up);
                    }
                    action
                }
            },
            Err(e) => {
                self.active_runs.insert(run_id.to_string(), prior_run);
                self.release_claim_on_park(run_id);
                return Err(e);
            }
        };
        Ok(action)
    }

    // ── EPIC C: out-of-band approval plane ──────────────────────────

    /// Read-only config access for the approval resolver.
    pub fn config(&self) -> &SopConfig {
        &self.config
    }

    /// The live `[sop.approval]` config - the single source of truth for approval
    /// groups and policies. The broker resolves membership/policy from this at
    /// use-time rather than holding a cloned copy that could drift on reload.
    pub fn approval_config(&self) -> &clawcrew_config::schema::SopApprovalConfig {
        &self.config.approval
    }

    /// Fallible lookup for the approval policy that applies to the run's current
    /// parked step. `Ok(None)` means the step is intentionally unpoliced; `Err`
    /// means the live run/SOP/step state is unavailable and callers must fail
    /// closed rather than treating it as unpoliced.
    pub(crate) fn current_step_policy_lookup(&self, run_id: &str) -> Result<Option<String>> {
        let run = self
            .get_run(run_id)
            .ok_or_else(|| anyhow::Error::msg(format!("Active run not found: {run_id}")))?;
        let sop = self.get_sop(&run.sop_name).ok_or_else(|| {
            anyhow::Error::msg(format!("SOP '{}' no longer loaded", run.sop_name))
        })?;
        // Match the step by its `number`, NOT by vec position: routed / non-contiguous
        // step numbers mean position != number, and a positional lookup would read the
        // wrong step's policy (silently unpolicing a policied gate, or vice versa).
        let step = sop
            .steps
            .iter()
            .find(|s| s.number == run.current_step)
            .ok_or_else(|| {
                anyhow::Error::msg(format!(
                    "SOP '{}' no longer contains step {}",
                    run.sop_name, run.current_step
                ))
            })?;
        let Some(name) = step.policy.as_deref() else {
            return Ok(None);
        };
        let name = name.trim();
        // An empty/whitespace name means "no policy", same as the Markdown parser's
        // `policy:` bullet (mod.rs). Without this, a TOML `policy = ""` step would
        // deserialize as `Some("")` and the broker would treat it as a NAMED-but-absent
        // policy (fail closed, gate stuck waiting forever) instead of unpoliced -
        // diverging from the equivalent Markdown SOP, which normalizes to `None`.
        Ok((!name.is_empty()).then(|| name.to_string()))
    }

    /// The name of the approval policy that applies to the run's current step, if
    /// that step names one. Read surfaces collapse unavailable live state to
    /// `None`; the broker uses the fallible lookup above to fail closed.
    pub fn current_step_policy_name(&self, run_id: &str) -> Option<String> {
        self.current_step_policy_lookup(run_id).ok().flatten()
    }

    /// Classify a run's approval gate for `resolve_gate` (idempotency + typed
    /// not-found). `Running` (already approved) and terminal runs are
    /// `AlreadyResolved`; an unknown run or a non-`WaitingApproval` active status
    /// (e.g. a deterministic `PausedCheckpoint`, which `approve_step` owns) is
    /// `NotApplicable`.
    pub(crate) fn gate_state(&self, run_id: &str) -> GateState {
        if let Some(run) = self.active_runs.get(run_id) {
            match run.status {
                SopRunStatus::WaitingApproval => GateState::Waiting {
                    step: run.current_step,
                },
                SopRunStatus::Running => GateState::AlreadyResolved,
                _ => GateState::NotApplicable,
            }
        } else if self.finished_runs.iter().any(|r| r.run_id == run_id) {
            GateState::AlreadyResolved
        } else {
            GateState::NotApplicable
        }
    }

    /// Ordered event/ledger history for a run (from the durable store).
    pub fn run_events(&self, run_id: &str) -> Result<Vec<SopEventRecord>, StoreError> {
        self.store.list_events(run_id)
    }

    /// EPIC G (broker quorum): record an approver's vote on a still-waiting gate as
    /// an append-only ledger row (kind `gate_vote`, actor = the principal). Quorum is
    /// counted from these rows so votes are durable and survive a restart. Distinct
    /// from `gate_resolved`, which is appended only once the gate actually clears.
    ///
    /// IDEMPOTENT per `(run, step, policy, voter_key)`: a repeat vote by the same voter
    /// under the same policy is a no-op, so retries (e.g. an approver clicking twice
    /// while the gate is still pending quorum) do not grow the append-only log with
    /// duplicate rows. The count already dedups by `voter_key`, so this changes storage
    /// footprint, not the tally. A read failure is surfaced (fail-closed) rather than
    /// risking a duplicate append.
    pub(crate) fn record_gate_vote(
        &self,
        run_id: &str,
        step: u32,
        policy: &str,
        gate_revision: u32,
        principal: &super::approval::ApprovalPrincipal,
    ) -> Result<(), StoreError> {
        self.record_gate_vote_scoped(
            run_id,
            step,
            policy,
            Some(gate_revision),
            None,
            None,
            principal,
        )
    }

    /// Record a quorum vote for a deterministic checkpoint presentation. Checkpoint
    /// votes must be scoped tighter than approval-gate votes because the same step
    /// can be answered with materially different public-mutation decisions.
    pub(crate) fn record_checkpoint_gate_vote(
        &self,
        run_id: &str,
        step: u32,
        policy: &str,
        checkpoint_revision: u32,
        decision_label: &str,
        decision_identity: &str,
        principal: &super::approval::ApprovalPrincipal,
    ) -> Result<(), StoreError> {
        self.record_gate_vote_scoped(
            run_id,
            step,
            policy,
            Some(checkpoint_revision),
            Some(decision_label),
            Some(decision_identity),
            principal,
        )
    }

    fn record_gate_vote_scoped(
        &self,
        run_id: &str,
        step: u32,
        policy: &str,
        gate_revision: Option<u32>,
        decision_label: Option<&str>,
        decision_identity: Option<&str>,
        principal: &super::approval::ApprovalPrincipal,
    ) -> Result<(), StoreError> {
        let voter_key = principal.voter_key();
        if self.gate_votes_for_step(run_id, step)?.iter().any(|vote| {
            vote.voter_key == voter_key
                && vote.policy.as_deref() == Some(policy)
                && vote.gate_revision == gate_revision
                && vote.decision_identity.as_deref() == decision_identity
        }) {
            return Ok(());
        }
        let mut payload = serde_json::json!({
            "step": step,
            "source": principal.source_label(),
            "policy": policy,
            "identity": principal.identity,
        });
        if let Some(object) = payload.as_object_mut() {
            if let Some(revision) = gate_revision {
                object.insert(
                    "gate_revision".to_string(),
                    serde_json::Value::Number(revision.into()),
                );
                if decision_identity.is_some() {
                    object.insert(
                        "checkpoint_revision".to_string(),
                        serde_json::Value::Number(revision.into()),
                    );
                }
            }
            if let Some(label) = decision_label {
                object.insert(
                    "decision".to_string(),
                    serde_json::Value::String(label.to_string()),
                );
            }
            if let Some(identity) = decision_identity {
                object.insert(
                    "decision_identity".to_string(),
                    serde_json::Value::String(identity.to_string()),
                );
            }
        }
        let ev = SopEventRecord {
            run_id: run_id.to_string(),
            seq: 0,
            ts: now_iso8601(),
            kind: "gate_vote".to_string(),
            // `voter_key()` deliberately collapses `Http`/`Ws` to one canonical
            // `gateway:<id>` voter (same paired token, two transports = one voter),
            // while the agent/CLI sources stay distinct. See `ApprovalPrincipal::
            // voter_key`'s own doc for the full canonicalization rationale.
            actor: Some(voter_key),
            reason: None,
            // `policy` scopes the vote to the policy in effect when it was cast, and
            // `source`/`identity` capture enough to REVALIDATE the voter against the
            // current required group at count time - so a mid-flight policy or group
            // change cannot let a stale vote count toward the new quorum.
            payload,
        };
        self.store.append_event(&ev).map(|_| ())
    }

    /// EPIC G (broker quorum): the recorded approval votes on `run_id` AT `step`, read
    /// from the append-only `gate_vote` ledger rows. Each row carries the canonical
    /// `voter_key` (source-qualified, `Http`/`Ws` collapsed - see
    /// [`super::approval::ApprovalPrincipal::voter_key`]) plus the `policy` in effect
    /// when the vote was cast and the `source`/`identity` needed to REVALIDATE the
    /// voter against the current required group. The broker owns the tally (scope to
    /// the current policy, revalidate membership, then dedup by `voter_key`) because
    /// the policy/group/resolver live there; the engine only surfaces the durable rows.
    ///
    /// A read failure is SURFACED, never collapsed to an empty tally: an unreadable
    /// ledger must fail the resolve closed (gate stays waiting for a retry), not report
    /// a bogus zero quorum after a vote was durably appended.
    pub(crate) fn gate_votes_for_step(
        &self,
        run_id: &str,
        step: u32,
    ) -> Result<Vec<GateVote>, StoreError> {
        let events = self.store.list_events(run_id).map_err(|e| {
            ::clawcrew_log::record!(
                WARN,
                ::clawcrew_log::Event::new(module_path!(), ::clawcrew_log::Action::Note)
                    .with_outcome(::clawcrew_log::EventOutcome::Failure)
                    .with_attrs(::serde_json::json!({
                        "run_id": run_id,
                        "step": step,
                        "error": e.to_string(),
                    })),
                "SOP engine: quorum voter count could not read the gate ledger (fail-closed, gate stays waiting)"
            );
            e
        })?;
        let mut votes: Vec<GateVote> = Vec::new();
        for ev in events {
            if ev.kind == "gate_vote"
                && ev.payload.get("step").and_then(|s| s.as_u64()) == Some(u64::from(step))
                && let Some(voter_key) = ev.actor
            {
                let str_field = |k: &str| {
                    ev.payload
                        .get(k)
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                };
                votes.push(GateVote {
                    voter_key,
                    policy: str_field("policy"),
                    source: str_field("source"),
                    identity: str_field("identity"),
                    gate_revision: ev
                        .payload
                        .get("gate_revision")
                        .or_else(|| ev.payload.get("checkpoint_revision"))
                        .and_then(|value| value.as_u64())
                        .and_then(|value| u32::try_from(value).ok()),
                    checkpoint_revision: ev
                        .payload
                        .get("checkpoint_revision")
                        .and_then(|value| value.as_u64())
                        .and_then(|value| u32::try_from(value).ok()),
                    decision_identity: str_field("decision_identity"),
                });
            }
        }
        Ok(votes)
    }

    /// Record the approval completion metric at the gate-clearing chokepoint, so
    /// every principal (agent tool, CLI, gateway, WS, timeout) meters identically
    /// and the live counters agree with `SopMetricsCollector::rebuild_from_persistence`.
    /// `is_system` (the timeout principal) is metered as a timeout auto-approval;
    /// any other principal is a human approval. No-op if the run is gone.
    pub(crate) fn record_approval_metric(&self, run_id: &str, is_system: bool) {
        let Some(run) = self.get_run(run_id) else {
            return;
        };
        if is_system {
            self.metrics
                .record_timeout_auto_approve(&run.sop_name, &run.run_id);
        } else {
            self.metrics.record_approval(&run.sop_name, &run.run_id);
        }
    }

    pub fn resolve_gate(
        &mut self,
        run_id: &str,
        decision: super::approval::ApprovalDecision,
        principal: super::approval::ApprovalPrincipal,
    ) -> Result<super::approval::ResolveOutcome> {
        super::approval::resolve::resolve_gate(self, run_id, decision, principal)
    }
}

/// A recorded approval vote on a waiting gate (one `gate_vote` ledger row), as
/// surfaced by [`SopEngine::gate_votes_for_step`]. The broker scopes the tally to
/// the current `policy`, revalidates each voter (`source` + `identity`) against the
/// current required group, then dedups by `voter_key`.
pub(crate) struct GateVote {
    /// Canonical quorum-distinctness key (`Http`/`Ws` collapsed to `gateway`).
    pub voter_key: String,
    /// The `[sop.approval].policies.<name>` in effect when the vote was cast, or
    /// `None` for a vote recorded before this field existed (never counts toward a
    /// named current policy).
    pub policy: Option<String>,
    /// The voter's transport source label (`http`/`ws`/`cli`/`agent`), for membership
    /// revalidation.
    pub source: Option<String>,
    /// The voter's recorded identity (paired-token subject / agent alias / OS user),
    /// for membership revalidation. Recorded, not trusted.
    pub identity: Option<String>,
    /// Gate presentation revision used to prevent stale votes from a prior visit.
    pub gate_revision: Option<u32>,
    /// Checkpoint presentation revision, absent for ordinary approval-gate votes.
    pub checkpoint_revision: Option<u32>,
    /// Canonical hash identifying the exact positive checkpoint decision payload.
    pub decision_identity: Option<String>,
}

/// Classification of a run's approval-gate state (EPIC C `resolve_gate`).
pub(crate) enum GateState {
    /// Waiting on approval at this step number (resolvable).
    Waiting { step: u32 },
    /// Already resolved (running after approve, or terminal) - idempotent no-op.
    AlreadyResolved,
    /// Not a waiting-approval gate (unknown run, or a non-WaitingApproval status
    /// such as a deterministic `PausedCheckpoint`, which `approve_step` owns).
    NotApplicable,
}

// ── Trigger matching ────────────────────────────────────────────

/// Check whether a single trigger definition matches an incoming event.
///
/// Source class is the cheap gate: a trigger can only match an event from its
/// own source. Past that, matching is the trigger's own responsibility via its
/// `TriggerBehavior`, so there is no per-source logic to drift here.
fn trigger_matches(trigger: &SopTrigger, event: &SopEvent) -> bool {
    trigger.source() == event.source && trigger.behavior().matches(event)
}

/// Match a channel trigger against an event topic. Two producer forms are
/// accepted through the shared [`ChannelSopTopic`] grammar: the plain
/// `channel` / `channel/alias` form used by agent-loop message triggers, and
/// the forge form `channel.alias:event_type`. Channel type compares
/// case-insensitively; an aliased trigger requires an exact alias, an
/// alias-less trigger matches any instance. No topic fails closed. The
/// `event_type` (forge form) is left for an authored `condition` to match.
pub(crate) fn channel_trigger_topic_matches(
    channel: &str,
    alias: Option<&str>,
    topic: Option<&str>,
) -> bool {
    let Some(topic) = topic else {
        return false;
    };
    let (topic_channel, topic_alias, _event_type) =
        clawcrew_api::channel::ChannelSopTopic::parse(topic);
    if !topic_channel.eq_ignore_ascii_case(channel) {
        return false;
    }
    match alias {
        Some(a) => topic_alias.is_some_and(|ta| ta == a),
        None => true,
    }
}

pub(crate) fn calendar_trigger_matches(
    calendar_source: &str,
    calendar_ids: &[String],
    event: &SopEvent,
) -> bool {
    if event.topic.as_deref() != Some(CALENDAR_NO_SHOW_TOPIC) {
        return false;
    }

    let Some(payload) = event.payload.as_deref() else {
        return false;
    };
    let Ok(payload) = serde_json::from_str::<CalendarNoShowEvent>(payload) else {
        return false;
    };

    if payload.calendar_source != calendar_source {
        return false;
    }

    if calendar_ids.is_empty() {
        return true;
    }

    calendar_ids.iter().any(|id| id == &payload.calendar_id)
}

/// Simple MQTT topic matching with `+` (single-level) and `#` (multi-level) wildcards.
pub(crate) fn mqtt_topic_matches(pattern: &str, topic: &str) -> bool {
    let pat_parts: Vec<&str> = pattern.split('/').collect();
    let top_parts: Vec<&str> = topic.split('/').collect();

    let mut pi = 0;
    let mut ti = 0;

    while pi < pat_parts.len() && ti < top_parts.len() {
        match pat_parts[pi] {
            "#" => return true, // multi-level wildcard matches everything remaining
            "+" => {
                // single-level wildcard matches one segment
                pi += 1;
                ti += 1;
            }
            seg => {
                if seg != top_parts[ti] {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }

    // Both must be fully consumed (unless pattern ended with #)
    pi == pat_parts.len() && ti == top_parts.len()
}

/// AMQP topic-exchange routing-key matching. Keys are `.`-delimited words;
/// `*` matches exactly one word and `#` matches zero or more words. A `#` that
/// can absorb zero segments is what distinguishes this from MQTT matching.
pub(crate) fn amqp_routing_key_matches(pattern: &str, key: &str) -> bool {
    let pat: Vec<&str> = pattern.split('.').collect();
    let words: Vec<&str> = key.split('.').collect();
    amqp_match_from(&pat, &words)
}

fn amqp_match_from(pat: &[&str], words: &[&str]) -> bool {
    match pat.first() {
        None => words.is_empty(),
        Some(&"#") => (0..=words.len()).any(|skip| amqp_match_from(&pat[1..], &words[skip..])),
        Some(&"*") => !words.is_empty() && amqp_match_from(&pat[1..], &words[1..]),
        Some(seg) => {
            !words.is_empty() && *seg == words[0] && amqp_match_from(&pat[1..], &words[1..])
        }
    }
}

/// Glob match a filesystem trigger `pattern` against a normalized `path`,
/// supporting `*` (single segment) and `**` (recursive) wildcards via the
/// `glob` crate. A bare directory pattern also matches paths nested beneath it.
pub(crate) fn filesystem_path_matches(pattern: &str, path: &str) -> bool {
    if let Ok(compiled) = glob::Pattern::new(pattern)
        && compiled.matches(path)
    {
        return true;
    }
    let prefix = pattern.trim_end_matches('/');
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// Whether the payload's `event` field names one of the trigger's listed kinds.
pub(crate) fn filesystem_event_listed(
    events: &[FilesystemEventKind],
    payload: Option<&str>,
) -> bool {
    let Some(payload) = payload else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return false;
    };
    let Some(kind) = value.get("event").and_then(|e| e.as_str()) else {
        return false;
    };
    events.iter().any(|e| e.to_string() == kind)
}

// ── Execution mode resolution ───────────────────────────────────

fn execution_mode_needs_approval(mode: SopExecutionMode, sop: &Sop, step: &SopStep) -> bool {
    match mode {
        // Deterministic mode is handled via start_deterministic_run;
        // if we reach here via the standard path, treat as Auto.
        SopExecutionMode::Auto | SopExecutionMode::Deterministic => false,
        SopExecutionMode::Supervised => {
            // Supervised: approval only before the first step
            step.number == 1
        }
        SopExecutionMode::StepByStep => true,
        SopExecutionMode::PriorityBased => match sop.priority {
            // [SEC-FLIP] Critical/High are the MOST dangerous runs, so they MUST
            // gate (was `=> false`, an inversion that auto-ran the riskiest SOPs).
            SopPriority::Critical | SopPriority::High => true,
            SopPriority::Normal | SopPriority::Low => {
                // Supervised behavior for normal/low
                step.number == 1
            }
        },
    }
}

fn step_requires_approval_gate(sop: &Sop, step: &SopStep) -> bool {
    if step.requires_confirmation {
        return true;
    }

    let effective_mode = step.mode.unwrap_or(sop.execution_mode);
    execution_mode_needs_approval(sop.execution_mode, sop, step)
        || execution_mode_needs_approval(effective_mode, sop, step)
}

fn pending_step_blocks_direct_advance(sop: &Sop, step: &SopStep) -> bool {
    step.kind == SopStepKind::Checkpoint || step_requires_approval_gate(sop, step)
}

/// Determine the action for a step based on the effective execution mode.
fn resolve_step_action(sop: &Sop, step: &SopStep, run_id: String, context: String) -> SopRunAction {
    let mut step = step.clone();
    step.agent = step
        .effective_agent(sop.agent.as_deref())
        .map(str::to_string);
    let step = &step;

    if step_requires_approval_gate(sop, step) {
        SopRunAction::WaitApproval {
            run_id,
            step: step.clone(),
            context,
        }
    } else {
        SopRunAction::ExecuteStep {
            run_id,
            step: step.clone(),
            context,
        }
    }
}

// ── Step context formatting ─────────────────────────────────────

/// Build the structured context message that gets injected into the agent.
fn format_step_context(sop: &Sop, run: &SopRun, step: &SopStep, config: &SopConfig) -> String {
    let mut ctx = format!(
        "[SOP: {} (run {}) — Step {} of {}]\n\n",
        sop.name, run.run_id, step.number, run.total_steps
    );

    let marker_id = if run.frame_marker_id.is_empty() {
        run.run_id.as_str()
    } else {
        run.frame_marker_id.as_str()
    };
    ctx.push_str(&ContentSafety::from_sop_config(config).frame_for_context(
        run.trigger_event.payload.as_deref(),
        run.trigger_event.topic.as_deref(),
        run.trigger_event.source,
        marker_id,
    ));

    // Previous step summary
    if let Some(prev) = run.step_results.last() {
        let _ = writeln!(
            ctx,
            "Previous: Step {} {} — {}",
            prev.step_number, prev.status, prev.output
        );
    }

    let _ = write!(ctx, "\nCurrent step: **{}**\n{}\n", step.title, step.body);

    if !step.suggested_tools.is_empty() {
        let _ = write!(
            ctx,
            "\nSuggested tools: {}\n",
            step.suggested_tools.join(", ")
        );
    }

    ctx.push_str("\nWhen done, report your result.\n");

    ctx
}

pub(crate) fn step_input_value(run: &SopRun, step_number: u32) -> Value {
    if step_number <= 1 {
        return run
            .trigger_event
            .payload
            .as_deref()
            .map(jsonish_value)
            .unwrap_or(Value::Null);
    }

    run.step_results
        .last()
        .map(step_result_value)
        .unwrap_or(Value::Null)
}

/// Gate re-presentations per checkpoint a `Revise` may spend before the gate
/// insists on approve / edit / deny. Bounds operator-driven model spend.
pub(crate) const MAX_GATE_REVISIONS: u32 = 3;

/// The input that fed `step_number` when it originally ran: the output of the
/// step completed immediately BEFORE it in EXECUTION order (`step_results` is
/// append-only, so vec order IS execution order — numeric order would lie under
/// `Goto` routing), or the trigger payload when nothing ran before it. Used to
/// replay a step (a gate `Revise` re-draft) with exactly what it saw the first
/// time.
pub(crate) fn replay_input_for_step(run: &SopRun, step_number: u32) -> Value {
    let executed_at = run
        .step_results
        .iter()
        .rposition(|r| r.step_number == step_number && r.status == SopStepStatus::Completed);
    executed_at
        .and_then(|idx| {
            run.step_results[..idx]
                .iter()
                .rev()
                .find(|r| r.status == SopStepStatus::Completed)
                .map(step_result_value)
        })
        .unwrap_or_else(|| {
            run.trigger_event
                .payload
                .as_deref()
                .map(jsonish_value)
                .unwrap_or(Value::Null)
        })
}

fn retry_input_value(run: &SopRun, step_number: u32) -> Value {
    if step_number <= 1 {
        return run
            .trigger_event
            .payload
            .as_deref()
            .map(jsonish_value)
            .unwrap_or(Value::Null);
    }

    run.step_results
        .iter()
        .rev()
        .find(|result| {
            result.status == SopStepStatus::Completed && result.step_number != step_number
        })
        .map(step_result_value)
        .unwrap_or(Value::Null)
}

fn step_result_value(result: &SopStepResult) -> Value {
    jsonish_value(&result.output)
}

fn declared_step_output_value(step: &SopStep, raw: &str) -> Value {
    let schema = step
        .schema
        .as_ref()
        .and_then(|schema| schema.output.as_ref());
    super::rundata::parse_step_output_value(raw, schema)
}

fn forge_comment_input_matches_checkpoint_output(
    input: &Value,
    checkpoint_result: &SopStepResult,
) -> bool {
    let Ok(target) = super::capability::resolve_forge_comment_target(input) else {
        return false;
    };
    let approved = step_result_value(checkpoint_result);
    let Some(approved) = approved.as_object() else {
        return false;
    };
    let approved_repo = approved
        .get("repo")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|repo| !repo.is_empty());
    let approved_number = approved.get("number").and_then(Value::as_u64);
    let approved_body = approved.get("body").and_then(Value::as_str);
    let approved_channel = approved
        .get("channel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|channel| !channel.is_empty());
    let channel_matches = match target.channel {
        Some(channel) => approved_channel == Some(channel),
        None => true,
    };

    approved_repo == Some(target.repo)
        && approved_number == Some(target.number)
        && approved_body == Some(target.body)
        && channel_matches
}

fn jsonish_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

// ── Utilities ───────────────────────────────────────────────────

pub fn now_iso8601() -> String {
    // Use chrono if available, otherwise fallback to SystemTime
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // Simple UTC timestamp without chrono dependency
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Days since epoch to Y-M-D (simplified — good enough for run IDs)
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    days += 719_468;
    let era = days / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// A1: whether a run in `active_runs` currently occupies an execution slot (holds
/// a store CAS claim). A run parked at a HITL approval / deterministic checkpoint
/// releases its claim on park, so it does NOT hold a slot; every other non-terminal
/// status does. Keeps the in-memory admission fallback aligned with the store's
/// `claim_counts`, which counts only live (executing) claims.
fn holds_exec_claim(status: SopRunStatus) -> bool {
    !matches!(
        status,
        SopRunStatus::WaitingApproval | SopRunStatus::PausedCheckpoint
    )
}

/// Check if enough time has elapsed since a timestamp string.
fn cooldown_elapsed(completed_at: &str, cooldown_secs: u64) -> bool {
    // Parse the ISO-8601 timestamp we generate
    let completed = parse_iso8601_secs(completed_at);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match completed {
        Some(ts) => now.saturating_sub(ts) >= cooldown_secs,
        None => true, // Can't parse timestamp; allow start
    }
}

/// Minimal ISO-8601 parser returning seconds since epoch.
fn parse_iso8601_secs(input: &str) -> Option<u64> {
    // Expected format: YYYY-MM-DDTHH:MM:SSZ
    let input = input.trim_end_matches('Z');
    let parts: Vec<&str> = input.split('T').collect();
    if parts.len() != 2 {
        return None;
    }
    let date_parts: Vec<u64> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    let time_parts: Vec<u64> = parts[1].split(':').filter_map(|p| p.parse().ok()).collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return None;
    }
    let (year, month, day) = (date_parts[0], date_parts[1], date_parts[2]);
    let (hour, min, sec) = (time_parts[0], time_parts[1], time_parts[2]);

    // Reverse of days_to_ymd: compute days since epoch
    let year_adj = if month <= 2 { year - 1 } else { year };
    let month_adj = if month > 2 { month - 3 } else { month + 9 };
    let era = year_adj / 400;
    let yoe = year_adj - era * 400;
    let doy = (153 * month_adj + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

#[cfg(test)]
mod tests;

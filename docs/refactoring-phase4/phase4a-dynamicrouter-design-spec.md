# Phase 4a Design Spec — DynamicRouter Architecture

Status: DRAFT, for Principle architecture-gate review.
Scope: closes the 5 spec gaps blocking Phase 4 Backend start (monolith split + PluginRegistry).

## 0. Ground truth check (read before anything below)

Before writing this, I read the actual repo instead of designing from the brief in a vacuum. Two facts change the brief materially:

1. **`crates/clawcrew-routing/src/lib.rs` already exists.** It defines `RouteHandler` (async `handle`, `path_pattern`, `http_method`, `name`, `capability`) and `DynamicRouter` (`HashMap<String, Box<dyn RouteHandler>>` behind `tokio::sync::RwLock`, `register()` that rejects on path collision, optional `fallback`, `dispatch()`). It has 3 passing unit tests (register, collision-reject, no-match). **It is not wired into anything** — grep across the repo finds it only in its own `Cargo.toml` and the lockfile. No binary depends on it.
2. **`PluginCapability` already exists** in `crates/clawcrew-plugins/src/lib.rs` as exactly `Tool | Channel | Memory | Observer | Skill`. The brief's Gap 1 "define a capability enum" is a reuse decision, not a new design.
3. **Ed25519 signature verification already exists**: `crates/clawcrew-plugins/src/signature.rs` (`SignatureMode::{Strict,Permissive,Disabled}`, `VerificationResult`), used for the WASM *manifest* signature. It is unrelated to in-process route dispatch and needs no changes for this spec.
4. `crates/clawcrew-plugins/src/registry.rs` is the **install/distribution** registry (name/version/URL/sha256 index for fetching plugins) — a different concern from the in-memory `DynamicRouter` route table. Do not conflate the two "registry" words.
5. `clawcrew-gateway/src/lib.rs` is the real handler barrel: 49 `pub mod` declarations (not 39 — see Gap 5). `security_headers::apply` is a working `axum::middleware::Next` layer; `auth_rate_limit::AuthRateLimiter` is a standalone per-IP sliding-window struct consumed at the auth call site, not yet a tower `Layer`.

Net effect: Phase 4a is **"finish and wire an existing 150-line prototype,"** not "design DynamicRouter from zero." This changes risk (lower — core logic is already tested) and changes Gap 1/3 from "design" to "review + extend."

---

## Gap 1 — RouteHandler trait

**Decision: keep the existing trait as-is, extend `capability()` to return the real enum instead of `Option<&str>`.**

Current signature (`clawcrew-routing/src/lib.rs`):

```rust
#[async_trait::async_trait]
pub trait RouteHandler: Send + Sync + std::fmt::Debug {
    async fn handle(&self, req: Request) -> Result<Response, String>;
    fn path_pattern(&self) -> &str;
    fn http_method(&self) -> &str;
    fn name(&self) -> &str;
    fn capability(&self) -> Option<&str> { None }   // <-- change below
}
```

**Change required**: `capability()` returns `Option<&str>` (free-form string). This is a stringly-typed authorization hook — the kind of thing that silently drifts (`"tool"` vs `"Tool"` vs typo). Reuse the existing enum instead:

```rust
// clawcrew-routing/src/lib.rs
use clawcrew_plugins::PluginCapability;

pub trait RouteHandler: Send + Sync + std::fmt::Debug {
    async fn handle(&self, req: Request) -> Result<Response, String>;
    fn path_pattern(&self) -> &str;
    fn http_method(&self) -> &str;
    fn name(&self) -> &str;
    /// Capability this route exercises, for authz/discovery. `None` = core
    /// infra route with no plugin-capability equivalent (e.g. /api/config).
    fn capability(&self) -> Option<PluginCapability> { None }
}
```

This adds a `clawcrew-routing -> clawcrew-plugins` dependency edge. `clawcrew-plugins` has no dependency back on `clawcrew-routing`, so no cycle.

**Middleware encoding**: the trait does **not** encode auth/rate-limit requirements as trait methods (e.g. no `fn requires_auth() -> bool`). Reasoning: auth and rate-limit are cross-cutting and already implemented as axum middleware (`security_headers::apply`, `AuthRateLimiter`) applied at the router layer, ahead of dispatch — see Gap 4. Putting auth flags on every handler impl duplicates a decision that belongs in one place (the middleware stack) and invites a handler that forgets to declare itself. The one exception: a plugin-provided route that needs a *stricter* rate limit than the global default declares it via `PluginManifest.permissions` (already exists) at registration time, enforced in `register()` — see Gap 3.

**Reference impl** — reuse of a real existing handler (`api_config.rs`'s config-get), adapted to the trait:

```rust
// clawcrew-gateway/src/api_config.rs (adapter, not a rewrite of the handler body)
#[derive(Debug)]
pub struct ConfigApiHandler {
    state: AppState,
}

#[async_trait::async_trait]
impl RouteHandler for ConfigApiHandler {
    async fn handle(&self, req: Request) -> Result<Response, String> {
        // delegates to the existing get_config/set_config axum handler fns —
        // no logic duplication, this is a thin adapter.
        crate::api_config::dispatch(self.state.clone(), req)
            .await
            .map_err(|e| e.to_string())
    }
    fn path_pattern(&self) -> &str { "/api/config" }
    fn http_method(&self) -> &str { "GET" }
    fn name(&self) -> &str { "api_config" }
    fn capability(&self) -> Option<PluginCapability> { None } // core infra, immutable (Gap 2)
}
```

---

## Gap 2 — Plugin loading boundary

```
┌─────────────────────────────┐  ┌──────────────────────────────┐  ┌───────────────────────────┐
│  COMPILE-TIME (feature-gated)│  │  RUNTIME (WASM, wasmtime)     │  │  CORE ROUTE PROTECTION     │
│                              │  │                                │  │                            │
│  Existing gateway features:  │  │  Existing crate:               │  │  Immutable paths (never    │
│  - channel-linq              │  │  clawcrew-plugins (feature     │  │  overridable by any        │
│  - channel-nextcloud         │  │  "plugins-wasmtime")           │  │  plugin, compile- or       │
│  - channel-whatsapp-cloud    │  │                                │  │  runtime-registered):      │
│  - webauthn, a2a,            │  │  - component.rs: wasmtime      │  │                            │
│    gateway-voice-duplex,     │  │    component instantiation     │  │   /api/config              │
│    plugins-wasm              │  │  - signature.rs: Ed25519       │  │   /api/logs                │
│                              │  │    verify (REUSE, no change)   │  │   /api/webauthn/*          │
│  These are cfg(feature=...)  │  │  - host.rs: WASI host imports  │  │   /auth/* (auth_rate_limit)│
│  modules in clawcrew-gateway,│  │  - wasm_tool / wasm_channel /  │  │   /api/plugins/* (mgmt)    │
│  compiled INTO the gateway   │  │    wasm_memory: capability-    │  │                            │
│  binary. NOT a separate      │  │    scoped adapters             │  │  Enforcement: REGISTRATION │
│  plugin-crate concept — the  │  │                                │  │  TIME, not dispatch-time.  │
│  brief's "plugin-telegram /  │  │  Signature policy: reuse       │  │  DynamicRouter.register()  │
│  plugin-slack" framing does  │  │  SignatureMode (existing).     │  │  is seeded with these      │
│  not match the codebase —   │  │  Strict in prod, Permissive/   │  │  paths at construction     │
│  there is no crates/plugin-* │  │  Disabled in dev only.         │  │  (owned by core, never via │
│  directory today. Treat     │  │                                │  │  plugin manifest), and     │
│  "compile-time plugin" as    │  │  Dependency rule: a WASM       │  │  register() already        │
│  "feature-gated core module",│  │  component talks to the host   │  │  rejects path collision —  │
│  not a 3rd-party crate.      │  │  ONLY through the WIT-defined  │  │  so a plugin registering   │
│                              │  │  imports (egress, config,      │  │  /api/config errors out,   │
│  Can a feature-gated module  │  │  secrets, memory) — it CANNOT  │  │  it does not get silently  │
│  depend on clawcrew-api?     │  │  depend on clawcrew-api as a   │  │  rejected-then-ignored.    │
│  YES — these are first-party │  │  Rust crate; it is a sandboxed │  │  Rationale over            │
│  modules inside the gateway  │  │  binary with no Rust ABI       │  │  dispatch-time whitelist:  │
│  crate, same as every other  │  │  access.                       │  │  fail fast at plugin-      │
│  handler. Not a 3rd-party     │  │                                │  │  install time, not on the │
│  dependency question.        │  │                                │  │  first live request.       │
└─────────────────────────────┘  └──────────────────────────────┘  └───────────────────────────┘
```

**Open item for Principle**: the brief assumed a `crates/plugin-telegram`-style compile-time plugin crate exists or is planned. It does not exist today. If Phase 4 needs that structure (first-party code split into separately-versioned crates), that is a *new* decision — flag as a one-way-door scope question, not folded silently into this spec. Recommendation: **do not build it now**; feature-gating inside `clawcrew-gateway` already achieves the stated goal (optional compilation) with zero new crates.

---

## Gap 3 — Collision handling + rate-limit placement + lock strategy

| Decision point | Choice | Rationale |
|---|---|---|
| **Collision detection** | (a) Reject registration with an error + log warning — **already implemented**, verified in `clawcrew-routing`'s `test_register_collision` (asserts `Err`, router length stays 1). | A silently-overridden core route is a security hole (plugin shadows `/api/config`). A silent "keep both" is undefined dispatch order. Reject-and-log is the only choice that fails loud at install time instead of at request time. No code change needed here — carry the existing behavior forward. |
| **Rate-limit on failed routes** | (b) In dispatch logic, after failed lookup — specifically: `DynamicRouter::dispatch()` increments a per-source-IP miss counter *before* falling to the fallback handler, so repeated 404-probing (route enumeration) is throttled independent of the global `AuthRateLimiter` (which only guards `/auth/*`). | Placing it in global middleware (a) would rate-limit legitimate traffic uniformly, including hits. Per-plugin quota at registration (c) does not address scanning/enumeration abuse, which happens at dispatch against *unregistered* paths. The existing `AuthRateLimiter` sliding-window struct is reused as the implementation (`crates/clawcrew-gateway/src/rate_limit.rs::SlidingWindowRateLimiter` already exists for this exact shape — reuse it, do not write a second limiter). |
| **RwLock contention strategy** | 1 global `HashMap` behind `RwLock` — **already implemented**, keep it. Do not split into per-capability registries. | Route registration is a cold-path (plugin install/startup), dispatch is a read-lock (`routes.read().await`) that `tokio::sync::RwLock` services concurrently with no writer contention in steady state. Splitting into 5 per-capability maps multiplies lock sites for zero measured benefit — ponytail: this is premature optimization against a problem (lock contention) that has not been observed, and the existing prototype's own benchmark-free design already made this call correctly. Revisit ONLY if profiling under load shows read-lock contention (ceiling noted below). |

`ponytail:` per-capability sharding deferred — single `RwLock<HashMap>` is correct until a load test shows read contention; upgrade path is sharding by capability if/when that happens, not before.

---

## Gap 4 — Middleware pipeline

```
Incoming request
      │
      ▼
┌─────────────────────────┐  GLOBAL, every request
│ security_headers::apply  │  (existing axum::middleware::Next layer,
└─────────────────────────┘   clawcrew-gateway/src/security_headers.rs)
      │
      ▼
┌─────────────────────────┐  GLOBAL, /auth/* and any route a handler
│ auth_rate_limit check    │  marks as auth-sensitive via its own logic
└─────────────────────────┘  (existing AuthRateLimiter; NOT a trait flag — see Gap 1)
      │
      ▼
┌─────────────────────────┐  GLOBAL, standard request/response trace
│ tracing/logging layer    │  (clawcrew-log crate, existing)
└─────────────────────────┘
      │
      ▼
┌─────────────────────────┐  PER-ROUTE: DynamicRouter.dispatch(req)
│ DynamicRouter dispatch   │  - exact path_pattern match -> handler.handle()
│                          │  - miss -> dispatch-time rate-limit check (Gap 3)
│                          │  - miss + no fallback -> 404
└─────────────────────────┘
      │
      ▼
┌─────────────────────────┐  GLOBAL, response-side only
│ response logging/metrics │  (clawcrew-log, existing)
└─────────────────────────┘
      │
      ▼
   Response
```

**Per-route vs global**: auth and security headers are global (every request pays the cost, correctness depends on uniform application). A plugin cannot inject its own middleware ahead of these — `RouteHandler::handle()` runs *inside* the dispatch stage, after global middleware has already run. This is deliberate: a plugin that could prepend middleware could bypass auth for its own route.

**Axum integration**: `DynamicRouter` is not `.route("/*", ...)` per path — it is a single axum handler mounted as the **fallback** of the existing `Router` (`.fallback(dynamic_dispatch_handler)`), so statically-defined axum routes (if any remain) still take precedence, and everything else funnels through one dispatch function that calls `DynamicRouter::dispatch()`. This matches the existing `fallback: Option<Box<dyn RouteHandler>>` field already in the prototype — that field's role is the *router's own* fallback (unregistered-path handler), one level below axum's fallback.

---

## Gap 5 — Core handler migration plan

**Correction to the brief**: the brief says "39 core handlers." The actual count in `crates/clawcrew-gateway/src/lib.rs` is **49** `pub mod` declarations (some `#[cfg(feature = ...)]`-gated, so the *compiled* count varies by feature set — a default build compiles fewer). Full list, copied from the barrel:

```
state, gateway, rate_limit,                      -- infra, not routes
a2a (feature "a2a"), acp, agent_owned_state,
api, api_backup, api_browse, api_config, api_logs,
api_pairing, api_personality, api_plugins (feature "plugins-wasm"),
api_quickstart, api_sections, api_skills, api_sop,
api_sop_author, api_sop_webhook (private), api_tasks,
api_upload, api_webauthn (feature "webauthn"),
api_webhook (feature channel-linq|nextcloud|whatsapp-cloud),
auth_rate_limit, canvas, hardware_context, node_tool, nodes,
openapi, plugin_webhook (private, feature "plugins-wasm"),
security_headers, session_queue, sse, static_files, tls,
version, grpc_system_gateway,
voice_duplex (feature "gateway-voice-duplex"),
webhook_ingress (private, feature channel-linq|nextcloud|whatsapp-cloud),
api_audit, api_providers, ws, ws_approval, ws_sop_runs
```

Of these, `state`, `gateway`, `rate_limit` are infrastructure (not HTTP routes) and are out of scope for `RouteHandler` migration. That leaves **~41 route-bearing modules** (count varies with feature flags), not 39 — close enough that the brief's estimate was reasonable, but Principle should sign off on the corrected number, not the stated one.

**Migration phasing: phased, not all-at-once.**

- **Phase 1 — security-critical core (5 modules, immutable per Gap 2):** `api_config`, `api_logs`, `auth_rate_limit`, `api_webauthn`, `api_audit`. These are the paths that must never be shadowable by a plugin, so migrating them first means collision-rejection is exercised against real core routes from day one, not retrofitted later.
- **Phase 2 — remaining `api_*` handlers (~15 modules):** `api_backup`, `api_browse`, `api_pairing`, `api_personality`, `api_plugins`, `api_quickstart`, `api_sections`, `api_skills`, `api_sop`, `api_sop_author`, `api_tasks`, `api_upload`, `api_webhook`, `api_providers`, `grpc_system_gateway`. These are independent of each other, so they can migrate in any order and in parallel across BE crew members without file collisions (each is already its own file).
- **Phase 3 — protocol/transport handlers (~10 modules):** `acp`, `a2a`, `canvas`, `hardware_context`, `node_tool`, `nodes`, `sse`, `tls`, `version`, `voice_duplex`, `ws`, `ws_approval`, `ws_sop_runs`, `webhook_ingress`, `plugin_webhook`. Deferred because these carry stateful protocol semantics (websocket upgrade, SSE streams) that interact with axum's extractor machinery more than a plain request/response handler does — migrating them needs the `RouteHandler::handle()` signature validated against streaming responses first (open question below), not assumed to just work from Phase 1/2 experience.
- **`static_files`, `openapi`, `security_headers`, `session_queue`**: stay as axum middleware/static-serve, **not migrated** — these are not per-plugin-overridable routes by nature (static file serving and OpenAPI schema generation aren't "handlers" in the plugin sense).

**Rationale for phased over all-at-once**: `DynamicRouter` is untested under axum's streaming/websocket extractors (its current tests all use `Body::empty()`/`Body::from(&str)`). All-at-once migration of all 41 modules risks discovering a streaming-incompatibility on the critical path with everything already moved. Phased migration surfaces that risk in Phase 3, after Phase 1/2 have already de-risked the common case.

**Open question for Backend to resolve during Phase 1**: does `RouteHandler::handle(&self, req: Request) -> Result<Response, String>` support an axum `Response` whose body is a `Body::from_stream` (SSE/WS upgrade)? The trait signature returns `Response` generically so it should, but this has not been exercised by the existing prototype's tests — first thing Phase 1 should add a regression test for before Phase 3 relies on it.

---

## Summary

**Timeline**: Phase 1 (5 handlers) is the gate — once collision-rejection + capability enum change are proven against real core routes, Phase 2 (15 handlers, parallelizable across BE crew) and Phase 3 (13 handlers, streaming-risk) follow. Estimate: Phase 1 = 1 PR cycle, Phase 2 = 2-3 PR cycles (parallel), Phase 3 = 2 PR cycles (serialized, streaming test first).

**Risk**: lower than the brief implied, because `DynamicRouter` core logic (register/collision/dispatch/fallback) is already written and unit-tested — this is an integration task, not greenfield. The actual unknowns are (1) streaming-response compatibility (Phase 3), and (2) whether Principle wants a real compile-time plugin-crate split (flagged above as a separate one-way-door decision, not assumed).

**Rollback plan**: each phase is additive — `DynamicRouter` is mounted as axum's `.fallback()`, so un-migrated handlers keep working on the existing static route table untouched. Rolling back a phase means removing that phase's `register()` calls and restoring the handler's static `.route()` entry; no handler logic is rewritten during migration (only wrapped in a thin `RouteHandler` adapter per Gap 1's example), so rollback is a revert of the adapter + registration call, not a code rewrite.

**Decisions requiring Principle sign-off before Backend starts**:
1. Capability enum reuse (Gap 1) — approve changing `capability() -> Option<&str>` to `Option<PluginCapability>`.
2. No compile-time plugin-crate split for now (Gap 2) — approve deferring that structure.
3. Corrected handler count (41 route-bearing modules, not 39) and the 3-phase migration order (Gap 5).

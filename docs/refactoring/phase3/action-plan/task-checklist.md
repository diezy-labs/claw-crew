# Claw-Crew Phase 3 — Action Plan: Granular Task Checklist

> **Status:** Implementation Backlog  
> **Parent Directory:** [`docs/refactoring/phase3/action-plan/`](./)  

---

## 1. Engine Backend API Tasks (`engine/src/llm/delivery.go`)

- [ ] `GET /api/v1/provider-accounts`: List configured provider accounts for workspace.
- [ ] `POST /api/v1/provider-accounts`: Register new provider account (endpoint, type, secret ref).
- [ ] `PATCH /api/v1/provider-accounts/{id}`: Update account labels, enabled state, or secret ref.
- [ ] `POST /api/v1/provider-accounts/{id}/health-check`: Trigger active latency and ping test.
- [ ] `POST /api/v1/provider-accounts/{id}/discover-models`: Fetch live catalog from upstream API.
- [ ] `GET /api/v1/models`: List active and available models with verified capability flags.
- [ ] `GET /api/v1/models/{id}`: Retrieve detailed model descriptors, pricing, and limits.
- [ ] `PATCH /api/v1/models/{id}`: Enable/disable or adjust manual pricing overrides.
- [ ] `GET /api/v1/route-policies`: List route profiles (`local-only`, `balanced`, `code`, etc.).
- [ ] `POST /api/v1/route-policies`: Create custom routing policy with specific scoring weights.
- [ ] `POST /api/v1/route-policies/simulate`: Dry-run candidate ranking for a hypothetical task envelope.
- [ ] `GET /api/v1/runs/{id}/route-decisions`: Retrieve historical routing rationale and candidate scores.
- [ ] `GET /api/v1/runs/{id}/usage`: Retrieve aggregate token consumption and cost for a run.
- [ ] `POST /api/v1/runs/{id}/model-override`: Pin a specific model for subsequent turns.
- [ ] `POST /api/v1/runs/{id}/budget-approval`: Grant user approval to exceed soft spending threshold.

---

## 2. Tauri IPC Desktop Commands (`apps/tauri/src-tauri/`)

- [ ] `list_provider_accounts`: Interop bridge to fetch account configurations.
- [ ] `create_provider_account`: Securely save account and store credential in OS keychain.
- [ ] `check_provider_health`: Perform endpoint connectivity check.
- [ ] `list_models`: Retrieve catalog models for UI selection.
- [ ] `simulate_route`: Return real-time candidate score rankings for UI visualizer.
- [ ] `get_run_route_decisions`: Provide data for Run Inspector route explanation panel.
- [ ] `get_run_usage`: Provide data for token and cost counter widgets.
- [ ] `set_run_model_override`: Apply user model pinning.

---

## 3. Desktop & Web UI Pages (`web/src/pages/` & `apps/tauri/`)

- [ ] `/providers`: Provider Account Management (add account modal, health indicators, endpoint config).
- [ ] `/models`: Model Catalog Browser (capability badges, context window size, pricing confidence).
- [ ] `/routing`: Route Policy & Simulator (interactive weight sliders, live ranking preview).
- [ ] `/budgets`: Spending Limits & Alerts (per-run hard caps, soft warning thresholds, approval queues).
- [ ] `/runs/:runId`: Run Inspector Route View (explainable route rationale, fallback history, token ledger).
- [ ] `/metrics`: Analytics Dashboard (P95 latency per route, fallback frequency, cost per accepted task).

---

## 4. Core Engine Internal Wiring (`engine/`)

- [ ] Wire `llm.ProviderSet` in `engine/src/llm/wire.go` with `CatalogService`, `RouteSelector`, `UsageLedger`.
- [ ] Inject `llm.RouteSelector` into `crew.NewService` in `engine/src/crew/services.go`.
- [ ] Replace hardcoded provider references in `crew/services.go` with dynamic route resolution.
- [ ] Implement token estimation middleware in `engine/src/llm/budget_service.go`.
- [ ] Wire `tool.ProviderSet` with `ApprovalGate` and `PolicyEngine` in `engine/src/tool/services.go`.
- [ ] Add Prometheus metric collectors in `engine/core/metrics/llm_metrics.go`.
- [ ] Add OpenTelemetry trace spans around route selection and upstream streaming.

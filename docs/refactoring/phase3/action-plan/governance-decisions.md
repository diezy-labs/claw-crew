# Claw-Crew Phase 3 — Action Plan: Governance Decisions & Definition of Done

> **Status:** Governance Baseline & Project Sign-Off Criteria  
> **Parent Directory:** [`docs/refactoring/phase3/action-plan/`](./)  

---

## 1. Ten Explicit Decisions Required Before Coding

Before merging Milestone M1 implementation code, engineering leadership must ratify the following 10 policy decisions:

| # | Policy Question | Proposed Default Decision | Rationale |
|---|---|---|---|
| **1** | Approved Initial Providers | Local Ollama, OpenAI-compatible endpoints, OpenRouter. | Delivers privacy, enterprise compatibility, and broad cloud model access. |
| **2** | Default Workspace Profile | `balanced` (optimizes quality vs. cost; not cheapest). | Prevents user frustration caused by degraded reasoning on cheap models. |
| **3** | Data Egress Policy | `confidential` and `restricted` data strictly local-only. | Guarantees compliance and protects user IP from cloud leaks. |
| **4** | Budget Approval Threshold | \$0.50 per run soft warning; \$2.00 hard limit. | Protects users from runaway loops while permitting typical agent turns. |
| **5** | Credential Storage Mechanism | OS Keychain / Secret Reference (`secret://`). | Never persists raw API tokens in plain-text configuration files. |
| **6** | OpenRouter Activation | Explicit opt-in per workspace. | Respects organizations requiring direct, private cloud billing accounts. |
| **7** | Fallback Interruption Rule | Fallback allowed **only before** partial tool output. | Prevents duplicate real-world side effects (double commit, double API call). |
| **8** | Mandatory Model Capabilities | Tool calling and structured output mandatory for action agents. | Non-tool models cannot participate in autonomous execution loops. |
| **9** | Third-Party Stance (Kiro/Antigravity) | Official, documented APIs/MCP only; no scraping. | Complies with vendor terms of service and eliminates fragile hacks. |
| **10** | Model Promotion Gate | Eval benchmark score $\ge 90\%$ + 50 canary runs. | Prevents unverified models from degrading agent reliability. |

---

## 2. Final Prioritization Roadmap

```text
┌────────────────────────────────────────────────────────┐
│ BUILD NOW (Phase 3 Core - Immediate Value)             │
├────────────────────────────────────────────────────────┤
│ 1. Canonical provider contracts, DTOs, and error codes │
│ 2. Local Ollama adapter and OpenAI-compatible adapter  │
│ 3. OpenRouter official adapter                         │
│ 4. Route profiles and capability filters               │
│ 5. Route decision explainability in Run Inspector      │
│ 6. Health check monitor and circuit breaker            │
│ 7. Safe fallback controller (before tool output)       │
└────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────┐
│ BUILD NEXT (Phase 3 Extensions)                        │
├────────────────────────────────────────────────────────┤
│ 8. Model catalog dynamic sync and verification gates   │
│ 9. Token estimator and context budgeter                │
│ 10. Usage and cost ledger with confidence labels       │
│ 11. RAG-aware long-context routing                     │
│ 12. Golden evaluation test suite                       │
└────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────┐
│ DEFER (Beyond Phase 3)                                 │
├────────────────────────────────────────────────────────┤
│ 13. Generic OpenAI-compatible reverse proxy for IDEs   │
│ 14. Proxy pools and network tunnels                    │
│ 15. Consumer subscription session-scraping             │
│ 16. Full multi-tenant enterprise billing engine        │
└────────────────────────────────────────────────────────┘
```

---

## 3. Definition of Done (DoD)

Phase 3 is complete and ready for release when all of the following conditions are met:

1. **Provider Abstraction:** A crew workflow can seamlessly execute against local Ollama, OpenAI-compatible cloud, or OpenRouter routes without modifying agent code.
2. **Capability Safety:** Tasks requiring tool calling or structured output are automatically rejected before invocation if routed to incompatible models.
3. **Auditability & Explainability:** Every model request creates an auditable `RouteDecision` displaying selected routes, rejected alternatives, and cost estimates.
4. **Security Integrity:** Upstream credentials are referenced exclusively via secret URIs (`secret://`); zero plaintext tokens appear in logs, API responses, or UI views.
5. **No Double Side Effects:** Model fallbacks execute only prior to partial tool execution; mutating tools never trigger silent replays.
6. **Codebase Quality:** All new code conforms to existing `engine/src/llm/` standards, passes Wire generation (`wire ./...`), and adheres to Clean Architecture error envelopes.
7. **Race Clean:** `go test -race ./...` passes across all engine packages with zero data races.
8. **UI Integration:** Tauri and Web clients visualize provider health, model catalogs, and live route explainability.

# Claw-Crew Phase 3 — Provider Routing: Security, Compliance & Governance

> **Status:** Proposed Security Architecture & Compliance Policy  
> **Parent Directory:** [`docs/refactoring/phase3/provider-routing/`](./)  

---

## 1. Credential Management & Secret References

Claw Model Gateway enforces a zero-plaintext policy for upstream provider credentials:

```text
┌────────────────────────────────────────────────────────┐
│ Database / Configuration Storage                       │
│ Store ONLY: "secret://workspace/default/openrouter-key" │
│ NEVER store: "sk-or-v1-abcdef123456..."                │
└───────────────────────────┬────────────────────────────┘
                            │ Resolved at request time via
                            ▼
┌────────────────────────────────────────────────────────┐
│ OS Keychain / Enterprise Secret Vault                  │
│ (macOS Keychain, Linux Secret Service, Windows CredMgr)│
└───────────────────────────┬────────────────────────────┘
                            │ Injected in-memory directly to
                            ▼
┌────────────────────────────────────────────────────────┐
│ Provider Adapter Client Transport                      │
│ (Destroyed from heap after request termination)        │
└────────────────────────────────────────────────────────┘
```

### Invariants:
1. **Secret Masking:** API endpoints (`/api/v1/provider-accounts`) and Tauri IPC handlers always redact key fields, returning only credential references and metadata status.
2. **Log Sanitization:** Upstream error strings and HTTP headers (`Authorization`, `x-api-key`) are scrubbed by `engine/core/logger` before logging to prevent token leakage.

---

## 2. Data Classification Matrix

Every model request is tagged with a `data_classification` level that sets an unbreachable floor on eligible provider routes:

| Classification | Definition | Permitted Routing Targets | Fallback Boundary |
|---|---|---|---|
| **`public`** | Open-source code, public documentation, generic web search. | Any enabled provider (Local, OpenRouter, Cloud APIs). | May fallback across all providers. |
| **`internal`** | Internal team documentation, standard product code. | Approved cloud providers with enterprise data protection agreements and local endpoints. | May fallback to other approved cloud or local models. |
| **`confidential`**| Proprietary business logic, sensitive architectures. | Dedicated self-hosted enterprise endpoints or local models. | **Never fallback to public multi-tenant cloud.** |
| **`restricted`**| Personally Identifiable Information (PII), customer data, credentials. | **Local-only execution (Ollama)** or air-gapped on-premise inference. | **Zero external egress allowed under any circumstance.** |

---

## 3. Legitimate Interfaces & Third-Party Integration Policy

Claw-Crew strictly limits provider integrations to **authorized, documented, and terms-compliant mechanisms**:

```text
Permitted Integration Hierarchy:
1. Official Vendor REST / gRPC API (e.g. OpenAI, Anthropic, Google Gemini)
2. Official Vendor SDK
3. Official CLI invoked via sandboxed tool execution
4. Official Model Context Protocol (MCP) Server
5. Manual user file export / import
─────────────────────────────────────────────────────────────
ANYTHING BELOW THIS LINE IS STRICTLY PROHIBITED:
6. Reverse-engineering private web/browser endpoints
7. Harvesting browser session cookies or JWT tokens
8. Intercepting or MITM-proxying desktop application clients
9. Rotating consumer accounts to evade free-tier rate limits
```

### Specific Stance on Kiro and Antigravity:
- **Kiro:** Treat primarily as a peer architectural benchmark and workspace pattern comparator. Direct LLM provider integration will only be implemented if Kiro releases an official, stable, public API or MCP protocol.
- **Antigravity:** Will only be connected via official, documented developer APIs or standard MCP extensions. Claw-Crew will not implement undocumented hacks or private client protocol emulations.

---

## 4. Explicit Anti-Patterns

The following techniques are permanently forbidden in Claw-Crew:

1. **Quota / Limit Evasion:** No rotating account pools or synthetic IP switching to bypass provider rate limits.
2. **Hidden Cloud Fallbacks:** Never silently route a failed local or confidential request to a public cloud API without explicit user consent.
3. **Unverified Model Activation:** Never make an automatically discovered model available for production agent tasks before passing validation tests.
4. **Invoice Spoofing:** Never present an estimated cost as an exact billing charge; always display pricing confidence labels.

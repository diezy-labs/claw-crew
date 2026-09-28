# Claw-Crew Phase 3 — Provider Routing: Routing Algorithm & Scoring Pipeline

> **Status:** Proposed Algorithm Specification  
> **Package Target:** `engine/src/llm/route_service.go`  
> **Parent Directory:** [`docs/refactoring/phase3/provider-routing/`](./)  

---

## 1. The 15-Stage Deterministic Routing Pipeline

To guarantee that routing decisions are explainable, reproducible, and verifiable, Claw Model Gateway avoids black-box heuristics in favor of an explicit 15-stage filtering and scoring pipeline:

```text
 1. Request Envelope Ingestion (Identity, Task, Purpose, Budget)
    │
 2. Workspace & Data Classification Resolution (e.g. "confidential")
    │
 3. Route Profile Resolution (Run override > Task > Agent > Workspace)
    │
 4. Load Active Provider Accounts for Workspace
    │
 5. Filter Unhealthy, Down, or Quarantined Accounts (Circuit Breaker)
    │
 6. Hard Capability Filter (Reject models lacking tool_calling, vision, etc.)
    │
 7. Hard Privacy & Data Residency Gate (Local-only if confidential)
    │
 8. Hard Context Window & Output Limit Gate (Reject if context < min_tokens)
    │
 9. Token & Cost Estimation (Input prompt + tools + history estimation)
    │
10. Hard Budget Ceiling Check (Reject if estimated_cost > max_budget)
    │
11. Multi-Attribute Weighted Scoring (Score all remaining candidates)
    │
12. Select Primary Route & Rank Bounded Fallbacks (Max 2 fallbacks)
    │
13. Persist Route Decision & Emit SSE Route Event
    │
14. Execute with Context Cancellation, Timeout & Stream Monitoring
    │
15. Record Actual Usage, Latency, and Outcome in Usage Ledger
```

---

## 2. Multi-Attribute Candidate Scoring Formula

Remaining candidates that satisfy all hard constraints are ranked using a normalized, weighted linear scoring function:

$$\text{CandidateScore} = \sum (W_i \times S_i)$$

$$\begin{aligned}
\text{Score} =\; & (W_{\text{quality}} \times S_{\text{quality}}) \\
+\; & (W_{\text{reliability}} \times S_{\text{reliability}}) \\
+\; & (W_{\text{latency}} \times S_{\text{latency}}) \\
+\; & (W_{\text{cost}} \times S_{\text{cost}}) \\
+\; & (W_{\text{locality}} \times S_{\text{locality}}) \\
+\; & (W_{\text{cache}} \times S_{\text{cache}})
\end{aligned}$$

### Score Component Definitions:
1. **$S_{\text{quality}} \in [0.0, 1.0]$:** Evaluated benchmark score for the specific task domain (Code vs. Reasoning vs. Extraction).
2. **$S_{\text{reliability}} \in [0.0, 1.0]$:** Rolling success rate over the last 100 requests ($1.0 - \text{error\_rate}$).
3. **$S_{\text{latency}} \in [0.0, 1.0]$:** Normalized inverse of P95 latency ($1.0 - \frac{\text{latency}}{\text{max\_latency}}$).
4. **$S_{\text{cost}} \in [0.0, 1.0]$:** Normalized inverse of estimated cost ($1.0 - \frac{\text{estimated\_cost}}{\text{max\_budget}}$).
5. **$S_{\text{locality}} \in \{0.0, 1.0\}$:** $1.0$ if endpoint executes entirely on local hardware (Ollama); $0.0$ for cloud APIs.
6. **$S_{\text{cache}} \in \{0.0, 1.0\}$:** $1.0$ if upstream provider officially supports prompt prefix caching.

### Profile Weight Presets:

| Route Profile | $W_{\text{quality}}$ | $W_{\text{reliability}}$ | $W_{\text{latency}}$ | $W_{\text{cost}}$ | $W_{\text{locality}}$ | $W_{\text{cache}}$ |
|---|---|---|---|---|---|---|
| `local-only` | 0.30 | 0.20 | 0.10 | 0.00 | **0.40** | 0.00 |
| `economy` | 0.20 | 0.20 | 0.10 | **0.40** | 0.05 | 0.05 |
| `balanced` | **0.35** | **0.25** | 0.15 | 0.15 | 0.05 | 0.05 |
| `premium` | **0.55** | 0.25 | 0.10 | 0.00 | 0.00 | 0.10 |
| `code` | **0.45** | 0.25 | 0.15 | 0.05 | 0.05 | 0.05 |
| `research` | **0.40** | 0.25 | 0.10 | 0.10 | 0.05 | **0.10** |

---

## 3. Fallback Policy & Safety Boundaries

Fallback is essential for resilience, but **unrestricted fallback creates critical security and consistency vulnerabilities**:

```text
┌────────────────────────────────────────────────────────┐
│ Permitted Automatic Fallback Scenarios                 │
│                                                        │
│ ✓ Upstream connection timeout prior to any output      │
│ ✓ Upstream 429 Rate Limited (capacity exhausted)       │
│ ✓ Upstream 503 Service Unavailable / 502 Bad Gateway   │
│ ✓ Upstream explicit overload error                     │
└────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────┐
│ STRICTLY FORBIDDEN Fallback Scenarios                  │
│                                                        │
│ ✗ Model has already emitted partial tool calls         │
│   (Replaying on another model risks double mutation)   │
│ ✗ Target fallback violates data classification level   │
│   (e.g., falling back from local to public cloud)      │
│ ✗ Request failed validation or schema constraints      │
│ ✗ Provider returned 401 Unauthorized / Invalid Key    │
│ ✗ User explicitly pinned model with fallback=false     │
└────────────────────────────────────────────────────────┘
```

When a partial stream failure occurs after tool execution has started, the engine halts the turn, reports an error, and prompts the user for manual recovery rather than guessing model intent.

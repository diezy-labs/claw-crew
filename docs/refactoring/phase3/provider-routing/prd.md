# Claw-Crew Phase 3 — Provider Routing: Product Requirements Document (PRD)

> **Status:** Approved Product Requirements  
> **Product Subsystem:** Claw Model Gateway (CMG)  
> **Parent Directory:** [`docs/refactoring/phase3/provider-routing/`](./)  

---

## 1. Product Subsystem Identity: Claw Model Gateway (CMG)

**Claw Model Gateway (CMG)** is an internal Go-engine subsystem that:
1. Registers supported provider adapters (local endpoints, OpenAI-compatible APIs, OpenRouter, native Gemini, native Anthropic).
2. Manages provider accounts and endpoints without exposing raw credentials.
3. Maintains a normalized model capability and pricing catalog.
4. Applies workspace, crew, agent, and task-specific routing policies.
5. Selects primary routes and bounded fallback chains deterministically.
6. Enforces privacy boundaries, budget limits, and capability prerequisites.
7. Emits explainable route-decision and usage events for run inspectors.
8. Normalizes streaming responses, tool calls, token usage, and errors for the rest of Claw-Crew.

---

## 2. Goals & Non-Goals

### 2.1 Core Goals
- **Capability-Driven Routing:** Never route a task requiring tool calling or structured JSON to a model that lacks those capabilities.
- **Privacy & Data Boundary Preservation:** Strictly restrict confidential/restricted workspace data to local or approved private endpoints.
- **Explainable Decision Making:** Every routing decision must record its reason codes, rejected alternatives, and cost estimates.
- **Bounded Fault Tolerance:** Automatic fallback on transient infrastructure errors (rate limits, timeouts), without silent side-effect duplication.
- **Cost Transparency:** Track estimated vs. provider-reported token usage and cost with explicit confidence indicators.

### 2.2 Explicit Non-Goals
- **Not a Universal Reverse Proxy:** CMG will not expose a public OpenAI-compatible proxy to serve arbitrary third-party IDEs or tools in v1.
- **Not an Account Rotator:** No automated rotation across multiple consumer accounts to bypass provider rate limits or quotas.
- **No Unofficial Protocol Scraping:** Strictly reject reverse-engineering private browser endpoints, harvesting web session cookies, or proxying unauthorized consumer subscription tiers.
- **Not a Replacement for User Control:** Users retain the ability to pin specific models and override automatic routing.

---

## 3. Model Capability Matrix

CMG filters models based on technical capabilities prior to candidate scoring:

| Capability Flag | Why Claw-Crew Needs It | Routing Requirement |
|---|---|---|
| `text_generation` | Baseline conversation and content generation. | Mandatory for all agent tasks. |
| `streaming` | Live token feedback in TUI, Tauri, and Web. | Mandatory for interactive user turns. |
| `tool_calling` | Autonomous agent execution via Tool Runtime. | Mandatory for action and research agents. |
| `structured_output` | Task graphs, JSON extraction, and DTO contracts. | Mandatory for workflow engines and validators. |
| `json_schema` | Strict schema conformity without hallucinations. | Preferred/Required for code generation tasks. |
| `vision` | Screenshot analysis, UI verification, document OCR. | Required only when task payload includes images. |
| `embeddings` | Semantic search and RAG indexing in `src/memory`. | Routed to dedicated embedding provider classes. |
| `long_context` | Ingesting large repositories or multi-turn history. | Filtered by task-specified `min_context_tokens`. |
| `reasoning_quality`| Architectural planning, debugging, and synthesis. | Evaluated benchmark score, not marketing label. |
| `local_availability`| Zero external data egress and offline operations. | Enforced by confidential data classification. |
| `prompt_caching` | Reusing system prompts and tool schemas to cut cost. | Optimization flag where upstream provider supports it. |

---

## 4. Standard Route Profiles

| Profile Name | Intent & Behavior | Default Route Archetype |
|---|---|---|
| `local-only` | Total privacy; zero external network egress. | Local Ollama endpoint (e.g., Llama 3.3, Qwen 2.5 Coder). |
| `economy` | Lowest operational cost for lightweight tasks. | Economical cloud models (e.g., Haiku, Flash, DeepSeek-V3). |
| `balanced` | Optimized balance between reasoning quality, speed, and cost. | Standard cloud models (e.g., Claude 3.5 Sonnet, GPT-4o-mini). |
| `premium` | Highest capability for complex refactoring and deep research. | Frontier models (e.g., Claude 3.7 Sonnet, o3-mini, GPT-4o). |
| `research` | High context window and citation adherence. | Long-context models (e.g., Gemini 1.5 Pro / 2.0 Flash). |
| `code` | Strong code generation, AST awareness, and tool use. | Code-tuned models with verified tool calling. |
| `pinned` | Strict user override; executes exact account and model. | Pinned model; no fallback unless user explicitly permits. |

---

## 5. UI and UX Plan

### 5.1 Provider Settings Page
A focused management interface across desktop and web:
1. **Provider Accounts:** Display connection status, endpoint URL, auth status, allowed workspaces, and one-click health check.
2. **Model Catalog:** Visual table showing model names, capability badges, context limits, pricing confidence, and an enable/disable switch.
3. **Route Profiles:** Interactive editor to adjust weights (Quality vs. Cost vs. Latency vs. Locality) per profile.
4. **Budget & Guardrails:** Configurable soft warning thresholds and hard per-run spending caps.

### 5.2 Run Inspector & Route Explainability
For every agent turn, the Run Inspector provides full transparency:

```text
┌────────────────────────────────────────────────────────────────────────┐
│ Agent: Architecture Researcher                                         │
│ Task: Analyze provider routing tradeoffs                              │
│ Route Profile: research-balanced-v1                                    │
│ Selected Route: OpenRouter / Qwen 2.5 72B Instruct                     │
│ Fallback Route: Ollama / Local Qwen 2.5 14B                            │
│ Why Selected: Matches tool_calling, 128k context, within $0.05 budget  │
│ Rejected Candidates:                                                   │
│   - Cloud Model A: Lacks tool_calling support                          │
│   - Cloud Model B: Exceeds estimated budget ($0.22 > $0.05)            │
│ Usage & Cost: 7,420 input / 1,680 output tokens ($0.032 - Reported)    │
│ Latency: 4.8s | Cache Hit: 4,100 tokens (System & Tool Definitions)   │
└────────────────────────────────────────────────────────────────────────┘
```

### 5.3 Route Simulator
An interactive tool in the developer dashboard allowing users to input hypothetical task parameters (e.g. 64k tokens, tool calling, internal data classification) and view the exact ranking, cost projection, and candidate filtering in real time before executing a run.

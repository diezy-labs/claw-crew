# Fleet AI (Galleon) — System Architecture & Technical Specification

> **Platform:** Galleon Local-First AI Organization Workspace  
> **Repository:** `diezy-labs/claw-crew` (Branch: `feat/enhance-agent-phase`)  
> **Target Subsystem:** `claw-crew/web` (React + Tailwind CSS + Zustand)  
> **Runtime Ecosystem:** Go Cognitive Engine (`engine/src/`) + Rust Tauri Security Shield + React 19 Frontend

---

## 1. Executive Summary & Fundamental Business Model

Galleon is a **local-first, self-hosted AI organization workspace** designed for engineers, operators, and founders who demand persistent autonomous AI collaboration without platform credit lock-in.

### The Problem It Solves:
1. **Credit Exhaustion Frustration:** Traditional autonomous agent workspaces freeze when proprietary platform tokens or credits run out, even when the user possesses their own direct API keys (Anthropic, OpenAI, Gemini) or local models (Ollama, LM Studio).
2. **Generic Chat vs. Structured Organization:** Chat transcripts are ephemeral. Real work requires persistent specialist teams, explicit inputs, quality gates, and reviewable evidence-backed deliverables.
3. **Unbounded Autonomy vs. Human Command:** AI agents should not make external commits, deploy code, or execute destructive actions without explicit cryptographic Owner authorization.

### The Fundamental Operating Promise:
```text
Your Key.
Your Model.
Your Infrastructure.
Your Fleet.
Your Rules.
```

---

## 2. Separation of Concerns & Three-Tier Architecture

To guarantee security, concurrency performance, and UI responsiveness, Galleon separates concerns across three primary technologies:

```
┌─────────────────────────────────────────────────────────────┐
│ 1. VISUAL INTERFACE (React 19 / Tailwind / Zustand)         │
│    - Web Dashboard & Tauri Desktop Webview                  │
│    - Quartermaster Office, Mission Board, Ships, Treasury   │
│    - Real-time Notifications & Command Palette (⌘K)         │
└──────────────────────────────▲──────────────────────────────┘
                               │ JSON-RPC / WebSocket / REST
┌──────────────────────────────▼──────────────────────────────┐
│ 2. COGNITIVE ENGINE & ORCHESTRATION (Go Engine / Daemon)    │
│    - engine/src/: Orchestrator, Fleet, Ship, Quest, Map     │
│    - Concurrent goroutine worker pool, Voyage lifecycle     │
│    - BYOK/BYOM LLM routing & token cost accounting          │
└──────────────────────────────▲──────────────────────────────┘
                               │ IPC / Landlock System Calls
┌──────────────────────────────▼──────────────────────────────┐
│ 3. HOST & SECURITY SHIELD (Rust / Tauri Landlock Sandbox)   │
│    - Linux Landlock & macOS Seatbelt kernel sandboxing      │
│    - Cryptographic ActionDigest verification                │
│    - Local filesystem boundary enforcement                  │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Domain Model & Work Hierarchy

The Galleon operating model translates abstract goals into validated organization value through two intersecting hierarchies:

### A. Organizational Hierarchy
```text
Fleet / Dermaga (Organization Hub)
 └── Ship (Persistent Specialist AI Team Container)
      ├── Navigator (Ship-level Planner & Orchestrator)
      ├── Charter (Living Blueprint, Permitted Quests, Prohibitions, Budget)
      └── Squad (Functional Team Unit)
           └── Crew Member (Specialist AI with Scoped Skills, Tools & Memory)
```

### B. Work Delivery Hierarchy
```text
Workspace
 └── Project
      └── Quest (Living SOP / Workflow with defined outcome)
           └── Map (Step-by-step Execution Plan & Dependencies)
                └── Voyage (Single Execution Run by Specialist Crew)
                     └── Artifact (Durable Evidence-Backed Deliverable)
                          └── Discovery (Insight, Risk, or Opportunity)
                               └── Treasure (Validated Value Claimed by Owner)
```

---

## 4. UI/UX Surface Architecture (14 Core Surfaces)

The frontend follows the progressive disclosure design philosophy: **Conversation-first at the entry point, operations-first in the workspace, and technical depth on demand.**

| Group | Surface | Purpose | Business Entity |
|---|---|---|---|
| **COMMAND** | **Quarterdeck** | Executive command console, natural language goal intake, live Fleet pulse, decision cards, and chat-to-artifact promotion | AI CEO / Command Console |
| **COMMAND** | **Quests** | Workspace & Project-oriented workflow planning, living Map steps, Map Studio (Guided vs Advanced), and Quest creation | Living SOPs / Workflows |
| **COMMAND** | **Captain’s Journal** | Private working sessions, exploratory thinking, notes, and session-to-work conversion with Quartermaster (separate from Logbook) | Private Sessions & Notes |
| **FLEET** | **Mission Board** | Global work queue across Backlog, Ready to Sail, Underway, Awaiting Captain, and Treasures Claimed | Fleet-wide Work Routing |
| **FLEET** | **Ships** | Operational home for persistent specialist teams, readable Charters, Navigator briefings, and active voyages | Specialist Team Containers |
| **FLEET** | **Crew Members** | Specialist profile cards (Cartographer, QA Reviewer) and "Make Me a Squad" wizard | Persistent AI Specialists |
| **FLEET** | **Artifacts** | Durable, reviewable deliverables with citation links, risk discoveries, and "Mark as Treasure" action | Core Value Deliverables |
| **FLEET** | **Captain’s Approval** | High-impact external side-effect review queue (e.g. GitHub issue creation, PR commits) | Governance & Safety Gate |
| **OPERATIONS** | **Treasury** | BYOK/BYOM provider cost tracking, per-voyage ledger, and local model routing recommendations | Cost Transparency |
| **OPERATIONS** | **Logbook** | Official chronological audit trail with correlation IDs, actor attribution, and JSON export | Audit & Activity Record |
| **OPERATIONS** | **Harbor** | Model provider accounts (Anthropic, Gemini, OpenAI, Ollama), tools, and workspace connectors | Integrations Hub |
| **CONTROL** | **Fleet Code** | Policy engine, risk class tiers (`read_only` to `destructive`), and spending caps | Rules & Boundaries |
| **CONTROL** | **Crow’s Nest** | Gateway health, goroutine telemetry, and automated Doctor diagnostic checks | Observability Console |
| **CONTROL** | **Shipyard** | Timber capacity metaphor, active Ships 1/3, Crew berths 6/15, and community tier info | Organizational Scaling |

### Critical Distinction: Captain’s Journal vs. Logbook
- **Captain’s Journal:** Owner's personal and private workspace for informal notes, exploratory thinking, and ongoing dialogues with Quartermaster. It is private, can be temporary, and can be promoted into an Artifact or Quest.
- **Logbook:** Official, immutable audit and activity record of the Fleet, capturing system events, voyage executions, approved actions, and cryptographic correlation traces.

---

## 5. Performance, Responsive Design & Anti-Slop Discipline

1. **Lazy Loading:** All feature views in `src/components/features/` are dynamically loaded via `React.lazy` and `Suspense`, ensuring the initial bundle remains lightweight and fast to mount.
2. **Tabular Numerals:** All financial figures, token counts, timestamps, and metrics strictly utilize `tabular-nums` (`font-mono` / JetBrains Mono) to prevent layout shifting.
3. **Zero-Pill Metadata:** Informational metadata avoids candy pill badges in favor of unboxed typographic text with `·` separators.
4. **Theme Parity:** Full dark mode and light mode parity supported via CSS custom properties and standard Tailwind CSS variables.

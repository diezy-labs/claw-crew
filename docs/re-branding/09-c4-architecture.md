# C4 Architecture

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## C4 Level 1 — System Context

```text
┌──────────────────────┐
│ Pirate King          │
│ Human Owner          │
└──────────┬───────────┘
           │ goals, approvals, policy decisions
           ▼
┌────────────────────────────────────────────────────────────┐
│ Claw-Crew Fleet Command System                              │
│                                                            │
│ Coordinates Ships, Captains, Squads, Crew Members,         │
│ Voyages, reports, budget, policy, and approvals.           │
└───────┬──────────────────┬──────────────────┬──────────────┘
        │                  │                  │
        ▼                  ▼                  ▼
┌──────────────┐  ┌────────────────┐  ┌─────────────────────┐
│ AI Providers │  │ MCP / External │  │ Workspace / Data    │
│ Local/Cloud  │  │ Integrations   │  │ Repositories/docs   │
└──────────────┘  └────────────────┘  └─────────────────────┘
```

### External actors

| Actor/system | Relationship |
|---|---|
| Pirate King | Owns fleet, approves strategic actions, receives reports |
| TUI/Tauri/Web clients | Render command deck, collect approvals, configure fleet |
| AI providers | Provide model inference through Model Gateway |
| MCP servers | Provide approved tools/resources/prompts |
| Workspace/repositories | Ship-scoped data and code |
| External systems | CMS, GitHub, analytics, databases, only through governed tools |

## C4 Level 2 — Containers

```text
┌───────────────────────────────────────────────────────────────────────────┐
│ Claw-Crew System                                                          │
│                                                                           │
│  ┌────────────────────────────┐                                           │
│  │ Rust TUI / Tauri / Web UI  │                                           │
│  │ Command Deck + Ship Views  │                                           │
│  └──────────────┬─────────────┘                                           │
│                 │ REST/gRPC/SSE                                            │
│  ┌──────────────▼──────────────────────────────────────────────────────┐  │
│  │ Go Agent Engine                                                      │  │
│  │                                                                      │  │
│  │ Fleet Command  │ Crew Runtime │ Tool Runtime │ Model Gateway         │  │
│  │ Memory/RAG     │ Policy       │ Approval     │ Artifact/Audit         │  │
│  └───────┬───────────────────────────────────────────────┬─────────────┘  │
│          │                                               │                │
│  ┌───────▼────────────────┐                 ┌────────────▼────────────┐  │
│  │ Durable Data Plane      │                 │ Capability Plane         │  │
│  │ Postgres/Event Store    │                 │ Providers/MCP/Tools      │  │
│  │ Object/Artifact Store   │                 │ Sandboxed Executors      │  │
│  └────────────────────────┘                 └─────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────────┘
```

## C4 Level 3 — Go Engine components

```text
Go Agent Engine
│
├── Fleet Command Module
│   ├── Fleet Service
│   ├── Ship Service
│   ├── Quartermaster Service
│   ├── Fleet Report Service
│   ├── Fleet Order Service
│   ├── Cross-Ship Handoff Service
│   ├── Budget Allocation Service
│   └── Escalation Service
│
├── Crew Runtime Module
│   ├── Captain Runtime
│   ├── Squad Coordinator
│   ├── Crew Member Runtime
│   ├── Job Order Scheduler
│   └── Voyage State Machine
│
├── Skill Module
│   ├── Skill Registry
│   ├── Skill Resolver
│   ├── Skill Evaluation
│   └── Skill Versioning
│
├── Tool Runtime Module
│   ├── Tool Registry
│   ├── Policy Evaluator
│   ├── Approval Manager
│   ├── Sandbox Executor
│   ├── MCP Client
│   ├── Output Sanitizer
│   └── Audit Writer
│
├── Model Gateway Module
│   ├── Provider Adapters
│   ├── Route Policy
│   ├── Capability Filter
│   ├── Budget Guard
│   ├── Usage Ledger
│   └── Circuit Breaker
│
├── Knowledge Module
│   ├── Ship Memory
│   ├── Fleet Knowledge
│   ├── Artifact Index
│   ├── Retrieval Service
│   └── Lesson Promotion
│
└── Platform Core
    ├── Config
    ├── Secrets
    ├── Events
    ├── Metrics
    ├── Logging
    ├── Errors
    └── Auth/RBAC
```

## C4 Level 4 — Quartermaster interaction sequence

```text
Pirate King          UI            Go Fleet API       Quartermaster       Ship Captain       Crew Runtime
     │               │                  │                   │                  │                  │
     │ Objective     │                  │                   │                  │                  │
     ├──────────────>│                  │                   │                  │                  │
     │               ├─────────────────>│                   │                  │                  │
     │               │                  ├──────────────────>│                  │                  │
     │               │                  │ Fleet intake      │                  │                  │
     │               │                  │                   ├─────────────────>│                  │
     │               │                  │                   │ request summary  │                  │
     │               │                  │                   │<─────────────────┤                  │
     │               │                  │                   │ create proposal  │                  │
     │               │<─────────────────┤                   │                  │                  │
     │ Approve plan  │                  │                   │                  │                  │
     ├──────────────>│                  │                   │                  │                  │
     │               ├─────────────────>│                   │                  │                  │
     │               │                  ├──────────────────>│                  │                  │
     │               │                  │                   ├─────────────────>│                  │
     │               │                  │                   │ scoped ship order│                  │
     │               │                  │                   │                  ├─────────────────>│
     │               │                  │                   │                  │ execute voyage  │
     │               │                  │                   │                  │<────────────────┤
     │               │                  │                   │                  │ Ship report     │
     │               │                  │                   │<─────────────────┤                  │
     │               │                  │<──────────────────┤ Fleet report     │                  │
     │<──────────────┤                  │                   │                  │                  │
```

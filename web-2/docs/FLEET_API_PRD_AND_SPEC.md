# Fleet AI (Galleon) — API Gap Analysis, PRD, and Technical Specification

> **Repository Reference:** `https://github.com/diezy-realm/galleon-fleet/tree/feat/enhance-agent-phase2`  
> **Workspace Subsystem:** Fleet AI Orchestration Layer (`web-2` / `crates/clawcrew-gateway`)  
> **Document Version:** `2.0.0-phase2`  
> **Status:** Architecture Blueprint, PRD, and Production API Specification  

---

## 1. Executive Summary & Bidirectional Gap Analysis

An exhaustive audit of the `galleon-fleet` codebase on branch `feat/enhance-agent-phase2` reveals a functional bifurcation between the **Rust Gateway Daemon (`crates/clawcrew-gateway`)** and the modern **Sovereign Fleet UI (`web-2`)**:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ 1. BACKEND GATEWAY READY, UI MISSING (APIs available without Web-2 views)   │
│    - Agent-to-Agent (A2A) Mesh Protocol & Catalog Discovery                │
│    - Real-Time Full-Duplex Voice Gateway (Silero VAD / Audio WebSockets)    │
│    - Live Canvas (A2UI - Agent-to-UI dynamic streaming & history)           │
│    - Per-Agent Sandboxed Workspace Filesystem Explorer                      │
│    - System Doctor Diagnostics & Automated One-Click Remediation           │
│    - WebAssembly (Wasm) Plugin Marketplace & Permission Grants             │
│    - WebAuthn Hardware Security Key (FIDO2 / YubiKey) Enrollment           │
│    - Disaster Recovery Backup Creator & Dry-Run Restore Planner            │
│    - Authorized Device Pairing & Token Rotation Engine                      │
│    - In-Place Daemon Self-Upgrade & Rollback Engine                         │
└─────────────────────────────────────────────────────────────────────────────┘
                                      ▲
                                      │ Architectural Bridge
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 2. UI SURFACES READY, BACKEND API MISSING (Sovereign concepts mock in store)│
│    - Fleet & Ship Containers (`/api/ships`, `/api/fleet`)                   │
│    - Living Quests & Map Steps (`/api/quests`, `/api/quests/{id}/map`)      │
│    - Evidence-Backed Artifacts & Treasure (`/api/artifacts`)                │
│    - Sovereign Captain's Risk Gate Approvals (`/api/approvals`)             │
│    - Captain's Journal & Entity Transformation (`/api/journal`)             │
│    - Treasury Ledger & Per-Ship Budget Caps (`/api/treasury`)               │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Part I: Features with Available Backend APIs but Missing UI

The following capabilities are already compiled and active in the Rust daemon (`clawcrew-gateway`), but lack a first-class visual interface in the `web-2` application:

### 2.1. Agent-to-Agent (A2A) Mesh Network & Peer Discovery
* **Active Backend Routes:**
  * `GET /.well-known/agent.json` & `GET /a2a/catalog`: Publishes the local agent's capability card, JSON-Schema tool parameters, and supported models.
  * `POST /a2a/{alias}`: Decodes, verifies signature, and executes inter-agent delegation requests.
  * `GET /ws/nodes`: Streams local mDNS network peers and active gateway nodes.
* **Current UI Status in `web-2`:** Missing. There is no view to inspect neighboring agent nodes, import remote agent capabilities, or establish trust relationships.
* **Recommended UI Implementation:** An **"A2A Mesh Explorer"** panel under Harbor/Control to browse local and remote agent nodes, verify peer TLS certificates, and configure inter-agent dispatch rules.

### 2.2. Real-Time Full-Duplex Voice Gateway (`voice_duplex.rs`)
* **Active Backend Routes:**
  * WebSocket route `/ws/voice` (via `voice_duplex.rs`): Handles bi-directional PCM audio streaming, Silero VAD (Voice Activity Detection), barge-in interruption, and streaming TTS.
* **Current UI Status in `web-2`:** Missing. Quarterdeck currently only exposes a text `<textarea>`.
* **Recommended UI Implementation:** A **"Voice Quartermaster Orb"** in the Quarterdeck composer that triggers browser audio capture, visualizes audio wave frequencies, and handles real-time voice interruptions.

### 2.3. Live Canvas (A2UI - Agent-to-UI) Dynamic Workspace
* **Active Backend Routes:**
  * `GET /api/canvas`, `GET /api/canvas/{id}`, `POST /api/canvas/{id}`, `DELETE /api/canvas/{id}`
  * `GET /api/canvas/{id}/history`, `GET /ws/canvas/{id}`
* **Current UI Status in `web-2`:** Missing. When agents generate UI elements, forms, or interactive diagrams, there is no designated live canvas to render them.
* **Recommended UI Implementation:** A slide-out or split-screen **Live Canvas** pane accessible from Quarterdeck and Quests where agents stream interactive components, charts, and draft documents directly into the viewport.

### 2.4. Agent Sandboxed Workspace File Browser
* **Active Backend Routes:**
  * `GET /api/agents/{alias}/workspace/list`, `GET /api/agents/{alias}/workspace/read`
  * `DELETE /api/agents/{alias}/workspace/path`, `POST /api/agents/{alias}/workspace/move`, `POST /api/agents/{alias}/workspace/mkdir`
  * `GET /api/browse`, `POST /api/browse/mkdir`, `DELETE /api/browse/rmdir`
* **Current UI Status in `web-2`:** Missing. Users can inspect completed Artifacts, but cannot explore the active working directory, scratch files, or test outputs generated by agents.
* **Recommended UI Implementation:** A **"Crew Working Directory"** tab within `CrewView.tsx` allowing direct inspection of each agent's sandboxed local filesystem.

### 2.5. System Doctor Diagnostics & Self-Remediation (`/api/doctor`)
* **Active Backend Routes:**
  * `GET /api/doctor`: Runs diagnostic health suites (daemon memory, port collisions, model provider reachability, disk permissions, TLS validity).
  * `POST /api/doctor`: Executes automated remediation (fixing corrupted configs, repairing permissions, cycling ports).
* **Current UI Status in `web-2`:** Missing.
* **Recommended UI Implementation:** A **"Doctor Diagnostic Modal"** linked from the ContextBar and Settings to run automated system audits with a single-click "Apply Automated Fix" action.

### 2.6. WebAssembly Plugin Sandbox (`/api/plugins`)
* **Active Backend Routes:**
  * `GET /api/plugins`: Lists loaded WASM plugins, exported tools, memory limits, and webhook listeners.
  * `plugin_webhook.rs`: Ingress pipeline for external webhook-driven plugin triggers.
* **Current UI Status in `web-2`:** Missing.
* **Recommended UI Implementation:** A **"Plugin Manager & Grants"** section in Settings where users inspect plugin bytecode digests, memory quotas, and active tool permissions.

### 2.7. WebAuthn / FIDO2 Hardware Security Keys (`/api/webauthn/*`)
* **Active Backend Routes:**
  * `POST /api/webauthn/register/start`, `POST /api/webauthn/register/finish`
  * `POST /api/webauthn/auth/start`, `POST /api/webauthn/auth/finish`
  * `GET /api/webauthn/credentials`, `DELETE /api/webauthn/credentials/{id}`
* **Current UI Status in `web-2`:** Missing. Settings currently only contains basic software password fields.
* **Recommended UI Implementation:** A **"Hardware Security Keys"** panel in Settings allowing enrollment of physical YubiKeys and biometric platform authenticators (Touch ID, Windows Hello).

### 2.8. Disaster Recovery & Backup Restore Planner (`/api/backup/*`)
* **Active Backend Routes:**
  * `GET /api/backup/schema-versions`, `POST /api/backup/create`
  * `POST /api/backup/plan-restore`, `POST /api/backup/restore`
* **Current UI Status in `web-2`:** Missing.
* **Recommended UI Implementation:** A **"Snapshot & Recovery"** wizard with dry-run diff preview prior to state restoration.

---

## 3. Part II: Product Requirements Document (PRD) — Sovereign Fleet APIs

This PRD formalizes the requirements to elevate the simulated client-side concepts in `web-2` into first-class, persistent, and secure backend REST and WebSocket APIs in `clawcrew-gateway`.

### 3.1. Problem Statement
The modern Fleet UI (`web-2`) presents high-value sovereign abstractions:
* **Ships & Charters:** Persistent specialist AI teams with bounded permissions and budgets.
* **Quests & Maps:** Multi-step autonomous execution workflows with discovery categorization.
* **Artifacts & Treasure:** Durable deliverables validated through human review.
* **Captain’s Approval:** Cryptographic human-in-the-loop authorization for high-impact actions.
* **Captain’s Journal:** Executive thinking converted seamlessly into actionable work.
* **Treasury Governance:** Per-voyage financial guardrails and emergency halt capabilities.

Currently, these exist as in-memory state in `fleetStore.ts`. When the page reloads or runs on multiple devices, state diverges. We must provide authoritative backend APIs for each domain entity.

### 3.2. Target User Personas
1. **The Fleet Admiral (Engineering Lead / Founder):** Defines ship charters, assigns specialist AI squads, sets monthly spending caps, and authorizes high-risk actions.
2. **The Autonomous AI Navigator:** Dispatches quests, tracks map step progress, assigns specialist agents, and surfaces risk discoveries.
3. **The Specialist AI Crew Member:** Executes bounded tasks using sandboxed tools, collects evidence, and produces structured deliverables.

---

## 4. Part III: Architecture & Technical Design Plan

### 4.1. Entity Relationship Model

```
                    ┌────────────────────────┐
                    │      Workspace         │
                    └───────────┬────────────┘
                                │ 1:N
                    ┌───────────▼────────────┐
                    │       Project          │
                    └───────────┬────────────┘
                                │ 1:N
┌──────────────────┐            │            ┌──────────────────┐
│      Ship        │ 1:N        │        1:N │   JournalSession │
│  - Charter       ├────────────┼───────────►│  - Scratchpad    │
│  - Berths        │            │            │  - ConvertToWork │
└────────┬─────────┘            │            └──────────────────┘
         │ 1:N                  │
┌────────▼─────────┐ 1:N        ▼        1:N ┌──────────────────┐
│   CrewMember     ├────────► Quest ◄────────┤  CaptainApproval │
│  - Authority     │       - MapSteps        │  - Risk Tier     │
│  - Scoped Tools  │       - Status          │  - Dollar Impact │
└──────────────────┘            │ 1:N        └──────────────────┘
                                ▼
                       ┌──────────────────┐
                       │     Artifact     │
                       │  - Discoveries   │
                       │  - Evidence      │
                       │  - Treasure Flag │
                       └──────────────────┘
```

### 4.2. Storage Layer & Concurrency
* **Database Engine:** Embedded SQLite with WAL mode (`fleet.db`) or embedded Sled key-value store, ensuring zero external database dependencies.
* **State Synchronization:** Gateway broadcasts state changes across the unified WebSocket bus (`/ws/fleet`) and Server-Sent Events (`/api/events`).
* **Audit Enforcement:** Every write to Ships, Quests, Approvals, and Treasury generates an immutable `LogbookEntry` with correlation ID and cryptographic signature.

---

## 5. Part IV: Production API Specification (OpenAPI 3.1 Standard)

### 5.1. Ship & Fleet Management (`/api/ships`, `/api/fleet`)

#### `GET /api/ships`
* **Summary:** List all configured Ships, active voyages, and charter constraints.
* **Headers:** `Authorization: Bearer <token>`
* **Response (200 OK):**
```json
[
  {
    "id": "ship-dev",
    "name": "Developer Delivery Ship",
    "fleetId": "fleet-diezy",
    "tagline": "Persistent specialist team for delivery quality and release readiness.",
    "homeScope": "engineering",
    "navigatorName": "Horizon (Orchestrator)",
    "status": "active",
    "activeVoyagesCount": 1,
    "monthlySpentUSD": 4.10,
    "crewIds": ["crew-repo-analyst", "crew-eng-planner", "crew-qa-reviewer"],
    "charter": {
      "purpose": "Maintain delivery quality under read-first policy.",
      "acceptedQuestTypes": ["repository_health", "ci_triage", "release_readiness"],
      "crewAuthority": "Read repository and run tests. Writes require Captain's Approval.",
      "prohibitedActions": ["Direct merge to main", "Production deployment"],
      "budgetPerVoyageUSD": 2.00,
      "monthlyBudgetUSD": 25.00,
      "memorySharing": "ship_scoped"
    },
    "createdAt": "2026-09-20T10:00:00Z",
    "updatedAt": "2026-09-29T12:00:00Z"
  }
]
```

#### `POST /api/ships`
* **Summary:** Commission a new Ship with multi-squad berths and charter limits.
* **Request Body:**
```json
{
  "name": "Security & SRE Frigate",
  "navigatorName": "Atlas (Orchestrator)",
  "tagline": "Continuous penetration auditing and canary rollback automation.",
  "homeScope": "engineering",
  "crewIds": ["crew-sec-auditor", "crew-sre-lead"],
  "charter": {
    "purpose": "Harden perimeter and monitor telemetry.",
    "acceptedQuestTypes": ["security_audit", "canary_verification"],
    "crewAuthority": "Read-only access to infrastructure logs.",
    "prohibitedActions": ["Modifying firewall rules without Captain sign-off"],
    "budgetPerVoyageUSD": 3.00,
    "monthlyBudgetUSD": 45.00,
    "memorySharing": "ship_scoped"
  }
}
```
* **Response (201 Created):** Returns the created `Ship` object.

#### `POST /api/fleet/anchor`
* **Summary:** Emergency toggle to halt or resume all autonomous voyages across the Fleet.
* **Request Body:**
```json
{
  "dropped": true,
  "reason": "Emergency halt triggered by Captain: anomaly detected in test runner."
}
```
* **Response (200 OK):**
```json
{
  "isAnchorDropped": true,
  "affectedShipsCount": 3,
  "activeVoyagesHalted": 2,
  "correlationId": "cid-anchor-984210"
}
```

---

### 5.2. Quests & Living Map Steps (`/api/quests`)

#### `GET /api/quests`
* **Query Parameters:**
  * `workspace`: Filter by workspace string (e.g. `Diezy Labs`).
  * `project`: Filter by initiative project.
  * `status`: `ready` | `underway` | `awaiting_captain` | `review` | `completed` | `archived`.
  * `shipId`: Filter by assigned Ship.
* **Response (200 OK):** Array of `Quest` objects with nested `MapStep` arrays.

#### `POST /api/quests`
* **Summary:** Launch or schedule a new Quest workflow.
* **Request Body:**
```json
{
  "title": "Audit Branch feat/enhance-agent-phase2 for Release",
  "objective": "Verify AST integrity, execute regression suite, and stage PR draft.",
  "workspaceId": "Diezy Labs",
  "projectId": "galleon-fleet",
  "priority": "high",
  "assignedShipId": "ship-dev",
  "requiredArtifacts": ["Release Readiness Brief", "Regression Test Evidence"],
  "budgetLimitUSD": 2.50,
  "mapSteps": [
    {
      "stepNumber": 1,
      "title": "Analyze repository diff and AST modifications",
      "assignedCrewId": "crew-repo-analyst"
    },
    {
      "stepNumber": 2,
      "title": "Execute deterministic regression test suite",
      "assignedCrewId": "crew-qa-reviewer"
    },
    {
      "stepNumber": 3,
      "title": "Stage Captain Approval for GitHub PR draft",
      "assignedCrewId": "crew-eng-planner"
    }
  ]
}
```
* **Response (201 Created):** Returns initialized `Quest` with unique ID and `ready` status.

#### `POST /api/quests/{id}/voyage`
* **Summary:** Trigger execution of the Quest voyage across assigned specialist crew.
* **Response (202 Accepted):**
```json
{
  "questId": "quest-17275892",
  "voyageId": "voyage-9031",
  "status": "underway",
  "streamEndpoint": "/api/quests/quest-17275892/stream"
}
```

#### `GET /api/quests/{id}/stream`
* **Summary:** Server-Sent Events (SSE) feed streaming map step transitions, discoveries, and token burn in real time.

---

### 5.3. Evidence-Backed Artifacts & Treasure (`/api/artifacts`)

#### `GET /api/artifacts`
* **Query Parameters:**
  * `questId`: Filter by originating quest.
  * `status`: `needs_review` | `approved` | `treasure`.
  * `type`: `health-brief` | `ci-triage` | `readiness-checklist` | `content-strategy` | `adr-draft`.
* **Response (200 OK):**
```json
[
  {
    "id": "art-ci-triage-1",
    "questId": "quest-ci-triage",
    "shipId": "ship-dev",
    "producerCrewId": "crew-qa-reviewer",
    "title": "CI Regression & Teardown Root Cause Analysis",
    "type": "ci-triage",
    "summary": "Isolated persistent socket teardown race condition under concurrent test runs.",
    "content": "# CI Regression Brief\n\nExplicit socket timeout deadlines remediated flakiness.",
    "discoveries": [
      {
        "id": "disc-101",
        "type": "risk",
        "title": "Socket Teardown Hang",
        "detail": "Test goroutines leak socket handles without context timeout cancellation.",
        "evidenceSource": "crates/clawcrew-gateway/src/ws.rs:142"
      }
    ],
    "evidenceCount": 6,
    "voyageCostUSD": 0.15,
    "status": "needs_review",
    "createdAt": "2026-09-29T11:20:00Z"
  }
]
```

#### `POST /api/artifacts/{id}/promote-treasure`
* **Summary:** Owner promotes validated Artifact to Sovereign Treasure.
* **Response (200 OK):**
```json
{
  "id": "art-ci-triage-1",
  "status": "treasure",
  "promotedAt": "2026-09-29T12:45:00Z",
  "promotedBy": "Pirate King",
  "auditRecordId": "log-treasure-8812"
}
```

---

### 5.4. Sovereign Captain's Approval Desk (`/api/approvals`)

#### `GET /api/approvals`
* **Summary:** List pending and decided human-in-the-loop authorization gates.
* **Query Parameters:** `status=pending|approved|rejected`
* **Response (200 OK):**
```json
[
  {
    "id": "appr-github-draft",
    "questId": "quest-ci-triage",
    "shipId": "ship-dev",
    "crewId": "crew-eng-planner",
    "title": "Publish GitHub Issue Draft",
    "actionType": "github_issue_create",
    "targetResource": "diezy-realm/galleon-fleet#issues",
    "draftSummary": "Automated filing of CI teardown bug with reproduction script.",
    "justification": "Required to notify upstream maintainers of socket leak regression.",
    "effect": "Creates public issue draft in GitHub repository.",
    "costUSD": 0.05,
    "status": "pending",
    "createdAt": "2026-09-29T11:45:00Z"
  }
]
```

#### `POST /api/approvals/{id}/decide`
* **Summary:** Execute Captain approval or rejection.
* **Request Body:**
```json
{
  "decision": "approved",
  "comment": "Verified against CI logs. Safe to file draft issue.",
  "signature": "ed25519-sig-883a910f"
}
```
* **Response (200 OK):** Returns updated approval record and triggers downstream voyage continuation.

---

### 5.5. Captain’s Executive Journal (`/api/journal`)

#### `GET /api/journal/sessions`
* **Summary:** List private working sessions with note previews and workspace context.
* **Response (200 OK):**
```json
[
  {
    "id": "session-1",
    "title": "Product Direction & Multi-Ship Capacity",
    "updatedAt": "2026-09-29T10:45:00Z",
    "workspaceId": "Diezy Labs",
    "lastNote": "Evaluate whether Fleet capacity should be one Ship or five on Community vs Pro.",
    "isPinned": true,
    "isArchived": false,
    "messageCount": 14
  }
]
```

#### `POST /api/journal/sessions/{id}/convert`
* **Summary:** Instant one-click conversion of private executive dialogue into a Quest or Artifact.
* **Request Body:**
```json
{
  "target": "quest",
  "title": "Multi-Ship Capacity Architecture",
  "workspaceId": "Diezy Labs",
  "projectId": "galleon-fleet",
  "targetShipId": "ship-dev"
}
```
* **Response (201 Created):**
```json
{
  "convertedType": "quest",
  "entityId": "quest-984210",
  "deepLink": "/quests?selected=quest-984210"
}
```

---

### 5.6. Treasury Governance & Hard Budget Caps (`/api/treasury`)

#### `GET /api/treasury/summary`
* **Summary:** Real-time BYOK spend vs monthly budget cap, per-ship allocation, and model breakdown.
* **Response (200 OK):**
```json
{
  "currency": "USD",
  "period": "2026-09",
  "totalSpentUSD": 4.10,
  "monthlyCapUSD": 25.00,
  "spentPercentage": 16.4,
  "isAnchored": false,
  "shipSpend": [
    { "shipId": "ship-dev", "name": "Developer Delivery Ship", "spentUSD": 4.10, "capUSD": 25.00 },
    { "shipId": "ship-market", "name": "Marketing Launch Ship", "spentUSD": 0.00, "capUSD": 15.00 }
  ],
  "modelSpend": [
    { "model": "claude-3-7-sonnet", "tokens": 82000, "costUSD": 2.46 },
    { "model": "gemini-2-5-pro", "tokens": 145000, "costUSD": 1.45 },
    { "model": "deepseek-r1-local", "tokens": 620000, "costUSD": 0.00 }
  ]
}
```

#### `GET /api/treasury/ledger`
* **Query Parameters:** `limit=50&offset=0&shipId=ship-dev`
* **Response (200 OK):** Paginated chronological list of LLM token dispatches with cost attribution.

---

## 6. Implementation Roadmap

| Phase | Milestone | Scope | Deliverables |
|---|---|---|---|
| **Phase 2.1** | **Core Fleet REST Engine** | Ships, Crew Berths, and Charters | Axum router integration in `crates/clawcrew-gateway/src/api_fleet.rs`, SQLite table migration, and full TypeScript SDK client. |
| **Phase 2.2** | **Quests & Artifact Pipeline** | Living Map Steps & Treasure Registry | Quest state machine, voyage execution goroutine, artifact storage, and SSE streaming pipeline. |
| **Phase 2.3** | **Sovereign Governance** | Captain's Desk & Hard Treasury Enforcement | Risk tier cryptographic gating, Voyage budget limiter middleware, and Drop Anchor kill-switch. |
| **Phase 2.4** | **Backend-Exposed UI Surfaces** | A2A Explorer, Voice Orb, Live Canvas | React views in `web-2` connecting directly to existing `/a2a`, `/ws/voice`, `/api/canvas`, and `/api/doctor` endpoints. |

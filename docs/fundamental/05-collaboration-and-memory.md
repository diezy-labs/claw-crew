## 12. Inter-Agent Collaboration
## 12.1 Differentiation

The product’s core differentiation is not generic multi-agent delegation.

> **Persistent AI specialists collaborate through explicit Artifacts, controlled handoffs, quality gates, scoped memory, and shared objectives—not temporary sub-agents that only return a summary.**

Formula:

```text
Persistent roles
+ explicit artifacts
+ controlled handoffs
+ quality gates
+ scoped memory
+ approved learning
+ approval-gated action
= a real AI team, not a swarm of temporary agents
```
## 12.2 Collaboration patterns

### Sequential handoff

```mermaid
flowchart LR
    A[Analyst] -->|Brief Artifact| B[Producer]
    B -->|Draft Artifact| C[Reviewer]
    C -->|Review Artifact| N[Navigator]
    N --> Q[Quartermaster]
    Q --> U[Owner Decision]
```

### Parallel investigation

```mermaid
flowchart TD
    N[Navigator] --> A[Research Specialist]
    N --> B[Technical Specialist]
    N --> C[Risk Reviewer]
    A --> D[Evidence Artifacts]
    B --> D
    C --> D
    D --> N
    N --> Q[Quartermaster]
```

### Producer-reviewer loop

```mermaid
flowchart LR
    P[Producer] --> D[Draft Artifact]
    D --> R[Reviewer]
    R -->|Accepted| N[Navigator]
    R -->|Revision request| P
```

Default rules:

- Maximum one or two revision cycles.
- Escalate to Quartermaster after the limit.
- Never allow endless agent-to-agent revisions.

### Owner escalation

```mermaid
flowchart TD
    S[Specialist] --> X{Can proceed safely?}
    X -->|Yes| A[Create Artifact]
    X -->|No / unclear| N[Navigator]
    N --> Q[Quartermaster]
    Q --> D[Decision Request]
    D --> U[Owner]
    U --> Q
```
## 12.3 Cross-Ship Quest example

```mermaid
sequenceDiagram
    participant PK as Pirate King
    participant QM as Quartermaster
    participant MB as Mission Board
    participant DN as Developer Navigator
    participant MN as Marketing Navigator
    participant DC as Developer Crew
    participant MC as Marketing Crew

    PK->>QM: Prepare product launch
    QM->>MB: Create Launch Quest
    MB->>DN: Assign technical readiness sub-Quest
    MB->>MN: Assign launch campaign sub-Quest

    DN->>DC: Analyze feature, release risk, limitations
    DC-->>DN: Technical Summary Artifact
    DN-->>MN: Approved Feature Brief reference

    MN->>MC: Research audience and prepare content
    MC-->>MN: Campaign Artifacts
    MN-->>QM: Marketing Ship Report

    DN-->>QM: Developer Ship Report
    QM->>QM: Create Executive Launch Brief
    QM-->>PK: Decisions needed: launch date, positioning, content approval
```
## 12.4 Collaboration rules

- Use Artifact references, not unrestricted transcript forwarding.
- Share only the minimum context required by the next Ship/Squad/Crew.
- Cross-Ship memory sharing is blocked by default.
- Every delegation carries task ID, parent/child correlation, budget, timeout, allowed tools, and Artifact contract.
- Navigator controls Ship-level delegation; Crew Members do not recursively spawn unlimited agents.
- Default delegation depth for Community is one: Quartermaster/Navigator to Crew or temporary agent.

---
## 13. Memory and Learning Model
## 13.1 Learning principle

> The Fleet learns the Owner’s process through approved feedback—not through uncontrolled automatic self-modification.

Feedback may become:

- Personal preference memory.
- Workspace terminology/knowledge.
- Project decision record.
- Ship procedure/rule.
- Crew-specific lesson.
- One-time run note.
## 13.2 Memory hierarchy

```mermaid
flowchart TD
    FM[Fleet Memory\nOwner preferences and organization strategy]
    WM[Workspace Memory\nCompany/client/environment knowledge]
    PM[Project Memory\nInitiative context and decisions]
    SM[Ship Memory\nTeam procedures and domain knowledge]
    CM[Crew Memory\nSpecialist lessons]
    RM[Run Memory\nTemporary task context]

    FM --> Q[Quartermaster]
    WM --> Q
    PM --> Q
    SM --> S[Ship Crew]
    CM --> C[Crew Member]
    RM --> R[Current Voyage]
```

| Scope | Example | Default access |
|---|---|---|
| Fleet | Owner preferences, global priorities, Fleet Code | Quartermaster; selectively summarized |
| Workspace | Client terminology, workspace documents, allowed integrations | Authorized Ships/Quartermaster |
| Project | Launch scope, milestone, decision history | Assigned Ship(s)/Quartermaster |
| Ship | Release conventions, reusable procedures, domain rules | Ship Crew; Quartermaster summary |
| Crew | Specialist lessons, quality patterns | Owning Crew; summary by policy |
| Voyage | Temporary task context | Relevant run only |
| Restricted | Sensitive people/finance/client data | Explicit policy/access grant only |
## 13.3 Proposed learning flow

```mermaid
flowchart LR
    A[Artifact created] --> B[Owner reviews]
    B --> C{Decision}
    C -->|Approve| D[Approved example]
    C -->|Edit| E[Correction]
    C -->|Reject| F[Exception / rejection reason]
    D --> G[Memory or rule proposal]
    E --> G
    F --> G
    G --> H{Owner confirms scope?}
    H -->|Fleet| I[Fleet memory]
    H -->|Workspace/Project| J[Context memory]
    H -->|Ship/Crew| K[Operational memory]
    H -->|One-time| L[Run history only]
    I --> M[Future work improves]
    J --> M
    K --> M
    L --> M
```

Example UX:

```text
You changed the release brief.

What should the Developer Ship remember?

[ ] Always list blockers before recommended actions
[ ] Use this changelog structure for this project
[ ] Require evidence links for every risk
[ ] This was a one-time correction only

[Save proposed rule] [Not now]
```

---

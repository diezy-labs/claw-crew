# UI and UX Design

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Navigation structure

```text
Fleet Command
├── Command Deck
├── Ships
├── Quartermaster
├── Voyages
├── Crew Members
├── Skills
├── Approvals
├── Fleet Budget
├── Fleet Knowledge
├── Ports & Providers
├── Policies
└── Ship Logs
```

## Command Deck

```text
Pirate King Command Deck

Fleet Health: Stable
Ships: 4 active / 1 docked
Active Voyages: 6
Crew Working: 13
Awaiting Approvals: 2
Budget: 38% daily / 24% monthly
Provider Health: 3 healthy / 1 degraded

Quartermaster Brief
- Development Ship needs a decision on an MCP pilot.
- Marketing Ship has two drafts ready for review.
- Research Ship completed provider-routing evidence collection.
- One model route crossed a soft budget threshold.

[View Decision Briefs] [Open Approval Queue] [View All Ships]
```

## Quartermaster page

Sections:

- [ ] Current mission and status.
- [ ] Fleet priority queue.
- [ ] Report collection status.
- [ ] Decision briefs.
- [ ] Escalations.
- [ ] Cross-Ship handoff proposals.
- [ ] Budget/watch alerts.
- [ ] Fleet lesson proposals.
- [ ] Recent actions/audit summary.

## Ship card

```text
Development Ship
Status: Working
Captain: Engineering Lead
Crew: 6 members
Active Voyages: 2
Blocked: 1
Budget: $6.24 / $20.00
Ports: GitHub (pending), Local Ollama (healthy), OpenRouter (healthy)

Latest report:
Tool Calling threat-model complete. MCP pilot decision required.

[Open Ship] [View Report] [Pause Ship]
```

## Ship detail page — crew action bar

Inside a Ship detail view, the Pirate King sees action buttons for crew management:

```text
┌─────────────────────────────────────────────────────────────────┐
│ Development Ship                                    [Pause Ship]│
│ Status: Working · Captain: Engineering Lead · Crew: 6 members   │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Squads (2)                                                     │
│  ┌──────────────────┐  ┌──────────────────┐                     │
│  │ Developer Squad   │  │ QA Squad          │                    │
│  │ 4 members         │  │ 2 members         │                    │
│  └──────────────────┘  └──────────────────┘                     │
│                                                                 │
│  [+ New Squad]  [+ Crew Member]                                 │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

- **[+ New Squad]** — opens the AI-assisted Squad Builder (see below).
- **[+ Crew Member]** — adds a single member to an existing Squad.

## Crew Members submenu

```text
Crew Members
├── Overview
├── Squads
├── Member Directory
├── Role Templates
├── Skills
├── Tool Permissions
├── Model Profiles
├── Performance
└── Activity
```

---

## AI-assisted Squad Builder

When the Pirate King clicks **[+ New Squad]**, the system opens an interactive creation flow where the Quartermaster researches and proposes an ideal crew composition.

### Step 1 — Natural language prompt

The Pirate King describes the squad they need in natural language:

```text
┌─────────────────────────────────────────────────────────────────┐
│ Create New Squad                                                │
│                                                                 │
│ What kind of squad do you need?                                 │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │ Tolong buatkan saya 1 squad tim marketing                  │ │
│ │                                                             │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                                                                 │
│ Or choose a template:                                           │
│ [Developer Squad]  [Marketing Squad]  [Research Squad]          │
│ [Operations Squad] [Custom...]                                  │
│                                                                 │
│                                        [Cancel]  [Ask QM →]    │
└─────────────────────────────────────────────────────────────────┘
```

- [ ] Free-text input for natural language squad description.
- [ ] Quick-pick template buttons for common squad types.
- [ ] **[Ask QM →]** sends the request to Quartermaster for research.

### Step 2 — Quartermaster research phase

After the Pirate King submits the request:

```text
┌─────────────────────────────────────────────────────────────────┐
│ Quartermaster is assembling your crew...                        │
│                                                                 │
│  ⟳ Researching ideal marketing team composition                │
│  ✓ Analyzing real-world industry standards                      │
│  ⟳ Mapping skills and roles for each position                  │
│  ○ Preparing crew proposal                                      │
│                                                                 │
│  Estimated time: ~15 seconds                                    │
│                                                                 │
│                                                       [Cancel]  │
└─────────────────────────────────────────────────────────────────┘
```

The Quartermaster performs the following behind the scenes:

- [ ] Research real-world team composition for the requested domain (e.g., marketing teams typically have 5-8 specialized roles).
- [ ] Map industry-standard roles to Galleon Fleet Crew Member definitions.
- [ ] Assign recommended skills, tool permissions, and model profiles per member.
- [ ] Consider the Ship's existing policy ceiling and budget when sizing the squad.
- [ ] Generate a structured `SquadCompositionProposal` with rationale.

### Step 3 — Proposal review modal

Quartermaster presents a full proposal popup for the Pirate King to review and edit:

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ Squad Proposal from Quartermaster                                        │
│                                                                          │
│ Squad: Marketing Squad                                                   │
│ Domain: marketing                                                        │
│ Rationale: Based on industry-standard digital marketing teams,           │
│ a squad of 7 members covers strategy, research, content                  │
│ creation, SEO, analytics, and brand governance.                          │
│                                                                          │
│ ┌──────────────────────────────────────────────────────────────────────┐ │
│ │ # │ Role              │ Model AI            │ Skills         │ Act. │ │
│ │───│───────────────────│─────────────────────│────────────────│──────│ │
│ │ 1 │ Marketing Lead ★  │ gemini-2.5-pro      │ Campaign, Pos  │[✎][🗑]│
│ │ 2 │ Market Researcher  │ gemini-2.5-flash    │ Source, Synth  │[✎][🗑]│
│ │ 3 │ SEO Strategist     │ gemini-2.5-flash    │ Keyword, Gap   │[✎][🗑]│
│ │ 4 │ Copywriter         │ gemini-2.5-pro      │ Voice, Draft   │[✎][🗑]│
│ │ 5 │ Content Editor     │ gemini-2.5-flash    │ Editorial, Val │[✎][🗑]│
│ │ 6 │ Growth Analyst     │ gemini-2.5-flash    │ Analytics, Exp │[✎][🗑]│
│ │ 7 │ Brand Strategist   │ gemini-2.5-pro      │ Framework, Cr  │[✎][🗑]│
│ └──────────────────────────────────────────────────────────────────────┘ │
│                                                                          │
│ ★ = proposed Captain                                                     │
│ Model AI selected by Quartermaster based on role complexity.             │
│ Click [✎] on any member to change the model.                            │
│                                                                          │
│ [+ Add Member]                                                           │
│                                                                          │
│ Budget estimate: ~$5.00/day for 7 members                                │
│ Ship budget remaining: $13.76 / $20.00                                   │
│                                                                          │
│                                   [Cancel]  [Submit & Create Squad]      │
└──────────────────────────────────────────────────────────────────────────┘
```

### Model AI recommendation by Quartermaster

The Quartermaster selects the optimal AI model for each crew member based on:

| Factor | How Quartermaster decides |
|---|---|
| Role complexity | Strategic/creative roles (Lead, Copywriter, Brand Strategist) → larger model (e.g., `gemini-2.5-pro`). Execution/lookup roles (Researcher, Analyst) → faster model (e.g., `gemini-2.5-flash`). |
| Skill requirements | Roles needing deep reasoning, multi-step planning, or nuanced output get a more capable model. |
| Cost optimization | Quartermaster balances model capability against Ship/Fleet budget ceiling — uses the cheapest adequate model. |
| Ship/Fleet model allowlist | Only models permitted by Ship and Fleet policy are proposed. |
| Provider availability | Quartermaster checks provider health and fallback routes when recommending. |

The Pirate King can **always override** the Quartermaster's model recommendation:

- [ ] In the proposal modal via **[✎]** on any member.
- [ ] After squad creation via **Crew Member detail → Model Profile**.
- [ ] At Ship level via **Ship Settings → Model Allowlist** (affects all members).
- [ ] At Fleet level via **Fleet Policies → Provider/Model Ceiling** (affects all Ships).

### Pirate King actions on the proposal

| Action | Button | Behavior |
|---|---|---|
| Edit member | **[✎]** | Opens inline editor to change role name, mission, skills, tool permissions, model profile |
| Remove member | **[🗑]** | Removes the member from the proposal (with undo) |
| Add member | **[+ Add Member]** | Adds a blank member row or prompts for role description |
| Change Captain | Click **★** on another row | Reassigns the Captain designation |
| Cancel | **[Cancel]** | Discards the entire proposal, no Squad created |
| Submit | **[Submit & Create Squad]** | Sends the finalized composition to Go engine for creation |

### Step 3a — Inline member editor

When the Pirate King clicks **[✎]** on a crew member:

```text
┌─────────────────────────────────────────────────────────────────┐
│ Edit Crew Member                                                │
│                                                                 │
│ Display Name:  [SEO Strategist          ]                       │
│ Mission:       [Find content opportunity and on-page issues   ] │
│                                                                 │
│ ── Model AI ──────────────────────────────────────────────────  │
│ Provider:  [OpenRouter ▾]  [Ollama (local) ▾]  [Google AI ▾]   │
│ Model:     [gemini-2.5-flash ▾]                                 │
│            ⓘ Recommended by QM: gemini-2.5-flash                │
│            Reason: Lookup/analysis role — fast model sufficient  │
│            Est. cost: ~$0.40/day                                 │
│                                                                 │
│ ── Skills ────────────────────────────────────────────────────  │
│ [Keyword analysis] [Content gap] [Search intent] [+ Add]       │
│                                                                 │
│ ── Tool Permissions ──────────────────────────────────────────  │
│ ☑ Web crawl/read    ☑ Analytics read    ☐ CMS write            │
│ ☑ Report generation  ☐ External publish                         │
│                                                                 │
│ ── Approval Policy ───────────────────────────────────────────  │
│ ◉ CMS changes require Pirate King approval                     │
│ ◉ External publish denied by default                            │
│                                                                 │
│                                        [Cancel]  [Save]         │
└─────────────────────────────────────────────────────────────────┘
```

- [ ] Pirate King can edit display name, mission statement, and skills.
- [ ] **Model AI is a first-class editable field** — Pirate King can override QM's recommendation.
- [ ] Provider dropdown shows only healthy providers allowed by Ship/Fleet policy.
- [ ] Model dropdown filters to models available from the selected provider.
- [ ] QM recommendation shown as info tag with reasoning and cost estimate.
- [ ] Skills shown as tags with add/remove capability.
- [ ] Tool permissions as checkboxes constrained by Ship/Fleet policy ceiling.
- [ ] Approval policy rules inherited from Ship policy, overridable per member.

### Step 4 — Confirmation and creation

After **[Submit & Create Squad]**:

```text
┌─────────────────────────────────────────────────────────────────┐
│ ✓ Marketing Squad created successfully                          │
│                                                                 │
│ Ship: Development Ship                                          │
│ Members: 7 crew members activated                               │
│ Captain: Marketing Lead                                         │
│ Budget allocated: $5.00/day                                     │
│                                                                 │
│ [Open Squad]  [View Ship]  [Close]                              │
└─────────────────────────────────────────────────────────────────┘
```

Go engine actions on submit:

- [ ] Validate all members against Ship and Fleet policy ceilings.
- [ ] Create Squad entity with all Crew Members.
- [ ] Initialize per-member tool/model/memory policy references.
- [ ] Allocate Squad budget from Ship budget.
- [ ] Emit `squad.created` and `crew_member.created` events per member.
- [ ] Record proposal → approval audit trail.

## Approval UX

Fleet-level approvals must be clearly distinct from Ship-level approvals:

```text
Fleet Approval Required

Action: Transfer a confidential research artifact from Research Ship to Marketing Ship
Requested by: Quartermaster
Purpose: Create evidence-backed content brief
Transfer mode: Redacted summary only
Policy: fleet-data-sharing-v1

[View Source Summary] [Approve Transfer] [Deny]
```

## Visual style guidance

- [ ] Use clean information hierarchy first.
- [ ] Use pirate language in labels/empty states/illustration, not in every technical field.
- [ ] Show clear status color semantics.
- [ ] Avoid decorative overload such as excessive skulls, maps, or cartoon elements in operational screens.
- [ ] Use subtle nautical visual cues: ship icons, compass indicators, port markers, logbook cards.
- [ ] Make professional mode possible through terminology settings if needed.

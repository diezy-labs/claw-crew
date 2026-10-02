## 5. Brand Narrative
## 5.1 The Independent Fleet

Galleon Fleet uses a maritime/exploration narrative as a product world. It is not a claim that AI should be lawless, reckless, or uncontrolled.

The intended meaning is:

```text
Freedom to choose
→ no forced provider, model, cloud, or credit gate.

Freedom to build
→ create the specialist teams required by the work.

Freedom to own
→ keep artifacts, memory, rules, and configurations portable.

Freedom with command
→ define policies, budgets, permissions, approvals, and boundaries.

Freedom to explore
→ turn unknown problems into evidence, decisions, and valuable outcomes.
```

The proper framing is:

> **Independent, not ungoverned.**
>
> **Autonomous work, under your command.**
## 5.2 Core narrative

```text
You are the Pirate King.

Quartermaster is your AI executive.
It understands your goals, coordinates your Fleet,
watches the Treasury, reads the Logbook,
and brings meaningful decisions back to you.

Every Quest begins with a Map.
Every Voyage produces Artifacts.
Every useful Discovery can become Treasure.

Your Fleet. Your Rules.
```
## 5.3 Vocabulary system

| Narrative term | Functional subtitle | Canonical meaning |
|---|---|---|
| Pirate King | Owner | Human final decision maker |
| Quartermaster | Executive Orchestrator | Fleet-level personal super-agent and executive coordinator |
| Fleet / Dermaga | Organization | Top-level AI operating environment |
| Ship | Persistent AI Team | Operational home for a specialist AI team |
| Navigator | Ship Orchestrator | Ship-level planner, router, and report owner |
| Squad | Functional Team | Group of Crew Members serving a capability/function |
| Crew Member | AI Specialist | Persistent worker with defined role, skills, tools, memory, and authority |
| Mission Board | Work Queue | Fleet-wide Quest intake, priority, and routing board |
| Quest | Workflow / SOP / Mission | Repeatable or one-off objective with a defined outcome |
| Map | Plan / Playbook | Execution plan for a Quest |
| Voyage | Run / Execution | One execution of a Quest or sub-Quest |
| Artifact | Deliverable / Evidence | Report, brief, draft, patch, checklist, evidence bundle, or decision record |
| Discovery | Finding / Insight | Important evidence, risk, opportunity, unknown, or recommendation |
| Treasure | Validated Outcome | Artifact/decision/outcome validated as valuable |
| Fleet Code | Policy & Permissions | Governance, tools, approvals, budget, retention, and sharing rules |
| Ship Charter | Team Blueprint | Purpose, Crew, Quest types, skill bindings, artifact contracts, and authority of a Ship |
| Logbook | Audit & Activity Log | Event, decision, run, and learning history |
| Treasury | Provider Cost & Budget | Cost ledger, budget, allocation, warnings, and caps |
| Harbor | Integrations | Model providers, local endpoints, tools, APIs, channels, and connectors |
| Compass | Setup & Guidance | Onboarding and next-best-action guidance |
| Crow’s Nest | Monitoring | Health, alerts, logs, traces, and metrics |
| Captain’s Approval | Approval Queue | Owner review for external actions |
| Shipyard | Capacity & Upgrade | Fleet capacity, additional Ships, Crew berths, and licensing |
| Timber | Capacity metaphor | Visual metaphor for Fleet capacity; never an inference-credit currency |
| Drop Anchor | Emergency Stop | Pause/cancel automated work safely |

### Terminology rules

- Use narrative terms in onboarding, dashboards, product marketing, empty states, and progressive UX.
- Use functional subtitles on safety, cost, data, legal, and technical screens.
- Keep canonical source-code, API, database, and SDK language neutral: `fleet`, `workspace`, `project`, `ship`, `squad`, `crew`, `workflow`, `run`, `artifact`, `policy`, `approval`, `audit_event`, `budget`, `integration`.
- Do not encode lore-dependent terms into irreversible technical namespaces where a future terminology change would be expensive.

---
## 6. Core User Roles

| Role | Product identity | Authority |
|---|---|---|
| **Pirate King** | User / Owner | Defines strategy, budget, policy, secret grants, high-impact approvals, and final decisions |
| **Quartermaster** | AI CEO / Executive Orchestrator | Understands goals, prioritizes work, reads summaries, coordinates Ships, tracks cost/health/risk, creates briefings, requests decisions |
| **Navigator** | Ship-level Orchestrator | Accepts/routs Quest work, creates Maps, delegates to Squads/Crew, manages Ship execution, prepares Ship Reports |
| **Squad** | Functional team | Performs a related set of work such as delivery, research, marketing, review, or operations |
| **Crew Member** | AI Specialist | Executes bounded tasks using scoped skills/tools/memory and produces typed Artifacts |
| **Temporary Agent** | Short-lived delegate | Completes one-off work under a strict time, budget, tool, and memory scope |

---

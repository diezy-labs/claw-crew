## 11. Implementation Plan

## Phase 0 — Design foundation

Deliverables:

- Design tokens and dark/light theme primitives.
- New application shell, sidebar, context bar, command palette.
- Terminology mapping guidelines.
- Page migration map from current pages to Fleet surfaces.
- Empty/loading/error state library.

Success criteria:

- Current functionality remains accessible.
- Navigation is grouped by command/fleet/operations/control.
- No new feature work is buried inside `Dashboard.tsx`.

## Phase 1 — Quartermaster and Artifact-first experience

Deliverables:

- Quartermaster Office page built from existing chat/workspace capabilities.
- Executive briefing card system.
- Artifact Gallery and Artifact detail page/drawer.
- Chat-to-artifact promotion action.
- Right-rail Fleet Pulse using existing metrics/health/provider data where available.

Success criteria:

- User can ask Quartermaster a question, save a result as Artifact, and find it later.
- User can see cost/approval/health summary without opening technical pages.

## Phase 2 — Mission Board and Ship experience

Deliverables:

- Mission Board UI built over existing TaskBoard, Runs, SOPs, and Cron concepts.
- Ship overview page and Crew Member cards built over existing agents list/config.
- Quest lifecycle UI and basic Map view.
- Ship Charter readable view.

Success criteria:

- User can create a Quest, assign a Ship/Crew, watch Voyage progress, and review output.
- Existing TaskBoard/Runs remain reachable through the new UX.

## Phase 3 — Build a Ship / Make Me a Squad

Deliverables:

- Guided Ship/Squad wizard.
- Developer Delivery Ship official blueprint.
- Crew draft/review/activation flow.
- Model/profile/tool scope/policy/budget configuration in progressive layers.
- Community capacity presentation: one active Ship, five persistent Crew.

Success criteria:

- New user can build a Developer Ship without seeing low-level agent configuration first.
- Advanced user can inspect/edit all relevant settings.

## Phase 4 — Governance and cost surfaces

Deliverables:

- Captain’s Approval page over existing approvals capability.
- Treasury page over provider/model/cost data.
- Fleet Code page over policy/config capability.
- Logbook over audit/activity history.
- Crow’s Nest consolidation for Logs/Metrics/Doctor/Recovery/health.

Success criteria:

- Approval action shows exact impact and evidence.
- Treasury clearly distinguishes product capacity from provider usage cost.
- Technical diagnostics remain accessible without polluting main user flow.

## Phase 5 — Advanced collaboration

Deliverables:

- Navigator briefing and Ship Report experience.
- Cross-Ship Quest visualization.
- Artifact references/handoff UI.
- Proposed memory/rule review UI.
- Temporary agent and promote-to-Crew interaction.

Success criteria:

- User can understand which Ship performed which work and why.
- Quartermaster can present a cross-Ship executive briefing without exposing raw transcript chaos.

---


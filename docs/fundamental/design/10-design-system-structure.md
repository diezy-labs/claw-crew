## 10. Design System Structure

## 10.1 Recommended frontend module structure

Do not add all behavior to current large page files. The current `Dashboard.tsx`, `Config.tsx`, `Cron.tsx`, `Sops.tsx`, and `SopCanvas.tsx` are already large and should not become Fleet concept dumping grounds.

```text
web/src/
├── app/
│   ├── AppShell.tsx
│   ├── AppSidebar.tsx
│   ├── ContextBar.tsx
│   ├── RightRail.tsx
│   └── CommandPalette.tsx
├── design-system/
│   ├── tokens.css
│   ├── typography.css
│   ├── primitives/
│   │   ├── Button.tsx
│   │   ├── Card.tsx
│   │   ├── Badge.tsx
│   │   ├── Drawer.tsx
│   │   ├── EmptyState.tsx
│   │   ├── StatusPill.tsx
│   │   └── DataList.tsx
│   └── patterns/
│       ├── ArtifactCard.tsx
│       ├── QuestCard.tsx
│       ├── ShipCard.tsx
│       ├── CrewCard.tsx
│       ├── CostSummary.tsx
│       ├── ApprovalCard.tsx
│       └── ActivityTimeline.tsx
├── features/
│   ├── quartermaster/
│   ├── mission-board/
│   ├── fleet/
│   ├── ships/
│   ├── crew/
│   ├── quests/
│   ├── artifacts/
│   ├── approvals/
│   ├── treasury/
│   ├── logbook/
│   ├── harbor/
│   ├── fleet-code/
│   ├── crows-nest/
│   ├── shipyard/
│   └── onboarding/
├── pages/
│   ├── QuartermasterOfficePage.tsx
│   ├── MissionBoardPage.tsx
│   ├── ShipsPage.tsx
│   ├── ShipDetailPage.tsx
│   ├── ArtifactsPage.tsx
│   ├── ApprovalPage.tsx
│   ├── TreasuryPage.tsx
│   ├── LogbookPage.tsx
│   ├── HarborPage.tsx
│   ├── FleetCodePage.tsx
│   ├── CrowsNestPage.tsx
│   └── ShipyardPage.tsx
└── legacy-adapters/
    ├── dashboard-adapter.ts
    ├── agent-adapter.ts
    ├── runs-adapter.ts
    ├── sops-adapter.ts
    └── diagnostics-adapter.ts
```

## 10.2 State boundaries

| Domain state | UI ownership |
|---|---|
| Fleet/Ship/Crew/Quest/Artifact | Feature-specific query/cache/store |
| Selected Realm/Fleet/Workspace/Project | App context + URL state |
| Quartermaster conversation | Quartermaster feature state, persisted session reference |
| Command palette | App shell transient state |
| Wizard drafts | Local wizard state until submit; server draft after durable save |
| Technical configuration | Harbor/Fleet Code/Crow’s Nest features |
| Legacy gateway data | Adapter layer until API contracts stabilize |

## 10.3 API alignment

Use product-facing API concepts in the UI even if backend implementation is introduced incrementally:

```text
/fleets
/ships
/ships/{id}/crew
/mission-board/quests
/quests/{id}
/quests/{id}/map
/voyages
/artifacts
/approvals
/treasury
/logbook
/harbor
/fleet-code
/health
```

Until APIs exist, build UI adapters over current agent/run/SOP/task/tool/config surfaces. Avoid hard-coding legacy transport names into final component naming.

---


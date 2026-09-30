# State Management & Real-Time Synchronization Specification

> **Subsystem:** Frontend State Layer  
> **Library:** Zustand (`zustand@^5.0`) + React 19  
> **Source Path:** `src/store/fleetStore.ts`

---

## 1. Why Zustand?

Zustand was chosen over Redux or React Context due to:
1. **Zero Boilerplate:** Clean, minimal API without complex reducer/action boilerplate.
2. **Atomic Subscriptions:** Components subscribe strictly to the state slices they require, avoiding full-tree re-renders during high-frequency agent telemetry updates.
3. **Outside React Access:** State can be read and manipulated directly from background WebSocket message handlers, Tauri command callbacks, or simulation timers without requiring React hook context wrapping.
4. **Persistent Sync:** Easy integration with local storage or IndexedDB for offline-first resilience.

---

## 2. Store Structure & Slices

The global state in `useFleetStore` is structured into domain slices:

```typescript
interface FleetState {
  // Navigation & Theme
  theme: 'dark' | 'light';
  activeTab: NavigationTab;
  isCommandPaletteOpen: boolean;

  // Workspace & Organizational Hierarchy
  realmName: string;
  fleetName: string;
  selectedWorkspace: string;
  selectedProject: string;

  // Core Business Entities
  ships: Ship[];
  crew: CrewMember[];
  quests: Quest[];
  artifacts: Artifact[];
  approvals: CaptainApproval[];

  // Auditing & Accounting
  logbook: LogbookEntry[];
  treasuryLedger: TreasuryLedger[];
  notifications: NotificationItem[];
  chatMessages: ChatMessage[];

  // Selection Drawers
  selectedQuestId: string | null;
  selectedArtifactId: string | null;
}
```

---

## 3. End-to-End Action Lifecycle Example

### Example: Discovering a Blocker → Artifact → Approval → GitHub Write

1. **Intake / Chat:** Owner instructs Quartermaster in `QuartermasterOffice`:  
   *`"Triage the integration test timeout bug on the Developer Ship."`*
2. **Quest State Transition:**
   - Quartermaster routes the request to `Developer Delivery Ship`.
   - Quest status transitions: `ready` → `underway`.
   - `activeVoyageProgress` increments in real time.
3. **Artifact Generation:**
   - Specialist `QA & Risk Reviewer` completes Map Step 2 and generates `CI Triage Report #104`.
   - Discoveries array populated with: `Risk: Release Blocker Detected`.
   - Artifact saved to `artifacts` slice; notification dispatched.
4. **Captain’s Approval Request:**
   - Because creating a GitHub issue is classified under Risk Class `sensitive`, an approval item is inserted into `approvals` with status `pending`.
   - Badge counter on `Captain’s Approval` sidebar increments with real-time audio/visual alert.
5. **Human Command Execution:**
   - Owner visits `ApprovalsView`, reviews `Why Now`, `Target`, `Exact Effect`, and clicks `[Approve & Sign Action]`.
   - Approval transitions to `approved`.
   - Entry appended to `Logbook` with signature hash and actor `Pirate King (You)`.

---

## 4. Real-Time Notification & Simulation Hook

`src/hooks/useSimulation.ts` coordinates background updates:
- Runs an event loop updating active voyages in 8-second increments.
- Dispatches unread signals to the Notification Center popover in `ContextBar`.
- Simulates external webhook notifications and autonomous Map step advancements.

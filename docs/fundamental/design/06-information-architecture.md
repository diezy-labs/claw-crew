## 6. Information Architecture

## 6.1 Primary navigation

The sidebar should be compact, grouped, and role-aware.

```text
[Product Mark] Fleet AI / Working Brand

COMMAND
• Quartermaster
• Mission Board
• Artifacts
• Captain’s Approval   [badge]

FLEET
• Ships
• Crew Members
• Workspaces & Projects

OPERATIONS
• Treasury
• Logbook
• Harbor

CONTROL
• Fleet Code
• Crow’s Nest
• Shipyard

[Search / Command Palette]
[Owner profile + plan]
```

### Community simplified navigation

```text
• Quartermaster
• Mission Board
• My Ship
• Artifacts
• Captain’s Approval
• Treasury
• Settings
```

The full information architecture appears after the user activates advanced mode or has more than one Ship.

## 6.2 Context bar

Every work screen needs a context bar, analogous to project/workspace selectors in modern AI desktop tools:

```text
Realm / Fleet selector    Workspace selector    Project selector    Current Ship / Quest context
```

Example:

```text
Adiet’s Realm  ›  Diezy Labs Fleet  ›  Product Platform  ›  Developer Ship
```

Rules:

- Realm/Fleet identity must be visible but not dominate every page.
- Context switches must be deliberate and clear to prevent accidental cross-client/workspace data leakage.
- Ship context should appear whenever viewing Crew, Voyage, Artifact, or Charter records.

## 6.3 Command palette

Use `⌘K` / `Ctrl+K` as a central fast-navigation and action layer:

```text
Ask Quartermaster: “Prepare release readiness for Product Platform”
Create Quest
Open Mission Board
Go to Developer Ship
Review 2 approvals
Connect a provider
Open Treasury
Search Artifacts
Run Repository Health Quest
```

This is the advanced-user escape hatch inspired by desktop AI tools without replacing normal navigation.

---


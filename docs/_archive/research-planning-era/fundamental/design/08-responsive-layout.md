## 8. Responsive Layout

## 8.1 Desktop-first breakpoints

| Breakpoint | Layout |
|---|---|
| ≥1440px | Three-column command layout: sidebar, primary canvas, contextual right rail |
| 1024–1439px | Two-column layout: sidebar + canvas; right rail becomes collapsible drawer |
| 768–1023px | Tablet: sidebar collapses to icon rail; details become side sheet |
| <768px | Companion/mobile read-review experience; avoid building a full operations console first |

## 8.2 Desktop density modes

| Mode | User | Behavior |
|---|---|---|
| Comfortable | Default/owner | Generous whitespace, visible summaries, less technical metadata |
| Compact | Professional/technical | Dense tables, more metadata, list-first layout |
| Focus | Artifact reader/chat | Hides sidebar/right rail, preserves context breadcrumb |

## 8.3 Keyboard support

```text
⌘/Ctrl + K      Command palette
⌘/Ctrl + Enter  Send Quartermaster request / start selected action
G then Q        Mission Board
G then S        Ships
G then A        Artifacts
G then L        Logbook
G then T        Treasury
Esc             Close detail panel/drawer
```

Keyboard shortcuts must remain discoverable in command palette and help view.

---


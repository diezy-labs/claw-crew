## 5. Visual Direction

## 5.1 Design personality

```text
Premium
Calm
Precise
Capable
Warm but not cute
Maritime-inspired but not themed UI
Dark-mode-first with strong light-mode parity
```

The product should resemble:

```text
A modern executive command room
+ a high-quality AI desktop workspace
+ a disciplined developer tool
```

It should not resemble:

```text
A pirate game
A logistics fleet dashboard
A military command center
A cyberpunk terminal
A crowded observability console
```

## 5.2 Color system

### Base palette

| Token | Dark | Light | Purpose |
|---|---|---|---|
| `--bg-canvas` | `#121315` | `#F7F7F5` | App background |
| `--bg-surface` | `#191B1F` | `#FFFFFF` | Cards, panels |
| `--bg-elevated` | `#22252A` | `#F2F3F4` | Hover, elevated panels |
| `--border-subtle` | `#2C3036` | `#E1E3E5` | Dividers and card edges |
| `--text-primary` | `#F3F4F1` | `#17191C` | Main text |
| `--text-secondary` | `#A7ADB5` | `#5F6872` | Supporting text |
| `--text-muted` | `#747C86` | `#8B949E` | Metadata |
| `--accent-sea` | `#66C7C5` | `#087E8B` | Primary interactive accent |
| `--accent-treasure` | `#D8A94A` | `#A46F00` | Treasure/value/highlight, used sparingly |
| `--accent-sail` | `#EDE7D9` | `#4A4A42` | Off-white nautical neutral |
| `--success` | `#59B38A` | `#18794E` | Completed/healthy |
| `--warning` | `#E8B65B` | `#B26A00` | Risk/budget/attention |
| `--danger` | `#E16C68` | `#C83B3B` | Blocked/destructive/failure |
| `--info` | `#7AB7E8` | `#286FA7` | Informational state |

### Color usage rules

- Black/charcoal is the foundation, not pure black; use soft charcoal to reduce eye strain.
- White/off-white is used for readability and the ship mark.
- Teal is the main action color, representing sea, navigation, health, and active command.
- Gold is only for Treasure, meaningful completion, or premium/capacity—not primary buttons.
- Red is reserved for danger, blocking errors, destructive actions, and policy violations.
- Do not use a red/black pirate palette as the default visual language.

## 5.3 Typography

Use a clean sans-serif system stack or a single readable UI font already compatible with the web application.

Suggested hierarchy:

| Role | Size | Weight | Usage |
|---|---:|---:|---|
| Display | 28–32px | 600 | Quartermaster welcome, page title |
| Heading 1 | 22–24px | 600 | Primary section title |
| Heading 2 | 16–18px | 600 | Card/panel title |
| Body | 14–15px | 400 | Primary readable content |
| UI label | 12–13px | 500–600 | Navigation, state, metadata label |
| Mono | 12–13px | 400 | IDs, costs, code, logs, technical fields |

Rules:

- Do not use decorative pirate fonts in UI.
- A restrained serif may be used only in marketing/landing-page editorial headers, never for operational screens.
- Keep line length around 60–80 characters in Artifact reading views.

## 5.4 Iconography

- Use simple line icons with rounded 1.5–2px strokes.
- The product mark may use a minimal off-white sailing ship on charcoal background.
- Use narrative icons only when clear: compass, map, logbook, treasury, ship, sail, telescope/crow’s nest.
- For critical actions, use universal icons and plain language first: check, warning, lock, stop, edit, export.

---


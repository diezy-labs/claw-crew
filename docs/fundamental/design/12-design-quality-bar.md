## 12. Design Quality Bar

Every implementation should satisfy:

### Clarity

- A new user can name what screen they are on and what they can do next.
- Every narrative term has a clear functional meaning when stakes are high.
- Critical actions have plain-language impact text.

### Trust

- User can find source/evidence for an Artifact.
- User can see which Ship/Crew/Quest/Voyage produced it.
- User can see cost and permission implications.
- User can inspect and edit what the system remembers.

### Control

- User can pause a Ship, cancel a Voyage, reject approval, change budget, and edit policy without hunting through settings.
- High-impact actions remain visibly under Owner command.

### Performance

- App shell appears immediately.
- Heavy logs/metrics render on demand.
- Long-running work streams progress rather than blocking UI.
- Large Artifact content is virtualized/lazy loaded if necessary.

### Accessibility

- Keyboard navigation works across board, drawers, dialogs, command palette, and approval flows.
- Color is never the only status indicator.
- Contrast meets WCAG AA for core text and interaction.
- Statuses use icon + text + color.
- Motion is reduced with user OS preference.

---


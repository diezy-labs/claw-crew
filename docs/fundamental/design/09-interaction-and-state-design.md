## 9. Interaction and State Design

## 9.1 Status language

Use clear narrative + functional state pairing.

| Narrative | Functional state | Visual treatment |
|---|---|---|
| Backlog | Not prioritized | Neutral gray |
| Ready to Sail | Ready | Teal outline/fill accent |
| Underway | Running | Blue/teal progress |
| Awaiting Captain | Approval needed | Gold attention state |
| Blocked | Needs intervention | Amber/red depending severity |
| Review | Artifact review | Blue/purple neutral attention |
| Completed | Finished | Green |
| Treasure Claimed | Validated value | Gold accent, never excessive |
| Anchored | Paused | Gray muted |
| Lost at Sea | Failed | Red with recovery action |

## 9.2 Loading state

Avoid generic spinners for long AI work. Use Voyage progress cards:

```text
Developer Ship is underway

1. Reading repository structure               Complete
2. Mapping affected components                Complete
3. Reviewing CI evidence                      In progress
4. Preparing health brief                     Waiting

Estimated provider cost so far: US$0.14
[View Voyage] [Drop Anchor]
```

## 9.3 Error state

Errors should explain impact and recovery, not expose raw stack traces first.

```text
Quest paused: GitHub permissions need attention.

Developer Ship cannot read repository checks with the current token.
No external changes were attempted.

[Reconnect GitHub] [View technical detail] [Ask Quartermaster]
```

## 9.4 Empty states

Every empty state must guide one useful next action.

```text
No Artifacts yet
Start a Quest and your Fleet will return with reviewable evidence.

[Open Mission Board]
```

```text
No Ship in this Fleet
Quartermaster can help build a specialist team for the work you repeat.

[Build your first Ship]
```

---


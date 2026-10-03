# Definition of Done

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

The Quartermaster/Fleet Command feature is complete for its first production-ready release when:

- [ ] One Pirate King can create and manage multiple Ships within a Fleet.
- [ ] Every Ship has isolated policy, budget, workspace, memory, artifact, and audit boundaries.
- [ ] A Quartermaster exists as a bounded Fleet coordinator with no implicit high-risk authority.
- [ ] Quartermaster consumes Ship Summary Projections rather than unrestricted raw Ship data.
- [ ] Quartermaster can create Fleet Reports, Decision Briefs, and Escalations.
- [ ] Pirate King can approve/reject Fleet Order proposals and high-risk handoffs.
- [ ] Ships can have Captains, Squads, and Crew Members configured with real skill/tool/model/memory policies.
- [ ] Effective permissions are computed as an intersection of Fleet-to-task policy layers.
- [ ] Cross-Ship artifact transfer is policy-checked, approval-bound where required, and audited.
- [ ] Fleet and Ship budget thresholds are visible and enforced.
- [ ] Emergency freeze can safely halt configured routes without deleting audit history.
- [ ] TUI, Tauri, and web clients render the same canonical Go-engine state/events.
- [ ] Fleet reports include traceable source Ship report/artifact references.
- [ ] No credential, restricted raw data, or unauthorized workspace content leaks across Ship boundaries.
- [ ] Unit, integration, security, and race tests pass consistently.

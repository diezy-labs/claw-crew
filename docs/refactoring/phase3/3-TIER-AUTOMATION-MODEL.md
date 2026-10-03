---
title: 3-Tier Automation Model — galleon-fleet Phase 3+ Governance
---

# 3-Tier Automation Model

**Goal:** Automate routine development fully. Escalate critical decisions to Lead. Escalate strategic decisions to you (Achmad).

**Cost savings:** ~93% on routine work (0 min vs 30+ min per PR).

---

## Tier 1: Routine (Fully Automated)

**Who decides:** Principle crew (automated logic)  
**What:** Bug fixes, validation gaps, small features  
**Cost to you:** 0 minutes

### Examples
✅ Add null check validation  
✅ Fix typo in error message  
✅ Add missing workspace validation  
✅ Update error handling  
✅ Minor UI improvements

### Principle Logic (Auto-Approve)
```
IF severity = "normal" OR "minor":
  AND issue_type = "bug_fix" OR "improvement"
  AND scope = "single_file" OR "isolated_component"
  THEN: Auto-APPROVE (no human review)
  
ELSE: Post CHANGES-REQUESTED (hold for Lead review)
```

### Workflow
```
Principle reviews PR
     ↓
Issue severity = NORMAL
     ↓
Principle auto-approves (no human needed)
     ↓
QA crew runs integration tests (auto)
     ↓
QA approves (tests pass)
     ↓
Auto-merge to testing (no human button)
     ↓
❌ You don't see this
```

### Crew Commands (Normal)
```
@squad-backend fix: add null check validation line 45
@squad-frontend feat: add error boundary component
@squad-qa integration: test workspace override behavior
```

---

## Tier 2: Critical (Lead Filter + Your Approval)

**Who decides:** Lead crew (analysis) → You (approval)  
**What:** Security issues, high severity bugs, architecture questions  
**Cost to you:** 5-15 minutes (read Lead analysis + decide)

### Examples
🔴 Unauthenticated RCE endpoint  
🔴 Workspace escape vulnerability  
🔴 Timeout enforcement gap  
🔴 Multi-crew coordination needed  
🔴 Architecture design question

### Lead Logic (Analysis + Options)
```
IF keywords = CRITICAL OR HIGH OR architecture OR security OR breaking:
  → Lead crew spawned (auto)
  → Lead analyzes issue
  → Lead posts 3 options:
      Option A: ...
      Option B: ...
      Option C: ...
  → Lead adds: "Achmad approval needed"
  
ELSE: Route to Tier 1 (Principle auto-approves)
```

### Workflow
```
Principle reviews PR
     ↓
Issue found: "@squad-lead CRITICAL: RCE blocker"
     ↓
Workflow detects CRITICAL keyword
     ↓
Lead crew spawned (auto)
     ↓
Lead analyzes → posts decision options
     ↓
Lead: "Achmad, which approach?"
     ↓
✅ YOU decide → comment choice
     ↓
Crew executes fix (auto)
     ↓
Principle re-reviews (auto-approves)
     ↓
Rest flows to testing (auto)
```

### Principle Logic (Hold for Lead)
```
IF severity = CRITICAL OR HIGH:
  THEN: Post CHANGES-REQUESTED
  AND: "Lead crew analyzing, awaiting decision"
  
(Don't approve until Lead decides)
```

### Crew Commands (Critical)
```
@squad-lead CRITICAL: unauthenticated endpoint found
@squad-lead HIGH: workspace escape vector in execute_bash
@squad-backend fix: remove RCE endpoint + CRITICAL
@squad-lead review: is 60s timeout too aggressive for CLI?
```

---

## Tier 3: Strategic (You Only)

**Who decides:** You + Lead + PO (alignment)  
**What:** Timeline changes, scope decisions, architecture strategy  
**Cost to you:** 10-30 minutes (discussion + approval)

### Examples
📋 Move Track B to Phase 3B (timeline)  
📋 Change workspace isolation strategy  
📋 Approve Phase 3 as "known risk" (security acceptance)  
📋 Redirect budget/resources  
📋 Release approval (prerelease → main)

### Lead + PO Logic (Discussion)
```
IF topic = architecture_strategy OR timeline_change OR scope_expansion:
  → Lead + PO discuss
  → Lead posts: "This needs Achmad alignment"
  → Achmad joins discussion
  → Decides + approves
  
(No auto-decision possible, needs your judgment)
```

### Workflow (Strategic)
```
Example: "Phase 3 timeline slip?"

Lead + PO sync (offline or async)
     ↓
Lead posts: "Moving Track B to Phase 3B. 
 Achmad, approve timeline change?"
     ↓
✅ YOU decide → comment approval
     ↓
Documentation updates (PR labels, roadmap, milestones)
     ↓
Crews execute per new timeline (auto)
```

### Your Commands (Strategic)
```
@squad-lead "Approved: move Track B to Phase 3B, update roadmap"
@squad-lead "HOLD PR #10: need architecture review before merge"
"Approved for release: PR #xyz → main"
```

---

## Routing Decision Tree

```
Issue Found by Principle
     ↓
┌─────────────────────────┐
│ Severity + Keywords?    │
└─────────────────────────┘
     ↓
CRITICAL/HIGH/
architecture/
security/breaking?
     ↙          ↘
   YES          NO
     ↓          ↓
  TIER 2     TIER 1
  (Lead)    (Auto-approve)
     ↓          ↓
Lead        Principle
analyzes    auto-approves
     ↓          ↓
Posts        QA tests
options      (auto)
     ↓          ↓
"Achmad?"   Auto-merge
     ↓      to testing
YOU        (no human)
decide
     ↓
Crew executes
     ↓
Rest auto-flows
```

---

## Decision Criteria

### Tier 1 (Principle Auto-Approves)
- [ ] Severity: NORMAL or MINOR
- [ ] Scope: Single file or isolated component
- [ ] Type: Bug fix, validation improvement, or small feature
- [ ] Risk: Low (no security, no architecture change)
- [ ] **Action:** Approve automatically

### Tier 2 (Lead Filters + You Decide)
- [ ] Severity: CRITICAL or HIGH
- [ ] OR Keywords: architecture, security, breaking, RCE
- [ ] OR Multi-crew coordination needed
- [ ] OR Design question, not just bug fix
- [ ] **Action:** Lead analyzes → posts options → you decide

### Tier 3 (Strategic, You Only)
- [ ] Topic: Timeline, scope, budget, release gate
- [ ] OR Architecture strategy decision
- [ ] OR Policy change (acceptance, allowlist, etc.)
- [ ] **Action:** You decide (with Lead + PO input)

---

## GitHub Actions Workflow Mapping

### `principle-review-trigger.yml`
- **Trigger:** PR opened to testing
- **Action:** Spawn Principle crew (auto)

### `principle-command-parser.yml` (v3 with auto-approval)
```yaml
jobs:
  tier-route:
    steps:
      - Check: severity + keywords?
      
      - If TIER 1 (normal):
          → Principle auto-approves
          → Mark as "✅ Approved (auto)"
          
      - If TIER 2 (critical):
          → Post "⚠️ Critical, routing to Lead"
          → Trigger Lead webhook
          
      - If TIER 3 (strategic):
          → Post "📋 Strategic decision needed"
          → Mention Lead + PO + Achmad
```

### `lead-escalation.yml` (new)
- **Trigger:** Tier 2 critical keyword detected
- **Action:** Lead webhook → Lead crew analyzes
- **Output:** 3 options + "Achmad approval needed"

### `testing-to-main.yml` (new)
- **Trigger:** Testing branch QA passes
- **Action:** Hold at prerelease (manual gate)
- **Owner:** You (Achmad) — release approval only

---

## Communication Pattern

### Tier 1 (Routine)
```
Principle: ✅ "Approved (auto). Severity: normal. PR auto-merged."
You: [See it in Slack notification after merge]
```

### Tier 2 (Critical)
```
Principle: ⚠️ "CHANGES-REQUESTED. Critical issue escalated to Lead."
Lead: 📋 "Issue: RCE endpoint. Options: remove/gate/accept.
       Achmad, which approach?"
You: @squad-lead "Remove the endpoint (fastest)"
Crew: [Executes]
```

### Tier 3 (Strategic)
```
Lead: 📋 "Timeline decision: Move Track B to Phase 3B?
       Achmad + PO input needed."
PO: [Provides business context]
You: @squad-lead "Approved: Phase 3B timeline, update roadmap"
Crew: [Updates docs + milestones]
```

---

## Timeline Expectations

| Tier | Principle | Lead | You | Total |
|------|-----------|------|-----|-------|
| **1 (auto)** | 5 min (auto) | — | 0 min | ~5 min |
| **2 (critical)** | 5 min (hold) | 5 min (analysis) | 5 min (decide) | ~15 min |
| **3 (strategic)** | — | 5-10 min (sync) | 10-30 min (decide) | ~20-40 min |

---

## Cost per 100 PRs

Assuming: 70 Tier 1 + 20 Tier 2 + 10 Tier 3

| Scenario | Manual Review | 3-Tier Auto |
|----------|---|---|
| **Principle time** | 70 × 10 min = 700 min | 20 × 10 min = 200 min |
| **Lead time** | — | 20 × 5 min = 100 min |
| **Your time** | 100 × 30 min = 3000 min | (20 × 5 + 10 × 20) min = 300 min |
| **QA time** | 100 × 15 min = 1500 min | 90 × 15 min = 1350 min |
| **Total** | **5700 min** | **1950 min** |
| **Savings** | — | **~66% overall, ~90% for you** |

---

## Implementation Checklist

### Phase 1: Principle Auto-Approval
- [ ] Update Principle crew briefing with Tier 1 criteria
- [ ] Add auto-approve logic (severity + type check)
- [ ] Test with dummy PR (normal fix)
- [ ] Verify GitHub Actions auto-comment "✅ Approved (auto)"

### Phase 2: Lead Escalation (Already Live)
- [ ] Verify Lead webhook hook:github:lead-escalation:critical-issues
- [ ] Test with dummy PR (CRITICAL keyword)
- [ ] Verify Lead crew spawns automatically
- [ ] Verify Lead posts analysis + options

### Phase 3: Strategic Gate (New)
- [ ] Update Lead briefing on Tier 3 (strategic topics)
- [ ] Add your approval gate for prerelease → main
- [ ] Document release gate process
- [ ] Set up Slack notification for "Achmad approval needed"

### Phase 4: Automation Testing
- [ ] Dry-run: Normal PR → Tier 1 auto-approve → merge
- [ ] Dry-run: Critical PR → Tier 2 → Lead analysis → your decision
- [ ] Dry-run: Strategic change → Tier 3 → your approval

---

## Rollback / Adjustments

**If Principle auto-approves wrong thing:**
- GitHub: Revert PR from testing
- Update: Refine Tier 1 criteria (too loose)
- Example: Add "AND NOT security-related" check

**If Lead escalation is noisy:**
- Adjust: Critical keywords list (too broad)
- Example: Remove "HIGH", keep only "CRITICAL"

**If you need more granularity:**
- Add: Custom severity levels (CRITICAL > HIGH > MEDIUM)
- Add: Crew-specific rules (Backend RCE = always escalate, Frontend UI = auto-approve)

---

## FAQ

**Q: What if Principle auto-approves but code is bad?**  
A: QA testing catches it. QA can request changes → Lead reviews → you decide. Rare case (Principle + QA both miss = escalate both).

**Q: Can I override auto-approval?**  
A: Yes. Post comment on PR: "HOLD: needs review" → Lead re-reviews.

**Q: What if I disagree with Lead's options?**  
A: Comment: "@squad-lead different approach: ..." → Lead analyzes new approach.

**Q: Does this work for non-Phase-3 PRs?**  
A: Yes, same logic applies to all testing PRs (Phase 3B, 4, etc.).

---

## Success Metrics

✅ **Routine PRs:** 0 human review (fully automated)  
✅ **Critical PRs:** <15 min for your approval  
✅ **Strategic decisions:** Explicit escalation (not hidden in routine)  
✅ **Quality:** QA catches any auto-approval mistakes  
✅ **Transparency:** You see critical + strategic, not routine noise

---

## Next Steps

1. **Update Principle briefing** — Add Tier 1 auto-approval rules
2. **Test Tier 1** — Create dummy normal fix PR, verify auto-approval
3. **Test Tier 2** — Create dummy CRITICAL PR, verify Lead escalation
4. **Add Tier 3 gate** — Set up prerelease → main manual approval
5. **Go live** — Start using for Phase 3B PRs


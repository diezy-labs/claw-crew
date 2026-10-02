---
title: Principle Crew — Tier 1 Auto-Approval Rules (Fully Automated)
---

# Principle Crew: Tier 1 Auto-Approval Rules

**Goal:** Automate routine PR approval so Principle crew posts "✅ Approved (auto)" without waiting for human review. Enables 100% automated Tier 1 → testing merge path.

---

## Auto-Approval Criteria (Tier 1)

**Approve automatically (post "✅ Approved (auto)" comment) when ALL conditions are met:**

```
IF
  severity = NORMAL OR MINOR
  AND
  scope = single_file OR isolated_component (not multi-file cross-cutting change)
  AND
  type = bug_fix OR improvement OR small_feature (minor <10 LOC additions)
  AND
  risk = LOW (no security implications, no architecture change)
THEN
  → Post: "✅ Approved (auto). Severity: normal. Scope: isolated. Type: fix."
  → Do NOT post CHANGES-REQUESTED
  → Do NOT wait for human review
  → Let GitHub Actions workflow auto-merge to testing
END
```

---

## Examples: Auto-Approve (Tier 1)

✅ **Auto-approve these PRs:**

| PR | Reason | Auto-Approve? |
|----|----|---|
| Add null check in workspace validation (single file) | Severity: normal, scope: isolated, type: fix | ✅ YES |
| Fix typo in error message (1 file, 1 line) | Severity: minor, scope: isolated, type: improvement | ✅ YES |
| Add missing validation in execute_bash (contained fix) | Severity: normal, scope: single function, type: fix | ✅ YES |
| Improve error handling for timeout (single file) | Severity: normal, scope: isolated, type: improvement | ✅ YES |

---

## Examples: Do NOT Auto-Approve (Tier 2+)

❌ **Do NOT auto-approve these — escalate to CHANGES-REQUESTED + Lead:**

| PR | Reason | What To Do |
|----|----|---|
| Refactor workspace isolation architecture (5 files, cross-cutting) | Scope: multi-file, risk: architecture change | Post CHANGES-REQUESTED, escalate to Lead |
| Fix unauthenticated RCE endpoint | Risk: HIGH (security), severity: CRITICAL | Post CHANGES-REQUESTED, escalate to Lead |
| Add new validation framework (adds complexity) | Type: new feature, scope: multi-component | Post CHANGES-REQUESTED, escalate to Lead |
| Change default timeout from 60s to 30s (impacts all paths) | Scope: system-wide, risk: behavior change | Post CHANGES-REQUESTED, escalate to Lead |

---

## When to Auto-Approve

### **Review checklist (Principle):**

1. **Read PR title + description**
   - Does it say "fix: ...", "improvement: ...", or "refactor: ..."?
   - Is it focused on ONE concern?

2. **Check files changed**
   - Is it 1-3 files? (isolated = auto-approve)
   - Or 5+ files? (cross-cutting = escalate)

3. **Check severity**
   - Does PR description mention CRITICAL/HIGH/architecture/security keywords?
   - If yes → escalate, don't auto-approve

4. **Check diff lines**
   - Is it <50 LOC additions? (small = auto-approve)
   - Or >200 LOC? (large = escalate)

5. **Decide:**
   - All checks pass? → Post "✅ Approved (auto)"
   - Any check fails? → Post "CHANGES-REQUESTED" (escalate to Lead)

---

## Comment Format (Auto-Approve)

**Exact format for Tier 1 auto-approval comment:**

```
✅ **Approved (auto)**

Severity: normal
Scope: isolated (1 file, 1 function)
Type: bug_fix
Risk: low
Gates: ✅ 7/7 pass

Next: QA testing → auto-merge to testing

No human review needed for routine fixes.
```

**Exact format for Tier 2+ escalation (CHANGES-REQUESTED):**

```
CHANGES-REQUESTED

⚠️ This PR needs Lead crew review:
- Severity: CRITICAL
- Scope: multi-file, cross-cutting
- Risk: architecture change
- Reason: <specific gate that failed>

🔄 Routing to Lead for analysis...
Status: awaiting Lead decision
```

---

## Automation Workflow (After Auto-Approval)

```
Principle posts: "✅ Approved (auto)"
     ↓
GitHub Actions workflow detects comment with "✅ Approved (auto)"
     ↓
tier1-auto-approval-notification.yml triggers
     ↓
Discord notification posted: ✅ "Auto-approved & merged"
     ↓
GitHub Actions auto-merge rule (if configured):
  - Merge PR to testing branch
  - All required checks pass (if CI configured)
  ↓
QA crew triggered (or manual, depending on setup)
     ↓
🎉 Tier 1 fully automated end-to-end
```

---

## Decision Tree (Quick Reference)

```
PR received by Principle
     ↓
┌──────────────────────────────────┐
│ Severity + Keywords Check        │
└──────────────────────────────────┘
     ↓
CRITICAL/HIGH/
architecture/
security/breaking?
     ├─ YES → Post CHANGES-REQUESTED (escalate to Lead)
     └─ NO → Continue
          ↓
┌──────────────────────────────────┐
│ Scope Check                      │
└──────────────────────────────────┘
     ↓
Multi-file cross-cutting
or 5+ files changed?
     ├─ YES → Post CHANGES-REQUESTED (escalate to Lead)
     └─ NO → Continue
          ↓
┌──────────────────────────────────┐
│ Risk Assessment                  │
└──────────────────────────────────┘
     ↓
Low risk (bug fix, improvement)?
     ├─ YES → Post "✅ Approved (auto)"
     └─ NO → Post CHANGES-REQUESTED (escalate)
```

---

## FAQ

**Q: What if auto-approve was wrong?**  
A: QA testing catches it. If tests fail, post comment: "Hold, revert auto-approval" → new command to fix. This is rare; PR never merged if QA fails.

**Q: Can I override auto-approval?**  
A: Yes. If human review needed, post new comment: "CHANGES-REQUESTED — needs review" → hold merge.

**Q: What if I'm unsure?**  
A: Post CHANGES-REQUESTED + escalate to Lead. Better to over-escalate than miss a real issue. Lead will re-route if false alarm.

**Q: Does auto-approval affect security gates?**  
A: No. All 7 security gates still run (path validation, workspace containment, timeout enforcement, etc.). Auto-approval only skips human review for routine, low-risk fixes.

---

## Testing Auto-Approval

### **Test Scenario 1: Normal Bug Fix**
1. Create test PR to testing branch: `fix: add null check in workspace validation`
2. Single file change (web-2/src/gateway.ts)
3. 5 LOC addition
4. Principle receives for review
5. **Expected:** Principle posts "✅ Approved (auto)"
6. **Verify:** GitHub Actions auto-approval workflow triggers
7. **Result:** Discord notification: ✅ "Auto-approved & merged"

### **Test Scenario 2: Critical Issue (Should NOT auto-approve)**
1. Create test PR to testing branch: `fix: RCE endpoint + CRITICAL`
2. Principle receives for review
3. **Expected:** Principle posts "CHANGES-REQUESTED" (not auto-approve)
4. **Verify:** Principle escalates to Lead (not auto-merged)

---

## Rollback / Adjustments

**If auto-approval approves wrong PR:**
1. Comment: "HOLD — revert auto-approval"
2. Adjust decision tree (tighten scope/risk check)
3. Test with dummy PR again

**If auto-approval is too strict (approves too little):**
1. Loosen criteria (e.g., allow up to 100 LOC instead of 50)
2. Add more keywords to "safe" list (if applicable)

---

## Implementation (For Principle Crew)

**When Principle crew reviews a PR:**

1. **Read PR** — title, description, files changed, LOC diff
2. **Apply decision tree** above
3. **If Tier 1:** Post "✅ Approved (auto)"
4. **If Tier 2+:** Post "CHANGES-REQUESTED" + escalate reason
5. **Never post APPROVE** for routine fixes — only "✅ Approved (auto)" or "CHANGES-REQUESTED"

---

## Success Metrics

✅ **Tier 1 PRs:** 0 human review needed (fully automated)  
✅ **Approval latency:** <2 min (auto-posted)  
✅ **False positives:** <5% (QA catches if wrong)  
✅ **Cost:** Zero (Principle crew handles it)  
✅ **Time saved:** ~25 min per routine PR (you don't see CHANGES-REQUESTED, PR auto-merges)

---

## Next Steps (Phase 3+)

1. **Principle applies auto-approval rules** to all new Phase 3B+ PRs
2. **QA crew handles integration testing** (auto-triggered after Tier 1 approval)
3. **Lead still handles critical issues** (escalation is explicit, not hidden)
4. **You focus on strategic decisions** only (Tier 3: release gates, timeline changes)

**Result:** Fully automated development pipeline with explicit escalation for critical issues.


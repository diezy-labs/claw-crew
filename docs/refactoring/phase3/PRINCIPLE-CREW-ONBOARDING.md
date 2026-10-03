---
title: Principle Crew Onboarding — Role, Scope, Review Checklist
author: squad-assistant (on behalf of Achmad)
date: 2026-10-03
status: active
---

# Principle Crew Onboarding

## Role Definition

**Principle** = Code quality gatekeeper & PR reviewer. Acts as a **severity triage hub** between daily development and Lead Squad strategic oversight.

### Authority & Boundaries

| Authority | Details |
|-----------|---------|
| **✅ CAN** | Review code quality on all PRs (FE/BE/Go/Rust/TS) |
| **✅ CAN** | Request changes for style, clarity, or standards compliance |
| **✅ CAN** | Merge PRs to `testing` branch after quality gate passes |
| **✅ CAN** | Triage bug severity (minor/medium/critical/high) |
| **✅ CAN** | Route critical/high bugs to Lead for strategy; coordinate FE/BE on fix approach |
| **❌ CANNOT** | Push directly to `main` or `prerelease` (Lead only) |
| **❌ CANNOT** | Make architecture decisions (Lead Squad owns architecture) |
| **❌ CANNOT** | Approve Phase merges (Lead verifies DoD + testing results) |

---

## Workflow Integration

```
FE/BE Code Ready
    ↓
    └─→ Push feature branch → PR to testing
           ↓
           Principle Review Gate
           ├─→ Quality checks pass? ✅ → Merge to testing → QA Testing
           └─→ Issues found? ❌ → Request changes → FE/BE revise → Re-review
                  ↓ (if critical/high bugs discovered by QA)
           QA Reports Bug + Severity
           ├─→ Minor? → Principle routes back to FE/BE in same branch
           ├─→ Medium? → Principle coordinates, FE/BE fix → Re-review Principle → Test again
           └─→ Critical/High? → Principle escalates to Lead for strategy → Lead directs fix plan
                  ↓
           If All Features Ready for Phase
           ├─→ Principle creates PR: testing → prerelease
           ├─→ Lead reviews DoD + QA sign-off → Approves/requests changes
           └─→ If approved → Lead merges to prerelease/main
```

---

## Review Checklist — Code Quality Gates

Apply this checklist to every PR before approving:

### ✅ **Structural Integrity**

- [ ] **Single concern per file:** File has one clear responsibility, not mixed layers (e.g., no business logic in UI component, no UI in service)
- [ ] **Imports organized:** Standard lib → third-party → internal (alphabetical per group)
- [ ] **Dead code removed:** No `#[allow(dead_code)]`, no commented-out logic; remove or connect
- [ ] **Unused variables:** None (neither `_name` nor bare `_` without documented reason)
- [ ] **Constants extracted:** Magic numbers/strings in business logic are named constants, not literals

### ✅ **Error Handling & Safety**

- [ ] **Production code:** No `unwrap()` or `expect()` unless a documented invariant makes panic impossible
- [ ] **Trust boundaries:** Input from external sources (HTTP, CLI, files) validated & type-safe
- [ ] **Error propagation:** Errors bubble up or are logged at decision points; no silent failures
- [ ] **Panics documented:** If panic is intentional, comment explains why and when it cannot occur

**Language-specific:**
- **Rust:** `Result<T, E>` preferred over panics; error context included
- **Go:** Explicit `if err != nil` checks; errors are values, not exceptions
- **TypeScript:** No uncaught Promise rejections; error boundaries present in React components

### ✅ **Testing & Verification**

- [ ] **Test coverage:** New logic has at least one test (or doc explains why untestable)
- [ ] **Edge cases:** Tests include boundary conditions (empty, null, max size, timeout)
- [ ] **Integration paths:** If code integrates with another module, a test verifies the integration
- [ ] **CI passes:** All status checks green (build, lint, test)

### ✅ **Performance & Resource Management**

- [ ] **No gratuitous complexity:** Algorithm matches problem scale (O(n) for linear, not O(n²) scan)
- [ ] **Resource cleanup:** Files/sockets/connections closed; no leaks (Rust Drop, Go defer, TS cleanup)
- [ ] **Timeouts present:** Long-running operations have timeouts (network, process execution, cache TTL)
- [ ] **Concurrency safe:** No race conditions; shared state protected (Rust Send/Sync, Go sync.Mutex, TS promises serialized)

### ✅ **Security**

- [ ] **No secrets in code:** No API keys, tokens, or credentials in source; use env vars or vault
- [ ] **Path traversal prevented:** Filesystem paths canonicalized & contained; no `../../../` escapes
- [ ] **Command injection prevented:** Shell commands use parameterized APIs, not string interpolation
- [ ] **SQL/database injection prevented:** Queries use prepared statements or ORMs, not string concat
- [ ] **Auth boundaries:** Public/private APIs clearly marked; auth checks at trust boundaries

### ✅ **Documentation**

- [ ] **Public interfaces documented:** Functions/types/exports have brief doc comments (what, not how)
- [ ] **Complex logic explained:** Non-obvious logic has inline comments explaining intent
- [ ] **Comments accurate:** Comments match code behavior (stale comments worse than none)
- [ ] **Changelog updated:** Feature has entry in CHANGELOG or PR description if user-visible

### ✅ **Style & Consistency**

- [ ] **Language conventions followed:** Rust idioms, Go style, TypeScript typing, SQL naming
- [ ] **Formatter passed:** `cargo fmt`, `gofmt`, `prettier` all green
- [ ] **Linter passed:** `cargo clippy`, `golangci-lint`, `eslint` all green
- [ ] **Naming clear:** Variables/functions use domain terms, not `x`, `temp`, `data`

---

## Severity Triage Matrix

When bugs are found (by QA or during review):

| Severity | Definition | Action | Coordinator |
|----------|-----------|--------|-------------|
| **Critical** | System unusable, data loss, security breach | Escalate to Lead immediately | Principle + Lead plan fix |
| **High** | Major feature broken, significant perf loss | Coordinate FE/BE fix, Lead reviews plan | Principle + FE/BE, Lead oversight |
| **Medium** | Feature partially broken, workaround exists | FE/BE fix in same PR cycle | Principle + FE/BE |
| **Minor** | Typo, style, non-urgent perf | FE/BE fix in next cycle or defer | Principle routes back to FE/BE |

---

## Daily Cadence

### **Morning (If PRs Waiting)**
1. Check GitHub for new PRs to `testing` branch
2. Review code against checklist (15–30 min per PR)
3. Post review: either ✅ approve + merge, or ❌ request changes with specific feedback

### **On QA Reports**
1. Receive severity + description from QA crew
2. Triage: minor/medium → coordinate FE/BE fix; critical/high → escalate to Lead
3. Update PR status (comment thread on GitHub)
4. Re-review after FE/BE push

### **Weekly (Phase Boundaries)**
1. When all features for a phase are in `testing` + QA signed off
2. Create PR: `testing` → `prerelease`
3. Post summary: list of features, test results, known issues (if any)
4. Wait for Lead review + approval

---

## Communication Templates

### **Approving a PR**
```
✅ Code review passed. Quality checklist: all items green.
- [x] Structural integrity
- [x] Error handling
- [x] Testing
- [x] Security
- [x] Documentation
- [x] Style

Merging to testing now. QA, please test.
```

### **Requesting Changes**
```
❌ Review: [N] issues found, please address:

1. **Security:** Path not canonicalized in line 42. Use validate_path_with_workspace().
2. **Testing:** Edge case (empty input) not tested. Add test case.
3. **Style:** Variable `x` should be `cache_key` for clarity.

Please update and push again. I'll re-review.
```

### **Escalating to Lead**
```
🔴 **CRITICAL BUG FOUND** (by QA)

**Issue:** Workspace routing fails when path contains spaces (Yoga 6 user)
**Impact:** All file operations fail on Windows user directories
**Root cause:** (investigation needed)
**Severity:** Critical

**Action:** Lead Squad, please advise fix strategy. I'll coordinate FE/BE on implementation.
```

---

## Hand-off to Lead

When Principle has completed all code reviews for a phase and QA has tested:

**Principle → Lead Summary**
- Feature list (with PR numbers)
- Test results (pass/fail, coverage %)
- Known issues (severity, status)
- Blockers (if any)

**Lead verifies:**
- DoD checklist complete
- Testing sign-off from QA
- Architecture alignment
- Decision: approve merge to `prerelease` or request changes

---

## Principle Authority Limits

⚠️ **Principle DOES NOT:**
- Decide architecture direction (Lead Squad owns)
- Merge to `prerelease` or `main` (Lead only)
- Commit code (review-only role)
- Override Lead's architectural decisions

✅ **Principle DOES:**
- Set daily code quality bar
- Triage severity for bugs
- Route work efficiently (minor → FE/BE loop-back, critical → Lead escalation)
- Reduce Lead's daily code review load

---

## Success Metrics

After Principle is live for Phase 3:

| Metric | Target | How to Measure |
|--------|--------|---|
| Lead code-review time | < 10% of Phase | No Lead reviews of quality issues (Principle caught them) |
| PR feedback loop | 1–2 cycles | PRs merged on first/second review (fewer back-and-forths) |
| Critical bugs in testing | 0 | QA finds only expected edge cases, not security/structure issues |
| Phase merge confidence | High | Principle PR to prerelease has "all checks passed" with clear summary |

---

## Next Steps (for Achmad)

1. **Review this checklist** — adjust any items for your team's standards
2. **Brief Principle crew** — share this doc + run through 1–2 example PRs together
3. **Use workflow starting Phase 3 Track A+B merge gate** — Principle reviews first PR to `testing`
4. **Iterate** — after 1 week, adjust checklist based on what you learned

---

**Questions for Principle crew?**
- Review checklist too strict? (Tell Achmad; he'll adjust)
- Unclear authority boundaries? (Ask Achmad; he decides)
- Need escalation guidance for a specific bug? (Ask Principle crew lead or Achmad)


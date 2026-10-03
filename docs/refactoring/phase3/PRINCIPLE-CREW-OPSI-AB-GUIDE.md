---
title: Principle Crew — Opsi A+B Hybrid Command Coordination Guide
---

# Principle Crew: Hybrid Opsi A+B Command Coordination

## Quick Reference

| Situation | Command Syntax | Routing | Latency |
|-----------|---|---|---|
| **Normal bug fix** | `@squad-backend fix: add null check line 95` | Direct → Backend crew | ~1 min |
| **Minor feature** | `@squad-frontend feat: add error boundary` | Direct → Frontend crew | ~1 min |
| **Critical bug** | `@squad-lead CRITICAL: RCE in execute_bash` | Escalate → Lead analysis | ~2 min |
| **High severity** | `@squad-backend fix: workspace escape + HIGH` | Escalate → Lead analysis | ~2 min |
| **Architecture issue** | `@squad-lead review: namespace isolation broken` | Escalate → Lead review | ~2 min |

---

## How It Works

### **Normal Fixes (Opsi A: Direct)**

**When:** Routine bugs, small features, straightforward fixes  
**Command:** `@squad-backend fix: <description>` or `@squad-frontend feat: <description>`

```
You post comment:
  "@squad-backend fix: add workspace root validation in line 45"
         ↓
.github/workflows/principle-command-parser.yml triggers
         ↓
Workflow validates: crew=Backend ✅, task length >10 ✅
         ↓
Workflow posts ack: "✅ Spawning backend crew..."
         ↓
Workflow POSTs to Kiro webhook (direct crew spawn)
         ↓
Squad-assistant receives webhook
         ↓
Backend crew spawned immediately
         ↓
Backend: fix code + commit + push to PR branch
         ↓
PR auto-updates
         ↓
You re-review (should be quick if fix is right)
         ↓
If OK: APPROVE → QA
         ↓
If not OK: new @squad-backend command
```

**Latency:** ~1 minute (direct spawn, no middleman)  
**Cost:** 1 model turn (crew only)  
**Your role:** Issue command → re-review → approve

---

### **Critical/High Escalation (Opsi B: Lead-Gated)**

**When:** CRITICAL severity, HIGH severity, architecture issues, security-breaking changes, design questions  
**Command:** `@squad-lead CRITICAL: <description>` OR `@squad-backend fix: <task> CRITICAL`

```
You post comment:
  "@squad-lead CRITICAL: unauthenticated endpoint bypasses all gates"
         ↓
.github/workflows/principle-command-parser.yml triggers
         ↓
Workflow detects keyword: CRITICAL ✅
         ↓
Workflow skips crew spawn (prevents wasted fix attempt!)
         ↓
Workflow posts ack: "⚠️ Critical issue detected, routing to Lead..."
         ↓
Workflow POSTs to Kiro webhook (Lead escalation, NOT crew)
         ↓
Squad-assistant receives webhook
         ↓
Lead crew spawned for analysis
         ↓
Lead: analyzes issue + decides:
  - Option A: "Endpoint is legacy, remove it"
  - Option B: "Gate with auth, here's the fix"
  - Option C: "This needs architecture review, hold PR"
         ↓
Lead coordinates with you + crews
         ↓
Implementation happens (Opsi A direct command from Lead to crew)
         ↓
Lead confirms fix → you re-review
         ↓
If OK: APPROVE → QA
```

**Latency:** ~2 minutes (Lead analysis first, then crew)  
**Cost:** 2 model turns (Lead analysis + crew execution, no wasted attempts)  
**Your role:** Escalate → Lead decides → you approve result

---

## Critical Keywords

Workflow detects these keywords and **automatically routes to Lead** (not crew):

- `CRITICAL`
- `HIGH` (severity)
- `architecture`
- `security`
- `breaking`
- `RCE`

**Use them when:**
- Issue affects core system (not just UI)
- Multiple crews need coordination
- Decision needed before fix attempts
- Design question, not just bug fix

**Example escalations:**
```
❌ @squad-backend fix: typo in error message
✅ @squad-backend fix: remove unauthenticated RCE endpoint CRITICAL

❌ @squad-frontend fix: button color
✅ @squad-frontend feat: redesign auth flow — needs architecture review

❌ @squad-backend fix: add null check
✅ @squad-backend fix: workspace isolation gap found in execute_bash HIGH
```

---

## Workflow Behavior

### **Valid Command**
```
Condition: @squad-crew + min 10 chars task
Result: ✅ Ack comment posted → crew spawned
```

### **Invalid Command**
```
Condition: No crew mentioned OR task too short
Result: ⚠️ Error comment posted with correct syntax
```

### **Critical Detected**
```
Condition: CRITICAL|HIGH|architecture|security|breaking keyword found
Result: Skips crew → routes to Lead instead
```

---

## Your Decision Matrix

| Found Issue | Severity | Action | Command |
|---|---|---|---|
| Typo in comment | Minor | Post new comment | `@squad-frontend fix: update error text` |
| Missing null check | Normal | Direct command | `@squad-backend fix: add null check line 45` |
| Path traversal bypass | High | Escalate | `@squad-lead HIGH: workspace escape in execute_bash` |
| Needs architecture decision | Critical | Escalate | `@squad-lead review: timestamp isolation strategy` |
| Multi-crew coordination needed | Critical | Escalate | `@squad-lead CRITICAL: needs BE + FE sync` |

---

## Response Time Expectations

| Path | Spawn Time | Work Time | Re-review Time | Total |
|---|---|---|---|---|
| **Opsi A (direct)** | ~30s | 10–20 min | 5 min | **15–25 min** |
| **Opsi B (Lead)** | ~30s (Lead) + ~30s (crew) | 5 min (analysis) + 10–20 min (crew) | 5 min | **20–35 min** |

---

## Cost Efficiency (Hybrid vs Pure)

Per 100 PRs (80 normal + 20 critical):

| Model | Total Cost | Latency Avg |
|---|---|---|
| **Opsi A+B Hybrid** | ~$0.12 | ~15 min avg |
| **Pure Opsi A** | ~$0.12 | ~15 min avg (but 20 wasted attempts on critical) |
| **Pure Opsi B** | ~$0.20 | ~25 min avg (every fix requires Lead relay) |

**Hybrid is best:** Same cost as pure A, same speed for 80% of fixes, safety for critical 20%.

---

## Troubleshooting

### **Command not recognized**
```
✗ "@backend fix: x"  (should be @squad-backend)
✗ "@squad-backend fix: x" with NO 10+ char task (too short)
✗ "@squaa-backend fix: typo in username" (crew name misspelled)
```
→ Check ack comment for error + correct syntax

### **Crew didn't spawn**
```
→ Check GitHub Actions tab (.github/workflows/principle-command-parser.yml)
→ Verify GH_TOKEN is set (should be)
→ Check webhook logs in Kiro dashboard (if available)
```

### **Crew spawned but fix looks wrong**
```
→ Post new @squad-crew command with correction
→ No need to wait for original fix to complete
→ Newer command takes priority
```

---

## Examples: Real Scenarios

### **Scenario 1: Missing Validation (Normal Fix)**
```
You: Review shows workspace_root not validated in line 88

You post:
@squad-backend fix: add workspace_root null check in execute_bash line 88, return error if null

✅ Workflow posts ack
✅ Backend crew spawned in ~30s
✅ Backend: adds check → commits → pushes
✅ PR auto-updates
You re-review → looks good
You: APPROVE
```

### **Scenario 2: Unauthenticated Endpoint (Critical)**
```
You: Review finds /api/engine/execute is unauthenticated RCE vector

You post:
@squad-lead CRITICAL: /api/engine/execute endpoint is unauthenticated, anyone on LAN can execute arbitrary bash

⚠️ Workflow posts ack (escalating to Lead)
⚠️ Workflow skips Backend spawn (prevents wasted attempt!)
✅ Lead crew spawned in ~1 min
✅ Lead: analyzes → posts decision:
   "Endpoint is legacy, Track B replaces it. Recommend: remove it now."

You follow up:
@squad-backend fix: remove /api/engine/execute endpoint from web-2/server.ts (legacy, Track B replaces)

✅ Backend crew spawned
✅ Backend: removes lines 165-195 → commits → pushes
✅ PR auto-updates
You re-review → clean removal
You: APPROVE
```

### **Scenario 3: Architecture Question (Escalation)**
```
You: Reviewing timeout logic, unsure about default 60s vs 30s

You post:
@squad-lead review: is 60s default timeout appropriate for all tool paths? Should CLI commands get longer timeout?

⚠️ Lead crew spawned (Lead handles architecture)
✅ Lead: analyzes usage patterns → posts recommendation:
   "60s good for most tools. CLI commands should timeout at 300s max. Adjust via config."

You coordinate next steps with Lead + crews
```

---

## For Lead Squad (When You Escalate)

When Principle routes a critical issue to you:

1. **Analyze** the issue (context already provided)
2. **Decide**: Is this a fix, a hold, or architecture decision?
3. **Post** your recommendation:
   - `@squad-backend fix: <task>` if crew should fix
   - `"Hold PR pending decision"` if waiting on architecture
   - `"Approved as-is, proceed to QA"` if no change needed
4. **Coordinate** with Principle + crews as needed

Lead's role: Safety valve + architect on critical issues.

---

## Summary

✅ **Hybrid Opsi A+B:**
- **Normal fixes:** Fast (Opsi A), cheap, autonomous
- **Critical issues:** Safe (Opsi B), Lead-gated, no wasted attempts
- **Cost:** Same as pure Opsi A (~$0.12/100 PRs)
- **Pattern:** Routine → crew direct, critical → Lead first
- **Your job:** Issue commands (normal) or escalate (critical) → review result → approve

**You control the flow. Commands trigger automation, decisions stay with you.** 🎯

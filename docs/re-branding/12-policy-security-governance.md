# Policy, Security, and Governance

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Fleet policy categories

```text
Data classification policy
Tool capability ceiling
Provider/model allowlist
Budget policy
Approval policy
Cross-Ship sharing policy
Artifact retention policy
MCP integration policy
Audit retention policy
Emergency freeze policy
```

## Data classification

```text
public
internal
confidential
restricted
```

### Cross-Ship default policy

| Classification | Default handoff behavior |
|---|---|
| Public | Allow if destination Ship allows it |
| Internal | Require source/destination policy match |
| Confidential | Require explicit handoff proposal and approval |
| Restricted | Deny by default; only dedicated approved route |

## Budget hierarchy

```text
Fleet total budget
  ↓
Ship allocation
  ↓
Squad allocation
  ↓
Voyage budget
  ↓
Job Order budget
  ↓
Tool/model request limits
```

Budget can only narrow downward. A Captain cannot increase Ship allocation. A Quartermaster can recommend reallocation but needs Pirate King approval for increase/rebalance beyond configured limits.

## Emergency freeze

Pirate King can initiate:

```text
Freeze Fleet
Freeze Ship
Freeze tool risk class
Freeze provider route
Freeze MCP server
Freeze external actions
```

Quartermaster can propose or, if explicitly authorized by emergency policy, temporarily freeze a narrowly scoped dangerous route such as a degraded provider or policy-violating MCP server. It cannot unfreeze that route without policy/user authority.

## Audit rules

Every cross-Ship action requires audit fields:

```text
source_fleet_id
source_ship_id
destination_ship_id
artifact_id or summary_id
classification
policy_version
handoff_mode
approval_id if required
actor_id
quartermaster involvement flag
created_at
```

---

## Existing Engine Policy Infrastructure

> The `engine/src/tool/` module already implements significant policy infrastructure
> that forms the foundation for Fleet governance.

### Already implemented in `src/tool/`

| Fleet Concept | Existing Implementation | Status |
|---|---|---|
| Tool capability ceiling | `tool.PolicyEngine.Evaluate()` | ✅ Exists |
| Risk classification | `tool.RiskTier` (READ/WRITE/EXECUTE), `tool.RiskClass` (9 levels) | ✅ Exists |
| Policy verdict | `tool.PolicyVerdict` (ALLOW/REQUIRE_APPROVAL/DENY) | ✅ Exists |
| Approval workflow | `tool.ApprovalGate` with hash-binding, expiry, resolve | ✅ Exists |
| Data classification | `tool.ExecutionContext.DataClassification` | ✅ Exists |
| Capability scoping | `tool.ExecutionContext.Capabilities` | ✅ Exists |
| Workspace scoping | `tool.ExecutionContext.WorkspaceID` + `AllowedRoots` | ✅ Exists |
| Execution audit | `tool.ToolExecution` with full trace | ✅ Exists |

### What Fleet adds on top

| Fleet Addition | Description |
|---|---|
| Fleet policy ceiling | New hierarchy layer above Ship |
| Ship policy intersection | Enforce Ship ∩ Fleet ∩ Squad ∩ Member ∩ Skill |
| Cross-Ship sharing policy | Classification-based handoff rules |
| Budget hierarchy | Fleet → Ship → Squad → Voyage → JobOrder → Tool |
| Emergency freeze | Route-level, Ship-level, Fleet-level freeze |
| Provider/model allowlist | Fleet-level ceiling for model access |
| MCP integration policy | Per-Ship MCP server enablement |


# Quartermaster Responsibilities

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Core responsibilities

| Responsibility | Description |
|---|---|
| Fleet intake | Interpret Pirate King objectives and identify affected Ships |
| Objective routing | Propose which Ship(s) should execute work |
| Portfolio visibility | Summarize Ship health, voyages, blockers, costs, risk, and approval state |
| Executive reporting | Turn Ship Reports into concise Fleet Reports |
| Escalation | Surface decisions, risks, policy issues, and budget exceptions |
| Cross-Ship coordination | Propose artifact handoff and dependency sequencing |
| Resource monitoring | Watch budget, concurrency, provider health, queue depth, and rate limits |
| Priority support | Recommend order based on impact, urgency, dependency, and configured priority rules |
| Lesson curation | Propose promotion of reusable lessons from Ship memory to Fleet Knowledge |
| Fleet hygiene | Identify stale voyages, abandoned approvals, inactive Ships, and degraded ports |
| **Squad composition research** | Research industry-standard team composition and propose optimal crew with roles, skills, and model assignments when Pirate King requests a new Squad |
| **Model AI recommendation** | Select the optimal AI model per Crew Member based on role complexity, skill requirements, cost optimization, Ship/Fleet model allowlist, and provider health |

## Quartermaster output types

```text
Fleet Report
Decision Brief
Risk Register
Fleet Order Proposal
Cross-Ship Voyage Proposal
Artifact Handoff Proposal
Squad Composition Proposal
Budget Alert
Provider/Port Health Alert
Policy Escalation
Fleet Lesson Proposal
```

## Decision brief format

```markdown
# Decision Brief

## Decision required
Approve a read-only GitHub MCP pilot for the Development Ship.

## Why now
The Developer Squad needs repository issue/PR context for codebase audit workflows.

## Options
1. Approve read-only GitHub MCP access for Development Ship only.
2. Keep current repository-local workflow without GitHub access.
3. Defer integration until Tool Runtime T3.

## Trade-offs
- Option 1: Better context, new external integration surface.
- Option 2: Lower risk, less complete repository intelligence.
- Option 3: Avoids immediate work but delays audit capability.

## Recommended option
Option 1 with read-only tool allowlist, no write operations, 30-day pilot, audit logging.

## Budget/risk
- Cost: low
- Security risk: medium, mitigated by read-only allowlist
- Approval required: Pirate King
```

## Escalation policy

| Level | Meaning | Quartermaster action |
|---|---|---|
| `info` | Non-actionable status update | Include in digest only |
| `attention` | Needs monitoring/review | Flag in Fleet Report |
| `decision_required` | Strategic/approval decision needed | Create Decision Brief |
| `high_risk` | Security, budget, sensitive data, irreversible action | Immediate Pirate King alert; freeze action if policy says so |
| `critical` | Active violation/incident | Freeze affected route/voyage where authorized, alert Pirate King immediately |

## Quartermaster permission profile

```yaml
role: quartermaster
mission: Coordinate fleet work, summarize status, surface decisions, and preserve governance.

allow:
  - fleet.read_summary
  - fleet.read_budget
  - fleet.read_policy_summary
  - ship.read_status
  - ship.request_status_report
  - voyage.create_proposal
  - artifact.read_summary
  - artifact.create_report
  - escalation.create
  - lesson.propose_promotion
  - artifact.transfer_proposal
  - squad.compose_proposal
  - crew_member.recommend_model

deny:
  - fleet.policy.update
  - fleet.budget.update
  - provider.configure
  - credential.read
  - workspace.apply_patch
  - git.commit
  - git.push
  - deployment.deploy
  - cms.publish
  - email.send
  - approval.self_approve
```

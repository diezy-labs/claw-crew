# Product Vocabulary

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Canonical mapping

| Pirate term | Product/technical meaning | Example |
|---|---|---|
| Pirate King | Human account owner / executive authority | The user who owns the Fleet |
| Quartermaster | Fleet-level AI coordinator | Synthesizes reports and routes objectives |
| Fleet | Collection of related Ships under one owner | Personal Product Fleet |
| Ship | Isolated workspace/project/domain | Development Ship |
| Captain | Per-Ship orchestration agent | Engineering Captain |
| Squad | Functional group inside a Ship | Developer Squad |
| Squad Lead | Domain coordinator inside a Squad | Engineering Lead |
| Crew Member | Specialist agent | QA Engineer |
| Voyage | Workflow or run | Phase 3 Tool Runtime Review |
| Job Order | Task assignment | Run race tests and report failures |
| Ship Log | Immutable activity/audit feed | Tool calls, approvals, outputs |
| Fleet Report | Consolidated executive summary | Weekly Fleet Brief |
| Treasure | Generated artifact/value output | PRD, report, patch, article |
| Map | Task graph, plan, or workflow definition | Multi-Ship project plan |
| Port | External integration or provider endpoint | GitHub MCP, Ollama, OpenRouter |
| Cargo | Source data/documents/input artifacts | Repository, briefs, PDFs |
| Docked | Ship inactive but preserved | Archived marketing project |
| Distress Signal | Escalation/alert | Budget exceeded, policy violation |
| Rules of the Fleet | Fleet-level policy | Restricted data must remain local |

## Naming guardrail

The pirate vocabulary should enrich product identity, but technical terms should remain visible in settings, APIs, logs, and documentation.

Example:

```text
Voyage (Workflow Run)
Job Order (Task)
Ship Log (Audit Timeline)
Treasure (Artifact)
```

This makes the interface approachable for users who enjoy the theme and understandable for professional/enterprise users.

---

## Engine code mapping

> How pirate vocabulary maps to existing Go modules in `engine/src/`.

| Pirate term | Existing code entity | Module | Change type |
|---|---|---|---|
| Voyage | `run.Run`, `run.RunStatus` | `src/run/` | ✅ Rename |
| Job Order | `task.Task`, `task.TaskStatus` | `src/task/` | ✅ Rename |
| Treasure | `artifact.Artifact` | `src/artifact/` | ✅ Rename + extend |
| Crew Member | `crew.AgentDefinition`, `crew.AgentStatus` | `src/crew/` | ✅ Rename + extend |
| Squad | `crew.CrewDefinition` | `src/crew/` | ✅ Rename |
| Map | `workflow.WorkflowTemplate`, `workflow.StepTemplate` | `src/workflow/` | ✅ Rename |
| Port | `tool.Registry`, `llm.MultiProvider`, `tool.MCPClient` | `src/tool/`, `src/llm/` | ✅ Exists |
| Ship Log | `run.RunEvent`, `run.EventHub` | `src/run/` | ✅ Extend |
| Ship | — | — | 🆕 New |
| Fleet | — | — | 🆕 New |
| Pirate King | — | — | 🆕 New |
| Quartermaster | — | — | 🆕 New |
| Captain | — | — | 🆕 New |
| Fleet Report | — | — | 🆕 New |
| Distress Signal | — | — | 🆕 New (Escalation) |


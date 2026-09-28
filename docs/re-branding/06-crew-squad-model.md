# Crew and Squad Model

> Part of [Quartermaster — Fleet Command Architecture](00-overview.md)

---

## Developer Ship

```text
Development Ship
  ├── Engineering Lead / Captain
  ├── Product Owner
  ├── Backend Engineer
  ├── Frontend Engineer
  ├── QA Engineer
  └── R&D Engineer
```

| Member | Mission | Skills | Default tools | Default restrictions |
|---|---|---|---|---|
| Engineering Lead | Decompose work, coordinate, review | Architecture, delegation, integration review | Read repo, inspect artifacts, plan | No direct deploy/push by default |
| Product Owner | Define requirements and acceptance criteria | PRD, backlog, prioritization | Docs/read/report | No code mutation |
| Backend Engineer | APIs, domain, persistence, integration | Go, API, concurrency, schema | Read/search/test/draft patch | Apply patch requires approval |
| Frontend Engineer | UI, state, accessibility | TypeScript, React, UX state | Read/search/build/test/draft patch | Apply patch requires approval |
| QA Engineer | Test strategy and regression prevention | Test plan, contract test, failure triage | Read/search/lint/test/report | No write by default |
| R&D Engineer | Spikes, benchmarks, technology evaluation | Research, prototype, benchmark | Web/research/sandbox/report | Dependency adoption requires approval |

## Marketing Ship

```text
Marketing Ship
  ├── Marketing Lead / Captain
  ├── Market Researcher
  ├── SEO Strategist
  ├── Copywriter
  ├── Content Editor
  ├── Growth Analyst
  └── Brand Strategist
```

| Member | Mission | Skills | Default tools | Default restrictions |
|---|---|---|---|---|
| Marketing Lead | Plan campaigns and prioritize work | Funnel, campaign, positioning | Research/report/plan | No external activation |
| Market Researcher | Find evidence about market/audience/competitors | Source evaluation, synthesis | Web search/fetch, RAG, report | Read-only |
| SEO Strategist | Find content opportunity and on-page issues | Keyword, intent, content gap | Crawl/read/analytics/report | CMS changes require approval |
| Copywriter | Draft content | Brand voice, article/draft workflow | Draft artifact writer | Publish denied by default |
| Content Editor | Check quality/evidence/style | Editorial review, claim validation | Read/review/report | No publish by default |
| Growth Analyst | Interpret performance data | Analytics, experiment design | Read analytics/report | No campaign change by default |
| Brand Strategist | Maintain positioning/messaging | Brand framework, creative brief | Research/draft/report | Public release needs approval |

## Crew Member configuration

A Crew Member must be more than a prompt. It must resolve into:

```text
role
+ mission
+ skill references
+ tool policy reference
+ model route profile
+ memory policy reference
+ evaluation profile
+ budget ceiling
+ concurrency ceiling
+ approval policy
+ artifact output contract
```

## Model route profile

Each Crew Member has a **model route profile** that determines which AI model serves its requests. The Quartermaster recommends the model based on role optimization; the Pirate King can override at any time.

### How the Quartermaster selects models

```text
Role/Skill Analysis
  → classify task complexity: strategic / creative / analytical / lookup
  → match to model tier:
       strategic / creative / multi-step reasoning → pro-tier model
       analytical / structured output             → balanced model
       lookup / data extraction / simple format    → flash-tier model
  → check Ship/Fleet model allowlist
  → check provider health and fallback routes
  → estimate cost per model option
  → select cheapest model that meets the capability threshold
```

### Default model mapping by role type

| Role type | Example roles | Recommended model tier | Rationale |
|---|---|---|---|
| Strategic / Lead | Captain, Marketing Lead, Engineering Lead | `pro` (e.g., `gemini-2.5-pro`) | Needs multi-step reasoning, planning, delegation |
| Creative / Writing | Copywriter, Brand Strategist, Content Editor | `pro` | Needs nuanced language, brand voice, editorial judgment |
| Analytical | Growth Analyst, SEO Strategist, QA Engineer | `balanced` / `flash` | Structured analysis, pattern recognition, data interpretation |
| Research / Lookup | Market Researcher, R&D Engineer | `flash` (e.g., `gemini-2.5-flash`) | High-volume retrieval, source evaluation, synthesis |
| Execution | Backend Engineer, Frontend Engineer | `balanced` / `pro` | Code generation quality matters; context-dependent |

### Pirate King model override

The Pirate King can change the model at four levels:

```text
Fleet Policy → Provider/Model Ceiling (affects all Ships)
  ↓
Ship Settings → Model Allowlist (affects all members in this Ship)
  ↓
Squad Settings → Default Model Profile (affects members without override)
  ↓
Crew Member → Model Route Profile (per-member override)
```

The effective model is always:

```text
Effective Model = PK override (if set)
                  ∩ Squad default (if no member override)
                  ∩ Ship model allowlist
                  ∩ Fleet model ceiling
                  ∩ Provider availability
```

### Model route profile format

```yaml
model_route_profile:
  recommended_by: quartermaster
  recommended_model: gemini-2.5-flash
  recommended_reason: "Lookup/analysis role — fast model sufficient for keyword research and content gap analysis"
  override_model: null           # set by Pirate King to override
  provider_preference:
    - google_ai
    - openrouter
  fallback_model: gemini-2.5-flash-lite
  max_context_tokens: 128000
  estimated_cost_per_day_usd: 0.40
```

## Skill package format

```text
skills/
├── developer/
│   ├── go-concurrency-review/
│   ├── api-contract-review/
│   ├── test-strategy/
│   ├── regression-analysis/
│   └── frontend-accessibility/
├── marketing/
│   ├── competitor-research/
│   ├── keyword-clustering/
│   ├── search-intent-analysis/
│   ├── evidence-backed-copywriting/
│   ├── editorial-review/
│   └── website-growth-audit/
├── fleet/
│   ├── fleet-intake/
│   ├── executive-reporting/
│   ├── risk-escalation/
│   ├── budget-monitoring/
│   ├── artifact-handoff/
│   └── fleet-lesson-curation/
└── shared/
    ├── tool-safety/
    ├── citation-quality/
    ├── artifact-reporting/
    └── approval-aware-execution/
```

### Skill manifest example

```yaml
id: fleet-executive-reporting
version: 0.1.0
name: Fleet Executive Reporting
description: Consolidates approved Ship summaries into concise decision-oriented Fleet Reports.

mode: on_demand
eligible_roles:
  - quartermaster

required_capabilities:
  - fleet.read_summary
  - fleet.read_budget
  - artifact.read_summary
  - artifact.create_report

forbidden_tools:
  - workspace.apply_patch
  - provider.configure
  - policy.update
  - external.publish

inputs:
  - fleet_id
  - report_window
  - escalation_policy

outputs:
  - fleet_report
  - decision_brief
  - risk_register

evaluation:
  rubric: evals/fleet/executive-reporting.yaml
```

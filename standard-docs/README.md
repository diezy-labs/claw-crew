# Galleon Fleet Standard Documentation

**Single source of truth for architecture, conventions, and design standards.**

Last updated: 2026-10-03

---

## Purpose

This directory contains **normative standards** that define how Galleon-fleet is built:

- **Architecture decisions** (ADRs) — recorded one-way-door choices
- **Tech stack conventions** — language-specific patterns, Go/Rust/TS/Tauri rules
- **API contracts** — gRPC/HTTP interfaces between tiers
- **Design system** — UI colors, typography, components, layouts

These docs are **prescriptive** (how things SHOULD be), not descriptive (how things currently are). When code contradicts a standard, the code is wrong, not the doc.

---

## Directory Structure

```
standard-docs/
├── README.md                   # This file
├── decisions/                  # Architecture Decision Records (ADRs)
│   ├── 0001-llm-ssot-rust.md
│   ├── 0002-web2-backend-policy.md
│   └── 0003-tauri-metrics-real-api.md
├── architecture/               # System design, tier boundaries, C4 diagrams
│   ├── 00-overview.md
│   ├── 01-boundary-policy.md
│   └── 03-c4-diagram.puml
├── tech-stack/                 # Language conventions
│   ├── go-conventions.md
│   ├── rust-conventions.md
│   ├── tauri-conventions.md
│   └── web-conventions.md
├── design-system/              # UI standards
│   ├── colors.md
│   ├── typography.md
│   ├── components.md
│   └── layout-grid.md
└── api-contracts/              # Interface specs
    ├── grpc-system-gateway.md  # Rust :50052 ← Go client
    ├── grpc-agent-engine.md    # Go :50051 ← web/Tauri HTTP
    └── http-engine-api.md      # Go :9090 routes
```

---

## When to Read This

### **Crew Roles**

| Role | Start Here |
|------|------------|
| **Backend** (Go/Rust) | `tech-stack/go-conventions.md`, `tech-stack/rust-conventions.md`, `api-contracts/` |
| **Frontend** (React/TS) | `tech-stack/web-conventions.md`, `design-system/`, `api-contracts/http-engine-api.md` |
| **Lead Squad** | `architecture/`, `decisions/` (ADRs), all tech-stack/* |
| **Product Owner** | `architecture/00-overview.md`, `decisions/` (ADRs) |
| **QA** | `api-contracts/`, `tech-stack/` (test patterns per language) |
| **Design** | `design-system/` (all), `architecture/00-overview.md` (context) |

### **Task Types**

| Task | Read This |
|------|-----------|
| New feature | `architecture/01-boundary-policy.md` (which tier?), relevant `tech-stack/*.md` |
| Bug fix | Relevant `tech-stack/*.md` (coding standards), `api-contracts/` (if cross-tier) |
| API change | `api-contracts/` (contract spec), ADRs if it affects SSOT |
| UI/UX work | `design-system/` (all), `tech-stack/web-conventions.md` |
| Refactor | ADRs (understand past decisions), `architecture/01-boundary-policy.md` |

---

## Relationship to Other Docs

| Directory | Purpose | Relationship |
|-----------|---------|--------------|
| `docs/refactoring/` | Living refactoring plans (Phase 3, 4, 5) | **Tactical** — short-term execution plans that become stale when done |
| `standard-docs/` | **Normative standards** | **Strategic** — evergreen rules that outlive any single phase |
| `docs/maintainers/` | Operational runbooks (audit, security, UTF-8) | **Procedural** — how to maintain, not how to build |
| `docs/security/` | Security policies, threat models | **Specialized** — security-only subset, referenced by `architecture/01-boundary-policy.md` |
| `docs/_archive/` | Obsolete planning docs (research/, book/) | **Historical** — read-only reference, not authoritative |

**Rule:** If a refactoring doc contradicts a standard doc, **the standard wins**. Refactoring plans must comply with standards, not replace them.

---

## How to Update

### **ADRs (Architecture Decision Records)**

ADRs are **immutable once accepted** — never edit the decision or rationale. To reverse a decision:

1. Create a new ADR that **supersedes** the old one (e.g. "ADR 0004: Reversal of ADR 0001")
2. Mark old ADR status as `Superseded by ADR 0004`
3. Explain why the original rationale no longer holds

### **Tech Stack Conventions**

When a new pattern becomes standard (approved by Lead + PO):

1. Update relevant `tech-stack/*.md` with the rule
2. Add concrete example (code snippet or file reference)
3. PR to `standard-docs/` (reviewed by Lead Squad)

### **API Contracts**

When a gRPC/HTTP contract changes:

1. Update proto/OpenAPI spec first
2. Update `api-contracts/*.md` to match
3. Both in same PR (contract + doc never diverge)

### **Design System**

When a new UI component or pattern is approved:

1. Update `design-system/*.md` with spec
2. Add Figma/screenshot reference
3. PR reviewed by Design + Frontend lead

---

## Decision Authority

| Standard Type | Approver |
|---------------|----------|
| ADR (architecture) | Product Owner (Achmad) + Lead Squad |
| Tech conventions | Lead Squad + relevant crew lead (Backend/Frontend) |
| API contracts | Lead Squad + Backend lead |
| Design system | Design crew + Product Owner |

---

## Phase 1 Status (Current)

✅ **Completed:**
- 3 ADRs (LLM SSOT, web-2 policy, Tauri metrics)
- Directory structure created
- Obsolete docs archived (`docs/_archive/research-planning-era/`, `docs/_archive/book-kirocrew-upstream/`)

⏳ **In Progress (Phase 2):**
- `architecture/*.md` (4-tier overview, boundary policy, C4 diagram)
- `tech-stack/*.md` (Go/Rust/Tauri/web conventions from audit)
- `design-system/*.md` (extract from archived `docs/research/fundamental/design/`)
- `api-contracts/*.md` (gRPC proto + HTTP routes)

**ETA Phase 2:** 2-3 days after ADR decisions are implemented (Phase 3 refactor work).

---

## Related Reading

- `.gemini/architecture.md` — High-level 4-tier description (being rewritten per audit findings)
- `AGENTS.md` — Crew automation rules, single source of truth principles
- `docs/maintainers/audit-policy.md` — How to audit for SSOT violations
- `docs/security/` — Security boundaries enforced by `architecture/01-boundary-policy.md`

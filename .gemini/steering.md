# Steering: Galleon Architecture & Codebase Navigation

This steering document governs how AI agents architect, implement, query, and navigate the Galleon Fleet AI codebase. It applies to all agent sessions (Antigravity, Claude Code, subagents).

Related documents & skills:
- Architecture & Layering Specification: [`.gemini/architecture.md`](architecture.md)
- Rust Standards: [`.gemini/rules/rust-standards.md`](rules/rust-standards.md)
- Graph-First Codebase Navigation: [`.gemini/skills/graphify/SKILL.md`](skills/graphify/SKILL.md)

---

## 1. 3-Tier Layering Governance (STRICT RULE)

All implementations MUST adhere to the following separation of concerns:

| Component | Lokasi | Bahasa & Runtime | Tugas Riil di Repo | Jalur Akses / Komunikasi |
|---|---|---|---|---|
| **Desktop Shell** | `apps/tauri-2/` | Rust (Tauri v2 + WRY) | Window framing, system tray icon, OS dialogs/notifikasi, packaging desktop `.exe`. | **Tauri IPC** (`invoke`) dari webview React. |
| **System Core** | `crates/` | Rust (2024 edition) | Security microkernel (`clawcrew-runtime`), Landlock LSM sandboxing, native tool execution (`clawcrew-tools`), mDNS/A2A network (`clawcrew-gateway`), channel adapters (`clawcrew-channels`), WASI plugins (`clawcrew-plugins`). | **gRPC Server** `SystemGateway` (:50052) dipanggil oleh Go; atau direct CLI. |
| **AI Orchestrator** | `engine/` | Go 1.25+ / Go 1.27 | Agent Brain (`StartTurn`), Fleet governance (Quests, Ships, Squads, Crew, Approvals), vector memory RAG (`memory/`), persistensi kanonikal `DiskStore`. | **gRPC Server** `AgentEngine` (:50051) & **HTTP Gateway** (:9090). |
| **Interface / UI** | `web-2/` | TypeScript, React 19 | Visual rendering, view-models (`useFleetStore`), client-bridge (`apiClient.ts`). Zero backend APIs. | HTTP reverse proxy ke Go (:9090) / Tauri IPC. |

*Detail arsitektur, boundary invariants, dan sequence diagram terdokumentasi lengkap di [`.gemini/architecture.md`](architecture.md).*

---

## 2. Graph-First Codebase Navigation Principle

**Graph first, grep second, manual browsing never.**

All codebase queries — symbol lookup, path discovery, dependency tracing, module understanding, call-flow tracing, and architecture questions — MUST follow this resolution order:

1. **Graphify indexing** → Refresh or build the knowledge graph first.
2. **Graphify query** → Query `graphify-out/` with `query`, `path`, or `explain`.
3. **Grep** → Use targeted `grep`/`Select-String` only for line-level detail after Graphify has identified the target file(s) and symbol(s).
4. **Direct file read** → Open specific files only after the Graphify query and targeted search have identified them.

For every query/path/flow investigation, run `graphify update .` when
`graphify-out/graph.json` exists. If it does not exist, run `graphify .` first.
This indexing step is mandatory even when the requested lookup appears simple.
VS Code Copilot follows the project-scoped rules in `.github/copilot-instructions.md`.

> **NEVER** start with broad recursive grep, `find`, or directory walks to understand architecture, locate symbols, or discover relationships. Use the graph.

## Graphify Output Structure

```
graphify-out/
├── graph.html        # Interactive graph visualization
├── GRAPH_REPORT.md   # God nodes, surprising connections, suggested questions
├── graph.json        # Persistent knowledge graph (query without re-reading)
├── obsidian/         # Open as Obsidian vault for visual exploration
├── wiki/             # Wikipedia-style articles for agent navigation
└── cache/            # SHA256 cache — re-runs only process changed files
    ├── ast/          # Per-file AST analysis
    ├── semantic/     # Semantic relationship extraction
    └── stat-index.json  # File metadata and hash index
```

## Agent Query Protocol

### Step 1: Check graph availability

Before any codebase query, verify that `graphify-out/` exists:

```powershell
# Windows (pwsh)
Test-Path graphify-out/graph.json

# If missing, generate the graph first:
graphify .
# Or if graphify CLI is unavailable:
python -m graphify .
```

If `graphify-out/` does not exist or `graph.json` is missing, **generate the graph before proceeding**. Do not skip this step.

### Step 2: Query the graph

Use graphify's query capabilities to answer questions:

```powershell
# Semantic query — find concepts and relationships
graphify query "what modules handle task cancellation?"

# Path query — find connections between concepts
graphify path "TaskRecord" "SessionLifecycle"

# For structural queries, read graph.json or the wiki/
```

For agents without CLI access to `graphify`, read these files in order:

1. `graphify-out/GRAPH_REPORT.md` — Architecture overview, god nodes, key connections.
2. `graphify-out/wiki/` — Per-module and per-concept articles.
3. `graphify-out/graph.json` — Raw graph data for programmatic queries.
4. `graphify-out/cache/stat-index.json` — File index with sizes, hashes, and word counts.

### Step 3: Targeted grep for detail

After graphify identifies the relevant file(s), use `Select-String` for line-level precision:

```powershell
# GOOD: graphify told us TaskRecord is in task_registry.rs
Select-String -Pattern "pub struct TaskRecord" crates/clawcrew-runtime/src/control_plane/task_registry.rs

# BAD: blind recursive search without checking the graph
Get-ChildItem -Recurse -Filter "*.rs" | Select-String "TaskRecord"
```

### Step 4: Read file content

Open only the specific files and line ranges that step 2 and 3 identified.

## When to Re-index

Run `graphify . --update` (incremental) when:

- New modules or crates are added.
- Major refactors change module boundaries.
- Before starting a new architectural task (P0, P1, P2, etc.).

Run `graphify .` (full re-index) when:

- The graph is more than a week old.
- The codebase has undergone a large merge or rebase.

## Anti-Patterns (Prohibited)

| ❌ Don't | ✅ Do instead |
|---|---|
| `Get-ChildItem -Recurse \| Select-String "pattern"` as first step | `graphify query "what handles pattern?"` |
| Browsing random directories to "understand the codebase" | Read `GRAPH_REPORT.md` for architecture overview |
| Opening 10+ files to find a symbol definition | Query the graph, then open the one identified file |
| Guessing file paths from memory | Check `stat-index.json` or the graph for canonical paths |
| Skipping graph when it exists | Always check graph first, even for "simple" lookups |

## Subagent Directive

All subagents (research, implementation, review) inherit this steering. When spawning subagents:

- Include "Use graphify-out/ for codebase navigation per steering.md" in the subagent prompt.
- Subagents MUST set their working directory to the repository root before any graphify or file operation.

## Compatibility

This steering is designed to work with:

- **Antigravity** (this platform) — via `.gemini/steering.md`
- **Claude Code** — via `.claude/CLAUDE.md` reference
- **Other AI agents** — via `AGENTS.md` reference

The canonical source of truth for this policy is this file: `.gemini/steering.md`.


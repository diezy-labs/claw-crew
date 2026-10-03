---
title: Principle Briefing — Phase 3 Track A + B (Code Review Readiness)
author: squad-lead
date: 2026-10-03
status: active
branch: phase-3-track-ab
---

# Principle Briefing — Phase 3

What this is: the design intent behind Phase 3 code, so review catches *wrong behavior*, not just style.
What I checked before writing this: read `proto/agent_service.proto`, `crates/clawcrew-gateway/src/grpc_system_gateway.rs`
(full file), `engine/pkg/client/system_gateway.go`, and grepped `engine/src/**` for the 5 Track B handlers. Findings
below reflect what's **actually on disk on `phase-3-track-ab` right now**, not the original plan — the plan and the
code have already diverged in two places (noted in §5).

## 1. Scope Summary

**Track A — per-request workspace + timeout routing for `ExecuteNativeTool`.**
Today one Rust gateway process (`:50052`) enforces one sandbox root fixed at construction. Track A lets a Go caller
pass a per-request `workspace_path` + `timeout_seconds`, so one gateway process can safely touch different fleet
workspaces per call instead of being locked to one root for its whole lifetime. **Not** multi-process fleet routing
— one process, one Landlock-style boundary, request-scoped path argument. Backward compatible by construction: both
new proto fields are `optional`, unset behaves byte-identical to pre-Phase-3.

**Track B — 5 orphaned API handlers.** Frontend (`web-2/src/utils/apiClient.ts`) already calls 5 (effectively 7,
2 folded together) Go HTTP routes that were never implemented server-side. These are read-mostly introspection
endpoints (metrics, process list, health, network, Ollama status/generate) plus one real-risk one,
`POST /api/engine/execute` (arbitrary shell exec). The frontend contract already exists; this track implements the
missing Go side to match it, not invent a new contract.

**Status check — Track B is NOT yet implemented on this branch.** I grepped `engine/src/**` for all 5 route
strings; zero matches. Only Track A is coded (proto + Rust gateway + Go client wrapper). If a PR lands claiming
Track B, verify the handler file actually exists before reviewing style — don't assume it's there because the spec
says it should be.

## 2. Technical Architecture (Track A)

```
Go engine (ExecuteNativeToolWithWorkspace) --gRPC--> Rust gateway (execute_native_tool)
    workspacePath, timeoutSeconds (int32)                validate_path_with_workspace()  <- containment check
                                                            tokio::time::timeout()         <- enforcement
```

- Proto (`proto/agent_service.proto:89-101`): `ToolCallRequest.workspace_path` (optional string, field 3),
  `timeout_seconds` (optional int32, field 4). `ToolCallResponse.exit_code` (optional int32, field 4). Matches spec
  exactly — verified by reading the file, not trusting the doc.
- Go (`engine/pkg/client/system_gateway.go:19,85`): new method `ExecuteNativeToolWithWorkspace(ctx, toolName,
  argumentsJSON, workspacePath, timeoutSeconds)`. Old `ExecuteNativeTool` (no workspace/timeout) kept as-is for
  existing callers — this is the compat path, confirm no caller was silently migrated without a diff you can see.
  `workspacePath == ""` → field left unset on the wire (line ~103-104), not sent as empty string. **This is the
  detail to check in review**: if a PR changes this to always-set, backward compat silently breaks.
- Rust (`crates/clawcrew-gateway/src/grpc_system_gateway.rs`): see §3 for the security-relevant parts.

## 3. Security Gates — Track A

**Rust-side canonicalization + containment (`validate_path_with_workspace`, lines 45-78).**
Walks up from the requested path to the nearest existing ancestor, canonicalizes that ancestor, re-attaches the
non-existent suffix, then checks `canonical_requested.starts_with(&canonical_root)`. This exists because the naive
version — canonicalize the full requested path directly — fails for paths that don't exist yet (a file about to be
written). **Review rule: any new path-accepting code path must call this existing function, not a new one.** There
is exactly one containment check in this file; a PR adding a second one is a duplicate-logic smell, not a feature.

**Timeout clamp (lines 213-215).** `req.timeout_seconds.unwrap_or(60)`, then
`.max(1).min(300)` before building the `Duration`. Correctly handles negative/zero (clamped to 1, not an
instant-timeout DoS) and `i32::MAX` (clamped to 300). **This is clamp, not reject** — differs slightly from the
original spec (which said reject negative values with an error). Clamping is arguably fine (fails safe, not
silent-wrong), but if a PR changes clamp behavior, confirm the new behavior still can't produce an unbounded or
zero-second timeout.

**Two gaps found in the current code — flag these in review if a PR touches this file without fixing them:**

1. **`execute_bash` ignores `workspace_root` entirely** (around line 86-100 in the method body). `git` gets
   `.current_dir(&root)` (line ~117); `bash` does not — the spawned `bash -c <command>` runs in the gateway
   process's actual OS cwd, not the validated/requested workspace. The containment check validates the
   *argument*, but nothing scopes the *process* to it for `bash`. A workspace-escape via `bash "cat ../../etc/..."`
   bypasses `validate_path_with_workspace` entirely because that function is never called for the `bash` tool at
   all — it's only invoked for `read_file`/`write_file` and (indirectly, for cwd only) `git`. **If a Track A
   follow-up PR adds `.current_dir(&workspace_root)` to `execute_bash`, that's the fix — approve it. If a PR
   extends `bash` capability without this fix, request changes.**
2. **`ALLOWED_TOOLS` is `["bash", "read_file", "write_file", "git"]` — `bash` is unrestricted-command, not
   allowlisted-command.** The tool-name allowlist gates *which tool*, not *which command* — once `bash` is picked,
   any shell command runs. This is a known, already-live risk (not new to Phase 3), but Phase 3 is the first time
   `workspace_path` makes it plausible for an attacker-controlled or buggy caller to point that `bash` at a
   different fleet's workspace path argument (even though, per gap #1, the argument isn't actually enforced for
   bash's cwd — so right now the blast radius is "wrong path reported," not "wrong path executed in," but don't
   assume that stays true after gap #1 is fixed without re ‑checking that the fix doesn't just enable scoped bash
   without re-sandboxing it, e.g. Landlock/chroot, which this file does not have).

**TypeScript-side validation requirement.** Track A has no FE work landing yet (E4, the dropdown + bounded numeric
input for `workspace_path`/`timeout_seconds`, is still planned per the tech spec, not seen in this diff). When it
does land: `workspace_path` must be a **dropdown from the known Ships list, not free text** — free text here is a
path-traversal UI footgun feeding directly into the one Rust check above. `timeout_seconds` must be a bounded
numeric input `min=1 max=300` matching the server clamp — if the UI allows a wider range than the server accepts,
that's not a security bug but is a correctness one (user sees "accepted" then gets silently clamped). Both are
review-blocking if the FE PR skips them, because the Rust check is sound but is the *last* layer, not the only
useful one — bad UI input still costs a round trip and a confusing error instead of a disabled control.

## 4. API Spec — Track B (not yet implemented; spec for when it lands)

```
GET  /api/system/metrics            -> 200 Record<string, unknown>
GET  /api/engine/processes          -> 200 Array<{pid:int, name:string, memory:int, cpu:number}>
POST /api/engine/execute             body {command:string}
                                     -> 200 {stdout:string, exitCode:int, duration:string} | 403 (approval denied)
GET  /api/health                    -> 200 {status:string, uptime:string, version:string}
GET  /api/system/network            -> 200 Record<string, unknown>
GET  /api/providers/ollama/status   -> 200 {available:bool, host:string, models?:[...]}
POST /api/providers/ollama/generate  body {model:string, prompt:string, stream:bool} -> 200 {response:string}
```

**Authentication model.** I did not find a shared auth middleware wired into `engine/src/**` HTTP routes in this
repo (no `RequireAuth`/`authMiddleware` hits across the delivery files). Track B does not change this — these are
new routes added to the same unauthenticated HTTP surface the rest of `engine/src/fleet/delivery.go` already runs
on. **This is a pre-existing condition, not a Track B regression — don't block a Track B PR on "add auth" unless
Lead has separately scoped an auth pass.** But `POST /api/engine/execute` landing on that same unauthenticated
surface is the one item in Track B that actually matters for this gap (see §5 below) — everything else is
read-only introspection.

**`POST /api/engine/execute` — review this one hardest.** Per the tech spec, it MUST reuse the existing
approval-gate pattern (`engine/src/approval/**`, `riskTiers.json`'s `approvalRequired` flag, same mechanism the
builtin tool policy already uses) — not a new approval mechanism. When the PR lands: confirm (a) it calls into
that existing approval service rather than reimplementing a check, (b) the 403 body shape matches the existing
approval-rejection shape used elsewhere, not a bespoke one, (c) it does not bypass the gate for any input shape
(e.g. an empty command, a command the allowlist-equivalent for HTTP exec considers "safe" — there's no such
allowlist for this route yet based on the spec, so confirm one exists if the PR claims risk-tiering by command
content).

## 5. Divergence From the Original Plan — Flag, Don't Silently Accept

1. **Timeout: clamp vs reject.** Spec said reject negative/out-of-range `timeout_seconds` with an error. Code
   clamps instead (`.max(1).min(300)`). Both are defensible; this is a design decision Lead should confirm was
   intentional, not something Principle should wave through as "matches spec" without noticing it doesn't, nor
   block as "wrong" without checking with Lead first — flag it in the PR comment, don't unilaterally request a
   revert.
2. **Track B is unimplemented.** If a PR title says "Phase 3 Track B" but the diff is small or doesn't touch
   `engine/src/fleet/delivery.go` (or wherever the 5 handlers end up), that's a scope mismatch — check the diff
   against the §4 route list before approving, not against the PR title.

## 6. Code Review Focus Areas — Checklist Specific to Phase 3 Patterns

- [ ] Any new path-handling code calls the **existing** `validate_path_with_workspace` — no second containment
      routine introduced.
- [ ] `execute_bash` sets `.current_dir(&workspace_root)` (or equivalent) before this PR merges — if it still
      doesn't, and the PR is adding workspace-aware capability to bash, this is a request-changes blocker.
- [ ] `timeout_seconds` handling stays within `[1, 300]` under every code path, including the response error
      message's own copy of `timeout_seconds` (don't let the error message report an unclamped value while the
      actual `Duration` used the clamped one — misleading on timeout).
- [ ] New proto fields stay `optional`; no PR flips an optional Phase-3 field to required (breaks every existing
      caller that never set it).
- [ ] `POST /api/engine/execute` calls the existing approval/`riskTiers.json` gate — reject a PR that adds its own
      inline risk check instead of reusing `engine/src/approval/**`.
- [ ] Track B response shapes match §4 exactly (these are frontend-dictated contracts, read from
      `web-2/src/utils/apiClient.ts`, not invented) — a handler returning a different field name/shape than the FE
      already expects is a correctness bug, not a style nit.
- [ ] Ollama routes (`/api/providers/ollama/*`) fail soft (`503 {available:false,...}`) when the local Ollama
      daemon isn't running — must not panic/crash the handler for an expected "not installed" case.
- [ ] No new auth middleware expected on Track B (pre-existing gap, see §4) — don't block on it unless Lead has
      separately asked for an auth pass across the whole `engine/src/fleet` surface.

## References

- Full tech spec: `docs/refactoring/phase3/PHASE3-SCOPE-TECH-SPEC.md`
- Principle role/workflow/generic checklist: `docs/refactoring/phase3/PRINCIPLE-CREW-ONBOARDING.md`
- Provider routing correction (separate track, not reviewed by Principle unless assigned): `docs/refactoring/phase3/provider-routing/PHASE3-CORRECTION.md`

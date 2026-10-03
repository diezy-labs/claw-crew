---
title: Terminal UI Tech Spec — Option A (Tauri IPC + portable-pty)
status: ready-for-execution
owner: squad-lead
created: 2026-10-03
---

# Terminal UI Tech Spec — Tauri IPC + portable-pty

## 0. Context (verified against current code, not assumed)

- `apps/tauri-2/src/main.rs:execute_terminal_command` (line ~165) is a **fake shell**: it pattern-matches a handful of literal strings (`"fleet status"`, `"fleet bell"`, `"fleet check"`, `echo `) and otherwise just echoes back `"[Tauri IPC] Executed: {cmd}"`. It never runs a real process. This is the function being replaced.
- The real frontend is `web-2` (React 19.0.1, Vite — `tauri.conf.json` points `frontendDist` at `../../web-2/dist`). There is **no React tree inside `apps/tauri-2`**; the task brief's "apps/tauri-2 React" wording is corrected to `web-2`.
- The terminal panel today is `web-2/src/components/features/EngineRoomView.tsx`: a hand-rolled `TerminalLine[]` state array rendered as styled `<div>`s, not an xterm.js instance. There is no xterm dependency yet (`web-2/package.json` confirmed clean).
- `apps/tauri-2/Cargo.toml` has no `portable-pty`; standalone crate (`[workspace]` empty — not part of the root Cargo workspace), so adding a dependency here does not touch `crates/`.
- `apps/tauri-2/capabilities/default.json` currently grants `core:default, shell:allow-open, dialog:default, fs:default, notification:default` — no shell-execute capability. Good: nothing to revoke.
- `web-2/server.ts:165-195 POST /api/engine/execute` is the known **unauthenticated RCE** (separate Lead-flagged issue, token-gated as an interim measure). This spec does **not** reuse that endpoint — the PTY lives entirely in the Tauri process via IPC, never over HTTP. That boundary is the point of Option A.

## 1. Architecture

```
┌─────────────────────────── Tauri desktop process ───────────────────────────┐
│                                                                               │
│  web-2 (React, webview)                 apps/tauri-2 (Rust, src/pty.rs)     │
│  ┌─────────────────────┐                ┌──────────────────────────────┐    │
│  │ EngineRoomView.tsx   │  invoke()      │ terminal_spawn(cwd)          │    │
│  │  xterm.js instance   │ ─────────────► │   -> workspace allowlist     │    │
│  │                      │                │   -> PtySystem::openpty      │    │
│  │  terminal_write()    │ ─────────────► │ terminal_write(id, data)     │    │
│  │  terminal_resize()   │ ─────────────► │ terminal_resize(id, cols,rows│    │
│  │  terminal_kill()     │ ─────────────► │ terminal_kill(id)            │    │
│  │                      │                │                              │    │
│  │  listen(`pty:data:   │ ◄───────────── │ reader thread -> emit event │    │
│  │   {session_id}`)     │   event        │   (ConPTY on Windows /       │    │
│  │  xterm.write(bytes)  │                │    Unix PTY elsewhere, via   │    │
│  └─────────────────────┘                │    portable-pty)             │    │
│                                          │                              │    │
│                                          │ Mutex<HashMap<session_id,    │    │
│                                          │   PtySession>>               │    │
│                                          │  { master, writer, child,    │    │
│                                          │    log_file }                │    │
│                                          └──────────────────────────────┘    │
│                                                                               │
└───────────────────────────────────────────────────────────────────────────────┘
```

No network hop. No HTTP server. The PTY child process is spawned and owned by the desktop process; it is reachable only through Tauri's IPC bridge, which the webview capability file scopes to the `terminal_*` commands explicitly listed below — nothing else gets shell access.

## 2. Rust module — `apps/tauri-2/src/pty.rs`

### 2.1 Dependencies (add to `apps/tauri-2/Cargo.toml`)

```toml
[dependencies]
portable-pty = "0.8"
# tokio already present ("1.36", features=["full"]) — reuse it, no version bump.
uuid = { version = "1", features = ["v4"] }
```

Do not add a second `tokio` or a `crossbeam` channel dependency — `std::sync::mpsc` + a spawned `std::thread` reader is enough for one PTY reader loop; this is I/O-bound and tiny, pulling in an async PTY crate would be the un-lazy choice here.

### 2.2 Session manager

```rust
// apps/tauri-2/src/pty.rs
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize, MasterPty, Child};
use tauri::{AppHandle, Emitter, State};

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
pub struct PtyState(pub Mutex<HashMap<String, PtySession>>);
```

`PtyState` is registered once via `app.manage(PtyState::default())` in `main.rs`'s `.setup()` — same lifecycle pattern already used there for the tray, no new bootstrapping concept introduced.

### 2.3 Commands

```rust
#[tauri::command]
fn terminal_spawn(
    app: AppHandle,
    state: State<PtyState>,
    cwd: String,
) -> Result<String, String> {
    let safe_cwd = validate_workspace_path(&cwd)?; // see §4.1 — same gate as execute endpoint

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("pty open failed: {e}"))?;

    let shell = if cfg!(windows) { "powershell.exe" } else { "/bin/sh" };
    let mut cmd = CommandBuilder::new(shell);
    cmd.cwd(&safe_cwd);

    let child = pair.slave.spawn_command(cmd).map_err(|e| format!("spawn failed: {e}"))?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let log_path = audit_log_path(&app, &session_id); // §4.2

    // Reader thread: PTY -> audit log + frontend event. One thread per session;
    // ponytail: acceptable ceiling for a desktop app with a handful of concurrent
    // terminal tabs, not for a server fanning out thousands of sessions.
    let emit_id = session_id.clone();
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).ok();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF: child exited
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                    if let Some(f) = log.as_mut() { let _ = f.write_all(chunk.as_bytes()); }
                    let _ = app_handle.emit(&format!("pty:data:{emit_id}"), chunk);
                }
                Err(_) => break,
            }
        }
        let _ = app_handle.emit(&format!("pty:exit:{emit_id}"), ());
    });

    state.0.lock().unwrap().insert(session_id.clone(), PtySession {
        master: pair.master, writer, child,
    });
    Ok(session_id)
}

#[tauri::command]
fn terminal_write(state: State<PtyState>, session_id: String, data: String) -> Result<(), String> {
    let mut sessions = state.0.lock().unwrap();
    let session = sessions.get_mut(&session_id).ok_or("session not found")?;
    session.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
fn terminal_resize(state: State<PtyState>, session_id: String, cols: u16, rows: u16) -> Result<(), String> {
    let sessions = state.0.lock().unwrap();
    let session = sessions.get(&session_id).ok_or("session not found")?;
    session.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())
}

#[tauri::command]
fn terminal_kill(state: State<PtyState>, session_id: String) -> Result<(), String> {
    let mut sessions = state.0.lock().unwrap();
    if let Some(mut session) = sessions.remove(&session_id) {
        let _ = session.child.kill();
    }
    Ok(())
}
```

Register all four in `main.rs`'s existing `tauri::generate_handler![...]` list alongside the current commands, and remove `execute_terminal_command` from that list (and delete the function) in the same change — do not leave the fake command dangling as dead code once the real one ships, per DoD below.

### 2.4 Event channel

- Event name: `pty:data:{session_id}` (per-session, not a single shared `pty:data` — avoids every open tab re-filtering every other tab's output).
- `pty:exit:{session_id}` fired once on EOF so the frontend can mark the tab closed without polling.
- No request/response needed for streaming output — Tauri's event system is the native fit here (already a plugin in use); do not build a second WebSocket channel, that would duplicate what IPC events already do.

## 3. Frontend — `web-2`

### 3.1 Dependency

```json
// web-2/package.json dependencies
"@xterm/xterm": "^5.5.0",
"@xterm/addon-fit": "^0.10.0"
```

(`@xterm/*` is the maintained successor to the old `xterm`/`xterm-addon-fit` packages — pin exact-ish minors, not a caret range wider than minor.)

### 3.2 Binding — replace the fake buffer in `EngineRoomView.tsx`

Current state (`terminalLines: TerminalLine[]`, manually appended, rendered as styled divs) is removed for the `'terminal'` tab specifically — the other tabs (`sessions`, `history`, `processes`, `logs`, `connections`) are untouched; this is a surgical swap of one tab's implementation, not a rewrite of the view.

```tsx
// web-2/src/components/features/EngineRoomView.tsx (terminal tab only)
import { Terminal as XTerm } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// inside EngineRoomView, terminal tab mount:
useEffect(() => {
  if (activeTab !== 'terminal') return;
  const term = new XTerm({ convertEol: true, fontSize: 13 });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(terminalContainerRef.current!);
  fit.fit();

  let sessionId: string;
  let unlistenData: () => void;
  let unlistenExit: () => void;

  (async () => {
    sessionId = await invoke<string>('terminal_spawn', { cwd: workspaceRoot });
    unlistenData = await listen<string>(`pty:data:${sessionId}`, (e) => term.write(e.payload));
    unlistenExit = await listen(`pty:exit:${sessionId}`, () => term.write('\r\n[session ended]\r\n'));
    term.onData((data) => invoke('terminal_write', { sessionId, data }));
    term.onResize(({ cols, rows }) => invoke('terminal_resize', { sessionId, cols, rows }));
  })();

  return () => {
    unlistenData?.();
    unlistenExit?.();
    if (sessionId) invoke('terminal_kill', { sessionId }).catch(() => {});
    term.dispose();
  };
}, [activeTab]);
```

`workspaceRoot` is whatever the app already uses as the sandboxed project root (check `apiClient.ts` / existing workspace-path plumbing before inventing a new source for it — reuse, don't duplicate).

### 3.3 What is explicitly NOT in scope

- No multi-tab terminal session switcher UI, no persistent scroll-back-to-disk, no copy/paste toolbar beyond what xterm.js gives for free. The task brief's `sessions`/`history` tabs in `EngineRoomView` stay backed by their current (separate, non-PTY) data for now — wiring them to real PTY session metadata is a follow-up, not part of this DoD.

## 4. Security & logging

### 4.1 Workspace containment

Reuse (do not reimplement) the path-validation logic already written for the `/api/engine/execute` token-gate fix (`web-2/server.ts` — same canonicalize + prefix-check approach). Port it to Rust as `validate_workspace_path(cwd: &str) -> Result<PathBuf, String>`:
- Canonicalize the requested `cwd`.
- Reject if it resolves outside the configured workspace root (same root `apiClient.ts`/engine already treats as the sandbox boundary).
- Reject path-traversal sequences before canonicalization as a defense-in-depth check (canonicalize alone is sufficient on both Windows and Unix, but the existing fix already does the belt-and-suspenders pattern — match it for consistency rather than inventing a different check here).

### 4.2 Audit logging

- PTY I/O (both directions is **not** required for v1 — logging the output stream, which is what a reviewer/incident responder needs, is sufficient; logging raw keystrokes doubles disk writes for no investigative gain beyond what server-side command auditing tools already do, and risks capturing passwords typed interactively).
- Log path: `{app_data_dir}/pty-logs/{session_id}.log`, written append-only by the same reader thread that emits to the frontend (one write, two sinks — do not open a second read of the PTY to produce a duplicate log stream).
- No log rotation/retention policy in v1 (desktop app, low volume) — flag as a known ceiling, not solved here.

### 4.3 Capability scope

Add nothing to `shell:*` permissions — `portable-pty` spawns the shell directly from Rust, bypassing `tauri-plugin-shell` entirely, so the existing capability file's lack of `shell:allow-execute` is correct and must stay that way. The only capability change needed is none, **provided** the four `terminal_*` commands are plain `#[tauri::command]` functions (core IPC, covered by `core:default` already granted) rather than going through the shell plugin. Confirm this with `tests/capability_security.rs` (already present, asserts the webview does not get plugin/remote access) — extend it with one assertion that `shell:allow-execute` is absent, so a future PR cannot silently add it back.

## 5. Definition of Done

- [ ] `apps/tauri-2/src/pty.rs` compiles; `terminal_spawn`/`write`/`resize`/`kill` registered in `main.rs`, `execute_terminal_command` deleted (not left as dead code).
- [ ] Manual local test: spawn a session, run `echo hello`, see it round-trip through xterm.js in the running app.
- [ ] `validate_workspace_path` rejects a `cwd` outside the sandbox root (unit test, mirrors the existing `/api/engine/execute` traversal test).
- [ ] `cargo test -p fleet-ai-desktop` passes, including the extended `capability_security.rs` assertion.
- [ ] xterm.js panel renders in `EngineRoomView.tsx`'s terminal tab; resize propagates (`fit.fit()` → `terminal_resize`).
- [ ] PTY output is written to `{app_data_dir}/pty-logs/{session_id}.log`.
- [ ] No `shell:*` execute permission added to `capabilities/default.json`.

## 6. Task split

| Crew | Scope (file ownership) | Depends on |
|---|---|---|
| **Backend** | `apps/tauri-2/src/pty.rs` (new), `apps/tauri-2/src/main.rs` (remove `execute_terminal_command`, register new commands + `app.manage(PtyState::default())`), `apps/tauri-2/Cargo.toml` (deps), `apps/tauri-2/tests/capability_security.rs` (extend) | `validate_workspace_path` logic ported from `web-2/server.ts` — read that file first, do not re-derive the traversal rules from scratch |
| **Frontend** | `web-2/package.json` (xterm deps), `web-2/src/components/features/EngineRoomView.tsx` (terminal tab only — do not touch other tabs' files) | Backend's four command names + event name contract (§2.3/§2.4) — frozen by this spec, frontend can start against the contract without waiting for Backend's implementation to land |

Serialization note: both crews touch files under `apps/tauri-2` vs `web-2` respectively — zero file overlap, so this genuinely parallelizes (~6-8h total as estimated). The only shared contract is the command/event names in §2.3-2.4, which are fixed by this spec and should not change without re-syncing both crews.

## 7. Open items for Principle review (flag, do not block start)

- `terminal_spawn`'s default shell is unconditional `powershell.exe` / `/bin/sh` — no user-configurable shell choice in v1. Acceptable ceiling; note it, don't build a shell-picker nobody asked for.
- Reader thread has no backpressure if the frontend is slow to drain events — fine at interactive-terminal data rates, would need a bounded channel if this were ever repurposed for high-throughput log streaming (it should not be).

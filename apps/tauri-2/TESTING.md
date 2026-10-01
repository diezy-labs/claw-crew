# Desktop app — testing notes

## Startup flow

The desktop app is a thin shell over a running ClawCrew **web gateway**. There
is no longer a macOS/Windows/Linux permission-setup wizard — the app goes
straight to the gateway, and first-time setup happens in the web Quickstart.

The system tray menu includes `Toggle Service`. Turning the service off stops
only the daemon supervisor owned by this desktop app; the desktop window and
tray remain open. Turning it on starts the supervisor again. A daemon that was
already running before the app launched is reused but is not stopped by this
toggle.

On Windows, `clawcrew desktop` launches the companion app in a new process
group without attaching a console, so closing the launching terminal does not
close or stop the desktop app.

On launch:

1. A small **splash** window (`apps/tauri/splash/index.html`) appears and polls
   the gateway's `/health` (via the `get_health` IPC command) every ~1.2s.
2. Once the gateway is healthy, the splash calls the `open_dashboard` command,
   which pairs with the gateway (when pairing is required), creates the **main**
   window pointed at the gateway **root** (`http://127.0.0.1:42617/`), seeds the
   bearer token via an initialization script, and closes the splash.
3. The web app's fresh-install redirect (`FreshInstallRedirect` in
   `web/src/App.tsx`) sends first-time users — no agents yet, Quickstart never
   completed — to `/quickstart`. Returning users land on the dashboard.

> The app looks for a gateway on `127.0.0.1:42617`: it reuses a running daemon, or starts the discovered kernel with `clawcrew service run-desktop-daemon --port 42617` (preferring a kernel bundled next to the app executable, then `PATH` and the common install dirs — see `src/daemon.rs::find_clawcrew_binary`). Before launch, Desktop verifies that the kernel accepts this supervisor command. An externally installed kernel must therefore support the Desktop supervisor command; an older unsupported kernel produces an actionable startup error instead of falling back to an uncaptured daemon. The self-contained installer below bundles the matching kernel as a Tauri sidecar — the "full experience" distribution from architecture RFC fnd-001, D5.
>
> **Run `clawcrew daemon`, not `clawcrew gateway start`.** Both serve the
> dashboard on 42617, but only the daemon attaches the supervisor that powers
> in-place reload. After the Quickstart applies config it calls `/admin/reload`;
> a standalone `gateway start` has no supervisor and returns
> `503 "no daemon supervisor — running as standalone gateway"`, so the new agent
> won't go live until the process is restarted. The daemon hot-reloads instead.

The desktop supervisor writes combined daemon stdout and stderr to `<config-dir>/logs/clawcrew-desktop-daemon.log`, where `<config-dir>` follows canonical config resolution precedence: `CLAWCREW_CONFIG_DIR`, then `CLAWCREW_DATA_DIR`, then deprecated `CLAWCREW_WORKSPACE`, then Homebrew/default resolution. The capture is capped at 8 MiB and retains the newest tail when the cap is crossed.

## Self-contained build (bundled kernel)

The plain `cargo tauri build` produces an app that *finds* an installed
`clawcrew`. To produce the zero-install artifact — double-click on a machine
with nothing pre-installed and get a running agent — bundle the kernel as a
sidecar:

```sh
# 1. Build the dashboard, then embed it in the staged kernel.
cargo web build
scripts/desktop/prepare-kernel.sh --features embedded-web
scripts/desktop/prepare-kernel.sh --target universal-apple-darwin --features embedded-web

# 2. Bundle with the sidecar overlay (adds bundle.externalBin).
cd apps/tauri && cargo tauri build --config tauri.bundled.conf.json
```

`CLAWCREW_KERNEL_PATH` can reuse a prebuilt single-target kernel, but that
binary must already have been built with `--features embedded-web`; the staging
script cannot add embedded assets to an existing executable.

The overlay keeps the default config untouched, so `cargo tauri build`
without the staged kernel keeps working. Tauri places the sidecar next to the
app executable as `clawcrew`, which is the first place
`find_clawcrew_binary()` looks — so the bundled app starts its own daemon
from its own kernel.

To verify self-containment, launch on a machine (or shell) where `clawcrew`
is not on `PATH` and not in `~/.cargo/bin`, then check the daemon's process
path points inside the app bundle:

```sh
pgrep -fl 'clawcrew daemon'   # expect .../ClawCrew.app/Contents/MacOS/clawcrew
```

> Size note: the kernel dominates the artifact. A stripped release kernel is
> ~146 MB per arch (~55–65 MB compressed dmg); a universal (two-slice) kernel
> roughly doubles that. The unstripped dev kernel is ~228 MB — always let
> `prepare-kernel.sh` strip it.

## macOS (current target)

### Reset to fresh-install state
```sh
pkill -f 'target/debug/clawcrew-desktop'
rm "$HOME/Library/Application Support/ai.clawcrewlabs.desktop/settings.json"
killall Dock                                   # if dock icon looks stale
bash dev/run-tauri-dev.sh
```

To exercise the full first-run path, also reset the gateway's config so the
Quickstart auto-launches (the gateway reports `quickstart_completed=false` and
an empty agents list via `GET /api/quickstart/state`).

For a real installed-bundle test:
```sh
cd apps/tauri && cargo tauri build
cp -R target/release/bundle/macos/ClawCrew.app /Applications/
xattr -dr com.apple.quarantine /Applications/ClawCrew.app
open /Applications/ClawCrew.app
```

### What to verify
- With **no gateway running**: splash shows "Connecting to your ClawCrew
  gateway…" and, after a few seconds, the "make sure the gateway is running"
  hint. The tray icon shows Disconnected.
- Start the daemon (`cargo run -p clawcrew -- daemon`, or `clawcrew daemon`):
  within ~1–2s the splash hands off — the dashboard window opens, splash closes.
- **First run** (fresh gateway config): the dashboard opens straight onto the
  **Quickstart**; completing it configures an agent and the gateway becomes
  usable. After completion, relaunching the app lands on the dashboard.
- **Returning run** (agent already configured): the dashboard opens on the
  normal dashboard, not the Quickstart.
- Quit from the tray → relaunch → splash → dashboard again (tray icon persists
  in the menu bar).
- Inspect `<config-dir>/logs/clawcrew-desktop-daemon.log` after startup to verify combined stdout/stderr capture; the file stays at or below 8 MiB and keeps the newest tail during continuous output.

### Native command boundary

The Rust app still registers `take_screenshot` and `run_applescript`, but the
gateway-served main window receives no remote Tauri capability and cannot invoke
them. Exposing either command requires a separate, narrowly scoped approval and
ACL design.

## Linux / Windows

The app builds and runs the same splash → gateway → Quickstart flow. Bundle
targets are unchanged (`.deb`/`.AppImage` on Linux, `.exe`/`.msi` on Windows).
Screen capture and AppleScript capabilities remain macOS-only; the other
platforms register stubs that return an unsupported-platform error.

### How to build
```sh
cd apps/tauri
cargo tauri build          # native build on each platform
# Or cross-compile with the appropriate target + toolchain:
#   cargo build --release --target x86_64-unknown-linux-gnu
#   cargo build --release --target x86_64-pc-windows-msvc
```

## CI matrix to add (separate issue)

```yaml
# Suggested when #6501 lands — run all three at minimum on cargo check
matrix:
  os: [macos-14, ubuntu-22.04, windows-2022]
```

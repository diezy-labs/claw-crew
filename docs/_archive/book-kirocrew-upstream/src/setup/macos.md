# macOS

Install, update, run as a LaunchAgent, and uninstall on macOS (Intel or Apple Silicon).

## Install

```sh
./install.sh
```

That is the whole install. Run it from a clone, or pipe it from `curl`:

<div class="os-tabs-src">

#### sh

<!-- >>> generated:unix-fast-command by `cargo generate installers` - do not edit <<< -->
```sh
curl -fsSL https://raw.githubusercontent.com/clawcrew-labs/clawcrew/master/install.sh | sh
```
<!-- >>> end generated:unix-fast-command <<< -->

</div>

The [canonical installation paths](../getting-started/quickstart.md#install) explain the fast and guided routes, source fallback, app selection, PATH handoff, and the next Quickstart step.

### Homebrew

<div class="os-tabs-src">

#### sh

```sh
brew install clawcrew
```

</div>

Gets you `brew services` integration. Binary lives at `$HOMEBREW_PREFIX/bin/clawcrew`.

**Workspace location gotcha:** with Homebrew, the service user and the CLI user may be different, so the workspace lives at `$HOMEBREW_PREFIX/var/clawcrew/` rather than `~/.clawcrew/`. Point CLI invocations at the same workspace:

<div class="os-tabs-src">

#### sh

```sh
export CLAWCREW_WORKSPACE="$HOMEBREW_PREFIX/var/clawcrew"
```

</div>

Add that to your shell profile if you want it permanent.

## System dependencies

Most features work with a stock macOS install. Optional extras:

| Feature | Install |
|---|---|
| Docs translation | `brew install gettext` |
| Browser tool | Playwright pulls Chromium automatically on first use |
| Hardware | No native GPIO on macOS; use a USB-attached board. See [Hardware](../hardware/index.md) |
| iMessage channel | Requires macOS 11+. See [Channels → Other chat platforms](../channels/chat-others.md) |

## Running as a service

<div class="os-tabs-src">

#### sh

```sh
clawcrew service install   # writes ~/Library/LaunchAgents/com.clawcrew.daemon.plist
clawcrew service start
clawcrew service status
```

</div>

Logs go to `~/.clawcrew/logs/` (Homebrew installs: `$HOMEBREW_PREFIX/var/clawcrew/logs/`):

<div class="os-tabs-src">

#### sh

```sh
tail -f ~/.clawcrew/logs/daemon.stdout.log
```

</div>

For Homebrew installs, prefer:

<div class="os-tabs-src">

#### sh

```sh
brew services start clawcrew
brew services info clawcrew
```

</div>

Both methods produce the same end state, a loaded LaunchAgent that starts on login. Pick one and stick with it.

Full details: [Service management](./service.md).

## Update

Re-run the installer, it detects the existing install and upgrades in place:

<div class="os-tabs-src">

#### sh

```sh
curl -fsSL https://raw.githubusercontent.com/clawcrew-labs/clawcrew/master/install.sh | sh -s -- --skip-quickstart
clawcrew service restart
```

</div>

Or from a clone:

<div class="os-tabs-src">

#### sh

```sh
cd /path/to/clawcrew
git pull
./install.sh --skip-quickstart
clawcrew service restart
```

</div>

If installed via Homebrew instead:

<div class="os-tabs-src">

#### sh

```sh
brew update && brew upgrade clawcrew
brew services restart clawcrew
```

</div>

## Uninstall

<div class="os-tabs-src">

#### sh

```sh
# stop and unregister the service
clawcrew service stop
clawcrew service uninstall

# Homebrew
brew uninstall clawcrew

# bootstrap / cargo
rm ~/.cargo/bin/clawcrew
```

</div>

Remove config and workspace (optional: this deletes conversation history):

<div class="os-tabs-src">

#### sh

```sh
# Homebrew workspace
rm -rf "$HOMEBREW_PREFIX/var/clawcrew"

# Default workspace (includes logs at ~/.clawcrew/logs)
rm -rf ~/.clawcrew ~/.config/clawcrew
```

</div>

## Gotchas

- **Homebrew config path mismatch.** The `brew services` daemon reads config from `$HOMEBREW_PREFIX/var/clawcrew/`, not `~/.clawcrew/`. If your service is reading stale config, check which one the daemon sees and set `CLAWCREW_WORKSPACE` accordingly.
- **First launch of the browser tool** downloads Chromium (~150 MB) via Playwright.
- **Apple Silicon and Intel:** the bootstrap script detects the architecture and uses a matching prebuilt release artifact when one is available. If the release has no matching artifact, it falls back to a source build. Homebrew selects the appropriate package for the host.

## Next

- [Service management](./service.md)
- [Quickstart](../getting-started/quickstart.md)
- [Operations → Overview](../ops/overview.md)

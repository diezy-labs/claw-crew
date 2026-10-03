# Service Management

ClawCrew ships with first-class service integration for systemd (Linux), launchctl (macOS), and Task Scheduler (Windows). All three are driven by one CLI surface:

<div class="os-tabs-src">

#### sh

```sh
clawcrew service install     # register the service
clawcrew service start       # start it
clawcrew service stop        # stop it
clawcrew service restart     # stop + start
clawcrew service status      # running / stopped, last exit code
clawcrew service uninstall   # remove it
```

</div>

The platform-specific backends are implemented in `crates/clawcrew-runtime/src/service/`. You don't have to think about them, but knowing what they produce helps when debugging.

## Linux: systemd

`clawcrew service install` writes a user-scoped unit at `~/.config/systemd/user/clawcrew.service`.

The unit:

- `Type=simple` with the agent process staying in the foreground
- `ExecStart={cargo-bin}/clawcrew daemon`
- `Restart=always` with `RestartSec=3`
- `Environment=HOME=%h` and `PassEnvironment=DISPLAY XDG_RUNTIME_DIR` so headless browser tools can create profile/cache dirs and reach the user session
- `WantedBy=default.target`

### Manual control (systemd)

<div class="os-tabs-src">

#### sh

```sh
systemctl --user start clawcrew
systemctl --user stop clawcrew
systemctl --user status clawcrew
systemctl --user enable clawcrew     # start on login
```

</div>

### Logs

<div class="os-tabs-src">

#### sh

```sh
journalctl --user -u clawcrew -f        # follow
journalctl --user -u clawcrew --since "1h ago"
```

</div>

### Environment overrides (systemd)

Use a user-service override when the daemon needs environment variables that are not present in your interactive shell:

<div class="os-tabs-src">

#### sh

```sh
systemctl --user edit clawcrew.service
```

</div>

For example, a Bedrock profile that uses `credential_process` needs `AWS_PROFILE` in the service environment:

```ini
[Service]
Environment=AWS_PROFILE=clawcrew-bedrock
```

After saving the override, reload and restart the user service:

<div class="os-tabs-src">

#### sh

```sh
systemctl --user daemon-reload
systemctl --user restart clawcrew
journalctl --user -u clawcrew -f
```

</div>

The generated user service sets `HOME=%h`, so provider code that reads files under the service user's home directory can resolve paths such as `~/.aws/config`. If an override references an executable, use an absolute path; systemd services often run with a smaller `PATH` than an interactive shell.

### Starting before user login

The CLI only ever writes a user-scoped unit (`systemctl --user`), which by default starts at login and stops at logout. To keep ClawCrew running on a headless box without an active session, enable lingering for the service user:

<div class="os-tabs-src">

#### sh

```sh
sudo loginctl enable-linger $USER
systemctl --user enable --now clawcrew
```

</div>

If you need a true system-scope unit (root-owned, `/etc/systemd/system/`, dedicated service account, or hardware groups via `SupplementaryGroups`), the CLI does not generate one; adapt the system-level template at [`scripts/clawcrew.service`](https://github.com/clawcrew-labs/clawcrew/blob/master/scripts/clawcrew.service) and install it yourself. On OpenRC hosts, `sudo clawcrew service install` does provision a dedicated `clawcrew` user and system paths (see below).

## Linux: OpenRC

Detected automatically when `/run/openrc` exists (Alpine, some Gentoo configs).

<div class="os-tabs-src">

#### sh

```sh
clawcrew service install   # writes /etc/init.d/clawcrew
rc-service clawcrew start
rc-update add clawcrew default    # start on boot
```

</div>

OpenRC keeps daemon output in `/var/log/clawcrew/access.log` and
`/var/log/clawcrew/error.log`. Each file retains recent output within an 8 MiB
bound. Reinstall and restart the service after upgrading so the generated init
script uses bounded logger processes.

## macOS: LaunchAgent

`clawcrew service install` writes `~/Library/LaunchAgents/com.clawcrew.daemon.plist` and loads it.

<div class="os-tabs-src">

#### sh

```sh
launchctl list | grep clawcrew
launchctl unload ~/Library/LaunchAgents/com.clawcrew.daemon.plist
launchctl load ~/Library/LaunchAgents/com.clawcrew.daemon.plist
```

</div>

Logs go to `<config-dir>/logs/` as `daemon.stdout.log` and `daemon.stderr.log` (for a default install, `~/.clawcrew/logs/`). Homebrew installs write to `$HOMEBREW_PREFIX/var/clawcrew/logs/` instead. Each launchd capture file retains recent output within an 8 MiB bound. Reinstall and restart the service after upgrading so the generated LaunchAgent uses bounded capture. `clawcrew service logs` tails whichever of the two files hold output, so a daemon that only writes to stdout still shows up; `--follow` watches both, so a failure written to `daemon.stderr.log` after startup still reaches a running viewer. When more than one file is shown, `tail` labels each block with a `==> path <==` header.

### Homebrew-managed

If installed via Homebrew, `brew services` is the preferred interface:

<div class="os-tabs-src">

#### sh

```sh
brew services start clawcrew
brew services restart clawcrew
brew services info clawcrew
```

</div>

Don't mix `clawcrew service` CLI commands with `brew services`, pick one. Both end up writing a plist; having both around confuses `launchctl`.

## Windows: Task Scheduler

`clawcrew service install` creates a per-user scheduled task named **ClawCrew Daemon**:

- Trigger: at logon (`/SC ONLOGON`)
- Run level: `LIMITED` (runs as the current user, not elevated)
- Action: runs the install wrapper `clawcrew-daemon.cmd`, which launches `clawcrew daemon`

Verify in Task Scheduler GUI (`taskschd.msc`) under Task Scheduler Library → ClawCrew Daemon.

Logs go to `<config-dir>\logs\` as `daemon.stdout.log` and `daemon.stderr.log` (for a default install, `%USERPROFILE%\.clawcrew\logs\`). `clawcrew service logs` prints whichever of the two files hold output, and `--follow` shows the others first and then streams `daemon.stdout.log`, or `daemon.stderr.log` when only that file holds output, because `Get-Content -Wait` tracks a single path. To read one directly:

<div class="os-tabs-src">

#### cmd

```cmd
type %USERPROFILE%\.clawcrew\logs\daemon.stdout.log
```

</div>

### Manual control (Task Scheduler)

The task is driven through `clawcrew service start|stop|status`, which wrap `schtasks /Run`, `/End`, and `/Query` against the **ClawCrew Daemon** task. You can also manage it directly:

<div class="os-tabs-src">

#### cmd

```cmd
schtasks /Run /TN "ClawCrew Daemon"
schtasks /End /TN "ClawCrew Daemon"
schtasks /Query /TN "ClawCrew Daemon" /FO LIST
```

</div>

The CLI installs only a per-user ONLOGON task; it does not register a `LocalSystem` Windows Service. For a true system service, wrap the binary with a third-party supervisor (e.g. NSSM) yourself.

## Config path resolution

The service reads config from whichever directory resolved at install time. Precedence (first match wins):

1. `$CLAWCREW_CONFIG_DIR` (config lives directly under `$CLAWCREW_CONFIG_DIR`)
2. `$CLAWCREW_DATA_DIR`
3. `$CLAWCREW_WORKSPACE` (**deprecated**, prefer `CLAWCREW_DATA_DIR`; resolves either `$CLAWCREW_WORKSPACE` or the legacy sibling `.clawcrew/`)
4. On macOS only, the Homebrew config dir (`$HOMEBREW_PREFIX/var/clawcrew/`) when installed via Homebrew
5. Default `~/.clawcrew/` (Linux/macOS) or `%USERPROFILE%\.clawcrew\` (Windows)

`CLAWCREW_CONFIG_DIR` overrides everything; setting it alongside `CLAWCREW_DATA_DIR` or `CLAWCREW_WORKSPACE` logs a warning and ignores the others.

If your service seems to ignore config changes, check which path the daemon resolved against, `clawcrew status` reports the active config file, and the runtime logs a resolution-source line at startup:

<div class="os-tabs-src">

#### sh

```sh
clawcrew status
```

</div>

The output includes the config file path it resolved against.

## Auto-update

The service does **not** auto-update. That's deliberate; you pick when to take new code. Subscribe to the GitHub release feed or the Discord `#releases` channel (see [Contributing → Communication](../contributing/communication.md)).

## See also

- [Linux setup](./linux.md), [macOS setup](./macos.md), [Windows setup](./windows.md)
- [Operations → Logs & observability](../ops/observability.md)
- [Operations → Troubleshooting](../ops/troubleshooting.md)

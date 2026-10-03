# Operations: Overview

How to run ClawCrew in production. The surface is intentionally small: one
binary, one config file, and one install root with a handful of runtime stores.
Most "operations" is "systemd and journald".

This section covers:

- [Service & daemon](./service.md): keeping the process alive
- [Logs & observability](./observability.md): reading what the agent did
- [Cost tracking](./cost-tracking.md): token spend and per-model cost
- [Troubleshooting](./troubleshooting.md): when things break
- [Network deployment](./network-deployment.md): exposing the gateway, tunnels, reverse proxies

## The shape of a deployment

A typical always-on ClawCrew install is:

{{#include ../_snippets/deployment-shape.md}}

Everything except the binary can move. The data dir defaults to
`~/.clawcrew/data/` (the legacy `~/.clawcrew/workspace/` name is still
accepted); config paths resolve per environment (Homebrew vs. bootstrap vs.
XDG), and log destinations are platform-native by default. For the full store
map, see [Runtime state and persistence](../architecture/runtime-state-and-persistence.md).

## What to monitor

Four signals matter:

### 1. Service liveness

Is the process running?

<div class="os-tabs-src">

#### Linux

```sh
systemctl --user is-active clawcrew
```

#### macOS

```sh
launchctl list | grep -c com.clawcrew.daemon
```

#### Windows

```cmd
schtasks /Query /TN "ClawCrew Daemon" /FO LIST | findstr Status
```

</div>

If it's dying repeatedly, check [Troubleshooting → Daemon keeps restarting](./troubleshooting.md).

### 2. Channel and component health

The gateway exposes a public component liveness snapshot at `/health`; detailed component errors remain available through the authenticated diagnostic routes `/api/health` and `/api/status`. Channels, providers, and other long-running components register themselves in the `components` map as they start, report OK, or error.

<div class="os-tabs-src">

#### sh

```sh
curl -s http://localhost:42617/health | jq
```

</div>

```json
{
  "status": "ok",
  "paired": true,
  "require_pairing": true,
  "runtime": {
    "pid": 4821,
    "updated_at": "2026-06-08T09:00:00+00:00",
    "uptime_seconds": 3600,
    "components": {
      "channel:telegram": {"status": "ok", "updated_at": "…", "last_ok": "…", "restart_count": 0},
      "channel:matrix":   {"status": "error", "updated_at": "…", "last_ok": "…", "restart_count": 3}
    }
  }
}
```

Each component in the public `/health` response carries `status` (`starting` / `ok` / `error`), `updated_at`, `last_ok`, and `restart_count`. Watch for `status: "error"` and climbing `restart_count`. The public response omits `last_error`; use `/api/health` or `/api/status` with the configured API authentication to inspect detailed component errors.

A channel reads `starting` with a null `last_ok` until it confirms it can actually reach its service, not merely until its listener starts. Some channels report what they observed while talking to the service, so a listener that is running but has never completed an exchange stays `starting` rather than `ok`, and one whose calls are failing reads `error`. A channel restarting under an alias that previously reported `ok` returns to `starting` until it produces its own successful exchange. Channels that offer no such signal are marked `ok` for as long as their listener runs.

### 3. Provider reliability

Providers surface as components in the same `/health` snapshot. For request-level signal (latency, success rate, token counts), scrape `/metrics` (see below) and read `clawcrew_llm_requests_total` and `clawcrew_request_latency_seconds`.

### 4. Tool-call volume and metrics

`/metrics` returns Prometheus text exposition. It requires `[observability] backend = "prometheus"` in config; without it the endpoint returns a one-line "backend not enabled" hint.

<div class="os-tabs-src">

#### sh

```sh
curl -s http://localhost:42617/metrics
```

</div>

```
clawcrew_tool_calls_total{success="true",tool="shell"} 342
clawcrew_tool_calls_total{success="false",tool="shell"} 6
clawcrew_tool_calls_total{success="true",tool="file_write"} 89
```

The `clawcrew_tool_calls_total` counter is labelled by `tool` and `success` (`"true"`/`"false"`). A rising `success="false"` count for one tool is worth looking at: either a policy block, a misbehaving agent, or a flaky tool. Other useful series include `clawcrew_llm_requests_total`, `clawcrew_errors_total`, `clawcrew_active_sessions`, and `clawcrew_tokens_input_total` / `clawcrew_tokens_output_total`.

## Capacity

A single ClawCrew instance can handle:

- Multiple concurrent conversations across all channels
- Tool calls at whatever rate the provider and sandbox allow
- Long-running agent loops (tool chains of 20+ calls)

Scale laterally by running one instance per workspace. Don't try to run two daemons on the same workspace: SQLite's single-writer model will produce lock contention and ultimately corruption.

For multi-tenant hosting, see the proposal in #2765 (closed, historical, the architecture for in-process multi-workspace routing).

## Backups

What to back up:

- `~/.clawcrew/data/memory/*.db`: SQLite conversation memory (`brain.db`, plus `audit.db`)
- `~/.clawcrew/data/sessions/`: persisted session state
- `~/.clawcrew/.secret_key`: master key for the encrypted secrets store (if used). **Without it, the config's encrypted secrets are unrecoverable.**

A plain `tar czf clawcrew-$(date +%F).tar.gz ~/.clawcrew` covers everything. Restic, borg, or Duplicacy work fine for incremental backups.

`~/.clawcrew/data/memory/response_cache.db` is a regenerable LLM response cache; it's safe to include in a full-directory backup or to exclude to save space. Tool receipts are in-band HMAC tokens in the conversation history (see [Tool receipts](../security/tool-receipts.md)), not an on-disk log, so there is nothing separate to back up for them.

## Updates

The service does not auto-update. Subscribe to the release feed (GitHub releases or the Discord `#releases` channel: see [Contributing → Communication](../contributing/communication.md)). Typical update cadence:

1. Read the release notes
2. Back up `~/.clawcrew/`
3. Update the binary (`brew upgrade`, bootstrap re-run, or `cargo install --force`)
4. `clawcrew service restart`
5. Verify the `/health` endpoint reports `status: "ok"` with no component in `error`

If the new version requires config migrations, the startup log emits a warning and the binary usually auto-migrates. Check `clawcrew config list` to spot-check values after upgrade, and `clawcrew config migrate` to apply any pending schema migrations manually.

## See also

- [Setup → Service management](../setup/service.md): install/remove/logs per platform
- [Logs & observability](./observability.md)
- [Troubleshooting](./troubleshooting.md)
- [Network deployment](./network-deployment.md)

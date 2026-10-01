// RF-A6 pure-move: CLI argument definitions extracted verbatim from src/main.rs.
// Pure clap derive data (Cli/Commands/*Commands enums) - no logic beyond this.
// Visibility raised to pub(crate) on top-level items only; bodies untouched.
use super::*;

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CompletionShell {
    #[value(name = "bash")]
    Bash,
    #[value(name = "fish")]
    Fish,
    #[value(name = "zsh")]
    Zsh,
    #[value(name = "powershell")]
    PowerShell,
    #[value(name = "elvish")]
    Elvish,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum EstopLevelArg {
    #[value(name = "kill-all")]
    KillAll,
    #[value(name = "network-kill")]
    NetworkKill,
    #[value(name = "domain-block")]
    DomainBlock,
    #[value(name = "tool-freeze")]
    ToolFreeze,
}

/// `ClawCrew` - Zero overhead. Zero compromise. 100% Rust.
#[derive(Parser, Debug)]
#[command(name = "clawcrew")]
#[command(author = "theonlyhennygod")]
#[command(version)]
// i18n-exempt: clap derive help — framework requires a compile-time literal
#[command(about = "The fastest, smallest AI assistant.", long_about = None)]
pub(crate) struct Cli {
    #[arg(long, global = true)]
    config_dir: Option<String>,

    /// Lowest severity recorded to the runtime trace (and capture
    /// layer). Immutable for the process. Precedence: this flag >
    /// RUST_LOG env > per-command default.
    #[arg(long, global = true, value_enum)]
    log_level: Option<LogLevel>,

    /// Surface recorded logs on the terminal. Off by default: logs go
    /// to the trace file only and the terminal shows just command
    /// output. When on, the terminal shows events down to the recorded
    /// floor. Immutable for the process.
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Recording-floor severities, mapped to `RUST_LOG`-style directive
/// fragments. Mirrors `tracing`'s level names so the flag reads the
/// same as the env var it overrides.
#[derive(clap::ValueEnum, Debug, Clone, Copy)]
pub(crate) enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn as_directive(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

/// Subcommands for `clawcrew eval`.
#[cfg(feature = "agent-runtime")]
#[derive(Subcommand, Debug)]
pub(crate) enum EvalCommands {
    /// Run a suite of evaluation cases.
    Run {
        /// Directory of `*.json` trace fixtures (defaults to `evals/regression`).
        #[arg(long)]
        suite: Option<String>,

        /// Execution mode: `replay` (deterministic) or `live` (later phase).
        /// Defaults to config `[eval] mode`.
        #[arg(long)]
        mode: Option<String>,

        /// Output format.
        #[arg(long, value_enum, default_value = "table")]
        format: commands::eval::OutputFormat,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Quickstart — create one working agent end-to-end. Replaces the
    /// section-by-section onboarding flow with a single preset-driven
    /// path. Interactive: the flags below pre-seed checklist selectors
    /// but do not skip them; a terminal is required.
    Quickstart {
        /// Provider type (anthropic / openai / openrouter / ollama).
        #[arg(long)]
        model_provider: Option<String>,

        /// Model id for the new provider entry.
        #[arg(long)]
        model: Option<String>,

        /// API key for the new provider entry (omit for ollama / local).
        #[arg(long)]
        api_key: Option<String>,

        /// Alias for the new agent. Defaults to a sanitized provider name.
        #[arg(long)]
        agent: Option<String>,
    },

    /// Deprecated. Use `clawcrew quickstart`. Any flags error.
    Onboard {
        /// Configure a specific section only. Omit to run the full flow.
        #[command(subcommand)]
        section: Option<clawcrew_config::sections::Section>,

        /// Skip interactive prompts; read from --api-key/--model-provider/--model/--memory.
        #[arg(long, hide = true)]
        quick: bool,

        /// Force the dialoguer CLI backend instead of the default ratatui TUI.
        #[arg(long, hide = true)]
        cli: bool,

        /// Deprecated: TUI is now the default. Accepted as a no-op for one release.
        #[arg(long, hide = true)]
        tui: bool,

        /// Don't ask "keep stored secret?" — always re-prompt.
        #[arg(long, hide = true)]
        force: bool,

        /// Back up existing config and start from defaults.
        #[arg(long, hide = true)]
        reinit: bool,

        /// API key for model_provider configuration.
        #[arg(long, hide = true)]
        api_key: Option<String>,

        /// ModelProvider name. Used as the type key for the synthesized
        /// `[providers.models.<type>.default]` entry.
        #[arg(long, hide = true)]
        model_provider: Option<String>,

        /// Model ID override.
        #[arg(long, hide = true)]
        model: Option<String>,

        /// Memory backend (sqlite, lucid, markdown, none).
        #[arg(long, hide = true)]
        memory: Option<String>,

        // Deprecated legacy flags — parsed for one release, each maps to a
        // subcommand with a stderr warning pointing at the new form.
        #[arg(long, hide = true)]
        channels_only: bool,
        #[arg(long, hide = true)]
        providers_only: bool,
        #[arg(long, hide = true)]
        memory_only: bool,
        #[arg(long, hide = true)]
        hardware_only: bool,
        #[arg(long, hide = true)]
        tunnel_only: bool,
    },

    /// Start the AI agent loop
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Start the AI agent loop.

Launches an interactive chat session with the configured AI model_provider. \
Use --message for single-shot queries without entering interactive mode.

Examples:
  clawcrew agent -a assistant                                          # interactive session
  clawcrew agent -a assistant -m \"Summarize today's logs\"              # single message
  clawcrew agent -a assistant -p anthropic --model claude-sonnet-4-20250514
  clawcrew agent -a assistant --peripheral nucleo-f401re:/dev/ttyACM0")]
    Agent {
        /// Configured agent alias to run as (must match `[agents.<alias>]`).
        /// Required — there is no default agent.
        #[arg(short = 'a', long)]
        agent: String,

        /// Single message mode (don't enter interactive mode)
        #[arg(short, long)]
        message: Option<String>,

        /// Load and save interactive session state in this JSON file
        #[arg(long)]
        session_state_file: Option<PathBuf>,

        /// Model provider to use (openrouter, anthropic, openai, openai-codex)
        #[arg(short = 'p', long = "model-provider", alias = "provider")]
        model_provider: Option<String>,

        /// Model to use
        #[arg(long)]
        model: Option<String>,

        /// Temperature (0.0 - 2.0, defaults to `providers.models.<type>.<alias>.temperature`)
        #[arg(short, long, value_parser = parse_temperature)]
        temperature: Option<f64>,

        /// Attach a peripheral (board:path, e.g. nucleo-f401re:/dev/ttyACM0)
        #[arg(long)]
        peripheral: Vec<String>,
    },

    /// Start/manage the gateway server (webhooks, websockets)
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Manage the gateway server (webhooks, websockets).

Start, restart, or inspect the HTTP/WebSocket gateway that accepts \
incoming webhook events and WebSocket connections.

Examples:
  clawcrew gateway start              # start gateway
  clawcrew gateway restart            # restart gateway
  clawcrew gateway get-paircode       # show pairing code")]
    Gateway {
        #[command(subcommand)]
        gateway_command: Option<clawcrew::GatewayCommands>,
    },

    /// Start ACP (Agent Control Protocol) server over stdio
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Start the ACP server (JSON-RPC 2.0 over stdio).

Launches a JSON-RPC 2.0 server on stdin/stdout for IDE and tool \
integration. Supports session management and streaming agent \
responses as notifications.

Methods: initialize, session/new, session/prompt, session/stop.

Examples:
  clawcrew acp                        # start ACP server
  clawcrew acp --agent fable         # default new sessions to agent fable
  clawcrew acp --max-sessions 5       # limit concurrent sessions")]
    Acp {
        /// Process-scoped default agent for alias-less session/new requests
        #[arg(long)]
        agent: Option<String>,

        /// Maximum concurrent sessions (default: 10)
        #[arg(long)]
        max_sessions: Option<usize>,

        /// Session inactivity timeout in seconds (default: 3600)
        #[arg(long)]
        session_timeout: Option<u64>,
    },

    /// Start long-running autonomous runtime (gateway + channels + heartbeat + scheduler)
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Start the long-running autonomous daemon.

Launches the full ClawCrew runtime: gateway server, all configured \
channels (Telegram, Discord, Slack, etc.), heartbeat monitor, and \
the cron scheduler. This is the recommended way to run ClawCrew in \
production or as an always-on assistant.

Use 'clawcrew service install' to register the daemon as an OS \
service (systemd/launchd) for auto-start on boot.

Examples:
  clawcrew daemon                   # use config defaults
  clawcrew daemon -p 9090           # gateway on port 9090
  clawcrew daemon --host 127.0.0.1  # localhost only")]
    Daemon {
        /// Port to listen on (use 0 for random available port); defaults to config gateway.port
        #[arg(short, long)]
        port: Option<u16>,

        /// Host to bind to; defaults to config gateway.host
        #[arg(long)]
        host: Option<String>,

        /// Self-terminate after all socket clients disconnect (with grace period)
        #[arg(long)]
        ephemeral: bool,

        /// Boot even when security-critical config sections were dropped to
        /// their defaults during load. Without this, the daemon refuses to
        /// start with a weakened posture; with it, the daemon boots so the
        /// operator can reach repair surfaces, emitting a repeating warning.
        #[arg(long)]
        allow_degraded_security: bool,
    },

    /// Manage OS service lifecycle (launchd/systemd user service)
    Service {
        /// Init system to use: auto (detect), systemd, or openrc
        #[arg(long, default_value = "auto", value_parser = ["auto", "systemd", "openrc"])]
        service_init: String,

        #[command(subcommand)]
        service_command: ServiceCommands,
    },

    /// Run diagnostics for daemon/scheduler/channel freshness
    Doctor {
        #[command(subcommand)]
        doctor_command: Option<DoctorCommands>,
    },

    /// Show system status (full details)
    Status {
        /// Output format: "exit-code" exits 0 if healthy, 1 otherwise (for Docker HEALTHCHECK)
        #[arg(long)]
        format: Option<String>,
    },

    /// Inspect the active security posture derived from local config and host detection
    #[cfg(feature = "agent-runtime")]
    Security {
        #[command(subcommand)]
        security_command: SecurityCommands,
    },

    Estop {
        #[command(subcommand)]
        estop_command: Option<EstopSubcommands>,

        /// Level used when engaging estop from `clawcrew estop`.
        #[arg(long, value_enum)]
        level: Option<EstopLevelArg>,

        /// Domain pattern(s) for `domain-block` (repeatable).
        #[arg(long = "domain")]
        domains: Vec<String>,

        /// Tool name(s) for `tool-freeze` (repeatable).
        #[arg(long = "tool")]
        tools: Vec<String>,
    },

    /// Configure and manage scheduled tasks
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Configure and manage scheduled tasks.

Schedule recurring, one-shot, or interval-based tasks using cron \
expressions, RFC3339 timestamps with explicit Z or offsets, durations, \
or fixed intervals.

Cron expressions use the standard 5-field format: \
'min hour day month weekday'. When --tz is omitted, cron schedules use \
the runtime local timezone. For user-facing schedules, pass --tz with \
an explicit IANA timezone.

Examples:
  clawcrew cron list
  clawcrew cron add '0 9 * * 1-5' 'Good morning' --agent sentinel --prompt --tz America/New_York
  clawcrew cron add '*/30 * * * *' 'Check system health' --agent sentinel --prompt
  clawcrew cron add '*/5 * * * *' 'echo ok' --agent sentinel
  clawcrew cron add-at 2099-01-15T14:00:00Z 'Send reminder' --agent sentinel --prompt
  clawcrew cron add-every 60000 'Ping heartbeat' --agent sentinel --prompt
  clawcrew cron once 30m 'Run backup in 30 minutes' --agent sentinel --prompt
  clawcrew cron pause TASK_ID
  clawcrew cron update TASK_ID --expression '0 8 * * *' --tz Europe/London")]
    Cron {
        #[command(subcommand)]
        cron_command: CronCommands,
    },

    /// Manage model_provider model catalogs
    Models {
        #[command(subcommand)]
        model_command: ModelCommands,
    },

    Providers {
        #[command(subcommand)]
        providers_command: Option<ProvidersCommands>,
    },

    /// Manage channels (telegram, discord, slack)
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Manage communication channels.

Add, remove, list, send, and health-check channels that connect ClawCrew \
to messaging platforms. Supported channel types: telegram, discord, \
slack, whatsapp, matrix, imessage, email.

Examples:
  clawcrew channel list
  clawcrew channel doctor
  clawcrew channel add telegram '{\"bot_token\":\"...\",\"name\":\"my-bot\"}'
  clawcrew channel remove my-bot
  clawcrew channel bind-telegram clawcrew_user
  clawcrew channel send 'Alert!' --channel-id telegram --recipient 123456789")]
    Channel {
        #[command(subcommand)]
        channel_command: ChannelCommands,
    },

    /// Manage agent aliases (create/list/rename/delete). Distinct from `agent`,
    /// which runs an agent.
    Agents {
        #[command(subcommand)]
        agents_command: AgentsCommands,
    },

    /// Manage channel aliases (create/list/rename/delete)
    Channels {
        #[command(subcommand)]
        channels_command: ChannelsCommands,
    },

    /// Browse 50+ integrations
    Integrations {
        #[command(subcommand)]
        integration_command: IntegrationCommands,
    },

    /// Manage skills (user-defined capabilities)
    Skills {
        #[command(subcommand)]
        skill_command: SkillCommands,
    },

    /// Browse the shared workspace one directory at a time
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
List children of a directory under `<install>`/shared/. Paths are relative \
to the shared workspace root; `..` traversal that escapes the root is \
rejected. Used by the dashboard's skill-bundle directory picker and by \
operators who want to inspect what's installed.

Examples:
  clawcrew browse                  # list shared/ root
  clawcrew browse skills           # list shared/skills/
  clawcrew browse skills/coding    # list shared/skills/coding/")]
    Browse {
        /// Path relative to `<install>/shared/`. Empty = root.
        #[arg(default_value = "")]
        path: String,
    },

    /// Manage standard operating procedures (SOPs)
    Sop {
        #[command(subcommand)]
        sop_command: SopCommands,
    },

    /// Migrate data from other agent runtimes
    Migrate {
        #[command(subcommand)]
        migrate_command: MigrateCommands,
    },

    /// Manage model_provider subscription authentication profiles
    Auth {
        #[command(subcommand)]
        auth_command: AuthCommands,
    },

    /// Discover and introspect USB hardware
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Discover and introspect USB hardware.

Enumerate connected USB devices, identify known development boards \
(STM32 Nucleo, Arduino, ESP32), and retrieve chip information via \
probe-rs / ST-Link.

Examples:
  clawcrew hardware discover
  clawcrew hardware introspect /dev/ttyACM0
  clawcrew hardware info --chip STM32F401RETx")]
    Hardware {
        #[command(subcommand)]
        hardware_command: clawcrew::HardwareCommands,
    },

    /// Manage hardware peripherals (STM32, RPi GPIO, etc.)
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Manage hardware peripherals.

Add, list, flash, and configure hardware boards that expose tools \
to the agent (GPIO, sensors, actuators). Supported boards: \
nucleo-f401re, rpi-gpio, esp32, arduino-uno.

Examples:
  clawcrew peripheral list
  clawcrew peripheral add nucleo-f401re /dev/ttyACM0
  clawcrew peripheral add rpi-gpio native
  clawcrew peripheral flash --port /dev/cu.usbmodem12345
  clawcrew peripheral flash-nucleo")]
    Peripheral {
        #[command(subcommand)]
        peripheral_command: clawcrew::PeripheralCommands,
    },

    /// Manage agent memory (list, get, stats, clear)
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Manage agent memory entries.

List, inspect, and clear memory entries stored by the agent. \
Supports filtering by category and session, pagination, and \
batch clearing with confirmation.

Examples:
  clawcrew memory stats
  clawcrew memory list
  clawcrew memory list --category core --limit 10
  clawcrew memory get KEY
  clawcrew memory clear --category conversation --yes")]
    Memory {
        #[command(subcommand)]
        memory_command: MemoryCommands,
    },

    /// Backup and Restore (P3.2)
    Backup {
        #[command(subcommand)]
        backup_command: BackupCommands,
    },

    /// Manage Ecosystem Apps (P3.3)
    App {
        #[command(subcommand)]
        app_command: AppCommands,
    },

    /// Manage configuration
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Manage ClawCrew configuration.

View, set, or initialize config properties by dotted path. \
Use 'schema' to dump the full JSON Schema for the config file.

Properties are addressed by dotted path (e.g. channels.matrix.mention-only).
Secret fields (API keys, tokens) automatically use masked input.
Enum fields offer interactive selection when value is omitted.

Examples:
  clawcrew config list                                  # list all properties
  clawcrew config list --secrets                        # list only secrets
  clawcrew config list --filter channels.matrix         # filter by prefix
  clawcrew config get channels.matrix.mention-only      # get a value
  clawcrew config set channels.matrix.mention-only true # set a value
  clawcrew config set channels.matrix.access-token      # secret: masked input
  clawcrew config set channels.matrix.stream-mode       # enum: interactive select
  clawcrew config init channels.matrix                  # init section with defaults
  clawcrew config init risk_profiles.strict             # create a new dynamic-map alias
  clawcrew config schema                                # print JSON Schema to stdout
  clawcrew config schema > schema.json

Property path tab completion is included automatically in `clawcrew completions <shell>`.")]
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
    },

    /// Check for and apply updates
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Check for and apply ClawCrew updates.

By default, downloads and installs the latest release with a \
6-phase pipeline: preflight, download, backup, validate, swap, \
and smoke test. Automatic rollback on failure.

Use --check to only check for updates without installing.
Use --force to skip the confirmation prompt.
Use --version to target a specific release instead of latest.

Examples:
  clawcrew update                      # download and install latest
  clawcrew update --check              # check only, don't install
  clawcrew update --force              # install without confirmation
  clawcrew update --version 0.6.0      # install specific version")]
    Update {
        /// Only check for updates, don't install
        #[arg(long)]
        check: bool,
        /// Install even if the target is not newer (reinstall or downgrade/pin to --version)
        #[arg(long)]
        force: bool,
        /// Target version (default: latest)
        #[arg(long)]
        version: Option<String>,
        /// With --check, emit machine-readable JSON instead of human text
        #[arg(long)]
        json: bool,
    },

    /// Run diagnostic self-tests
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Run diagnostic self-tests to verify the ClawCrew installation.

By default, runs the full test suite including network checks \
(gateway health, memory round-trip). Use --quick to skip network \
checks for faster offline validation.

Examples:
  clawcrew self-test             # full suite
  clawcrew self-test --quick     # quick checks only (no network)")]
    SelfTest {
        /// Run quick checks only (no network)
        #[arg(long)]
        quick: bool,
    },

    #[cfg(feature = "agent-runtime")]
    /// Run the agent evaluation harness
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Run the agent evaluation harness.

Phase 0 supports deterministic replay: every `*.json` trace fixture in the suite \
directory is replayed through the real agent loop and graded against its declarative \
expectations. No network calls, fully deterministic. Exits non-zero if any case fails, \
so it can gate CI.

Examples:
  clawcrew eval run                                  # replay ./evals/regression
  clawcrew eval run --suite evals/regression --format json")]
    Eval {
        #[command(subcommand)]
        eval_command: EvalCommands,
    },

    /// Generate shell completion script to stdout
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Generate shell completion scripts for `clawcrew`.

The script is printed to stdout so it can be sourced directly:

Examples (Unix shells):
  source <(clawcrew completions bash)
  clawcrew completions zsh > ~/.zfunc/_clawcrew
  clawcrew completions fish > ~/.config/fish/completions/clawcrew.fish

Examples (Windows PowerShell):
  clawcrew completions powershell | Out-String | Invoke-Expression
  clawcrew completions powershell > $PROFILE.CurrentUserAllHosts")]
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: CompletionShell,
    },

    /// Print the full CLI reference as Markdown (used by the docs pipeline).
    #[command(hide = true)]
    MarkdownHelp,

    /// Print the config JSON Schema (used by the docs pipeline).
    #[command(hide = true)]
    MarkdownSchema,

    /// Launch the companion desktop app, or open its download page
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Launch the ClawCrew companion desktop app.

The companion app is a lightweight menu bar / system tray application \
that connects to the same gateway as the CLI. It provides quick access \
to the dashboard, status monitoring, and device pairing.

Use --install to open the download page for your platform. It does not \
install anything itself.

Examples:
  clawcrew desktop              # launch the companion app
  clawcrew desktop --install    # open the download page")]
    Desktop {
        /// Open the companion app's download page
        #[arg(long)]
        install: bool,
    },

    /// Deprecated: use `clawcrew config` instead
    #[command(hide = true)]
    Props {
        #[command(subcommand)]
        props_command: DeprecatedPropsCommands,
    },

    /// Manage WASM plugins
    #[cfg(feature = "plugins-wasm")]
    Plugin {
        #[command(subcommand)]
        plugin_command: PluginCommands,
    },

    /// Fetch translated locale files (FTL) from upstream
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    #[command(long_about = "\
Fetch translated Fluent (.ftl) catalogues for a locale from the upstream \
repository and install them under `<config-dir>/data/ftl/<locale>/`, where the \
runtime and zerocode loaders read them.

Pass a single locale. By default every catalogue is fetched; restrict with \
--catalog (comma-separated): cli, tools, zerocode.

Examples:
  clawcrew locales fetch ja
  clawcrew locales fetch fr --catalog cli,tools
  clawcrew locales fetch zh-CN --catalog zerocode")]
    Locales {
        #[command(subcommand)]
        locales_command: LocalesCommands,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum LocalesCommands {
    // i18n-exempt: clap derive help — framework requires a compile-time literal
    /// Download translated FTL files for a locale from upstream
    Fetch {
        /// Locale code to fetch (e.g. `ja`, `fr`, `zh-CN`).
        locale: String,
        /// Comma-separated catalogues to fetch: cli, tools, zerocode.
        /// Omit to fetch all of them.
        #[arg(long)]
        catalog: Option<String>,
    },
}

/// Stub enum that mirrors the old `props` subcommands so clap can still parse
/// `clawcrew props <anything>` and print a deprecation message.
#[derive(Subcommand, Debug)]
pub(crate) enum DeprecatedPropsCommands {
    #[command(external_subcommand)]
    Any(Vec<String>),
}

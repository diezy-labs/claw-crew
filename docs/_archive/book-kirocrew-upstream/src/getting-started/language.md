# Language & translations

ClawCrew's interface strings (CLI messages, command help, and the `zerocode`
TUI) can be shown in languages other than English. English is always built in;
other languages are downloaded on demand.

## Set your language

ClawCrew reads a top-level `locale` key from your config. Set it to a locale
code such as `ja`, `fr`, or `zh-CN`. If `locale` is unset, ClawCrew uses your
operating system's language and falls back to English when no translation is
available.

## Fetch your language files

English ships inside the binary. For any other language you fetch the
translated files once:

<div class="os-tabs-src">

#### sh

```sh
clawcrew locales fetch ja
```

</div>

This downloads the Japanese translation files from the ClawCrew project and
installs them under `~/.clawcrew/data/ftl/ja/`, where ClawCrew looks for them
at startup. Restart ClawCrew (and `zerocode`) afterward to pick them up.

Fetch any locale the same way:

<div class="os-tabs-src">

#### sh

```sh
clawcrew locales fetch fr        # French
clawcrew locales fetch zh-CN     # Simplified Chinese
```

</div>

### Fetching only part of a language

By default `fetch` downloads every catalogue for the locale. To download only
some, pass `--catalog` with a comma-separated list:

| Catalog | Covers |
|---|---|
| `cli` | CLI messages and command help |
| `tools` | Built-in tool descriptions |
| `zerocode` | The `zerocode` terminal UI |

<div class="os-tabs-src">

#### sh

```sh
clawcrew locales fetch ja --catalog cli            # just CLI strings
clawcrew locales fetch ja --catalog cli,zerocode   # CLI + the TUI
```

</div>

If a catalogue has not been translated for your language yet, `fetch` skips it
and tells you: the catalogues that do exist are still installed.

## Where the files live

| Path | What |
|---|---|
| `~/.clawcrew/data/ftl/<locale>/cli.ftl` | CLI message translations |
| `~/.clawcrew/data/ftl/<locale>/tools.ftl` | Tool description translations |
| `~/.clawcrew/data/ftl/<locale>/zerocode.ftl` | `zerocode` TUI translations |

If you run ClawCrew with a custom config directory (`--config-dir` or
`CLAWCREW_CONFIG_DIR`), the files install under that directory's `data/ftl/`
instead.

## Troubleshooting

- **Still seeing English after fetching.** Confirm `locale` in your config
  matches the locale you fetched, and restart the process. ClawCrew loads
  language files at startup.
- **`fetch` reports a catalogue was skipped.** That catalogue has not been
  translated for your locale yet. The available catalogues are still installed;
  untranslated strings fall back to English.
- **A specific string is in English even though the rest is translated.** That
  individual string has no translation yet and falls back to English by design.

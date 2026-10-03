# API Reference

Full rustdoc for every public type in the workspace, auto-generated from the `///` comments on each type, function, and module. Use this when you need to know the exact shape of a struct, the methods on a trait, or what a function returns: anything the generated reference exposes better than prose can.

**[Open the rustdoc →](/api/clawcrew/index.html)**

## How to navigate it

- The rustdoc index lists every crate in the workspace. Open `clawcrew-api` to browse the shared trait contracts and boundary types.
- Use rustdoc's search box, keyboard search, or **All Items** to find a public name. Browser find searches only the current rendered page.
- Click a trait to see its methods and known implementors.

## Contract boundaries

`clawcrew-api` owns shared boundary types and trait definitions. Concrete implementations live in the owning capability crate and wire through that surface's existing construction boundary. Consumers depend on the applicable traits rather than concrete types.

The trait definitions are the source of truth for exact methods, defaults, and types. The generated rustdoc is the navigable reference built from them. Use [Architecture → Overview](./architecture/overview.md#core-traits) for the curated extension families and [ADR-002](./architecture/decisions/ADR-002-trait-driven-extensibility.md) for the accepted trait-driven boundary decision.

## Crate index

| Crate | What it exposes |
| --- | --- |
| [`clawcrew`](/api/clawcrew/index.html) | Top-level umbrella with re-exports |
| [`clawcrew-api`](/api/clawcrew_api/index.html) | Shared boundary types and first-party extension traits |
| [`clawcrew-config`](/api/clawcrew_config/index.html) | Config schema, autonomy types, secrets |
| [`clawcrew-runtime`](/api/clawcrew_runtime/index.html) | Agent loop, security, SOP, onboarding |
| [`clawcrew-providers`](/api/clawcrew_providers/index.html) | Every LLM-provider implementation |
| [`clawcrew-channels`](/api/clawcrew_channels/index.html) | Messaging integrations |
| [`clawcrew-gateway`](/api/clawcrew_gateway/index.html) | HTTP/WebSocket gateway |
| [`clawcrew-tools`](/api/clawcrew_tools/index.html) | Agent-callable tools |
| [`clawcrew-memory`](/api/clawcrew_memory/index.html) | Conversation memory, embeddings |
| [`clawcrew-plugins`](/api/clawcrew_plugins/index.html) | WASM plugin host |
| [`clawcrew-hardware`](/api/clawcrew_hardware/index.html) | GPIO / I2C / SPI / USB |
| [`clawcrew-log`](/api/clawcrew_log/index.html) | Workspace logging and observability integration |
| [`clawcrew-infra`](/api/clawcrew_infra/index.html) | Channel session backends, debouncing, and stall watchdog |

See [Architecture → Crates](./architecture/crates.md) for a plain-English description of how the crates fit together.

## Regenerating the API reference

The rustdoc ships with every doc deploy. For local builds:

<div class="os-tabs-src">

#### sh

```sh
cargo mdbook refs     # generates CLI + config reference + rustdoc
cargo mdbook build    # rebuilds the full book including rustdoc bridge
```

</div>

See [Maintainers → Docs & Translations](./maintainers/docs-and-translations.md).

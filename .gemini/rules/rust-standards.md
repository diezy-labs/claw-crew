# Rust Standards & Global Steering

## Branding
- The project has rebranded from `zeroclaw` / `clawcrew` to `Galleon`.
- Any new crates, files, documents, or modules MUST use `galleon` instead of `clawcrew`. (e.g., `galleon-channel-discord` instead of `clawcrew-channel-discord`).

## Clean Architecture & Idioms
- Favor Plugin Architecture. Do not build mega-monoliths. Features like channel integrations must be separated into independent crates.
- Move `#[cfg(test)]` blocks to a dedicated `tests/` directory at the root of the crate (Integration Tests). Keep inline `#[test]` blocks only for tiny, private unit tests.
- Do not use `unwrap()` or `expect()` in production paths. Propagate errors using `Result` and `?`.
- Use standard Rust idioms (e.g., standard library traits like `From`, `TryFrom`, `Default`).

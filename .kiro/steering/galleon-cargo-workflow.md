---
title: Galleon — Cargo workflow (delta repo-spesifik)
inclusion: fileMatch
fileMatchPattern: "**/{*.rs,Cargo.toml,Cargo.lock}"
---

# Cargo workflow — galleon (delta)

Panduan umum build/test cepat Rust ada di steering GLOBAL `rust-build-speed.md` (inner loop, profile dev, linker, sccache/nextest, eksekusi aman). File ini HANYA memuat yang spesifik repo galleon — jangan duplikasi yang global.

## Spesifik galleon
- Workspace 25+ crate; crate terberat `clawcrew-runtime` (288k LOC) → `cargo check --workspace` 1–2 menit. Selalu `-p <crate>`.
- sccache + nextest SUDAH dikonfigurasi di repo ini: `.cargo/config.toml` (`rustc-wrapper="sccache"`) + `.config/nextest.toml` (profil `default`/`ci`). Verifikasi hit: `sccache --show-stats`.
- Pre-PR penuh: `./dev/ci.sh all`.
- Build fixture wasm plugin (`crates/clawcrew-plugins/tests/fixtures/*`) butuh target `wasm32-wasip2`; test e2e gated `--features plugins-wasm-cranelift` dan menjalankan nested `cargo build` (puluhan detik — wajar, bukan hang).
- Branding: crate/modul BARU pakai `galleon-`, bukan `clawcrew-`.

Selebihnya ikuti `rust-build-speed.md`.

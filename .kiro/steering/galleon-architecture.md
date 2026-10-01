---
title: Galleon Fleet — architecture & layering
inclusion: always
---

# Galleon Fleet — project rules

Workspace steering untuk repo `galleon-fleet` (monorepo 3-tier: Tauri shell + Rust core + Go engine + React UI). Sumber kanonikal: `AGENTS.md`, `.gemini/steering.md`, `.gemini/architecture.md`, `docs/book/src/contributing/architecture-map.md`.

## Branding
- Project di-rebrand dari `zeroclaw`/`clawcrew` → **Galleon**. Crate/file/modul/dokumen BARU wajib pakai `galleon`, bukan `clawcrew` (mis. `galleon-channel-discord`). Kode lama `clawcrew-*` yang sudah ada tidak diubah massal kecuali diminta.

## 3-tier layering (STRICT — jaga boundary)
| Tier | Lokasi | Runtime | Tugas | Komunikasi |
|---|---|---|---|---|
| Desktop Shell | `apps/tauri-2/` | Rust (Tauri v2) | window, tray, dialog OS, packaging `.exe` | Tauri IPC (`invoke`) |
| System Core | `crates/` | Rust 2024 | security microkernel, sandbox, native tools, mDNS/A2A, channels, WASI plugins | gRPC `SystemGateway` :50052 |
| AI Orchestrator | `engine/` | Go 1.25+ | agent brain, fleet governance, vector memory RAG, persistensi | gRPC `AgentEngine` :50051 + HTTP :9090 |
| Interface / UI | `web-2/` | TS, React 19 | rendering, view-model, `apiClient.ts`. **Zero backend logic.** | HTTP reverse-proxy ke Go :9090 / Tauri IPC |

Aturan turunan:
- **UI tidak boleh berisi logika bisnis** — hanya memanggil engine/IPC. Logika di `web-2/server.ts` hanya gateway/host-telemetry lokal (network, Ollama, proses), bukan domain fleet.
- Fitur lintas-crate dipisah jadi crate independen (plugin architecture); jangan monolit.

## Single Source Of Truth (dari AGENTS.md — gate review, bukan gaya)
- Sebelum menambah field/config/schema/cache/lookup paralel, identifikasi sumber kanonikal. Jika fakta sudah ada, resolve dari sana saat dipakai — jangan snapshot/duplikat.
- **Jangan duplikasi fakta yang sama di dua tier.** Contoh pelanggaran nyata di repo: `chat_quartermaster`/risk-tier policies didefinisikan ganda di `apps/tauri-2/src/main.rs` (Rust) DAN `engine/src/fleet/services.go` (Go) dengan nilai berbeda. Engine HTTP = sumber tunggal; Tauri command cukup proxy.
- Dokumen arsitektur yang mengklaim perilaku (mis. "persistensi via DiskStore") harus cocok dengan kode yang benar-benar di-wire. Jangan klaim fitur yang belum diimplementasi.

## Alur kerja
- Perubahan non-trivial: baca `docs/book/src/contributing/architecture-map.md` dulu untuk rute ke dokumen yang relevan.
- Branch non-`master`, PR ke `master`, jangan push langsung ke `master`. Conventional commits + PR template penuh. Tanpa footer bot/AI.
- Produksi: propagasikan error, hindari `unwrap()`/`expect()`. Jangan sembunyikan perubahan perilaku di dalam refactor. Dead code: hapus/connect/track, jangan `#[allow(dead_code)]`.
- Validasi sesuai surface: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, test; `./dev/ci.sh all` untuk pre-PR penuh.

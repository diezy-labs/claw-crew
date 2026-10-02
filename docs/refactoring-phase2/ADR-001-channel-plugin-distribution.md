# ADR-001 — Channel distribution: embed vs feature-crate vs WASI plugin

- **Status:** Proposed
- **Tanggal:** 2026-10-01
- **Konteks repo:** `galleon-fleet` (3-tier; Rust Core)
- **Keputusan oleh:** menunggu sign-off Owner (Achmad)

## Konteks

`crates/clawcrew-channels` memuat **~35 channel** 3rd-party dalam **satu crate ~197k LOC** (matrix 505k, slack 398k, telegram 363k bytes). Edit satu channel memaksa recompile seluruh crate → build lambat, binary besar, maintenance rapuh. Banyak channel bersifat **regional/niche** (hanya populer di daerah tertentu), tidak perlu di-ship ke semua user.

Keluhan nyata Owner: build lambat + aplikasi besar + sulit maintain.

## Bukti terverifikasi (dibaca dari kode, bukan asumsi)

1. **Plugin runtime SUDAH ADA & matang** — `crates/clawcrew-plugins`: WASI/Wasmtime (`runtime.rs`, `host.rs`), WIT contract, `catalog.rs`, `registry.rs`, `signature.rs`, `wasm_channel.rs`.
2. **Channel SUDAH feature-gated** — `clawcrew-channels/src/lib.rs` memakai `#[cfg(feature = "channel-xxx")]` untuk hampir semua channel. Fondasi lapis feature-crate sudah ada.
3. **WIT channel-plugin mendukung polling + webhook** — `wit/v0/channel.wit` (`clawcrew:plugin@0.1.0`, world `channel-plugin`):
   - `poll-message: func() -> option<inbound-message>` — **required**, polling adalah baseline.
   - Flag `webhook-ingress` + `webhook-path` + `parse-webhook(webhook-request) -> result<webhook-response, webhook-rejection>` — webhook penuh: GET/POST, auth (`unauthorized`/`bad-request`), challenge reply, delivery messages.
4. **Fixture plugin channel yang jalan ada** — `clawcrew-plugins/tests/fixtures/channel-fixture/` (`wit_bindgen::generate!` + `export!` + `plugin-manifest.toml` dengan `config_schema`, `x-secret`, `permissions`). Jadi template nyata, bukan teori.
5. **Batas teknis WASI channel (penting):** channel-plugin **TIDAK diberi `wasi:http` outbound** (host `new_channel_store` menahannya — lihat komentar `wasm_channel.rs`/fixture). Artinya plugin channel **inbound-first** (terima via webhook/poll). Channel yang butuh kirim HTTP keluar aktif, WebSocket native dua-arah, atau native crypto **tidak** cocok sebagai WASI plugin v0.
6. **Kontrak masih `@unstable(feature = plugins-wit-v0)`** — lengkap tapi belum beku. Aman untuk pilot internal; **bekukan sebelum marketplace publik**.

## Keputusan

Terapkan model **3 tier distribusi channel**:

- **Tier A — Embed (first-party default, selalu ikut):** `telegram`, `discord`, `slack` + primitif transport (`webhook`, `email`, `cli`, `acp`, `filesystem`). Identity produk inti.
- **Tier B — Feature-crate opsional (compiled-in, user pilih saat install):** channel yang butuh long-lived WebSocket native / native crypto / SDK berat → **tidak cocok WASI**. Pecah bertahap jadi `galleon-channel-<name>` + Cargo feature flag.
  `matrix`, `signal`, `whatsapp`/`whatsapp-web`, `wechat`, `qq`, `imessage`, `irc`, `amqp`, `nextcloud`, `mattermost`, voice stack (`voice`, `tts`, `transcription`, `voice_call`, `voice_wake`).
- **Tier C — WASI plugin / marketplace (hot-load, signed):** channel inbound-first (webhook/polling), regional/niche → runtime `clawcrew-plugins` yang sudah ada.
  `line`, `lark`, `dingtalk`, `wecom`, `mochat`, `clawdtalk`, `twitter`, `bluesky`, `nostr`, `reddit`, `twitch`, `notion`, `linq`, `gmail_push`, `git`.

Penentu garis **B↔C** (teknis, dari kode): butuh socket native dua-arah / native crypto / HTTP outbound aktif → **B**. Cukup webhook/polling inbound → **C**.

## Mapping channel final

| Channel | Tier | Alasan teknis |
|---|---|---|
| telegram | A | inti global, HTTP/webhook |
| discord | A | inti global |
| slack | A | inti global |
| webhook, email, cli, acp, filesystem | A | primitif transport, bukan 3rd-party |
| matrix | B | matrix-sdk besar + long-lived sync |
| signal | B | libsignal native crypto |
| whatsapp / whatsapp-web | B | web reverse-eng + session storage native |
| wechat | B | protokol kompleks, regional CN |
| qq | B | protokol kompleks, regional CN |
| imessage | B | macOS-only native bridge |
| irc | B | persistent TCP |
| amqp | B | persistent AMQP (lapin) |
| nextcloud | B | persistent connection |
| mattermost | B | WS + payload besar |
| voice / tts / transcription / voice_call / voice_wake | B | audio pipeline native |
| line | C | webhook-based, regional JP |
| lark | C | webhook-based, regional |
| dingtalk | C | webhook-based, regional CN |
| wecom | C | webhook-based, regional CN |
| mochat | C | webhook-based, niche |
| clawdtalk | C | webhook-based, niche |
| twitter | C | polling/social |
| bluesky | C | polling/social |
| nostr | C | relay/polling |
| reddit | C | polling/social |
| twitch | C | webhook/polling |
| notion | C | webhook/API |
| linq | C | webhook/API |
| gmail_push | C | push/webhook |
| git | C | polling |

> Catatan: `twitter`/`bluesky`/`reddit` yang butuh HTTP outbound aktif untuk posting mungkin perlu partial-B sampai `wasi:http` outbound dibuka untuk channel. Validasi per-channel saat porting (lihat batas teknis #5).

## Urutan eksekusi

1. **Pilot (bukti kontrak):** 2 channel Tier C paling cocok inbound-first → bangun sebagai WASI plugin pakai template fixture + manifest. Target: build & e2e lewat.
2. Setelah pilot hijau → bekukan WIT v0 (`wit/v0/.frozen`), jalankan `wit-breaking-change-check`.
3. Lapis B: pecah channel berat jadi `galleon-channel-<name>` + feature flag (lazy win build/size).
4. Marketplace: catalog + signature untuk plugin Tier C komunitas (monetisasi ekspansi, bukan paywall akses dasar — sesuai steering produk).

## Konsekuensi

- **Positif:** build time turun (crate kecil, cache per-plugin/crate), binary core jauh lebih kecil, channel niche tak membebani semua user, selaras "Your Fleet. Your Rules." + monetisasi ekspansi.
- **Negatif / biaya:** butuh WIT contract stabil (bekukan sebelum publik); porting per-channel butuh effort; sebagian channel outbound-aktif mungkin tertahan sampai `wasi:http` channel dibuka.
- **Risiko rendah bila:** mulai dari pilot kecil, jangan migrasi 35 sekaligus, jangan sentuh Tier A.

## Referensi (file nyata di repo)
- `wit/v0/channel.wit` — kontrak channel-plugin
- `crates/clawcrew-plugins/src/wasm_channel.rs` — host adapter
- `crates/clawcrew-plugins/tests/fixtures/channel-fixture/` — template plugin + manifest
- `crates/clawcrew-channels/src/lib.rs` — feature gates saat ini
- `.kiro/steering/galleon-product-fundamentals.md` — arah monetisasi

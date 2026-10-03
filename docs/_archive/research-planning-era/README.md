# Galleon — Research (Bisnis, Strategi, Positioning)

> Folder ini memisahkan materi **riset produk & bisnis** (visi, monetisasi, positioning, analisa kompetitor) dari `docs/` yang berisi dokumentasi engineering/refactoring (beberapa legacy).
> Dipindah dari `docs/` pada 2026-10-01 dengan `git mv` (history terjaga).

## Isi

| Path | Fase | Isi | Status otoritas |
|---|---|---|---|
| [`00-business-gtm-analysis.md`](00-business-gtm-analysis.md) | — | Analisa bisnis/GTM/strategi (tinjauan jujur solo dev) | Analisa terbaru |
| [`finalize/`](finalize/README.md) | Phase 4 | Finalisasi end-to-end: PRD, persona, competitive positioning, task breakdown | **SSOT — menang untuk konflik** |
| [`fundamental/`](fundamental/01-executive-summary.md) | Phase 3 | Dasar produk: visi, operating model, monetisasi, north-star, UX | Referensi "mengapa" |
| [`re-branding/`](re-branding/00-overview.md) | Phase 2 | Arsitektur Fleet Command, vocabulary, permission model, API/event contract | Referensi struktur teknis |
| [`monet/`](monet/claw-crew-community-pricing-legal-strategy.md) | — | Pricing, legal, community, control-plane PRD/tech-spec | Referensi monetisasi |
| [`monetization-strategy/`](monetization-strategy/01-monetization-strategy.md) | — | Strategi monetisasi (bedah komersial solo dev) | Referensi monetisasi |

## Hirarki keputusan

Bila dokumen bertentangan: **`finalize/` (phase-4) menang.** Koreksi penting phase-4: Quartermaster = *fleet coordinator / router dua-lapis*, **bukan** "super-agent".

## Catatan tautan

Referensi `docs/fundamental/`, `docs/re-branding/`, `docs/finalize/`, `docs/monet/` yang muncul sebagai **teks dalam backtick** di dalam file-file ini adalah penanda provenance historis (lokasi lama sebelum dipindah) — bukan tautan rusak. Tautan markdown antar-dokumen memakai path relatif (`../finalize/...`) dan tetap resolve karena semua folder pindah bersama ke `docs/research/`.

## Yang TIDAK dipindah (tetap di `docs/`, bukan riset bisnis)

- `docs/refactoring/`, `docs/refactoring-rust/`, `docs/refactoring-phase2/` — rencana migrasi/dekomposisi kode (engineering).
- `docs/_archive/` — arsip task lama.
- `docs/book/`, `docs/security/`, `docs/maintainers/` — dokumentasi runtime/engineering warisan upstream.
- `docs/clawcrew-*.md`, `docs/kirocrew-ui-parity.md` — gap-analysis teknis vs Kiro Crew (engineering, bukan GTM).

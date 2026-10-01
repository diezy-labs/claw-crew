# Galleon — Finalize (Phase 4, End-to-End)

> **Status:** Sumber kebenaran TUNGGAL untuk eksekusi akhir (phase-4).
> Menyatukan: phase-2 `docs/re-branding/` (arsitektur Fleet Command + permission model) + phase-3 `docs/fundamental/` (dasar produk) + keputusan desain phase-4 (turn ini).
> **Jika dokumen lain bertentangan dengan folder ini, folder ini menang.**

## Fase proyek
- **Phase 2 — `docs/re-branding/`**: arsitektur Fleet Command, vocabulary, permission model, module layout, API/event contract. Tetap referensi otoritatif untuk *struktur teknis*.
- **Phase 3 — `docs/fundamental/`**: dasar produk (visi, operating model, monetisasi, north-star). Tetap referensi untuk *mengapa*.
- **Phase 4 — `docs/finalize/` (INI)**: finalisasi end-to-end — mengunci keputusan desain yang masih terbuka, lalu memandu implementasi nyata.

## Keputusan terkunci phase-4
| # | Keputusan | Sumber |
|---|-----------|--------|
| D-01 | **Quartermaster = Router dua-lapis**, BUKAN super-agent. Satu pintu untuk chat ringan + routing quest. | `01-quartermaster-router-design.md` |
| D-02 | Orchestrator-worker sesungguhnya ada di **Captain** (`crew.StartTurn`), bukan di Quartermaster. | D-01 |
| D-03 | Intent (`chat / engine_room / report / objective`) = enum SSOT di Go; UI tak menebak. | D-01 |
| D-04 | Aksi high-impact (`objective` → FleetOrderProposal) selalu lewat approval Pirate King. | re-branding `04`/`07` |

## Index dokumen finalize
| # | Dokumen | Isi |
|---|---------|-----|
| 01 | [Quartermaster Router Design](01-quartermaster-router-design.md) | Desain perilaku Quartermaster (dua-lapis router) |
| 02 | [PRD](02-prd.md) | Problem, user, goals, stories, scope phase-4, metrics |
| 03 | [Design & UX](03-design-ux.md) | Satu-pintu Quartermaster, layar inti, interaksi, visual, a11y |
| 04 | [C4 Architecture](04-c4-architecture.md) | Context→Container→Component→Code, penempatan Router |
| 05 | [Product Spec](05-product-spec.md) | Entitas, spec perilaku QM/Crew, persistence, risk tiers, API, DoD |
| 06 | [Core Business, Persona & Frasa](06-core-business-persona.md) | Landasan bisnis, vocabulary, persona Quartermaster, copy kanonikal |
| 07 | [Competitive Positioning & Tech-Gap](07-competitive-positioning.md) | Analisa Kiro Crew/OpenClaw/Hermes, gap, adopsi, pembeda |
| 08 | [Next Plan & Task Breakdown](08-next-plan-tasks.md) | SSOT task aktif (Milestone 1–5), gabungan task lama + adopsi kompetitor |

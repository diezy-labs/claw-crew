# Refactoring Phase 2 — SSOT Migration: Backend Logic ke Go/Rust

> Tujuan Owner: **semua backend logic ditulis di Go (`engine/`) atau Rust (`crates/`, `apps/tauri-2`)**; `web-2` murni presentasi (fetch, tanpa seed/mock, tanpa logika domain).
> Dasar: `AUDIT.md` (2026-10-01) + pembacaan tier aktual (`wire_gen.go`, `fleet/services.go`, `apps/tauri-2/src/main.rs`, `web-2/src/utils/{seedData,apiClient}.ts`).
> Status legenda: `[ ]` belum · `[~]` sedang dikerjakan (claim) · `[x]` selesai + terverifikasi.

---

## 1. Masalah yang diselesaikan (observasi aktual)

> Status per 2026-10-01 sore. ✅ = selesai+verifikasi · ⏳ = butuh keputusan · ⬜ = belum.

| Kode | Temuan | Lokasi | Dampak | Status |
|------|--------|--------|--------|--------|
| S1 | Sumber kebenaran data fleet masih di frontend (`seedData.ts` 24KB) | `web-2/src/utils/seedData.ts`, `store/fleetStore.ts` | Langgar "zero mock in UI"; **data domain di frontend = user tak benar-benar punya/kontrol** (M4) | ⬜ (E2) |
| S2 | Risk-tier & policies ganda (Go vs TS `initialRiskTiers`) | `fleet/services.go` vs `seedData.ts` | Pelanggaran SSOT | ⬜ (B3/E3) |
| S3 | `DiskStore` dead code → engine pakai `MemoryStore` → **state hilang saat restart** | `engine/src/persistence/`, `wire_gen.go` | **KRITIS: janji inti "persistent" belum nyata di kode** (M-obs 1) | ⏳ (B1) |
| S4 | Logic Go stub/hardcoded (metrics, diagnostics, briefing) | `fleet/services.go` | Backend "ada" tapi tidak nyata | ⬜ (C1/C3) |
| S5 | `ChatQuartermaster` string-matching, bukan `llmProvider` | `fleet/services.go` | **Kredibilitas "AI CEO" runtuh di interaksi pertama** (M-obs 2) | ⬜ (C2) |
| S6 | Tool execution jalan di Go, bypass sandbox Rust | `engine/src/tool/builtin_*.go` | **"Autonomy under command" belum punya penegak** (M-obs 3) | ⬜ (D1/D2) |
| S7 | Paket `orchestrator/` tidak di-wire | `engine/src/orchestrator/` | BUKAN dead code — ini **executive planner Quartermaster** (otak #2). Belum tersambung. | ⏳ (A2) |
| S8 | `apps/zerocode` tidak compile | `apps/zerocode/src/api/engine_client.rs` | Blocker build Rust | ✅ (A1 — modul dihapus) |
| S9 | `apiClient.ts` return `Promise<any>` | `web-2/src/utils/apiClient.ts` | Type-unsafe boundary | ⬜ (E1) |

**Sudah selesai sesi ini:** A1 (zerocode build ✅), A3 (skrip root → `scripts/migrations/` ✅), A4 (AUDIT sync ✅), A5 (clippy workspace tuntas ✅), **R1 Tauri proxy ✅** (`main.rs` kini proxy murni ke `:9090`), R4 (server.ts content-type ✅), B3-frontend-contract (server.ts endpoint ✅).

---

## 1b. Observasi arsitektur (memperkuat desain ↔ branding)

Hasil analisa klaim-produk vs kode nyata. Ini mengubah **prioritas**, bukan sekadar menambah task.

- **M-obs — "Persistent" masih fiksi.** Branding bertumpu pada *persistent*, tapi `MemoryStore` menghapus semua saat restart. **B1 bukan housekeeping — ia membuat core promise jadi nyata.** Prioritas setara A2.
- **M-obs — Quartermaster masih mock.** `if contains("release")` akan ketahuan user teknis dalam 2 pesan. **C2 = momen produk hidup/mati.**
- **M-obs — "Autonomy under command" belum ditegakkan.** Tool jalan langsung di Go; sandbox Rust (D1/D2) adalah penegak janji keamanan — bukan nice-to-have untuk produk yang menjual *"Independent, not ungoverned"*.
- **M1 — Quartermaster = router dua-mode.** `ChatQuartermaster` klasifikasi intent eksplisit: `chat` (jawab LLM) · `objective` (→ `orchestrator.ProcessObjective`) · `report` (tarik store) · `engine_room` (telemetry). SSOT intent di Go, UI tak menebak. → task **C2a**.
- **M2 — "Learning by consent" = mekanisme, bukan slogan.** Koreksi user jadi `MemoryProposal{scope, rule, versioned, reversible}` bertipe Artifact masuk antrian approval. Struktur data harus benar dari awal (mahal diubah). → task **C5**.
- **M3 — Treasury jangan jadi credit-lock-in.** BYOK cost transparan & **read-only**; `Timber` = metafora kapasitas, bukan unit inference. Invariant di steering. → task **F1**.
- **M4 — SSOT boundary = fitur ownership.** Data domain di frontend = user tak mengontrolnya. `tests/architecture/no_duplicate_state` dijadikan **CI gate** nyata. → task **F2**.

---

## 2. Lajur paralel (parallelizable lanes)

Lima lajur bisa berjalan **bersamaan** karena file-ownership-nya tidak tumpang tindih. Dependency antar-lajur ditandai `depends on`.

```
LANE A — Build Green & Hygiene        (no dep)          → crew-devops / crew-be
LANE B — SSOT Data ke Go              (depends A build)  → crew-be
LANE C — Isi stub Go jadi logic nyata (depends B seed)   → crew-be + crew-rnd
LANE D — Go→Rust SystemGateway        (depends A build)  → crew-be + Rust owner
LANE E — Frontend tipe & de-mock      (depends B API)    → crew-fe
```

### Boundary file-ownership (mencegah writer bentrok saat paralel)

| Lane | File yang DIMILIKI (boleh tulis) | JANGAN sentuh |
|------|----------------------------------|---------------|
| A | `apps/zerocode/**`, `engine/src/orchestrator/**`, root `fix_*.py`/`patch*.py`, `AUDIT.md` | `web-2/**`, `fleet/services.go` |
| B | `engine/src/persistence/**`, `engine/app/wire*.go`, `engine/**/*_store.go`, `engine/data/**` seed | `web-2/src/components/**` |
| C | `engine/src/fleet/services.go`, `engine/src/crew/**`, `engine/src/llm/**` | `web-2/**`, `apps/**` |
| D | `engine/src/tool/builtin_*.go`, `engine/pkg/client/**`, `crates/clawcrew-gateway/**` | `web-2/**`, `fleet/services.go` |
| E | `web-2/src/utils/apiClient.ts`, `web-2/src/utils/seedData.ts`, `web-2/src/store/fleetStore.ts`, `web-2/src/types/**` | `engine/**`, `apps/**` |

Lihat `task-breakdown.md` untuk daftar task per lane dengan acceptance criteria.

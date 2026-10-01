# Audit galleon-fleet — Dead code, redundansi, bug

> Dianalisa dengan tooling nyata (`go build`/`go vet`, pembacaan sumber per tier), bukan tebakan.
> Tanggal: 2026-10-01 (diperbarui sore — Lane A `docs/refactoring-phase2` sebagian selesai).
> Fokus pada tier aktif: `apps/tauri-2`, `web-2`, `engine` (Go).
> `crates/` (Rust ClawCrew warisan) diaudit terpisah via `cargo clippy` — lihat §6.

Legenda status: ✅ terverifikasi · 🔨 desain/klaim belum diimplementasi · ⚪ belum dijalankan.

---

## 1. Ringkasan prioritas

| # | Isu | Tingkat | Status | Effort |
|---|-----|---------|--------|--------|
| B1 | `orchestrator_test.go` tidak compile (`NewMockProvider` 2 arg) | 🔴 Bug | ✅ **DIPERBAIKI** (`go vet` exit 0) | 1 baris |
| B4 | `zerocode` tidak compile — modul `api/engine_client.rs` orphaned pakai `reqwest` tak dideklarasikan | 🔴 Bug/Dead | ✅ **DIPERBAIKI** (modul dihapus; `cargo check -p zerocode` exit 0) | — |
| B2 | Paket `orchestrator` tidak pernah di-wire (dead code + test menyesatkan) | 🔴 Bug/Dead | ⏳ **KEPUTUSAN** (A2 — lihat catatan) | Keputusan arsitektur |
| B3 | Kontrak route frontend ↔ engine tidak sinkron | 🔴 Bug | ✅ **DIPERBAIKI** (`server.ts` + Tauri proxy) | Sedang |
| D1 | `persistence.DiskStore` dead code — state hilang saat restart | 🟠 Dead | ⏳ **KEPUTUSAN** (B1 refactoring-phase2) | Keputusan arsitektur |
| R1 | Logika domain duplikat Tauri (Rust) vs engine (Go) | 🟡 Redundan | ✅ **DIPERBAIKI** (Tauri `main.rs` kini proxy murni ke `:9090`) | Sedang |
| R2 | `.unwrap()` di jalur produksi `tauri-2/main.rs` | 🟡 | ✅ (hilang saat R1 — command mock yang pakai `.unwrap()` dihapus/jadi proxy) | Kecil |
| R3 | `apiClient.ts` return `Promise<any>` | 🟡 | ⏳ (E1 refactoring-phase2) | Kecil |
| R4 | `server.ts` proxy menimpa `content-type` tanpa syarat | 🟡 | ✅ **DIPERBAIKI** | Kecil |
| D4 | Skrip migrasi sekali-pakai di root | ⚪ Housekeeping | ✅ **DIPERBAIKI** (dipindah ke `scripts/migrations/`) | Kecil |

> Catatan A2/B2: paket `orchestrator` BUKAN sekadar dead code — ia mengimplementasikan alur Quartermaster LLM nyata (`ProcessObjective` memakai `llm.Provider`) yang justru diminta task C2. Menghapusnya membuang satu-satunya kode orkestrasi LLM; mewire-nya butuh keputusan: `orchestrator` atau `fleet.ChatQuartermaster` yang memiliki logika Quartermaster. Ditahan untuk keputusan Owner (lihat akhir dokumen).

---

## 2. 🔴 BUG — pasti, terverifikasi

### B1 — `engine` test package tidak compile ✅ SUDAH DIPERBAIKI
`engine/src/orchestrator/orchestrator_test.go:22` memanggil `llm.NewMockProvider("test-model", "...")` dengan 2 argumen, tapi signature-nya 1 argumen (`engine/src/llm/mock_provider.go:16`). Ini mem-block `go test ./...` dan `go vet ./...` untuk SELURUH engine.

```
vet.exe: orchestrator_test.go:22:47: too many arguments in call to llm.NewMockProvider
    have (string, string); want (string)
```

**Fix yang diterapkan** — pakai 1 arg lalu `SetResponses`:
```go
mockLLM := llm.NewMockProvider("test-model")
mockLLM.SetResponses([]string{"Here is a drafted Squad for your objective."})
```
**Verifikasi:** `go vet ./...` sekarang **exit 0** (bersih).

### B2 — Dua implementasi `NewService` orchestrator yang bentrok
`engine/src/orchestrator/services.go` punya `NewService(llm, fleet)`, tapi `engine/app/wire_gen.go` TIDAK memakainya; yang di-wire adalah `crew.NewService(provider, toolDispatcher, systemGatewayClient)` yang disimpan ke variabel bernama `orchestrator`. Jadi paket `orchestrator` yang asli **tidak pernah di-wire** → dead code, dan `orchestrator_test.go` menguji kode yang tidak mencerminkan runtime.
**Rekomendasi:** putuskan satu — wire `orchestrator.NewService` beneran, atau hapus paket `orchestrator` + test-nya dan pindahkan test ke `crew`.

### B3 — Kontrak route frontend ↔ engine tidak sinkron
**Route yang BENAR diregistrasi engine** (`server.RegisterRouteFunc`, bukan stdlib mux):

| Paket | File | Route |
|-------|------|-------|
| fleet | `engine/src/fleet/delivery.go:23-27` | `/api/fleet/metrics`, `/api/fleet/deck-bell`, `/api/system/executive-briefing`, `/api/providers/harbor`, `/api/diagnostics` |
| crew | `engine/src/crew/delivery.go:41-44` | `/api/turn`, `/api/query`, `/api/v1/crews`, `/api/v1/crews/` |
| tool | `engine/src/tool/delivery.go:27-30` | `/api/v1/tools`, `/api/v1/approvals/`, `/api/v1/tool-executions/`, `/api/v1/tool-requests` |
| run/workflow/artifact/task | masing-masing `delivery.go` | `/api/v1/runs`, `/api/v1/workflows`, `/api/v1/artifacts/`, `/api/v1/tasks/` |

**Panggilan frontend yang TIDAK cocok:**

| Pemanggil | Path | Dilayani oleh |
|-----------|------|---------------|
| `web-2/src/utils/apiClient.ts:79` `getSystemMetrics` | `/api/system/metrics` | HANYA `web-2/server.ts:85` — bukan engine. 404/503 bila frontend menembak engine langsung. |
| `web-2/src/components/features/RemoteAccessModal.tsx:92` | `/api/system/network` | **TIDAK ADA** di mana pun → selalu gagal. |
| `RemoteAccessModal.tsx:109` | `/api/providers/ollama/status` | **TIDAK ADA** → selalu gagal. |
| `RemoteAccessModal.tsx:59` | `/api/providers/ollama/generate` | **TIDAK ADA** → selalu gagal. |

Catatan: `apiClient.ts:54` `/api/fleet/metrics`, `:91` `/api/system/executive-briefing`, `:103` `/api/providers/harbor` **cocok** dengan engine. Jadi masalahnya terlokalisir ke `/api/system/metrics` (salah prefix — harusnya `/api/fleet/metrics` atau proxy via server.ts) dan tiga call Ollama/network yang memang belum ada endpoint-nya.

**Rekomendasi:** lihat §4 (rencana perbaikan B3).

---

## 3. 🟠 Dead code / tidak terpakai

- **D1 — `engine/src/persistence/DiskStore` seluruh paket dead code.** `engine/app/wire_gen.go` memakai `run.NewMemoryStore()`, `task.NewMemoryTaskStore()`, `artifact.NewMemoryRepository()`. `DiskStore` (`engine/src/persistence/disk_store.go`) hanya direferensikan oleh dirinya sendiri + `disk_store_test.go`. **Konsekuensi: semua state fleet hilang saat restart**, bertentangan dengan klaim "persistensi kanonikal". Pilih: wire DiskStore, atau hapus + perbaiki doc arsitektur (sesuai aturan: jangan klaim fitur yang belum ada).
- **D2 — `engine/src/orchestrator/*`** tidak di-wire (lihat B2).
- **D3 — placeholder:** `engine/src/tool/hello.txt` (22 byte), file firmware 14-byte di `crates/clawcrew-runtime` & `clawcrew-hardware`. Artefak, bukan sumber.
- **D4 — skrip migrasi sekali-pakai di root repo:** ✅ **DIPERBAIKI** — `fix_xtask.py`, `fix_structs.py`, `fix_sidebar.py`, `fix_patch.py`, `patch_ui.py`, `patch.py` dipindah ke `scripts/migrations/` (via `git mv`, + README arsip). Root bersih. (`rebrand*.py`, `*_docs.py`, `create_logo.py` masih di root — di luar scope A3.)

---

## 4. Rencana perbaikan B3 & R1 (yang sedang dikerjakan)

**B3 — SUDAH DIPERBAIKI (`web-2/server.ts`):**
Node gateway (bukan Go engine) adalah pemilik yang benar untuk telemetri host-lokal. Ditambahkan sebelum catch-all proxy:
- `GET /api/system/network` — bentuk respons cocok dengan interface `NetworkInfo` di `RemoteAccessModal.tsx` (sebelumnya `/api/network/interfaces` dengan shape berbeda → fall-through ke proxy → 404).
- `GET /api/providers/ollama/status` & `POST /api/providers/ollama/generate` — mem-probe daemon Ollama host (`OLLAMA_HOST`, default `127.0.0.1:11434`), bukan engine.
`/api/system/metrics`, `/api/engine/processes`, `/api/engine/execute` ternyata SUDAH dilayani `server.ts` (route eksplisit menang atas catch-all proxy `app.use('/api', …)`), jadi OK di browser mode — koreksi atas analisa awal.

**R4 — SUDAH DIPERBAIKI (`web-2/server.ts`):** proxy tidak lagi menimpa `content-type` tanpa syarat; hanya set `application/json` bila header belum ada.

**R1 — SUDAH DIPERBAIKI (`apps/tauri-2/src/main.rs`):** 10 command fleet-domain (`get_fleet_metrics`, `ring_deck_bell`, `get_executive_briefing`, `get_harbor_providers`, `get_diagnostics`, `apply_remedy`, `get_snapshots`, `create_snapshot`, `get_fleet_policies`, `chat_quartermaster`) tidak lagi mengembalikan mock hardcoded — kini proxy async ke engine HTTP (`GALLEON_ENGINE_URL`, default `http://127.0.0.1:9090`) via helper `engine_get`/`engine_post`. Risk-tier & quartermaster-reply yang tadinya divergen Rust-vs-Go kini satu sumber (engine). `get_collection`/`save_collection` (file store lokal) & host-telemetry tetap lokal. Tambah dep `reqwest` (live code, justified). Bonus: `apps/tauri-2/Cargo.toml` diberi `[workspace]` kosong agar bisa build standalone (tadinya tidak bisa build sama sekali). Verifikasi: `cargo check` exit 0.
> Sisa SSOT frontend (hapus `seedData.ts`/`initialRiskTiers` dari TS) ada di Lane B/E `docs/refactoring-phase2`, belum dikerjakan.

---

## 5. 🟡 Redundansi & standar

- **R1** — lihat §4 (duplikasi SSOT di 3 tempat).
- **R2** — `apps/tauri-2/main.rs` memakai `.unwrap()` di jalur produksi (langgar standar Rust di `architecture.md`): `get_collection` (`serde_json::to_string_pretty(&initial).unwrap()`), `create_snapshot`/`get_engine_processes` (`duration_since(UNIX_EPOCH).unwrap()`). Ganti dengan `?`/`map_err` atau default aman.
- **R3** — `web-2/src/utils/apiClient.ts` hampir semua method return `Promise<any>`; tipe sudah tersedia di `src/types/index.ts`. Ketik ulang return value.
- **R4** — `web-2/server.ts` reverse-proxy menimpa `content-type` → `application/json` tanpa syarat untuk semua non-GET; akan merusak upload non-JSON bila nanti ada. Set hanya jika body JSON.

---

## 6. crates/ (Rust workspace) — hasil `cargo clippy`

Dijalankan: `cargo clippy --workspace --lib --message-format=short` (cargo 1.98.1) setelah B4 diperbaiki. **Exit 0 — workspace compile penuh** (sebelumnya abort di zerocode).

### 🔴 B4 — `zerocode` tidak compile ✅ DIPERBAIKI (dihapus)
`apps/zerocode/src/api/engine_client.rs` memakai `reqwest::Client` tapi `reqwest` tak ada di `Cargo.toml`. `EngineClient` + DTO-nya **tanpa caller nyata** (orphaned) → modul `api/` dihapus seluruhnya + baris `mod api;` di `main.rs`. Konsumen sisa (`turn_status.rs::from_engine_run_status`) ikut dihapus (A5/E1 hygiene). Verifikasi: `cargo check -p zerocode` **exit 0**.

### 🟡 Warning workspace (deduped, lib targets) — backlog A5
| Count | Lint | Lokasi utama |
|-------|------|--------------|
| 23 | use of disallowed macro `anyhow::anyhow` (dilarang `clippy.toml`) | `crates/clawcrew-runtime/src/control_plane/task_runner.rs` (+ `platform/app_registry.rs:238`) |
| 6 | this `impl` can be derived | `task_runner.rs:154` dll |
| 4 | this `if` statement can be collapsed | — |
| 1 | **`filter_map()` run-forever** → `map_while(Result::ok)` (suspicious) | `apps/tauri/src/commands/engine.rs:47` (tauri LAMA) |
| 1 | field `app_registry` never read | `crates/clawcrew-runtime/src/agent/tool_execution.rs:69` |
| 1 | add `Default` impl untuk `AcpAdapter` | `crates/clawcrew-runtime/src/rpc/acp.rs:17` |

Tidak ada error correctness. Satu-satunya lint suspicious (`filter_map`-forever) ada di `apps/tauri` (tauri lama, bukan tauri-2). `anyhow::anyhow` dilarang oleh `clippy.toml` repo → ganti pakai error enum / `bail!` sesuai aturan daemon-path.

### ⚪ Belum dijalankan
`--all-targets` (termasuk build test) **belum** — ditahan karena memory host TIGHT (4 GB). Jalankan saat memory lega:
```
cargo clippy --workspace --all-targets --message-format=short 2>&1
```
`apps/tauri-2` adalah workspace terpisah → lint sendiri: `cd apps/tauri-2 && cargo clippy`.


---

## 7. Catatan metodologi

- Engine: `go vet ./...` (dari `engine/`) — exit 0 setelah B1.
- Route map: diambil dari call site `server.RegisterRouteFunc(...)` di tiap `delivery.go`, bukan asumsi.
- `web-2` `tsc --noEmit` belum dijalankan (node_modules belum terpasang di folder itu) — ⚪.
- `crates/` clippy: sesi terpisah (lihat §6).

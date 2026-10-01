# Task Breakdown — SSOT Migration (parallel-ready)

> **SCOPE DOKUMEN INI = SSOT Migration (engine Go + web-2 + wiring).** Lane A–F di bawah.
> **Pemecahan crate/file Rust & optimisasi build DIPISAH** ke `rust-decomposition-plan.md`
> (sesi "refactoring Rust" tersendiri) agar kedua sesi tidak redundan dan tidak bentrok writer.
> Jangan kerjakan RF-A/RF-B/RF-M di sini; sebaliknya jangan sentuh `engine/**` atau `web-2/**` di sesi Rust.
> Catatan silang: A1 (hapus modul zerocode) & A5 (clippy workspace) sudah **selesai** dan juga menjadi
> prasyarat build-hijau untuk RF-A di dokumen Rust — tidak perlu dikerjakan ulang di kedua sisi.

> Setiap task punya **owner lane**, **depends**, **acceptance** (cara verifikasi), dan **file scope**.
> Klaim task dengan mengubah `[ ]` → `[~] @nama` saat mulai, `[x]` saat selesai + verifikasi lulus.
> Satu task = satu PR kecil (satu concern). Branch non-`master`, PR ke `master`.

---

## LANE A — Build Green & Hygiene (no dependency, kerjakan duluan)

- [x] **A1** — Hapus modul orphaned `apps/zerocode/src/api/engine_client.rs` + `mod.rs` + `mod api;` (tak ada caller nyata; pakai `reqwest` tak dideklarasikan). **Selesai**: `cargo check -p zerocode` exit 0; unused-import ikut dibersihkan (`run_workspace.rs`, `turn_status.rs::from_engine_run_status`).
  - depends: —  · scope: `apps/zerocode/**`
  - acceptance: ✅ `cargo build -p zerocode` exit 0; clippy tanpa unused-import dari modul ini.
- [~] **A2** — Wire paket `orchestrator/` sebagai **executive planner Quartermaster** (otak #2), BUKAN dihapus. Keputusan desain (opsi-a, dikonfirmasi dari deskripsi Owner): `fleet.ChatQuartermaster` = surface + chat (otak #1); `orchestrator.ProcessObjective` = analisa objective → bagi ke ships/squad/crew (otak #2); `crew.StartTurn` = mesin eksekusi. Tunggu go-ahead Owner sebelum ubah wiring.
  - depends: —  · scope: `engine/src/orchestrator/**`, `engine/app/wire*.go`
  - acceptance: `orchestrator.NewService` ter-wire & terpakai; `go vet ./...` + `go test ./...` exit 0; `orchestrator_test.go` menguji jalur nyata; komentar "Did I rename the folder?" dibersihkan.
- [x] **A3** — Pindahkan `fix_patch.py`, `fix_sidebar.py`, `fix_structs.py`, `fix_xtask.py`, `patch_ui.py`, `patch.py` ke `scripts/migrations/` (via `git mv` + README). **Selesai**: root bersih; tak ada referensi CI/build (hanya disebut di AUDIT & doc ini).
  - depends: —  · scope: root `*.py`
  - acceptance: ✅ root repo bersih dari skrip ad-hoc fix_/patch; referensi diperbarui.
- [x] **A4** — Sinkronkan `AUDIT.md`: R1 Tauri ✅ (main.rs proxy murni), B4 ✅ (dihapus), D4 ✅ (dipindah), B2/D1 ⏳ keputusan, tanggal diperbarui.
  - depends: —  · scope: `AUDIT.md`
  - acceptance: ✅ AUDIT.md tidak lagi mengklaim duplikasi Tauri yang sudah tidak ada.
- [x] **A5** — Clippy workspace jalan tuntas setelah A1. **Selesai (lib targets)**: `cargo clippy --workspace --lib` exit 0; backlog warning tercatat di AUDIT §6. `--all-targets` ditahan (memory TIGHT).
  - depends: A1  · scope: — (laporan)
  - acceptance: ✅ clippy berjalan sampai selesai (tidak abort di zerocode); daftar warning terdokumentasi.

## LANE B — SSOT Data ke Go (depends: build hijau)

- [ ] **B1** — Wire `persistence.DiskStore` menggantikan `MemoryStore` di `wire_gen.go` (S3): `run`, `task`, `artifact` pakai disk-backed store.
  - depends: A2  · scope: `engine/app/wire*.go`, `engine/src/persistence/**`, `engine/src/{run,task,artifact}/*_store.go`
  - acceptance: restart engine → data bertahan; `disk_store_test.go` lulus; tidak ada `NewMemoryStore` tersisa di jalur produksi.
- [ ] **B2** — Pindahkan `seedData.ts` (ships/crew/squads/quests) jadi seed JSON kanonikal di Go `engine/data/*.json`; engine serve via `/api/collections/{name}` saat kosong.
  - depends: B1  · scope: `engine/data/**`, `engine/src/fleet/services.go` (hanya bagian seed-load)
  - acceptance: `GET /api/collections/ships` mengembalikan data seed dari Go tanpa `seedData.ts`.
- [ ] **B3** — Jadikan `GET /api/fleet/policies` satu-satunya sumber risk-tier & policies (S2); hilangkan `initialRiskTiers`/`initialFleetPolicies` dari TS (koordinasi dengan E2).
  - depends: B2  · scope: `engine/src/fleet/services.go`
  - acceptance: nilai risk-tier hanya ada di Go; tidak ada tabel kedua.

## LANE C — Isi stub Go jadi logic nyata (depends: B seed)

- [ ] **C1** — `GetMetrics` baca state nyata dari store (bukan angka hardcoded fallback) (S4).
  - depends: B1  · scope: `engine/src/fleet/services.go`
  - acceptance: metrics mencerminkan jumlah collection aktual; test unit dengan store terisi.
- [ ] **C2** — `ChatQuartermaster` pakai `llmProvider` yang sudah di-inject, bukan string-matching (S5). **Momen produk hidup.**
  - depends: B1, A2  · scope: `engine/src/fleet/services.go`, `engine/src/llm/**`
  - acceptance: chat memanggil provider (mock di test); fallback aman saat provider offline; TIDAK ada `strings.Contains` sebagai logika balasan.
- [ ] **C2a** — (M1) Quartermaster router dua-mode: klasifikasi intent eksplisit `chat | objective | report | engine_room` sebagai enum SSOT di Go. `objective` → delegasi `orchestrator.ProcessObjective`; `report` → tarik store; `engine_room` → telemetry. UI tidak menebak intent.
  - depends: C2  · scope: `engine/src/fleet/services.go`, `engine/src/orchestrator/**`
  - acceptance: `QuartermasterIntent` enum tunggal di Go; tiap intent punya jalur; test per intent.
- [ ] **C3** — `GetDiagnostics`/`GetExecutiveBriefing` dari sinyal nyata (health check, store count, Engine Room RAM), bukan konstanta (S4).
  - depends: C1  · scope: `engine/src/fleet/services.go`
  - acceptance: diagnostics berubah sesuai kondisi; tidak ada latency/angka karangan.
- [ ] **C4** — Samakan kontrak route frontend↔engine (AUDIT §B3); dokumentasikan route map final.
  - depends: C1  · scope: `engine/src/*/delivery.go`
  - acceptance: setiap call `apiClient.ts` punya route yang benar-benar diregistrasi.
- [ ] **C5** — (M2) "Learning by consent" sebagai mekanisme: `MemoryProposal{scope, rule, versioned, reversible}` bertipe Artifact → antrian approval (seperti Captain's Approval). Struktur data benar dari awal.
  - depends: B1  · scope: `engine/src/memory/**`, `engine/src/approval/**`, `engine/src/artifact/**`
  - acceptance: koreksi user → MemoryProposal tersimpan sbg artifact pending; approve → tertulis scoped & reversible; reject → tercatat; test lifecycle.

## LANE D — Go→Rust SystemGateway (depends: build hijau)

- [ ] **D1** — Pastikan `builtin_*.go` (bash, read_file, git) mengeksekusi via `SystemGateway` gRPC ke Rust `:50052`, bukan langsung di Go (S6).
  - depends: A2  · scope: `engine/src/tool/builtin_*.go`, `engine/pkg/client/**`
  - acceptance: tool execution lewat Rust sandbox; test integrasi menolak path di luar workspace.
- [ ] **D2** — Verifikasi/implement sisi Rust `SystemGateway.ExecuteTool` di `crates/clawcrew-gateway` menerima panggilan Go.
  - depends: D1  · scope: `crates/clawcrew-gateway/**`
  - acceptance: round-trip Go→Rust→Go sukses untuk 1 tool nyata; Landlock boundary ditegakkan.

## LANE E — Frontend tipe & de-mock (depends: B API)

- [ ] **E1** — Ketik ulang `apiClient.ts`: ganti `Promise<any>` dengan tipe dari `src/types/index.ts` (S9).
  - depends: —  · scope: `web-2/src/utils/apiClient.ts`, `web-2/src/types/**`
  - acceptance: `tsc --noEmit` lulus; tidak ada `any` di return apiClient.
- [ ] **E2** — Hapus `seedData.ts` dari `fleetStore.ts`; store hidrasi dari `apiClient.getCollection()` saat init.
  - depends: B2  · scope: `web-2/src/store/fleetStore.ts`, `web-2/src/utils/seedData.ts`
  - acceptance: UI render dari data engine; `seedData.ts` dihapus; test store diperbarui.
- [ ] **E3** — Hapus referensi `initialRiskTiers`/`initialFleetPolicies` di TS; konsumsi dari `getFleetPolicies()` (S2).
  - depends: B3, E2  · scope: `web-2/src/store/fleetStore.ts`
  - acceptance: tidak ada definisi risk-tier di TS; FlagBridge/Policies view fetch dari engine.

---

## LANE F — Invariant branding ↔ core bisnis (dari analisa M3/M4)

- [ ] **F1** — (M3) Treasury: pastikan biaya provider BYOK ditampilkan **transparan & read-only**; Galleon tak pernah berdiri antara user & tagihan providernya. `Timber` tetap metafora kapasitas (jumlah Ship/Crew/Voyage), BUKAN unit konsumsi inference. Tulis sbg invariant di steering.
  - depends: —  · scope: `engine/src/treasury/**`, `.kiro/steering/galleon-product-fundamentals.md`
  - acceptance: tidak ada jalur yang menjual/mengurangi "timber" per inference; Treasury hanya lapor cost provider; invariant tertulis di steering.
- [ ] **F2** — (M4) Jadikan `tests/architecture/no_duplicate_state` CI gate nyata: fakta domain yang didefinisikan di dua tier (Rust+Go, atau TS+Go) → test gagal.
  - depends: B3  · scope: `tests/architecture/**`, CI
  - acceptance: menambah definisi risk-tier kedua di TS/Rust membuat test gagal; gate jalan di CI pre-merge.

---

## Urutan eksekusi (diurut ulang: tutup gap klaim-vs-kode lebih dulu)

Prinsip: tiap langkah membuat satu janji produk jadi nyata, bukan sekadar menyelesaikan task.

```
Fondasi      : A2 (wire orchestrator)  +  B1 (DiskStore → "persistent" nyata)
Produk hidup : C2 + C2a (Quartermaster LLM dua-mode)
Governance   : D1 + D2 (eksekusi tersandbox Rust — "autonomy under command")
Ownership    : B2 B3 + E1 E2 E3 (SSOT data ke Go, de-mock frontend)  + F2 (CI gate)
Nyata penuh  : C1 C3 (metrics/diagnostics/Engine Room nyata)  + C5 (learning-by-consent)  + F1 (Treasury invariant)
```

Peringatan (fundamentals §19): buktikan **satu Developer Ship end-to-end** (objective→quest→crew→artifact→treasure, persisten, tersandbox) sebelum menambah Ship Charter lain.

## Verifikasi global (sebelum tiap PR merge)

- Go: `cd engine && go vet ./... && go test ./...`
- Rust: `cargo clippy --workspace --all-targets -- -D warnings`
- Web: `cd web-2 && tsc --noEmit`
- SSOT gate: tidak ada fakta domain yang didefinisikan di dua tier (lihat `tests/architecture/no_duplicate_state.rs`).

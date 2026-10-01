# 08 — Next Plan & Task Breakdown (Phase 4 Finalize)

> SSOT task aktif. Menggabungkan task lama (refactoring-phase2 A–F, diarsip) yang masih relevan + adopsi kompetitor (`07`).
> Status: `[ ]` belum · `[~]` dikerjakan · `[x]` selesai+verifikasi. Satu task = satu PR kecil. Branch non-`master`.
> Verifikasi global tiap PR: Go `go vet ./... && go test ./...` · Rust `cargo clippy --workspace -- -D warnings` · Web `tsc --noEmit`.

## Prinsip urutan
Tiap langkah menutup satu gap klaim-vs-kode (`07` §2). Buktikan **satu Developer Ship end-to-end** sebelum menambah Ship lain (fundamentals §19).

---

## MILESTONE 1 — Fondasi jujur (janji "persistent" jadi nyata)

- [x] **F1-1 Wire DiskStore** — `DiskStore` kini implement `run.Store` (+Save/Get/UpdateStatus/List); wired di `wire_gen.go` via `cfg.DataDir`. **Selesai**: `go test ./src/persistence ./src/run` PASS. (Scope: run store. task/artifact DiskStore belum ada methodnya → F1-1b lanjutan.)
- [x] **F1-2 Reframe `orchestrator`** — komentar bingung dibersihkan; doc package = embrio `QuartermasterService.IntakeObjective`. **Bonus**: perbaiki deadlock `ProcessObjective` (chan tak ditutup). **Selesai**: test PASS 2.7s (sebelumnya hang 600s).
- [x] **F1-1b DiskStore untuk task & artifact** — `DiskTaskStore` (task.Store) + `DiskArtifactStore` (artifact.Repository), file-per-entity; wired di `wire_gen.go`. **Selesai**: round-trip tests PASS; tak ada `NewMemoryTaskStore`/`NewMemoryRepository` di produksi.
- [x] **F1-3 Task checkpoint/resume** (ADOPSI Kiro Crew) — `ResumableStore` interface + `run.Service.ResumeInterrupted`; dipanggil di `App.Run()` startup; emit `run.resumed`. **Selesai**: `TestService_ResumeInterrupted` PASS.

## MILESTONE 2 — Produk hidup (Quartermaster router)

- [x] **F2-1 ChatQuartermaster via LLM** — string-matching dibuang; streaming completion nyata (cabang `chat`) + fallback aman offline. **Selesai**: `chat_quartermaster_test.go` PASS (pakai-LLM + fallback).
- [x] **F2-2 QuartermasterIntent enum + cabang** — enum SSOT + rule-first classifier (`chat/engine_room/report/objective`), routing ter-log (intent+confidence+reason); `ChatQuartermaster` jadi router dispatch. **Selesai**: `TestClassifyIntent` PASS. (Objective branch placeholder; F2-3 wire ke IntakeObjective.)
- [x] **F2-3 IntakeObjective → FleetOrderProposal** — cabang `objective` kini drafting proposal bertipe via `orchestrator.IntakeObjective` (typed `FleetOrderProposal`, status selalu `awaiting_pirate_king_approval`), di-inject ke `fleetService` lewat `ObjectiveProposer` (dependency inversion, hindari import cycle orchestrator→fleet). `ChatQuartermaster` objective-branch panggil proposer; fallback aman saat tak ter-wire/LLM offline. **Tak pernah eksekusi** — Captain hanya jalan setelah approve. **Selesai**: `go test ./src/orchestrator ./src/fleet` PASS (IntakeObjective raw+JSON, ProposeFleetOrder gated, ChatQuartermaster objective+fallback); `go vet ./...` exit 0.

## MILESTONE 3 — Governance (autonomy under command)

- [~] **F3-1 Tool execution via Rust sandbox** — `builtin_*.go` eksekusi lewat `SystemGateway` gRPC `:50052`, bukan langsung Go. (eks-D1) **Edit (build-deferred):** sisi Go sudah lengkap sebelumnya (`tool.SystemGateway` iface + `withWorkspaceRoot` envelope + `ExecuteWithContext` route native→gateway + `pkg/client/system_gateway.go` gRPC client). **Gap ditemukan & diperbaiki:** `wire_gen.go:59` membuat `tool.NewService(...)` TANPA `.WithSystemGateway(systemGatewayClient)` → `s.gateway` nil di produksi → builtin selalu in-process (sandbox tak pernah aktif). Fix: (1) chain `.WithSystemGateway(systemGatewayClient)` di `wire_gen.go`; (2) tambah `WithSystemGateway` ke interface `tool.Service` (sebelumnya hanya di `*toolService`) agar chaining compile. gofmt bersih. Verifikasi build: tunggu Owner (`go vet ./... && go test ./...`).
  - scope: `engine/src/tool/builtin_*.go`, `engine/pkg/client/**`
  - acceptance: tool jalan di sandbox Rust; test tolak path di luar workspace.
- [ ] **F3-2 Rust SystemGateway.ExecuteTool** — sisi Rust terima panggilan Go; Landlock ditegakkan. (eks-D2) **SKIP — butuh approval + sesi build.** FAKTA (diverifikasi grep seluruh repo): proto `service SystemGateway { rpc ExecuteNativeTool }` ADA di `proto/agent_service.proto:20`; pb Go + server-iface Go ter-generate (`engine/pkg/pb/agent_service_grpc.pb.go:265`); Landlock ADA sebagai library (`crates/clawcrew-runtime/src/security/landlock.rs`). **Yang BELUM ADA: implementasi gRPC server `SystemGateway` di Rust** — tak ada tonic service impl, `execute_native_tool` handler, atau stub server Rust di mana pun di `crates/`. Butuh: generate stub tonic dari proto + impl service + map tool_name/args_json→eksekusi native dalam Landlock + listen `:50052`. Lintas-bahasa, lintas-crate, WAJIB full `cargo check`/build — tak bisa diverifikasi statis. Ditahan untuk sesi build-enabled + persetujuan Owner.
  - scope: `crates/clawcrew-gateway/**`
  - acceptance: round-trip Go→Rust→Go sukses 1 tool; boundary ditegakkan.
- [x] **F3-3 Learning-by-consent** (ADOPSI Hermes, versi governance) — `MemoryProposal{rule,scope,status,version,reversible}` + `ProposalStore` (propose→pending, approve→tulis ke VectorStore scoped, reject, revert) di `engine/src/memory/proposal.go`. Koreksi TIDAK pernah ditulis sebelum approve; write scoped via `Document.Scope`; reversible lewat `inMemoryVectorStore.Delete` (type-assert `revertableStore`); versioned (supersede bump). **Selesai**: `go test ./src/memory` PASS (lifecycle propose→approve→revert, reject-writes-nothing, scope-isolation, version-bump).

## MILESTONE 4 — Ownership (SSOT data ke Go, de-mock)

- [ ] **F4-1 Seed data ke Go** — `seedData.ts` → seed JSON kanonikal `engine/data/*.json`; serve via `/api/collections/{name}`. (eks-B2)
  - acceptance: `GET /api/collections/ships` dari Go tanpa seedData.ts.
- [x] **F4-2 Risk-tier/policies SSOT Go** — hapus `initialRiskTiers`/`initialFleetPolicies` dari TS. (eks-B3/S2) **Selesai (edit, saat F2):** kedua export orphaned (tak di-import) dihapus dari `web-2/src/utils/seedData.ts`; risk-tier/policy kini hanya di Go (`fleet/services.go` → `GET /api/fleet/policies`). Gate `tests/architecture/no_duplicate_risk_tier.rs` mencegah re-introduksi.
  - acceptance: nilai risk-tier hanya di Go. ✅
- [ ] **F4-3 apiClient tipe ketat** — ganti `Promise<any>` dgn tipe `src/types/index.ts`. (eks-E1)
  - acceptance: `tsc --noEmit` lulus; tak ada `any` di return.
- [ ] **F4-4 fleetStore hidrasi dari engine** — hapus `seedData.ts`; store init dari `apiClient.getCollection()`. (eks-E2/E3)
  - acceptance: UI render dari engine; seedData.ts dihapus; test store diperbarui.
- [~] **F4-5 CI gate no_duplicate_state** (M4) — fakta domain di dua tier → test gagal. (eks-F2) **Edit (build-deferred):** gate lama `tests/architecture/no_duplicate_state.rs` hanya scan peer-auth field di `crates/clawcrew-channels/src` (Rust-only) — tak bisa menangkap duplikasi risk-tier Go↔TS yang disebut F2. **Temuan nyata:** `web-2/src/utils/seedData.ts` MASIH mendefinisikan `initialFleetPolicies`+`initialRiskTiers` (orphaned, tak di-import) = duplikat SSOT risk-tier Go (`fleet/services.go:363`). **Tindakan:** (1) hapus kedua export orphaned itu; (2) tambah detector `tests/architecture/no_duplicate_risk_tier.rs` (scan `web-2/src`, fail bila `initialRiskTiers`/`initialFleetPolicies` di-redeclare, escape `// SOT:`), didaftarkan di `tests/test_architecture.rs`. Gate jalan di PR via `cargo nextest run --workspace` (ci.yml:745) + `cargo test --test architecture`. Verifikasi build: tunggu perintah Owner.
  - acceptance: menambah risk-tier kedua di TS/Rust → gagal di CI.

## MILESTONE 5 — Nyata penuh & hemat token

- [ ] **F5-1 Metrics/diagnostics/briefing nyata** — dari store count + health + Engine Room RAM, bukan konstanta. (eks-C1/C3)
- [ ] **F5-2 Cron no-LLM** (ADOPSI Kiro Crew) — scheduled script/command TANPA LLM call untuk polling/monitor. Dukung visi anti-boros-token.
  - scope: `engine/src/schedule/**`
  - acceptance: job deterministik jalan tanpa panggil provider.
- [ ] **F5-3 modelgateway swappable** (ADOPSI OpenClaw/Hermes) — BYOM plug-and-play (banyak provider), route policy + budget guard + circuit breaker.
  - scope: `engine/src/llm/**` (→ modelgateway)
  - acceptance: ganti provider tanpa ubah kode lain; fallback saat provider down.

## Tunda (bukan diferensiator sekarang)
Channel breadth (OpenClaw 20+), companion apps voice/camera, plugin/Charter marketplace, multi-Fleet, Marketing/Research/Ops Ship penuh, Fleet Knowledge F4-roadmap. Buka setelah 1 Developer Ship end-to-end terbukti.

## Dependency ringkas
```
M1 (F1-1 persist, F1-2 reframe, F1-3 checkpoint)
  → M2 (F2-1 chat LLM → F2-2 intent → F2-3 objective)
  → M3 (F3-1/F3-2 sandbox, F3-3 consent)
  → M4 (F4 SSOT data + CI gate)
  → M5 (F5 nyata penuh + cron no-LLM + modelgateway)
```

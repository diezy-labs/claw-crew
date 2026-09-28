# Claw-Crew Potential Bug Audit & Risk Register

> **Repository**: `diezy-labs/claw-crew`  
> **Branch reviewed**: `feat/enhance-agent-phase2`  
> **Assessment type**: Static architecture and codebase-risk audit  
> **Prepared**: 2026-09-28  
> **Scope**: Go Agent Engine, Rust ZeroCode runtime, Tauri sidecar/IPC, web UI, streaming, memory, orchestration, tool execution, and cross-layer API contracts.

---

## 1. Executive Summary

Dokumen ini mencatat potensi cacat (*potential defects*), skenario integrasi berisiko tinggi (*high-risk integration scenarios*), serta titik kegagalan (*failure modes*) pada repositori Claw-Crew selama proses migrasi arsitektur dari Rust ke Go engine.

Repositori ini sedang bertransisi dari aplikasi Rust TUI yang matang menuju arsitektur *hybrid* yang memadukan:
- **Rust ZeroCode**: Runtime terminal, interaksi desktop, dan rendering TUI.
- **Go 1.27 Agent Engine**: Orkestrasi multi-agen, eksekusi tool, pemanggilan LLM, dan memori RAG.
- **Lapisan Transport**: gRPC IPC lokal, REST `/api/v1`, dan Server-Sent Events (SSE).
- **Tauri Sidecar & Desktop IPC**: Pengelolaan siklus hidup daemon Go oleh shell desktop.
- **React / Vite Web Client**: Dashboard web manajemen dan observabilitas.
- **In-Memory Vector Store**: Pencarian kemiripan dokumen lokal berbasis SIMD.

Transisi ini memperluas permukaan risiko sistem secara signifikan. Titik kritis utama bukan sekadar kesalahan sintaksis atau kegagalan kompilasi, melainkan **cacat kebenaran saat runtime (*runtime correctness defects*)** yang timbul akibat eksekusi konkuren, batas pembatalan (*cancellation boundaries*), duplikasi kepemilikan state, rekoneksi streaming, *bypass* kebijakan keamanan, dan *contract drift* antar-bahasa.

### Batasan Penting (Important Limitation)
Audit ini merupakan laporan risiko berbasis telaah statis (*static-review risk report*). Cacat yang diidentifikasi didasarkan pada arsitektur repositori, batas modul, cakupan fitur, dan riwayat implementasi terbaru. Setiap isu wajib divalidasi melalui review kode sumber, pengujian deteksi *data race* (`go test -race`), tes integrasi lintas-lapisan, dan *smoke test* desktop/web sebelum diklasifikasikan sebagai bug terkonfirmasi.

### Prioritas Mendesak (Immediate Priorities)
- [ ] 1. Validasi penanganan pembatalan streaming dan pemutusan koneksi klien (*client disconnect*).
- [ ] 2. Jalankan deteksi race Go (`go test -race ./...`) secara berulang di seluruh engine.
- [ ] 3. Audit otorisasi dan gerbang persetujuan (*approval gate*) eksekusi tool pada setiap jalur pemanggilan.
- [ ] 4. Verifikasi aturan konkurensi dan isolasi cakupan (*scope isolation*) pada in-memory vector store.
- [ ] 5. Tambahkan pengujian kontrak (*contract tests*) otomatis antara Go, Tauri, Rust, dan TypeScript.
- [ ] 6. Pastikan penggabungan antara polling dan streaming tidak memicu duplikasi state pada antarmuka pengguna.
- [ ] 7. Lakukan pengujian regresi frontend dan desktop setelah pembaruan dependensi besar.

---

## 2. Arsitektur & Konteks Risiko

### 2.1 Topologi Sistem Saat Ini

```text
User
  |
  +-- Rust ZeroCode TUI (apps/zerocode)
  |
  +-- Tauri Desktop Client (apps/tauri)
  |
  +-- Web Client (web/)
          |
          +-- HTTP / gRPC / Tauri IPC / streaming bridge
                    |
                    v
              Go Agent Engine (engine/)
                    |
                    +-- Crew orchestration (engine/src/crew)
                    +-- LLM dispatcher/providers (engine/src/llm)
                    +-- Memory/vector store (engine/src/memory)
                    +-- Tool dispatcher (engine/src/tool)
                    +-- Metrics/logging/interceptors (engine/core)
                    |
                    v
             LLM providers, relay, tools, files, workspace, storage
```

### 2.2 Pernyataan Risiko Arsitektur (Architecture Risk Statement)
Sistem saat ini rentan memiliki banyak sumber state runtime independen:
- State chat dan terminal Rust.
- State kru, task, agen, dan run Go.
- State proses sidecar Tauri.
- State polling dan local React state pada Web UI.
- State event stream HTTP/gRPC.
- State request provider LLM.
- State in-memory vector store.
- State eksekusi dan approval tool.

Cacat terjadi saat representasi-representasi ini tidak tersinkronisasi, tidak memiliki pemilik kanonikal yang jelas, atau bereaksi secara berbeda terhadap pembatalan (*cancellation*), kegagalan, rekoneksi jaringan, restart proses, atau pengulangan (*retry*).

---

## 3. Klasifikasi Tingkat Keparahan (Severity)

| Tingkat | Makna & Dampak | Respon yang Diperlukan |
|---|---|---|
| **P0 / Critical** | Bypass batasan keamanan, korupsi data, eksekusi tool liar tanpa izin, kebocoran resource fatal, crash sistem meluas. | Wajib diinvestigasi dan diselesaikan sebelum fitur diaktifkan atau dirilis. |
| **P1 / High** | Perilaku salah yang terlihat oleh pengguna, eksekusi ganda, kehilangan progres, state basi (*stale*), kegagalan integrasi besar. | Wajib diperbaiki sebelum rilis stabil (*stable release*). |
| **P2 / Medium** | Inkonsistensi yang dapat dipulihkan, penurunan kualitas UI, metrik tidak akurat, masalah performa terisolasi. | Dijadwalkan pada fase pengerasan (*hardening phase*) berikutnya. |
| **P3 / Low** | Kasus batas (*edge case*), cacat kosmetik visual, celah observabilitas non-kritis. | Dicatat dan diselesaikan sesuai prioritas praktis. |

---

## 4. Master Register: Daftar 20 Potensi Bug

Seluruh rincian bug dikelompokkan ke dalam kategori file terpisah di dalam direktori `docs/refactoring/bug/`:

| Bug ID | Judul / Masalah | Severity | Kategori Dokumen |
|---|---|---|---|
| [**BUG-001**](./concurrency-streaming.md#bug-001--streamgoroutine-leak-after-client-disconnect) | Stream/Goroutine Leak After Client Disconnect | **P0 / Critical** | [`concurrency-streaming.md`](./concurrency-streaming.md) |
| [**BUG-002**](./concurrency-streaming.md#bug-002--cancellation-does-not-reach-llm-tool-or-sub-agent-tasks) | Cancellation Does Not Reach LLM, Tool, or Sub-Agent Tasks | **P0 / Critical** | [`concurrency-streaming.md`](./concurrency-streaming.md) |
| [**BUG-003**](./concurrency-streaming.md#bug-003--concurrent-runtaskagent-state-race) | Concurrent Run/Task/Agent State Race | **P0 / Critical** | [`concurrency-streaming.md`](./concurrency-streaming.md) |
| [**BUG-004**](./memory-persistence.md#bug-004--in-memory-vector-store-race-corruption-or-incorrect-query-results) | In-Memory Vector Store Race, Corruption, or Incorrect Query Results | **P0 / Critical** | [`memory-persistence.md`](./memory-persistence.md) |
| [**BUG-005**](./security-governance.md#bug-005--tool-dispatcher-bypasses-approval-authorization-or-workspace-policy) | Tool Dispatcher Bypasses Approval, Authorization, or Workspace Policy | **P0 / Critical** | [`security-governance.md`](./security-governance.md) |
| [**BUG-006**](./cross-layer-contracts.md#bug-006--event-duplication-event-loss-or-out-of-order-streaming) | Event Duplication, Event Loss, or Out-of-Order Streaming | **P1 / High** | [`cross-layer-contracts.md`](./cross-layer-contracts.md) |
| [**BUG-007**](./memory-persistence.md#bug-007--ephemeral-run-task-and-memory-state-is-lost-after-engine-restart) | Ephemeral Run, Task, and Memory State Is Lost After Engine Restart | **P1 / High** | [`memory-persistence.md`](./memory-persistence.md) |
| [**BUG-008**](./cross-layer-contracts.md#bug-008--go-grpc-http-tauri-rust-and-typescript-contract-drift) | Go, gRPC, HTTP, Tauri, Rust, and TypeScript Contract Drift | **P1 / High** | [`cross-layer-contracts.md`](./cross-layer-contracts.md) |
| [**BUG-009**](./cross-layer-contracts.md#bug-009--polling-and-streaming-cause-duplicate-or-stale-ui-state) | Polling and Streaming Cause Duplicate or Stale UI State | **P1 / High** | [`cross-layer-contracts.md`](./cross-layer-contracts.md) |
| [**BUG-010**](./platform-lifecycle.md#bug-010--major-frontend-dependency-upgrade-regression) | Major Frontend Dependency Upgrade Regression | **P1 / High** | [`platform-lifecycle.md`](./platform-lifecycle.md) |
| [**BUG-011**](./platform-lifecycle.md#bug-011--unit-tests-pass-but-cross-layer-integration-fails) | Unit Tests Pass but Cross-Layer Integration Fails | **P1 / High** | [`platform-lifecycle.md`](./platform-lifecycle.md) |
| [**BUG-012**](./cross-layer-contracts.md#bug-012--error-translation-hides-engine-failures-as-empty-or-normal-state) | Error Translation Hides Engine Failures as Empty or Normal State | **P1 / High** | [`cross-layer-contracts.md`](./cross-layer-contracts.md) |
| [**BUG-013**](./cross-layer-contracts.md#bug-013--canonical-state-is-duplicated-between-rust-and-go) | Canonical State Is Duplicated Between Rust and Go | **P1 / High** | [`cross-layer-contracts.md`](./cross-layer-contracts.md) |
| [**BUG-014**](./concurrency-streaming.md#bug-014--invalid-or-non-monotonic-state-transitions) | Invalid or Non-Monotonic State Transitions | **P1 / High** | [`concurrency-streaming.md`](./concurrency-streaming.md) |
| [**BUG-015**](./concurrency-streaming.md#bug-015--recovery-console-actions-race-with-live-agent-execution) | Recovery Console Actions Race With Live Agent Execution | **P1 / High** | [`concurrency-streaming.md`](./concurrency-streaming.md) |
| [**BUG-016**](./platform-lifecycle.md#bug-016--metrics-are-double-counted-during-retry-reconnect-or-late-event-delivery) | Metrics Are Double-Counted During Retry, Reconnect, or Late Event Delivery | **P2 / Medium** | [`platform-lifecycle.md`](./platform-lifecycle.md) |
| [**BUG-017**](./security-governance.md#bug-017--logs-traces-and-error-events-may-expose-secrets-or-sensitive-prompts) | Logs, Traces, and Error Events May Expose Secrets or Sensitive Prompts | **P1 / High** | [`security-governance.md`](./security-governance.md) |
| [**BUG-018**](./platform-lifecycle.md#bug-018--sidecar-startup-shutdown-and-restart-ordering-failure) | Sidecar Startup, Shutdown, and Restart Ordering Failure | **P1 / High** | [`platform-lifecycle.md`](./platform-lifecycle.md) |
| [**BUG-019**](./memory-persistence.md#bug-019--unbounded-event-log-artifact-or-memory-growth) | Unbounded Event, Log, Artifact, or Memory Growth | **P2 / Medium** | [`memory-persistence.md`](./memory-persistence.md) |
| [**BUG-020**](./platform-lifecycle.md#bug-020--incomplete-cross-platform-coverage) | Incomplete Cross-Platform Coverage | **P2 / Medium** | [`platform-lifecycle.md`](./platform-lifecycle.md) |

---

## 5. Navigasi & Hubungan Antar Dokumen

- [**`concurrency-streaming.md`**](./concurrency-streaming.md): Membahas kebocoran goroutine, propagasi pembatalan, race condition status eksekusi, serta state machine transisi status (BUG-001, BUG-002, BUG-003, BUG-014, BUG-015).
- [**`security-governance.md`**](./security-governance.md): Membahas otorisasi tool, bypass approval gate, path traversal sandbox, dan pencegahan kebocoran data rahasia pada log/event (BUG-005, BUG-017).
- [**`memory-persistence.md`**](./memory-persistence.md): Membahas konkurensi SIMD vector store, isolasi ruang data multi-tenant, kehilangan state saat restart sidecar, dan pembatasan pertumbuhan buffer memori (BUG-004, BUG-007, BUG-019).
- [**`cross-layer-contracts.md`**](./cross-layer-contracts.md): Membahas urutan dan deduplikasi event SSE, drift DTO antar-bahasa, konflik polling vs streaming, masking error, dan duplikasi state kanonikal Rust vs Go (BUG-006, BUG-008, BUG-009, BUG-012, BUG-013).
- [**`platform-lifecycle.md`**](./platform-lifecycle.md): Membahas regresi upgrade dependensi frontend, celah pengujian integrasi lintas-lapisan, metrik dobel, siklus hidup sidecar Tauri, dan kompatibilitas lintas-sistem operasi (BUG-010, BUG-011, BUG-016, BUG-018, BUG-020).
- [**`verification-testing.md`**](./verification-testing.md): Perintah verifikasi langsung (Go, Rust, Web), skenario uji end-to-end terpadu (Scenario A s.d. G), dan rekomendasi penambahan test suite.
- [**`action-backlog.md`**](./action-backlog.md): Backlog tindakan mitigasi terprioritisasi (P0, P1, P2), matriks kepemilikan kapabilitas, dan checklist *Definition of Done* per perbaikan bug.

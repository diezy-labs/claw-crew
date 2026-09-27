# Task Breakdown & Implementation Plan
## ClawCrew Hybrid Refactoring

Pemecahan *tasks* ini dibagi berdasarkan siklus (phase) kerja agar perpindahan dapat dilakukan bertahap tanpa mematikan fitur utama ClawCrew.

### Phase 1: Scaffolding, Protobuf & Core Infrastructure
- [ ] **TASK-1.1**: Buat direktori `engine/` untuk proyek Go dan inisialisasi `go.mod` (Go 1.27).
- [ ] **TASK-1.2**: Implementasi *Clean Architecture Modular* (direktori `src/`, `core/`).
- [ ] **TASK-1.3**: Buat package `core/errors` untuk standardisasi *error response* per layer.
- [ ] **TASK-1.4**: Setup `core/logger` dengan rotasi file (lumberjack) yang terintegrasi dengan config direktori `%APPDATA%` / `~/.clawcrew/logs`.
- [ ] **TASK-1.5**: Setup **Google Wire** (buat `wire.go`) untuk injeksi dependensi.
- [ ] **TASK-1.6**: Buat file kontrak `proto/agent_service.proto` berdasarkan *API Spec*.
- [ ] **TASK-1.7**: Konfigurasi auto-generate Protobuf untuk Rust (`tonic-build`) dan Go (`protoc-gen-go`).

### Phase 2: Core Go Engine (The Brain) & Observability
- [ ] **TASK-2.1**: Buat *entrypoint* `engine/cmd/agent-engine/main.go` yang me-*load* dependency via Wire dan mendengarkan port gRPC lokal.
- [ ] **TASK-2.2**: Ekspos HTTP server tambahan di port `:9090/metrics` untuk metrik **Prometheus**.
- [ ] **TASK-2.3**: Buat domain `src/crew` (Delivery, Service, Interfaces). Implementasi infrastruktur `Goroutines` & `Channels` untuk manajemen agen.
- [ ] **TASK-2.4**: Implementasikan interseptor gRPC untuk melakukan *logging error* tersentral dan meneruskan metrik *request duration* ke Prometheus.

### Phase 3: Rust Tauri Sidecar Wiring & UI Observability
- [ ] **TASK-3.1**: Modifikasi `clawcrew-runtime/daemon/mod.rs` untuk *spawn* binary `agent-engine` saat *startup* Tauri, serta membunuh (*graceful shutdown*) jika Tauri ditutup.
- [ ] **TASK-3.2**: Sambungkan gRPC client `tonic` di Rust Gateway API untuk rute `POST /api/chat/turn`.
- [ ] **TASK-3.3**: Tambahkan *endpoint* UI Dashboard/React untuk menarik metrik dari HTTP `:9090` Go dan me-render *log viewer* dari file `agent.log`.

### Phase 4: Integrasi LLM & Tool Dispatching
- [ ] **TASK-4.1**: Buat domain `src/llm` (memanfaatkan `encoding/json/v2`) untuk pemanggilan OpenAI/Gemini/Anthropic API.
- [ ] **TASK-4.2**: Implementasikan *dispatcher* dalam *Service* layer untuk memisahkan hasil *streaming text* murni dan *tool calls*.
- [ ] **TASK-4.3**: Integrasi `SystemGateway` gRPC (Go ke Rust) agar Go dapat mengeksekusi alat bawaan mesin (OS bash, read_file).

### Phase 5: Rilis & Pengujian Akhir
- [ ] **TASK-5.1**: Lakukan kompilasi *end-to-end* (Tauri Desktop App) membundel `agent-engine` executable.
- [ ] **TASK-5.2**: Pengujian *stress test* memori untuk puluhan agen berjalan paralel.
- [ ] **TASK-5.3**: Verifikasi logs file rotasi dan Prometheus Dashboard metrics di bawah beban tinggi.

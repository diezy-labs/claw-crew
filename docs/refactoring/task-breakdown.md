# Task Breakdown & Implementation Plan
## ClawCrew Hybrid Refactoring

Pemecahan *tasks* ini dibagi berdasarkan siklus (phase) kerja agar perpindahan dapat dilakukan bertahap tanpa mematikan fitur utama ClawCrew.

### Phase 1: Scaffolding & Protobuf Setup
- [ ] **TASK-1.1**: Buat direktori `engine/` di root repo untuk proyek Go.
- [ ] **TASK-1.2**: Inisialisasi `go.mod` (Target: Go 1.27) di dalam `engine/`.
- [ ] **TASK-1.3**: Buat file kontrak `proto/agent_service.proto` berdasarkan *API Spec*.
- [ ] **TASK-1.4**: Konfigurasi *build script* (contoh: `Makefile` atau `build.rs` di Rust) untuk melakukan auto-generate kode gRPC untuk Rust (`tonic-build`) dan Go (`protoc-gen-go`).

### Phase 2: Core Go Engine (The Brain)
- [ ] **TASK-2.1**: Buat *entrypoint* `engine/cmd/agent-engine/main.go` yang mendengarkan port gRPC lokal.
- [ ] **TASK-2.2**: Implementasikan `AgentEngine` service (membuat *dummy streaming response* terlebih dahulu untuk menguji koneksi IPC).
- [ ] **TASK-2.3**: Buat infrastruktur `Goroutines` & `Channels` di `engine/internal/crew/` untuk manajemen siklus hidup agen (Start, Pause, Terminate).

### Phase 3: Rust Tauri Sidecar Wiring
- [ ] **TASK-3.1**: Modifikasi `clawcrew-runtime/daemon/mod.rs` untuk secara otomatis menjalankan (spawn) binary `agent-engine` sebagai *child process* saat *startup* Tauri.
- [ ] **TASK-3.2**: Pastikan *child process* Go Engine dimatikan dengan bersih (*graceful shutdown*) jika aplikasi Tauri ditutup.
- [ ] **TASK-3.3**: Sambungkan gRPC client `tonic` di Rust Gateway API untuk meneruskan HTTP request `POST /api/chat/turn` dari Frontend React ke Go Engine.

### Phase 4: Integrasi LLM (Go 1.27) & Pemindahan Logika
- [ ] **TASK-4.1**: Tulis modul `engine/internal/llm/` yang melakukan API HTTP Call ke OpenAI/Gemini/Anthropic menggunakan `encoding/json/v2`.
- [ ] **TASK-4.2**: Implementasikan dispatcher untuk memisahkan *streaming text* dan *tool calls*.
- [ ] **TASK-4.3**: Panggil `SystemGateway` gRPC client (dari Go kembali ke Rust) untuk mengeksekusi *native tool* (bash, read_file) jika diminta oleh LLM.

### Phase 5: Rilis & Pengujian Akhir
- [ ] **TASK-5.1**: Lakukan kompilasi *end-to-end* (Tauri Desktop App). Pastikan installer (`.exe`/`.msi`) berhasil membundel binary Go `agent-engine`.
- [ ] **TASK-5.2**: Pengujian stabilitas saat menjalankan puluhan Sub-agent (Crew) yang bekerja secara simultan tanpa *memory leak* atau blokir pada UI.

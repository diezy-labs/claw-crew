# Task Breakdown & Implementation Plan
## ClawCrew Hybrid Refactoring

Dokumen ini berisi daftar pekerjaan (tasks) yang diurutkan dari yang **termudah** (setup awal) hingga yang **tersulit** (logika inti dan integrasi end-to-end). Silakan tandai *checkbox* `[ ]` menjadi `[x]` saat pekerjaan telah diselesaikan.

---

### Phase 1: Scaffolding, Protobuf & Core Infrastructure (Termudah)
Pekerjaan *setup* dan konfigurasi kerangka dasar proyek.

- [x] **TASK-1.1**: Buat direktori `engine/` untuk proyek Go dan inisialisasi `go.mod` (Go 1.27).
- [x] **TASK-1.2**: Implementasi *Clean Architecture Modular* (buat kerangka direktori `src/`, `core/`).
- [x] **TASK-1.3**: Buat package `core/errors` untuk standardisasi *error response* per layer.
- [x] **TASK-1.4**: Setup `core/logger` dengan rotasi file (lumberjack) yang terintegrasi dengan config direktori `%APPDATA%` / `~/.clawcrew/logs`.
- [x] **TASK-1.5**: Buat file kontrak `proto/agent_service.proto` berdasarkan *API Spec*.
- [x] **TASK-1.6**: Konfigurasi skrip auto-generate Protobuf untuk Rust (`tonic-build`) dan Go (`protoc-gen-go`).

### Phase 2: Observability & DI Wiring (Menengah - Mudah)
Pemasangan pustaka pendukung untuk Dependency Injection dan Metrics.

- [x] **TASK-2.1**: Setup **Google Wire** (buat `wire.go`) untuk injeksi dependensi awal di tingkat `app`.
- [x] **TASK-2.2**: Ekspos HTTP server tambahan di port `:9090/metrics` untuk metrik **Prometheus**.
- [x] **TASK-2.3**: Buat *entrypoint* `engine/cmd/agent-engine/main.go` yang me-*load* dependency via Wire dan mendengarkan port gRPC lokal.
- [x] **TASK-2.4**: Implementasikan interseptor gRPC untuk *logging error* tersentral dan pencatatan metrik *request duration* (Prometheus).

### Phase 3: Rust Tauri Sidecar Wiring (Menengah)
Pekerjaan di area perbatasan Rust (Desktop) dan Go (Daemon).

- [ ] **TASK-3.1**: Modifikasi `clawcrew-runtime/daemon/mod.rs` untuk *spawn* binary `agent-engine` saat *startup* Tauri, serta membunuhnya secara bersih (*graceful shutdown*) jika Tauri ditutup.
- [ ] **TASK-3.2**: Sambungkan gRPC client `tonic` di Rust Gateway API untuk rute frontend `POST /api/chat/turn`.
- [ ] **TASK-3.3**: Tambahkan fungsionalitas UI Dashboard/React untuk menarik metrik dari HTTP `:9090` Go dan me-render *log viewer* dari file `agent.log`.

### Phase 4: Core Go Engine & LLM Integration (Sulit)
Implementasi logika *multi-agent* dan *streaming LLM* pada Go.

- [ ] **TASK-4.1**: Buat domain `src/llm` (memanfaatkan `encoding/json/v2`) untuk pemanggilan OpenAI/Gemini/Anthropic API secara *streaming*.
- [ ] **TASK-4.2**: Buat domain `src/crew` (Delivery, Service, Interfaces). Implementasi infrastruktur `Goroutines` & `Channels` untuk manajemen agen paralel.
- [ ] **TASK-4.3**: Implementasikan *dispatcher* dalam *Service* layer untuk memisahkan dan mem-parsing hasil *streaming text* murni dengan *tool calls*.
- [ ] **TASK-4.4**: Integrasi `SystemGateway` gRPC (Go memanggil Rust) agar LLM Go dapat mengeksekusi alat bawaan mesin yang diamankan Rust (OS bash, read_file).

### Phase 5: Rilis & Pengujian Akhir (Tersulit)
Pengujian dan penyatuan seluruh infrastruktur.

- [ ] **TASK-5.1**: Lakukan kompilasi *end-to-end* (Tauri Desktop App) membundel `agent-engine` executable sebagai *Sidecar*.
- [ ] **TASK-5.2**: Pengujian *stress test* memori untuk puluhan agen berjalan paralel, pastikan *channels* dan *goroutines* tidak mengalami *deadlock/leak*.
- [ ] **TASK-5.3**: Verifikasi ketahanan file *logs* (rotasi) dan pengiriman metrik Prometheus di bawah beban tinggi (ribuan RPS lokal).

---

## Coding Standards & Go Idioms
Selama implementasi tugas-tugas di atas, semua kode Go **harus** mematuhi panduan kualitas (*global steering/skills*) SonarQube & idiom Go berikut:

1. **Context Passing**: Context (`ctx`) **tidak boleh** disimpan di dalam struktur (*struct fields*). Selalu teruskan `context.Context` sebagai argumen pertama di *signature* fungsi. Selalu gunakan `defer cancel()` setelah memanggil context yang memiliki timeout/cancel.
2. **Error Handling**: Jangan biarkan fungsi mengembalikan error kosong/tersembunyi. Tangkap error (`err`), dan jika berasal dari *repository* atau sistem eksternal, bungkus menggunakan custom error di layer `core/errors` sebelum dilempar ke atas.
3. **Defer Resource Closing**: Saat membuka HTTP Response Body atau Database Transaction, selalu *defer* penutupannya tepat setelah inisiasi berhasil (`defer resp.Body.Close()`, `defer tx.Rollback()`).
4. **Interface Naming (Idiom)**: Antarmuka (*interface*) dengan satu *method* harus diberi akhiran `-er` (contoh: `TurnStarter`, `AgentOrchestrator`).
5. **Simplicity (Cognitive Complexity)**: Hindari perulangan tersarang (*nested loops*) yang dalam (maksimal 3 level). Jika *switch-case* atau `if` terlalu panjang, *extract* menjadi fungsi-fungsi utilitas kecil.
6. **Concurrency Safety**: Jangan menggunakan *Busy waiting loops* (perulangan kosong menunggu kondisi). Selalu manfaatkan `sync.WaitGroup` dan `channels` standar Go untuk sinkronisasi sub-agen.
7. **Secrets**: Kredensial tidak boleh *hard-coded*. Semua token dan kunci harus dibaca dari *Config Vault* (Rust) yang dilempar via argumen atau di-*request* melalui IPC.

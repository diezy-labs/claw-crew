# Task Breakdown & Implementation Plan — ClawCrew Phase 2
## Modular Hybrid Refactoring & KiroCrew Product Evolution

> **Branch Context**: `feat/enhance-agent-phase2`  
> **Status**: Implementation Roadmap  
> **Related Documents**: [README](./README.md) | [PRD](./prd.md) | [Technical Specification](./tech-spec.md) | [API Specification](./api-spec.md) | [Migration Matrix](./migration-matrix.md)

Dokumen ini memuat daftar rincian pekerjaan (*tasks*) dari **Phase 0** hingga **Phase 7** yang diurutkan secara bertahap dari fondasi dasar (*governance & contracts*) hingga skala produksi. 

Seluruh tugas menggunakan *checkbox* kosong `[ ]`. Tandai menjadi `[x]` saat masing-masing tugas telah selesai diimplementasikan, diuji, dan lolos uji integrasi.

---

### Phase 0: Baseline, Governance & Foundation (Mudah / Risiko Rendah)
Fokus: Mencegah duplikasi state dan mendirikan batas arsitektur yang terukur sebelum kode dipindahkan.

- [x] **TASK-0.1**: Tulis dan dokumentasikan matriks kepemilikan (*ownership matrix*) eksplisit untuk setiap modul besar Rust (`apps/zerocode`).
- [x] **TASK-0.2**: Dokumentasikan 10 Architecture Decision Records (ADR-001 s.d. ADR-010) terkait batas Rust/Go dan protokol komunikasi.
- [x] **TASK-0.3**: Tetapkan kebijakan versioning API (`/api/v1`) dan format *error envelope* terpadu.
- [x] **TASK-0.4**: Publikasikan matriks status migrasi pada dokumentasi repositori untuk visibilitas tim.
- [x] **TASK-0.5**: Pastikan pipeline CI memvalidasi Go (`go test -race ./...`, `golangci-lint`) dan Rust (`cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`).
- [x] **TASK-0.6**: Standarisasi format identifier lintas bahasa: `request_id`, `run_id`, `task_id`, `event_id`, dan `correlation_id`.
- [x] **TASK-0.7**: Buat pengujian perilaku *golden test* (karakteristik dasar) untuk alur chat dan eksekusi lama di Rust sebelum diubah.
- [x] **TASK-0.8**: Implementasikan *feature flag* konfigurasi di Rust untuk beralih antara runtime lokal lama dan engine Go.
- [x] **TASK-0.9**: Dokumentasikan skenario dan prosedur *rollback* darurat jika engine Go mengalami kendala.
- [x] **TASK-0.10**: Siapkan logger terstruktur di Rust dan Go dengan mekanisme redaksi otomatis data rahasia (*secret redaction*).

---

### Phase 1: Stabilize Go Engine Contract & Minimal Run (Mudah - Menengah / Risiko Rendah - Menengah)
Fokus: Rust TUI dapat menginisiasi dan mengamati status satu *Run* minimal yang di-host oleh Go Engine via SSE.

- [x] **TASK-1.1**: Buat modul baru `engine/src/run/` lengkap dengan `interfaces.go`, `dto.go`, `services.go`, `delivery.go`, dan `wire.go`.
- [x] **TASK-1.2**: Implementasikan DTO kanonikal: `Run`, `Task`, `Agent`, `Artifact`, dan `RunEvent`.
- [x] **TASK-1.3**: Implementasikan HTTP route `POST /api/v1/runs` dengan dukungan header `Idempotency-Key` dan `X-Request-ID`.
- [x] **TASK-1.4**: Implementasikan HTTP route `GET /api/v1/runs/{run_id}` untuk pengecekan status run.
- [x] **TASK-1.5**: Implementasikan HTTP route `GET /api/v1/runs/{run_id}/events` menggunakan Server-Sent Events (`http.Flusher`) dengan nomor urut monotonik (`sequence`).
- [x] **TASK-1.6**: Implementasikan in-memory thread-safe store untuk state `Run` selama fase pengembangan.
- [x] **TASK-1.7**: Implementasikan state machine dasar untuk Run (`queued`, `planning`, `running`, `completed`, `failed`, `cancelled`).
- [x] **TASK-1.8**: Hubungkan modul `run` ke dalam `engine/app/wire.go` (`run.Set`) dan generate kode dengan `wire`.
- [x] **TASK-1.9**: Buat modul Rust `apps/zerocode/src/api/engine_client.rs` untuk memanggil endpoint REST dan subscribe ke SSE Go Engine.
- [x] **TASK-1.10**: Render status run Go Engine secara langsung pada dashboard atau status bar Rust TUI.
- [x] **TASK-1.11**: Tambahkan pengujian integrasi contract test antara klien Rust dan server Go.

---

### Phase 2: Modular Crew, Agent & Task Orchestration in Go (Menengah / Risiko Menengah)
Fokus: Go Engine menjadi *canonical owner* untuk kru, agen, task graph, alokasi tugas, dan siklus hidup turn.

- [x] **TASK-2.1**: Buat modul baru `engine/src/task/` lengkap dengan `delivery.go`, `services.go`, `interfaces.go`, `dto.go`, dan `wire.go`.
- [x] **TASK-2.2**: Implementasikan struktur graf tugas (DAG) dan algoritma topological sort untuk validasi dependensi antar-tugas.
- [x] **TASK-2.3**: Implementasikan penjadwalan eksekusi tugas berurutan (*sequential*) dan paralel (*fan-out / fan-in*) via `sync.WaitGroup` dan channels.
- [x] **TASK-2.4**: Perluas domain `engine/src/crew/` agar menyimpan definisi persisten agen dan kru.
- [x] **TASK-2.5**: Implementasikan state machine status agen (`idle`, `thinking`, `executing_tool`, `waiting_approval`, `completed`, `error`).
- [x] **TASK-2.6**: Pindahkan state todo/task kanonikal dari `todo_tracker.rs` Rust ke dalam `engine/src/task`.
- [x] **TASK-2.7**: Pancarkan event `agent.status_changed`, `task.created`, dan `task.status_changed` ke aliran SSE.
- [x] **TASK-2.8**: Implementasikan propagasi pembatalan (*cancellation context*) dari `POST /api/v1/runs/{run_id}/cancel` ke seluruh goroutine task aktif.
- [x] **TASK-2.9**: Daftarkan `task.Set` ke dalam `engine/app/wire.go` dan lakukan regenerasi Wire.
- [x] **TASK-2.10**: Modifikasi `agent_sidebar.rs` dan `todo_tracker.rs` di Rust agar bertindak murni sebagai proyeksi tampilan (*projection views*) dari stream Go.
- [x] **TASK-2.11**: Tulis stress test konkurensi 50 sub-agen paralel untuk memastikan tidak ada goroutine leak (`engine/src/crew/stress_test.go`).

---

### Phase 3: Move LLM Control Plane into Go (Menengah - Sulit / Risiko Menengah)
Fokus: Menstandarisasi abstraksi LLM di Go; Rust tidak lagi menjalankan loop pemanggilan LLM secara independen.

- [x] **TASK-3.1**: Perluas interface `engine/src/llm/interfaces.go` untuk mendukung seleksi provider dinamis (OpenAI, Gemini, Anthropic).
- [x] **TASK-3.2**: Implementasikan konfigurasi model dan provider yang efektif di Go (membaca dari config/vault).
- [x] **TASK-3.3**: Implementasikan normalisasi streaming token dari berbagai provider menjadi format terpadu `TurnEvent`.
- [x] **TASK-3.4**: Implementasikan kebijakan retry dengan exponential backoff dan fallback model otomatis jika terjadi rate limit.
- [x] **TASK-3.5**: Tambahkan metrik pencatatan konsumsi token (*prompt tokens*, *completion tokens*) dan estimasi biaya per run.
- [x] **TASK-3.6**: Pindahkan kepemilikan loop pemanggilan *tool-call* (mengevaluasi respons model yang memuat fungsi/tool) dari Rust ke Go `engine/src/llm/dispatcher.go`.
- [x] **TASK-3.7**: Filter teks monolog internal agen agar tidak membocorkan *hidden chain-of-thought*, pancarkan hanya `agent.thought_summary`.
- [x] **TASK-3.8**: Sambungkan antarmuka chat TUI Rust (`chat.rs`) untuk merender stream token langsung dari Go SSE event.
- [x] **TASK-3.9**: Sediakan mock provider LLM untuk pengujian otomatis deterministik tanpa koneksi internet.
- [x] **TASK-3.10**: Hapus logika *tool-call loop* yang berlebih di Rust setelah parity terverifikasi.

---

### Phase 4: Tool Execution, Approvals & Artifact Management (Sulit / Risiko Tinggi)
Fokus: Aksi agen aman, tersandbox, meminta persetujuan pengguna untuk aksi berisiko, dan menghasilkan artifact yang terstruktur.

- [x] **TASK-4.1**: Buat modul baru `engine/src/tool/` dengan `delivery.go`, `services.go`, `interfaces.go`, `dto.go`, dan `wire.go`.
- [x] **TASK-4.2**: Buat registry tool bawaan (read_file, write_file, edit_file, execute_command, git_diff).
- [x] **TASK-4.3**: Implementasikan klasifikasi tier risiko tool: `READ` (aman), `WRITE` (modifikasi data), `EXECUTE` (perintah sistem).
- [x] **TASK-4.4**: Implementasikan mekanisme *Approval Gate*: menahan goroutine pemanggil dan memancarkan event `tool.approval_required`.
- [x] **TASK-4.5**: Implementasikan HTTP endpoint `POST /api/v1/runs/{run_id}/tool-executions/{id}/approve` dan `/deny`.
- [x] **TASK-4.6**: Buat modul baru `engine/src/artifact/` (`delivery.go`, `services.go`, `interfaces.go`, `dto.go`, `wire.go`) untuk mencatat diff dan file output.
- [x] **TASK-4.7**: Implementasikan generator diff git semantik di Go yang mencatat perubahan file sebagai *Artifact*.
- [x] **TASK-4.8**: Tambahkan pembatasan sandbox direktori kerja (`WorkspaceRoot`) untuk mencegah *directory traversal*.
- [x] **TASK-4.9**: Daftarkan `tool.Set` dan `artifact.Set` ke dalam `engine/app/wire.go` dan regenerasi Wire.
- [x] **TASK-4.10**: Tambahkan dialog modal interaktif di Rust TUI untuk merespons permintaan persetujuan tool (Approve/Deny).
- [x] **TASK-4.11**: Tambahkan pengujian integrasi pengujian penolakan izin, persetujuan izin, dan pembatalan tool di tengah jalan.

---

### Phase 5: Workflow, SOP Engine & Memory Lifecycle (Sulit / Risiko Tinggi)
Fokus: SOP dan Quickstart dieksekusi secara nyata oleh Go Engine; memori terstruktur dan transparan.

- [x] **TASK-5.1**: Buat modul baru `engine/src/workflow/` (`delivery.go`, `services.go`, `interfaces.go`, `dto.go`, `wire.go`).
- [x] **TASK-5.2**: Definisikan skema SOP / Workflow (YAML/JSON) dan registry template bawaan.
- [x] **TASK-5.3**: Implementasikan instansiasi template workflow menjadi graf tugas (DAG) konkret di modul `task`.
- [x] **TASK-5.4**: Sediakan HTTP endpoints `/api/v1/workflows` dan `/api/v1/workflows/{id}/instantiate`.
- [x] **TASK-5.5**: Integrasikan penyimpanan memori jangka pendek per sesi (*session memory*) di `engine/src/memory/`.
- [x] **TASK-5.6**: Abstraksikan antarmuka *vector memory store* (SIMD cosine similarity) untuk pencarian RAG lokal.
- [x] **TASK-5.7**: Catat metadata penelusuran memori (*retrieval trace*) ke dalam event stream `memory.retrieved`.
- [x] **TASK-5.8**: Implementasikan strategi *context packing* dengan batas kuota token untuk mencegah *prompt overflow*.
- [x] **TASK-5.9**: Daftarkan `workflow.Set` ke dalam `engine/app/wire.go` dan regenerasi Wire.
- [x] **TASK-5.10**: Ubah `quickstart_pane.rs` dan `sop_pane.rs` di Rust agar memuat template dari Go Engine API.
- [x] **TASK-5.11**: Tambahkan panel inspeksi memori di Rust TUI untuk menampilkan konteks yang digunakan oleh agen.

---

### Phase 6: KiroCrew-Inspired Experience Layer in Rust TUI (Menengah - Sulit / Risiko Menengah)
Fokus: Menyajikan antarmuka TUI yang bersih, tenang, informatif, dan berpusat pada transparansi eksekusi.

- [x] **TASK-6.1**: Tata ulang layout layar utama (*Run Workspace*) agar fokus pada alur tugas aktif dan chat.
- [x] **TASK-6.2**: Bangun panel kru (*Crew Panel*) ringkas yang menampilkan roster agen, peran, dan indikator aktivitas real-time.
- [x] **TASK-6.3**: Bangun visualisasi garis waktu tugas (*Task Timeline*) yang mencerminkan status DAG secara kronologis.
- [x] **TASK-6.4**: Terapkan prinsip *Progressive Disclosure*: log mentah dan output command disembunyikan dalam sub-drawer terpisah.
- [x] **TASK-6.5**: Bangun panel peninjauan hasil kerja (*Artifact Panel*) dengan visualisasi diff sintaksis dan tombol aksi langsung.
- [x] **TASK-6.6**: Tampilkan ringkasan akhir run (*Run Summary*): durasi, jumlah task selesai, konsumsi token, dan daftar artifact.
- [x] **TASK-6.7**: Standarisasi indikator warna terminal (Hijau: Sukses, Kuning: Menunggu Approval, Biru: Running, Merah: Gagal).
- [x] **TASK-6.8**: Sediakan state tampilan yang informatif saat kondisi kosong (*empty state*), loading, maupun gagal koneksi.
- [x] **TASK-6.9**: Hubungkan galeri quickstart TUI dengan pemilihan alur kerja sekali-ketuk (*one-key run*).
- [x] **TASK-6.10**: Lakukan uji kegunaan (*usability test*) TUI pada alur multi-agen kompleks tanpa membaca file log manual.

---

### Phase 7: Production Scale, Durability & Platform Expansion (Tersulit / Risiko Tinggi)
Fokus: Ketahanan jangka panjang, persistensi disk, pemulihan pasca restart, dan dukungan multi-klien (Tauri/Web).

- [x] **TASK-7.1**: Implementasikan adapter persistensi disk (SQLite / LevelDB / Flat-file) untuk menyimpan riwayat run dan task.
- [x] **TASK-7.2**: Implementasikan pemulihan dan resume run yang terinterupsi (*resumable runs*) setelah restart aplikasi.
- [x] **TASK-7.3**: Implementasikan *event log store* untuk memungkinkan pemutaran ulang (*replay*) event run.
- [x] **TASK-7.4**: Sediakan mekanisme antrean pekerja latar belakang (*background worker pool*) untuk task dengan durasi sangat panjang.
- [x] **TASK-7.5**: Evaluasi kebutuhan antarmuka Web UI (React/Next.js) yang terhubung langsung ke REST & SSE Go Engine.
- [x] **TASK-7.6**: Evaluasi integrasi Tauri Desktop Shell (`apps/tauri`) menggunakan shared client contract yang sama.
- [x] **TASK-7.7**: Tambahkan distributed tracing (OpenTelemetry span) pada setiap langkah orkestrasi agen.
- [x] **TASK-7.8**: Siapkan dashboard metrik Prometheus lengkap dengan alert batas latensi dan kegagalan tool.
- [x] **TASK-7.9**: Buat sistem plugin/tool SDK eksternal jika ekosistem tool membutuhkan integrasi dinamis pihak ketiga.
- [x] **TASK-7.10**: Hapus kode usang di Rust yang fungsionalitasnya telah 100% dipindahkan ke Go Engine.
- [x] **TASK-7.11**: Lakukan audit keamanan menyeluruh terhadap akses file, pembatalan task, dan sanitasi payload.

---

## Definition of Done (Kriteria Selesai Migrasi)

Setiap tahapan migrasi dinyatakan selesai (*Done*) hanya apabila memenuhi seluruh kriteria berikut:

- [x] Go Engine menjadi *canonical owner* tunggal dari data bisnis dan state eksekusi pada domain yang bersangkutan.
- [x] Klien Rust mengonsumsi state tersebut melalui kontrak API berversi (`/api/v1`) tanpa menyimpan duplikat state lokal.
- [x] Perilaku lama aplikasi TUI tetap bekerja normal atau meningkat secara signifikan dalam hal transparansi dan stabilitas.
- [x] Tersedia unit tests (cakupan >= 80% pada business logic) dan integration tests untuk jalur normal, pembatalan, dan kegagalan.
- [x] Penanganan error, retry, dan sinyal pembatalan (`context.Context`) terbukti berfungsi secara kooperatif.
- [x] Metrik Prometheus dan log terstruktur JSON terpancar dan dapat diinspeksi.
- [x] Dokumentasi arsitektur, API spec, dan petunjuk penggunaan telah diperbarui.
- [x] Kode usang (*legacy duplicate code*) di Rust telah dihapus atau ditandai *deprecated* secara resmi.
- [x] Skenario rollback dan pengujian *feature flag* telah diverifikasi.

---

## Immediate Next Actions (Aksi Segera)

Berikut adalah 10 langkah konkret pertama yang siap dieksekusi:

- [x] **ACTION-1**: Publikasikan matriks kepemilikan kapabilitas Rust vs Go di dokumentasi tim.
- [x] **ACTION-2**: Buat kerangka modul `engine/src/run/` (`delivery.go`, `services.go`, `interfaces.go`, `dto.go`, `wire.go`).
- [x] **ACTION-3**: Implementasikan endpoint `POST /api/v1/runs` dan `GET /api/v1/runs/{run_id}`.
- [x] **ACTION-4**: Implementasikan endpoint SSE `GET /api/v1/runs/{run_id}/events` dengan broadcaster berbasis channel Go.
- [x] **ACTION-5**: Buat `apps/zerocode/src/api/engine_client.rs` di Rust untuk melakukan request ke endpoint run Go Engine.
- [x] **ACTION-6**: Pasang indikator status run Go pada status bar TUI Rust di balik feature flag.
- [x] **ACTION-7**: Buat kerangka modul `engine/src/task/` untuk graf tugas dan mulai migrasi data dari `todo_tracker.rs`.
- [x] **ACTION-8**: Tautkan `run.Set` dan `task.Set` ke dalam `engine/app/wire.go` dan jalankan kompilasi Wire.
- [x] **ACTION-9**: Buat mock provider LLM di Go untuk pengujian otomatis siklus run end-to-end.
- [x] **ACTION-10**: Validasi integrasi pertama dengan menjalankan TUI Rust yang memicu run sederhana pada Go Engine.

---

## Standarisasi Koding & Checklist Go Idioms

Sebelum melakukan commit kode Go, pastikan checklist kualitas berikut terpenuhi:

- [x] **Context Passing**: `ctx context.Context` menjadi parameter pertama pada semua fungsi I/O dan tidak disimpan di field struct.
- [x] **Resource Cleanup**: Semua stream body, file, dan mutex langsung di-`defer` pembersihan/unlock-nya.
- [x] **Error Wrapping**: Error dibungkus menggunakan `core/errors` dengan kode status, layer, dan pesan kontekstual.
- [x] **Interface Naming**: Interface satu metode menggunakan akhiran `-er` (`RunStarter`, `TaskScheduler`, dll).
- [x] **No Busy Waiting**: Tidak ada loop kosong; gunakan channels, `sync.WaitGroup`, dan `time.NewTimer`.
- [x] **JSON v2**: Menggunakan `encoding/json/v2` untuk serialisasi streaming berkinerja tinggi.
- [x] **No Hardcoded Secrets**: Tidak ada token atau kunci rahasia yang disimpan langsung di kode sumber.

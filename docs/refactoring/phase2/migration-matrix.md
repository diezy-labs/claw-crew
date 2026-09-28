# Migration Analysis, Ownership Matrix & Architecture Decision Records (Phase 2)

> **Branch Context**: `feat/enhance-agent-phase2`  
> **Status**: Approved Strategy Document  
> **Related Documents**: [README](./README.md) | [PRD](./prd.md) | [Technical Specification](./tech-spec.md) | [API Specification](./api-spec.md) | [Task Breakdown](./task-breakdown.md)

---

## 1. Temuan Migrasi Paling Penting (Executive Findings)

Berdasarkan audit mendalam terhadap basis kode repositori saat ini, `apps/zerocode` (Rust) masih menampung sebagian besar logika bisnis eksekusi agen. Sementara itu, `engine/` (Go) telah memiliki arsitektur modular yang sangat rapi (`core/`, `app/`, dan domain `crew`, `llm`, `memory` di bawah `engine/src/`), namun cakupannya belum seluas kebutuhan runtime multi-agen yang sesungguhnya.

Yang perlu dipindahkan **bukan** file fisik Rust satu per satu, melainkan **kepemilikan kapabilitas (capability ownership)**.

### Ringkasan Prioritas Migrasi Kapabilitas:

| Area Kapabilitas | Kondisi Saat Ini di Repositori | Arah & Target Kepemilikan | Tingkat Prioritas |
|---|---|---|---|
| **Crew Orchestration** | Go telah memiliki `engine/src/crew`, namun state agen masih parsial di Rust (`agent_sidebar.rs`). | Go menjadi *single source of truth* untuk kru, agen, status agen, dan penugasan. | **Sangat Tinggi** |
| **Chat & Run Lifecycle** | Dominan di `apps/zerocode/src/chat.rs` dan `app.rs` yang berukuran sangat besar. | Pindahkan siklus hidup *Run*, transisi status turn, retry, cancellation, dan event progress ke `engine/src/run`. Rust hanya merender visualnya. | **Sangat Tinggi** |
| **LLM Execution & Tool Loop**| Domain `engine/src/llm` sudah ada, namun `client.rs` dan `chat.rs` di Rust masih memegang loop pemanggilan tool sendiri. | Pusatkan seleksi provider, retry/fallback, streaming token, token accounting, dan tool-call loop di Go. | **Sangat Tinggi** |
| **Task / Todo Tracker** | Ada `todo_tracker.rs` di Rust dan sebagian task di Go. | Pindahkan state kanonikal task ke `engine/src/task` agar dapat dijadwalkan secara paralel, diulang (*retry*), dan diinspeksi oleh semua klien. | **Sangat Tinggi** |
| **Agent Status / Sidebar** | Dikelola secara visual di `agent_sidebar.rs` Rust. | Status agen wajib bersumber murni dari event stream Go; Rust mempertahankan rendering sidebar. | **Tinggi** |
| **Tool Execution & Approvals**| Belum ada domain eksplisit di Go; eksekusi tersebar di Rust. | Buat domain baru `engine/src/tool` untuk registry tool, evaluasi tier risiko, approval gates, audit log, dan sandboxing. | **Tinggi** |
| **Memory & Context Policy** | Go memiliki `engine/src/memory`, namun TUI masih memiliki context management lokal. | Go menjadi pemilik siklus hidup memori, pencarian kemiripan vektor SIMD, dan context packing. | **Tinggi** |
| **SOP & Quickstart** | Terdapat `sop_pane.rs` dan `quickstart_pane.rs` di Rust. | Rust tetap menjadi antarmuka UI; semantik alur kerja, parsing template, dan instansiasi dipindahkan ke `engine/src/workflow`. | **Tinggi** |
| **Artifacts & Diff Semantics**| Tersebar di `diff.rs`, `attachment.rs`, dan `file_explorer.rs` Rust. | Go mengelola pencatatan output/diff sebagai entitas *Artifact*; Rust mempertahankan rendering visual diff dan file picker lokal. | **Menengah** |
| **Configuration** | `config_manager.rs` di Rust sangat besar. | Pisahkan konfigurasi UI lokal (Rust) vs konfigurasi runtime engine dan provider AI yang efektif (Go). | **Menengah** |
| **Diagnostics & Health** | Ada `doctor.rs` dan `logs.rs` di Rust. | Pisahkan: Go menangani metrik sistem, status daemon, dan health probe; Rust menangani pemeriksaan terminal/OS lokal. | **Menengah** |
| **Terminal & UI Primitives** | `terminal_backend.rs`, `mouse.rs`, `clipboard.rs`, theme, keymaps. | **Tetap 100% di Rust**. Dilarang keras dipindahkan ke Go. | **Tidak Perlu** |

---

## 2. Matriks Kepemilikan Kapabilitas Lengkap (Ownership Matrix)

| Kapabilitas / Fitur | Pemilik Saat Ini | Pemilik Target | Prioritas | Strategi Migrasi |
|---|---|---|---|---|
| **Terminal Rendering (ANSI/Crossterm)** | Rust (`apps/zerocode`) | Rust | Tidak ada | Pertahankan performa native terminal. |
| **Keymaps, Mouse, Clipboard** | Rust | Rust | Tidak ada | Logika interaksi platform lokal. |
| **Chat Viewport & Scroll State** | Rust | Rust | Rendah | State tampilan lokal tetap di Rust. |
| **Run & Turn Execution Lifecycle** | Rust (`chat.rs`) / Campuran | Go (`engine/src/run`) | **Sangat Tinggi** | Pindahkan ke Go; stream status via SSE ke Rust. |
| **LLM Request & Dispatch Loop** | Rust (`client.rs`) / Campuran | Go (`engine/src/llm`) | **Sangat Tinggi** | Pindahkan tool-call loop dan retry ke Go. |
| **Crew & Agent Registry** | Campuran | Go (`engine/src/crew`) | **Sangat Tinggi** | Jadikan Go *single source of truth*. |
| **Agent Status State Machine** | Rust UI / Campuran | Go (`engine/src/crew`) | **Tinggi** | Event Go memicu re-render sidebar Rust. |
| **Task / Todo Graph** | Rust (`todo_tracker.rs`) | Go (`engine/src/task`) | **Sangat Tinggi** | Buat DAG scheduler di Go; Rust hanya memproyeksikan. |
| **Tool Execution & Approvals** | Campuran / Rust | Go (`engine/src/tool`) | **Tinggi** | Sandboxing dan approval channel di Go. |
| **Artifact Metadata & Storage** | Campuran | Go (`engine/src/artifact`) | **Tinggi** | Simpan diff dan file output sebagai entitas Go. |
| **Local File Browser** | Rust (`file_explorer.rs`) | Rust | Tidak ada | Eksplorasi direktori lokal tetap di Rust. |
| **Attachment Selection** | Rust (`attachment.rs`) | Rust (pilih) + Go (ingest) | Menengah | Rust memilih path; Go meng-ingest konten. |
| **Visual Diff Rendering** | Rust (`diff.rs`) | Rust | Tidak ada | Rust merender patch ANSI/split view. |
| **Diff Generation Semantics** | Campuran | Go (`engine/src/artifact`) | Menengah | Go menghitung diff git yang dihasilkan agen. |
| **UI Config & Theme Settings** | Rust (`config_manager.rs`) | Rust | Tidak ada | Pengaturan tema/shortcut lokal di Rust. |
| **Effective Engine Config** | Campuran | Go (`core/config`) | Menengah | Parameter model & runtime di Go. |
| **System Diagnostics** | Campuran (`doctor.rs`) | Split (Go Health + Rust OS) | Menengah | Gabungkan respons `/health` Go dengan check Rust. |
| **SOP / Quickstart UI** | Rust (`quickstart_pane.rs`) | Rust | Rendah | Rust merender kartu galeri template. |
| **SOP / Workflow Semantics** | Rust / Campuran | Go (`engine/src/workflow`) | **Tinggi** | Parsing dan eksekusi DAG dipusatkan di Go. |
| **Memory UI Inspector** | Rust | Rust | Rendah | Rust menampilkan panel pratinjau memori. |
| **Memory & RAG Lifecycle** | Campuran (`memory/`) | Go (`engine/src/memory`) | **Tinggi** | Cosine similarity & context packing di Go. |
| **Relay Protocol Schema** | Rust / Campuran | Shared Protobuf / Schema | Menengah | Hindari duplikasi manual model data. |

---

## 3. Rincian Analisis Pemindahan Rust-ke-Go

### 3.1 Run & Turn Orchestration
- **Area Sumber di Rust**: `chat.rs`, `app.rs`, `todo_tracker.rs`, `agent_sidebar.rs`.
- **Pindahkan ke Go**:
  - Pembuatan entitas Run dan siklus hidupnya (`queued` hingga `completed`/`failed`).
  - Transisi status giliran (*turn progression*).
  - Penanganan sinyal pembatalan (*cancellation handling*).
  - Kebijakan pengulangan tugas (*retry policy*).
  - Alokasi tugas ke agen (*delegation & fan-out/fan-in*).
  - Pencatatan tugas kanonikal (*todo persistence*).
  - Pancaran event eksekusi terstruktur via SSE.
- **Tetap di Rust**:
  - Layout dan rendering panel chat.
  - Penanganan scroll dan virtual scrolling.
  - Input bar dan shortcut keyboard.
  - Buffer teks draft sebelum dikirim.

### 3.2 LLM Request Coordination
- **Area Sumber di Rust**: `chat.rs`, `client.rs`, `client_crypto.rs`, `config_manager.rs`.
- **Pindahkan ke Go**:
  - Seleksi model dan provider (OpenAI, Anthropic, Gemini).
  - Pembentukan payload permintaan (*request construction*).
  - Manajemen loop pemanggilan tool (*tool-call loop*).
  - Pembatasan laju (*rate limiting*) dan exponential backoff.
  - Pencatatan konsumsi token dan estimasi biaya.
  - Normalisasi streaming token ke klien.
- **Tetap di Rust**:
  - Picker pemilihan model di antarmuka TUI.
  - Tampilan indikator model yang aktif.

### 3.3 Agent & Crew Domain State
- **Area Sumber di Rust**: `agent_sidebar.rs`, `chat.rs`, `dashboard.rs`, `sop_pane.rs`.
- **Pindahkan ke Go**:
  - Registry agen dan kru.
  - Definisi peran agen dan prompt kebijakan sistem.
  - State machine status agen (`idle`, `thinking`, `executing_tool`, dll).
  - Log eksekusi dan referensi output agen.
- **Tetap di Rust**:
  - Rendering grafis sidebar agen dan ikon status.
  - Animasi loading dan empty state.

### 3.4 Memory, Retrieval & Context Policy
- **Area Sumber di Rust**: `chat.rs`, `config_manager.rs`, file-file di folder `memory/`.
- **Pindahkan ke Go**:
  - Skema dan persistensi record memori sesi dan jangka panjang.
  - Pipeline pengambilan (*retrieval*) berbasis kemiripan vektor SIMD.
  - Aturan pengemasan konteks (*context packing*) dengan batas kuota token.
  - Kebijakan retensi dan pembersihan memori.
- **Tetap di Rust**:
  - Panel penampil memori di layar TUI.
  - Kontrol manual pengguna untuk menghapus konteks lokal.

### 3.5 Tool Execution & Workspace Operations
- **Area Sumber di Rust**: `chat.rs`, `file_explorer.rs`, `attachment.rs`, `diff.rs`.
- **Pindahkan ke Go**:
  - Registry tool dan evaluasi tier izin.
  - Siklus hidup eksekusi tool dan penahanan alur untuk persetujuan (*approval gate*).
  - Kebijakan pembatasan direktori kerja (*sandboxing*).
  - Semantik pembuatan diff dan pencatatan artifact.
  - Pencatatan jejak audit (*audit log*).
- **Tetap di Rust**:
  - File picker interaktif.
  - Penjelajah tree direktori lokal (*file explorer*).
  - Rendering visual diff berwarna.
  - Pemanggilan editor eksternal lokal (VSCode / Vim / Nano).

### 3.6 Primitives yang Wajib Tetap di Rust (Dilarang Migrasi)
Komponen berikut adalah kekuatan inti Rust untuk antarmuka terminal desktop dan **tidak boleh** dipindahkan ke Go:
- `terminal_backend.rs`, `mouse.rs`, `clipboard.rs`
- `color_depth.rs`, `display_width.rs`, `text_navigation.rs`, `text_selection.rs`
- `theme.rs`, `generated_themes.rs`
- Manajemen tata letak pane dan window lokal
- Internalisasi teks TUI lokal

### 3.7 Larangan Migrasi Go-ke-Rust
Tidak ada fitur inti Go yang boleh dipindahkan kembali ke Rust. Satu-satunya interaksi Go-ke-Rust yang diperbolehkan adalah pemanggilan adapter sistem melalui gRPC `SystemGateway` (misalnya saat tool Go memerlukan eksekusi perintah shell yang diamankan oleh Rust).

---

## 4. Architecture Decision Records (ADR)

### ADR-001: Go adalah Canonical Owner untuk Runtime State Agen
- **Status**: Disetujui
- **Konteks**: Keberadaan state bisnis ganda antara Rust TUI dan Go Engine menyebabkan risiko inkonsistensi status dan kegagalan sinkronisasi.
- **Keputusan**: Seluruh state operasional terkait Kru, Agen, Run, Task, Tool Execution, dan Memori secara mutlak dimiliki oleh Go Engine (`engine/src/`).
- **Konsekuensi**: Rust TUI dilarang menentukan alur giliran agen secara mandiri dan wajib bertindak murni sebagai penampil proyeksi (*projection view*).

### ADR-002: Rust Tetap Menjadi Canonical Owner untuk Primitif Terminal & Platform Native
- **Status**: Disetujui
- **Konteks**: Rust memiliki performa native tanpa garbage collection yang superior untuk rendering ANSI terminal, navigasi teks, dan integrasi OS lokal.
- **Keputusan**: Seluruh primitif antarmuka terminal, manajemen tema, clipboard, mouse, dan navigasi file explorer lokal tetap berada di bawah kepemilikan Rust (`apps/zerocode`).
- **Konsekuensi**: Tidak ada kode rendering terminal yang ditulis dalam Go.

### ADR-003: REST + Server-Sent Events (SSE) sebagai Protokol Klien-Engine Awal
- **Status**: Disetujui
- **Konteks**: Dibutuhkan protokol yang ringan, mudah diuji via cURL/browser, dan mendukung pengiriman progres tugas satu arah tanpa overhead handshake WebSocket yang kompleks.
- **Keputusan**: Gunakan REST/JSON (`/api/v1`) untuk perintah/kueri dan Server-Sent Events (`/events`) untuk aliran event reaktif.
- **Konsekuensi**: WebSocket hanya akan dipertimbangkan pada fase lanjutan jika kolaborasi dua arah berkecepatan sangat tinggi benar-benar dibutuhkan.

### ADR-004: Event Run Bersifat Monotonik dan Mendukung Reconnection
- **Status**: Disetujui
- **Konteks**: Gangguan koneksi sementara pada socket loopback tidak boleh merusak riwayat tampilan klien.
- **Keputusan**: Setiap event dalam sebuah run memiliki nomor urut sekuensial monotonik (`sequence`) dan event ID unik. Klien dapat melakukan reconnect menggunakan header `Last-Event-ID`.
- **Konsekuensi**: Go Engine memelihara ring-buffer event di memori untuk setiap run yang aktif.

### ADR-005: Eksekusi Tool Mengikuti Kebijakan Tier Risiko & Approval Gate
- **Status**: Disetujui
- **Konteks**: Aksi agen yang memodifikasi sistem operasi (menulis file, menjalankan script) berisiko tinggi jika dijalankan tanpa pengawasan.
- **Keputusan**: Tool diklasifikasikan ke dalam tier `READ`, `WRITE`, dan `EXECUTE`. Aksi dengan efek samping wajib menahan eksekusi sementara (*pause*) dan meminta persetujuan pengguna via API.
- **Konsekuensi**: Klien TUI harus menyediakan dialog konfirmasi (Approve/Deny) yang responsif.

### ADR-006: Semantik SOP & Alur Kerja Dikelola oleh Go Engine
- **Status**: Disetujui
- **Konteks**: Pengguna membutuhkan alur kerja terstruktur (SOP) yang dapat dieksekusi secara identik dari TUI, desktop GUI, maupun web dashboard.
- **Keputusan**: Definisi SOP, template registry, dan konversi alur kerja menjadi Task Graph (DAG) diimplementasikan di `engine/src/workflow`.
- **Konsekuensi**: Panel SOP di Rust TUI murni memuat katalog dari API Go.

### ADR-007: Skema Protokol Bersama Tidak Boleh Diduplikasi Secara Manual
- **Status**: Disetujui
- **Konteks**: Menulis ulang struct data gRPC/JSON yang sama di Rust dan Go secara manual rawan menimbulkan *schema drift*.
- **Keputusan**: Kontrak IPC antar-bahasa wajib didefinisikan dalam file Protobuf (`proto/`) atau skema terpusat dan di-generate secara otomatis via `tonic-build` dan `protoc-gen-go`.
- **Konsekuensi**: Setiap perubahan kontrak komunikasi wajib diawali dengan perubahan file proto.

### ADR-008: Dilarang Mengekspos Hidden Chain-of-Thought Mentah ke Event Stream Klien
- **Status**: Disetujui
- **Konteks**: Model reasoning modern menghasilkan penalaran internal yang panjang dan bising, serta berpotensi memuat token yang tidak ramah pengguna.
- **Keputusan**: Event stream klien hanya menyiarkan `agent.thought_summary` yang ringkas, terstruktur, dan aman dari data rahasia.
- **Konsekuensi**: Log mentah internal tetap disimpan di file lokal untuk kebutuhan diagnostik lanjutan, namun tidak dialirkan ke antarmuka utama.

### ADR-009: Permukaan Produk Baru Mengonsumsi Go Engine API Secara Langsung
- **Status**: Disetujui
- **Konteks**: Pengembangan antarmuka desktop Tauri atau Web Dashboard di masa mendatang rentan tergoda menduplikasi logika TUI Rust.
- **Keputusan**: Setiap klien baru wajib mengonsumsi endpoint REST dan SSE Go Engine yang sama dengan klien TUI.
- **Konsekuensi**: Seluruh kapabilitas multi-agen secara otomatis tersedia di seluruh platform tanpa penyesuaian backend.

### ADR-010: Migrasi Wajib Dilindungi Feature Flag dan Prosedur Rollback
- **Status**: Disetujui
- **Konteks**: Pemindahan kapabilitas kritis (chat lifecycle, tool loop) berisiko mengganggu produktivitas harian pengguna jika terjadi regresi.
- **Keputusan**: Seluruh pengalihan logika dari Rust ke Go dipasang di balik *feature flag* konfigurasi. Kode lama dipertahankan dalam status *read-only* atau *shadow mode* hingga verifikasi selesai.
- **Konsekuensi**: Tim dapat melakukan rollback instan dengan mengubah konfigurasi tanpa memerlukan rilis darurat.

---

## 5. Analisis Risiko Utama & Strategi Mitigasi

### 5.1 Risiko: Dual-Write & Dual-State
- **Bahaya**: Status tugas atau chat diperbarui di Rust dan Go secara bersamaan sehingga terjadi konflik data.
- **Mitigasi**: Tetapkan pemilik tunggal sebelum migrasi dimulai. Gunakan *shadow mode* (klien membaca data Go namun belum menulis) untuk validasi. Hapus jalur tulis di Rust segera setelah verifikasi API sukses.

### 5.2 Risiko: Big-Bang Rewrite
- **Bahaya**: Upaya memindahkan seluruh file sekaligus yang menyebabkan terhentinya pengembangan fitur lain dan sulitnya pelacakan bug.
- **Mitigasi**: Pindahkan fungsionalitas berdasarkan *capability slices* (slice per slice: Run -> Task -> LLM -> Tool -> Artifact), bukan per file fisik. Setiap slice memiliki rilis di balik feature flag.

### 5.3 Risiko: Goroutine Leak pada Multi-Agent Concurrency
- **Bahaya**: Sub-agen yang diluncurkan via goroutine tidak berhenti saat run dibatalkan, menyebabkan pemborosan CPU dan memori.
- **Mitigasi**: Wajib menerapkan propagasi `context.Context` kooperatif dengan `select { case <-ctx.Done(): return }`. Sertakan pengujian `stress_test.go` dengan 50 sub-agen yang memantau alokasi goroutine runtime Go.

### 5.4 Risiko: Bahaya Keamanan Eksekusi Tool
- **Bahaya**: Agen LLM mengeksekusi perintah penghapusan file di luar direktori kerja proyek (*path traversal*).
- **Mitigasi**: Sandboxing ketat pada `engine/src/tool`. Validasi bahwa canonical path file berada di dalam `WorkspaceRoot`. Aksi tulis dan eksekusi shell wajib melalui *approval gate*.

### 5.5 Risiko: Fitur KiroCrew Hanya Menjadi Imitasi Visual
- **Bahaya**: Menambahkan elemen UI yang tampak ramai tanpa menyelesaikan masalah transparansi eksekusi yang sesungguhnya.
- **Mitigasi**: Seluruh elemen antarmuka baru (timeline, approval modal, artifact panel) harus divalidasi berdasarkan kebutuhan penyelesaian masalah pengguna nyata: kejelasan alur, keselamatan eksekusi, dan kemudahan peninjauan output.

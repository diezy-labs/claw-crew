# Product Requirements Document (PRD) — ClawCrew Phase 2
## Multi-Agent Orchestration, Transparent Execution & Hybrid Evolution

> **Branch Context**: `feat/enhance-agent-phase2`  
> **Status**: Approved for Architecture Implementation  
> **Related Documents**: [README](./README.md) | [Technical Specification](./tech-spec.md) | [API Specification](./api-spec.md) | [Task Breakdown](./task-breakdown.md)

---

## 1. Purpose (Tujuan Produk)

Dokumen ini mendefinisikan persyaratan produk untuk **Claw-Crew Phase 2**. 

Tujuan fundamental dari fase ini adalah mentransformasi Claw-Crew dari sekadar klien terminal AI biasa menjadi platform orkestrasi **Multi-Agent (Crew)** kelas dunia yang transparan, aman, dan berkinerja tinggi, dengan memadukan keunggulan **Go 1.27** (untuk orkestrasi paralel, runtime state kanonikal, memory RAG, dan streaming LLM) dengan **Rust** (untuk desktop ergonomics, TUI native yang responsif, sandbox OS hardware, dan secret vault).

Selain itu, Phase 2 secara eksplisit mengadopsi prinsip desain produk yang terinspirasi oleh **KiroCrew**:
- Visibilitas peran dan aktivitas agen yang jelas (*crew visibility*).
- Kemajuan eksekusi tugas yang terstruktur dan terukur (*transparent execution progress*).
- Dashboard operasional yang tenang dan tidak bising (*concise operational dashboard*).
- Hirarki informasi bertahap (*progressive disclosure*).
- Peninjauan hasil kerja agen yang berpusat pada output (*artifact-centric review*).

---

## 2. Problem Statement & Background (Latar Belakang & Masalah)

### 2.1 Kondisi Saat Ini
Pada Phase 1, repository telah berhasil mendirikan pondasi mesin Go di `engine/` dengan domain awal `crew`, `llm`, dan `memory`. Namun, aplikasi TUI Rust (`apps/zerocode`) masih memiliki beban logika bisnis dan state eksekusi yang sangat besar di dalam modul seperti `chat.rs`, `app.rs`, `todo_tracker.rs`, `agent_sidebar.rs`, dan `config_manager.rs`.

### 2.2 Masalah yang Dihadapi
1. **Risiko Dual-State (State Duplication)**:
   Keberadaan status tugas dan percakapan di Rust TUI sekaligus di Go engine berisiko memunculkan inkonsistensi status giliran (*turn status*), tugas yang terduplikasi, dan kegagalan sinkronisasi saat koneksi terputus.
2. **Keterbatasan Concurrency Rust untuk Multi-Agent Massal**:
   Mengelola puluhan sub-agen yang berjalan paralel dengan model `async/await` dan sinkronisasi `Arc<Mutex<T>>` di Rust menciptakan kompleksitas kognitif tinggi dan potensi risiko *deadlock*.
3. **Keterikatan Antarmuka (Tight Coupling UI & Logic)**:
   Karena logika eksekusi agen terikat di dalam modul TUI Rust, platform lain seperti Tauri Desktop GUI atau Web Dashboard tidak dapat menggunakan kapabilitas agen yang sama tanpa menulis ulang logika eksekusi.
4. **Kurangnya Transparansi Eksekusi (Black-Box AI)**:
   Pengguna sering kali tidak mengetahui agen mana yang sedang bekerja, mengapa sebuah aksi tertunda, perintah sistem apa yang sedang dieksekusi, serta file apa saja yang telah diubah sebelum eksekusi selesai.

---

## 3. Target Users (Target Pengguna)

1. **Terminal-First Developers**: Pengembang perangkat lunak yang menginginkan kecepatan, fleksibilitas keyboard/shortcut, dan kenyamanan terminal tanpa kehilangan visibilitas multi-agen.
2. **Multi-Agent Coordinators**: Pengguna yang mengelola alur kerja kompleks (misalnya: *Researcher*, *Planner*, *Coder*, *Reviewer*) yang membutuhkan pembagian tugas terstruktur dan eksekusi paralel.
3. **Enterprise & Safety-Conscious Teams**: Tim yang memerlukan rekam jejak audit (*audit trails*), batasan sandboxing, dan sistem persetujuan (*approval gates*) sebelum agen memodifikasi kode atau menjalankan perintah shell.
4. **New Users / Beginners**: Pengguna baru yang membutuhkan panduan alur kerja baku (*Standard Operating Procedures / SOP*) dan galeri template *quickstart* yang mudah digunakan.

---

## 4. KiroCrew-Inspired Product Direction

### 4.1 Visi Pengalaman Pengguna (User Experience Vision)
Claw-Crew Phase 2 mengadopsi model mental produk KiroCrew yang menghadirkan **ketenangan, kejelasan, dan kendali penuh**:
- Pengguna tidak lagi disodori layar chat penuh dengan teks log mentah yang membingungkan.
- Pengguna melihat orkestrasi yang hidup: siapa yang memimpin, siapa yang mengeksekusi sub-tugas, tugas apa yang sedang berjalan, dan apa hasil nyatanya.

### 4.2 Prinsip Pengalaman Pengguna (Experience Principles)

1. **One Primary Task Context at a Time**:
   Setiap *Run* berfokus pada satu tujuan utama. Seluruh sub-agen, task graph, dan event bernaung di bawah konteks run ini.
2. **Visible but Quiet Agent Activity**:
   Aktivitas agen selalu dapat dipantau (indikator status, ringkasan pemikiran terkini), namun tidak membanjiri layar chat utama dengan kebisingan teks internal (*internal monologue/hidden chain-of-thought*).
3. **Explicit Lifecycle State Machine**:
   Setiap run dan task memiliki tahapan status yang terdefinisi secara baku: `queued`, `planning`, `running`, `waiting_for_input`, `completed`, `cancelling`, `cancelled`, `failed`.
4. **Inspectable & Safe Tool Invocations**:
   Pemanggilan alat (*tool calls*) memiliki tier risiko. Aksi yang mengubah sistem (*side-effecting*: modifikasi file, eksekusi shell) menuntut konfirmasi eksplisit (*approval modal*) yang jelas dan dapat ditolak oleh pengguna.
5. **Artifact-Centric Review**:
   Hasil utama agen (diff kode git, laporan Markdown, skema arsitektur) dikelompokkan sebagai **Artifact**. Pengguna dapat meninjau, menyetujui, atau mendiskusikan artifact tersebut dengan satu ketukan tombol.
6. **Progressive Disclosure**:
   Antarmuka secara default menyajikan visual ringkas (status bar, timeline, kartu hasil). Log mentah JSON dan jejak debug Prometheus tetap tersedia namun disembunyikan dalam laci (*drawer/pane*) diagnostik terpisah.
7. **Graceful Failure & Recovery**:
   Jika LLM mengalami rate limit atau tool mengalami error, sistem tidak langsung crash. Sistem menyajikan pesan kesalahan terstruktur dengan rekomendasi aksi pemulihan (*retry action button*).

### 4.3 Permukaan Produk Utama (Product Surfaces)

| Permukaan Produk | Tujuan & Fungsi | Sumber Data Utama (Canonical Truth) |
|---|---|---|
| **Run Workspace** | Layar kerja utama berisi chat, streaming respon agen, dan kontrol eksekusi. | Go Run API (`engine/src/run`) |
| **Crew & Agent Panel** | Roster anggota kru, peran (*role*), kemampuan (*capabilities*), dan status aktif agen. | Go Crew Service (`engine/src/crew`) |
| **Task Timeline / Graph** | Visualisasi urutan tugas, dependensi antar-tugas, status *pending/running/done*. | Go Task Service (`engine/src/task`) |
| **Artifact Review Panel** | Penampil diff kode, dokumen hasil sintesis, file yang dibuat oleh agen. | Go Artifact API (`engine/src/artifact`) |
| **Tool Activity & Approval Bar** | Riwayat eksekusi tool, argumen perintah, dan modal persetujuan izin (Approve/Deny). | Go Tool Service (`engine/src/tool`) |
| **Memory & Context Inspector** | Penampil referensi memori RAG dan konteks yang dimasukkan ke prompt model. | Go Memory API (`engine/src/memory`) |
| **Quickstart & SOP Gallery** | Katalog alur kerja baku yang siap dijalankan dengan sekali klik. | Go Workflow Service (`engine/src/workflow`) |
| **Diagnostics & Health Drawer**| Metrik real-time (RPS, token usage, latensi), status daemon, dan konektivitas provider. | Go Diagnostics & Prometheus HTTP |

---

## 5. Goals & Non-Goals

### 5.1 Goals
- Menjadikan Go Engine sebagai pemilik tunggal (*canonical owner*) dari state eksekusi kru, agen, tugas, run, memori, dan tool.
- Mempertahankan performa tinggi, ergonomi keyboard, rendering ANSI, dan nuansa native dari Rust TUI.
- Membuka jalan bagi klien masa depan (Tauri GUI Desktop, Web Dashboard) untuk terhubung ke Go Engine melalui kontrak API REST + SSE yang identik.
- Menyediakan siklus hidup *Run* yang dapat diinspeksi, dibatalkan secara instan (*cancellable*), dan diulang pada task yang gagal (*retryable*).
- Menghilangkan duplikasi state bisnis di sisi Rust.
- Memastikan penanganan error, batas timeout, pembatalan context, dan keamanan tool berjalan konsisten di semua platform.

### 5.2 Non-Goals
- Menulis ulang seluruh antarmuka TUI Rust ke dalam Go (Rust tetap menjadi pemilik terbaik untuk terminal rendering).
- Menggantikan pustaka platform-spesifik bawaan Rust (misalnya akses clipboard, deteksi lebar karakter terminal, rendering ANSI).
- Membangun infrastruktur server terdistribusi yang kompleks pada fase ini (fokus pada eksekusi lokal single-node yang sangat stabil dan andal).
- Memasukkan fitur-fitur visual berlebihan tanpa landasan kebutuhan fungsional yang nyata.
- Mengizinkan dual-write atau duplikasi state antar bahasa selama masa transisi.

---

## 6. Functional Requirements (Kebutuhan Fungsional)

### 6.1 Manajemen Kru & Agen (Crew & Agent Management)
- Sistem dapat memuat dan mendaftarkan kru beserta definisi agen di dalamnya.
- Setiap agen memiliki identitas unik, nama, peran (*role prompt*), batasan kemampuan (*tools whitelist*), dan status operasional.
- Status agen (`idle`, `thinking`, `executing_tool`, `waiting_approval`, `completed`, `error`) dikirimkan secara reaktif via event stream.

### 6.2 Siklus Hidup Run (Run Lifecycle Management)
- Pengguna dapat memulai sebuah *Run* baru melalui prompt bebas, template SOP, atau parameter input terstruktur.
- Setiap Run memiliki `run_id` berbasis UUID/ULID yang stabil.
- Siklus hidup Run harus mematuhi state machine: `queued` → `planning` → `running` → `waiting_for_input` → `completed` (atau `cancelling` → `cancelled` / `failed`).
- Pengguna dapat membatalkan Run kapan saja secara kooperatif melalui sinyal `Cancel`. Sinyal ini langsung menghentikan goroutine dan sub-proses yang terkait.

### 6.3 Task Graph & Eksekusi Alur Kerja (Task & Workflow Execution)
- Run dapat dipecah menjadi kumpulan *Tasks* yang terorganisir dalam grafik dependensi (DAG).
- Tugas memiliki status: `pending`, `ready`, `assigned`, `running`, `waiting_for_input`, `completed`, `failed`, `cancelled`.
- Mesin mendukung eksekusi tugas berurutan (*sequential*) maupun eksekusi paralel (*fan-out / fan-in*) menggunakan model konkurensi native Go (`sync.WaitGroup` dan channels).
- Jika sebuah task gagal, pengguna dapat memicu operasi `Retry` tanpa harus mengulang task-task sebelumnya yang telah sukses.

### 6.4 Eksekusi Tool & Kebijakan Persetujuan (Tool Execution & Approval Gates)
- Go Engine mengelola registrasi tool dan evaluasi kebijakan keamanan.
- Setiap eksekusi tool memancarkan event `tool.requested`, `tool.started`, `tool.completed`, atau `tool.failed`.
- Tindakan dengan efek samping (menulis file, menjalankan perintah shell, menghapus direktori) secara default berstatus `WAITING_APPROVAL`.
- Klien dapat mengirimkan konfirmasi persetujuan (`Approve`) atau penolakan (`Deny`) dengan alasan penolakan.
- Eksekusi tool dijalankan dalam batas sandbox direktori proyek (*workspace root*).

### 6.5 Manajemen Hasil Kerja & Diff (Artifact Management)
- Hasil eksekusi tugas berupa file baru, perubahan kode (diff), laporan sintesis, atau diagram dicatat sebagai entitas *Artifact*.
- Klien dapat mengambil daftar artifact per run, melihat pratinjau ringkasan (*summary*), serta mengunduh/membaca konten lengkap via API.

### 6.6 Manajemen Memori & Konteks RAG (Memory Lifecycle)
- Engine mendukung penyimpanan memori percakapan sesi (*session memory*) dan memori jangka panjang (*vector similarity memory*).
- Setiap Run mencatat metadata pemanggilan memori (*retrieval trace*) sehingga pengguna dapat menginspeksi potongan dokumen apa yang mempengaruhi keputusan agen.
- Strategi pengemasan konteks (*context packing*) memastikan kuota token model LLM tidak terlampaui.

### 6.7 Observabilitas & Keamanan (Observability & Security)
- Setiap event memiliki `event_id`, `run_id`, timestamp ISO-8601, dan nomor urut sekuensial monotonik (`sequence`) per run.
- Metrik Prometheus diekspos melalui endpoint HTTP (jumlah agen aktif, durasi turn, error count, token usage).
- Data sensitif (API key, token otentikasi, path sistem pribadi) wajib disaring (*redacted*) sebelum dicatat ke dalam log persisten atau dipancarkan ke event stream.

---

## 7. Success Criteria & Metrics (Metrik Keberhasilan)

1. **State Ownership Kanonikal**:
   Minimal **90%** status eksekusi yang ditampilkan pada antarmuka pengguna (TUI Rust atau Web) bersumber langsung dari event stream Go Engine, bukan dari asumsi lokal Rust.
2. **Kesesuaian Antarmuka Multi-Client**:
   Satu Run yang sama dapat diinisiasi, dipantau, dibatalkan, dan diperiksa artifact-nya dengan hasil identik baik melalui Rust TUI maupun cURL / HTTP client biasa.
3. **Zero State Duplication**:
   Tidak ada logika orchestrator ganda atau state todo tracker paralel yang disimpan di sisi Rust.
4. **Respon Streaming & Latensi**:
   Latensi pengiriman event pertama (*Time-to-First-Event* / TTFE) dari Go Engine ke klien TUI di bawah **50ms** pada koneksi loopback lokal.
5. **Ketahanan Concurrency**:
   Pengujian beban (*stress testing*) dengan 50 sub-agen yang aktif secara simultan berjalan stabil tanpa goroutine leak, tanpa memory leak, dan tanpa aplikasi desktop mengalami *hang/freeze*.
6. **Kualitas Penanganan Error**:
   100% kegagalan run menghasilkan kode error terstandarisasi beserta detail pesan dan rekomendasi langkah perbaikan bagi pengguna.

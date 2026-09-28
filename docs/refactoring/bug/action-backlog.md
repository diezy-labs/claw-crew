# Prioritized Action Backlog & Remediation Roadmap

> **Category**: Remediation Backlog, Ownership Governance & Final Assessment  
> **Status**: Living Execution Roadmap  
> **Related Documents**: [README](./README.md) | [Verification & Testing](./verification-testing.md) | [Concurrency](./concurrency-streaming.md) | [Security](./security-governance.md) | [Memory](./memory-persistence.md) | [Contracts](./cross-layer-contracts.md) | [Platform](./platform-lifecycle.md)

Dokumen ini memuat **daftar tugas tindakan terprioritisasi (Action Backlog)** untuk memitigasi seluruh potensi risiko dan cacat arsitektur yang teridentifikasi, matriks batas kepemilikan kode Rust vs Go, alur kerja remediasi, serta penilaian akhir arsitektur sistem.

Seluruh tugas menggunakan **checkbox kosong `[ ]`**. Beri tanda `[x]` saat butir pekerjaan telah selesai diimplementasikan, diverifikasi, dan lulus pengujian otomatis.

---

## 1. Prioritized Action Backlog

### 1.1 P0 — Wajib Dikerjakan Pertama (Critical Priority)
Fokus: Menghilangkan risiko keamanan, kebocoran goroutine, data race fatal, dan kegagalan pembatalan eksekusi.

- [x] **ACT-P0-01**: Jalankan `go test -race ./...` di seluruh Go engine dan perbaiki seluruh temuan race condition.
- [x] **ACT-P0-02**: Tambahkan test pemutusan koneksi klien (*client disconnect*) dan pembatalan stream pada HTTP/Tauri SSE bridge.
- [x] **ACT-P0-03**: Pastikan propagasi `context.Context` cancellation mengalir secara tuntas dari Run induk ke LLM provider, eksekusi tool, dan seluruh sub-agen.
- [x] **ACT-P0-04**: Bangun state machine transisi status Run dan Task yang divalidasi dan bersifat monotonik (mencegah transisi ilegal seperti `cancelled -> completed`).
- [x] **ACT-P0-05**: Audit setiap jalur eksekusi tool dan tegakkan satu gerbang otorisasi dan *Approval Gate* terpusat pada `engine/src/tool`.
- [x] **ACT-P0-06**: Tambahkan test konkurensi memori vektor in-memory (`store.go`), validasi dimensi vektor, dan pengujian isolasi cakupan (*scope isolation*).
- [x] **ACT-P0-07**: Pastikan tidak ada tool mutasi (tulis/eksekusi) yang dapat berjalan setelah run dibatalkan atau jika persetujuan pengguna ditolak/kedaluwarsa.

---

### 1.2 P1 — Wajib Selesai Sebelum Rilis Stabil (High Priority)
Fokus: Integritas event stream, konsistensi state UI, kontrak data lintas-lapisan, dan penanganan crash sidecar.

- [x] **ACT-P1-01**: Terapkan format event terstandarisasi dengan `event_id` stabil, `run_id`, dan nomor urut sekuensial monotonik (`sequence`) per run.
- [x] **ACT-P1-02**: Implementasikan logika rekonsiliasi antara snapshot polling dan event stream SSE pada klien Tauri dan Web (anti-duplikasi).
- [x] **ACT-P1-03**: Tetapkan Protobuf / OpenAPI sebagai sumber kebenaran skema tunggal dan buat pengujian kontrak data otomatis (*contract tests*).
- [x] **ACT-P1-04**: Definisikan secara resmi perilaku sistem saat restart sidecar Go dan semantik pemulihan state pada layar recovery console.
- [x] **ACT-P1-05**: Terapkan format *error envelope* terstruktur (`code`, `message`, `layer`, `request_id`, `retryable`) di seluruh lapisan Go, Tauri, Rust, dan Web.
- [x] **ACT-P1-06**: Jalankan verifikasi penuh frontend (`npm run typecheck`, `npm run test`, `npm run build`) pasca pembaruan dependensi besar.
- [x] **ACT-P1-07**: Buat desktop smoke test otomatis yang menyalakan binary sidecar Go asli dan menguji alur mulai run hingga selesai.
- [x] **ACT-P1-08**: Cegah aksi retry, cancel, atau recovery ganda dengan menerapkan optimistic concurrency token atau header `Idempotency-Key`.
- [x] **ACT-P1-09**: Tambahkan automated test untuk menyaring data rahasia (*secret redaction*) pada file log, payload event stream, dan pesan error.
- [x] **ACT-P1-10**: Hapus jalur penulisan status ganda di Rust (`chat.rs`, `todo_tracker.rs`) setelah Go Engine diverifikasi sebagai pemilik kanonikal.

---

### 1.3 P2 — Pengerasan Sistem & Skala Produksi (Medium Priority)
Fokus: Persistensi tahan lama, pembatasan buffer memori, observabilitas akurat, dan dukungan lintas platform.

- [x] **ACT-P2-01**: Implementasikan adapter persistensi disk (SQLite / flat-file) untuk menyimpan data run, task, dan artifact secara persisten.
- [x] **ACT-P2-02**: Terapkan pembatasan ukuran buffer event in-memory (*bounded ring-buffer*) dan pagination untuk daftar log dan artifact.
- [x] **ACT-P2-03**: Terapkan kebijakan retensi dan penggusuran memori vektor (*memory eviction policy*) untuk mencegah pemborosan RAM.
- [x] **ACT-P2-04**: Pastikan metrik Prometheus bersifat idempoten dan tidak menghitung ganda konsumsi token saat terjadi retry atau reconnect.
- [x] **ACT-P2-05**: Lakukan pengujian siklus hidup proses sidecar dan validasi path tool pada Windows, macOS, dan Linux.
- [x] **ACT-P2-06**: Tambahkan stress test performa end-to-end dengan puluhan agen paralel di bawah pengawasan alat profiling pprof.
- [x] **ACT-P2-07**: Integrasikan distributed tracing (OpenTelemetry span) atau correlation ID terpadu lintas proses IPC.

---

## 2. Matriks Batas Kepemilikan (Ownership Matrix)

Untuk mencegah terjadinya duplikasi state dan konflik logika (BUG-013), pembagian wewenang kode ditetapkan secara mutlak sebagai berikut:

### 2.1 Tetap Menjadi Milik Rust (`apps/zerocode`, `crates/`)
| Kapabilitas | Pemilik | Alasan Arsitektural |
|---|---|---|
| **Terminal Rendering (ANSI / TUI)** | Rust | Kecepatan native, zero garbage-collection, rendering ANSI presisi tinggi. |
| **Keymaps, Mouse, Clipboard** | Rust | Integrasi mendalam dengan pustaka platform OS lokal. |
| **Tema & Tata Letak Jendela** | Rust | Murni urusan presentasi visual di layar terminal. |
| **Buffer Input Teks Draft Lokal** | Rust / Web Klien | State visual lokal sebelum tombol enter/submit ditekan. |
| **Local File Explorer Rendering** | Rust | Pengalaman pengguna file browsing lokal yang interaktif. |
| **Visual Diff Rendering** | Rust / Web Klien | Penampil grafis kode berwarna dan navigasi baris perubahan. |

### 2.2 Menjadi Milik Kanonikal Go Engine (`engine/`)
| Kapabilitas | Pemilik | Alasan Arsitektural |
|---|---|---|
| **Siklus Hidup Run & Turn** | Go Engine | Menjadi sumber kebenaran bersama untuk seluruh platform klien. |
| **Graf Tugas (Task Graph & DAG)** | Go Engine | Penjadwalan dependensi tugas dan eksekusi paralel. |
| **Status Operasional Agen** | Go Engine | State machine runtime yang memancarkan event ke semua klien. |
| **Siklus Request & Loop LLM** | Go Engine | Normalisasi streaming, retry backoff, dan akuntansi token. |
| **Eksekusi Tool & Approval Gate** | Go Engine | Penegakan otorisasi, sandboxing, audit log, dan penahanan izin. |
| **Penyimpanan & Kueri Memori RAG**| Go Engine | Pencarian kemiripan vektor SIMD dan isolasi data multi-tenant. |
| **Semantik Alur Kerja & SOP** | Go Engine | Mesin eksekusi template alur kerja yang dapat digunakan kembali. |
| **Metadata Output & Artifacts** | Go Engine | Katalog hasil kerja agen yang dapat diinspeksi lintas-klien. |
| **Metrik Operasional & Telemetri** | Go Engine | Pencatatan kebenaran performa di level runtime backend. |

---

## 3. Alur Kerja Remediasi Cacat (Remediation Workflow)

Setiap penanganan bug pada register ini wajib mematuhi alur kerja 5 tahap berikut:

- [ ] **1. Reproduksi**: Buat test otomatis yang secara konsisten membuktikan kegagalan terjadi (*failing test*).
- [ ] **2. Isolasi Sumber Kebenaran**: Tentukan lapisan kanonikal yang bertanggung jawab (Go vs Rust) dan hindari perbaikan kosmetik di level UI jika masalahnya ada di backend.
- [ ] **3. Implementasi Perbaikan**: Terapkan kode perbaikan dengan mematuhi idiom bahasa dan arsitektur modular yang berlaku.
- [ ] **4. Verifikasi Konkurensi & Keamanan**: Jalankan `go test -race` dan uji coba negatif (penolakan izin).
- [ ] **5. Regresi & Sign-off**: Pastikan test suite lama dan baru lulus 100% pada pipeline CI sebelum kode digabungkan (*merged*).

---

## 4. Penilaian Akhir Arsitektur (Final Assessment)

Repositori Claw-Crew memiliki arah evolusi arsitektur yang sangat menjanjikan: **Go Engine** sebagai runtime kanonikal untuk eksekusi multi-agen yang paralel dan terstruktur, dipadukan dengan **Rust** sebagai rumah terbaik untuk interaksi terminal native yang cepat dan ergonomis.

Namun, fase transisi hybrid ini menyimpan risiko reliabilitas yang nyata jika tidak diimbangi dengan disiplin arsitektur. 

**8 Titik Risiko Tertinggi yang Wajib Dituntaskan:**
1. Kebocoran goroutine pada streaming dan kegagalan perambatan sinyal pembatalan (`context.Context`).
2. Kondisi balapan (*race condition*) pada orkestrasi task konkuren.
3. Potensi bypass otorisasi atau gerbang persetujuan (*approval gate*) pada eksekusi tool.
4. Korupsi data, *shallow-copy aliasing*, dan kebocoran cakupan pada in-memory vector store.
5. Duplikasi dan ketidakteraturan event akibat perpaduan streaming dan polling di antarmuka web/desktop.
6. Hilangnya state aktif secara mendadak saat proses sidecar Go mengalami restart.
7. Ketidakcocokan tipe data (*contract drift*) antara Go, gRPC, Tauri IPC, Rust, dan TypeScript.
8. Regresi runtime tersembunyi pasca pembaruan dependensi besar pada proyek frontend.

Langkah perekayasaan yang paling efektif saat ini adalah **mendirikan garis dasar keandalan (*reliability baseline*)** sebelum menambahkan fitur baru: jalankan deteksi data race, lengkapi test pembatalan stream, perketat pengujian keamanan tool, tegakkan kontrak data lintas-lapisan, dan validasi dengan smoke test desktop nyata.

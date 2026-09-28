# Cross-Layer Contracts, State Ownership & Event Delivery Audit

> **Category**: API Contracts, Serialization, State Duplication, Event Streaming & Polling Conflicts  
> **Status**: Potential Architecture Defect Register  
> **Related Documents**: [README](./README.md) | [Action Backlog](./action-backlog.md) | [Verification & Testing](./verification-testing.md)

---

## BUG-006 — Event Duplication, Event Loss, or Out-of-Order Streaming

**Severity:** P1 / High  
**Status:** Potential  
**Affected areas:** HTTP streaming bridge, LLM streaming, Tauri IPC, React polling, Rust terminal chat rendering.

### Description
Sistem memadukan streaming event real-time dengan mekanisme polling status pada UI. Rekoneksi jaringan, pengulangan pengiriman (*retry*), balapan antara fetch snapshot data dengan event langsung, atau adanya beberapa koneksi subscription yang aktif secara bersamaan dapat menyebabkan event diproses dua kali, diproses tidak berurutan, atau hilang sepenuhnya.

### Contoh Urutan Kegagalan
```text
1. UI mengambil snapshot task pada versi 10.
2. Stream SSE memancarkan event sekuens 11.
3. Polling latar belakang mengembalikan snapshot lama pada versi 10.
4. UI menimpa state terkini dengan data snapshot yang basi.
5. Rekoneksi stream memutar ulang event sekuens 11.
6. UI menambahkan pesan output yang sama untuk kedua kalinya.
```

### Gejala yang Terlihat oleh Pengguna
- Pesan asisten terduplikasi di layar chat.
- Jumlah hitungan task yang selesai tiba-tiba melompat mundur.
- Agen tampak dimulai dua kali pada status bar.
- Run tampak berstatus `completed` lalu beberapa saat kemudian berubah kembali menjadi `running`.
- Artifact yang sama muncul ganda di panel peninjauan.
- UI menunggu tanpa henti (*hangs*) karena event terminal penutupan stream terlewat.

### Kontrak Event yang Wajib Diterapkan (Required Event Contract)
Setiap event wajib memiliki struktur identitas dan nomor urut sekuensial yang konsisten:

```json
{
  "event_id": "evt_01h87b92mkq1",
  "run_id": "run_01h87b92mkq1",
  "task_id": "task_01",
  "sequence": 42,
  "event_type": "task.status_changed",
  "occurred_at": "2026-09-28T05:12:00Z",
  "payload": {}
}
```

### Perilaku Klien yang Wajib Diterapkan (Required Client Behavior)
- [ ] Klien melakukan deduplikasi berdasarkan `event_id`.
- [ ] Klien mengabaikan event yang nilai `sequence`-nya sudah pernah diterapkan sebelumnya.
- [ ] Klien mendeteksi adanya lompatan nomor urut (*gap in sequence*) dan meminta sinkronisasi ulang (*rehydration*).
- [ ] Klien merekonsiliasi snapshot data menggunakan `version` atau `updated_at`.
- [ ] Klien tidak menambahkan chunk teks secara buta tanpa memeriksa ID turn saat rekoneksi.
- [ ] Klien memelihara cache deduplikasi terbatas (*bounded dedupe cache*) per run.

### Kasus Pengujian (Test Cases Checklist)
- [ ] Putuskan koneksi setelah event ke-10, reconnect dengan `Last-Event-ID: evt_010`, pastikan event ke-11 diterima tepat 1 kali.
- [ ] Simulasikan penerimaan event ke-12 sebelum event ke-11; pastikan klien menahan event dalam buffer atau meminta sinkronisasi ulang.
- [ ] Respon polling yang lebih tua dari state stream terkini tidak boleh menimpa state antarmuka.
- [ ] Pengiriman ulang event yang sama tidak memicu mutasi ganda pada antarmuka.
- [ ] Dua tab browser/jendela UI terhubung secara simultan; pastikan masing-masing tab memiliki state deduplikasi independen.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Setiap event memiliki ID yang stabil dan nomor urut monotonik per run.
- [ ] Reducer antarmuka pengguna bersifat idempoten.
- [ ] Mekanisme rekonsiliasi antara snapshot dan event stream teruji dalam automated tests.
- [ ] Respon streaming duplikat mustahil terjadi pada lapisan proyeksi klien.

---

## BUG-008 — Go, gRPC, HTTP, Tauri, Rust, and TypeScript Contract Drift

**Severity:** P1 / High  
**Status:** Potential  
**Affected areas:** DTOs, Protobuf messages, Tauri commands, React service clients, Rust adapters, HTTP handlers.

### Description
Aplikasi menggunakan beberapa batas serialisasi: Protobuf gRPC, REST JSON, Tauri IPC bindings, dan antarmuka TypeScript. Jika model data diduplikasi secara manual di antara struct Go, schema Proto, struct Rust, tipe return Tauri, dan interface TypeScript, perubahan nama field atau penambahan status baru dapat lolos kompilasi di satu bahasa namun gagal secara diam-diam (*silent failure*) atau memicu crash runtime di bahasa lain.

### Modus Kegagalan yang Kerap Terjadi
- Go mengembalikan `task_id` (snake_case), sedangkan frontend Web mengharapkan `taskId` (camelCase).
- Go mengembalikan `completed_at: null`, sedangkan Rust mengharapkan string waktu non-null.
- Penambahan nilai enum baru seperti `waiting_for_approval` tidak dikenali oleh frontend TypeScript atau parser Rust, sehingga memicu panic/fallback error.
- Command Tauri mengubah error Go menjadi respons kosong `[]` alih-alih melempar error terstruktur.
- Skema gRPC memiliki field tertentu, namun jembatan HTTP mendrop field tersebut saat parsing.
- Angka ID berbasis integer 64-bit kehilangan presisi saat di-parse oleh JavaScript.
- Nilai nol (*zero-value*) pada Go dianggap sama dengan field yang tidak diset (*undefined*) oleh TypeScript.
- Format serialisasi `time.Time` Go tidak cocok dengan parser tanggal bawaan JavaScript.

### Pengendalian yang Wajib Diterapkan (Required Controls)
- [ ] Menjadikan satu sumber skema (Protobuf / OpenAPI) sebagai sumber kebenaran tunggal (*single source of truth*).
- [ ] Menerapkan versioning eksplisit pada route dan payload API (`/api/v1`).
- [ ] Menambahkan aturan fallback yang aman (`unknown`) untuk nilai enum baru di semua bahasa klien.
- [ ] Menggunakan format error envelope terstruktur di seluruh lapisan.
- [ ] Menghindari penggunaan nilai default implisit untuk penentuan status siklus hidup.
- [ ] Menjalankan contract test otomatis yang dijalankan dari sisi Go dan TypeScript di CI.

### Pengujian Kontrak yang Wajib Dibuat (Contract Tests Checklist)
- [ ] **Go -> HTTP**: Uji kompatibilitas JSON snapshot dan schema validation.
- [ ] **Go -> gRPC**: Uji kompatibilitas backward/forward Protobuf.
- [ ] **Go -> Tauri**: Uji serialisasi hasil dan error command Tauri.
- [ ] **Tauri -> Web**: Uji integrasi tipe TypeScript dengan respons IPC Tauri.
- [ ] **Go -> Rust TUI**: Uji fixture adapter klien Rust Zerocode terhadap respons Go.
- [ ] **All Consumers**: Uji penanganan enum yang tidak dikenal (*unknown enum fallback*).

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Kebijakan versioning dan pembaruan skema data terdokumentasi resmi.
- [ ] Seluruh enum status siklus hidup ditangani secara tuntas (*exhaustive match*) atau memiliki fallback aman.
- [ ] Test kompatibilitas DTO berjalan otomatis pada pipeline CI.
- [ ] Tidak ada lapisan delivery yang membangun ulang objek domain secara manual tanpa validasi tipe.

---

## BUG-009 — Polling and Streaming Cause Duplicate or Stale UI State

**Severity:** P1 / High  
**Status:** Potential  
**Affected areas:** Web TaskBoard, `usePolling`, metrics/dashboard UI, event streaming, Tauri UI state.

### Description
Web UI menyertakan mekanisme polling berkala berbasis visibilitas tab browser (`usePolling`), sementara backend dan jembatan Tauri juga menyediakan aliran event langsung (streaming). Jika kedua mekanisme ini memperbarui store data yang sama tanpa aturan reducer otoritatif dan rekonsiliasi yang ketat, antarmuka dapat menyisipkan entri duplikat, menimpa data stream yang masih segar dengan data polling yang basi, atau memicu aksi ganda.

### Contoh Cacat
- Respon polling menimpa status agen yang baru saja diperbarui melalui stream SSE.
- Stream menambahkan sebuah artifact baru; beberapa saat kemudian polling menggantikan seluruh daftar artifact dengan snapshot lama yang belum memuat artifact tersebut.
- Pengguna kembali membuka tab browser yang sempat disembunyikan (*tab visibility resume*), memicu polling instan saat request sebelumnya masih dalam perjalanan (*in-flight*).
- Komponen di-mount dan di-unmount berulang kali, meninggalkan beberapa loop polling aktif di latar belakang.
- Pembatalan unmount tidak meng-abort fetch, sehingga respons basi memperbarui komponen yang baru dibuka.

### Mitigasi yang Wajib Diterapkan (Required Mitigations Checklist)
- [ ] **Satu normalized client store**: Menerapkan satu store klien terpusat per tipe entitas (misal menggunakan ID sebagai key map).
- [ ] **Version-aware merge**: Data hanya diperbarui jika nomor versi atau timestamp `updated_at` dari respons lebih tinggi dari data lokal.
- [ ] **AbortController**: Selalu menggunakan `AbortController` untuk membatalkan request polling yang sedang berjalan saat komponen di-unmount.
- [ ] **Concurrency lock**: Mencegah request polling paralel untuk sumber daya yang sama.
- [ ] **Cleanup subscription**: Memastikan listener event stream dibersihkan secara tuntas saat unmount.
- [ ] **Idempotent stream events**: Seluruh event stream menerapkan mutasi yang aman dari duplikasi.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Antarmuka web memiliki dokumen aturan rekonsiliasi snapshot-dan-event yang jelas.
- [ ] Tidak ada fungsi penambahan array langsung (`items.push(...)`) yang digunakan tanpa pemeriksaan key unik.
- [ ] Pengujian browser otomatis mencakup skenario perubahan visibilitas tab (*hidden/visible*) dan rekoneksi.

---

## BUG-012 — Error Translation Hides Engine Failures as Empty or Normal State

**Severity:** P1 / High  
**Status:** Potential  
**Affected areas:** Go delivery layer, gRPC handlers, Tauri commands, web API client, Rust client adapter, recovery UI.

### Description
Sebuah error yang terjadi di kedalaman engine sering kali ditangkap dan diubah secara keliru menjadi `nil`, array kosong `[]`, status default netral, string generik yang tidak informatif, atau respons HTTP 200 OK dengan status kegagalan yang tertanam di dalam body. Hal ini menyembunyikan kegagalan sistem dari pantauan pengguna maupun sistem observabilitas.

### Contoh Modus Kegagalan
- Kueri memori gagal di level database vektor, namun handler mengembalikan `[]` sehingga UI menganggap sistem tidak memiliki memori relevan.
- Koneksi ke sidecar terputus, namun API client mengembalikan daftar kosong sehingga halaman aplikasi tampak kosong (*empty state* alih-alih *error state*).
- Pembatalan run gagal dieksekusi oleh engine, tetapi handler mengabaikan error dan UI tetap menampilkan status `cancelled`.
- Eksekusi tool menghasilkan error internal, namun diubah menjadi daftar artifact kosong tanpa pesan penjelasan.
- Status run yang tidak dikenal secara default dipetakan menjadi `offline` atau `completed`.
- Kode status gRPC hilang saat dikonversikan menjadi string biasa oleh wrapper Tauri.

### Format Error Envelope Standar
```json
{
  "error": {
    "code": "ENGINE_UNAVAILABLE",
    "message": "The agent engine is not reachable.",
    "request_id": "req_01h874xkm9",
    "retryable": true,
    "details": {}
  }
}
```

### Kasus Pengujian (Test Cases Checklist)
- [ ] **Engine tidak dapat dijangkau**: UI menyajikan layar error yang actionable, bukan layar kosong.
- [ ] **Request tidak valid (400)**: UI menerima rincian kesalahan validasi spesifik, bukan pesan generik.
- [ ] **Run tidak ditemukan (404)**: UI tidak menganggapnya sebagai run aktif yang baru.
- [ ] **Izin ditolak (403)**: UI tidak melakukan retry terus-menerus tanpa henti.
- [ ] **Approval tool kedaluwarsa**: UI menyajikan status kedaluwarsa yang dapat ditindaklanjuti.
- [ ] **Nilai enum tidak dikenal**: UI merender status eksplisit `Unknown/Unsupported`.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Seluruh pesan error memiliki struktur terstandarisasi dari backend hingga frontend.
- [ ] Tidak ada klien API yang secara diam-diam mengubah kegagalan request menjadi data kosong.
- [ ] Log mencatat correlation ID untuk setiap kegagalan yang terjadi.
- [ ] UI mampu membedakan dengan tegas error yang dapat diulang (*retryable*), error validasi, error perizinan, dan error internal.

---

## BUG-013 — Canonical State Is Duplicated Between Rust and Go

**Severity:** P1 / High  
**Status:** Architectural migration risk  
**Affected areas:** Rust `chat.rs`, `app.rs`, `todo_tracker.rs`, `agent_sidebar.rs`, Go crew service, LLM dispatcher, client adapters.

### Description
Selama proses migrasi bertahap, Rust dan Go rentan sama-sama menyimpan logika independen untuk fakta bisnis yang sama: penyelesaian turn, status agen, status task/todo, siklus hidup percakapan, keputusan retry, atau status eksekusi tool. Ketika dua komponen perangkat lunak memiliki wewenang untuk mengubah fakta yang sama, inkonsistensi data pasti akan terjadi.

### Contoh Kontradiksi
- Rust menyimpulkan bahwa sebuah turn telah selesai setelah menerima chunk token terakhir, sementara Go Engine masih menunggu persetujuan tool (*waiting_for_approval*).
- Go menandai sebuah task gagal, tetapi state lokal Rust menandainya selesai berdasarkan callback klien lama.
- Tombol retry di Rust langsung membuat status tugas lokal baru sebelum Go Engine menerima dan menyetujui request retry tersebut.
- Go memancarkan status agen `waiting_for_input`, sementara antarmuka Rust tidak memiliki padanannya sehingga memetakannya secara keliru menjadi `idle`.

### Kebijakan Pembagian Tanggung Jawab Kanonikal
| State / Fakta Bisnis | Pemilik Kanonikal (Single Source of Truth) | Peran Klien (Rust/Tauri/Web) |
|---|---|---|
| **Run Status** | Go Engine (`engine/src/run`) | Menampilkan status dan kontrol aksi. |
| **Task State & DAG** | Go Engine (`engine/src/task`) | Memproyeksikan visualisasi timeline. |
| **Agent Operational Status** | Go Engine (`engine/src/crew`) | Menampilkan roster dan indikator status. |
| **Tool Execution State** | Go Engine (`engine/src/tool`) | Menampilkan modal persetujuan izin. |
| **Workflow / SOP Runtime** | Go Engine (`engine/src/workflow`) | Memilih dan memicu template. |
| **Memory Lifecycle** | Go Engine (`engine/src/memory`) | Menginspeksi konteks yang digunakan. |
| **Terminal Rendering & Layout** | Rust (`apps/zerocode`) | Pemilik tunggal rendering TUI. |
| **Keymaps, Clipboard, Draft Input** | Rust / Web Client | State lokal antarmuka pengguna. |

### Aturan Migrasi yang Wajib Ditaati
Dilarang memindahkan file secara borongan (*wholesale*). Pindahkan kapabilitas bisnis hanya setelah Go Engine dideklarasikan sebagai pemilik kanonikal, seluruh klien mengonsumsi API-nya, dan jalur penulisan data lama di Rust telah dihapus atau dinonaktifkan di balik *feature flag*.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Setiap entitas bisnis hanya memiliki satu pemilik backend kanonikal.
- [ ] Klien antarmuka murni memproyeksikan state dari backend dan tidak menghitung ulang status bisnis secara mandiri.
- [ ] Seluruh jalur mutasi ganda (*duplicate write paths*) di Rust dihapus setelah proses migrasi diverifikasi.

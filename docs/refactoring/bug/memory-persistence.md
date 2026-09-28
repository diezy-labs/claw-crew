# Memory, Vector Store & Persistence Audit

> **Category**: Vector Store Concurrency, Memory Scoping, Lifecycle Durability & Resource Growth  
> **Status**: Potential Architecture Defect Register  
> **Related Documents**: [README](./README.md) | [Action Backlog](./action-backlog.md) | [Verification & Testing](./verification-testing.md)

---

## BUG-004 — In-Memory Vector Store Race, Corruption, or Incorrect Query Results

**Severity:** P0 / Critical  
**Status:** Potential  
**Affected areas:** `engine/src/memory/store.go`, memory interfaces, gRPC QuickQuery path.

### Description
Engine Go menyertakan penyimpanan vektor in-memory (`engine/src/memory/store.go`) yang terhubung dengan pencarian kemiripan dokumen. Penyimpanan in-memory sangat rentan terhadap cacat mutasi/kueri konkuren, aliasing *shallow-copy*, kegagalan akibat ketidakcocokan dimensi vektor (*dimension mismatch*), perankingan non-deterministik, dan hilangnya data saat proses di-restart.

### Potensi Cacat yang Teridentifikasi
- Operasi Upsert dan Query mengakses map/slice yang sama secara serentak tanpa proteksi mutex.
- Vektor atau record memori yang dikembalikan mengekspos referensi pointer/slice internal yang dapat dimutasi oleh pemanggil.
- Vektor yang tersimpan diubah nilainya setelah penyisipan karena pemanggil mempertahankan slice aslinya (*aliasing hazard*).
- Vektor query memiliki dimensi yang berbeda dengan vektor yang telah tersimpan di store.
- Vektor dengan panjang magnitudo nol memicu pembagian dengan nol (*divide-by-zero*) atau menghasilkan nilai similarity `NaN`.
- Nilai `topK` bernilai negatif, nol, atau lebih besar dari jumlah elemen di dalam store.
- Nilai kemiripan yang sama menghasilkan urutan pengurutan yang tidak deterministik (*nondeterministic ranking*).
- Context cancellation diabaikan selama proses scanning store dalam jumlah data besar.
- Record memori tidak diisolasi berdasarkan workspace, kru, pengguna, atau run.
- Restart engine menghapus memori secara diam-diam tanpa indikator yang jelas.

### Dampak Keamanan (Security Impact)
Jika batasan cakupan memori (*memory scopes*) tidak ditegakkan pada lapisan kueri, sebuah kru atau workspace dapat mengambil record memori milik workspace lain. Ini adalah **cacat isolasi data (data-isolation defect)**, bukan sekadar masalah relevansi pencarian.

### Kasus Validasi yang Diperlukan (Validation Cases Checklist)
- [ ] **Insert vektor kosong**: Wajib menghasilkan error validasi eksplisit.
- [ ] **Insert dimensi tidak cocok**: Menolak vektor yang tidak sesuai panjang dimensi store.
- [ ] **Query dimensi tidak cocok**: Menolak query dengan pesan error validasi yang jelas.
- [ ] **Query dengan `topK = 0`**: Mengembalikan hasil kosong secara aman.
- [ ] **Query dengan `topK < 0`**: Menolak permintaan dengan error validasi.
- [ ] **Query dengan `topK > jumlah item`**: Mengembalikan seluruh item yang ada tanpa panic/out-of-bounds error.
- [ ] **Skor kemiripan bernilai sama**: Pengurutan tetap deterministik (menggunakan ID atau timestamp sebagai *tie-breaker*).
- [ ] **100 upsert dan query serentak**: Bebas dari data race, tidak ada panic, dan output data tidak terkorupsi.
- [ ] **Mutasi slice input setelah upsert**: Nilai yang tersimpan di dalam memory store tetap tidak berubah (*deep-copy input*).
- [ ] **Mutasi slice hasil query**: Data internal di dalam memory store tetap tidak berubah (*deep-copy output*).
- [ ] **Restart engine**: Perilaku terdokumentasi secara eksplisit (ephemeral atau terpersistensi).
- [ ] **Kueri lintas workspace**: Tidak boleh mengembalikan record di luar cakupan otorisasi pemanggil.

### Pengamanan yang Direkomendasikan
- Melakukan *deep-copy* pada seluruh slice vektor saat input dan saat output.
- Menggunakan `sync.RWMutex` atau *immutable snapshot* untuk isolasi kueri.
- Memvalidasi seluruh dimensi dan parameter kueri di awal pemanggilan fungsi.
- Menambahkan aturan pemecah seri (*tie-breakers*) yang deterministik dalam algoritma sorting.
- Memeriksa `ctx.Err()` secara berkala pada pemindaian dataset besar.
- Menyertakan filter scoping (workspace/crew/run) langsung di dalam logika penyimpanan.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Detektor race Go (`go test -race ./src/memory`) lulus di bawah beban stress konkuren.
- [ ] Seluruh input parameter yang tidak valid ditolak secara bersih dengan kode error domain.
- [ ] Hasil kueri memori bersifat deterministik untuk record dengan skor kemiripan identik.
- [ ] Uji isolasi ruang lingkup data (*scope isolation*) terverifikasi pada lapisan penyimpanan.

---

## BUG-007 — Ephemeral Run, Task, and Memory State Is Lost After Engine Restart

**Severity:** P1 / High  
**Status:** Architectural risk  
**Affected areas:** Go run registry, crew services, memory store, sidecar lifecycle, recovery UI.

### Description
Jika engine Go menyimpan data run aktif, daftar tugas, state agen, riwayat event, dan memori hanya di dalam struktur data RAM (*in-memory*), mematikan atau me-restart proses engine/sidecar akan menghapus seluruh state tersebut seketika. Klien UI kemudian menampilkan data basi, salah melaporkan keberhasilan/kegagalan, atau gagal melakukan tindakan pemulihan (*recovery actions*).

### Skenario Pemicu
- Aplikasi desktop Tauri ditutup dan dibuka kembali.
- Sidecar Go mengalami crash atau di-upgrade versinya.
- Komputer pengguna memasuki mode sleep / hibernate dan kemudian resume.
- Hot-reload developer me-restart proses daemon Go.
- Jembatan jaringan loopback mengalami reset.
- Engine mengalami unhandled panic pada goroutine pekerja.

### Gejala yang Dapat Muncul
- Run yang sedang aktif tiba-tiba lenyap dari dashboard.
- UI tetap menampilkan tugas berstatus `running` padahal engine tidak lagi mengenali run tersebut.
- Konsol pemulihan menampilkan data yang tidak lengkap atau basi.
- Referensi artifact ada di disk tetapi metadata run-nya telah hilang.
- Pencarian memori tidak menghasilkan konteks apa pun setelah aplikasi di-restart.
- Klien memicu run ganda karena mencoba mengulang request tanpa idempotency key yang tersimpan.

### Keputusan Produk yang Wajib Ditetapkan (Required Product Decision)
Pilih salah satu model persistensi berikut secara resmi:
1. **Model Ephemeral**: Seluruh state aktif sengaja hilang saat restart; UI memperingatkan pengguna dan menandai run yang terputus sebagai `interrupted`.
2. **Model Durable**: State run, task, dan event disimpan ke disk (SQLite/flat-file) dan dapat dipulihkan/dilanjutkan setelah restart.
3. **Model Hybrid (Direkomendasikan)**: Metadata dan status run disimpan secara persisten dengan status `interrupted`; eksekusi tool dan streaming LLM tidak dilanjutkan secara otomatis tanpa konfirmasi pengguna.

### Pengujian yang Wajib Dijalankan (Required Tests Checklist)
- [ ] Mulai sebuah run, matikan paksa sidecar, jalankan kembali, lalu periksa konsistensi status run di UI.
- [ ] Mulai tool mutasi, matikan engine, pastikan tidak terjadi pemutaran ulang tool tanpa izin saat engine hidup kembali.
- [ ] Restart saat stream LLM berlangsung, verifikasi UI menampilkan pesan interupsi yang jelas.
- [ ] Restart setelah pembuatan artifact, verifikasi referensi artifact tetap valid dan dapat dibuka.
- [ ] Ulangi request klien asli setelah restart dengan idempotency key yang sama; pastikan tidak memicu run duplikat.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Perilaku saat restart terdokumentasi secara jelas dalam arsitektur produk.
- [ ] UI tidak pernah mengasumsikan run yang terinterupsi sebagai `completed`.
- [ ] Layar recovery mampu membedakan dengan tegas status `failed`, `cancelled`, `lost`, dan `interrupted`.

---

## BUG-019 — Unbounded Event, Log, Artifact, or Memory Growth

**Severity:** P2 / Medium  
**Status:** Potential  
**Affected areas:** Streaming event buffers, logs, vector store, artifact arrays, UI timeline.

### Description
Sistem multi-agen dapat menghasilkan ratusan ribu token stream, output log alat, artifact perubahan kode, dan memori percakapan. Jika seluruh entitas ini diakumulasikan di dalam RAM tanpa batas (*unbounded accumulation*) atau dikirimkan sekaligus secara utuh ke antarmuka, engine dan aplikasi desktop dapat mengalami degradasi performa drastis hingga Out-Of-Memory (OOM) crash.

### Contoh Kasus Risiko
- Setiap token LLM disimpan sebagai event terpisah di memori secara permanen.
- Seluruh output mentah tool eksekusi command disimpan langsung di dalam objek Run.
- UI mencoba me-render ribuan entri log sekaligus pada setiap siklus rendering.
- Vector store tidak memiliki batas retensi atau mekanisme penggusuran (*eviction policy*).
- Konten penuh artifact disertakan dalam payload list endpoint `/api/v1/runs/{id}/artifacts`.
- Label metrik Prometheus menggunakan ID yang tidak berbatas (*unbounded cardinality*).

### Mitigasi yang Wajib Diterapkan (Required Mitigations Checklist)
- [ ] **Event batching & chunk coalescing**: Menggabungkan chunk teks kecil sebelum disimpan ke buffer persisten.
- [ ] **Bounded in-memory ring buffers**: Membatasi ukuran buffer event aktif di memori (misal maks 1.000 event terakhir per run).
- [ ] **Pagination & cursor**: Menerapkan pagination untuk list log dan list artifact.
- [ ] **On-demand artifact fetching**: Endpoint list hanya mengembalikan metadata; konten mentah diambil via endpoint `/content` terpisah.
- [ ] **Size limits**: Membatasi batas maksimal ukuran output tool dan file attachment (misal maks 10MB).
- [ ] **Memory eviction policy**: Menerapkan kebijakan retensi (LRU / TTL) untuk vector memory store lokal.
- [ ] **Prometheus cardinality guard**: Membatasi label metrik hanya pada enum berdimensi tetap, bukan UUID dinamis.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Pengujian beban dengan volume streaming dan artifact besar berhasil diselesaikan tanpa lonjakan memori tak terkontrol.
- [ ] UI tetap responsif saat membuka riwayat run yang berdurasi sangat panjang.
- [ ] Penggunaan memori Go Engine tetap berada pada batas wajar (*steady-state bounded memory*) untuk beban kerja standar.

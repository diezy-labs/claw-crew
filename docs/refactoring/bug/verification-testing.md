# Verification, Test Plans & End-to-End Scenarios

> **Category**: Automated Quality Verification, Test Suites & End-to-End Scenarios  
> **Status**: Living Verification Guide  
> **Related Documents**: [README](./README.md) | [Action Backlog](./action-backlog.md)

Dokumen ini menyediakan rencana verifikasi dinamis, perintah evaluasi segera, skenario pengujian end-to-end terpadu, serta penambahan rangkaian test suite untuk memvalidasi dan mencegah terjadinya 20 potensi bug yang telah diidentifikasi.

Setiap butir langkah dan skenario pengujian dilengkapi dengan **checkbox kosong `[ ]`** untuk memudahkan pelacakan progres verifikasi oleh tim *quality assurance* dan perekayasa perangkat lunak.

---

## 1. Perintah Verifikasi Segera (Immediate Commands)

Jalankan perintah-perintah berikut dari kondisi checkout bersih (*clean working tree*).

### 1.1 Verifikasi Go Agent Engine (`engine/`)
- [ ] 1. Tidy dependensi Go:
  ```bash
  cd engine
  go mod tidy
  ```
- [ ] 2. Static analysis standar Go:
  ```bash
  go vet ./...
  ```
- [ ] 3. Unit test normal:
  ```bash
  go test ./...
  ```
- [ ] 4. Deteksi data race konkuren secara menyeluruh:
  ```bash
  go test -race ./...
  ```
- [ ] 5. Uji ketahanan terhadap flaky test & race (100x run):
  ```bash
  go test -count=100 ./...
  ```
- [ ] 6. Stress test modul orkestrasi kru (20x run berulang):
  ```bash
  go test -race -count=20 ./src/crew
  ```
- [ ] 7. Stress test modul memori vektor SIMD:
  ```bash
  go test -race -count=20 ./src/memory
  ```
- [ ] 8. Stress test modul provider & dispatcher LLM:
  ```bash
  go test -race -count=20 ./src/llm
  ```

### 1.2 Verifikasi Rust Workspace (`crates/` & `apps/zerocode/`)
- [ ] 1. Verifikasi format kode Rust:
  ```bash
  cargo fmt --all -- --check
  ```
- [ ] 2. Linting ketat dengan Clippy tanpa toleransi warning:
  ```bash
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  ```
- [ ] 3. Jalankan seluruh test suite Rust:
  ```bash
  cargo test --workspace
  ```

### 1.3 Verifikasi Web Client & Dashboard (`web/`)
- [ ] 1. Instalasi dependensi bersih:
  ```bash
  cd web
  npm ci
  ```
- [ ] 2. Pemeriksaan tipe statis TypeScript:
  ```bash
  npm run typecheck
  ```
- [ ] 3. Eksekusi test suite unit frontend:
  ```bash
  npm run test
  ```
- [ ] 4. Kompilasi bundle produksi:
  ```bash
  npm run build
  ```

---

## 2. Rangkaian Skenario End-to-End Minimum (E2E Scenarios)

### Skenario A — Streamed Run Normal (Happy Path)
- [ ] 1. Jalankan daemon Go Engine (`go run ./cmd/agent-engine`).
- [ ] 2. Luncurkan klien desktop Tauri / Web.
- [ ] 3. Konfirmasi bahwa probe `/api/v1/health` dan `/api/v1/ready` mengembalikan status OK.
- [ ] 4. Mulai tugas sederhana menggunakan satu agen.
- [ ] 5. Konfirmasi aliran event SSE tiba secara berurutan dengan nomor sekuensial yang konsisten.
- [ ] 6. Konfirmasi pembaruan status akhir menjadi `completed` dan artifact tersimpan dengan benar.
- [ ] 7. Konfirmasi bahwa log dan metrik Prometheus mencatat durasi serta konsumsi token yang akurat.

### Skenario B — Pembatalan Streamed Run (Cancellation)
- [ ] 1. Siapkan mock respons LLM dengan latensi panjang (misal streaming 100 token dengan jeda).
- [ ] 2. Mulai sebuah Run baru dari UI.
- [ ] 3. Tunggu hingga event chunk pertama diterima di layar.
- [ ] 4. Klik tombol 'Cancel' pada antarmuka pengguna.
- [ ] 5. Konfirmasi UI langsung menampilkan status `cancelled`.
- [ ] 6. Konfirmasi goroutine provider LLM di Go Engine menerima pembatalan `ctx.Done()`.
- [ ] 7. Konfirmasi tidak ada token susulan (*late chunks*) yang ditambahkan ke layar setelah status batal.
- [ ] 8. Konfirmasi eksekusi tool berikutnya tidak dilanjutkan.

### Skenario C — Pemutusan Koneksi Klien (Client Disconnect)
- [ ] 1. Mulai sebuah Run streaming.
- [ ] 2. Tutup jendela aplikasi desktop atau putuskan koneksi socket secara paksa.
- [ ] 3. Amati proses Go Engine di task manager / monitor sistem.
- [ ] 4. Konfirmasi bahwa engine membatalkan eksekusi atau melanjutkan tugas sebagai *detached run* sesuai konfigurasi.
- [ ] 5. Konfirmasi tidak ada goroutine yang menggantung (*goroutine leak*) atau channel yang tertahan.
- [ ] 6. Buka kembali aplikasi klien dan periksa status otoritatif dari backend.

### Skenario D — Restart Sidecar di Tengah Eksekusi (Engine Restart)
- [ ] 1. Mulai sebuah Run yang membutuhkan waktu beberapa detik.
- [ ] 2. Matikan paksa (*kill -9*) proses sidecar Go di tengah jalan.
- [ ] 3. Nyalakan kembali sidecar Go dan sambungkan kembali UI.
- [ ] 4. Konfirmasi antarmuka menandai run lama sebagai `interrupted` atau `lost`, bukan `completed`.
- [ ] 5. Konfirmasi tidak ada task mutasi file yang otomatis diulang tanpa konfirmasi pengguna.

### Skenario E — Orkestrasi Agen Paralel (Parallel Fan-Out)
- [ ] 1. Jalankan alur kerja yang melibatkan minimal 3 task paralel secara serentak.
- [ ] 2. Buat kondisi di mana: Task 1 sukses, Task 2 gagal, dan Task 3 menunggu persetujuan tool.
- [ ] 3. Konfirmasi transisi status pada grafik task DAG tetap konsisten dan tidak saling menimpa.
- [ ] 4. Kirim sinyal pembatalan global pada Run.
- [ ] 5. Konfirmasi seluruh pekerjaan anak (*child goroutines*) berhenti secara serentak dan bersih.
- [ ] 6. Jalankan pengujian ini berulang kali dengan flag `-race`.

### Skenario F — Batasan Approval Gate pada Tool Berisiko (Security Boundary)
- [ ] 1. Minta agen memanggil tool baca aman (`read_file`); pastikan berjalan otomatis tanpa popup modal.
- [ ] 2. Minta agen memanggil tool tulis berisiko (`write_file` atau `execute_command`).
- [ ] 3. Konfirmasi alur eksekusi terhenti sementara (*paused*) dan UI menampilkan dialog persetujuan.
- [ ] 4. Klik tombol 'Deny' pada dialog; pastikan file di disk tidak berubah sama sekali.
- [ ] 5. Ulangi skenario dan klik tombol 'Approve'; pastikan tool berjalan tepat satu kali.
- [ ] 6. Kirimkan request approval kedua untuk ID yang sama; pastikan sistem menolak eksekusi duplikat (*idempotent*).

### Skenario G — Rekonsiliasi Polling dan Stream (Deduplication)
- [ ] 1. Buka koneksi streaming event SSE secara langsung.
- [ ] 2. Aktifkan fitur polling berkala di latar belakang UI.
- [ ] 3. Picu beberapa mutasi task dan pembuatan artifact baru.
- [ ] 4. Simulasikan respons polling yang mengalami perlambatan (*delayed response*).
- [ ] 5. Putuskan koneksi stream sebentar dan biarkan melakukan auto-reconnect.
- [ ] 6. Konfirmasi tidak ada pesan chat, task, atau artifact yang muncul dobel pada tampilan akhir pengguna.

---

## 3. Rekomendasi Penambahan Test Suite

### 3.1 Test Suite Go Engine
Tambahkan file pengujian terfokus pada modul-modul berikut:

- [ ] **`engine/src/crew/run_state_machine_test.go`**: Validasi transisi status run yang sah dan penolakan lompatan state ilegal.
- [ ] **`engine/src/crew/cancellation_propagation_test.go`**: Memverifikasi context cancellation merambat ke seluruh level sub-agen.
- [ ] **`engine/src/crew/concurrent_finalization_test.go`**: Pengujian finalisasi run di bawah kondisi penyelesaian task serentak.
- [ ] **`engine/src/crew/event_ordering_test.go`**: Memastikan nomor urut `sequence` monotonik naik tanpa celah.
- [ ] **`engine/src/crew/idempotency_test.go`**: Memverifikasi pengiriman ID request yang sama tidak memicu turn duplikat.
- [ ] **`engine/src/llm/stream_disconnect_test.go`**: Pengujian penghentian konsumsi token saat pembaca terputus.
- [ ] **`engine/src/llm/provider_cancellation_test.go`**: Memastikan sinyal pembatalan diteruskan ke client HTTP provider AI.
- [ ] **`engine/src/llm/late_chunk_test.go`**: Memastikan chunk yang datang setelah context batal langsung dibuang.
- [ ] **`engine/src/memory/concurrent_store_test.go`**: Pengujian 100 upsert dan query paralel dengan flag `-race`.
- [ ] **`engine/src/memory/vector_validation_test.go`**: Validasi penolakan dimensi yang tidak cocok dan magnitudo nol.
- [ ] **`engine/src/memory/scope_isolation_test.go`**: Memastikan kueri tidak membocorkan data dari workspace/tenant lain.
- [ ] **`engine/src/memory/deterministic_ranking_test.go`**: Memverifikasi konsistensi urutan hasil pencarian dengan skor kemiripan sama.
- [ ] **`engine/src/tool/approval_enforcement_test.go`**: Memastikan tool berbahaya tidak bisa dieksekusi tanpa approval token valid.
- [ ] **`engine/src/tool/path_containment_test.go`**: Pengujian penolakan path traversal (`../`) dan symlink berbahaya.
- [ ] **`engine/src/tool/direct_dispatch_denial_test.go`**: Memastikan tidak ada metode eksekusi langsung yang lolos dari middleware otorisasi.

### 3.2 Fixture & Contract Tests Lintas Bahasa
Sediakan file fixture JSON bersama di direktori `fixtures/` yang dikonsumsi oleh Go, Rust, Tauri, dan TypeScript:

- [ ] `fixtures/run-created.json`
- [ ] `fixtures/run-running.json`
- [ ] `fixtures/run-cancelled.json`
- [ ] `fixtures/run-failed.json`
- [ ] `fixtures/task-waiting-for-approval.json`
- [ ] `fixtures/event-stream-replay.json`
- [ ] `fixtures/tool-approval-required.json`
- [ ] `fixtures/structured-error.json`
- [ ] `fixtures/unknown-status.json`

Integrasikan fixture di atas ke dalam:
- [ ] Pengujian serialisasi JSON di Go.
- [ ] Pengujian deserialisasi command Tauri di Rust.
- [ ] Pengujian unit adapter klien Rust Zerocode (`engine_client.rs`).
- [ ] Pengujian reducer dan parser schema di TypeScript.

### 3.3 Test Suite Antarmuka (Browser & Desktop)
- [ ] Pengujian auto-reconnect dan deduplikasi event stream SSE.
- [ ] Pembersihan polling interval saat komponen React di-unmount.
- [ ] Penanganan transisi perubahan visibilitas tab browser (*visibility change handler*).
- [ ] Penanganan visual saat sidecar daemon Go belum siap atau mati mendadak.
- [ ] Pemetaan kode error terstruktur ke pesan instruksi perbaikan yang ramah pengguna.
- [ ] Validasi bahwa dialog approval tool hanya dapat diklik sekali (*disable on click*).

---

## 4. Definition of Done (DoD) Perbaikan Bug

Setiap perbaikan bug hanya dianggap tuntas (*Done*) apabila memenuhi kriteria berikut:

- [ ] Modus kegagalan berhasil direproduksi terlebih dahulu melalui automated test yang gagal (*reproduction test*).
- [ ] Akar penyebab masalah diselesaikan pada lapisan pemilik kanonikal yang tepat.
- [ ] Tersedia regression test permanen di CI agar bug yang sama tidak terulang kembali.
- [ ] Penanganan pembatalan, retry, dan error diuji secara eksplisit.
- [ ] File log dan metrik menyertakan informasi yang memadai untuk mendiagnosis jika isu terjadi lagi.
- [ ] Perubahan kontrak data telah disesuaikan dan diuji di semua bahasa klien (Rust, Go, TypeScript).
- [ ] Perbaikan yang berkaitan dengan keamanan memiliki *negative/bypass test* yang memverifikasi penolakan akses.
- [ ] Perbaikan yang berkaitan dengan konkurensi lulus uji deteksi race (`go test -race`).
- [ ] Perilaku visual antarmuka pengguna telah diverifikasi terhadap respons otoritatif backend.

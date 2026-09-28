# Platform, Sidecar Lifecycle & UI Integration Audit

> **Category**: Tauri Sidecar Daemon, Frontend Tooling, Cross-Platform Compatibility & Metrics  
> **Status**: Potential Architecture Defect Register  
> **Related Documents**: [README](./README.md) | [Action Backlog](./action-backlog.md) | [Verification & Testing](./verification-testing.md)

---

## BUG-010 — Major Frontend Dependency Upgrade Regression

**Severity:** P1 / High  
**Status:** Requires full build and smoke verification  
**Affected areas:** Web application, Tauri frontend, CodeMirror, React, Vite, TypeScript, Node type definitions.

### Description
Pembaruan dependensi besar-besaran telah dilakukan pada ekosistem frontend, mencakup TypeScript, Node types, React, Vite, React Router, CodeMirror, Tailwind, dan Rollup. Pembaruan skala besar seperti ini rentan memicu ketidakcocokan kompilasi, kegagalan *tree-shaking*, perubahan perilaku hook siklus hidup React, dan kegagalan jembatan komunikasi desktop-web Tauri.

### Titik Kegagalan yang Kerap Terjadi
- Tipe binding IPC Tauri tidak lagi cocok dengan fungsi yang di-generate.
- Perubahan siklus hidup React mengekspos bug pembersihan effect (*cleanup hooks*) yang memicu kebocoran listener.
- Inferensi tipe TypeScript yang lebih ketat menolak kode yang sebelumnya lolos tanpa error.
- Perubahan resolusi modul Vite/Rollup merusak pemuatan variabel lingkungan (`import.meta.env`).
- Konflik versi ekstensi CodeMirror saat runtime editor diff.
- Perubahan perilaku routing pada React Router merusak navigasi nested routes.

### Validasi yang Wajib Dijalankan (Validation Commands Checklist)
- [ ] Jalankan audit dan build penuh pada proyek web:
  ```bash
  cd web
  npm ci
  npm run typecheck
  npm run test
  npm run build
  ```
- [ ] Jalankan pengujian *smoke test* desktop pada bundle produksi (`tauri build`), bukan hanya mode development (`tauri dev`).

### Alur Uji Minimum (Minimum Smoke Flow Checklist)
- [ ] 1. Luncurkan aplikasi desktop Tauri.
- [ ] 2. Pastikan sidecar daemon Go berjalan dan endpoint `/api/v1/health` merespons UP.
- [ ] 3. Buka seluruh rute utama: Dashboard, Crew/Task Board, Apps, Instances, Recovery, Metrics, dan Diff Viewer.
- [ ] 4. Mulai satu tugas agen sederhana.
- [ ] 5. Amati kelancaran streaming token dan pembaruan antarmuka.
- [ ] 6. Batalkan tugas yang sedang berjalan.
- [ ] 7. Picu aksi rekoneksi jaringan.
- [ ] 8. Pastikan halaman metrik dan log terbarui dengan benar.
- [ ] 9. Tutup aplikasi dan buka kembali.
- [ ] 10. Pastikan tidak ada rute yang rusak atau state tampilan yang nyangkut.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Build produksi web (`npm run build`) sukses tanpa peringatan fatal.
- [ ] Bundle produksi desktop Tauri dapat dibuka dan berjalan normal.
- [ ] Seluruh rute kritis dapat diakses tanpa unhandled JavaScript runtime exceptions.
- [ ] Pemanggilan IPC Tauri bersifat type-safe dan mengembalikan error envelope terstruktur.

---

## BUG-011 — Unit Tests Pass but Cross-Layer Integration Fails

**Severity:** P1 / High  
**Status:** Likely coverage gap  
**Affected areas:** All new engine-to-client features.

### Description
Pengujian unit pada fungsi utilitas murni sangat berguna, namun tidak cukup untuk menjamin keandalan aplikasi arsitektur hybrid. Suatu fitur dapat lulus 100% pada unit test lokal, namun tetap gagal total saat melewati batas serialisasi, proses OS terpisah, soket jaringan loopback, streaming SSE, pembungkusan sidecar Tauri, dan rendering UI.

### Pola Rasa Aman Semu (Typical False Confidence Pattern)
```text
Unit test lulus dengan sempurna:
- Helper penentu warna status agen
- Helper tombol retry
- Helper interval polling
- Fungsi pemetaan DTO

Namun produk tetap gagal di tangan pengguna:
- Command Tauri tidak dapat menjangkau sidecar Go
- Sidecar Go terlambat menyala saat aplikasi start
- URL koneksi event stream salah port atau salah path
- Go mengirimkan tipe field yang tidak didukung parser klien
- UI menangkap error backend tetapi mengubahnya menjadi array kosong
- Perintah pembatalan dikirimkan ke ID run yang keliru
```

### Cakupan Pengujian Integrasi yang Wajib Ada
- [ ] **Go Unit**: State machine domain, perankingan vector store, logika dispatcher tool.
- [ ] **Go Integration**: Server HTTP/gRPC dengan mock provider LLM dan mock tool OS.
- [ ] **Tauri Integration**: Siklus hidup proses sidecar, jembatan command IPC, dan penanganan crash sidecar.
- [ ] **Web Integration**: Klien API, store reducer, dan rekonsiliasi polling/stream.
- [ ] **End-to-End**: Mulai run, stream output, batalkan, tinjau artifact, dan pemulihan pasca restart.
- [ ] **Security**: Penegakan otorisasi, penolakan akses luar sandbox, dan approval gate tool.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Minimal terdapat 1 alur CI yang menjalankan Go engine + transport + mock LLM secara end-to-end.
- [ ] Minimal terdapat 1 pengujian desktop smoke test yang menyalakan binary sidecar Go asli.
- [ ] Fixture data kontrak bersama digunakan secara konsisten oleh seluruh adapter klien.

---

## BUG-016 — Metrics Are Double-Counted During Retry, Reconnect, or Late Event Delivery

**Severity:** P2 / Medium  
**Status:** Potential  
**Affected areas:** Prometheus metrics, agent metrics page, task performance table, event processing.

### Description
Metrik performa seperti jumlah konsumsi token LLM, estimasi biaya, latensi penyelesaian tugas, jumlah kegagalan/fallback, dan durasi run rentan terhitung ganda (*double-counted*) saat terjadi operasi retry, pemutaran ulang event saat reconnect, atau pengiriman event terlambat.

### Contoh Kasus Dobel
- Pemutaran ulang event stream saat reconnect dibaca sebagai kemunculan token baru oleh reducer metrik UI.
- Retry task menambah counter eksekusi, tetapi upaya pertama yang gagal juga tetap dihitung sebagai selesai.
- Timer durasi run terus berjalan setelah run sebenarnya telah dibatalkan.
- Fallback ke model cadangan mencatat error pada model utama sekaligus sukses pada model cadangan tanpa label dimensi pembeda yang jelas.
- Polling UI berulang kali mengagregasi hasil task yang sama ke dalam counter lokal.

### Pendekatan yang Diwajibkan (Required Mitigations Checklist)
- [ ] Metrik wajib dipancarkan langsung dari transisi siklus hidup backend, bukan direkonstruksi dari event UI.
- [ ] Menggunakan identifier unik untuk setiap percobaan: `run_id`, `task_id`, dan `attempt_id`.
- [ ] Menetapkan secara eksplisit apakah retry dihitung sebagai attempt terpisah atau satu task logis.
- [ ] Memastikan event terminal hanya menaikkan counter status akhir tepat satu kali.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Pemutaran ulang event (*event replay*) tidak mengubah nilai metrik di sisi server.
- [ ] Metrik Prometheus menyertakan dimensi label yang membedakan upaya awal dan percobaan retry.
- [ ] Semantik penghitungan metrik saat retry terdokumentasi secara resmi.

---

## BUG-018 — Sidecar Startup, Shutdown, and Restart Ordering Failure

**Severity:** P1 / High  
**Status:** Potential  
**Affected areas:** Tauri sidecar wiring, Go engine bootstrap, desktop commands, health checks.

### Description
Integrasi sidecar desktop Tauri menimbulkan potensi kegagalan urutan proses: UI mengirim request sebelum engine siap (*premature request*), proses sidecar dijalankan dua kali (*duplicate instance*), port soket lama masih tertahan oleh proses zombie, penutupan desktop meninggalkan proses daemon Go yang menggantung (*orphan process*), atau restart sidecar merusak koneksi stream UI yang aktif.

### Modus Kegagalan
- UI memanggil API engine sebelum endpoint `/health` dan `/ready` bernilai UP.
- Binary sidecar tidak ditemukan pada path yang diharapkan atau salah arsitektur CPU (x86 vs ARM64).
- Tabrakan port (*port collision*) menyebabkan aplikasi terhubung ke daemon lain di mesin pengguna.
- Restart sidecar meninggalkan koneksi stream lama dalam status menggantung tanpa notifikasi.
- Desktop ditutup tetapi tidak mengirimkan sinyal SIGTERM/SIGINT ke sidecar Go.
- Pembukaan kembali aplikasi desktop terhambat oleh file lock / PID lama yang belum dihapus.

### Pengendalian yang Wajib Diterapkan (Required Controls Checklist)
- [ ] Memisahkan endpoint `/api/v1/health` (liveness) dan `/api/v1/ready` (readiness).
- [ ] Supervisor proses sidecar yang melacak PID secara ketat.
- [ ] Mekanisme rekoneksi dengan exponential backoff terbatas.
- [ ] Status visual eksplisit pada antarmuka: `Starting`, `Ready`, `Degraded`, `Unavailable`, `Restarting`.
- [ ] Penutupan paksa seluruh child process saat aplikasi utama Tauri dihentikan.
- [ ] Pencatatan correlation ID yang menghubungkan log IPC Tauri dengan log sidecar Go.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Pengujian buka-tutup dan restart sidecar terotomatisasi dalam test suite.
- [ ] UI tidak mengaktifkan tombol aksi sebelum kesiapan sidecar dikonfirmasi oleh probe `/ready`.
- [ ] Tidak ada proses sidecar yatim (*orphan sidecar*) yang tertinggal setelah desktop ditutup.

---

## BUG-020 — Incomplete Cross-Platform Coverage

**Severity:** P2 / Medium  
**Status:** Potential  
**Affected areas:** Tauri, Rust terminal/runtime, Go sidecar process management, filesystem tools, path handling.

### Description
Aplikasi menggabungkan Rust, Go, Tauri, manipulasi sistem file, terminal ANSI, dan tool shell. Perbedaan lintas platform (Windows, macOS, Linux) kerap memicu kegagalan tersembunyi pada pemisah path, pembatalan proses pohon (*process-tree kill*), pengiriman sinyal OS, permission file, perbedaan baris baru (CRLF vs LF), format escaping shell, symlink, dan eksekusi binary sidecar.

### Titik Kritis Perbedaan Sistem Operasi
- **Windows**: Pemisah path backslash (`\`), drive letters (`C:`), pembatalan subprocess pohon (`taskkill`), dan escaping PowerShell.
- **macOS**: App sandbox quarantine, permission gate, arsitektur Apple Silicon vs Intel, dan shortcut keyboard (`Cmd` vs `Ctrl`).
- **Linux**: Dependensi pustaka desktop (WebKitGTK, OpenSSL), sinyal POSIX, dan permission eksekusi binary.

### Matriks Pengujian Platform (Test Matrix Checklist)
- [ ] **Windows**: Pembunuhan pohon proses sidecar, validasi path drive letter, batasan direktori workspace, dan eksekusi PowerShell tool.
- [ ] **macOS**: Bundle aplikasi Tauri, izin akses disk pengguna, penanganan sinyal sidecar, dan kompatibilitas shortcut.
- [ ] **Linux**: Peluncuran daemon sidecar, socket domain IPC lokal, penanganan sinyal SIGTERM, dan streaming SSE.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Seluruh perilaku khusus platform teruji di pipeline CI atau terdokumentasi sebagai platform yang belum didukung.
- [ ] Validasi path tool menggunakan kanonikalisasi yang sadar platform (*platform-aware canonicalization*).
- [ ] Pengujian siklus hidup sidecar berjalan sukses pada Windows dan minimal satu sistem Unix (Linux/macOS).

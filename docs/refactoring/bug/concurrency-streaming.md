# Concurrency, Streaming & State Lifecycle Audit

> **Category**: Concurrency, Goroutine Lifecycle, Cancellation & State Machines  
> **Status**: Potential Architectural Defect Register  
> **Related Documents**: [README](./README.md) | [Action Backlog](./action-backlog.md) | [Verification & Testing](./verification-testing.md)

---

## BUG-001 — Stream/Goroutine Leak After Client Disconnect

**Severity:** P0 / Critical  
**Status:** Potential; requires dynamic verification  
**Affected areas:** Go HTTP streaming bridge, LLM dispatcher, Tauri sidecar, web/Tauri clients, event delivery.

### Description
Klien memulai *agent turn* atau *run* yang distreaming. Klien kemudian terputus karena menutup aplikasi desktop, berpindah halaman (*navigate away*), kehilangan koneksi jaringan, me-restart sidecar Go, atau membatalkan request dari Tauri. Penulis stream HTTP/event berhenti, tetapi goroutine produser Go, pembaca stream LLM, goroutine orkestrasi agen, atau eksekusi tool tetap berjalan di latar belakang.

Hal ini dapat membocorkan goroutine, membiarkan request LLM tetap aktif dan menyedot kuota token secara sia-sia saat pengguna sudah tidak melihat hasilnya, menahan channel secara permanen, atau membiarkan operasi file/tool berlanjut setelah klien ditinggalkan.

### Pola Kegagalan Tipikal (Typical Failure Pattern)
```text
Client connects
  -> Go starts producer goroutine
  -> Go starts LLM stream consumer
  -> Go forwards chunks through a channel
  -> Client disconnects
  -> HTTP writer returns an error
  -> Consumer/producer still attempts to send to channel
  -> Goroutine blocks or continues without an owner
```

### Kemungkinan Akar Masalah
- Loop streaming tidak melakukan seleksi pada `ctx.Done()`.
- Request context digantikan dengan `context.Background()`.
- Pengiriman channel (`channel <- event`) tidak memiliki cabang pembatalan (`select`).
- Konsumer keluar tetapi tidak menutup atau menguras (*drain*) channel.
- Produser terus memancarkan event setelah pengiriman downstream gagal.
- Eksekusi tool terlepas (*detached*) dari konteks parent run.
- Permintaan ke provider LLM tidak menerima atau tidak mematuhi sinyal pembatalan.
- Penutupan proses Tauri tidak mematikan sidecar Go atau parent run.

### Gejala yang Dapat Diamati
- Jumlah memori dan goroutine engine terus meningkat tanpa pernah turun.
- Aktivitas CPU atau jaringan tetap tinggi setelah jendela aplikasi desktop ditutup.
- Biaya/token provider meningkat drastis setelah pembatalan.
- Run berikutnya menerima event basi (*stale events*) dari sisa run sebelumnya.
- Shutdown engine menggantung (*hangs*) karena goroutine terblokir saat mengirim ke channel.
- Rekoneksi stream menerima data usang yang tidak diharapkan.

### Tugas Verifikasi yang Diperlukan (Verification Tasks)
- [ ] Jalankan deteksi data race pada engine:
  ```bash
  cd engine
  go test -race ./...
  go test -count=100 ./...
  ```
- [ ] Buat pengujian terfokus yang menjalankan aliran stream mock LLM berdurasi panjang.
- [ ] Buka koneksi stream klien HTTP/Tauri dan baca 1 event pertama.
- [ ] Putuskan koneksi klien secara paksa di tengah aliran streaming.
- [ ] Tunggu pemicuan pembatalan context (`ctx.Done()`).
- [ ] Verifikasi bahwa mock provider LLM menerima sinyal pembatalan.
- [ ] Verifikasi bahwa seluruh goroutine produser dan worker berhenti secara tuntas.
- [ ] Verifikasi bahwa tidak ada operasi pengiriman channel yang terblokir.

### Pola Implementasi yang Direkomendasikan
```go
for {
    select {
    case <-ctx.Done():
        return ctx.Err()

    case event, ok := <-events:
        if !ok {
            return nil
        }

        if err := writeEvent(writer, event); err != nil {
            return err
        }
    }
}
```

Semua pekerjaan anak (*child work*) wajib mewarisi root context dari run:
```go
childCtx, cancel := context.WithCancel(parentRunContext)
defer cancel()
```

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Memutuskan koneksi klien membatalkan stream terkait atau melepaskannya secara aman sesuai kebijakan eksplisit.
- [ ] Tidak ada goroutine yang tertahan atau terblokir setelah pembatalan terjadi.
- [ ] Operasi LLM dan eksekusi tool menerima propagasi context cancellation.
- [ ] Status terminal stream dipancarkan atau dipersistensikan secara konsisten.
- [ ] Uji berulang putus-sambung koneksi (*disconnect/reconnect*) tidak menaikkan jumlah goroutine *steady-state*.

---

## BUG-002 — Cancellation Does Not Reach LLM, Tool, or Sub-Agent Tasks

**Severity:** P0 / Critical  
**Status:** Potential  
**Affected areas:** Crew services, LLM dispatcher, tool dispatcher, multi-agent task scheduler, HTTP/Tauri cancellation commands.

### Description
Pengguna menekan tombol *Cancel* pada antarmuka, namun pembatalan hanya mengubah flag lokal UI atau field status Run di level teratas Go. Request LLM yang sedang aktif, sub-agen yang telah di-spawn, pemanggilan tool yang menunggu antrean, atau goroutine pekerja tetap berjalan.

Dampaknya dapat berupa mutasi file yang tidak diinginkan di disk pengguna, pembengkakan biaya token LLM, penambahan output setelah status ditandai `cancelled`, serta kebingungan state UI di mana status berubah dari `cancelled` menjadi `completed`.

### Skenario Kegagalan (Failure Scenario)
```text
User clicks Cancel
  -> UI sends cancel request
  -> Engine marks Run.Status = cancelled
  -> Agent A has active LLM request
  -> Agent B is waiting on tool output
  -> Tool process continues writing files
  -> LLM returns a final answer
  -> Event consumer appends output
  -> Run can incorrectly transition from cancelled to completed
```

### Pengendalian yang Wajib Diterapkan (Required Controls)
- [ ] Satu root context per *Run*.
- [ ] Child contexts untuk setiap task, LLM request, tool request, dan stream writer.
- [ ] Propagasi sinyal pembatalan dilakukan secara rekursif ke seluruh sub-agen.
- [ ] Transisi status dilindungi agar tidak dapat ditimpa oleh status akhir yang terlambat (*terminal-state overwrite*).
- [ ] Run yang telah berstatus `cancelled` dilarang keras berubah menjadi `completed`.
- [ ] Output yang datang terlambat (*late chunks*) wajib dibuang atau disimpan sebagai log diagnostik tersembunyi.

### Kasus Pengujian (Test Cases Checklist)
- [ ] **Batal saat streaming token LLM**: Context provider dibatalkan; tidak ada chunk lanjutan yang dikirim ke UI.
- [ ] **Batal saat tool menunggu approval**: Tool dibatalkan dan tidak pernah dieksekusi.
- [ ] **Batal saat tool sedang menulis file**: Tool menerima sinyal pembatalan; file parsial ditangani secara aman.
- [ ] **Batal pada satu task paralel**: Mematuhi kebijakan yang ditentukan (batalkan seluruh task saudara atau isolasi task).
- [ ] **Batal setelah penyelesaian task namun sebelum finalisasi run**: Finalizer tidak boleh menimpa status pembatalan terminal.
- [ ] **Request pembatalan berulang**: Respon bersifat idempoten; tidak menghasilkan error ganda atau double-cleanup.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Seluruh anak pohon turunan eksekusi run menggunakan pohon pembatalan context yang sama.
- [ ] State terminal run bersifat monotonik dan tidak dapat ditimpa oleh penyelesaian yang terlambat.
- [ ] Operasi pembatalan bersifat idempoten.
- [ ] Operasi tool tidak dapat dimulai setelah run berstatus dibatalkan.

---

## BUG-003 — Concurrent Run/Task/Agent State Race

**Severity:** P0 / Critical  
**Status:** Potential  
**Affected areas:** `engine/src/crew`, shared run registry, agent state, task graph, event aggregation.

### Description
Orkestrasi multi-agen memungkinkan banyak sub-tugas berjalan secara paralel. Jika map status, slice tugas, counter, daftar artifact, state agen, atau array event dimutasi secara konkuren tanpa proteksi penguncian (*locking*) atau *message-passing ownership* yang ketat, engine rentan mengalami *data races*, *lost updates*, transisi ganda, state run tidak valid, hingga panic runtime Go (`fatal error: concurrent map writes`).

### Pola Kegagalan Tipikal (Typical Failure Pattern)
```text
Agent A completes task A
Agent B fails task B
Scheduler A sees all tasks complete and marks run completed
Scheduler B marks run failed
Finalizer runs twice
UI receives contradictory events
```

### Gejala Potensial
- Runtime crash: `fatal error: concurrent map writes`.
- Counter tugas yang tersisa bernilai negatif.
- Run tercatat berstatus `completed` sekaligus `failed`.
- Duplikasi pembuatan artifact pada sistem file.
- Event terminal dikirimkan dua kali ke subscriber.
- Agen tetap berstatus `running` meskipun task telah gagal.
- Antarmuka menampilkan status tugas yang mustahil secara logika.

### Tugas Verifikasi yang Diperlukan (Verification Tasks)
- [ ] Jalankan verifikasi race detector pada modul orkestrator:
  ```bash
  cd engine
  go test -race ./...
  go test -race -count=20 ./src/crew
  go test -run TestStress -race -count=20 ./src/crew
  ```

### Desain Penanganan yang Direkomendasikan
Pilih salah satu pendekatan berikut secara konsisten:
1. **Mutex-protected aggregate**: Semua mutasi pada state run berada di bawah satu `sync.RWMutex`.
2. **Single-owner event loop**: Satu goroutine menjadi pemilik tunggal state run; pekerja mengirimkan event/perintah via channels.
3. **Transactional persistence**: Transisi status menggunakan compare-and-swap atau nomor versi.

Transisi status wajib divalidasi dan bersifat atomik:
```go
func (r *Run) Transition(from, to Status) error {
    if r.Status != from {
        return ErrInvalidStateTransition
    }
    r.Status = to
    return nil
}
```

*Dilarang keras melakukan assignment field secara telanjang tanpa validasi transisi:*
```go
// Tidak aman sebagai mekanisme transisi domain!
run.Status = StatusCompleted
```

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Seluruh pengujian `go test -race ./...` lulus tanpa temuan race condition.
- [ ] Stress test berhasil dieksekusi dengan puluhan worker paralel tanpa deadlock.
- [ ] Setiap run memancarkan tepat satu event terminal.
- [ ] Setiap task memancarkan tepat satu event terminal.
- [ ] Seluruh transisi status divalidasi dan bersifat idempoten.

---

## BUG-014 — Invalid or Non-Monotonic State Transitions

**Severity:** P1 / High  
**Status:** Potential  
**Affected areas:** Crew run state, task state, agent status, recovery console, UI reducer.

### Description
Tanpa validasi state machine yang eksplisit, status dapat melompat mundur atau melompati tahapan wajib. Hal ini kerap terjadi pada orkestrasi konkuren dan pengiriman stream event yang datang terlambat.

Contoh transisi status yang tidak valid:
```text
cancelled -> completed
failed -> running (tanpa melalui alur retry eksplisit)
completed -> waiting_for_approval
pending -> completed (tanpa melalui tahap assignment dan running)
waiting_for_input -> completed (tanpa adanya input balikan)
```

### State Machine yang Direkomendasikan

#### Run State Machine
```text
queued
  -> planning
  -> running
  -> waiting_for_input
  -> completed

queued | planning | running | waiting_for_input
  -> failed
  -> cancelling

cancelling
  -> cancelled
```

#### Task State Machine
```text
pending
  -> ready
  -> assigned
  -> running
  -> waiting_for_input
  -> completed

ready | assigned | running | waiting_for_input
  -> failed
  -> cancelled

failed
  -> retrying
  -> running
```

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Tabel transisi state diimplementasikan secara eksplisit dalam kode domain.
- [ ] Setiap percobaan transisi yang tidak valid menghasilkan *typed error* (`ErrInvalidStateTransition`).
- [ ] Klien UI mendukung dan mengenali seluruh nilai status yang valid dari backend.
- [ ] Status terminal tidak dapat digantikan atau ditimpa oleh event yang datang terlambat.

---

## BUG-015 — Recovery Console Actions Race With Live Agent Execution

**Severity:** P1 / High  
**Status:** Potential  
**Affected areas:** Web recovery console, Go crew/task service, retry/cancel/acknowledge actions.

### Description
Konsol pemulihan (*recovery console*) mengelola tugas-tugas dengan status bermasalah seperti `needs_review`, `lost`, `timed_out`, dan `failed`. Pengguna dapat menekan tombol retry, cancel, atau acknowledge pada UI saat engine sedang menerapkan event baru, melakukan retry otomatis, atau memfinalisasi task yang sama.

Contoh kondisi race:
```text
1. Task ditandai timed_out.
2. UI melakukan polling dan menampilkan tombol 'Retry'.
3. Engine tiba-tiba menerima hasil tool yang sukses meskipun terlambat.
4. Pengguna mengklik tombol 'Retry'.
5. Engine membuat eksekusi duplikat sementara eksekusi lama sedang difinalisasi.
```

### Pengamanan yang Wajib Diterapkan (Required Safeguards)
- [ ] Penerapan entity versioning atau *optimistic concurrency token*.
- [ ] Penyertaan header/parameter `Idempotency-Key` pada setiap aksi retry/cancel.
- [ ] Validasi transisi status di sisi server sebelum mengeksekusi aksi perbaikan.
- [ ] Maksimal satu attempt aktif per task kecuali alur kerja mengizinkan secara eksplisit.
- [ ] Respon aksi mengembalikan state dan nomor versi otoritatif terbaru.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Request retry/cancel/acknowledge yang dilakukan serentak tidak memicu pengerjaan tugas duplikat.
- [ ] Antarmuka recovery langsung memperbarui tampilannya dari respons otoritatif backend.
- [ ] Aksi UI yang basi (*stale UI action*) menghasilkan pesan konflik yang jelas (`409 Conflict`), bukan eksekusi liar.

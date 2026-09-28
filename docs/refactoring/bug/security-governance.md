# Security, Authorization & Sensitive Data Governance Audit

> **Category**: Tool Execution Security, Approval Gates, Sandbox Containment & Secret Redaction  
> **Status**: Potential Security Vulnerability Register  
> **Related Documents**: [README](./README.md) | [Action Backlog](./action-backlog.md) | [Verification & Testing](./verification-testing.md)

---

## BUG-005 — Tool Dispatcher Bypasses Approval, Authorization, or Workspace Policy

**Severity:** P0 / Critical  
**Status:** Potential security defect  
**Affected areas:** Tool dispatcher, HTTP/gRPC/Tauri entry points, MCP integration, workspace/file tool adapters.

### Description
Dispatcher tool diperkenalkan bersamaan dengan orkestrasi multi-agen. Dalam sistem multi-transport, terdapat banyak jalur potensial yang dapat memicu eksekusi sebuah tool:

```text
Web UI -> Tauri IPC -> Go sidecar -> Tool dispatcher
HTTP API -> Tool dispatcher
gRPC service -> Tool dispatcher
Agent worker -> Tool dispatcher
Test/helper/admin path -> Tool dispatcher
MCP server -> Tool dispatcher
```

Jika terdapat salah satu jalur yang memanggil fungsi eksekusi tool secara langsung tanpa melewati pemeriksaan otorisasi yang sama, persetujuan pengguna (*approval gate*), batasan ruang lingkup workspace, pencatatan jejak audit, dan pembatalan context, sistem dapat mengeksekusi aksi berbahaya tanpa izin.

### Aksi Berbahaya Berisiko Tinggi (High-Risk Side Effects)
- Menulis, memodifikasi, atau menghapus file lokal.
- Menjalankan perintah shell atau bash script.
- Menjalankan perintah Git dengan perilaku mutasi (commit, push, checkout force).
- Mengakses layanan jaringan eksternal yang tidak diizinkan.
- Mengubah konfigurasi aplikasi atau kredensial provider.
- Memanggil tool MCP (*Model Context Protocol*) dengan hak akses luas.

### Model Kebijakan Konteks Eksekusi yang Wajib Ada
Setiap permintaan eksekusi tool wajib menyertakan konteks eksekusi yang tidak boleh bernilai kosong/opsional:

```go
type ExecutionContext struct {
    ActorID       string
    SessionID     string
    WorkspaceID   string
    CrewID        string
    RunID         string
    TaskID        string
    RequestID     string
    ApprovalState string
    AllowedRoots  []string
}
```

### Pemeriksaan Keamanan yang Wajib Diterapkan (Required Security Controls)
- [ ] Menerapkan prinsip *Deny by Default*.
- [ ] Otorisasi ditegakkan langsung di dalam `ToolDispatcher` inti, bukan hanya di level UI.
- [ ] Pemeriksaan persetujuan (*approval check*) dievaluasi sesaat sebelum aksi mutasi dijalankan.
- [ ] Kanonikalisasi path file terhadap direktori kerja yang diizinkan (`WorkspaceRoot`).
- [ ] Penanganan dan penolakan *symlink escape* (symlink yang mengarah ke luar folder workspace).
- [ ] Parsing perintah shell menggunakan allowlist dan validasi token perintah, bukan sekadar pencocokan string biasa.
- [ ] Tool MCP menerima permission yang dibatasi berdasarkan cakupan kapabilitas (*capability-scoped*).
- [ ] Setiap pemanggilan tool memancarkan event audit lengkap.
- [ ] Setiap proses tool menerima propagasi context cancellation dari run induk.

### Uji Coba Negatif (Negative Tests Checklist)
- [ ] Pemanggilan dispatcher langsung tanpa identitas aktor (`ActorID`) wajib ditolak (`PermissionDenied`).
- [ ] Pemanggilan HTTP tanpa izin approval untuk tool kategori `WRITE` wajib ditolak.
- [ ] Pemanggilan gRPC yang mencoba mengeksekusi tool `WRITE` tanpa kebijakan izin wajib ditolak.
- [ ] Path argumen tool yang mengandung direktori traversal `../` wajib ditolak.
- [ ] Path argumen tool yang menggunakan symlink menuju ke luar workspace wajib ditolak.
- [ ] Tool yang dipicu setelah run dibatalkan wajib langsung dibatalkan/ditolak.
- [ ] Pengiriman approval ganda pada tool yang sama bersifat idempoten (tool hanya jalan tepat 1 kali).
- [ ] Permintaan approval di UI yang telah kedaluwarsa (*expired*) otomatis membatalkan tool.
- [ ] Output tool yang mengandung token rahasia otomatis disaring (*redacted*) sebelum disimpan ke log/event.

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Seluruh titik masuk (*entry points*) tool bermuara pada satu gerbang kebijakan keamanan yang seragam.
- [ ] Tidak ada metode eksekusi langsung yang dapat diakses dari lapisan delivery tanpa melalui validasi keamanan.
- [ ] Catatan audit tool memuat rekaman lengkap: request, keputusan approval, waktu mulai, selesai, kegagalan, dan pembatalan.

---

## BUG-017 — Logs, Traces, and Error Events May Expose Secrets or Sensitive Prompts

**Severity:** P1 / High  
**Status:** Potential security/privacy issue  
**Affected areas:** Centralized logging, metrics/log viewer, LLM prompts, tool output, Tauri logs, web logs UI.

### Description
Engine telah dilengkapi dengan logging tersentralisasi, metrik Prometheus, penampil log, event streaming, eksekusi tool, dan konfigurasi provider. Tanpa aturan redaksi data sensitif yang ketat, data rahasia dapat bocor ke dalam file log, event stream SSE, atau antarmuka UI melalui:
- API Keys LLM (OpenAI, Anthropic, Gemini).
- Header otorisasi HTTP (`Authorization: Bearer ...`).
- Konfigurasi provider AI.
- Argumen perintah tool (misal token di parameter CLI).
- Variabel lingkungan (*environment variables*).
- Isi file sensitif yang dibaca oleh agen.
- Prompt dan respons LLM yang memuat data pribadi.
- Record memori dan embedding.
- Pesan kesalahan (error trace) yang mencetak ulang seluruh payload request mentah.

### Pengendalian yang Wajib Diterapkan (Required Controls)
- [ ] Logging terstruktur dengan penandaan eksplisit untuk field-field sensitif.
- [ ] Middleware redaksi data otomatis sebelum pesan disimpan ke disk atau dipancarkan ke event stream.
- [ ] Pembersihan header otorisasi mentah agar tidak pernah tercatat di log HTTP.
- [ ] Kunci API provider hanya disimpan dan ditampilkan dalam bentuk referensi tersandi (*masked reference*, misal: `sk-...xxxx`).
- [ ] Kebijakan batasan ukuran dan konten pada output tool.
- [ ] Pemisahan tingkat detail diagnostik internal dari pesan error yang disajikan kepada pengguna biasa.

### Kasus Pengujian (Test Cases Checklist)
- [ ] Picu error provider dengan data secret palsu; pastikan secret tidak muncul di file log maupun event stream.
- [ ] Eksekusi tool dengan token rahasia di argumennya; pastikan log mencatat argumen dalam bentuk ter-redaksi.
- [ ] Simpan memori dengan nilai sensitif; pastikan UI normal tidak merender isi mentah tanpa otorisasi.
- [ ] Lakukan serialisasi objek konfigurasi; pastikan API key selalu disamarkan (*masked*).

### Kriteria Penerimaan (Acceptance Criteria)
- [ ] Kebijakan redaksi data sensitif terdokumentasi dan diterapkan secara konsisten.
- [ ] Pengujian otomatis memverifikasi penyaringan rahasia di seluruh lapisan: HTTP, gRPC, Tauri, dan Web Log Viewer.

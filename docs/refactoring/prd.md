# Product Requirements Document (PRD)
## ClawCrew: Hybrid Rust & Go 1.27 Architecture

### 1. Tujuan Produk
Meningkatkan kapabilitas ClawCrew menjadi platform orkestrasi **Multi-Agent** sekelas Kiro Crew, tanpa mengorbankan performa *desktop native*, keamanan sistem, dan stabilitas akses perangkat keras (hardware) yang selama ini berjalan di atas Rust.

### 2. Latar Belakang & Masalah Saat Ini
ClawCrew saat ini berjalan 100% menggunakan bahasa Rust (via Tauri). Meskipun Rust sangat cepat dan aman, mengorkestrasi puluhan hingga ratusan "Crew" (Agent) secara paralel menggunakan `async/await` Rust sangatlah rumit dan berisiko tinggi terhadap *deadlock* memori (akibat penggunaan `Arc<Mutex<T>>`). 
Selain itu, *streaming* respon JSON LLM secara masif lebih efisien ditangani menggunakan pemrosesan *garbage-collected* yang sangat ringan.

### 3. Solusi (Hybrid Architecture)
Kita akan memisahkan peran sistem menjadi dua:
1. **Rust (Platform Core)**: Tetap menangani UI (Tauri), keamanan *Secret Vault*, validasi TOML, eksekusi terminal (bash/pwsh sandbox), dan akses serial port.
2. **Go 1.27 (AI Brain / Orchestrator)**: Khusus menangani logika multi-agent, *prompting*, parsing LLM, tool dispatching, dan memori vektor lokal.

Mengapa Go 1.27?
- **Goroutines & Channels**: Model konkurensi native Go sempurna untuk mendelegasikan tugas ke sub-agent (setiap agen hidup di *goroutine*-nya sendiri).
- **`encoding/json/v2`**: Kecepatan *parsing* *streaming tokens* dan *tool calls* menjadi instan.
- **SIMD**: Native vector similarity search yang cepat untuk konteks jangka panjang/RAG tanpa butuh *database server* terpisah.

### 4. Metrik Keberhasilan (Success Criteria)
- Agen dapat bekerja secara paralel dengan aman tanpa menyebabkan *crash* pada aplikasi desktop.
- Kecepatan pemrosesan token (*Time-to-First-Chunk* / TTFC) meningkat atau stabil.
- Pengguna tetap menginstall 1 aplikasi tunggal (Tauri membundel *Sidecar* Go).
- Integrasi ke Dashboard React/Next.js eksisting tetap mulus.

# Technical Specification
## ClawCrew Hybrid Engine (Rust + Go 1.27)

### 1. High-Level Architecture
Sistem akan menggunakan **Tauri Sidecar Pattern**.
Aplikasi Rust akan meluncurkan `go-agent-engine` (binary terpisah yang dicompile dari Go) sebagai *child process* pada saat aplikasi *start*.

Keduanya berkomunikasi melalui **Local gRPC over TCP/Domain Socket** atau **JSON-RPC over stdin/stdout** (standard *stdio* IPC). Mengingat sifat interaktif Agent yang butuh *two-way streaming*, **gRPC** adalah pilihan terbaik.

```mermaid
flowchart TD
    UI[Frontend React/Vite] <-->|HTTP/WS| Gateway(Rust Gateway API)
    
    subgraph Rust Core
    Gateway <--> Config(TOML Config Vault)
    Gateway <--> Hardware(Hardware/OS Gateway)
    Gateway <--> Tauri(Tauri System Shell)
    end
    
    Gateway <-->|gRPC (localhost:50051)| GoEngine(Go 1.27 Agent Engine)
    
    subgraph Go 1.27 AI Engine
    GoEngine <--> Orchestrator(Crew Orchestrator / Goroutines)
    Orchestrator <--> LLM(LLM Stream Parsers json/v2)
    Orchestrator <--> Memory(SIMD Vector Memory)
    end
    
    GoEngine -.->|gRPC Call| Hardware
```

### 2. Porsi Tanggung Jawab (Separation of Concerns)

#### A. Rust (`crates/`)
- **`clawcrew-config`**: Parsing `.toml`, validasi makro `#[natural_key]`, enkripsi/dekripsi credential (API Keys).
- **`clawcrew-runtime`**: Pengelolaan lifecycle daemon Go, manajemen *Sidecar* Tauri.
- **`clawcrew-hardware`**: Menjalankan fungsi-fungsi berbahaya/tingkat rendah (OS bash, file explorer, USB/Serial interface). *Tools* ini terekspos via gRPC untuk dipanggil oleh Go.

#### B. Go (`engine/`)
- **`cmd/agent-engine`**: Entry point untuk *Sidecar* server gRPC.
- **`internal/crew`**: Logika orkestrasi `Subagents`. Setiap agen mendelegasikan tugas ke sub-agen lain via Go Channels.
- **`internal/llm`**: Koneksi ke provider AI (OpenAI, Gemini). Memanfaatkan fitur *native* Go 1.27 seperti `encoding/json/v2` untuk serialisasi yang sangat ringan.
- **`internal/memory`**: *Vector storage* untuk *Retrieval-Augmented Generation* (RAG) secara in-memory.

### 3. Ekosistem Go 1.27
- **Toolchain**: Go 1.27.
- **Protobuf**: Menggunakan `protoc-gen-go` dan `protoc-gen-go-grpc`.
- **Parsing**: `encoding/json/v2` (performa tinggi).
- **State Management**: native `uuid` dari standard library.

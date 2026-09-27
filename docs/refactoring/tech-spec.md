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
    UI -.->|HTTP GET :9090| Metrics(Prometheus / Metrics)
    UI -.->|SSE / Read File| Logs(Centralized Logs)
    
    subgraph Go 1.27 AI Engine (Modular Clean Architecture)
    GoEngine <--> Delivery(gRPC Delivery Layer)
    Delivery <--> Usecase(Service / Usecase Layer)
    Usecase <--> Repo(Repository / Adapter Layer)
    
    Repo <--> Memory(SIMD Vector Memory)
    Repo <--> LLM(LLM APIs JSON v2)
    end
    
    Usecase -.->|gRPC Client| Hardware
```

### 2. Arsitektur Modular (Clean Code) & Google Wire
Proyek Go (`engine/`) akan diorganisasikan menggunakan struktur **Clean Architecture Modular**, di mana setiap domain/fitur (misal: `crew`, `llm`, `memory`) berdiri secara independen di dalam direktori `src/`.

**Struktur Direktori**:
- `src/<domain>/delivery.go`: Handler gRPC/HTTP yang bertugas mem-parsing request dan mengirim respons (termasuk *error handling* awal).
- `src/<domain>/services.go` (*Usecase*): Berisi *business logic* (orkestrasi agen, *prompting*).
- `src/<domain>/interfaces.go`: Kontrak abstraksi agar modul lain tidak *tightly coupled*.
- `src/<domain>/dto.go`: Data Transfer Objects.
- `src/<domain>/wire.go` & `wire_gen.go`: Integrasi **Google Wire** untuk dependency injection secara *compile-time*.

**Google Wire** akan digunakan di setiap domain serta pada *entry point* (seperti `app/wire.go`) untuk menyatukan semua dependensi secara *type-safe* dan bebas *magic/reflection*.

### 3. Porsi Tanggung Jawab (Separation of Concerns)

#### A. Rust (`crates/`)
- **`clawcrew-config`**: Parsing `.toml`, validasi, *Secret Vault*.
- **`clawcrew-runtime`**: Pengelolaan lifecycle daemon Go.
- **`clawcrew-hardware`**: Eksekusi *tool* sistem operasi, diekspos ke Go via gRPC.

#### B. Go (`engine/`)
Terdiri atas berbagai modul dalam `src/`:
- **`src/crew`**: Orkestrasi agen via goroutines.
- **`src/llm`**: Integrasi provider AI via `encoding/json/v2`.
- **`src/memory`**: Vector/RAG in-memory storage (SIMD).
- **`core/errors`**: Paket *error handling* tersentral.
- **`core/logger`**: Layanan logging ke file dan CLI.

### 4. Logging & Observability (Metrics)

#### A. Fleksibilitas File Logs & Integrasi UI
- Go Engine akan menggunakan pustaka logging modern (seperti `slog` atau `zap`) yang dikonfigurasi jalurnya (path) berdasarkan parameter *startup* dari config Rust.
- **File Rotation**: Log ditulis ke file spesifik (contoh: `%APPDATA%/clawcrew/logs/agent.log`) menggunakan `lumberjack` agar tidak memenuhi disk.
- Log berformat JSON terstruktur sehingga **UI Dashboard (React/Desktop)** dapat membaca file tersebut secara langsung, menampilkannya sebagai *console* terpadu, atau menariknya secara *real-time* lewat Server-Sent Events (SSE) / *File watcher*.

#### B. Metrik Prometheus
- Go Engine akan meng-*expose* HTTP endpoint (misal `:9090/metrics`) menggunakan `github.com/prometheus/client_golang`.
- **UI Dashboard** dapat langsung melakukan *fetching* metrik ini untuk menampilkan visualisasi:
  - Jumlah agen berjalan (`agent_active_goroutines`).
  - Total konsumsi token LLM (`llm_token_usage_total`).
  - Durasi penyelesaian *turn* (`agent_turn_duration_seconds`).
  - Error rate per layer (`core_error_count`).

### 5. Clean Error Handling
Standarisasi Error disentralisasi pada direktori `core/errors`.
- Setiap kesalahan yang terjadi di lapisan *Repository* atau *Service* dibungkus (`wrapped`) dengan custom error struct yang berisi `Code` (enum/konstanta), `Message`, dan `Layer` asal (misal: "LLM_PROVIDER_ERROR").
- Lapisan *Delivery* akan memetakan *custom error* ini menggunakan `errors.As` / `errors.Is` menjadi gRPC Status Codes (`codes.Internal`, `codes.InvalidArgument`, dll) atau JSON HTTP standard.
- Semua *error* tercatat *(logged)* secara rapi oleh lapisan middleware/interceptor, sehingga *business logic* di lapisan `services.go` tetap bersih dari repetisi log *error*.

### 6. Ekosistem Go 1.27
- **DI & Struktur**: Google Wire (`github.com/google/wire`).
- **Observability**: Prometheus & `slog`/`zap`.
- **Parsing**: `encoding/json/v2` (performa tinggi).
- **State Management**: native `uuid` dari standard library.

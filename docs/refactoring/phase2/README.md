# Claw-Crew Phase 2 Refactoring: Architecture, Migration & Product Evolution

> **Branch**: `feat/enhance-agent-phase2`  
> **Status**: Living Architectural Documentation & Roadmap  
> **Parent Directory**: [`docs/refactoring/`](../)

---

## 1. Executive Summary & Purpose

Dokumentasi ini menetapkan arsitektur target, batas kepemilikan (*ownership boundaries*), spesifikasi teknis modular Go, kontrak API, serta rencana migrasi bertahap untuk **Claw-Crew Phase 2**.

Tujuan utama Phase 2 **bukan** memindahkan seluruh kode Rust secara membabi-buta ke Go. Tujuannya adalah membagi tanggung jawab sistem secara tegas dan elegan:

- **Go 1.27 (`engine/`)**: Bertindak sebagai **AI Brain & Execution Engine**. Menjadi *single source of truth* (kanonikal) untuk orkestrasi kru, agen, *run lifecycle*, *task graph*, loop pemanggilan *tool*, memori/RAG, *workflow/SOP execution*, kebijakan persetujuan (*approval policy*), abstraksi LLM, serta *event streaming* (SSE/gRPC).
- **Rust (`apps/zerocode`, `crates/`)**: Bertindak sebagai **Native Platform & Presentation Shell**. Tetap mengelola UI terminal berkecepatan tinggi (TUI), kenyamanan keyboard/mouse, rendering ANSI, *workspace file browsing*, akses *terminal backend*, dan *secret vault*.
- **Client Contract Terstandarisasi**: Kontrak API REST + Server-Sent Events (SSE) dan IPC lokal memungkinkan TUI Rust, desktop Tauri, dan Web UI masa depan mengonsumsi Go engine yang sama tanpa menduplikasi logika bisnis.
- **Evolusi Produk Terinspirasi KiroCrew**: Mengadopsi prinsip transparansi eksekusi KiroCrew: visibilitas kru yang jelas, status agen terstruktur, *task timeline* real-time, *approval gates* untuk aksi berisiko, serta tampilan terpusat pada *output artifacts* dan *progressive disclosure* log.

---

## 2. Peta Dokumen Phase 2

Dokumentasi Phase 2 dikelompokkan ke dalam file-file spesifik berikut:

| Dokumen | Deskripsi & Fokus |
|---|---|
| [**`prd.md`**](./prd.md) | **Product Requirements Document (PRD)**: Latar belakang masalah, target pengguna, prinsip UX KiroCrew, tujuan (*goals/non-goals*), kebutuhan fungsional, dan metrik keberhasilan. |
| [**`tech-spec.md`**](./tech-spec.md) | **Technical Specification**: Arsitektur modular Clean Architecture di `engine/src/`, integrasi **Google Wire**, standarisasi **Go Idioms**, *domain model*, *state machines*, event streaming SSE, dan strategi pengujian. |
| [**`api-spec.md`**](./api-spec.md) | **API Specification**: Kontrak endpoint REST `/api/v1`, format streaming SSE (`/events`), standarisasi *error envelope*, DTO request/response, serta endpoint diagnostik & health. |
| [**`task-breakdown.md`**](./task-breakdown.md) | **Task Breakdown & Implementation Plan**: Rencana implementasi bertahap (Phase 0 hingga Phase 7) dengan *checkbox* kosong `[ ]`, *Definition of Done*, serta *Immediate Next Actions*. |
| [**`migration-matrix.md`**](./migration-matrix.md) | **Migration Analysis & Architecture Decision Records (ADR)**: Matriks kepemilikan kapabilitas Rust vs Go, analisa kandidat migrasi prioritas tinggi, 10 ADR resmi, dan strategi mitigasi risiko. |

---

## 3. High-Level Architecture Topology

```mermaid
flowchart TD
    subgraph Presentation Layer [Presentation & Local Shell - Rust / Tauri / Web]
        TUI[Rust Zerocode TUI\napps/zerocode]
        Tauri[Tauri Desktop Shell\napps/tauri]
        Web[Future Web Client\nweb/]
    end

    subgraph Transport [Client-Engine Boundary]
        HTTP[REST / JSON v2\n/api/v1/*]
        SSE[Server-Sent Events\n/api/v1/runs/:id/events]
        GRPC[Local gRPC IPC\nlocalhost:50051]
    end

    subgraph GoEngine [Go 1.27 Agent Engine - engine/]
        direction TB
        AppRoot[Composition Root\nengine/app/wire.go]
        Core[Shared Infrastructure\nengine/core/{config,errors,logger,metrics}]

        subgraph ModularDomains [Modular Domains - engine/src/]
            RunMod[src/run\nRun Lifecycle & SSE]
            TaskMod[src/task\nTask Graph & Scheduler]
            CrewMod[src/crew\nCrew & Agent Orchestration]
            WorkflowMod[src/workflow\nSOP & Template Engine]
            ToolMod[src/tool\nTool Registry & Approvals]
            LLMMod[src/llm\nProvider & Dispatcher]
            MemMod[src/memory\nVector Store & RAG]
            ArtMod[src/artifact\nDiffs & Output Artifacts]
        end
    end

    subgraph ExternalServices [External Boundaries & Hardware]
        LLMProviders[LLM APIs\nOpenAI / Gemini / Anthropic]
        NativeOS[OS Tools / Bash / Filesystem\nvia Rust System Gateway]
        LocalVault[Secret Vault\nRust Config Vault]
    end

    TUI <-->|REST + SSE| HTTP
    TUI <-->|Local IPC| GRPC
    Tauri <-->|REST + SSE| HTTP
    Web <-->|REST + SSE| HTTP

    HTTP --> RunMod
    SSE --> RunMod
    GRPC --> CrewMod

    AppRoot --> ModularDomains
    Core -.-> ModularDomains

    RunMod --> TaskMod
    RunMod --> CrewMod
    TaskMod --> CrewMod
    CrewMod --> LLMMod
    CrewMod --> ToolMod
    CrewMod --> MemMod
    WorkflowMod --> TaskMod
    ToolMod --> ArtMod

    LLMMod --> LLMProviders
    ToolMod <-->|gRPC SystemGateway| NativeOS
    AppRoot <-->|Config IPC| LocalVault
```

---

## 4. Prinsip Pembagian Tanggung Jawab

| Aspek / State | Pemilik Kanonikal (Single Source of Truth) | Perilaku Klien (Rust TUI / Tauri / Web) |
|---|---|---|
| **Definisi Kru & Agen** | **Go Engine (`engine/src/crew`)** | Menampilkan daftar, memilih kru via API. |
| **Siklus Hidup Run** | **Go Engine (`engine/src/run`)** | Memulai, memantau progress, meminta pembatalan (*cancel*). |
| **Status Task / Todo Graph** | **Go Engine (`engine/src/task`)** | Memproyeksikan visual timeline, daftar todo dari event Go. |
| **Loop & Eksekusi Tool** | **Go Engine (`engine/src/tool`)** | Menampilkan dialog approval untuk aksi berisiko, menerima hasil. |
| **Orkestrasi LLM & Streaming** | **Go Engine (`engine/src/llm`)** | Menampilkan token stream secara real-time di layar. |
| **SOP & Workflow Semantics** | **Go Engine (`engine/src/workflow`)** | Memilih template via quickstart/gallery, mengeksekusi via API. |
| **Output Artifacts & Diffs** | **Go Engine (`engine/src/artifact`)** | Merender tampilan visual diff, preview file output. |
| **Memori & Konteks RAG** | **Go Engine (`engine/src/memory`)** | Menginspeksi konteks memori yang digunakan agen. |
| **Rendering TUI Terminal** | **Rust (`apps/zerocode`)** | Pemrosesan ANSI, mouse/cursor, layout terminal murni lokal. |
| **Keybindings & Window/Panes**| **Rust (`apps/zerocode`)** | Navigasi shortcut, switching tabs/panes murni lokal. |
| **Workspace File Explorer** | **Rust (`apps/zerocode`)** | Navigasi tree direktori lokal, mengirim path ke Go API. |
| **Draft Chat Input** | **Rust / Web** | State teks lokal sebelum tombol submit ditekan. |

---

## 5. Ringkasan Arah Evolusi KiroCrew

KiroCrew memberikan inspirasi produk yang krusial bagi Claw-Crew:

1. **Visibilitas Kru Terang-benderang**: Pengguna selalu tahu agen mana yang sedang aktif, siapa yang menunggu, dan siapa yang selesai.
2. **Task Timeline Nyata**: Daftar tugas bukan sekadar catatan teks lokal, melainkan *directed acyclic graph* (DAG) yang dikelola Go Engine.
3. **Approval Gate yang Tenang & Aman**: Alat berbahaya (menulis file, menjalankan bash) meminta izin pengguna secara terstruktur tanpa merusak alur chat.
4. **Sentrisitas pada Artifact**: Hasil kerja agen (diff git, laporan, kode baru) dikelompokkan sebagai artifact yang mudah ditinjau (*review-first*).
5. **Progressive Disclosure**: Log debug mentah disembunyikan secara rapi di belakang antarmuka utama, siap dibuka kapan saja diperlukan tanpa membuat panik pengguna awam.

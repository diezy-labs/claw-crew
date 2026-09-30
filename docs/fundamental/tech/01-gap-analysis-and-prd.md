# Galleon: Gap Analysis, PRD, & Architecture Spec

Dokumen ini merupakan hasil audit teknis mendalam terhadap kode *existing* di dalam `engine/src/` bahasa Go, dibandingkan dengan dokumen visi **Galleon** (sebelumnya Claw-Crew). 

Tujuan utama dokumen ini adalah **menghindari redundansi** (penulisan kode berulang) dan merencanakan migrasi *naming convention* serta penambahan struktur yang benar-benar baru.

## 1. Gap Analysis: Existing Code vs Galleon Vision

Setelah memeriksa isi folder `engine/src/`, terlihat bahwa sistem ini sudah memiliki *core orchestration* yang sangat matang (berbasis *event-streaming* dan *turn-based state machine*).

### A. Apa yang SUDAH ADA (Bisa Digunakan Ulang / Ganti Nama)
Sistem *engine* saat ini telah mengimplementasikan komponen-komponen berat berikut:
1.  **State Machine & Event Streaming (`engine/src/crew/interfaces.go` & `dto.go`)**
    *   *Existing:* Memiliki `Orchestrator.StartTurn`, `TurnEvent` (`THOUGHT_CHUNK`, `TOOL_CALL`), dan `AgentStatus`.
    *   *Pemetaan ke Galleon:* Ini adalah fondasi dari **Run / Voyage** (eksekusi tugas). Tidak perlu membuat dari nol. Kita hanya perlu mengintegrasikannya ke level `Ship`.
2.  **Grouping Agen (`engine/src/crew/dto.go`)**
    *   *Existing:* `CrewDefinition` (berisi *array* dari `AgentDefinition`).
    *   *Pemetaan ke Galleon:* Terdapat ketidakselarasan istilah. Di *existing code*, `Crew` adalah **Grup**. Di Galleon, **Grup** adalah `Squad`, dan individunya adalah `Crew Member`.
    *   **Action:** *Refactor/Rename* struktur data ini. `CrewDefinition` menjadi `Squad`, dan `AgentDefinition` menjadi `Crew`.
3.  **LLM Dispatcher & Memory (`engine/src/llm`, `engine/src/memory`)**
    *   *Existing:* Sangat lengkap (ada *packer*, *vector validation*, *multi-provider*).
    *   *Action:* Tidak perlu disentuh, ini sudah mendukung visi *BYOK/BYOM*.
4.  **Tooling & Sandbox (`engine/src/tool/`)**
    *   *Existing:* Memiliki `approval_gate.go`, `registry.go`, `sandbox.go`.
    *   *Action:* Ini luar biasa lengkap. `approval_gate` sudah ada. Kita hanya perlu menarik konfigurasi batasannya (*limits*) dari *package* `policy` baru.

### C. Desktop (Tauri) & Web (React) Integration Gap
Galleon beroperasi dengan prinsip *Separation of Concerns* yang ketat (Go untuk Otak/AI, Rust untuk Keamanan/Host, React untuk Wajah/UI).
1.  **Frontend API Consumption:** 
    *   *Gap:* React Dashboard saat ini mungkin masih menembak *endpoint* lama (misal: `/api/v1/crews`).
    *   *Action:* UI React perlu disesuaikan untuk memanggil *endpoint* Galleon yang baru (misal: `api/v1/squads`, `api/v1/fleets`).
2.  **Tauri (Rust) vs Go IPC:**
    *   *Gap:* Tauri bertugas menjalankan *engine* Go sebagai *sidecar* daemon. 
    *   *Action:* Pastikan *script build* Tauri membundel *binary* Go hasil kompilasi `engine/` dengan benar, dan aplikasi React di dalam Tauri berkomunikasi dengan *localhost port* dari Go Engine tersebut.

### D. Apa yang BELUM ADA (Harus Dibuat Baru)
Sistem saat ini sangat fokus pada "Satu Tim Agen menjalankan Satu Tugas". Belum ada konsep *Multi-Tenancy* organisasi berskala besar.
1.  **Level Makro (Fleet & Ship):** `engine/src/fleet` dan `engine/src/ship` yang baru di-*scaffold* benar-benar entitas baru. Diperlukan untuk membungkus `Squad`.
2.  **Quartermaster (Executive Orchestrator):** `engine/src/orchestrator` harus bertindak sebagai AI Meta-Agent (mengambil keputusan tingkat Fleet, bukan sekadar *Turn* level bawah).
3.  **Policy Engine Global:** `engine/src/policy/interfaces.go` (menyatukan *FleetCode* untuk budget dan akses).

---

## 2. Product Requirements Document (PRD) - Tahap 1 (Transisi Arsitektur)

### Objektif
Menyelaraskan struktur data *engine* Go dengan taksonomi Galleon tanpa merusak *workflow/turn-execution* yang sudah berjalan stabil.

### Requirements (Tugas Teknis)
*   **[REQ-1] Naming Refactoring (Squad & Crew):** 
    *   Ubah *package* `crew` (jika difungsikan sebagai grup) menjadi `squad`.
    *   Ubah nama `CrewDefinition` menjadi `Squad`.
    *   Ubah nama `AgentDefinition` menjadi `CrewMember` atau `Crew`.
*   **[REQ-2] Hierarki Data (Aggregate Roots):**
    *   Setiap `Squad` harus memiliki `ShipID` (merujuk ke kapal tempat mereka bertugas).
    *   Setiap `Ship` harus memiliki `FleetID` (merujuk ke kepemilikan/Owner).
*   **[REQ-3] Implementasi Quartermaster:**
    *   Buat *service* di `engine/src/orchestrator` yang memiliki akses ke `Fleet`, dapat membaca laporan dari `Mission Board`, dan merekomendasikan pembentukan `Squad` baru (Fitur *Squad Builder* AI).
*   **[REQ-4] Integrasi Approval Gate dengan FleetCode:**
    *   Modifikasi `engine/src/tool/approval_gate.go` agar membaca regulasi keamanan (*RiskClass*, *Spend Limit*) dari `engine/src/policy.FleetCode`.

---

## 3. Architecture Flow (Galleon Engine)

Berikut adalah diagram arsitektur interaksi (*existing* vs *new*) yang akan kita bangun di Go:

```mermaid
flowchart TD
    subgraph Galleon Fleet (Macro Level - NEW)
        PK[Pirate King / User] --> QM[Quartermaster (orchestrator.Service)]
        QM --> F[Fleet Registry (fleet.Service)]
        F --> SH[Ship (ship.Ship)]
        PK --> PE[Policy Engine (policy.FleetCode)]
    end

    subgraph Operation Level (Refactored Existing)
        SH --> SQ[Squad (formerly CrewDefinition)]
        SQ --> CM1[Crew Member (formerly AgentDefinition)]
        SQ --> CM2[Crew Member 2]
    end

    subgraph Execution Level (Stable Existing)
        CM1 --> OR[Turn Orchestrator (crew.Orchestrator)]
        OR --> LLM[LLM Dispatcher (llm.Provider)]
        OR --> MEM[Memory Packer (memory.Service)]
        OR --> TG[Tool Gate (tool.ApprovalGate)]
        PE -. "Enforces Rules" .-> TG
    end
```

### Panduan Implementasi Selanjutnya
Untuk menjaga agar PR dan *commit* tetap rapi sesuai standar `AGENTS.md` ("*Keep one concern per PR*"):
1.  **Langkah 1:** Lakukan eksekusi REQ-1 (Refactoring DTO di dalam *package* `crew`).
2.  **Langkah 2:** Isi logika *CRUD/Persistence* sederhana untuk `Fleet` dan `Ship`.
3.  **Langkah 3:** Mulai memprogram `Quartermaster` untuk fungsi *Objective Routing*.

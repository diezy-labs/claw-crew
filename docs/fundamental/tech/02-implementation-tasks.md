# Galleon: Implementation Task Tracker

Dokumen ini berisi daftar urutan tugas (*tasks*) implementasi teknis untuk transisi arsitektur Galleon di dalam `engine/src/`. Tugas disusun dari level kompleksitas terendah (Refactoring) hingga tertinggi (Meta-AI Logic/Quartermaster).

Gunakan *checkbox* di bawah ini sebagai penanda progres pengembangan (*Development Tracker*).

## Phase 1: Structural Refactoring (Low Complexity)
Fokus pada penyesuaian istilah agar selaras dengan taksonomi Galleon tanpa mengubah *logic* utama.

- [x] **Task 1.1:** *Rename* `CrewDefinition` menjadi `Squad` di `engine/src/crew/dto.go`.
- [x] **Task 1.2:** *Rename* `AgentDefinition` menjadi `CrewMember` di `engine/src/crew/dto.go`.
- [x] **Task 1.3:** Perbarui semua *method signatures* di `engine/src/crew/interfaces.go` yang menggunakan struktur data lama (misal: `RegisterCrew` menjadi `RegisterSquad`).
- [x] **Task 1.4:** Perbaiki *unit test* di `engine/src/crew/crew_test.go` agar menggunakan *struct* yang baru dan memastikan tes lulus (*pass*).
- [x] **Task 1.5:** (Opsional) *Rename folder* `crew` menjadi `squad` jika ingin secara struktur *package* lebih presisi merepresentasikan pengelompokan.

## Phase 2: Macro Aggregate Definitions (Low-Medium Complexity)
Membangun cangkang organisasi (*multi-tenancy*) untuk menampung *Squads*.

- [ ] **Task 2.1:** Definisikan skema *persistence* (penyimpanan DB/Disk) untuk agregat `Fleet` di `engine/src/persistence/disk_store.go`.
- [ ] **Task 2.2:** Definisikan skema *persistence* untuk agregat `Ship`.
- [x] **Task 2.3:** Implementasikan fungsi Create/Get/List di `engine/src/fleet/services.go`.
- [x] **Task 2.4:** Implementasikan fungsi Create/Get/List di `engine/src/ship/services.go`.
- [x] **Task 2.5:** Tambahkan atribut `ShipID` pada data model `Squad` agar relasi hierarki tersambung.

## Phase 3: Governance & Policy Wiring (Medium Complexity)
Menghubungkan sistem regulasi keamanan ke mesin eksekusi *tools*.

- [x] **Task 3.1:** Implementasikan fungsi untuk me-*load* (membaca) konfigurasi `FleetCode` di `engine/src/policy/services.go`.
- [ ] **Task 3.2:** Modifikasi konstruktor `engine/src/tool/approval_gate.go` agar menerima dependensi dari *policy*.
- [ ] **Task 3.3:** Tambahkan logika di `approval_gate.go`: Jika *tool* memiliki level `RiskClassWrite` atau `RiskClassDestructive`, hentikan eksekusi dan wajibkan konfirmasi dari Pirate King (User).
- [x] **Task 3.4:** Implementasikan pembentukan `ActionDigest` di *package* `approval` sebagai bukti kriptografis/deterministik atas apa yang akan dieksekusi *tool*.

## Phase 4: The Quartermaster (High Complexity)
Membangun agen eksekutif (meta-agent) yang mengatur keseluruhan *Fleet*.

- [x] **Task 4.1:** Implementasikan `orchestrator.Service` (*Quartermaster Service*).
- [x] **Task 4.2:** Buat fungsi *Objective Parsing*: Quartermaster menerima *prompt* target global dan memecahnya menjadi daftar misi.
- [x] **Task 4.3:** Buat fitur *Squad Builder*: Quartermaster melakukan panggilan LLM untuk merancang spesialis (Crew Member) apa saja yang dibutuhkan berdasarkan misi, lalu menyimpannya sebagai *draft Squad*.
- [ ] **Task 4.4:** Buat *endpoint/fungsi* validasi di mana Pirate King menyetujui, mengedit, atau menolak rancangan Squad dari Quartermaster.
- [ ] **Task 4.5:** Implementasikan transisi status `Quest`: dari `backlog` -> dilempar ke `Mission Board` -> dijemput oleh `Ship` -> dieksekusi oleh `Squad`.

## Phase 5: End-to-End Integration Tests (High Complexity)
- [ ] **Task 5.1:** Buat simulasi tes E2E: Pirate King membuat *Fleet*, Quartermaster membentuk *Squad*, Squad dieksekusi pada *Ship*, dan dihalangi oleh *Policy Engine* secara otomatis ketika mencoba tindakan destruktif.

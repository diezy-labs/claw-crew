# 01 — Quartermaster Router Design (dua-lapis)

> Phase-4 finalize. Mengunci perilaku Quartermaster. Menang atas deskripsi informal mana pun.
> Dasar: Anthropic *Building Effective Agents* (Routing ≠ Orchestrator-workers), permission profile `docs/re-branding/07`, `04`.

## 1. Prinsip inti

> **Quartermaster adalah ROUTER, bukan super-agent.** Ia satu pintu untuk Pirate King: menjawab chat ringan langsung, dan merutekan kerja berat ke jalur yang sudah terdefinisi. Ia **propose/report/coordinate**, tidak pernah eksekusi aksi high-impact sendiri.

Alasan arsitektur (bukan selera):
- **Router murah & deterministik**; orchestrator-worker mahal (~2–15× token) & gagal diam-diam. Pakai orchestrator HANYA saat subtask tak bisa diprediksi.
- Kategori intake Quartermaster **bisa dinamai di muka** (chat/engine_room/report/objective) → Router, bukan orchestrator.
- Orchestrator-worker sesungguhnya ada di **Captain** (`crew.StartTurn`): mendekomposisi objective yang sudah disetujui jadi kerja Crew. Di sanalah biaya 15× dibayar, dan hanya saat perlu.

## 2. Alur dua-lapis

```
Pirate King
   │
   ▼
┌─────────────── Quartermaster (SATU PINTU) ───────────────┐
│ LAPIS 1 — Intent Router (1 LLM call murah / rule-first)  │
│   klasifikasi pesan → QuartermasterIntent                 │
└───────────────────────────────────────────────────────────┘
   │
   ├─ chat        → jawab LANGSUNG via modelgateway (asisten pribadi).           Tanpa delegation.
   ├─ engine_room → baca telemetry (RAM/CPU/health/budget) read-only.           Tanpa LLM delegation.
   ├─ report      → tarik Ship Summary Projection → Fleet Report (ringkas).     Propose/summarize.
   └─ objective   → QuartermasterService.IntakeObjective
                      → FleetOrderProposal  (status: awaiting_pirate_king_approval)
                           │  (Pirate King approve)
                           ▼
                      LAPIS 2 — Captain (crew.StartTurn) = ORCHESTRATOR-WORKER
                           → dekomposisi → Squad → Crew → Treasure → Ship Report
```

## 3. QuartermasterIntent (SSOT di Go)

Enum tunggal, didefinisikan di engine; UI/klien hanya konsumen, tak pernah menebak intent.

```go
// QuartermasterIntent — hasil klasifikasi Lapis 1. SSOT: hanya di Go.
type QuartermasterIntent string

const (
    IntentChat       QuartermasterIntent = "chat"        // pertanyaan/percakapan umum → jawab langsung
    IntentEngineRoom QuartermasterIntent = "engine_room" // status mesin: RAM/CPU/health/budget → telemetry
    IntentReport     QuartermasterIntent = "report"      // minta laporan/status fleet/ship → summary projection
    IntentObjective  QuartermasterIntent = "objective"   // minta kerja/quest → FleetOrderProposal (gated)
)
```

Hasil klasifikasi membawa `confidence` + `reason` (untuk instrumentasi & fallback).

## 4. Perilaku tiap cabang

| Intent | Perilaku | Authority | Output |
|--------|----------|-----------|--------|
| `chat` | Jawab langsung via LLM (modelgateway). Boleh baca memory Fleet yang diizinkan. | Read-only | Balasan chat |
| `engine_room` | Baca telemetry host/engine (RAM, CPU, uptime, provider health, budget). | Read-only | Status card |
| `report` | Kumpulkan Ship Summary Projection (bounded worker pool, timeout per Ship) → sintesis Fleet Report. | Read summary only | Fleet Report / Decision Brief |
| `objective` | `IntakeObjective` → usul Ship + estimasi budget → `FleetOrderProposal`. **Tak eksekusi**. | Propose only | FleetOrderProposal (awaiting PK approval) |

## 5. Pemisahan Router vs Orchestrator (jangan tertukar)

| | Quartermaster (LAPIS 1) | Captain (LAPIS 2) |
|---|---|---|
| Mesin | **Router** (klasifikasi → jalur terdefinisi) | **Orchestrator-worker** (dekomposisi runtime) |
| Keputusan subtask | Tidak — kategori sudah ada | Ya — ditemukan dari objective yang di-approve |
| Biaya | 1 call murah per pesan | Mahal; hanya menyala setelah approval |
| Modul Go | `fleet`/`quartermaster` service | `crew` (`StartTurn`) — sudah ada |

## 6. Risiko & mitigasi wajib (dari riset produksi)

- **Mis-route diam-diam / topic-switch**: user di tengah `objective` menyelipkan `chat`.
  - Confidence rendah → **tanya balik**, jangan tebak.
  - Log tiap keputusan routing: `input → intent → confidence/reason`.
  - Log payload handoff ke Captain (apa yang menyeberang — eksplisit & minimal, bukan transcript penuh).
  - Pantau frekuensi fallback intent; lonjakan = ruang klasifikasi perlu diseimbangkan.
- **Instruction dilution**: jangan satukan 4 cabang jadi satu prompt raksasa; tiap cabang punya prompt/jalur sempit.
- **Governance**: cabang `objective` TIDAK pernah auto-eksekusi; selalu `FleetOrderProposal` → approval PK (selaras `04`/`07`).

## 7. Implementasi (peta ke task)

- **C2** (refactoring-phase2): `fleet.ChatQuartermaster` berhenti string-matching → jalankan Lapis-1 router + cabang `chat` via LLM. *Ini membuat produk hidup DAN memenuhi kebutuhan asisten-sederhana.*
- **C2a**: enum `QuartermasterIntent` + cabang `report`/`engine_room`/`objective`.
- **A2**: paket `orchestrator` di-reframe jadi embrio `QuartermasterService.IntakeObjective` (cabang `objective`), bukan di-wire sebagai crew-orchestrator, bukan dihapus.
- **Captain**: tetap `crew.StartTurn` (sudah di-wire) — orchestrator-worker Lapis 2.

## 8. Acceptance (Definition of Done perilaku)

- [ ] Pesan "halo, apa itu Galleon?" → cabang `chat`, dijawab langsung, TANPA membuat quest/proposal.
- [ ] Pesan "berapa RAM engine sekarang?" → cabang `engine_room`, balas telemetry nyata.
- [ ] Pesan "ringkas status semua ship" → cabang `report`, Fleet Report dari summary.
- [ ] Pesan "siapkan rilis v1.4" → cabang `objective` → FleetOrderProposal awaiting approval, TANPA eksekusi.
- [ ] Tidak ada `strings.Contains` sebagai logika balasan.
- [ ] Setiap keputusan routing ter-log (intent + confidence + reason).

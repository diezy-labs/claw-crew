# 04 — C4 Architecture (Phase 4 Finalize)

> Dasar: `docs/re-branding/08-system-design`, `09-c4-architecture`, `10-technical-specification`. Mengunci penempatan Quartermaster Router (`01`).

## C1 — Context
```
Pirate King (human, otoritas final)
   │  chat / objective / report / engine_room
   ▼
Galleon Fleet (local-first, self-hosted)
   │  BYOK/BYOM/BYOI
   ▼
Provider model user (cloud / Ollama lokal / OpenAI-compatible)  ← user bayar langsung
```

## C2 — Containers (3-tier, STRICT boundary)
| Container | Tier | Tugas |
|---|---|---|
| `apps/tauri-2` (Rust/Tauri) | Desktop Shell | window/tray/dialog; proxy IPC ke engine `:9090` |
| `crates/` (Rust) | System Core | sandbox Landlock, native tools, `SystemGateway` gRPC `:50052`, channels, WASI plugin |
| `engine/` (Go) | AI Orchestrator | brain, fleet governance, policy authority, memory RAG, persistence; gRPC `:50051` + HTTP `:9090` |
| `web-2` (React) | Interface | render + view-model; **zero backend logic**; HTTP proxy ke `:9090` / Tauri IPC |

## C3 — Components (engine Go, per `10`)
```
Fleet Command Domain
  ├── fleet/            (FleetService)
  ├── quartermaster     ← Router dua-lapis (01). IntakeObjective/BuildFleetReport/Escalate. PROPOSE only.
  ├── ships/            (ShipService)
  ├── voyages/          (= run/, rename)
  ├── reporting/ escalation/ handoff/ budget/ skills/   (baru)
Core/Existing Domains
  ├── crew/             ← Captain = ORCHESTRATOR-WORKER (StartTurn). Lapis 2.
  ├── tools/ (=tool)  modelgateway/ (=llm)  memory/  artifacts/ (=artifact)  job-orders/ (=task)  maps/ (=workflow)
  └── persistence/      ← DiskStore (harus di-wire; state persisten)
```

### Penempatan Router (keputusan phase-4)
```
HTTP :9090
  POST /api/v1/fleets/{id}/quartermaster/intake   → quartermaster.IntakeObjective (cabang objective)
  POST /api/chat/quartermaster                     → quartermaster Router Lapis-1 (chat/engine_room/report/objective)
  (objective) → FleetOrderProposal → approval PK → crew.StartTurn (Captain, Lapis-2) via SystemGateway → Rust sandbox
```

## C4 — Code (interface kunci, dari `10`)
```go
type QuartermasterService interface {
    Classify(ctx, msg) (QuartermasterIntent, Confidence, Reason)   // Lapis 1 (phase-4 baru)
    Chat(ctx, msg) (Reply, error)                                   // cabang chat
    IntakeObjective(ctx, FleetObjective) (FleetOrderProposal, error)// cabang objective (propose)
    BuildFleetReport(ctx, fleetID, window) (FleetReport, error)     // cabang report
    Escalate(ctx, EscalationRequest) (Escalation, error)
}
// Captain tetap: crew.Orchestrator.StartTurn(ctx, *TurnRequest, chan<- *TurnEvent)
```

## Aturan boundary (gate review)
- Go = satu-satunya policy authority & state owner; klien = proyeksi.
- Quartermaster baca **Ship Summary Projection** saja (default); tak akses raw cross-Ship.
- Tool execution crew WAJIB lewat `SystemGateway` → Rust sandbox (bukan langsung di Go).
- `correlation_id` di-plumb Quest→Voyage→Artifact→Ship Report.

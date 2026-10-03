# 09 — Route Map (C4 / F-contract)

> SSOT untuk kontrak HTTP `web-2/src/utils/apiClient.ts` ↔ resolver-nya.
> Dibuat dari pembacaan kode nyata 2026-10-01: `apiClient.ts` (24 call), `engine/src/fleet/delivery.go`,
> `web-2/server.ts`. Verifikasi: setiap call apiClient punya resolver (Go engine ATAU gateway lokal server.ts).

## Topologi

```
apiClient.ts  --fetch /api/*-->  web-2/server.ts (:5173/host)
                                   ├─ handle LOKAL (host telemetry)   → tidak ke Go
                                   └─ app.use('/api', proxy)          → Go engine :9090 /api/*
apiClient.ts  --invoke-->        Tauri IPC (desktop) → fallback ke fetch bila gagal
```

Setiap method apiClient punya dua jalur: **Tauri `invoke`** (desktop) dengan **fallback `fetch`** ke jalur HTTP. Baris di bawah mendaftar jalur HTTP (yang menentukan kontrak lintas-tier).

## Peta kontrak

| apiClient method | HTTP | Resolver | Handler |
|---|---|---|---|
| `getCollection(name)` | GET `/api/collections/:name` | Go (proxy) | `fleet.handleCollections` |
| `saveCollection(name)` | POST `/api/collections/:name` | Go (proxy) | `fleet.handleCollections` |
| `getFleetMetrics()` | GET `/api/fleet/metrics` | Go (proxy) | `fleet.handleMetrics` |
| `ringDeckBell()` | POST `/api/fleet/deck-bell` | Go (proxy) | `fleet.handleDeckBell` |
| `getFleetPolicies()` | GET `/api/fleet/policies` | Go (proxy) | `fleet.handlePolicies` |
| `getExecutiveBriefing()` | GET `/api/system/executive-briefing` | Go (proxy) | `fleet.handleBriefing` |
| `getHarborProviders()` | GET `/api/providers/harbor` | Go (proxy) | `fleet.handleHarborProviders` |
| `getDiagnostics()` | GET `/api/diagnostics` | Go (proxy) | `fleet.handleDiagnostics` |
| `applyRemedy()` | POST `/api/diagnostics/remedy` | Go (proxy) | `fleet.handleRemedy` |
| `getSnapshots()` | GET `/api/snapshots` | Go (proxy) | `fleet.handleSnapshots` |
| `createSnapshot(title)` | POST `/api/snapshots` | Go (proxy) | `fleet.handleSnapshots` |
| `chatQuartermaster(message, context)` | POST `/api/chat/quartermaster` | Go (proxy) | `fleet.handleQuartermasterChat` |
| `getSystemMetrics()` | GET `/api/system/metrics` | **server.ts lokal** | host CPU/RAM/uptime (`os.*`) |
| `getNetwork()` | GET `/api/system/network` | **server.ts lokal** | host network info |
| `getOllamaStatus()` | GET `/api/providers/ollama/status` | **server.ts lokal** | probe Ollama daemon `:11434` |
| `getEngineProcesses()` | GET `/api/engine/processes` | **server.ts lokal** | daftar proses host |
| `executeTerminalCommand(cmd)` | POST `/api/engine/execute` | **server.ts lokal** | shell lokal (browser fallback) |
| `getHealth()` | GET `/api/health` | Go (proxy) | *(lihat Gap-3)* |

**Host-telemetry lokal** (`/api/system/*`, `/api/providers/ollama/*`, `/api/engine/*`, `/api/network/*`) sengaja di-handle `server.ts`, bukan Go — ini gateway/telemetri host lokal, bukan domain fleet (sesuai boundary 3-tier di `galleon-architecture.md`). Bukan pelanggaran "zero backend logic in UI": tak ada logika domain fleet di sini.

## Rute Go yang BELUM dipakai frontend (bukan mismatch — surface backend/forward)

Terdaftar di engine, belum ada call `apiClient`: `/api/v1/runs`, `/api/v1/tasks/`, `/api/v1/artifacts/`, `/api/v1/crews`, `/api/v1/workflows`, `/api/v1/tools`, `/api/v1/approvals/`, `/api/v1/tool-executions/`, `/api/v1/tool-requests`, `/api/turn`, `/api/query`. Dipakai lewat jalur lain / belum di-wire ke UI.

## Gap kontrak ditemukan & status

- **Gap-1 (DIPERBAIKI)** — `createSnapshot` kirim `{title}`, Go `handleSnapshots` dulu hanya decode `{label}` → judul user selalu diabaikan, jatuh ke default. Fix: decoder terima `title` (dan `label` untuk back-compat). `engine/src/fleet/delivery.go`.
- **Gap-2 (BENIGN, didokumentasikan)** — `chatQuartermaster` kirim `{message, context}`; Go hanya baca `message`. `context` di-ignore oleh `json.Decode` (tak error). Konsumsi `context` menyusul saat router intent butuh (C2a/F2-2), bukan scope C4.
- **Gap-3 (TERBUKA)** — `getHealth()` fetch `/api/health`; fleet delivery TIDAK register `/api/health` (yang ada `/healthz` di `core/metrics`, path beda). Call ini akan 404/503 lewat proxy. Perlu salah satu: register `/api/health` di Go, atau arahkan `getHealth()` ke `/healthz`. Ditunda — bukan bug blocking (dipakai polling health, gagal-aman).

## Verifikasi
Statis (sesi ini): grep rute Go + baca `apiClient.ts`/`server.ts` penuh; `gofmt -l` bersih atas `delivery.go`.
Build (atas perintah Owner): `cd engine && go vet ./... && go test ./...`; `cd web-2 && tsc --noEmit`.

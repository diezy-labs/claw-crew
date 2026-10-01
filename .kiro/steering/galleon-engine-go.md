---
title: Galleon engine — konvensi Go
inclusion: fileMatch
fileMatchPattern: "engine/**/*.go"
---

# Engine (Go) — konvensi

Loads saat menyentuh `.go` di `engine/`. Go 1.25+, module `github.com/diezy-labs/claw-crew/engine`. DI pakai `google/wire`. Ikuti pola yang SUDAH ada; jangan bikin gaya baru.

## Struktur bounded context (`engine/src/<ctx>/`)
Satu domain = satu package dengan file berperan tetap:
- `interfaces.go` — `Service` interface + DTO/struct domain (satu-satunya tempat kontrak publik package).
- `services.go` — implementasi `Service` (`NewService(...)`), berisi logika domain.
- `delivery.go` — `HTTPHandler` + `RegisterHTTP(server *metrics.Server)` yang memanggil `server.RegisterRouteFunc("/api/...", h.handleX)`.
- `wire.go` — `var Set = wire.NewSet(NewService, NewHTTPHandler)`.
Scaffold baru (orchestrator, fleet, ship, squad, charter, mission, navigator, briefing, policy, approval, treasury, audit, entitlement, schedule, integration) mengikuti pola ini. **Extend package existing (crew/run/workflow/task/tool/llm/memory/artifact/persistence), jangan duplikat.**

## HTTP handler
- Guard method eksplisit di awal: `if r.Method != http.MethodGet { http.Error(w, "method not allowed", http.StatusMethodNotAllowed); return }`.
- Balas sukses lewat helper `writeJSON(w, status, data)` (set `Content-Type: application/json` + encode). Jangan tulis header JSON manual kecuali ada alasan (mis. raw bytes collection).
- Error service → `http.Error(w, err.Error(), http.StatusInternalServerError)`; body invalid → `http.StatusBadRequest`. Jangan bocorkan secret/stack ke response.
- Decode body: `json.NewDecoder(r.Body).Decode(&req)`; tangani error decode untuk input wajib.

## Idiom Go
- `context.Context` sebagai parameter PERTAMA di semua method service yang bisa blocking/IO.
- **Error adalah nilai**: `return nil, fmt.Errorf("...: %w", err)` (wrap dengan `%w` agar `errors.Is/As` jalan). JANGAN `panic` di jalur produksi; panic hanya untuk invariant programmer yang mustahil.
- Accept interface, return struct. Interface didefinisikan di sisi consumer bila memungkinkan; `Service` interface per package sudah jadi kontrak di `interfaces.go`.
- Zero value berguna; hindari konstruktor hanya untuk set default yang bisa jadi zero value.
- `defer` untuk cleanup (Close/Unlock) tepat setelah acquire. Cek error dari `Close()` pada writer.
- Konkurensi: goroutine harus punya exit path jelas (ctx cancel / channel close); jangan bocor. Lindungi shared state dengan mutex atau channel — jangan dua-duanya untuk data yang sama.
- Logging via `logger.Get()` (pola repo); field terstruktur, English, jangan `fmt.Println`.

## Single Source Of Truth (gate review)
- Fakta yang dipakai lintas-tier resolve dari engine, bukan disalin. **Jangan** definisikan ulang risk-tier/policy/quartermaster-reply di Rust (`apps/tauri-2`) dengan nilai berbeda — engine HTTP adalah sumber; Tauri cukup proxy. (Pelanggaran nyata tercatat di `galleon-architecture.md`.)
- Risk tiers kanonikal di package `policy`/`tool`: `read_only → draft → write → sensitive → destructive`. Policy memutuskan izin SEBELUM eksekusi tool.
- Persistensi: jangan klaim DiskStore di doc kalau `wire_gen.go` masih pakai `NewMemory*`. Wire beneran atau perbaiki klaim.
- Plumb `correlation_id` lintas Quest→Voyage→Artifact→Ship Report.

## Verifikasi
- `go build ./...` dan `go vet ./...` dari `engine/` harus hijau sebelum selesai. Test: `go test ./...` (atau targeted `go test ./src/<ctx>/`).
- Jangan tinggalkan dead code / exported symbol tanpa caller — hapus atau wire.

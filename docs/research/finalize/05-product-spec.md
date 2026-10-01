# 05 — Product Spec (Phase 4 Finalize)

> Spesifikasi fungsional yang mengikat implementasi. Dasar: `docs/fundamental`, `docs/re-branding/11-data-model`,`13-api-contracts`.

## 1. Entitas inti (istilah kode netral)
`fleet, ship, squad, crew_member, quartermaster, voyage(run), job_order(task), map(workflow), artifact(treasure), discovery, policy, approval, budget, audit_event, integration, memory_proposal`.

## 2. Quartermaster (spec perilaku — ikat ke `01`)
- Endpoint pintu: `POST /api/chat/quartermaster { message }` → `{ intent, confidence, reason, reply|report|proposal|telemetry }`.
- Lapis-1 Classify → `QuartermasterIntent{chat,engine_room,report,objective}` (enum SSOT Go).
- `objective` → `POST /quartermaster/intake` → `FleetOrderProposal{status:awaiting_pirate_king_approval, proposed_ships[], budget_estimate_usd}`. **Tak eksekusi tanpa approval.**
- Permission (deny by default): credential.read, git.push, deploy, publish, policy.update, budget.update, self-approve.

## 3. Crew Member (spec konfigurasi)
`role + mission + skill_refs + tool_policy_ref + model_route_profile + memory_policy_ref + budget_ceiling + concurrency_ceiling + approval_policy + artifact_contract`. Skills persistent + belajar (via memory_proposal, consent). Model route: Quartermaster rekomendasi, PK override di 4 level (Fleet/Ship/Squad/Member).

## 4. Persistence (wajib, phase-4)
`DiskStore` menggantikan `MemoryStore` untuk run/task/artifact → state (laporan, artifact, treasure, crew, memory) bertahan lintas restart. Tanpa ini janji "persistent" fiktif.

## 5. Learning by consent
Koreksi user → `MemoryProposal{scope(Fleet|Workspace|Project|Ship|Crew|Run), rule, versioned, reversible}` bertipe Artifact → antrian approval. Tak ada self-modify otomatis.

## 6. Autonomy under command (risk tiers — SSOT package policy Go)
`read_only → draft → write → sensitive → destructive`. Satu definisi di Go; TS/Rust konsumsi, tak duplikasi. Approval binding ke action digest (policy version, identity, tool, target, redacted args, credential scope).

## 7. Treasury (BYOK transparan)
Lapor cost provider per Voyage/Quest/Crew/Ship/Fleet (estimate+actual bila tersedia), soft/hard cap, forecast, anomaly alert. **Read-only** terhadap tagihan provider. Timber = kapasitas, bukan kredit inference.

## 8. API surface phase-4 (minimal end-to-end, satu Developer Ship)
`/api/chat/quartermaster` · `/api/v1/fleets/{id}/quartermaster/intake` · `/api/fleet/metrics|policies|diagnostics` · `/api/v1/ships/{id}/crew-members` · `/api/v1/voyages/{id}` · `/api/collections/{name}` (seed dari Go, bukan seedData.ts).

## 9. Acceptance global (DoD phase-4)
- `go vet ./...` + `go test ./...` hijau; `cargo clippy --workspace -- -D warnings`; `tsc --noEmit` hijau.
- Loop end-to-end nyata: objective → proposal → approve → Captain → Crew → Treasure, **persisten** + **tersandbox**.
- Tidak ada fakta domain terdefinisi di dua tier (gate `no_duplicate_state`).

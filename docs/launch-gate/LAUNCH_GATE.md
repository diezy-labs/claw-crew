# Phase 1 Launch Gate Checklist

**Date:** 2026-10-02  
**Phase:** Phase 1 (B2/C2/D1/E2/E3/F1 convergence)  
**Objective:** Verify all Phase 1 components are integration-ready before launch.

---

## Build & Compilation

Prerequisites: All code compiles without errors or warnings.

### Rust Build Verification (Refactoring RF-A: 3 crates)
- [ ] `cargo check -p clawcrew-orchestrator` passes (RF-A1: module extraction)
- [ ] `cargo check -p clawcrew-runtime` passes (RF-A2: core types)
- [ ] `cargo check -p clawcrew-gateway` passes (RF-A3: gRPC gateway)
- [ ] All Rust tests compile with `cargo test --no-run -p <crate>` (no execution yet)
- [ ] No clippy warnings: `cargo clippy --all -- -D warnings`

### Go Build Verification (B2/C2/D1 routes)
- [ ] `go test -v ./... -run=^$ ` compiles all tests (no execution)
- [ ] `go build ./cmd/...` builds main binaries
- [ ] No lint warnings: `golangci-lint run ./...` (if linter available)

### TypeScript/Frontend Build (E2/E3/F1)
- [ ] `npm run build` succeeds in `web-2/`
- [ ] No TypeScript compilation errors
- [ ] No eslint errors in frontend components (ApprovalForm, ApprovalPage, etc.)

---

## Integration Test Suite Readiness

All integration tests exist and are discoverable (not necessarily passing yet).

### B2: Seed Data Endpoint
- [ ] Test file: `engine/tests/integration_test.go` exists
- [ ] Test function: `TestSeedDataEndpoint` defined
- [ ] Test function: `TestSeedDataSchema` defined
- [ ] Mock helper: `mockSeedData()` implemented
- [ ] Helper: `isValidJSON()` implemented for schema validation

### C2: ExecuteTask gRPC
- [ ] Test file: `engine/tests/integration_test.go` exists
- [ ] Test function: `TestExecuteTaskGRPC` defined
- [ ] Test function: `TestExecuteTaskRequestValidation` defined
- [ ] gRPC service definition: `proto/agent_service.proto` defines ExecuteTask

### D1: Go↔Rust Integration
- [ ] Test file: `engine/tests/integration_test.go` exists
- [ ] Test function: `TestRustExecuteToolIntegration` defined
- [ ] Test function: `TestRustExecuteToolErrorHandling` defined
- [ ] Rust gateway listening port (50052) documented in config

### E2: Fleet Store Hydration
- [ ] Test function: `TestFleetStoreHydration` defined
- [ ] `fleetStore.HydrateSeedData()` method signature verified

### E2E: End-to-End Flow
- [ ] Test function: `TestIntegrationEndToEnd` defined

---

## Code Structure Verification

Manual verification of key files (no build execution):

### B2: Seed Data Handler
- [ ] File `engine/src/fleet/delivery.go` exists
- [ ] Function `HandleSeedData(w http.ResponseWriter, r *http.Request)` exists (or similar)
- [ ] Route registered in main handler (e.g., `GET /api/fleet/seed`)
- [ ] Response marshals to JSON

### C2: ExecuteTask Service
- [ ] File `engine/pkg/pb/agent_service.pb.go` exists (auto-generated from proto)
- [ ] File `engine/pkg/pb/agent_service_grpc.pb.go` exists
- [ ] gRPC service stub includes `ExecuteTask` method
- [ ] Request/response types marshaled in proto

### D1: Rust Gateway Integration
- [ ] Rust service: `clawcrew-gateway` crate exists
- [ ] Main entry point (`main.rs` or `lib.rs`) starts gRPC listener
- [ ] Port configuration: 50052 for Rust gateway

### E2/E3: Frontend Hydration & Approval Form
- [ ] File `web-2/src/pages/ApprovalPage.tsx` exists
- [ ] File `web-2/src/components/ApprovalForm.tsx` exists
- [ ] Function `hydrateSeedData()` called in useEffect or similar hook
- [ ] Form submission triggers ExecuteTask gRPC call

---

## Pre-Launch Smoke Tests

Quick sanity checks (manual, before live traffic):

### Service Startup
- [ ] Start Go engine server: `go run ./cmd/engine`
  - [ ] Server listens on port 9090
  - [ ] Server logs startup message
- [ ] Start Rust gateway: `cargo run -p clawcrew-gateway --release`
  - [ ] Rust server listens on port 50052
  - [ ] Rust server logs startup message

### HTTP Endpoint Check
- [ ] `curl -X GET http://localhost:9090/api/fleet/seed`
  - [ ] Response code: 200 OK
  - [ ] Response body: valid JSON
  - [ ] Response contains `seedID` field
  - [ ] Response contains `taskList` array

### gRPC Service Check (manual or grpcurl)
- [ ] gRPC service reachable on `localhost:50052`
- [ ] ExecuteTask method responds (even if mock)

### Frontend Load
- [ ] Frontend builds and loads at `http://localhost:3000` (or configured dev port)
- [ ] ApprovalPage renders without errors
- [ ] ApprovalForm component loads
- [ ] No browser console errors (TypeScript types valid)

---

## Configuration & Documentation

- [ ] Environment file `.env` or config documented (API endpoints, ports, credentials if any)
- [ ] README.md or docs explain how to start each service
- [ ] Seed data location documented (file path or API endpoint)
- [ ] Rust gateway address hardcoded or configurable in Go engine

---

## Rollback Plan

Procedure to revert if critical issue discovered after launch:

- [ ] Last stable commit on `main` branch is identified: `git log --oneline main | head -1`
- [ ] Rollback procedure documented:
  1. Stop all running services
  2. `git checkout <last-stable-commit>`
  3. Rebuild binaries (`go build`, `cargo build`)
  4. Restart services
  5. Verify endpoints respond

---

## Sign-Off

Gate must be approved by all three roles before launch:

### QA Verification
- [ ] QA completed integration test skeleton review
- [ ] QA verified build compliance (no compile errors)
- [ ] QA verified code structure (files exist, functions defined)
- **QA Approval:** __________________ **Date:** __________

### Product Owner (PO) Approval
- [ ] PO reviewed Phase 1 feature scope (B2/C2/D1/E2/E3/F1 complete)
- [ ] PO approved go-live risk assessment
- [ ] PO confirmed rollback plan acceptable
- **PO Approval:** __________________ **Date:** __________

### Lead Engineer Approval
- [ ] Lead verified all compilation checks pass
- [ ] Lead confirmed integration points (Go↔Rust) are wired correctly
- [ ] Lead approved deployment procedure
- **Lead Approval:** __________________ **Date:** __________

---

## Launch Execution

**Scheduled Launch Time:** __________________ (UTC+7)  
**Launched By:** __________________  
**Status:** ☐ Pending | ☐ Live | ☐ Rolled Back

**Notes:**
```
[Post-launch notes, incidents, or rollback reason if applicable]
```

---

## Post-Launch Monitoring (First 24h)

- [ ] Engine service stable (0 restarts)
- [ ] Rust gateway service stable (0 restarts)
- [ ] Seed data endpoint latency <100ms (p99)
- [ ] ExecuteTask latency <500ms (p99)
- [ ] No error logs in engine or gateway
- [ ] Frontend loads without errors

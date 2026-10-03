# Go Conventions

**Status:** Normative (code MUST follow this)  
**Last updated:** 2026-10-03  
**Go version:** 1.25+

---

## Module Structure

```
engine/
├── cmd/agent-engine/
│   └── main.go              # Entrypoint, calls app.InitializeApp()
├── app/
│   ├── app.go               # gRPC + HTTP server lifecycle
│   ├── wire.go              # Wire DI providers (hand-written)
│   └── wire_gen.go          # Wire DI injector (generated, DO NOT EDIT)
├── proto/
│   ├── system_gateway.proto # Rust gRPC service definitions
│   └── agent_engine.proto   # Go gRPC service definitions (placeholder)
├── src/
│   ├── fleet/               # Fleet governance (crews, policies, roster)
│   ├── crew/                # Agent orchestration (turn, task, memory)
│   ├── memory/              # RAG, vector store, embeddings
│   ├── workflow/            # Workflow engine
│   ├── llm/                 # [DEPRECATED ADR 0001] Remove after Rust delegation
│   └── tools/               # [DEPRECATED ADR 0001] Remove after Rust delegation
└── data/                    # Runtime data (SQLite, config, logs)
```

**Convention:** Each `src/*` package has `services.go` (domain logic), `store.go` (persistence), `handlers.go` (HTTP routes).

---

## Dependency Injection (Wire)

### Why Wire?

- **Compile-time DI** (not runtime reflection like dig/fx)
- **Type-safe** (missing dependency = compile error, not runtime panic)
- **Explicit** (dependency graph in `wire.go`, not hidden in `init()`)

### Setup

1. Install Wire: `go install github.com/google/wire/cmd/wire@latest`
2. Write providers in `app/wire.go`
3. Generate injector: `go generate ./app`
4. Use injector in `main.go`

### Example: `app/wire.go`

```go
//go:build wireinject
// +build wireinject

package app

import (
    "github.com/google/wire"
    "galleon-fleet/engine/src/fleet"
    "galleon-fleet/engine/src/crew"
)

// ProvideFleetStore creates FleetStore
func ProvideFleetStore(cfg *Config) (*fleet.Store, error) {
    return fleet.NewStore(cfg.DatabasePath)
}

// ProvideFleetService creates FleetService
func ProvideFleetService(store *fleet.Store) *fleet.Service {
    return fleet.NewService(store)
}

// ProvideCrewService creates CrewService
func ProvideCrewService(
    fleetSvc *fleet.Service,
    gatewayCli pb.SystemGatewayClient, // Rust gRPC client
) *crew.Service {
    return crew.NewService(fleetSvc, gatewayCli)
}

// InitializeApp wires all dependencies
func InitializeApp(cfg *Config) (*App, error) {
    wire.Build(
        ProvideFleetStore,
        ProvideFleetService,
        ProvideCrewService,
        wire.Struct(new(App), "*"), // Inject into App struct
    )
    return &App{}, nil
}
```

**Convention:** Provider functions named `Provide<Type>`, return `(*Type, error)`.

### Example: `cmd/agent-engine/main.go`

```go
package main

import (
    "log"
    "galleon-fleet/engine/app"
)

func main() {
    cfg := app.LoadConfig()
    
    // Wire-generated injector
    application, err := app.InitializeApp(cfg)
    if err != nil {
        log.Fatalf("Failed to initialize app: %v", err)
    }
    
    if err := application.Run(); err != nil {
        log.Fatalf("App failed: %v", err)
    }
}
```

**Convention:** `main.go` is thin (load config → inject → run), no business logic.

---

## gRPC Patterns

### Client (Go → Rust)

Go engine calls Rust `SystemGatewayService` for tool + LLM execution (ADR 0001).

```go
// src/crew/services.go
type Service struct {
    gateway pb.SystemGatewayClient // Injected by Wire
}

func (s *Service) StartTurn(ctx context.Context, req *StartTurnRequest) (*TurnResponse, error) {
    // 1. Build turn context (Go orchestration)
    messages := s.buildMessages(req)
    tools := s.buildTools(req)
    
    // 2. Delegate LLM execution to Rust
    stream, err := s.gateway.ExecuteTurn(ctx, &pb.ExecuteTurnRequest{
        Model:    req.Model,
        Messages: messages,
        Tools:    tools,
    })
    if err != nil {
        return nil, fmt.Errorf("gateway.ExecuteTurn: %w", err)
    }
    
    // 3. Consume stream, save result
    result := s.consumeStream(stream)
    s.memory.Save(req.CrewID, result)
    
    return &TurnResponse{Result: result}, nil
}
```

**Convention:** Wrap gRPC errors with `fmt.Errorf(..., %w, err)` for error chain.

### Server (Rust → Go) — Placeholder

Go `AgentEngineService` gRPC server is NOT YET IMPLEMENTED (internal control plane only, no frontend calls).

**Future design:**
```go
// src/fleet/grpc_server.go
type AgentEngineServer struct {
    pb.UnimplementedAgentEngineServiceServer
    fleet *fleet.Service
}

func (s *AgentEngineServer) GetFleetPolicies(ctx context.Context, req *pb.GetFleetPoliciesRequest) (*pb.FleetPoliciesResponse, error) {
    policies := s.fleet.GetPolicies(req.FleetID)
    return &pb.FleetPoliciesResponse{Policies: policies}, nil
}
```

**Convention:** gRPC server methods return `(*Response, error)`, never panic.

---

## HTTP Handlers

### REST API Structure

```
/api/fleet/*        # Fleet management (crews, policies, roster)
/api/crews/*        # Crew operations (turn, history, artifacts)
/api/system/*       # System metrics, health, telemetry
/api/network/*      # [TEMP ADR 0002] QR code, network utils
```

### Example: `src/fleet/handlers.go`

```go
package fleet

import (
    "encoding/json"
    "net/http"
    "github.com/gorilla/mux"
)

type Handlers struct {
    service *Service // Injected by Wire
}

func NewHandlers(svc *Service) *Handlers {
    return &Handlers{service: svc}
}

// RegisterRoutes registers HTTP routes
func (h *Handlers) RegisterRoutes(r *mux.Router) {
    r.HandleFunc("/api/fleet/crews", h.ListCrews).Methods("GET")
    r.HandleFunc("/api/fleet/crews/{id}", h.GetCrew).Methods("GET")
    r.HandleFunc("/api/fleet/crews/{id}/run", h.RunCrew).Methods("POST")
}

// ListCrews returns all crews
func (h *Handlers) ListCrews(w http.ResponseWriter, r *http.Request) {
    crews, err := h.service.ListCrews(r.Context())
    if err != nil {
        http.Error(w, err.Error(), http.StatusInternalServerError)
        return
    }
    
    w.Header().Set("Content-Type", "application/json")
    json.NewEncoder(w).Encode(crews)
}

// RunCrew starts a crew turn
func (h *Handlers) RunCrew(w http.ResponseWriter, r *http.Request) {
    vars := mux.Vars(r)
    crewID := vars["id"]
    
    var req RunCrewRequest
    if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
        http.Error(w, "Invalid request", http.StatusBadRequest)
        return
    }
    
    resp, err := h.service.RunCrew(r.Context(), crewID, &req)
    if err != nil {
        http.Error(w, err.Error(), http.StatusInternalServerError)
        return
    }
    
    w.Header().Set("Content-Type", "application/json")
    json.NewEncoder(w).Encode(resp)
}
```

**Conventions:**
- Handler methods: `func (h *Handlers) MethodName(w http.ResponseWriter, r *http.Request)`
- Return JSON with `json.NewEncoder(w).Encode(data)`
- Error responses: `http.Error(w, msg, statusCode)`
- Route params: `mux.Vars(r)["param"]`

### Example: `app/app.go` (HTTP server lifecycle)

```go
package app

import (
    "context"
    "net/http"
    "github.com/gorilla/mux"
    "galleon-fleet/engine/src/fleet"
    "galleon-fleet/engine/src/crew"
)

type App struct {
    FleetHandlers *fleet.Handlers
    CrewHandlers  *crew.Handlers
}

func (a *App) Run() error {
    r := mux.NewRouter()
    
    // Register routes
    a.FleetHandlers.RegisterRoutes(r)
    a.CrewHandlers.RegisterRoutes(r)
    
    // CORS middleware (localhost only)
    r.Use(corsMiddleware)
    
    // Start HTTP server
    return http.ListenAndServe(":9090", r)
}

func corsMiddleware(next http.Handler) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        w.Header().Set("Access-Control-Allow-Origin", "http://localhost:5173")
        w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE")
        w.Header().Set("Access-Control-Allow-Headers", "Content-Type")
        
        if r.Method == "OPTIONS" {
            w.WriteHeader(http.StatusOK)
            return
        }
        
        next.ServeHTTP(w, r)
    })
}
```

**Convention:** Middleware wraps `http.Handler`, returns new handler.

---

## Error Handling

### Propagate Errors (Don't Swallow)

```go
// ✅ Correct: wrap error with context
func (s *Service) LoadCrew(id string) (*Crew, error) {
    crew, err := s.store.Get(id)
    if err != nil {
        return nil, fmt.Errorf("store.Get(%s): %w", id, err)
    }
    return crew, nil
}

// ❌ Wrong: swallow error
func (s *Service) LoadCrew(id string) *Crew {
    crew, err := s.store.Get(id)
    if err != nil {
        log.Println(err) // Logged but not returned
        return nil       // Caller can't distinguish "not found" vs "DB error"
    }
    return crew
}
```

### Error Types

```go
// Domain-specific errors
var (
    ErrCrewNotFound   = errors.New("crew not found")
    ErrInvalidPolicy  = errors.New("invalid policy")
    ErrUnauthorized   = errors.New("unauthorized")
)

// Check error type
if errors.Is(err, ErrCrewNotFound) {
    http.Error(w, "Crew not found", http.StatusNotFound)
    return
}
```

**Convention:** Exported errors start with `Err`, use `errors.New()` for sentinel values.

---

## Logging

### Use Structured Logging (zap)

```go
import "go.uber.org/zap"

func (s *Service) StartTurn(ctx context.Context, req *StartTurnRequest) (*TurnResponse, error) {
    s.logger.Info("Starting crew turn",
        zap.String("crew_id", req.CrewID),
        zap.String("task", req.Task),
    )
    
    resp, err := s.executeInternal(ctx, req)
    if err != nil {
        s.logger.Error("Turn failed",
            zap.String("crew_id", req.CrewID),
            zap.Error(err),
        )
        return nil, err
    }
    
    s.logger.Info("Turn completed",
        zap.String("crew_id", req.CrewID),
        zap.Int("tokens", resp.TokensUsed),
    )
    return resp, nil
}
```

**Conventions:**
- `logger.Info` for normal flow (start/end)
- `logger.Error` for errors (with `zap.Error(err)`)
- `logger.Debug` for verbose detail (disabled in prod)
- Log structured fields (not `fmt.Sprintf` strings)

---

## Testing

### Unit Tests

```go
// src/fleet/services_test.go
package fleet_test

import (
    "testing"
    "github.com/stretchr/testify/assert"
    "galleon-fleet/engine/src/fleet"
)

func TestService_GetCrew(t *testing.T) {
    store := fleet.NewInMemoryStore() // Mock store
    svc := fleet.NewService(store)
    
    // Setup
    crew := &fleet.Crew{ID: "test-crew", Name: "Test Crew"}
    store.Save(crew)
    
    // Execute
    result, err := svc.GetCrew("test-crew")
    
    // Assert
    assert.NoError(t, err)
    assert.Equal(t, "Test Crew", result.Name)
}
```

**Conventions:**
- Test file: `*_test.go` in same package or `*_test` package (for integration)
- Test function: `func TestService_MethodName(t *testing.T)`
- Use `testify/assert` for assertions
- Mock external dependencies (store, gRPC client)

### Integration Tests

```go
// src/crew/integration_test.go
// +build integration

package crew_test

import (
    "context"
    "testing"
    "github.com/stretchr/testify/require"
    "galleon-fleet/engine/src/crew"
)

func TestCrewService_StartTurn_Integration(t *testing.T) {
    // Setup real DB + gRPC client
    cfg := loadTestConfig()
    app, err := app.InitializeApp(cfg)
    require.NoError(t, err)
    
    // Execute real turn
    resp, err := app.CrewService.StartTurn(context.Background(), &crew.StartTurnRequest{
        CrewID: "test-crew",
        Task:   "Hello world",
    })
    
    require.NoError(t, err)
    require.NotEmpty(t, resp.Result)
}
```

**Conventions:**
- Build tag: `// +build integration` (run with `go test -tags=integration`)
- Use real dependencies (DB, gRPC), not mocks
- `require` for assertions that must pass (else abort test)

---

## Naming Conventions

| Type | Pattern | Example |
|------|---------|---------|
| Package | lowercase, short | `fleet`, `crew`, `memory` |
| File | lowercase, underscore | `services.go`, `fleet_store.go` |
| Struct | PascalCase | `CrewService`, `FleetStore` |
| Interface | PascalCase + `-er` suffix | `Storer`, `Executor` |
| Method | PascalCase (exported) | `GetCrew`, `StartTurn` |
| Variable | camelCase | `crewID`, `turnRequest` |
| Constant | PascalCase or UPPER_SNAKE | `DefaultTimeout`, `MAX_RETRIES` |

---

## Code Organization

### Package Layout (Clean Architecture)

```
src/fleet/
├── models.go         # Domain structs (Crew, Policy, RiskTier)
├── services.go       # Business logic (orchestration, validation)
├── store.go          # Persistence interface
├── sqlite_store.go   # SQLite implementation of Storer
├── handlers.go       # HTTP routes
└── services_test.go  # Unit tests
```

**Dependency rule:** `handlers` → `services` → `store` (never reverse).

### Avoid Circular Dependencies

```
// ❌ Wrong: crew imports fleet, fleet imports crew
package crew
import "galleon-fleet/engine/src/fleet"

package fleet
import "galleon-fleet/engine/src/crew" // Circular!
```

**Solution:** Extract shared types to `common/` or `models/` package.

---

## Performance

### Connection Pooling

```go
// Store keeps persistent connections
type Store struct {
    db *sql.DB // Connection pool (DO NOT close per query)
}

func NewStore(path string) (*Store, error) {
    db, err := sql.Open("sqlite3", path)
    if err != nil {
        return nil, err
    }
    
    // Configure pool
    db.SetMaxOpenConns(10)
    db.SetMaxIdleConns(5)
    db.SetConnMaxLifetime(time.Hour)
    
    return &Store{db: db}, nil
}
```

**Convention:** Open connections in `New*()`, close in `Close()` (NOT per query).

### Context Propagation

```go
func (s *Service) StartTurn(ctx context.Context, req *StartTurnRequest) (*TurnResponse, error) {
    // Pass context to all downstream calls
    crew, err := s.store.Get(ctx, req.CrewID)
    if err != nil {
        return nil, err
    }
    
    resp, err := s.gateway.ExecuteTurn(ctx, &pb.ExecuteTurnRequest{...})
    if err != nil {
        return nil, err
    }
    
    return resp, nil
}
```

**Convention:** Always accept `context.Context` as first arg, pass to all I/O calls.

---

## Related Documents

- [Architecture Overview](../architecture/00-overview.md) — 4-tier system design
- [Boundary Policy](../architecture/01-boundary-policy.md) — What Go owns vs delegates
- [ADR 0001: LLM SSOT Rust](../decisions/0001-llm-ssot-rust.md) — Go delegates LLM to Rust
- [Rust Conventions](./rust-conventions.md) — Rust crate patterns

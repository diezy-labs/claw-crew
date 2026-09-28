# Claw-Crew Phase 3 — Provider Routing: API & Tauri Specification

> **Status:** Proposed API & IPC Specification  
> **Target Endpoints:** `/api/v1/provider-accounts`, `/api/v1/models`, `/api/v1/route-policies`  
> **Parent Directory:** [`docs/refactoring/phase3/provider-routing/`](./)  

---

## 1. REST API Contracts

### 1.1 Provider Accounts API
Manages configured upstream provider credentials, endpoints, and health status.

#### `GET /api/v1/provider-accounts`
List all configured provider accounts for the active workspace. **Raw API keys are never returned.**

```json
{
  "accounts": [
    {
      "id": "pa_ollama_local",
      "provider_type": "ollama",
      "display_name": "Local Ollama Endpoint",
      "endpoint_url": "http://localhost:11434",
      "credential_ref": "secret://workspace/default/ollama",
      "enabled": true,
      "health_status": "healthy",
      "allowed_workspaces": ["workspace_default"],
      "labels": ["local", "offline", "zero-cost"]
    },
    {
      "id": "pa_openrouter_primary",
      "provider_type": "openrouter",
      "display_name": "OpenRouter Cloud",
      "endpoint_url": "https://openrouter.ai/api/v1",
      "credential_ref": "secret://workspace/default/openrouter-key",
      "enabled": true,
      "health_status": "healthy",
      "allowed_workspaces": ["workspace_default"],
      "labels": ["cloud", "frontier"]
    }
  ]
}
```

#### `POST /api/v1/provider-accounts/{id}/health-check`
Triggers an immediate round-trip latency and connectivity check.

```json
{
  "account_id": "pa_openrouter_primary",
  "health_status": "healthy",
  "latency_ms": 142,
  "checked_at": "2026-09-28T10:14:00Z"
}
```

---

### 1.2 Model Catalog API

#### `GET /api/v1/models`
Returns models available to the active workspace with verified capability flags.

```json
{
  "models": [
    {
      "id": "openrouter/qwen/qwen-2.5-coder-32b",
      "provider_account_id": "pa_openrouter_primary",
      "display_name": "Qwen 2.5 Coder 32B Instruct",
      "capabilities": {
        "text_generation": true,
        "streaming": true,
        "tool_calling": true,
        "structured_output": true,
        "json_schema": true,
        "vision": false,
        "embeddings": false,
        "prompt_caching": true
      },
      "limits": {
        "context_window_tokens": 131072,
        "max_output_tokens": 8192
      },
      "pricing": {
        "input_per_million_usd": 0.07,
        "output_per_million_usd": 0.16,
        "confidence": "provider_reported"
      },
      "enabled": true
    }
  ]
}
```

---

### 1.3 Route Policies & Simulation

#### `POST /api/v1/route-policies/simulate`
Simulates candidate ranking without executing an upstream model call.

##### Request Payload:
```json
{
  "workspace_id": "workspace_default",
  "purpose": "code_refactor",
  "route_profile": "balanced",
  "data_classification": "internal",
  "required_capabilities": {
    "tool_calling": true,
    "streaming": true
  },
  "min_context_tokens": 32000,
  "max_estimated_cost_usd": 0.05
}
```

##### Response `200 OK`:
```json
{
  "decision": {
    "selected": {
      "provider_account_id": "pa_openrouter_primary",
      "model_id": "openrouter/qwen/qwen-2.5-coder-32b"
    },
    "fallbacks": [
      {
        "provider_account_id": "pa_ollama_local",
        "model_id": "local/qwen-2.5-coder-14b"
      }
    ],
    "reason_codes": [
      "capability_match:tool_calling",
      "within_budget:0.012<0.05",
      "high_health_score"
    ],
    "rejected_candidates": [
      {
        "model_id": "openrouter/deepseek/deepseek-r1-distill",
        "reason_code": "missing_capability:tool_calling"
      },
      {
        "model_id": "openai/gpt-4o",
        "reason_code": "exceeds_budget:0.180>0.05"
      }
    ],
    "estimated_cost_usd": 0.012,
    "pricing_confidence": "provider_reported"
  }
}
```

---

### 1.4 Run Route Explainability & Overrides

#### `GET /api/v1/runs/{run_id}/route-decisions`
Returns historical route decisions, attempted fallbacks, and cost ledger records for an active or completed run.

#### `POST /api/v1/runs/{run_id}/model-override`
Enables user pinning for a specific run.

```json
{
  "pinned_model_id": "local/qwen-2.5-coder-14b",
  "allow_fallback": false
}
```

---

## 2. Tauri Desktop IPC Commands

Desktop clients (`apps/tauri`) communicate with the Go engine via type-safe Tauri IPC commands:

| Command Name | Input Arguments | Output Return | Description |
|---|---|---|---|
| `list_provider_accounts` | `{ workspace_id: string }` | `Vec<ProviderAccount>` | Loads configured provider accounts. |
| `check_provider_health` | `{ account_id: string }` | `HealthReport` | Runs live ping on selected endpoint. |
| `list_models` | `{ workspace_id: string }` | `Vec<ModelDescriptor>` | Retrieves model catalog. |
| `simulate_route` | `ModelRequestPayload` | `RouteDecision` | Runs deterministic routing simulation. |
| `get_run_route_decisions`| `{ run_id: string }` | `Vec<RouteDecision>` | Displays routing rationale in Run Inspector. |
| `get_run_usage` | `{ run_id: string }` | `TokenUsageReport` | Fetches aggregated token and cost metrics. |
| `set_run_model_override` | `{ run_id: string, model_id: string }` | `StatusResult` | Pins a model for subsequent turns. |

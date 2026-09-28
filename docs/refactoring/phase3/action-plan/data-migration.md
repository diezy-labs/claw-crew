# Claw-Crew Phase 3 — Action Plan: Database Entities & Storage Strategy

> **Status:** Proposed Data Architecture & Persistence Specification  
> **Storage Layer:** `engine/src/persistence/` (Embedded SQLite / JSON Disk Store)  
> **Parent Directory:** [`docs/refactoring/phase3/action-plan/`](./)  

---

## 1. Storage Architecture

In Phase 3, Claw-Crew persists model catalog, provider account metadata, routing policies, and usage ledgers using the existing embedded storage abstraction in `engine/src/persistence/`:
- **Development & Single-Node Desktop:** Local embedded SQLite database or atomic append-only JSON disk store (`disk_store.go`).
- **Production & Multi-Client Server:** PostgreSQL or shared SQLite with write-ahead logging (WAL).

---

## 2. Relational Schema & Entities

```sql
-- 1. Provider Accounts Table
CREATE TABLE IF NOT EXISTS provider_accounts (
    id VARCHAR(64) PRIMARY KEY,
    provider_type VARCHAR(32) NOT NULL, -- 'ollama', 'openai_compatible', 'openrouter', 'gemini_native'
    display_name VARCHAR(128) NOT NULL,
    endpoint_url TEXT NOT NULL,
    credential_ref TEXT NOT NULL,       -- 'secret://workspace/default/openrouter-key'
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    health_status VARCHAR(32) NOT NULL DEFAULT 'healthy',
    allowed_workspaces TEXT NOT NULL,   -- JSON array of workspace IDs
    labels TEXT,                        -- JSON array of tags
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 2. Models Catalog Table
CREATE TABLE IF NOT EXISTS models (
    id VARCHAR(128) PRIMARY KEY,        -- 'openrouter/qwen/qwen-2.5-coder-32b'
    provider_account_id VARCHAR(64) NOT NULL REFERENCES provider_accounts(id) ON DELETE CASCADE,
    provider_model_id VARCHAR(128) NOT NULL,
    display_name VARCHAR(128) NOT NULL,
    context_window_tokens INTEGER NOT NULL,
    max_output_tokens INTEGER NOT NULL,
    input_price_per_million NUMERIC(10, 4),
    output_price_per_million NUMERIC(10, 4),
    pricing_confidence VARCHAR(32) NOT NULL, -- 'provider_reported', 'catalog_estimate', 'unknown'
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    lifecycle_state VARCHAR(32) NOT NULL DEFAULT 'unverified', -- 'unverified', 'canary', 'enabled', 'quarantined'
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    verified_at TIMESTAMP WITH TIME ZONE
);

-- 3. Model Capabilities Table
CREATE TABLE IF NOT EXISTS model_capabilities (
    model_id VARCHAR(128) PRIMARY KEY REFERENCES models(id) ON DELETE CASCADE,
    text_generation BOOLEAN NOT NULL DEFAULT TRUE,
    streaming BOOLEAN NOT NULL DEFAULT TRUE,
    tool_calling BOOLEAN NOT NULL DEFAULT FALSE,
    structured_output BOOLEAN NOT NULL DEFAULT FALSE,
    json_schema BOOLEAN NOT NULL DEFAULT FALSE,
    vision BOOLEAN NOT NULL DEFAULT FALSE,
    embeddings BOOLEAN NOT NULL DEFAULT FALSE,
    prompt_caching BOOLEAN NOT NULL DEFAULT FALSE
);

-- 4. Route Policies Table
CREATE TABLE IF NOT EXISTS route_policies (
    id VARCHAR(64) PRIMARY KEY,         -- 'balanced-v1', 'local-only'
    workspace_id VARCHAR(64) NOT NULL,
    weight_quality NUMERIC(4, 2) NOT NULL,
    weight_reliability NUMERIC(4, 2) NOT NULL,
    weight_latency NUMERIC(4, 2) NOT NULL,
    weight_cost NUMERIC(4, 2) NOT NULL,
    weight_locality NUMERIC(4, 2) NOT NULL,
    weight_cache NUMERIC(4, 2) NOT NULL,
    max_fallback_attempts INTEGER NOT NULL DEFAULT 2,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 5. Route Decisions Table (Auditability)
CREATE TABLE IF NOT EXISTS route_decisions (
    id VARCHAR(64) PRIMARY KEY,         -- 'route_01JM5A...'
    request_id VARCHAR(64) NOT NULL,
    run_id VARCHAR(64) NOT NULL,
    task_id VARCHAR(64) NOT NULL,
    selected_model_id VARCHAR(128) NOT NULL REFERENCES models(id),
    selected_account_id VARCHAR(64) NOT NULL REFERENCES provider_accounts(id),
    fallback_chain TEXT,                -- JSON array of fallback candidate targets
    reason_codes TEXT NOT NULL,         -- JSON array of strings
    rejected_candidates TEXT,           -- JSON array of rejected candidates and reasons
    estimated_cost_usd NUMERIC(10, 6) NOT NULL,
    pricing_confidence VARCHAR(32) NOT NULL,
    selected_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 6. Model Execution Attempts Table
CREATE TABLE IF NOT EXISTS model_attempts (
    id VARCHAR(64) PRIMARY KEY,         -- 'attempt_01JM5A...'
    route_decision_id VARCHAR(64) NOT NULL REFERENCES route_decisions(id) ON DELETE CASCADE,
    run_id VARCHAR(64) NOT NULL,
    task_id VARCHAR(64) NOT NULL,
    model_id VARCHAR(128) NOT NULL,
    attempt_number INTEGER NOT NULL DEFAULT 1,
    status VARCHAR(32) NOT NULL,        -- 'success', 'rate_limited', 'timed_out', 'failed'
    latency_ms INTEGER NOT NULL,
    error_code VARCHAR(64),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 7. Token Usage Ledger Table
CREATE TABLE IF NOT EXISTS usage_records (
    id VARCHAR(64) PRIMARY KEY,
    workspace_id VARCHAR(64) NOT NULL,
    run_id VARCHAR(64) NOT NULL,
    task_id VARCHAR(64) NOT NULL,
    model_id VARCHAR(128) NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    cached_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd NUMERIC(10, 6) NOT NULL,
    pricing_confidence VARCHAR(32) NOT NULL,
    recorded_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);
```

---

## 3. Required Indexes & Performance Optimization

To ensure real-time query performance during high-throughput agent runs:

```sql
CREATE INDEX idx_models_account_enabled ON models(provider_account_id, enabled);
CREATE INDEX idx_models_lifecycle ON models(lifecycle_state, enabled);
CREATE INDEX idx_route_decisions_run ON route_decisions(run_id, selected_at);
CREATE INDEX idx_model_attempts_run_task ON model_attempts(run_id, task_id, created_at);
CREATE INDEX idx_usage_records_workspace_date ON usage_records(workspace_id, recorded_at);
CREATE INDEX idx_usage_records_run ON usage_records(run_id);
```

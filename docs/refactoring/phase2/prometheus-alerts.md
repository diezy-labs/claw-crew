# Prometheus Metrics & Alerting Specification (Phase 7)

## 1. Metrics Catalogue

The Go Agent Engine exposes Prometheus metrics on `:9090/metrics` (or configured port).

| Metric Name | Type | Labels | Description |
|---|---|---|---|
| `clawcrew_active_agents` | Gauge | `crew_id`, `role` | Current active running agents in engine |
| `clawcrew_agent_turn_duration_seconds` | Histogram | `crew_id`, `status` | Latency distribution of agent turns |
| `clawcrew_llm_token_usage_total` | Counter | `provider`, `model`, `type` | Token consumption counters (prompt/completion) |
| `clawcrew_tool_executions_total` | Counter | `tool_name`, `tier`, `status` | Cumulative tool invocations by status |
| `clawcrew_tool_execution_duration_seconds` | Histogram | `tool_name` | Latency distribution of tool runs |
| `clawcrew_task_executions_total` | Counter | `status` | Cumulative task completions and failures |

## 2. Alert Rules (`alerts.yml`)

```yaml
groups:
  - name: clawcrew_engine_alerts
    rules:
      - alert: HighToolFailureRate
        expr: rate(clawcrew_tool_executions_total{status="failed"}[5m]) / rate(clawcrew_tool_executions_total[5m]) > 0.15
        for: 2m
        labels:
          severity: warning
        annotations:
          summary: "Tool failure rate exceeded 15% over 5m window"
          description: "Tool {{ $labels.tool_name }} is failing frequently."

      - alert: AgentTurnLatencySpike
        expr: histogram_quantile(0.95, sum(rate(clawcrew_agent_turn_duration_seconds_bucket[5m])) by (le)) > 30
        for: 3m
        labels:
          severity: critical
        annotations:
          summary: "p95 Agent turn duration > 30s"
          description: "Agent execution is experiencing severe latency."

      - alert: LLMTokenExhaustion
        expr: rate(clawcrew_llm_token_usage_total[1h]) > 1000000
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Abnormal token consumption rate (> 1M tokens/hour)"
          description: "Inspect active runs for possible runaway execution loops."
```

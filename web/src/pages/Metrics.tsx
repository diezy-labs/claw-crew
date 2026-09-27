import { useCallback, useState } from 'react';
import { Link } from 'react-router-dom';
import { Activity, CheckCircle2, Loader2, RefreshCw, XCircle } from 'lucide-react';
import {
  getAgentMetrics,
  getTaskStats,
  type AgentMetrics,
  type TaskAgentStat,
} from '@/lib/api';
import { usePolling } from '@/hooks/usePolling';
import { formatTokens, formatLatency, formatCostUsd } from './metrics.logic';
import { Badge, Button, Card, PageHeader, StatCard } from '@/components/ui';
import { getEngineMetrics, getEngineHealth } from '@/lib/tauri';

type SortKey = 'agent' | 'total_tasks' | 'total_tokens' | 'total_cost_usd' | 'avg_latency_ms' | 'fallback_count';

export default function Metrics() {
  const [metrics, setMetrics] = useState<AgentMetrics[]>([]);
  const [stats, setStats] = useState<TaskAgentStat[]>([]);
  const [engineHealthy, setEngineHealthy] = useState<boolean | null>(null);
  const [enginePrometheus, setEnginePrometheus] = useState<string>('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [sortKey, setSortKey] = useState<SortKey>('total_cost_usd');
  const [sortAsc, setSortAsc] = useState(false);

  const fetchMetrics = useCallback(async (isStale: () => boolean) => {
    try {
      const [metricsResult, statsResult, healthStatus, promText] = await Promise.all([
        getAgentMetrics().catch(() => ({ agents: [] as AgentMetrics[] })),
        getTaskStats().catch(() => ({ agents: [] as TaskAgentStat[] })),
        getEngineHealth().catch(() => false),
        getEngineMetrics().catch(() => ''),
      ]);
      if (!isStale()) {
        setMetrics(metricsResult.agents);
        setStats(statsResult.agents);
        setEngineHealthy(healthStatus);
        setEnginePrometheus(promText);
        setError(null);
      }
    } catch (cause) {
      if (!isStale()) setError(cause instanceof Error ? cause.message : 'Unable to load metrics');
    } finally {
      if (!isStale()) setLoading(false);
    }
  }, []);

  usePolling(fetchMetrics, 10_000);

  const handleSort = (key: SortKey) => {
    if (sortKey === key) {
      setSortAsc(!sortAsc);
    } else {
      setSortKey(key);
      setSortAsc(false);
    }
  };

  const sorted = [...metrics].sort((a, b) => {
    const av = a[sortKey] ?? 0;
    const bv = b[sortKey] ?? 0;
    if (typeof av === 'string' && typeof bv === 'string') {
      return sortAsc ? av.localeCompare(bv) : bv.localeCompare(av);
    }
    return sortAsc ? (av as number) - (bv as number) : (bv as number) - (av as number);
  });

  const totalCost = metrics.reduce((sum, m) => sum + m.total_cost_usd, 0);
  const totalTokens = metrics.reduce((sum, m) => sum + m.total_tokens, 0);
  const avgLatency = metrics.length > 0
    ? metrics.reduce((sum, m) => sum + (m.avg_latency_ms ?? 0), 0) / metrics.filter((m) => m.avg_latency_ms != null).length || null
    : null;
  const totalFallbacks = metrics.reduce((sum, m) => sum + m.fallback_count, 0);

  function SortHeader({ label, field }: { label: string; field: SortKey }) {
    const active = sortKey === field;
    return (
      <th className="px-4 py-2 font-medium">
        <button
          type="button"
          className="inline-flex items-center gap-1 hover:text-pc-text transition-colors"
          onClick={() => handleSort(field)}
        >
          {label}
          {active && <span className="text-pc-accent">{sortAsc ? '↑' : '↓'}</span>}
        </button>
      </th>
    );
  }

  return (
    <div className="space-y-4">
      <PageHeader
        title="Metrics"
        description="Unified cost, token, latency, and fallback metrics per agent."
        actions={
          <Button variant="ghost" size="sm" onClick={() => void fetchMetrics(() => false)} disabled={loading}>
            {loading ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : <RefreshCw className="h-4 w-4" aria-hidden />}
            Refresh
          </Button>
        }
      />

      {error ? (
        <Card className="border-status-error/30 text-sm text-status-error">
          <p>Metrics are unavailable: {error}</p>
          <p className="mt-1 text-xs text-pc-text-muted">
            The metrics endpoint must be wired before this view can display agent performance data.
          </p>
        </Card>
      ) : loading && metrics.length === 0 ? (
        <div className="flex items-center justify-center py-16 text-pc-text-muted">
          <Loader2 className="mr-2 h-5 w-5 animate-spin" aria-hidden />
          Loading metrics…
        </div>
      ) : metrics.length === 0 ? (
        <Card className="p-6 text-center text-sm" style={{ color: 'var(--pc-text-faint)' }}>
          <p>No metrics data available yet. Agent activity will populate this view.</p>
        </Card>
      ) : (
        <>
          <div className="grid gap-4 md:grid-cols-4">
            <StatCard label="Total cost" value={formatCostUsd(totalCost)} />
            <StatCard label="Total tokens" value={formatTokens(totalTokens)} />
            <StatCard label="Avg latency" value={formatLatency(avgLatency)} />
            <StatCard
              label="Fallbacks"
              value={totalFallbacks}
              tone={totalFallbacks > 0 ? 'warn' : 'ok'}
            />
          </div>

          <Card className="overflow-hidden p-0">
            <div className="border-b border-pc-border px-4 py-2.5">
              <h2 className="text-sm font-semibold text-pc-text">Agent metrics</h2>
              <p className="text-[11px] text-pc-text-muted">Cost, token usage, latency, and fallback counts per agent.</p>
            </div>
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-pc-border text-left text-xs uppercase tracking-wide text-pc-text-muted">
                  <SortHeader label="Agent" field="agent" />
                  <SortHeader label="Tasks" field="total_tasks" />
                  <SortHeader label="Tokens" field="total_tokens" />
                  <th className="px-4 py-2 font-medium">Prompt / Completion</th>
                  <SortHeader label="Cost" field="total_cost_usd" />
                  <SortHeader label="Avg latency" field="avg_latency_ms" />
                  <SortHeader label="Fallbacks" field="fallback_count" />
                </tr>
              </thead>
              <tbody className="divide-y divide-pc-border">
                {sorted.map((m) => (
                  <tr key={m.agent} className="hover:bg-pc-elevated/50">
                    <td className="px-4 py-2 font-medium">
                      <Link to={`/agent/${encodeURIComponent(m.agent)}`} className="text-pc-accent hover:underline">
                        {m.agent}
                      </Link>
                    </td>
                    <td className="px-4 py-2 tabular-nums text-pc-text-secondary">{m.total_tasks}</td>
                    <td className="px-4 py-2 tabular-nums text-pc-text-secondary">{formatTokens(m.total_tokens)}</td>
                    <td className="px-4 py-2 text-xs text-pc-text-muted">
                      {formatTokens(m.prompt_tokens)} / {formatTokens(m.completion_tokens)}
                    </td>
                    <td className="px-4 py-2 tabular-nums text-pc-text-secondary">{formatCostUsd(m.total_cost_usd)}</td>
                    <td className="px-4 py-2 tabular-nums text-pc-text-muted">{formatLatency(m.avg_latency_ms)}</td>
                    <td className="px-4 py-2 tabular-nums">
                      {m.fallback_count > 0
                        ? <Badge tone="warn">{m.fallback_count}</Badge>
                        : <span className="text-pc-text-muted">0</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>

          {stats.length > 0 && (
            <Card className="overflow-hidden p-0">
              <div className="border-b border-pc-border px-4 py-2.5">
                <h2 className="text-sm font-semibold text-pc-text">Task performance</h2>
                <p className="text-[11px] text-pc-text-muted">Task outcomes and average duration per agent.</p>
              </div>
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-pc-border text-left text-xs uppercase tracking-wide text-pc-text-muted">
                    <th className="px-4 py-2 font-medium">Agent</th>
                    <th className="px-4 py-2 font-medium">Total</th>
                    <th className="px-4 py-2 font-medium">Active</th>
                    <th className="px-4 py-2 font-medium">Completed</th>
                    <th className="px-4 py-2 font-medium">Failed</th>
                    <th className="px-4 py-2 font-medium">Avg duration</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-pc-border">
                  {[...stats].sort((a, b) => b.total - a.total).map((stat) => (
                    <tr key={stat.agent} className="hover:bg-pc-elevated/50">
                      <td className="px-4 py-2 font-medium">
                        <Link to={`/agent/${encodeURIComponent(stat.agent)}`} className="text-pc-accent hover:underline">
                          {stat.agent}
                        </Link>
                      </td>
                      <td className="px-4 py-2 tabular-nums text-pc-text-secondary">{stat.total}</td>
                      <td className="px-4 py-2 tabular-nums text-pc-text-secondary">{stat.active}</td>
                      <td className="px-4 py-2 tabular-nums text-status-success">{stat.completed}</td>
                      <td className="px-4 py-2 tabular-nums text-status-error">{stat.failed}</td>
                      <td className="px-4 py-2 tabular-nums text-pc-text-muted">
                        {stat.avg_duration_ms != null ? `${(stat.avg_duration_ms / 1000).toFixed(1)}s` : '—'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </Card>
          )}

          <Card className="overflow-hidden p-0 border-pc-border">
            <div className="border-b border-pc-border px-4 py-2.5 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Activity className="h-4 w-4 text-pc-accent" aria-hidden />
                <h2 className="text-sm font-semibold text-pc-text">Go 1.27 Agent Engine Observability</h2>
              </div>
              <div className="flex items-center gap-2">
                {engineHealthy ? (
                  <Badge variant="success" className="flex items-center gap-1">
                    <CheckCircle2 className="h-3 w-3" /> Engine Active (:9090)
                  </Badge>
                ) : (
                  <Badge variant="neutral" className="flex items-center gap-1">
                    <XCircle className="h-3 w-3" /> Standby / Offline
                  </Badge>
                )}
              </div>
            </div>
            <div className="p-4 bg-pc-elevated/30">
              <p className="text-xs text-pc-text-muted mb-2">
                Live Prometheus metrics scraped from local sidecar daemon at <code>http://127.0.0.1:9090/metrics</code>:
              </p>
              <pre className="text-[11px] font-mono bg-pc-input p-3 rounded-[var(--radius-md)] border border-pc-border overflow-x-auto max-h-48 text-pc-text-secondary leading-relaxed">
                {enginePrometheus || 'No metrics returned from Go engine yet.'}
              </pre>
            </div>
          </Card>
        </>
      )}
    </div>
  );
}

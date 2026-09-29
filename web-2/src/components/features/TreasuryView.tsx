import React from 'react';
import {
  Coins,
  ShieldCheck,
  TrendingDown,
  Sparkles,
  ArrowUpRight,
  Server,
  Cpu,
  Layers
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';

export const TreasuryView: React.FC = () => {
  const { treasuryLedger, ships } = useFleetStore();

  const totalSpent = treasuryLedger.reduce((sum, item) => sum + item.costUSD, 0);
  const monthlyCap = 25.00;
  const percentage = Math.min(100, Math.round((totalSpent / monthlyCap) * 100));

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2">
          <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
            Treasury
          </h1>
          <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            BYOK / BYOM
          </span>
        </div>
        <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
          Provider Cost &amp; Budget Ledger. Pay your chosen AI providers directly with zero platform credit markup.
        </p>
      </div>

      {/* Main Budget Card */}
      <div className="p-6 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
          <div>
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Monthly Active Budget Allocation
            </span>
            <div className="flex items-baseline gap-2 mt-1">
              <span className="text-3xl font-extrabold text-neutral-900 dark:text-neutral-100 font-mono tabular-nums">
                ${totalSpent.toFixed(2)}
              </span>
              <span className="text-xs text-neutral-400 font-mono">
                / ${monthlyCap.toFixed(2)} USD cap
              </span>
            </div>
          </div>

          <div className="text-right">
            <span className="text-xs font-mono text-emerald-500 font-semibold flex items-center gap-1 sm:justify-end">
              <ShieldCheck className="w-4 h-4" />
              16.4% Consumed
            </span>
            <span className="text-[11px] text-neutral-400">
              Projected monthly total: $9.80 USD
            </span>
          </div>
        </div>

        <div className="w-full bg-neutral-100 dark:bg-neutral-800 h-2.5 rounded-full overflow-hidden">
          <div
            className="bg-teal-500 h-full rounded-full transition-all duration-500"
            style={{ width: `${percentage}%` }}
          />
        </div>

        <div className="flex items-center justify-between text-[11px] text-neutral-400 font-mono">
          <span>Soft Warning: $18.00 (72%)</span>
          <span>Hard Drop-Anchor Cap: $25.00 (100%)</span>
        </div>
      </div>

      {/* Optimization Recommendations */}
      <div className="p-4 rounded-xl border border-teal-500/30 bg-teal-50/50 dark:bg-teal-950/20 space-y-2 text-xs">
        <div className="flex items-center gap-2 text-teal-800 dark:text-teal-300 font-bold">
          <Sparkles className="w-4 h-4 text-amber-500" />
          <span>Quartermaster Cost Optimization Advice</span>
        </div>
        <p className="text-neutral-700 dark:text-neutral-300 leading-relaxed">
          Routing static code AST mapping to your local <strong className="font-mono">Ollama (deepseek-r1:14b)</strong> instance eliminated ~$1.40 in recurring token costs over the past 48 hours without compromising reasoning depth.
        </p>
      </div>

      {/* Ledger Table */}
      <div className="space-y-3">
        <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
          Provider Usage &amp; Voyage Ledger
        </span>

        <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-left text-xs">
              <thead className="bg-neutral-50 dark:bg-neutral-900/60 border-b border-neutral-200 dark:border-neutral-800 text-neutral-500 dark:text-neutral-400 font-mono">
                <tr>
                  <th className="p-3">Date</th>
                  <th className="p-3">Quest / Deliverable</th>
                  <th className="p-3">Provider</th>
                  <th className="p-3">Model Profile</th>
                  <th className="p-3 text-right">Tokens</th>
                  <th className="p-3 text-right">Cost (USD)</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-neutral-100 dark:divide-neutral-800 font-mono">
                {treasuryLedger.map((row) => (
                  <tr
                    key={row.id}
                    className="hover:bg-neutral-50/50 dark:hover:bg-neutral-900/30 transition-colors"
                  >
                    <td className="p-3 text-neutral-400">{row.date}</td>
                    <td className="p-3 font-sans font-medium text-neutral-900 dark:text-neutral-100">
                      {row.questTitle}
                    </td>
                    <td className="p-3 text-neutral-700 dark:text-neutral-300">{row.provider}</td>
                    <td className="p-3 text-neutral-500">{row.model}</td>
                    <td className="p-3 text-right tabular-nums text-neutral-600 dark:text-neutral-400">
                      {row.tokensUsed.toLocaleString()}
                    </td>
                    <td className="p-3 text-right tabular-nums font-bold text-teal-600 dark:text-teal-400">
                      ${row.costUSD.toFixed(2)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
};

import React from 'react';
import {
  Shield,
  Lock,
  CheckCircle2,
  AlertTriangle,
  Coins,
  Cpu,
  Brain,
  FileCode
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { apiClient } from '../../utils/apiClient';

export const FleetCodeView: React.FC = () => {
  const { fleetPolicies, riskTiers: storeRiskTiers } = useFleetStore();
  // E3: risk-tier/policies SSOT is the Go engine (GET /api/fleet/policies).
  // Seed from the store's hydrated copy (fetchRealData), engine fetch below
  // is authoritative. No TS-side initialFleetPolicies/initialRiskTiers fallback.
  const [policies, setPolicies] = React.useState<Record<string, unknown>[]>(fleetPolicies ?? []);
  const [riskTiers, setRiskTiers] = React.useState<Record<string, unknown>[]>(storeRiskTiers ?? []);

  React.useEffect(() => {
    apiClient.getFleetPolicies().then((res) => {
      if (res?.policies && res.policies.length > 0) setPolicies(res.policies);
      if (res?.riskTiers && res.riskTiers.length > 0) setRiskTiers(res.riskTiers);
    }).catch(console.error);
  }, []);

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2">
          <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
            Fleet Code
          </h1>
          <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            Policy &amp; Safety v2.4
          </span>
        </div>
        <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
          Policies, permissions, and safety rules that govern agent autonomy and guarantee human command.
        </p>
      </div>

      {/* Risk Classes Hierarchy */}
      <div className="space-y-3">
        <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
          Risk Class Tiers &amp; Tool Classification
        </span>

        <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] overflow-hidden text-xs">
          <div className="divide-y divide-neutral-100 dark:divide-neutral-800">
            {riskTiers.map((r: any) => (
              <div
                key={r.tier}
                className="p-3.5 flex flex-col sm:flex-row sm:items-center justify-between gap-2"
              >
                <div className="space-y-0.5">
                  <div className="font-semibold text-neutral-900 dark:text-neutral-100">
                    {r.label}
                  </div>
                  <div className="text-[11px] text-neutral-500 dark:text-neutral-400">
                    {r.desc}
                  </div>
                </div>
                <span
                  className={`text-[10px] font-mono px-2 py-0.5 rounded uppercase font-semibold self-start sm:self-auto ${
                    r.behavior === 'Autonomous'
                      ? 'bg-blue-500/20 text-blue-500'
                      : r.behavior === 'Gated'
                      ? 'bg-teal-500/20 text-teal-500'
                      : r.behavior === 'Captain Approval'
                      ? 'bg-amber-500/20 text-amber-500'
                      : 'bg-rose-500/20 text-rose-500'
                  }`}
                >
                  {r.behavior}
                </span>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Enforced Sovereign Guardrails */}
      <div className="space-y-3">
        <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
          Enforced Sovereign Policies
        </span>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          {policies.map((p: any, idx: number) => (
            <div
              key={idx}
              className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-2 flex flex-col justify-between"
            >
              <div className="space-y-1.5">
                <div className="flex items-center justify-between">
                  <span className="text-[11px] font-mono font-medium text-teal-600 dark:text-teal-400">
                    {p.category}
                  </span>
                  <div className="flex items-center gap-1 text-[11px] font-medium text-emerald-600 dark:text-emerald-400">
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    <span>{p.status}</span>
                  </div>
                </div>
                <h3 className="text-xs font-semibold text-neutral-900 dark:text-neutral-100">
                  {p.rule}
                </h3>
                <p className="text-[11px] text-neutral-500 dark:text-neutral-400 leading-relaxed">
                  {p.description}
                </p>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Verification Seal */}
      <div className="p-4 rounded-xl border border-teal-500/30 bg-teal-500/5 flex items-center justify-between text-xs">
        <div className="flex items-center gap-2.5 text-neutral-700 dark:text-neutral-300">
          <Shield className="w-4 h-4 text-teal-500 shrink-0" />
          <span>Cryptographic Proof: All policy checks enforced by Rust host &amp; Landlock sandbox.</span>
        </div>
        <span className="font-mono text-[10px] text-teal-600 dark:text-teal-400 font-semibold uppercase">
          Enforced
        </span>
      </div>
    </div>
  );
};

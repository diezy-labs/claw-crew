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

export const FleetCodeView: React.FC = () => {
  const policies = [
    {
      category: 'External Side-Effects',
      rule: 'Require Captain’s Approval for External Writes',
      description: 'Agents may never submit GitHub issues, merge pull requests, trigger deployments, or send outbound messages without explicit cryptographic Owner confirmation.',
      status: 'Enforced'
    },
    {
      category: 'Memory & Learning Boundaries',
      rule: 'Scoped Operational Memory & Proposed Learning',
      description: 'Ship operational memory does not leak across unrelated Workspaces. Any persistent rule update proposed by Crew must be confirmed by the Owner.',
      status: 'Enforced'
    },
    {
      category: 'Treasury & Provider Spending',
      rule: 'Hard Per-Voyage & Monthly Drop-Anchor Caps',
      description: 'Automatically pause any autonomous agent voyage that incurs more than $2.00 USD in model API consumption.',
      status: 'Enforced'
    },
    {
      category: 'Host Sandboxing',
      rule: 'Tauri & Landlock OS Isolation',
      description: 'Rust host layer prevents agent execution from accessing root filesystem directories, SSH keys, or environment secrets.',
      status: 'Enforced'
    }
  ];

  const riskTiers = [
    { tier: 'read_only', label: 'Read-Only (Tier 1)', behavior: 'Autonomous', desc: 'Code search, git log analysis, documentation review.' },
    { tier: 'draft', label: 'Draft Only (Tier 2)', behavior: 'Autonomous', desc: 'Synthesizing PR descriptions, drafting markdown files.' },
    { tier: 'write', label: 'Local Write (Tier 3)', behavior: 'Gated', desc: 'Creating localized patch branches or unit tests.' },
    { tier: 'sensitive', label: 'Sensitive Write (Tier 4)', behavior: 'Captain Approval', desc: 'Creating public repository issues, external webhooks.' },
    { tier: 'destructive', label: 'Destructive (Tier 5)', behavior: 'Strictly Denied', desc: 'Direct production deployment, master branch merge, secret deletion.' }
  ];

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2">
          <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
            Fleet Code
          </h1>
          <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            Policy &amp; Safety v1.4
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
            {riskTiers.map((r) => (
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

      {/* Active Governance Rules */}
      <div className="space-y-3 pt-2">
        <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
          Enforced Operating Invariants
        </span>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {policies.map((p) => (
            <div
              key={p.rule}
              className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-2 text-xs shadow-xs"
            >
              <div className="flex items-center justify-between">
                <span className="text-[10px] font-mono text-neutral-400 uppercase">
                  {p.category}
                </span>
                <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-emerald-500/20 text-emerald-500 font-semibold">
                  {p.status}
                </span>
              </div>
              <h3 className="font-bold text-neutral-900 dark:text-neutral-100">
                {p.rule}
              </h3>
              <p className="text-neutral-600 dark:text-neutral-400 leading-relaxed text-[11px]">
                {p.description}
              </p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};

import React from 'react';
import {
  Anchor,
  CheckCircle2,
  Cpu,
  Key,
  FolderGit2,
  FileCode2,
  Webhook,
  ExternalLink,
  ShieldCheck
} from 'lucide-react';

export const HarborView: React.FC = () => {
  const providers = [
    {
      name: 'Anthropic Claude',
      type: 'Cloud Model Provider',
      status: 'Connected (Active)',
      defaultModel: 'Claude 3.7 Sonnet (Reasoning)',
      keyMask: 'sk-ant-api03-••••••••',
      activeUsage: 'Active on Developer Ship'
    },
    {
      name: 'Google Gemini',
      type: 'Cloud Model Provider',
      status: 'Connected (Active)',
      defaultModel: 'Gemini 2.5 Pro & Flash',
      keyMask: 'AIzaSy••••••••',
      activeUsage: 'Active on Market & Research Ships'
    },
    {
      name: 'Ollama Local Endpoint',
      type: 'Local Inference (Zero Cost)',
      status: 'Running (http://127.0.0.1:11434)',
      defaultModel: 'deepseek-r1:14b / qwen2.5:7b',
      keyMask: 'No API Key Required',
      activeUsage: 'Static AST & Code Audits'
    },
    {
      name: 'OpenAI-Compatible Local Proxy',
      type: 'Self-Hosted Gateway',
      status: 'Standby',
      defaultModel: 'Custom Endpoint',
      keyMask: 'bearer-token-••••',
      activeUsage: 'Configured for Private VPC'
    }
  ];

  const tools = [
    {
      name: 'GitHub Repository Connector',
      target: 'diezy-labs/claw-crew (Branch: feat/enhance-agent-phase)',
      auth: 'Read-only default + Captain Approval for issues/PRs',
      status: 'Synced'
    },
    {
      name: 'Local Filesystem Sandbox',
      target: 'Tauri Host Landlock Sandboxing Active',
      auth: 'Scoped to current repository directory',
      status: 'Secure'
    },
    {
      name: 'Webhook Event Listener',
      target: 'https://ais-dev-n2xbpzaqb6qts6lag3gugq-963610025570.asia-southeast1.run.app/api/webhooks',
      auth: 'HMAC SHA-256 Signature Verification',
      status: 'Active'
    }
  ];

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2">
          <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
            Harbor
          </h1>
          <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            BYOK &amp; Integrations
          </span>
        </div>
        <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
          Connect the intelligence models, local endpoints, git repositories, and external tools your Fleet can use.
        </p>
      </div>

      {/* Intelligence Providers Grid */}
      <div className="space-y-3">
        <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
          Connected Intelligence &amp; Model Accounts (BYOK / BYOM)
        </span>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {providers.map((p) => (
            <div
              key={p.name}
              className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3 text-xs shadow-xs"
            >
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-2">
                  <Cpu className="w-4 h-4 text-teal-500 shrink-0" />
                  <div>
                    <h3 className="font-bold text-neutral-900 dark:text-neutral-100">
                      {p.name}
                    </h3>
                    <div className="text-[11px] text-neutral-400 font-mono">
                      {p.type}
                    </div>
                  </div>
                </div>
                <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-emerald-500/20 text-emerald-500 font-semibold shrink-0">
                  {p.status}
                </span>
              </div>

              <div className="p-2.5 rounded-lg bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 font-mono text-[11px] space-y-1">
                <div className="flex justify-between">
                  <span className="text-neutral-400">Default Model:</span>
                  <span className="text-neutral-800 dark:text-neutral-200 font-semibold">
                    {p.defaultModel}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-neutral-400">Credential:</span>
                  <span className="text-neutral-500">{p.keyMask}</span>
                </div>
              </div>

              <div className="text-[11px] text-neutral-500 dark:text-neutral-400 flex items-center justify-between">
                <span>{p.activeUsage}</span>
                <button className="text-teal-600 dark:text-teal-400 hover:underline">
                  Configure →
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Tool & Work Connections */}
      <div className="space-y-3 pt-2">
        <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
          Work Environment &amp; Tool Connections
        </span>

        <div className="space-y-3">
          {tools.map((t) => (
            <div
              key={t.name}
              className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs"
            >
              <div className="space-y-1">
                <div className="flex items-center gap-2">
                  <FolderGit2 className="w-4 h-4 text-teal-500 shrink-0" />
                  <span className="font-bold text-neutral-900 dark:text-neutral-100">
                    {t.name}
                  </span>
                </div>
                <div className="text-[11px] text-neutral-500 font-mono">
                  {t.target}
                </div>
                <div className="text-[11px] text-neutral-600 dark:text-neutral-400">
                  {t.auth}
                </div>
              </div>

              <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/20 text-teal-600 dark:text-teal-400 font-semibold self-start sm:self-auto">
                {t.status}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};

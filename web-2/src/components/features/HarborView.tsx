import React, { useState } from 'react';
import {
  Anchor,
  CheckCircle2,
  Cpu,
  Key,
  FolderGit2,
  FileCode2,
  Webhook,
  ExternalLink,
  ShieldCheck,
  Radio,
  Network,
  Share2,
  Terminal,
  Layers,
  Sparkles,
  RefreshCw,
  Plus,
  ShieldAlert,
  Sliders,
  Check,
  Zap
} from 'lucide-react';
import { SubMenuScroller } from '../common/SubMenuScroller';

export const HarborView: React.FC = () => {
  const [activeHarborTab, setActiveHarborTab] = useState<'providers' | 'connectors' | 'a2a_mesh' | 'plugins'>('providers');
  const [isScanningPeers, setIsScanningPeers] = useState(false);
  const [peerList, setPeerList] = useState([
    {
      id: 'peer-node-alpha',
      name: 'Orion Defense Fleet Node',
      endpoint: 'http://192.168.1.14:8080',
      discoveryMethod: 'mDNS Beacon',
      status: 'Verified & Trusted',
      agentCard: {
        agentAlias: 'sentinel-orion',
        capabilities: ['security_audit', 'canary_rollback', 'vulnerability_scan'],
        models: ['Claude 3.7 Sonnet', 'Gemini 2.5 Flash'],
        publicKeyMask: 'ed25519-pk-9f8a••••4412'
      },
      lastPing: '4s ago'
    },
    {
      id: 'peer-node-beta',
      name: 'Adiet Workstation Node (Local Mesh)',
      endpoint: 'http://127.0.0.1:8080',
      discoveryMethod: 'Self (Local Host)',
      status: 'Active Anchor Node',
      agentCard: {
        agentAlias: 'horizon-orchestrator',
        capabilities: ['repository_health', 'ci_triage', 'code_refactoring'],
        models: ['Claude 3.7 Sonnet', 'DeepSeek-R1 (Local)'],
        publicKeyMask: 'ed25519-pk-33c1••••99a8'
      },
      lastPing: 'Active'
    }
  ]);

  const [plugins, setPlugins] = useState([
    {
      id: 'plug-fts5',
      name: 'SQLite-FTS5-Indexer.wasm',
      version: 'v1.2.0',
      runtime: 'Wasmtime / WASI Preview 2',
      memoryQuotaMB: 32,
      permissions: ['read_workspace_cache', 'write_vector_db'],
      status: 'Sandboxed & Active',
      size: '4.2 MB'
    },
    {
      id: 'plug-linter',
      name: 'AST-Syntax-Auditor.wasm',
      version: 'v0.9.4',
      runtime: 'Wasmtime / WASI Preview 2',
      memoryQuotaMB: 16,
      permissions: ['read_file_ast'],
      status: 'Sandboxed & Active',
      size: '1.8 MB'
    },
    {
      id: 'plug-crypto',
      name: 'Ed25519-Verifiable-Intent.wasm',
      version: 'v2.1.0',
      runtime: 'Wasmtime / WASI Preview 2',
      memoryQuotaMB: 8,
      permissions: ['cryptographic_signing'],
      status: 'Sandboxed & Active',
      size: '950 KB'
    }
  ]);

  const providers = [
    {
      name: 'Anthropic Claude',
      type: 'Cloud Model Provider',
      status: 'Connected (Active)',
      defaultModel: 'Claude 3.7 Sonnet (Hybrid Reasoning)',
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
      target: 'diezy-labs/claw-crew (Branch: feat/enhance-agent-phase2)',
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
      target: 'https://ais-dev-gmtvncp5itg5m7aoliehtz-965837518617.asia-southeast1.run.app/api/webhooks',
      auth: 'HMAC SHA-256 Signature Verification',
      status: 'Active'
    }
  ];

  const handleScanMdnsPeers = () => {
    setIsScanningPeers(true);
    setTimeout(() => {
      setIsScanningPeers(false);
    }, 700);
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-5xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
              Harbor
            </h1>
            <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
              Intelligence &amp; Mesh Connectors
            </span>
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
            Connect the intelligence models, local endpoints, git repositories, A2A mesh peers, and WebAssembly tools your Fleet can use.
          </p>
        </div>

        {activeHarborTab === 'a2a_mesh' && (
          <button
            onClick={handleScanMdnsPeers}
            disabled={isScanningPeers}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold text-xs hover:opacity-90 transition-all cursor-pointer shadow-xs disabled:opacity-50 self-start"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isScanningPeers ? 'animate-spin' : ''}`} />
            <span>Scan mDNS Peers</span>
          </button>
        )}
      </div>

      {/* Harbor Navigation Tabs */}
      <div className="flex items-center gap-2 border-b border-neutral-200 dark:border-neutral-800 pb-3 pt-1 overflow-x-auto scrollbar-none shrink-0 text-xs">
        <button
          onClick={() => setActiveHarborTab('providers')}
          className={`px-3 py-1.5 rounded-lg font-medium transition-colors cursor-pointer ${
            activeHarborTab === 'providers'
              ? 'bg-neutral-200 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold'
              : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
          }`}
        >
          Model Intelligence (BYOK / BYOM)
        </button>
        <button
          onClick={() => setActiveHarborTab('connectors')}
          className={`px-3 py-1.5 rounded-lg font-medium transition-colors cursor-pointer ${
            activeHarborTab === 'connectors'
              ? 'bg-neutral-200 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold'
              : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
          }`}
        >
          Work Connectors &amp; Git
        </button>
        <button
          onClick={() => setActiveHarborTab('a2a_mesh')}
          className={`px-3 py-1.5 rounded-lg font-medium transition-colors cursor-pointer flex items-center gap-1.5 ${
            activeHarborTab === 'a2a_mesh'
              ? 'bg-neutral-200 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold'
              : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
          }`}
        >
          <Network className="w-3.5 h-3.5 text-teal-500" />
          <span>A2A Mesh Network &amp; Peers</span>
          <span className="text-[9px] font-mono px-1 rounded bg-teal-500/20 text-teal-600 dark:text-teal-300">
            {peerList.length}
          </span>
        </button>
        <button
          onClick={() => setActiveHarborTab('plugins')}
          className={`px-3 py-1.5 rounded-lg font-medium transition-colors cursor-pointer flex items-center gap-1.5 ${
            activeHarborTab === 'plugins'
              ? 'bg-neutral-200 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold'
              : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
          }`}
        >
          <Layers className="w-3.5 h-3.5 text-teal-500" />
          <span>Wasm Plugins ({plugins.length})</span>
        </button>
      </div>

      {/* Tab 1: Model Intelligence (BYOK / BYOM) */}
      {activeHarborTab === 'providers' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Connected Intelligence &amp; Model Accounts
            </span>
            <span className="text-[11px] font-mono text-teal-600 dark:text-teal-400">
              No vendor lock-in &middot; Zero forced credits
            </span>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {providers.map((p) => (
              <div
                key={p.name}
                className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3 text-xs shadow-xs"
              >
                <div className="flex items-start justify-between">
                  <div className="space-y-0.5">
                    <div className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                      <Cpu className="w-3.5 h-3.5 text-teal-500" />
                      <span>{p.name}</span>
                    </div>
                    <div className="text-[11px] text-neutral-400">{p.type}</div>
                  </div>
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/20 text-emerald-500 font-semibold flex items-center gap-1">
                    <CheckCircle2 className="w-3 h-3" />
                    <span>Active</span>
                  </span>
                </div>

                <div className="space-y-1.5 pt-2 border-t border-neutral-100 dark:border-neutral-800/80 font-mono text-[11px]">
                  <div className="flex justify-between">
                    <span className="text-neutral-400">Default Model:</span>
                    <span className="text-neutral-700 dark:text-neutral-300 font-semibold">{p.defaultModel}</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-neutral-400">API Key Mask:</span>
                    <span className="text-neutral-500">{p.keyMask}</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-neutral-400">Routing:</span>
                    <span className="text-teal-600 dark:text-teal-400 font-sans">{p.activeUsage}</span>
                  </div>
                </div>

                <div className="pt-2 flex items-center justify-end gap-2">
                  <button className="px-2.5 py-1 rounded-md border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white transition-colors cursor-pointer">
                    Configure Keys
                  </button>
                  <button className="px-2.5 py-1 rounded-md bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 hover:bg-neutral-200 dark:hover:bg-neutral-700 transition-colors cursor-pointer">
                    Test Latency
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Tab 2: Work Connectors & Git */}
      {activeHarborTab === 'connectors' && (
        <div className="space-y-4">
          <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
            Active Work Connectors &amp; Scopes
          </span>

          <div className="space-y-3">
            {tools.map((t) => (
              <div
                key={t.name}
                className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs shadow-xs"
              >
                <div className="space-y-1">
                  <div className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                    <FolderGit2 className="w-4 h-4 text-teal-500" />
                    <span>{t.name}</span>
                  </div>
                  <div className="font-mono text-[11px] text-neutral-500 dark:text-neutral-400">{t.target}</div>
                  <div className="text-[11px] text-neutral-600 dark:text-neutral-400">{t.auth}</div>
                </div>

                <div className="flex items-center gap-2 self-start sm:self-auto">
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/20 text-emerald-500 font-semibold">
                    {t.status}
                  </span>
                  <button className="px-2.5 py-1 rounded-md border border-neutral-200 dark:border-neutral-700 text-neutral-700 dark:text-neutral-300 hover:border-teal-500 transition-colors cursor-pointer">
                    Review Scopes
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Tab 3: A2A Mesh Network & Peer Discovery */}
      {activeHarborTab === 'a2a_mesh' && (
        <div className="space-y-4">
          <div className="p-3.5 rounded-xl border border-teal-500/30 bg-teal-500/5 space-y-1 text-xs">
            <div className="flex items-center justify-between">
              <span className="font-bold text-teal-700 dark:text-teal-300 flex items-center gap-1.5">
                <Network className="w-4 h-4 text-teal-500" />
                A2A Mesh Protocol v1.0 Active
              </span>
              <span className="text-[10px] font-mono text-neutral-400">Endpoint: /.well-known/agent.json</span>
            </div>
            <p className="text-neutral-600 dark:text-neutral-300 leading-relaxed text-[11px]">
              The Fleet discovers and exchanges tasks with neighboring agent networks via mDNS and A2A catalog cards.
              All remote delegation is signed via Ed25519 and constrained by your local Fleet Code policies.
            </p>
          </div>

          <div className="space-y-3">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Discovered Mesh Nodes &amp; Remote Fleets ({peerList.length})
            </span>

            <div className="space-y-3">
              {peerList.map((peer) => (
                <div
                  key={peer.id}
                  className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] text-xs shadow-xs space-y-3"
                >
                  <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-neutral-100 dark:border-neutral-800 pb-2.5">
                    <div>
                      <div className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                        <Radio className="w-4 h-4 text-teal-500" />
                        <span>{peer.name}</span>
                        <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-500">
                          {peer.discoveryMethod}
                        </span>
                      </div>
                      <div className="font-mono text-[11px] text-neutral-400 mt-0.5">{peer.endpoint}</div>
                    </div>

                    <div className="flex items-center gap-2">
                      <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/20 text-emerald-500 font-semibold">
                        {peer.status}
                      </span>
                    </div>
                  </div>

                  {/* Remote Agent Card Info */}
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-[11px] font-mono">
                    <div>
                      <span className="text-neutral-400 block text-[10px] uppercase">Exported Capabilities:</span>
                      <div className="flex flex-wrap gap-1 mt-1 font-sans">
                        {peer.agentCard.capabilities.map((c) => (
                          <span key={c} className="px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 text-[10px]">
                            {c}
                          </span>
                        ))}
                      </div>
                    </div>
                    <div>
                      <span className="text-neutral-400 block text-[10px] uppercase">Cryptographic Signature:</span>
                      <div className="text-teal-600 dark:text-teal-400 mt-1">{peer.agentCard.publicKeyMask}</div>
                    </div>
                  </div>

                  <div className="flex items-center justify-end gap-2 pt-2 border-t border-neutral-100 dark:border-neutral-800">
                    <button className="px-2.5 py-1 rounded-md border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 text-xs hover:border-teal-500 transition-colors cursor-pointer">
                      Inspect Card JSON
                    </button>
                    <button className="px-2.5 py-1 rounded-md bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold text-xs hover:opacity-90 transition-opacity cursor-pointer">
                      Delegate Mission &rarr;
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Tab 4: WebAssembly Plugins & Sandbox Grants */}
      {activeHarborTab === 'plugins' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Sandboxed WebAssembly (WASM) Modules ({plugins.length})
            </span>
            <button className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold cursor-pointer">
              <Plus className="w-3 h-3" />
              <span>Install Wasm Plugin</span>
            </button>
          </div>

          <div className="space-y-3">
            {plugins.map((plug) => (
              <div
                key={plug.id}
                className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] text-xs shadow-xs space-y-2.5"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Layers className="w-4 h-4 text-teal-500" />
                    <div>
                      <div className="font-bold font-mono text-neutral-900 dark:text-neutral-100">
                        {plug.name}
                      </div>
                      <div className="text-[10px] text-neutral-400">
                        {plug.runtime} &middot; {plug.size} &middot; Memory Quota: {plug.memoryQuotaMB} MB
                      </div>
                    </div>
                  </div>

                  <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/20 text-emerald-500 font-semibold">
                    {plug.status}
                  </span>
                </div>

                <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800/80 flex items-center justify-between">
                  <div className="flex items-center gap-1.5 flex-wrap">
                    <span className="text-[10px] text-neutral-400">Grants:</span>
                    {plug.permissions.map((perm) => (
                      <span key={perm} className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400">
                        {perm}
                      </span>
                    ))}
                  </div>

                  <button className="text-[11px] text-teal-600 dark:text-teal-400 hover:underline cursor-pointer">
                    Manage Sandbox Limits
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

import React, { useState, useRef, useEffect } from 'react';
import {
  Terminal,
  Play,
  RotateCcw,
  Trash2,
  Copy,
  Check,
  CheckCircle2,
  AlertTriangle,
  XCircle,
  Cpu,
  HardDrive,
  Activity,
  Layers,
  Search,
  Download,
  ExternalLink,
  Wifi,
  Radio,
  Server,
  Sparkles,
  ArrowRight,
  ShieldCheck,
  Zap,
  Clock,
  Send,
  CornerDownLeft,
  X,
  Plus
} from 'lucide-react';
import { PageHeaderNav } from '../common/PageHeaderNav';
import { Button } from '../common/Button';
import { ItemCard } from '../common/ItemCard';
import { StatusBadge } from '../common/StatusBadge';
import { EmptyState } from '../common/EmptyState';
import { SearchBar } from '../common/SearchBar';
import { ConfirmationModal } from '../common/ConfirmationModal';
import { CodeSnippet } from '../common/CodeSnippet';
import { apiClient } from '../../utils/apiClient';

export type EngineRoomTab =
  | 'terminal'
  | 'sessions'
  | 'history'
  | 'processes'
  | 'logs'
  | 'connections';

interface TerminalLine {
  id: string;
  type: 'input' | 'output' | 'error' | 'system' | 'success';
  content: string;
  timestamp: string;
}

interface ActiveSession {
  id: string;
  tty: string;
  title: string;
  actor: string;
  pid: number;
  cpu: string;
  memory: string;
  status: 'active' | 'idle' | 'busy';
  uptime: string;
}

interface CommandHistoryItem {
  id: string;
  command: string;
  actor: string;
  exitCode: number;
  duration: string;
  timestamp: string;
}

interface ProcessItem {
  id: string;
  name: string;
  command: string;
  pid: number;
  cpu: number;
  memoryMB: number;
  uptime: string;
  status: 'running' | 'idle' | 'stopped';
}

interface SystemLogEntry {
  id: string;
  level: 'info' | 'warn' | 'error' | 'debug';
  source: string;
  message: string;
  timestamp: string;
}

export const EngineRoomView: React.FC = () => {
  const [activeTab, setActiveTab] = useState<EngineRoomTab>('terminal');
  const [commandInput, setCommandInput] = useState('');
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);
  const [autoScrollLogs, setAutoScrollLogs] = useState(true);
  const [logFilterLevel, setLogFilterLevel] = useState<string>('all');
  const [logSearchQuery, setLogSearchQuery] = useState('');
  const [sessionToKill, setSessionToKill] = useState<ActiveSession | null>(null);
  const [systemMetrics, setSystemMetrics] = useState<any>(null);
  const [isRefreshingProcs, setIsRefreshingProcs] = useState(false);

  // Terminal scroll reference
  const terminalBottomRef = useRef<HTMLDivElement | null>(null);
  const logsBottomRef = useRef<HTMLDivElement | null>(null);

  // Initial terminal history
  const [terminalLines, setTerminalLines] = useState<TerminalLine[]>([
    {
      id: 'init-1',
      type: 'system',
      content: '⚓ Sovereign Galleon Engine Room v2.4.0 [x86_64-linux-gnu / PTY host]',
      timestamp: '08:00:12'
    },
    {
      id: 'init-2',
      type: 'system',
      content: 'Environment Landlocked: /app/workspace [Read+Scoped Execution]',
      timestamp: '08:00:12'
    },
    {
      id: 'init-3',
      type: 'input',
      content: 'fleet check-services --all',
      timestamp: '08:00:14'
    },
    {
      id: 'init-4',
      type: 'success',
      content: '✔ Gateway (:8080)   [OK - 14ms latency]\n✔ Mesh Peer Discovery [OK - 2 peers linked]\n✔ Ollama Host         [OK - deepseek-r1:14b ready]\n✔ WASI Plugin Sandbox [OK - 3 active engines]',
      timestamp: '08:00:15'
    }
  ]);

  // Command History
  const [commandHistory, setCommandHistory] = useState<CommandHistoryItem[]>([
    {
      id: 'cmd-1',
      command: 'fleet check-services --all',
      actor: 'Captain',
      exitCode: 0,
      duration: '42ms',
      timestamp: '08:00:14'
    },
    {
      id: 'cmd-2',
      command: 'cargo check --workspace --color=always',
      actor: 'Specialist: Orion',
      exitCode: 0,
      duration: '1.24s',
      timestamp: '07:54:20'
    },
    {
      id: 'cmd-3',
      command: 'git status -s && git branch --show-current',
      actor: 'Navigator',
      exitCode: 0,
      duration: '18ms',
      timestamp: '07:48:11'
    },
    {
      id: 'cmd-4',
      command: 'docker inspect galleon-gateway --format "{{.State.Status}}"',
      actor: 'Captain',
      exitCode: 0,
      duration: '65ms',
      timestamp: '07:35:02'
    },
    {
      id: 'cmd-5',
      command: 'systemctl restart mdns-discovery-agent',
      actor: 'Auto-Doctor',
      exitCode: 0,
      duration: '210ms',
      timestamp: '07:20:44'
    },
    {
      id: 'cmd-6',
      command: 'curl -fsSL http://127.0.0.1:11434/api/tags',
      actor: 'Captain',
      exitCode: 0,
      duration: '11ms',
      timestamp: '06:58:30'
    },
    {
      id: 'cmd-7',
      command: 'kill -9 9412',
      actor: 'Specialist: Astra',
      exitCode: 137,
      duration: '5ms',
      timestamp: '06:40:15'
    }
  ]);

  // Active Sessions
  const [sessions, setSessions] = useState<ActiveSession[]>([
    {
      id: 'pts-0',
      tty: 'pts/0',
      title: 'Interactive Captain Console',
      actor: 'Captain (Console Host)',
      pid: 1042,
      cpu: '0.4%',
      memory: '24.2 MB',
      status: 'active',
      uptime: '4h 12m'
    },
    {
      id: 'pts-1',
      tty: 'pts/1',
      title: 'Horizon Orchestrator Navigator',
      actor: 'Navigator Agent (Ship Orion)',
      pid: 2180,
      cpu: '1.6%',
      memory: '48.6 MB',
      status: 'active',
      uptime: '2h 45m'
    },
    {
      id: 'pts-2',
      tty: 'pts/2',
      title: 'AST Syntax Auditor Sandbox',
      actor: 'Specialist: Astra (Developer Ship)',
      pid: 3904,
      cpu: '0.1%',
      memory: '14.8 MB',
      status: 'idle',
      uptime: '1h 10m'
    }
  ]);

  // Process Monitor
  const [processes, setProcesses] = useState<ProcessItem[]>([
    {
      id: 'proc-1',
      name: 'galleon-gateway',
      command: 'galleon-core --port 8080 --host 0.0.0.0 --auth-hmac',
      pid: 1420,
      cpu: 1.8,
      memoryMB: 54.2,
      uptime: '4h 12m',
      status: 'running'
    },
    {
      id: 'proc-2',
      name: 'mesh-discovery-mdns',
      command: 'galleon-mesh-mdns --zone sovereign.local --interval 3s',
      pid: 1682,
      cpu: 0.3,
      memoryMB: 18.4,
      uptime: '4h 11m',
      status: 'running'
    },
    {
      id: 'proc-3',
      name: 'ollama-bridge-proxy',
      command: 'ollama-proxy --upstream 127.0.0.1:11434 --timeout 30s',
      pid: 1890,
      cpu: 0.8,
      memoryMB: 36.1,
      uptime: '3h 50m',
      status: 'running'
    },
    {
      id: 'proc-4',
      name: 'landlock-fs-sandbox',
      command: 'kernel-lsm-guard --root /app/workspace --mode enforce',
      pid: 2040,
      cpu: 0.1,
      memoryMB: 8.9,
      uptime: '4h 12m',
      status: 'running'
    },
    {
      id: 'proc-5',
      name: 'sqlite-fts5-worker',
      command: 'wasmtime /plugins/fts5.wasm --max-mem 32MB',
      pid: 2410,
      cpu: 0.0,
      memoryMB: 28.5,
      uptime: '2h 15m',
      status: 'idle'
    },
    {
      id: 'proc-6',
      name: 'vector-memory-engine',
      command: 'embedded-vector-db --storage /tmp/fleet-embed.bin',
      pid: 2812,
      cpu: 0.5,
      memoryMB: 62.0,
      uptime: '3h 10m',
      status: 'running'
    }
  ]);

  // System Logs
  const [logs, setLogs] = useState<SystemLogEntry[]>([
    {
      id: 'log-1',
      level: 'info',
      source: 'galleon-gateway',
      message: 'TCP socket bound to 0.0.0.0:8080 (IPv4 + IPv6 Dual Stack)',
      timestamp: '08:00:10'
    },
    {
      id: 'log-2',
      level: 'info',
      source: 'landlock-guard',
      message: 'Kernel Landlock LSM ABI v3 active; filesystem access restricted to project workspace.',
      timestamp: '08:00:11'
    },
    {
      id: 'log-3',
      level: 'debug',
      source: 'mesh-mdns',
      message: 'Broadcasting A2A node announcement: horizon-orchestrator.sovereign.local',
      timestamp: '08:00:13'
    },
    {
      id: 'log-4',
      level: 'info',
      source: 'mesh-mdns',
      message: 'Discovered peer: Orion Defense Fleet Node [192.168.1.14:8080] via mDNS',
      timestamp: '08:00:14'
    },
    {
      id: 'log-5',
      level: 'warn',
      source: 'ollama-bridge',
      message: 'Context window allocated: 8,192 tokens. Local GPU memory at 68% threshold.',
      timestamp: '08:01:22'
    },
    {
      id: 'log-6',
      level: 'info',
      source: 'vector-engine',
      message: 'Compacting semantic index: 1,420 document chunks indexed with cosine similarity.',
      timestamp: '08:02:40'
    },
    {
      id: 'log-7',
      level: 'debug',
      source: 'galleon-gateway',
      message: 'Heartbeat response sent to 127.0.0.1 (RTT: 0.8ms)',
      timestamp: '08:03:00'
    }
  ]);

  // Preset troubleshooting commands
  const presetCommands = [
    { label: 'Check Services', cmd: 'fleet check-services --all' },
    { label: 'Process List', cmd: 'ps aux --sort=-%cpu' },
    { label: 'Git Status', cmd: 'git status -s' },
    { label: 'Mesh Topology', cmd: 'fleet mesh status' },
    { label: 'Memory Usage', cmd: 'free -h && vmstat -s' },
    { label: 'Port Listeners', cmd: 'ss -tulpn' }
  ];

  // Auto scroll terminal
  useEffect(() => {
    if (activeTab === 'terminal') {
      terminalBottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [terminalLines, activeTab]);

  // Auto scroll logs
  useEffect(() => {
    if (activeTab === 'logs' && autoScrollLogs) {
      logsBottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [logs, autoScrollLogs, activeTab]);

  // Load real backend processes and system metrics
  useEffect(() => {
    apiClient.getEngineProcesses().then((data) => {
      if (data && data.length > 0) {
        setProcesses(data);
      }
    }).catch(console.error);

    apiClient.getSystemMetrics().then((metrics) => {
      if (metrics) setSystemMetrics(metrics);
    }).catch(console.error);
  }, []);

  const handleRefreshProcesses = async () => {
    setIsRefreshingProcs(true);
    try {
      const data = await apiClient.getEngineProcesses();
      if (data && data.length > 0) setProcesses(data);
      const metrics = await apiClient.getSystemMetrics();
      if (metrics) setSystemMetrics(metrics);
    } catch (err) {
      console.error('Failed to refresh processes:', err);
    } finally {
      setIsRefreshingProcs(false);
    }
  };

  const handleRunCommand = async (cmdText?: string) => {
    const textToRun = cmdText || commandInput.trim();
    if (!textToRun) return;

    const time = new Date().toTimeString().slice(0, 8);

    if (textToRun.toLowerCase() === 'clear' || textToRun.toLowerCase() === 'cls') {
      setTerminalLines([]);
      setCommandInput('');
      return;
    }

    // Add input line
    const inputLine: TerminalLine = {
      id: `cmd-${Date.now()}`,
      type: 'input',
      content: textToRun,
      timestamp: time
    };

    setTerminalLines((prev) => [...prev, inputLine]);
    setCommandInput('');

    try {
      const res = await apiClient.executeTerminalCommand(textToRun);
      const outputLine: TerminalLine = {
        id: `out-${Date.now()}`,
        type: res.exitCode === 0 ? 'success' : 'error',
        content: res.stdout,
        timestamp: new Date().toTimeString().slice(0, 8)
      };

      setTerminalLines((prev) => [...prev, outputLine]);

      setCommandHistory((prev) => [
        {
          id: `cmd-${Date.now()}`,
          command: textToRun,
          actor: 'Captain',
          exitCode: res.exitCode,
          duration: res.duration || '14ms',
          timestamp: time
        },
        ...prev
      ]);
    } catch (err: any) {
      const errLine: TerminalLine = {
        id: `out-${Date.now()}`,
        type: 'error',
        content: `Command error: ${err?.message || 'Host execution failed'}`,
        timestamp: new Date().toTimeString().slice(0, 8)
      };
      setTerminalLines((prev) => [...prev, errLine]);
    }

    // Add log
    setLogs((prev) => [
      ...prev,
      {
        id: `log-${Date.now()}`,
        level: 'info',
        source: 'terminal-pty',
        message: `Command executed by Captain: "${textToRun}" (Exit 0)`,
        timestamp: time
      }
    ]);

    setCommandInput('');
  };

  const handleRestartProcess = (procId: string) => {
    setProcesses((prev) =>
      prev.map((p) => (p.id === procId ? { ...p, status: 'running', uptime: '1s' } : p))
    );
    const proc = processes.find((p) => p.id === procId);
    if (proc) {
      setLogs((prev) => [
        ...prev,
        {
          id: `log-${Date.now()}`,
          level: 'warn',
          source: proc.name,
          message: `Process PID ${proc.pid} gracefully restarted by operator.`,
          timestamp: new Date().toTimeString().slice(0, 8)
        }
      ]);
    }
  };

  const handleCopyCommand = (cmd: string, index: number) => {
    navigator.clipboard?.writeText(cmd);
    setCopiedIndex(index);
    setTimeout(() => setCopiedIndex(null), 1500);
  };

  const filteredLogs = logs.filter((log) => {
    const matchesLevel = logFilterLevel === 'all' || log.level === logFilterLevel;
    const matchesSearch =
      !logSearchQuery ||
      log.message.toLowerCase().includes(logSearchQuery.toLowerCase()) ||
      log.source.toLowerCase().includes(logSearchQuery.toLowerCase());
    return matchesLevel && matchesSearch;
  });

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto px-4 sm:px-6 pt-0 pb-6 space-y-4 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Reusable General Header with Integrated Chips */}
      <PageHeaderNav
        icon={<Terminal className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Engine Room"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10 items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
            PTY / Host Bridge Active
          </span>
        }
        description="Local terminal operations, daemon process monitoring, background worker inspection, and system logging."
        actions={
          <div className="flex items-center gap-1.5 sm:gap-2 shrink-0">
            <Button
              variant="outline"
              size="sm"
              icon={<RotateCcw className="w-3.5 h-3.5" />}
              shortLabel="Restart"
              onClick={() => handleRunCommand('fleet check-services --all')}
              title="Verify and ping core fleet services"
            >
              Ping Services
            </Button>
            <Button
              variant="primary"
              size="sm"
              icon={<Terminal className="w-3.5 h-3.5" />}
              shortLabel="Command"
              onClick={() => {
                setActiveTab('terminal');
                handleRunCommand('fleet doctor --deep');
              }}
            >
              Run Doctor
            </Button>
          </div>
        }
        chips={{
          items: [
            { id: 'terminal', label: 'Local Terminal', icon: <Terminal className="w-3.5 h-3.5 text-teal-500" /> },
            { id: 'sessions', label: 'Active Sessions', count: sessions.length },
            { id: 'history', label: 'Command History', count: commandHistory.length },
            { id: 'processes', label: 'Process Monitor', count: processes.length },
            { id: 'logs', label: 'System Logs', count: logs.length },
            { id: 'connections', label: 'Connections', count: 4 }
          ],
          selectedId: activeTab,
          onSelect: (id) => setActiveTab(id as EngineRoomTab),
          variant: 'pills'
        }}
      />

      {/* TAB 1: LOCAL TERMINAL */}
      {activeTab === 'terminal' && (
        <div className="flex-1 flex flex-col space-y-3 min-h-[500px]">
          {/* Quick presets toolbar */}
          <div className="flex items-center gap-1.5 overflow-x-auto pb-1 scrollbar-none text-xs">
            <span className="text-[11px] font-mono text-neutral-400 uppercase tracking-wider shrink-0 mr-1 flex items-center gap-1">
              <Zap className="w-3 h-3 text-amber-500" />
              Presets:
            </span>
            {presetCommands.map((preset) => (
              <button
                key={preset.label}
                type="button"
                onClick={() => handleRunCommand(preset.cmd)}
                className="px-2.5 py-1 rounded-md border border-neutral-200 dark:border-neutral-800 bg-white/70 dark:bg-neutral-900/60 text-[11px] font-mono text-neutral-700 dark:text-neutral-300 hover:border-teal-500/50 hover:text-teal-600 dark:hover:text-teal-400 transition-colors whitespace-nowrap cursor-pointer shrink-0 shadow-2xs"
              >
                {preset.label}
              </button>
            ))}
          </div>

          {/* Terminal Shell Window */}
          <div className="flex-1 flex flex-col rounded-2xl border border-neutral-800 bg-[#0c0d10] text-neutral-200 shadow-lg overflow-hidden min-h-[440px]">
            {/* Terminal Window Header Bar */}
            <div className="px-4 py-2.5 border-b border-neutral-800 bg-[#121418] flex items-center justify-between select-none">
              <div className="flex items-center gap-2">
                <div className="flex items-center gap-1.5">
                  <span className="w-2.5 h-2.5 rounded-full bg-rose-500/80 inline-block" />
                  <span className="w-2.5 h-2.5 rounded-full bg-amber-500/80 inline-block" />
                  <span className="w-2.5 h-2.5 rounded-full bg-emerald-500/80 inline-block" />
                </div>
                <span className="text-xs font-mono text-neutral-400 ml-2">
                  pts/0 · bash · 80x24 · Landlocked Sandbox
                </span>
              </div>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setTerminalLines([])}
                  className="px-2 py-1 rounded text-[11px] font-mono text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 transition-colors cursor-pointer flex items-center gap-1"
                  title="Clear Terminal Display"
                >
                  <Trash2 className="w-3 h-3" />
                  <span>Clear</span>
                </button>
              </div>
            </div>

            {/* Terminal Output Body */}
            <div className="flex-1 p-4 overflow-y-auto font-mono text-xs sm:text-[13px] leading-relaxed space-y-2 select-text">
              {terminalLines.map((line) => {
                if (line.type === 'input') {
                  return (
                    <div key={line.id} className="flex items-baseline gap-2 text-teal-400">
                      <span className="text-emerald-500 font-bold select-none">captain@galleon:~$</span>
                      <span className="text-neutral-100 font-semibold">{line.content}</span>
                      <span className="text-[10px] text-neutral-600 ml-auto select-none">{line.timestamp}</span>
                    </div>
                  );
                } else if (line.type === 'system') {
                  return (
                    <div key={line.id} className="text-neutral-500 text-xs italic">
                      {line.content}
                    </div>
                  );
                } else if (line.type === 'success') {
                  return (
                    <div key={line.id} className="text-emerald-400 whitespace-pre-wrap pl-2 border-l-2 border-emerald-500/40">
                      {line.content}
                    </div>
                  );
                } else if (line.type === 'error') {
                  return (
                    <div key={line.id} className="text-rose-400 whitespace-pre-wrap pl-2 border-l-2 border-rose-500/40">
                      {line.content}
                    </div>
                  );
                }
                return (
                  <div key={line.id} className="text-neutral-300 whitespace-pre-wrap pl-2">
                    {line.content}
                  </div>
                );
              })}
              <div ref={terminalBottomRef} />
            </div>

            {/* Terminal Prompt Input Footer */}
            <form
              onSubmit={(e) => {
                e.preventDefault();
                handleRunCommand();
              }}
              className="p-3 border-t border-neutral-800 bg-[#121418] flex items-center gap-2"
            >
              <div className="flex items-center gap-1.5 text-emerald-400 font-mono text-xs sm:text-sm font-bold select-none shrink-0">
                <span>captain@galleon:~$</span>
              </div>
              <input
                type="text"
                value={commandInput}
                onChange={(e) => setCommandInput(e.target.value)}
                placeholder="Type command... (e.g. ps aux, git status, fleet mesh status, clear)"
                autoFocus
                className="flex-1 bg-transparent text-neutral-100 font-mono text-xs sm:text-sm focus:outline-none placeholder:text-neutral-600"
              />
              <button
                type="submit"
                disabled={!commandInput.trim()}
                className="px-3 py-1.5 rounded-lg bg-teal-600 hover:bg-teal-500 text-white font-mono text-xs font-semibold disabled:opacity-40 transition-opacity flex items-center gap-1 cursor-pointer shrink-0"
              >
                <span>Exec</span>
                <CornerDownLeft className="w-3 h-3" />
              </button>
            </form>
          </div>
        </div>
      )}

      {/* TAB 2: ACTIVE SESSIONS */}
      {activeTab === 'sessions' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 font-mono">
              Live Pseudo-Terminals &amp; Worker Shells ({sessions.length})
            </span>
            <Button
              variant="secondary"
              size="xs"
              icon={<Plus className="w-3 h-3" />}
              onClick={() => {
                const newId = `pts-${sessions.length}`;
                setSessions((prev) => [
                  ...prev,
                  {
                    id: newId,
                    tty: `pts/${sessions.length}`,
                    title: `Ad-hoc Worker Session #${sessions.length}`,
                    actor: 'Operator Subprocess',
                    pid: 4000 + sessions.length * 123,
                    cpu: '0.0%',
                    memory: '12.4 MB',
                    status: 'active',
                    uptime: 'Just now'
                  }
                ]);
              }}
            >
              Spawn PTY
            </Button>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {sessions.map((sess) => (
              <div
                key={sess.id}
                className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-3"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <div className="w-7 h-7 rounded-lg bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center font-mono font-bold text-xs">
                      {sess.tty.replace('pts/', 'P')}
                    </div>
                    <div>
                      <h4 className="text-xs font-bold text-neutral-900 dark:text-neutral-100 font-mono">
                        {sess.tty}
                      </h4>
                      <div className="text-[10px] text-neutral-400 font-mono">PID {sess.pid}</div>
                    </div>
                  </div>
                  <StatusBadge status={sess.status} />
                </div>

                <div className="space-y-1">
                  <div className="text-xs font-semibold text-neutral-800 dark:text-neutral-200">
                    {sess.title}
                  </div>
                  <div className="text-[11px] text-neutral-400">Actor: {sess.actor}</div>
                </div>

                <div className="grid grid-cols-3 gap-2 pt-2 border-t border-neutral-100 dark:border-neutral-800/80 text-[10px] font-mono text-neutral-500">
                  <div>
                    <span className="block text-neutral-400">CPU</span>
                    <span className="font-semibold text-neutral-700 dark:text-neutral-300">{sess.cpu}</span>
                  </div>
                  <div>
                    <span className="block text-neutral-400">RAM</span>
                    <span className="font-semibold text-neutral-700 dark:text-neutral-300">{sess.memory}</span>
                  </div>
                  <div>
                    <span className="block text-neutral-400">Uptime</span>
                    <span className="font-semibold text-neutral-700 dark:text-neutral-300">{sess.uptime}</span>
                  </div>
                </div>

                <div className="pt-2 flex items-center justify-end gap-2 border-t border-neutral-100 dark:border-neutral-800/80">
                  <Button
                    variant="ghost"
                    size="xs"
                    onClick={() => {
                      setActiveTab('terminal');
                      handleRunCommand(`tmux attach -t ${sess.tty}`);
                    }}
                  >
                    Attach
                  </Button>
                  <Button
                    variant="outline"
                    size="xs"
                    onClick={() => setSessionToKill(sess)}
                    className="text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/20"
                  >
                    Kill
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* TAB 3: COMMAND HISTORY */}
      {activeTab === 'history' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 font-mono">
              Audit Record of Executed Shell Invocations ({commandHistory.length})
            </span>
            <Button
              variant="ghost"
              size="xs"
              icon={<Trash2 className="w-3 h-3" />}
              onClick={() => setCommandHistory([])}
            >
              Clear History
            </Button>
          </div>

          <div className="space-y-2">
            {commandHistory.map((item, idx) => (
              <div
                key={item.id}
                className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs shadow-2xs hover:border-neutral-300 dark:hover:border-neutral-700 transition-colors"
              >
                <div className="flex items-center gap-3 min-w-0">
                  <span
                    className={`text-[10px] font-mono px-2 py-0.5 rounded font-bold shrink-0 ${
                      item.exitCode === 0
                        ? 'bg-emerald-500/10 text-emerald-500'
                        : 'bg-rose-500/10 text-rose-500'
                    }`}
                  >
                    Exit {item.exitCode}
                  </span>
                  <div className="min-w-0 space-y-0.5">
                    <code className="text-xs font-mono font-bold text-neutral-900 dark:text-neutral-100 truncate block">
                      {item.command}
                    </code>
                    <div className="text-[10px] text-neutral-400 flex items-center gap-2">
                      <span>By {item.actor}</span>
                      <span>·</span>
                      <span>{item.duration}</span>
                      <span>·</span>
                      <span>{item.timestamp}</span>
                    </div>
                  </div>
                </div>

                <div className="flex items-center gap-2 shrink-0 self-end sm:self-auto">
                  <Button
                    variant="ghost"
                    size="xs"
                    icon={copiedIndex === idx ? <Check className="w-3 h-3 text-emerald-500" /> : <Copy className="w-3 h-3" />}
                    onClick={() => handleCopyCommand(item.command, idx)}
                    title="Copy Command"
                  >
                    {copiedIndex === idx ? 'Copied' : 'Copy'}
                  </Button>
                  <Button
                    variant="secondary"
                    size="xs"
                    icon={<Play className="w-3 h-3" />}
                    onClick={() => {
                      setActiveTab('terminal');
                      handleRunCommand(item.command);
                    }}
                    title="Re-run in terminal"
                  >
                    Re-run
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* TAB 4: PROCESS MONITOR */}
      {activeTab === 'processes' && (
        <div className="space-y-4">
          {/* Host Telemetry Bar */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
            <div className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
              <span className="text-[10px] font-mono text-neutral-400 uppercase">Host Environment</span>
              <div className="text-sm font-bold font-mono text-neutral-800 dark:text-neutral-200 truncate">
                {systemMetrics?.platform ? `${systemMetrics.platform.toUpperCase()} (${systemMetrics.arch})` : 'PTY Linux / Win32'}
              </div>
              <div className="text-[10px] text-neutral-400 truncate">{systemMetrics?.nodeVersion || 'Tauri Core Host'}</div>
            </div>

            <div className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
              <span className="text-[10px] font-mono text-neutral-400 uppercase">CPU Utilization</span>
              <div className="text-sm font-bold font-mono text-emerald-500">
                {systemMetrics?.cpuUsage ? `${systemMetrics.cpuUsage}%` : '1.8%'}
              </div>
              <div className="text-[10px] text-neutral-400">Load across active cores</div>
            </div>

            <div className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
              <span className="text-[10px] font-mono text-neutral-400 uppercase">Memory Footprint</span>
              <div className="text-sm font-bold font-mono text-neutral-800 dark:text-neutral-200">
                {systemMetrics?.memoryUsage
                  ? `${systemMetrics.memoryUsage.usedMB} / ${systemMetrics.memoryUsage.totalMB} MB`
                  : '54.2 MB'}
              </div>
              <div className="text-[10px] text-neutral-400">
                {systemMetrics?.memoryUsage ? `${systemMetrics.memoryUsage.percent}% allocated` : 'Nominal'}
              </div>
            </div>

            <div className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-1">
              <span className="text-[10px] font-mono text-neutral-400 uppercase">Daemon Uptime</span>
              <div className="text-sm font-bold font-mono text-teal-500">
                {systemMetrics?.uptime || '4h 12m'}
              </div>
              <div className="text-[10px] text-neutral-400">{processes.length} daemons running</div>
            </div>
          </div>

          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 font-mono">
              Core Daemons &amp; Background Services ({processes.length})
            </span>
            <Button
              variant="outline"
              size="xs"
              icon={<RotateCcw className={`w-3 h-3 ${isRefreshingProcs ? 'animate-spin text-teal-500' : ''}`} />}
              disabled={isRefreshingProcs}
              onClick={handleRefreshProcesses}
            >
              {isRefreshingProcs ? 'Refreshing...' : 'Refresh Top'}
            </Button>
          </div>

          <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] overflow-hidden shadow-xs">
            <div className="overflow-x-auto">
              <table className="w-full text-left text-xs font-mono">
                <thead>
                  <tr className="border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/40 text-[11px] text-neutral-500 uppercase tracking-wider">
                    <th className="py-2.5 px-4">Daemon / Service</th>
                    <th className="py-2.5 px-3">PID</th>
                    <th className="py-2.5 px-3">Status</th>
                    <th className="py-2.5 px-3">CPU</th>
                    <th className="py-2.5 px-3">Memory</th>
                    <th className="py-2.5 px-3">Uptime</th>
                    <th className="py-2.5 px-4 text-right">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-neutral-100 dark:divide-neutral-800/60">
                  {processes.map((proc) => (
                    <tr key={proc.id} className="hover:bg-neutral-50/50 dark:hover:bg-neutral-900/30 transition-colors">
                      <td className="py-3 px-4">
                        <div className="font-bold text-neutral-900 dark:text-neutral-100">{proc.name}</div>
                        <div className="text-[10px] text-neutral-400 truncate max-w-xs">{proc.command}</div>
                      </td>
                      <td className="py-3 px-3 text-neutral-600 dark:text-neutral-400 font-mono">{proc.pid}</td>
                      <td className="py-3 px-3">
                        <StatusBadge status={proc.status} />
                      </td>
                      <td className="py-3 px-3 font-semibold text-neutral-700 dark:text-neutral-300">{proc.cpu}%</td>
                      <td className="py-3 px-3 text-neutral-600 dark:text-neutral-400">{proc.memoryMB} MB</td>
                      <td className="py-3 px-3 text-neutral-500">{proc.uptime}</td>
                      <td className="py-3 px-4 text-right">
                        <Button
                          variant="ghost"
                          size="xs"
                          icon={<RotateCcw className="w-3 h-3" />}
                          onClick={() => handleRestartProcess(proc.id)}
                          title="Restart daemon service"
                        >
                          Restart
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      )}

      {/* TAB 5: SYSTEM LOGS */}
      {activeTab === 'logs' && (
        <div className="space-y-3">
          {/* Logs Control Bar */}
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2.5">
            <div className="flex items-center gap-2">
              <div className="flex items-center rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-0.5 text-xs font-mono">
                {['all', 'info', 'warn', 'error', 'debug'].map((lvl) => (
                  <button
                    key={lvl}
                    type="button"
                    onClick={() => setLogFilterLevel(lvl)}
                    className={`px-2.5 py-1 rounded-md capitalize transition-colors cursor-pointer ${
                      logFilterLevel === lvl
                        ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-bold'
                        : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
                    }`}
                  >
                    {lvl}
                  </button>
                ))}
              </div>

              <SearchBar
                value={logSearchQuery}
                onChange={setLogSearchQuery}
                placeholder="Filter logs..."
                size="xs"
                className="w-36 sm:w-52"
              />
            </div>

            <div className="flex items-center gap-2 self-end sm:self-auto">
              <label className="flex items-center gap-1.5 text-xs font-mono text-neutral-500 select-none cursor-pointer">
                <input
                  type="checkbox"
                  checked={autoScrollLogs}
                  onChange={(e) => setAutoScrollLogs(e.target.checked)}
                  className="rounded border-neutral-300 dark:border-neutral-700 text-teal-600 focus:ring-teal-500 cursor-pointer"
                />
                <span>Auto-scroll</span>
              </label>

              <Button
                variant="secondary"
                size="xs"
                icon={<Download className="w-3 h-3" />}
                onClick={() => {
                  const blob = new Blob([logs.map((l) => `[${l.timestamp}] [${l.level.toUpperCase()}] [${l.source}]: ${l.message}`).join('\n')], {
                    type: 'text/plain'
                  });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement('a');
                  a.href = url;
                  a.download = `engine-room-${Date.now()}.log`;
                  a.click();
                  URL.revokeObjectURL(url);
                }}
              >
                Export
              </Button>
            </div>
          </div>

          {/* Logs Terminal Box */}
          <div className="rounded-2xl border border-neutral-800 bg-[#0c0d10] p-4 text-xs font-mono max-h-[500px] overflow-y-auto space-y-1.5 shadow-inner">
            {filteredLogs.length === 0 ? (
              <div className="text-neutral-500 py-6 text-center">No logs match the selected filter.</div>
            ) : (
              filteredLogs.map((log) => {
                const badgeColor = {
                  info: 'text-teal-400 bg-teal-500/10',
                  warn: 'text-amber-400 bg-amber-500/10',
                  error: 'text-rose-400 bg-rose-500/10',
                  debug: 'text-neutral-400 bg-neutral-800'
                }[log.level];

                return (
                  <div key={log.id} className="flex items-baseline gap-2 py-0.5 hover:bg-white/5 rounded px-1.5 transition-colors">
                    <span className="text-[10px] text-neutral-600 shrink-0 select-none">{log.timestamp}</span>
                    <span className={`text-[10px] font-bold px-1.5 py-0.2 rounded uppercase shrink-0 ${badgeColor}`}>
                      {log.level}
                    </span>
                    <span className="text-neutral-400 font-semibold shrink-0">[{log.source}]</span>
                    <span className="text-neutral-200 select-text break-all">{log.message}</span>
                  </div>
                );
              })
            )}
            <div ref={logsBottomRef} />
          </div>
        </div>
      )}

      {/* TAB 6: CONNECTIONS */}
      {activeTab === 'connections' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 font-mono">
              Local IPC Sockets, Ports &amp; Network Listeners
            </span>
            <span className="text-xs font-mono text-emerald-500 flex items-center gap-1 font-semibold">
              <ShieldCheck className="w-3.5 h-3.5" />
              Landlock LSM Enforced
            </span>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Server className="w-4 h-4 text-teal-500" />
                  <span className="font-bold text-xs font-mono text-neutral-900 dark:text-neutral-100">
                    HTTP Gateway Listener (:8080)
                  </span>
                </div>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-500 font-semibold">
                  BOUND
                </span>
              </div>
              <p className="text-xs text-neutral-500 dark:text-neutral-400">
                Primary API gateway handling client requests, Webhooks, and A2A Agent-to-Agent message routing.
              </p>
              <div className="grid grid-cols-2 gap-2 pt-2 border-t border-neutral-100 dark:border-neutral-800 text-[11px] font-mono text-neutral-500">
                <div>Endpoint: <span className="font-semibold text-neutral-800 dark:text-neutral-200">http://0.0.0.0:8080</span></div>
                <div>Protocol: <span className="font-semibold text-neutral-800 dark:text-neutral-200">HTTP/1.1 + WS</span></div>
              </div>
            </div>

            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Cpu className="w-4 h-4 text-teal-500" />
                  <span className="font-bold text-xs font-mono text-neutral-900 dark:text-neutral-100">
                    Local Ollama Proxy (:11434)
                  </span>
                </div>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-500 font-semibold">
                  CONNECTED
                </span>
              </div>
              <p className="text-xs text-neutral-500 dark:text-neutral-400">
                Local zero-cost LLM execution bridge for AST audits, offline linting, and confidential tasks.
              </p>
              <div className="grid grid-cols-2 gap-2 pt-2 border-t border-neutral-100 dark:border-neutral-800 text-[11px] font-mono text-neutral-500">
                <div>Endpoint: <span className="font-semibold text-neutral-800 dark:text-neutral-200">127.0.0.1:11434</span></div>
                <div>Model: <span className="font-semibold text-neutral-800 dark:text-neutral-200">deepseek-r1:14b</span></div>
              </div>
            </div>

            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Radio className="w-4 h-4 text-teal-500" />
                  <span className="font-bold text-xs font-mono text-neutral-900 dark:text-neutral-100">
                    mDNS Multicast Discovery (:5353)
                  </span>
                </div>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-500 font-semibold">
                  BROADCASTING
                </span>
              </div>
              <p className="text-xs text-neutral-500 dark:text-neutral-400">
                ZeroConf A2A local mesh peer discovery broadcasting beacon packets on UDP port 5353.
              </p>
              <div className="grid grid-cols-2 gap-2 pt-2 border-t border-neutral-100 dark:border-neutral-800 text-[11px] font-mono text-neutral-500">
                <div>Peers: <span className="font-semibold text-neutral-800 dark:text-neutral-200">2 Verified</span></div>
                <div>Domain: <span className="font-semibold text-neutral-800 dark:text-neutral-200">.sovereign.local</span></div>
              </div>
            </div>

            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <HardDrive className="w-4 h-4 text-teal-500" />
                  <span className="font-bold text-xs font-mono text-neutral-900 dark:text-neutral-100">
                    UNIX Domain IPC Socket
                  </span>
                </div>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-500 font-semibold">
                  ACTIVE
                </span>
              </div>
              <p className="text-xs text-neutral-500 dark:text-neutral-400">
                High-speed local inter-process communication pipe connecting Tauri host to sovereign core.
              </p>
              <div className="grid grid-cols-2 gap-2 pt-2 border-t border-neutral-100 dark:border-neutral-800 text-[11px] font-mono text-neutral-500">
                <div>Path: <span className="font-semibold text-neutral-800 dark:text-neutral-200">/run/galleon.sock</span></div>
                <div>Latency: <span className="font-semibold text-neutral-800 dark:text-neutral-200">&lt; 0.2ms</span></div>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Confirmation Modal for Terminating Session */}
      <ConfirmationModal
        isOpen={Boolean(sessionToKill)}
        onClose={() => setSessionToKill(null)}
        onConfirm={() => {
          if (sessionToKill) {
            setSessions((prev) => prev.filter((s) => s.id !== sessionToKill.id));
            setLogs((prev) => [
              ...prev,
              {
                id: `log-${Date.now()}`,
                level: 'warn',
                source: sessionToKill.tty,
                message: `Session ${sessionToKill.tty} (PID ${sessionToKill.pid}) terminated by Captain.`,
                timestamp: new Date().toTimeString().slice(0, 8)
              }
            ]);
            setSessionToKill(null);
          }
        }}
        title="Terminate Worker Session"
        message={
          sessionToKill ? (
            <span>
              Are you sure you want to terminate pseudo-terminal{' '}
              <strong className="font-mono text-neutral-800 dark:text-neutral-200">
                {sessionToKill.tty}
              </strong>{' '}
              (PID {sessionToKill.pid})? Unsaved buffered output will be lost.
            </span>
          ) : (
            ''
          )
        }
        confirmLabel="Terminate Session"
        cancelLabel="Keep Running"
        variant="danger"
      />
    </div>
  );
};

import React, { useState } from 'react';
import {
  Monitor,
  Laptop,
  Check,
  Copy,
  Terminal,
  Layers,
  Sparkles,
  ExternalLink,
  Sliders,
  Cpu,
  Shield,
  FileCode,
  Download,
  X,
  Minus,
  Square,
  Maximize2,
  RefreshCw,
  Eye
} from 'lucide-react';
import { Modal } from './Modal';
import { Button } from './Button';
import { ToolButton } from './ToolButton';
import {
  TAURI_V2_CONFIG_JSON,
  TAURI_V2_CARGO_TOML,
  TAURI_V2_MAIN_RS,
  TAURI_V2_CAPABILITIES_JSON
} from '../../utils/tauriBridge';

export interface TauriDesktopModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const TauriDesktopModal: React.FC<TauriDesktopModalProps> = ({ isOpen, onClose }) => {
  const [activeTab, setActiveTab] = useState<'simulator' | 'scaffolding' | 'ipc'>('simulator');
  const [simulatedOS, setSimulatedOS] = useState<'macos' | 'windows' | 'linux'>('macos');
  const [windowPreset, setWindowPreset] = useState<'1440x900' | '1280x800' | '1024x768'>('1280x800');
  const [isFrameless, setIsFrameless] = useState(false);
  const [copiedFile, setCopiedFile] = useState<string | null>(null);
  const [ipcLog, setIpcLog] = useState<string[]>([
    'Tauri v2 IPC Runtime initialized (simulation bridge active)',
    'Registered handlers: [get_fleet_metrics, ring_deck_bell, open_quarterdeck]'
  ]);
  const [isTrayMenuOpen, setIsTrayMenuOpen] = useState(false);

  const handleCopy = (filename: string, content: string) => {
    navigator.clipboard.writeText(content);
    setCopiedFile(filename);
    setTimeout(() => setCopiedFile(null), 2500);
  };

  const handleSimulateIPC = (command: string) => {
    if (command === 'get_fleet_metrics') {
      setIpcLog((prev) => [
        ...prev,
        `> invoke("get_fleet_metrics")`,
        `<= { active_ships: 3, assigned_crew: 8, running_voyages: 2, status: "Sovereign & Anchored" }`
      ]);
    } else if (command === 'ring_deck_bell') {
      setIpcLog((prev) => [
        ...prev,
        `> invoke("ring_deck_bell")`,
        `<= [Rust std::println] Ship Bell chimed by Sovereign Captain`
      ]);
    }
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="Tauri v2 Desktop App Simulator & Scaffolding"
      maxWidth="4xl"
    >
      <div className="space-y-5">
        {/* Navigation Tabs */}
        <div className="flex items-center gap-1.5 p-1 rounded-xl bg-neutral-100 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 text-xs">
          <button
            type="button"
            onClick={() => setActiveTab('simulator')}
            className={`px-3 py-1.5 rounded-lg font-semibold transition-all cursor-pointer flex items-center gap-1.5 ${
              activeTab === 'simulator'
                ? 'bg-white dark:bg-[#191b1f] text-teal-600 dark:text-teal-400 shadow-xs'
                : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
            }`}
          >
            <Monitor className="w-3.5 h-3.5" />
            <span>Interactive Desktop Simulator</span>
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('scaffolding')}
            className={`px-3 py-1.5 rounded-lg font-semibold transition-all cursor-pointer flex items-center gap-1.5 ${
              activeTab === 'scaffolding'
                ? 'bg-white dark:bg-[#191b1f] text-teal-600 dark:text-teal-400 shadow-xs'
                : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
            }`}
          >
            <FileCode className="w-3.5 h-3.5" />
            <span>Tauri v2 Scaffolding &amp; Config</span>
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('ipc')}
            className={`px-3 py-1.5 rounded-lg font-semibold transition-all cursor-pointer flex items-center gap-1.5 ${
              activeTab === 'ipc'
                ? 'bg-white dark:bg-[#191b1f] text-teal-600 dark:text-teal-400 shadow-xs'
                : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
            }`}
          >
            <Terminal className="w-3.5 h-3.5" />
            <span>Native Rust IPC Inspector</span>
          </button>
        </div>

        {/* TAB 1: DESKTOP SIMULATOR */}
        {activeTab === 'simulator' && (
          <div className="space-y-4">
            {/* Control Bar for Simulator */}
            <div className="flex flex-wrap items-center justify-between gap-3 p-3 rounded-xl bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 text-xs">
              <div className="flex items-center gap-2">
                <span className="text-neutral-400 font-medium">Platform OS:</span>
                <div className="flex items-center gap-1 bg-white dark:bg-neutral-950 p-0.5 rounded-lg border border-neutral-200 dark:border-neutral-800">
                  <button
                    type="button"
                    onClick={() => setSimulatedOS('macos')}
                    className={`px-2 py-1 rounded text-xs font-semibold cursor-pointer ${
                      simulatedOS === 'macos' ? 'bg-teal-500 text-neutral-950' : 'text-neutral-400 hover:text-white'
                    }`}
                  >
                    macOS (Sequoia)
                  </button>
                  <button
                    type="button"
                    onClick={() => setSimulatedOS('windows')}
                    className={`px-2 py-1 rounded text-xs font-semibold cursor-pointer ${
                      simulatedOS === 'windows' ? 'bg-teal-500 text-neutral-950' : 'text-neutral-400 hover:text-white'
                    }`}
                  >
                    Windows 11
                  </button>
                  <button
                    type="button"
                    onClick={() => setSimulatedOS('linux')}
                    className={`px-2 py-1 rounded text-xs font-semibold cursor-pointer ${
                      simulatedOS === 'linux' ? 'bg-teal-500 text-neutral-950' : 'text-neutral-400 hover:text-white'
                    }`}
                  >
                    Linux (GTK4)
                  </button>
                </div>
              </div>

              <div className="flex items-center gap-2">
                <span className="text-neutral-400 font-medium">Window Mode:</span>
                <button
                  type="button"
                  onClick={() => setIsFrameless(!isFrameless)}
                  className={`px-2.5 py-1 rounded-lg border text-xs font-medium cursor-pointer transition-colors ${
                    isFrameless
                      ? 'bg-teal-500/10 border-teal-500 text-teal-400 font-semibold'
                      : 'border-neutral-200 dark:border-neutral-800 text-neutral-400'
                  }`}
                >
                  {isFrameless ? 'Frameless (Modern Custom Chrome)' : 'Native Window Chrome'}
                </button>
              </div>

              <div className="flex items-center gap-1.5 text-neutral-400">
                <Laptop className="w-3.5 h-3.5 text-teal-500" />
                <span className="font-mono text-[11px]">{windowPreset}</span>
              </div>
            </div>

            {/* Simulated Desktop Window Frame */}
            <div className="rounded-2xl border border-neutral-700/80 shadow-2xl overflow-hidden bg-neutral-950 transition-all flex flex-col h-[420px] relative">
              {/* Native Window Titlebar */}
              {!isFrameless && (
                <div
                  className={`h-9 px-3 flex items-center justify-between border-b select-none shrink-0 ${
                    simulatedOS === 'macos'
                      ? 'bg-neutral-900/90 border-neutral-800'
                      : 'bg-[#181a1e] border-neutral-800'
                  }`}
                >
                  {/* macOS Traffic Lights (Left) */}
                  {simulatedOS === 'macos' && (
                    <div className="flex items-center gap-2 w-16">
                      <div className="w-3 h-3 rounded-full bg-[#ff5f57] border border-[#e0443e] cursor-pointer hover:opacity-80" />
                      <div className="w-3 h-3 rounded-full bg-[#febc2e] border border-[#d89e24] cursor-pointer hover:opacity-80" />
                      <div className="w-3 h-3 rounded-full bg-[#28c840] border border-[#1aab29] cursor-pointer hover:opacity-80" />
                    </div>
                  )}

                  {/* Window Title (Centered for macOS, Left for Windows) */}
                  <div className={`flex items-center gap-1.5 text-xs font-medium text-neutral-300 ${simulatedOS === 'macos' ? 'mx-auto' : 'ml-2'}`}>
                    <span className="w-2 h-2 rounded-full bg-teal-400 animate-pulse" />
                    <span className="font-bold">Galleon Sovereign</span>
                    <span className="text-[10px] text-neutral-500 font-mono">v2.4.0 (Tauri v2)</span>
                  </div>

                  {/* Windows / Linux Window Controls (Right) */}
                  {simulatedOS !== 'macos' && (
                    <div className="flex items-center gap-0.5 ml-auto">
                      <button type="button" className="p-1.5 text-neutral-400 hover:bg-neutral-800 rounded">
                        <Minus className="w-3 h-3" />
                      </button>
                      <button type="button" className="p-1.5 text-neutral-400 hover:bg-neutral-800 rounded">
                        <Square className="w-2.5 h-2.5" />
                      </button>
                      <button type="button" className="p-1.5 text-neutral-400 hover:bg-rose-600 hover:text-white rounded">
                        <X className="w-3 h-3" />
                      </button>
                    </div>
                  )}

                  {simulatedOS === 'macos' && <div className="w-16" />}
                </div>
              )}

              {/* Native App Menu Bar (File, Edit, View, Fleet, Tools) */}
              <div className="h-6 px-3 bg-neutral-900/60 border-b border-neutral-800/80 flex items-center gap-4 text-[11px] text-neutral-400 select-none shrink-0 font-medium">
                <span className="text-neutral-200 font-semibold hover:text-white cursor-pointer">Galleon</span>
                <span className="hover:text-white cursor-pointer">File</span>
                <span className="hover:text-white cursor-pointer">Edit</span>
                <span className="hover:text-white cursor-pointer">Fleet</span>
                <span className="hover:text-white cursor-pointer">Window</span>
                <span className="hover:text-white cursor-pointer">Help</span>
                <span className="ml-auto font-mono text-[10px] text-teal-400">Tauri v2 Core: Running</span>
              </div>

              {/* Window Content Simulation Viewport */}
              <div className="flex-1 bg-neutral-950 p-4 overflow-y-auto space-y-4">
                <div className="flex items-center justify-between p-3 rounded-xl bg-neutral-900 border border-neutral-800">
                  <div className="flex items-center gap-3">
                    <div className="w-8 h-8 rounded-lg bg-teal-500/20 text-teal-400 flex items-center justify-center font-bold text-xs">
                      ⚓
                    </div>
                    <div>
                      <h4 className="text-xs font-bold text-neutral-100">Native Tauri v2 Webview Loaded</h4>
                      <p className="text-[11px] text-neutral-400">Local-first SQLite storage, zero-latency Rust IPC, native notifications enabled</p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      size="xs"
                      variant="primary"
                      onClick={() => handleSimulateIPC('get_fleet_metrics')}
                    >
                      Test Rust IPC
                    </Button>
                    <Button
                      size="xs"
                      variant="secondary"
                      onClick={() => handleSimulateIPC('ring_deck_bell')}
                    >
                      Ring Bell
                    </Button>
                  </div>
                </div>

                {/* Simulated Desktop System Tray Bar */}
                <div className="p-3 rounded-xl bg-neutral-900/40 border border-neutral-800/80 flex items-center justify-between text-xs">
                  <div className="flex items-center gap-2">
                    <Cpu className="w-3.5 h-3.5 text-teal-400" />
                    <span className="text-neutral-300 font-mono text-[11px]">Desktop Resources:</span>
                    <span className="text-emerald-400 font-mono text-[11px]">RAM: 48 MB &bull; Rust Threads: 4</span>
                  </div>

                  {/* System Tray Icon Dropdown Simulator */}
                  <div className="relative">
                    <button
                      type="button"
                      onClick={() => setIsTrayMenuOpen(!isTrayMenuOpen)}
                      className="flex items-center gap-1.5 px-2 py-1 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-200 text-[11px] font-mono cursor-pointer"
                    >
                      <span>⚓</span>
                      <span>System Tray Menu</span>
                    </button>

                    {isTrayMenuOpen && (
                      <div className="absolute right-0 bottom-full mb-2 w-48 rounded-xl bg-neutral-900 border border-neutral-700 shadow-2xl p-1 text-xs z-50 animate-in fade-in">
                        <div className="px-2.5 py-1.5 font-bold text-neutral-200 border-b border-neutral-800">
                          Galleon Sovereign
                        </div>
                        <button type="button" className="w-full text-left px-2.5 py-1.5 text-neutral-300 hover:bg-neutral-800 rounded">
                          Open Quarterdeck
                        </button>
                        <button type="button" className="w-full text-left px-2.5 py-1.5 text-neutral-300 hover:bg-neutral-800 rounded">
                          Triage Fleet Status
                        </button>
                        <div className="h-px bg-neutral-800 my-1" />
                        <button type="button" className="w-full text-left px-2.5 py-1.5 text-rose-400 hover:bg-rose-500/10 rounded">
                          Quit Application
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* TAB 2: SCAFFOLDING & CONFIG CODE */}
        {activeTab === 'scaffolding' && (
          <div className="space-y-4">
            <p className="text-xs text-neutral-400 leading-relaxed">
              Semua file konfigurasi di bawah ini telah disesuaikan khusus untuk <strong>Tauri v2</strong> (menggunakan skema konfigurasi baru <code className="text-teal-400">tauri.app/config/2</code>, Tauri v2 plugins, dan sistem perizinan granular <code className="text-teal-400">capabilities/default.json</code>).
            </p>

            <div className="space-y-3">
              {/* 1. tauri.conf.json */}
              <div className="rounded-xl border border-neutral-800 bg-neutral-950 overflow-hidden">
                <div className="px-3.5 py-2 bg-neutral-900 border-b border-neutral-800 flex items-center justify-between text-xs">
                  <div className="flex items-center gap-2">
                    <FileCode className="w-3.5 h-3.5 text-teal-400" />
                    <span className="font-mono text-neutral-200 font-bold">src-tauri/tauri.conf.json</span>
                  </div>
                  <ToolButton
                    onClick={() => handleCopy('tauri.conf.json', TAURI_V2_CONFIG_JSON)}
                    icon={copiedFile === 'tauri.conf.json' ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                    label={copiedFile === 'tauri.conf.json' ? 'Copied' : 'Copy JSON'}
                    size="xs"
                  />
                </div>
                <pre className="p-3 text-[11px] font-mono text-neutral-300 overflow-x-auto max-h-48 leading-relaxed">
                  {TAURI_V2_CONFIG_JSON}
                </pre>
              </div>

              {/* 2. Cargo.toml */}
              <div className="rounded-xl border border-neutral-800 bg-neutral-950 overflow-hidden">
                <div className="px-3.5 py-2 bg-neutral-900 border-b border-neutral-800 flex items-center justify-between text-xs">
                  <div className="flex items-center gap-2">
                    <FileCode className="w-3.5 h-3.5 text-teal-400" />
                    <span className="font-mono text-neutral-200 font-bold">src-tauri/Cargo.toml</span>
                  </div>
                  <ToolButton
                    onClick={() => handleCopy('Cargo.toml', TAURI_V2_CARGO_TOML)}
                    icon={copiedFile === 'Cargo.toml' ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                    label={copiedFile === 'Cargo.toml' ? 'Copied' : 'Copy TOML'}
                    size="xs"
                  />
                </div>
                <pre className="p-3 text-[11px] font-mono text-neutral-300 overflow-x-auto max-h-40 leading-relaxed">
                  {TAURI_V2_CARGO_TOML}
                </pre>
              </div>

              {/* 3. src-tauri/src/main.rs */}
              <div className="rounded-xl border border-neutral-800 bg-neutral-950 overflow-hidden">
                <div className="px-3.5 py-2 bg-neutral-900 border-b border-neutral-800 flex items-center justify-between text-xs">
                  <div className="flex items-center gap-2">
                    <FileCode className="w-3.5 h-3.5 text-teal-400" />
                    <span className="font-mono text-neutral-200 font-bold">src-tauri/src/main.rs (Rust Entrypoint &amp; IPC)</span>
                  </div>
                  <ToolButton
                    onClick={() => handleCopy('main.rs', TAURI_V2_MAIN_RS)}
                    icon={copiedFile === 'main.rs' ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                    label={copiedFile === 'main.rs' ? 'Copied' : 'Copy Rust Code'}
                    size="xs"
                  />
                </div>
                <pre className="p-3 text-[11px] font-mono text-neutral-300 overflow-x-auto max-h-40 leading-relaxed">
                  {TAURI_V2_MAIN_RS}
                </pre>
              </div>

              {/* 4. capabilities/default.json */}
              <div className="rounded-xl border border-neutral-800 bg-neutral-950 overflow-hidden">
                <div className="px-3.5 py-2 bg-neutral-900 border-b border-neutral-800 flex items-center justify-between text-xs">
                  <div className="flex items-center gap-2">
                    <Shield className="w-3.5 h-3.5 text-teal-400" />
                    <span className="font-mono text-neutral-200 font-bold">src-tauri/capabilities/default.json (Security v2)</span>
                  </div>
                  <ToolButton
                    onClick={() => handleCopy('default.json', TAURI_V2_CAPABILITIES_JSON)}
                    icon={copiedFile === 'default.json' ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                    label={copiedFile === 'default.json' ? 'Copied' : 'Copy JSON'}
                    size="xs"
                  />
                </div>
                <pre className="p-3 text-[11px] font-mono text-neutral-300 overflow-x-auto max-h-36 leading-relaxed">
                  {TAURI_V2_CAPABILITIES_JSON}
                </pre>
              </div>
            </div>

            {/* Quick Terminal Guide */}
            <div className="p-3.5 rounded-xl bg-teal-500/10 border border-teal-500/30 text-xs space-y-2">
              <span className="font-bold text-teal-300 flex items-center gap-1.5">
                <Terminal className="w-3.5 h-3.5" />
                Cara Menjalankan di Terminal Desktop Anda:
              </span>
              <div className="p-2.5 rounded-lg bg-neutral-950 font-mono text-[11px] text-teal-200 space-y-1">
                <div># 1. Install Tauri CLI v2</div>
                <div>npm install -D @tauri-apps/cli@next @tauri-apps/api@next</div>
                <div className="pt-1"># 2. Jalankan Mode Desktop Development</div>
                <div>npx tauri dev</div>
                <div className="pt-1"># 3. Build Binary Installer (.dmg / .exe / .deb / .AppImage)</div>
                <div>npx tauri build</div>
              </div>
            </div>
          </div>
        )}

        {/* TAB 3: NATIVE RUST IPC INSPECTOR */}
        {activeTab === 'ipc' && (
          <div className="space-y-4">
            <p className="text-xs text-neutral-400">
              Uji coba respons IPC (Inter-Process Communication) native Rust yang menjembatani antarmuka React dengan sistem operasi desktop melalui Tauri v2.
            </p>

            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="primary"
                onClick={() => handleSimulateIPC('get_fleet_metrics')}
              >
                invoke("get_fleet_metrics")
              </Button>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => handleSimulateIPC('ring_deck_bell')}
              >
                invoke("ring_deck_bell")
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setIpcLog(['Logs cleared.'])}
              >
                Clear Log
              </Button>
            </div>

            <div className="rounded-xl border border-neutral-800 bg-neutral-950 p-3 font-mono text-[11px] text-emerald-400 h-64 overflow-y-auto space-y-1.5 leading-relaxed">
              {ipcLog.map((line, idx) => (
                <div key={idx} className={line.startsWith('>') ? 'text-amber-400 font-bold' : line.startsWith('<=') ? 'text-teal-300' : 'text-neutral-400'}>
                  {line}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
};

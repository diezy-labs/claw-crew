import React, { useState, useEffect, useRef } from 'react';
import QRCode from 'qrcode';
import {
  Wifi,
  Smartphone,
  Laptop,
  Server,
  QrCode,
  Copy,
  Check,
  ExternalLink,
  ShieldCheck,
  RefreshCw,
  Terminal,
  Activity,
  Layers,
  ChevronRight,
  Info
} from 'lucide-react';
import { PWAInstallButton } from '../common/PWAInstallButton';

interface NetworkInterface {
  name: string;
  address: string;
  url: string;
}

interface NetworkInfo {
  hostname: string;
  port: number;
  host: string;
  interfaces: NetworkInterface[];
  preferredLanUrl: string;
  isProduction: boolean;
  platform: string;
  memoryUsageMB: number;
}

interface RemoteAccessModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const RemoteAccessModal: React.FC<RemoteAccessModalProps> = ({
  isOpen,
  onClose
}) => {
  const [networkInfo, setNetworkInfo] = useState<NetworkInfo | null>(null);
  const [selectedUrl, setSelectedUrl] = useState<string>('');
  const [customUrl, setCustomUrl] = useState<string>('');
  const [qrCodeDataUrl, setQrCodeDataUrl] = useState<string>('');
  const [copied, setCopied] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [activeGuideTab, setActiveGuideTab] = useState<'mobile' | 'lan' | 'nginx' | 'docker'>('mobile');
  const [ollamaStatus, setOllamaStatus] = useState<{ available: boolean; host: string; models?: any[] } | null>(null);
  const modalCardRef = useRef<HTMLDivElement>(null);
  const handleTestOllama = async () => {
    try {
      const res = await fetch('/api/providers/ollama/generate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model: ollamaStatus?.models?.[0]?.name || 'llama3', prompt: 'Hello', stream: false })
      });
      if (res.ok) {
        const data = await res.json();
        alert('Ollama Response: ' + data.response);
      } else {
        alert('Error: ' + res.statusText);
      }
    } catch (e) {
      alert('Error testing Ollama: ' + e);
    }
  };

  // Close on Escape key press
  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose]);

  // Fetch server network info and local Ollama status
  const fetchNetworkData = async () => {
    setIsLoading(true);
    try {
      // 1. Fetch server LAN network interfaces
      const netRes = await fetch('/api/system/network');
      if (netRes.ok) {
        const data: NetworkInfo = await netRes.json();
        setNetworkInfo(data);
        // Default to current browser origin if on LAN, else preferred LAN URL
        const currentOrigin = window.location.origin;
        if (currentOrigin.includes('localhost') && data.preferredLanUrl) {
          setSelectedUrl(data.preferredLanUrl);
        } else {
          setSelectedUrl(currentOrigin);
        }
      } else {
        // Fallback to current browser URL
        setSelectedUrl(window.location.origin);
      }

      // 2. Fetch Ollama status
      const olRes = await fetch('/api/providers/ollama/status');
      if (olRes.ok) {
        const olData = await olRes.json();
        setOllamaStatus(olData);
      }
    } catch (e) {
      console.warn('Network discovery fallback:', e);
      setSelectedUrl(window.location.origin);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (isOpen) {
      fetchNetworkData();
    }
  }, [isOpen]);

  // Generate QR Code when selected URL changes
  useEffect(() => {
    const urlToEncode = customUrl.trim() || selectedUrl || window.location.origin;
    if (!urlToEncode) return;

    QRCode.toDataURL(urlToEncode, {
      width: 240,
      margin: 1.5,
      color: {
        dark: '#0f766e',
        light: '#ffffff'
      }
    })
      .then((dataUrl) => setQrCodeDataUrl(dataUrl))
      .catch((err) => console.error('Failed to generate QR code', err));
  }, [selectedUrl, customUrl]);

  if (!isOpen) return null;

  const currentActiveUrl = customUrl.trim() || selectedUrl || window.location.origin;

  const handleCopy = () => {
    navigator.clipboard.writeText(currentActiveUrl);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div
      onClick={onClose}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4 animate-fade-in overflow-y-auto cursor-pointer"
    >
      <div
        ref={modalCardRef}
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-3xl rounded-2xl bg-neutral-900 border border-neutral-800 shadow-2xl overflow-hidden my-auto max-h-[92vh] flex flex-col cursor-default"
      >
        {/* Header */}
        <div className="px-5 py-4 border-b border-neutral-800 flex items-center justify-between bg-neutral-950/70 shrink-0">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-teal-500/20 border border-teal-500/30 flex items-center justify-center text-teal-400">
              <Wifi className="w-5 h-5 animate-pulse" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-bold text-neutral-100">
                  Fleet AI Remote Access &amp; Multi-Platform Hub
                </h2>
                <span className="text-[10px] font-mono uppercase px-2 py-0.5 rounded-full bg-teal-500/15 text-teal-400 border border-teal-500/30 font-semibold">
                  Host Anywhere
                </span>
              </div>
              <p className="text-xs text-neutral-400">
                Run locally on CLI / Nginx / Cloud and access from any mobile, tablet, or laptop.
              </p>
            </div>
          </div>
          <div className="text-[11px] font-mono text-neutral-400 bg-neutral-900 border border-neutral-800/80 px-2.5 py-1 rounded-lg select-none">
            Click outside to close
          </div>
        </div>

        {/* Content Body */}
        <div className="p-5 overflow-y-auto space-y-6 text-neutral-300">
          {/* Top Info Banner: Server Daemon Health */}
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <div className="p-3 rounded-xl bg-neutral-950/80 border border-neutral-800 flex items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-emerald-500/15 text-emerald-400 flex items-center justify-center shrink-0">
                <Server className="w-4 h-4" />
              </div>
              <div className="min-w-0">
                <div className="text-[10px] font-mono uppercase text-neutral-500">Daemon Status</div>
                <div className="text-xs font-bold text-neutral-200 flex items-center gap-1.5 truncate">
                  <span className="w-2 h-2 rounded-full bg-emerald-400 animate-ping shrink-0" />
                  Active (0.0.0.0:{networkInfo?.port || 3000})
                </div>
              </div>
            </div>

            <div className="p-3 rounded-xl bg-neutral-950/80 border border-neutral-800 flex items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-teal-500/15 text-teal-400 flex items-center justify-center shrink-0">
                <Activity className="w-4 h-4" />
              </div>
              <div className="min-w-0">
                <div className="text-[10px] font-mono uppercase text-neutral-500">Host System</div>
                <div className="text-xs font-semibold text-neutral-200 truncate">
                  {networkInfo?.hostname || 'Local Machine'} ({networkInfo?.platform || 'linux'})
                </div>
              </div>
            </div>

            <div className="p-3 rounded-xl bg-neutral-950/80 border border-neutral-800 flex items-center gap-3">
              <div className={`w-8 h-8 rounded-lg ${ollamaStatus?.available ? 'bg-teal-500/15 text-teal-400' : 'bg-neutral-800 text-neutral-500'} flex items-center justify-center shrink-0`}>
                <Layers className="w-4 h-4" />
              </div>
              <div className="min-w-0">
                <div className="text-[10px] font-mono uppercase text-neutral-500">Ollama Local Bridge</div>
                <div className="text-xs font-semibold truncate text-neutral-200">
                  {ollamaStatus?.available ? 'Connected (Port 11434)' : 'Standalone / Cloud Mode'}
                </div>
              </div>
            </div>
          </div>

          {/* QR Code & Pairing Section */}
          <div className="grid grid-cols-1 md:grid-cols-12 gap-5 p-4 rounded-2xl bg-neutral-950 border border-neutral-800/90 items-center">
            {/* Left: Dynamic QR Code */}
            <div className="md:col-span-5 flex flex-col items-center justify-center p-3 bg-neutral-900/90 rounded-xl border border-neutral-800">
              <div className="text-[11px] font-mono text-teal-400 mb-2 flex items-center gap-1.5 font-medium">
                <QrCode className="w-3.5 h-3.5" />
                Scan to Open on Mobile / Tablet
              </div>

              {qrCodeDataUrl ? (
                <div className="p-2.5 bg-white rounded-xl shadow-lg border border-teal-500/20">
                  <img
                    src={qrCodeDataUrl}
                    alt="Fleet AI Access QR Code"
                    className="w-44 h-44 object-contain rounded-md"
                  />
                </div>
              ) : (
                <div className="w-44 h-44 flex items-center justify-center text-xs text-neutral-500">
                  Generating QR...
                </div>
              )}

              <p className="text-[10px] text-neutral-400 text-center mt-2.5 max-w-[200px]">
                Point your phone camera to instantly launch Fleet AI in mobile Safari or Chrome.
              </p>
            </div>

            {/* Right: URL Selector & Copy */}
            <div className="md:col-span-7 space-y-4">
              <div className="space-y-1.5">
                <label className="text-xs font-semibold text-neutral-200 flex items-center justify-between">
                  <span>Connect URL for Other Devices:</span>
                  <button
                    onClick={fetchNetworkData}
                    disabled={isLoading}
                    className="text-[10px] text-teal-400 hover:text-teal-300 flex items-center gap-1 cursor-pointer"
                  >
                    <RefreshCw className={`w-3 h-3 ${isLoading ? 'animate-spin' : ''}`} />
                    Refresh IPs
                  </button>
                </label>

                {/* Primary URL Card */}
                <div className="flex items-center gap-2 p-2 rounded-xl bg-neutral-900 border border-teal-500/30 ring-1 ring-teal-500/20">
                  <input
                    type="text"
                    readOnly
                    value={currentActiveUrl}
                    className="flex-1 bg-transparent px-2 font-mono text-xs text-teal-300 focus:outline-hidden select-all"
                  />
                  <button
                    onClick={handleCopy}
                    className="px-3 py-1.5 rounded-lg bg-teal-600 hover:bg-teal-500 text-white text-xs font-semibold flex items-center gap-1.5 transition-colors cursor-pointer shrink-0"
                  >
                    {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
                    <span>{copied ? 'Copied!' : 'Copy'}</span>
                  </button>
                  <a
                    href={currentActiveUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="p-1.5 rounded-lg hover:bg-neutral-800 text-neutral-400 hover:text-neutral-200 transition-colors"
                    title="Open in new window"
                  >
                    <ExternalLink className="w-4 h-4" />
                  </a>
                </div>
              </div>

              {/* Interface Picker (LAN vs Localhost vs Public) */}
              {networkInfo && networkInfo.interfaces.length > 0 && (
                <div className="space-y-1.5">
                  <div className="text-[11px] font-mono text-neutral-400 uppercase">Available Network Interfaces:</div>
                  <div className="flex flex-wrap gap-1.5">
                    {networkInfo.interfaces.map((iface) => (
                      <button
                        key={iface.address}
                        onClick={() => {
                          setSelectedUrl(iface.url);
                          setCustomUrl('');
                        }}
                        className={`px-2.5 py-1 rounded-lg text-xs font-mono transition-all cursor-pointer ${
                          selectedUrl === iface.url && !customUrl
                            ? 'bg-teal-500/20 text-teal-300 border border-teal-500/40 font-bold'
                            : 'bg-neutral-900 text-neutral-400 hover:bg-neutral-800 border border-neutral-800'
                        }`}
                      >
                        {iface.address} ({iface.name})
                      </button>
                    ))}
                    <button
                      onClick={() => {
                        setSelectedUrl(window.location.origin);
                        setCustomUrl('');
                      }}
                      className={`px-2.5 py-1 rounded-lg text-xs font-mono transition-all cursor-pointer ${
                        selectedUrl === window.location.origin && !customUrl
                          ? 'bg-teal-500/20 text-teal-300 border border-teal-500/40 font-bold'
                          : 'bg-neutral-900 text-neutral-400 hover:bg-neutral-800 border border-neutral-800'
                      }`}
                    >
                      Current Origin ({window.location.host})
                    </button>
                  </div>
                </div>
              )}

              {/* Custom Reverse Proxy Domain input */}
              <div className="space-y-1">
                <label className="text-[11px] text-neutral-400">
                  Or enter your Custom Domain / Cloudflare Tunnel / Tailscale URL:
                </label>
                <input
                  type="text"
                  placeholder="https://fleet.yourdomain.com"
                  value={customUrl}
                  onChange={(e) => setCustomUrl(e.target.value)}
                  className="w-full px-3 py-1.5 rounded-lg bg-neutral-900 border border-neutral-800 text-xs font-mono text-neutral-200 placeholder:text-neutral-600 focus:outline-hidden focus:border-teal-500/50"
                />
              </div>

              {/* In-App PWA Install Banner */}
              <div className="pt-2 border-t border-neutral-800 flex items-center justify-between">
                <div className="text-xs text-neutral-300 flex items-center gap-1.5">
                  <Smartphone className="w-3.5 h-3.5 text-teal-400" />
                  <span>PWA Standalone App Available</span>
                </div>
                <PWAInstallButton variant="full" />
              </div>
            </div>
          </div>

          {/* Multi-Platform Setup Guides Tabs */}
          <div className="space-y-3">
            <div className="flex items-center gap-1.5 border-b border-neutral-800 pb-2">
              {[
                { id: 'mobile', label: '📱 Mobile (iOS & Android)', icon: Smartphone },
                { id: 'lan', label: '💻 Other Laptop on LAN', icon: Laptop },
                { id: 'nginx', label: '🛡️ Nginx Reverse Proxy', icon: Server },
                { id: 'docker', label: '🐳 Docker & Cloud', icon: Terminal }
              ].map((tab) => {
                const isActive = activeGuideTab === tab.id;
                return (
                  <button
                    key={tab.id}
                    onClick={() => setActiveGuideTab(tab.id as any)}
                    className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all cursor-pointer flex items-center gap-1.5 ${
                      isActive
                        ? 'bg-neutral-800 text-teal-300 font-semibold shadow-2xs border border-neutral-700'
                        : 'text-neutral-400 hover:text-neutral-200 hover:bg-neutral-850'
                    }`}
                  >
                    <span>{tab.label}</span>
                  </button>
                );
              })}
            </div>

            {/* Guide Content */}
            <div className="p-4 rounded-xl bg-neutral-950 border border-neutral-800 text-xs leading-relaxed space-y-3">
              {activeGuideTab === 'mobile' && (
                <div className="space-y-2">
                  <h4 className="font-bold text-neutral-100 flex items-center gap-2">
                    <Smartphone className="w-4 h-4 text-teal-400" />
                    How to Run on iPhone, iPad, and Android Phones:
                  </h4>
                  <ol className="list-decimal list-inside space-y-1.5 text-neutral-300 pl-1">
                    <li>Pastikan smartphone Anda terhubung ke <strong>Wi-Fi yang sama</strong> dengan komputer ini.</li>
                    <li>Scan <strong>QR Code</strong> di atas menggunakan kamera HP, atau buka URL LAN di browser Safari / Chrome.</li>
                    <li>
                      <strong>Jadikan Aplikasi Mandiri (PWA)</strong>:
                      <ul className="list-disc list-inside pl-4 mt-1 text-neutral-400 space-y-0.5">
                        <li><strong>iOS (Safari)</strong>: Tekan tombol <em>Share</em> &rarr; pilih <em>Add to Home Screen</em>.</li>
                        <li><strong>Android (Chrome)</strong>: Tekan menu tiga titik &rarr; pilih <em>Install App</em> / <em>Add to Home screen</em>.</li>
                      </ul>
                    </li>
                    <li>Aplikasi akan langsung muncul di menu HP Anda dengan icon resmi, berlayar penuh tanpa URL bar, dan siap dipakai untuk voice command &amp; task approvals!</li>
                  </ol>
                </div>
              )}

              {activeGuideTab === 'lan' && (
                <div className="space-y-2">
                  <h4 className="font-bold text-neutral-100 flex items-center gap-2">
                    <Laptop className="w-4 h-4 text-teal-400" />
                    Accessing from Another Laptop or Desktop:
                  </h4>
                  <p className="text-neutral-300">
                    Anda tidak perlu meng-install apa pun di komputer kedua. Cukup buka browser (Chrome, Safari, Edge, Firefox) dan masukkan alamat:
                  </p>
                  <div className="p-2 rounded-lg bg-neutral-900 border border-neutral-800 font-mono text-teal-300 select-all">
                    {networkInfo?.preferredLanUrl || `http://<YOUR_IP>:3000`}
                  </div>
                  <p className="text-neutral-400 text-[11px]">
                    Semua state delegasi agen, Quarterdeck, Quests, dan Artifacts tersinkronisasi langsung ke host machine.
                  </p>
                </div>
              )}

              {activeGuideTab === 'nginx' && (
                <div className="space-y-2">
                  <h4 className="font-bold text-neutral-100 flex items-center gap-2">
                    <Server className="w-4 h-4 text-teal-400" />
                    Production Nginx Reverse Proxy Configuration:
                  </h4>
                  <p className="text-neutral-400 text-[11px]">
                    Letakkan konfigurasi ini di <code className="text-neutral-300 font-mono">/etc/nginx/sites-available/fleet-ai</code>:
                  </p>
                  <pre className="p-3 rounded-lg bg-neutral-900 border border-neutral-800 font-mono text-[11px] text-teal-300 overflow-x-auto whitespace-pre">
{`server {
    listen 80;
    server_name fleet.yourdomain.com;

    # Redirect to SSL (or terminate SSL with certbot)
    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}`}
                  </pre>
                </div>
              )}

              {activeGuideTab === 'docker' && (
                <div className="space-y-2">
                  <h4 className="font-bold text-neutral-100 flex items-center gap-2">
                    <Terminal className="w-4 h-4 text-teal-400" />
                    Docker &amp; CLI Background Execution:
                  </h4>
                  <p className="text-neutral-300">
                    Jalankan server daemon di latar belakang dengan 1 perintah:
                  </p>
                  <div className="p-2.5 rounded-lg bg-neutral-900 border border-neutral-800 font-mono text-teal-300 text-[11px] select-all space-y-2">
                    <div># Menggunakan Docker:</div>
                    <div className="text-neutral-100 font-bold">docker compose up -d</div>
                    <div className="mt-2 text-neutral-400"># Atau menggunakan Node / PM2:</div>
                    <div className="text-neutral-100 font-bold">npx pm2 start "npm start" --name fleet-ai</div>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Footer without close button - Dismiss via outside click */}
        <div className="px-5 py-3 border-t border-neutral-800 flex items-center justify-between bg-neutral-950/70 text-xs shrink-0 select-none">
          <div className="flex items-center gap-2 text-neutral-400">
            <ShieldCheck className="w-4 h-4 text-teal-400" />
            <span>Local-first architecture &bull; Zero telemetry &bull; BYOK model access</span>
          </div>
          <span className="text-[11px] font-mono text-neutral-500">
            Tap outside to dismiss &bull; Esc
          </span>
        </div>
      </div>
    </div>
  );
};


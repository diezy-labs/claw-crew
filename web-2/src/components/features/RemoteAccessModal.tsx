import React, { useState, useEffect } from 'react';
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
  Layers
} from 'lucide-react';
import { PWAInstallButton } from '../common/PWAInstallButton';
import { CardPopover } from '../common/CardPopover';

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

  // Fetch server network info and local Ollama status
  const fetchNetworkData = async () => {
    setIsLoading(true);
    try {
      const netRes = await fetch('/api/system/network');
      if (netRes.ok) {
        const data: NetworkInfo = await netRes.json();
        setNetworkInfo(data);
        const currentOrigin = window.location.origin;
        if (currentOrigin.includes('localhost') && data.preferredLanUrl) {
          setSelectedUrl(data.preferredLanUrl);
        } else {
          setSelectedUrl(currentOrigin);
        }
      } else {
        setSelectedUrl(window.location.origin);
      }

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

  const currentActiveUrl = customUrl.trim() || selectedUrl || window.location.origin;

  const handleCopy = () => {
    navigator.clipboard.writeText(currentActiveUrl);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <CardPopover
      isOpen={isOpen}
      onClose={onClose}
      variant="center"
      maxWidth="3xl"
      icon={<Wifi className="w-4 h-4 text-teal-500 animate-pulse" />}
      title="Remote Access & Multi-Platform"
      subtitle="Pair mobile devices, tablets, or secondary browsers on your network."
      badge={
        <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/10 text-teal-600 dark:text-teal-400 font-semibold uppercase">
          PORT {networkInfo?.port || 3000}
        </span>
      }
      footer={
        <div className="flex items-center justify-between w-full text-xs">
          <div className="flex items-center gap-1.5 text-neutral-500 font-mono text-[11px]">
            <ShieldCheck className="w-3.5 h-3.5 text-teal-500" />
            <span>Local-first &bull; Zero telemetry &bull; BYOK</span>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="px-3.5 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-xs font-medium cursor-pointer"
          >
            Close
          </button>
        </div>
      }
    >
      <div className="space-y-5 text-neutral-800 dark:text-neutral-200">
        {/* Top Info Banner: Server Daemon Health */}
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-2.5">
          <div className="p-3 rounded-xl bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-emerald-500/15 text-emerald-500 flex items-center justify-center shrink-0">
              <Server className="w-4 h-4" />
            </div>
            <div className="min-w-0">
              <div className="text-[10px] font-mono uppercase text-neutral-400">Daemon</div>
              <div className="text-xs font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5 truncate">
                <span className="w-2 h-2 rounded-full bg-emerald-500 animate-ping shrink-0" />
                Active (:{networkInfo?.port || 3000})
              </div>
            </div>
          </div>

          <div className="p-3 rounded-xl bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-teal-500/15 text-teal-600 dark:text-teal-400 flex items-center justify-center shrink-0">
              <Activity className="w-4 h-4" />
            </div>
            <div className="min-w-0">
              <div className="text-[10px] font-mono uppercase text-neutral-400">Host</div>
              <div className="text-xs font-semibold text-neutral-900 dark:text-neutral-100 truncate">
                {networkInfo?.hostname || 'Local Machine'} ({networkInfo?.platform || 'linux'})
              </div>
            </div>
          </div>

          <div className="p-3 rounded-xl bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 flex items-center gap-3">
            <div className={`w-8 h-8 rounded-lg ${ollamaStatus?.available ? 'bg-teal-500/15 text-teal-600 dark:text-teal-400' : 'bg-neutral-200 dark:bg-neutral-800 text-neutral-500'} flex items-center justify-center shrink-0`}>
              <Layers className="w-4 h-4" />
            </div>
            <div className="min-w-0">
              <div className="text-[10px] font-mono uppercase text-neutral-400">Ollama Bridge</div>
              <div className="text-xs font-semibold truncate text-neutral-900 dark:text-neutral-100">
                {ollamaStatus?.available ? 'Connected (:11434)' : 'Standalone Mode'}
              </div>
            </div>
          </div>
        </div>

        {/* QR Code & Pairing Section */}
        <div className="grid grid-cols-1 md:grid-cols-12 gap-5 p-4 rounded-xl bg-neutral-50/70 dark:bg-neutral-900/50 border border-neutral-200 dark:border-neutral-800 items-center">
          {/* Left: Dynamic QR Code */}
          <div className="md:col-span-5 flex flex-col items-center justify-center p-3.5 bg-white dark:bg-[#16181b] rounded-xl border border-neutral-200 dark:border-neutral-800 shadow-xs">
            <div className="text-[11px] font-mono text-teal-600 dark:text-teal-400 mb-2 flex items-center gap-1.5 font-medium">
              <QrCode className="w-3.5 h-3.5" />
              Scan with Mobile Camera
            </div>

            {qrCodeDataUrl ? (
              <div className="p-2.5 bg-white rounded-xl shadow-xs border border-teal-500/20">
                <img
                  src={qrCodeDataUrl}
                  alt="Fleet AI Access QR Code"
                  className="w-40 h-40 object-contain rounded-md"
                />
              </div>
            ) : (
              <div className="w-40 h-40 flex items-center justify-center text-xs text-neutral-400">
                Generating QR...
              </div>
            )}

            <p className="text-[10px] text-neutral-400 text-center mt-2 max-w-[200px]">
              Instantly opens Fleet AI in Safari or Chrome.
            </p>
          </div>

          {/* Right: URL Selector & Copy */}
          <div className="md:col-span-7 space-y-3.5">
            <div className="space-y-1.5">
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 flex items-center justify-between">
                <span>Connect URL:</span>
                <button
                  onClick={fetchNetworkData}
                  disabled={isLoading}
                  className="text-[10px] text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1 cursor-pointer"
                >
                  <RefreshCw className={`w-3 h-3 ${isLoading ? 'animate-spin' : ''}`} />
                  <span>Refresh</span>
                </button>
              </label>

              {/* Primary URL Card */}
              <div className="flex items-center gap-2 p-1.5 rounded-xl bg-white dark:bg-neutral-950 border border-teal-500/30 ring-1 ring-teal-500/20">
                <input
                  type="text"
                  readOnly
                  value={currentActiveUrl}
                  className="flex-1 bg-transparent px-2 font-mono text-xs text-teal-600 dark:text-teal-300 focus:outline-hidden select-all"
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
                  className="p-1.5 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 transition-colors"
                  title="Open URL"
                >
                  <ExternalLink className="w-4 h-4" />
                </a>
              </div>
            </div>

            {/* Interface Picker */}
            {networkInfo && networkInfo.interfaces.length > 0 && (
              <div className="space-y-1.5">
                <div className="text-[10px] font-mono text-neutral-400 uppercase">Interfaces:</div>
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
                          ? 'bg-teal-500/15 text-teal-700 dark:text-teal-300 border border-teal-500/40 font-bold'
                          : 'bg-white dark:bg-neutral-950 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 border border-neutral-200 dark:border-neutral-800'
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
                        ? 'bg-teal-500/15 text-teal-700 dark:text-teal-300 border border-teal-500/40 font-bold'
                        : 'bg-white dark:bg-neutral-950 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 border border-neutral-200 dark:border-neutral-800'
                    }`}
                  >
                    Origin ({window.location.host})
                  </button>
                </div>
              </div>
            )}

            {/* Custom Domain input */}
            <div className="space-y-1">
              <label className="text-[11px] text-neutral-500 dark:text-neutral-400">
                Custom Domain / Tunnel URL:
              </label>
              <input
                type="text"
                placeholder="https://fleet.yourdomain.com"
                value={customUrl}
                onChange={(e) => setCustomUrl(e.target.value)}
                className="w-full px-3 py-1.5 rounded-lg bg-white dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 text-xs font-mono text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-hidden focus:border-teal-500/50"
              />
            </div>

            {/* In-App PWA Install Banner */}
            <div className="pt-2 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-between">
              <div className="text-xs text-neutral-700 dark:text-neutral-300 flex items-center gap-1.5 font-medium">
                <Smartphone className="w-3.5 h-3.5 text-teal-500" />
                <span>PWA Standalone App</span>
              </div>
              <PWAInstallButton variant="full" />
            </div>
          </div>
        </div>

        {/* Multi-Platform Setup Guides Tabs */}
        <div className="space-y-3">
          <div className="flex items-center gap-1 border-b border-neutral-200 dark:border-neutral-800 pb-1.5 overflow-x-auto scrollbar-none">
            {[
              { id: 'mobile', label: 'Mobile (iOS & Android)', icon: Smartphone },
              { id: 'lan', label: 'LAN Devices', icon: Laptop },
              { id: 'nginx', label: 'Nginx Proxy', icon: Server },
              { id: 'docker', label: 'Docker & CLI', icon: Terminal }
            ].map((tab) => {
              const isActive = activeGuideTab === tab.id;
              const Icon = tab.icon;
              return (
                <button
                  key={tab.id}
                  onClick={() => setActiveGuideTab(tab.id as any)}
                  className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all cursor-pointer flex items-center gap-1.5 shrink-0 ${
                    isActive
                      ? 'bg-neutral-200 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold shadow-2xs border border-neutral-300 dark:border-neutral-700'
                      : 'text-neutral-500 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-200'
                  }`}
                >
                  <Icon className="w-3.5 h-3.5 text-teal-500" />
                  <span>{tab.label}</span>
                </button>
              );
            })}
          </div>

          {/* Guide Content */}
          <div className="p-4 rounded-xl bg-neutral-50/70 dark:bg-neutral-900/40 border border-neutral-200 dark:border-neutral-800 text-xs leading-relaxed space-y-2.5">
            {activeGuideTab === 'mobile' && (
              <div className="space-y-2">
                <h4 className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                  <Smartphone className="w-4 h-4 text-teal-500" />
                  Mobile Setup (iOS &amp; Android)
                </h4>
                <ol className="list-decimal list-inside space-y-1 text-neutral-600 dark:text-neutral-300 pl-1">
                  <li>Pastikan smartphone terhubung ke <strong>Wi-Fi yang sama</strong>.</li>
                  <li>Scan <strong>QR Code</strong> di atas menggunakan kamera smartphone.</li>
                  <li>
                    <strong>Install PWA</strong>:
                    <ul className="list-disc list-inside pl-4 mt-1 text-neutral-500 dark:text-neutral-400 space-y-0.5">
                      <li><strong>iOS (Safari)</strong>: Tekan <em>Share</em> &rarr; <em>Add to Home Screen</em>.</li>
                      <li><strong>Android (Chrome)</strong>: Tekan titik tiga &rarr; <em>Install App</em>.</li>
                    </ul>
                  </li>
                </ol>
              </div>
            )}

            {activeGuideTab === 'lan' && (
              <div className="space-y-2">
                <h4 className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                  <Laptop className="w-4 h-4 text-teal-500" />
                  Accessing from Another Computer
                </h4>
                <p className="text-neutral-600 dark:text-neutral-300">
                  Buka browser di laptop kedua dan akses URL berikut:
                </p>
                <div className="p-2 rounded-lg bg-neutral-100 dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 font-mono text-teal-600 dark:text-teal-300 select-all">
                  {networkInfo?.preferredLanUrl || `http://<IP>:3000`}
                </div>
              </div>
            )}

            {activeGuideTab === 'nginx' && (
              <div className="space-y-2">
                <h4 className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                  <Server className="w-4 h-4 text-teal-500" />
                  Nginx Reverse Proxy
                </h4>
                <pre className="p-3 rounded-lg bg-neutral-100 dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 font-mono text-[11px] text-teal-700 dark:text-teal-300 overflow-x-auto whitespace-pre">
{`server {
    listen 80;
    server_name fleet.yourdomain.com;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}`}
                </pre>
              </div>
            )}

            {activeGuideTab === 'docker' && (
              <div className="space-y-2">
                <h4 className="font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                  <Terminal className="w-4 h-4 text-teal-500" />
                  Background Execution
                </h4>
                <div className="p-2.5 rounded-lg bg-neutral-100 dark:bg-neutral-950 border border-neutral-200 dark:border-neutral-800 font-mono text-teal-700 dark:text-teal-300 text-[11px] select-all space-y-1.5">
                  <div className="text-neutral-500"># Docker:</div>
                  <div className="text-neutral-900 dark:text-neutral-100 font-bold">docker compose up -d</div>
                  <div className="text-neutral-500 pt-1"># PM2:</div>
                  <div className="text-neutral-900 dark:text-neutral-100 font-bold">npx pm2 start "npm start" --name fleet-ai</div>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </CardPopover>
  );
};

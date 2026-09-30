import React, { useState } from 'react';
import {
  Folder,
  FileText,
  Code2,
  FileCode,
  Download,
  Copy,
  Check,
  HardDrive,
  ShieldCheck,
  Terminal,
  Trash2,
  RefreshCw,
  ExternalLink
} from 'lucide-react';
import { CrewMember, Ship } from '../../types';

interface AgentWorkspaceModalProps {
  isOpen: boolean;
  onClose: () => void;
  crewMember: CrewMember | null;
  ship?: Ship;
}

export const AgentWorkspaceModal: React.FC<AgentWorkspaceModalProps> = ({
  isOpen,
  onClose,
  crewMember,
  ship
}) => {
  const [copied, setCopied] = useState(false);
  const [selectedFile, setSelectedFile] = useState<string>('scratchpad.md');

  if (!isOpen || !crewMember) return null;

  const mockFiles: Record<string, { size: string; modified: string; content: string; type: string }> = {
    'scratchpad.md': {
      size: '2.4 KB',
      modified: '12m ago',
      type: 'markdown',
      content: `# Operational Scratchpad · ${crewMember.name}
Role: ${crewMember.role}
Ship Scope: ${ship?.name || 'Assigned Ship'}
Authority: ${crewMember.authority}

## Working Discoveries
1. [Risk] Socket teardown timeout verified in \`crates/clawcrew-gateway/src/ws.rs:142\`.
2. Integration suite hung after 30 seconds due to missing explicit socket deadline.
3. Remediation proposed: Inject 5-second socket close deadline.

## Staged Deliverables
- Drafted: \`patches/socket_deadline_fix.diff\`
- Evidence: \`evidence/ci_socket_trace.log\` (6 socket handles leaked)`
    },
    'patches/socket_deadline_fix.diff': {
      size: '1.2 KB',
      modified: '24m ago',
      type: 'diff',
      content: `--- a/crates/clawcrew-gateway/src/ws.rs
+++ b/crates/clawcrew-gateway/src/ws.rs
@@ -142,6 +142,9 @@ pub async fn handle_socket_teardown(
     mut socket: WebSocket,
 ) -> Result<(), GatewayError> {
+    // Enforce explicit deadline to prevent test teardown hangs
+    tokio::time::timeout(Duration::from_secs(5), socket.close()).await
+        .map_err(|_| GatewayError::SocketTimeout)?;
     Ok(())
 }`
    },
    'evidence/ci_socket_trace.log': {
      size: '4.8 KB',
      modified: '32m ago',
      type: 'log',
      content: `[2026-09-29T11:14:02.128Z] DEBUG ws: client connection accepted: addr=127.0.0.1:54992
[2026-09-29T11:14:05.891Z] WARN  ws: connection drop without FIN handshake: socket_id=sk-8842
[2026-09-29T11:14:35.892Z] ERROR test_runner: socket teardown exceeded 30s deadline; test timed out.`
    },
    'metadata/agent_capabilities.json': {
      size: '890 B',
      modified: '1h ago',
      type: 'json',
      content: `{
  "agent_id": "${crewMember.id}",
  "name": "${crewMember.name}",
  "skills": ${JSON.stringify(crewMember.skills, null, 2)},
  "tools": ${JSON.stringify(crewMember.tools, null, 2)},
  "sandbox_jail": "/workspace/agents/${crewMember.id}/"
}`
    }
  };

  const currentFileData = mockFiles[selectedFile] || mockFiles['scratchpad.md'];

  const handleCopy = () => {
    navigator.clipboard.writeText(currentFileData.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div
      onClick={onClose}
      className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-5 bg-black/60 backdrop-blur-xs animate-in fade-in duration-150 cursor-pointer"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-4xl rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1e] shadow-2xl p-5 sm:p-6 space-y-4 max-h-[90vh] flex flex-col cursor-default text-xs"
      >
        {/* Header without X */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 border-b border-neutral-200 dark:border-neutral-800 pb-3.5">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center font-bold text-sm border border-teal-500/20">
              {crewMember.avatar || crewMember.name[0]}
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h3 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  {crewMember.name} &middot; Sandboxed Workspace
                </h3>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-emerald-500/15 text-emerald-500 font-semibold border border-emerald-500/30">
                  Landlock Jailed
                </span>
              </div>
              <div className="text-[11px] text-neutral-500 dark:text-neutral-400 font-mono mt-0.5">
                Path: /workspace/agents/{crewMember.id}/ &middot; {ship?.name || 'Vessel Workspace'}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <span className="text-[10px] font-mono text-neutral-400">
              Isolated Storage &middot; 4 Files
            </span>
          </div>
        </div>

        {/* Main Workspace Explorer Body (File Tree + Content Viewer) */}
        <div className="flex-1 grid grid-cols-1 md:grid-cols-3 gap-4 min-h-[360px] overflow-hidden">
          {/* File Tree Left Rail */}
          <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-[#131417] p-2.5 space-y-1 overflow-y-auto scrollbar-none">
            <div className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-neutral-400">
              Agent Scratchpad &amp; Output Tree
            </div>

            {Object.keys(mockFiles).map((fileName) => {
              const isSelected = selectedFile === fileName;
              return (
                <button
                  key={fileName}
                  onClick={() => setSelectedFile(fileName)}
                  className={`w-full text-left px-2.5 py-2 rounded-lg text-xs font-mono transition-colors flex items-center justify-between cursor-pointer ${
                    isSelected
                      ? 'bg-teal-500/15 text-teal-700 dark:text-teal-300 font-semibold border border-teal-500/30'
                      : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-200/50 dark:hover:bg-neutral-800/50'
                  }`}
                >
                  <div className="flex items-center gap-2 truncate">
                    {fileName.endsWith('.diff') ? (
                      <Code2 className="w-3.5 h-3.5 text-amber-500 shrink-0" />
                    ) : fileName.endsWith('.json') ? (
                      <FileCode className="w-3.5 h-3.5 text-blue-500 shrink-0" />
                    ) : (
                      <FileText className="w-3.5 h-3.5 text-teal-500 shrink-0" />
                    )}
                    <span className="truncate">{fileName}</span>
                  </div>
                  <span className="text-[10px] text-neutral-400 shrink-0 ml-1">
                    {mockFiles[fileName].size}
                  </span>
                </button>
              );
            })}
          </div>

          {/* File Preview Content Pane */}
          <div className="md:col-span-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#121316] flex flex-col overflow-hidden">
            {/* File Info Bar */}
            <div className="px-3.5 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-[#16181b] flex items-center justify-between">
              <div className="flex items-center gap-2 font-mono text-[11px]">
                <span className="font-semibold text-neutral-800 dark:text-neutral-200">{selectedFile}</span>
                <span className="text-neutral-400">&middot; {currentFileData.size}</span>
                <span className="text-neutral-400">&middot; Modified {currentFileData.modified}</span>
              </div>

              <div className="flex items-center gap-1.5">
                <button
                  onClick={handleCopy}
                  className="px-2 py-1 rounded border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-[11px] font-medium transition-colors flex items-center gap-1 cursor-pointer"
                  title="Copy file contents"
                >
                  {copied ? <Check className="w-3 h-3 text-emerald-500" /> : <Copy className="w-3 h-3" />}
                  <span>{copied ? 'Copied' : 'Copy'}</span>
                </button>
              </div>
            </div>

            {/* Code / Text Viewport */}
            <pre className="flex-1 p-3.5 overflow-y-auto font-mono text-[11px] leading-relaxed text-neutral-800 dark:text-neutral-200 bg-transparent select-text whitespace-pre-wrap break-all scrollbar-none">
              {currentFileData.content}
            </pre>
          </div>
        </div>

        {/* Footer */}
        <div className="pt-3 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-between text-neutral-500 text-[11px]">
          <span className="font-mono">
            Filesystem Sandbox enforced via Tauri / Landlock kernel isolation
          </span>
          <button
            onClick={onClose}
            className="px-4 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-700 dark:text-neutral-300 font-medium transition-colors cursor-pointer"
          >
            Close Workspace
          </button>
        </div>
      </div>
    </div>
  );
};

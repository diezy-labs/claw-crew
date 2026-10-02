import React, { useState } from 'react';
import { Copy, Check, Terminal } from 'lucide-react';

export interface CodeSnippetProps {
  code: string;
  language?: string;
  title?: string;
  showLineNumbers?: boolean;
  maxHeight?: string;
  wrapLines?: boolean;
  className?: string;
}

export const CodeSnippet: React.FC<CodeSnippetProps> = ({
  code,
  language = 'bash',
  title,
  showLineNumbers = false,
  maxHeight = 'max-h-80',
  wrapLines = false,
  className = ''
}) => {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard?.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  const lines = code.trim().split('\n');

  return (
    <div
      className={`rounded-xl border border-neutral-800 bg-[#0d0e12] text-neutral-200 overflow-hidden shadow-sm flex flex-col font-mono text-xs ${className}`}
    >
      {/* Header bar */}
      <div className="px-3.5 py-2 border-b border-neutral-800/80 bg-[#13151a] flex items-center justify-between select-none">
        <div className="flex items-center gap-2 min-w-0">
          <Terminal className="w-3.5 h-3.5 text-teal-400 shrink-0" />
          {title ? (
            <span className="font-semibold text-neutral-300 truncate text-[11px]">
              {title}
            </span>
          ) : (
            <span className="text-[10px] text-neutral-500 uppercase tracking-wider">
              {language}
            </span>
          )}
        </div>

        <button
          type="button"
          onClick={handleCopy}
          aria-label="Copy code"
          className="flex items-center gap-1.5 px-2 py-0.5 rounded text-[11px] text-neutral-400 hover:text-neutral-100 hover:bg-neutral-800 transition-colors cursor-pointer"
        >
          {copied ? (
            <>
              <Check className="w-3 h-3 text-emerald-400" />
              <span className="text-emerald-400 font-semibold">Copied</span>
            </>
          ) : (
            <>
              <Copy className="w-3 h-3" />
              <span>Copy</span>
            </>
          )}
        </button>
      </div>

      {/* Code body */}
      <div
        className={`p-3.5 overflow-x-auto overflow-y-auto leading-relaxed select-text scrollbar-thin ${maxHeight} ${
          wrapLines ? 'whitespace-pre-wrap break-all' : 'whitespace-pre'
        }`}
      >
        {showLineNumbers ? (
          <table className="w-full border-collapse">
            <tbody>
              {lines.map((line, idx) => (
                <tr key={idx} className="hover:bg-white/5 transition-colors">
                  <td className="w-8 pr-3 text-right text-neutral-600 select-none text-[11px] align-top">
                    {idx + 1}
                  </td>
                  <td className="text-neutral-300 align-top">{line || ' '}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <code>{code}</code>
        )}
      </div>
    </div>
  );
};

export const TerminalOutput = CodeSnippet;

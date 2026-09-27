// Tauri detection and IPC utilities for ClawCrew Desktop.

declare global {
  interface Window {
    __TAURI__?: unknown;
    __CLAWCREW_GATEWAY__?: string;
  }
}

/** Returns true when running inside a Tauri WebView. */
export const isTauri = (): boolean => '__TAURI__' in window;

/** Gateway base URL when running inside Tauri (defaults to localhost). */
export const tauriGatewayUrl = (): string =>
  window.__CLAWCREW_GATEWAY__ ?? 'http://127.0.0.1:42617';

type TauriBridge = {
  core?: {
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  };
};

export function invokeDesktop<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const bridge = window.__TAURI__ as TauriBridge | undefined;
  if (!bridge?.core?.invoke) {
    return Promise.reject(new Error('Desktop IPC is unavailable'));
  }
  return bridge.core.invoke<T>(command, args);
}

/** Fetches raw Prometheus metrics from the Go Agent Engine sidecar. */
export const getEngineMetrics = async (): Promise<string> => {
  if (isTauri()) {
    return invokeDesktop<string>('get_engine_metrics');
  }
  try {
    const res = await fetch('http://127.0.0.1:9090/metrics');
    return res.text();
  } catch {
    return '# Agent Engine metrics offline (standalone browser mode)';
  }
};

/** Fetches recent lines from the central agent.log file. */
export const getEngineLogs = async (lines = 100): Promise<string[]> => {
  if (isTauri()) {
    return invokeDesktop<string[]>('get_engine_logs', { lines });
  }
  return ['Agent Engine log viewer active. Logs recorded in %APPDATA%/clawcrew/logs/agent.log'];
};

/** Checks whether the Go Agent Engine sidecar is healthy. */
export const getEngineHealth = async (): Promise<boolean> => {
  if (isTauri()) {
    return invokeDesktop<boolean>('get_engine_health');
  }
  try {
    const res = await fetch('http://127.0.0.1:9090/healthz');
    return res.ok;
  } catch {
    return false;
  }
};

/** Dispatches an agent turn to the Go Agent Engine. */
export const startAgentTurn = async (sessionId: string, prompt: string, agentId?: string): Promise<{ status: string; raw_stream: string }> => {
  if (isTauri()) {
    return invokeDesktop<{ status: string; raw_stream: string }>('start_agent_turn', {
      sessionId,
      agentId,
      prompt,
    });
  }
  const res = await fetch('http://127.0.0.1:9090/api/turn', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session_id: sessionId, agent_id: agentId, prompt }),
  });
  const text = await res.text();
  return { status: 'success', raw_stream: text };
};

/** Queries local in-memory vector store on the Go Agent Engine. */
export const queryAgentMemory = async (query: string, topK = 5): Promise<any[]> => {
  if (isTauri()) {
    return invokeDesktop<any[]>('query_agent_memory', { query, topK });
  }
  const res = await fetch('http://127.0.0.1:9090/api/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, top_k: topK }),
  });
  return res.json();
};

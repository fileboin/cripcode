/**
 * Per-agent REMOTE binding: which SSH server + remote project an agent is
 * connected to. Persisted in localStorage (`remote-agent-binding.v1`) so the
 * binding survives app restarts — the user configures it once.
 *
 * NO secrets: the SSH password lives in the OS keystore (CripCode SSH flow)
 * and the OpenCode credential lives on the VPS (`opencode auth`). This
 * binding stores only identity/context.
 *
 * @module lib/remoteAgentBinding
 */

export interface RemoteAgentBinding {
  serverId: string;
  serverName: string;
  projectId: string;
  projectName: string;
  /** Remote working directory for the agent. */
  path: string;
  boundAt: number;
}

const STORAGE_KEY = 'remote-agent-binding.v1';

type Listener = () => void;
const listeners = new Set<Listener>();

function readAll(): Record<string, RemoteAgentBinding> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Record<string, RemoteAgentBinding>) : {};
  } catch {
    return {};
  }
}

function writeAll(all: Record<string, RemoteAgentBinding>) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  listeners.forEach((fn) => {
    try {
      fn();
    } catch {
      /* best-effort */
    }
  });
}

export function getRemoteAgentBinding(agentId: string): RemoteAgentBinding | null {
  return readAll()[agentId] ?? null;
}

export function setRemoteAgentBinding(agentId: string, binding: RemoteAgentBinding): void {
  const all = readAll();
  all[agentId] = binding;
  writeAll(all);
}

/** Clear the binding — returns to LOCAL/built-in mode. Does NOT touch the
 * SSH server, credentials, remote project or keystore. */
export function clearRemoteAgentBinding(agentId: string): void {
  const all = readAll();
  delete all[agentId];
  writeAll(all);
}

export function subscribeRemoteAgentBindings(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

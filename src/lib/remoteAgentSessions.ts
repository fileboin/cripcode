/**
 * Registry of REMOTE agent sessions (agents running on a VPS through the
 * SSH/PTY channel).
 *
 * Local agent sessions live in `sessionRegistry` (path-keyed, local
 * workspaces). Remote agent sessions have no local project path, so they are
 * tracked here instead and surfaced on the dashboard AgentsPanel.
 *
 * Keys: `remote:{projectId}:{agentId}`. Values contain NO secrets — the
 * password/keystore material stays in the OS keystore and inside the VPS
 * session; this registry only records session identity and status.
 *
 * @module lib/remoteAgentSessions
 */

export interface RemoteAgentSession {
  key: string;
  projectId: string;
  projectName: string;
  serverId: string;
  serverName: string;
  agentId: string;
  agentName: string;
  remotePath: string;
  /** Deterministic pty session id (`ssh-agent-{serverId}-{agentId}-{pathHash}`). */
  ptySessionId: string;
  status: 'running' | 'stopped';
  startedAt: number;
  lastActiveAt: number;
}

const STORAGE_KEY = 'remote-agent-sessions.v1';

type Listener = () => void;
const listeners = new Set<Listener>();

function readAll(): Record<string, RemoteAgentSession> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    return JSON.parse(raw) as Record<string, RemoteAgentSession>;
  } catch {
    return {};
  }
}

function writeAll(all: Record<string, RemoteAgentSession>) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  listeners.forEach((fn) => {
    try {
      fn();
    } catch {
      /* listener errors never break the store */
    }
  });
}

/** Register (or refresh) a remote agent session. Status becomes running. */
export function upsertRemoteAgentSession(
  session: Omit<RemoteAgentSession, 'key' | 'status' | 'startedAt' | 'lastActiveAt'> & {
    startedAt?: number;
  }
): void {
  const all = readAll();
  const key = `remote:${session.projectId}:${session.agentId}`;
  const startedAt = session.startedAt ?? all[key]?.startedAt ?? Date.now();
  all[key] = { ...session, key, status: 'running', startedAt, lastActiveAt: Date.now() };
  writeAll(all);
}

/** Mark a session stopped (pty exit / explicit stop). No-op if unknown. */
export function markRemoteAgentSessionStopped(ptySessionId: string): void {
  const all = readAll();
  let changed = false;
  for (const key of Object.keys(all)) {
    if (all[key].ptySessionId === ptySessionId) {
      all[key] = { ...all[key], status: 'stopped', lastActiveAt: Date.now() };
      changed = true;
    }
  }
  if (changed) writeAll(all);
}

/** Mark a session running again (e.g. re-attached to a live pty). */
export function markRemoteAgentSessionRunning(ptySessionId: string): void {
  const all = readAll();
  for (const key of Object.keys(all)) {
    if (all[key].ptySessionId === ptySessionId && all[key].status !== 'running') {
      all[key] = { ...all[key], status: 'running', lastActiveAt: Date.now() };
      writeAll(all);
      return;
    }
  }
}

/** All sessions, most-recently-active first. */
export function getRemoteAgentSessions(): RemoteAgentSession[] {
  return Object.values(readAll()).sort((a, b) => b.lastActiveAt - a.lastActiveAt);
}

export function removeRemoteAgentSession(key: string): void {
  const all = readAll();
  delete all[key];
  writeAll(all);
}

/** Subscribe to store changes; returns the unsubscribe function. */
export function subscribeRemoteAgentSessions(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

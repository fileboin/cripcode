/**
 * Tests for the remote agent session registry (GAP 1):
 * - upsert registers/refreshes sessions keyed `remote:{projectId}:{agentId}`
 * - pty session id lookup for stop/restart transitions
 * - sessions carry project/server/agent context and no secrets
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

const storage = new Map<string, string>();
const localStorageMock = {
  getItem: (k: string) => storage.get(k) ?? null,
  setItem: (k: string, v: string) => void storage.set(k, v),
  removeItem: (k: string) => void storage.delete(k),
};
vi.stubGlobal('localStorage', localStorageMock);

import {
  getRemoteAgentSessions,
  markRemoteAgentSessionRunning,
  markRemoteAgentSessionStopped,
  removeRemoteAgentSession,
  upsertRemoteAgentSession,
} from './remoteAgentSessions';

const SESSION = {
  projectId: 'proj-1',
  projectName: 'probe-remote',
  serverId: 'srv-1',
  serverName: 'testvps',
  agentId: 'opencode',
  agentName: 'OpenCode',
  remotePath: '/root',
  ptySessionId: 'ssh-agent-srv-1-opencode-root',
};

describe('remote agent sessions registry', () => {
  beforeEach(() => {
    storage.clear();
  });

  it('registers a session with project/server/agent context and no secrets', () => {
    upsertRemoteAgentSession(SESSION);
    const all = getRemoteAgentSessions();
    expect(all).toHaveLength(1);
    expect(all[0]).toMatchObject({
      key: 'remote:proj-1:opencode',
      projectId: 'proj-1',
      serverId: 'srv-1',
      agentId: 'opencode',
      status: 'running',
    });
    const serialized = JSON.stringify(all);
    expect(serialized).not.toContain('sk-ant');
    expect(serialized).not.toContain('password');
  });

  it('upsert refreshes the same key instead of duplicating', () => {
    upsertRemoteAgentSession(SESSION);
    upsertRemoteAgentSession(SESSION);
    expect(getRemoteAgentSessions()).toHaveLength(1);
  });

  it('stop marks by pty session id; restart marks running again', () => {
    upsertRemoteAgentSession(SESSION);
    markRemoteAgentSessionStopped(SESSION.ptySessionId);
    expect(getRemoteAgentSessions()[0].status).toBe('stopped');
    markRemoteAgentSessionRunning(SESSION.ptySessionId);
    expect(getRemoteAgentSessions()[0].status).toBe('running');
    // Unknown pty id is a no-op
    markRemoteAgentSessionStopped('unknown-id');
    expect(getRemoteAgentSessions()[0].status).toBe('running');
  });

  it('remove deletes the entry', () => {
    upsertRemoteAgentSession(SESSION);
    removeRemoteAgentSession('remote:proj-1:opencode');
    expect(getRemoteAgentSessions()).toHaveLength(0);
  });
});

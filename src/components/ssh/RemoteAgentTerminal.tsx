/**
 * RemoteAgentTerminal — an AI agent (Claude Code, Codex, OpenCode) running on
 * a remote VPS via SSH, working on a remote project.
 *
 * The agent CLI is spawned ON the VPS (`ssh user@host "cd /path && <agent>"`),
 * so it reads/writes the remote project's files directly. The PTY interaction
 * (xterm.js + pty_session) is identical to a local agent — input, output,
 * resize, scrollback replay all work the same way.
 *
 * @module components/ssh/RemoteAgentTerminal
 */

import { useEffect, useRef, useState } from 'react';
import { Terminal as XTerm } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { attachWebglRenderer } from '../../lib/terminalWebgl';
import { createWebLinksAddon } from '../../lib/terminalLinks';
import {
  openPtySession,
  attachPtySession,
  writePtySessionLogged,
  resizePtySessionLogged,
  killPtySession,
  detachPtySession,
  onPtySessionData,
  onPtySessionExit,
  createAttachGate,
} from '../../lib/ptySession';
import { getTerminalGpuEnabled } from '../../lib/settings';
import { loadNerdFonts } from '../../lib/fonts';
import { logger } from '../../lib/logger';
import { asCommandError, formatCommandError } from '../../lib/errors';
import {
  markRemoteAgentSessionRunning,
  markRemoteAgentSessionStopped,
  upsertRemoteAgentSession,
} from '../../lib/remoteAgentSessions';
import { buildRemoteAgentCommand, buildSshTerminalArgs, type SshServer } from '../../lib/ssh';
import type { AgentConfig } from '../../lib/agent';
import type { ExecutionTarget } from '../../hooks/useTerminalManagement';
import { useHostKeyGate } from '../../hooks/useHostKeyGate';
import { Button } from '../primitives/Button';
import '@xterm/xterm/css/xterm.css';

interface RemoteAgentTerminalProps {
  /** The agent to run (Claude Code, Codex, OpenCode). */
  agent: AgentConfig;
  /** The SSH server to run the agent on. */
  server: SshServer;
  /** The remote project path on the VPS. */
  remotePath: string;
  /** Remote project context — enables agent session registration (GAP 1). */
  projectId?: string;
  projectName?: string;
  /** Callback when the component goes back. */
  onBack: () => void;
}

/**
 * Decide how to spawn the remote agent PTY.
 *
 * With a registered remote project we hand the backend the same
 * `ExecutionTarget` the Workspace SSH tabs use: the backend builds the ssh
 * argv with `StrictHostKeyChecking=yes` and wires the keystore-backed askpass
 * helper, so password auth never needs a TTY prompt. Without a project id
 * there is no registered target the backend could validate, so the legacy
 * raw-ssh spawn is kept — still behind the host-key gate, and the server id
 * is passed so the backend can wire the same keystore askpass channel.
 * Exported for unit testing.
 */
export function buildRemoteAgentSpawn(
  server: SshServer,
  agent: AgentConfig,
  remotePath: string,
  projectId?: string
): {
  command: string;
  args: string[];
  executionTarget?: ExecutionTarget;
  sshServerId?: string;
} {
  if (projectId) {
    return {
      command: agent.binaryName,
      args: [],
      executionTarget: {
        kind: 'ssh',
        serverId: server.id,
        remoteProjectId: projectId,
        remotePath,
      },
    };
  }
  return {
    command: 'ssh',
    args: [...buildSshTerminalArgs(server), buildRemoteAgentCommand(remotePath, agent.binaryName)],
    // No registered remote project to build an execution target from, but the
    // backend can still wire ssh's askpass channel from this server's keystore
    // credential (id only — never a secret).
    sshServerId: server.id,
  };
}

export function RemoteAgentTerminal({
  agent,
  server,
  remotePath,
  projectId,
  projectName,
  onBack,
}: RemoteAgentTerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XTerm | null>(null);
  const [exited, setExited] = useState(false);
  const mountedRef = useRef(false);
  const { ensureHostKeyAccepted, hostKeyModal } = useHostKeyGate();

  const sessionId = `ssh-agent-${server.id}-${agent.id}-${remotePath.replace(/[^a-zA-Z0-9]/g, '-')}`;

  useEffect(() => {
    mountedRef.current = true;
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !mountedRef.current) return;
    let cancelled = false;
    let sessionOpened = false;
    const disposers: Array<() => void> = [];

    const term = new XTerm({
      fontFamily: '"JetBrainsMono NF", Menlo, Monaco, "Courier New", monospace',
      fontSize: 12,
      lineHeight: 1.2,
      cursorBlink: true,
      cursorStyle: 'bar',
      scrollback: 5000,
      allowProposedApi: true,
      minimumContrastRatio: 4.5,
      theme: {
        background: '#1e1e1e',
        foreground: '#cccccc',
        cursor: '#ffffff',
        selectionBackground: '#3a3d41',
      },
    });
    const fit = new FitAddon();
    const unicode11 = new Unicode11Addon();
    term.loadAddon(fit);
    term.loadAddon(unicode11);
    term.loadAddon(createWebLinksAddon());
    term.unicode.activeVersion = '11';
    term.open(container);
    termRef.current = term;

    const safeFit = () => {
      try {
        fit.fit();
      } catch {
        /* container detached mid-teardown */
      }
    };

    void loadNerdFonts().then(() => {
      if (!cancelled) safeFit();
    });

    void (async () => {
      if (cancelled) return;
      const gpuEnabled = await getTerminalGpuEnabled();
      if (cancelled || !gpuEnabled) return;
      disposers.push(attachWebglRenderer(term, container));
    })();

    setTimeout(() => {
      if (!cancelled) safeFit();
    }, 0);

    // User keystrokes → PTY
    const inputDisposable = term.onData((data) => {
      writePtySessionLogged(sessionId, data);
    });
    disposers.push(() => inputDisposable.dispose());

    // The spawn spec (backend execution target or legacy raw-ssh fallback) is
    // built by buildRemoteAgentSpawn; `user@host` stays the last element of the
    // legacy args so the remote command is appended AFTER the target.
    term.writeln(
      `\x1b[2mStarting ${agent.displayName} on ${server.username}@${server.host}:${remotePath}...\x1b[0m`
    );

    void (async () => {
      try {
        // Explicit host-key gate — the same one every SSH surface uses:
        // known → proceed, unknown → fingerprint confirmation, changed or
        // unprobeable → blocked.
        if (!(await ensureHostKeyAccepted(server))) return;
        const spawn = buildRemoteAgentSpawn(server, agent, remotePath, projectId);
        await openPtySession({
          sessionId,
          command: spawn.command,
          args: spawn.args,
          cwd: null,
          env: {},
          cols: Math.max(term.cols, 2),
          rows: Math.max(term.rows, 2),
          projectPath: null,
          executionTarget: spawn.executionTarget,
          sshServerId: spawn.sshServerId,
        });
        sessionOpened = true;
        if (cancelled) return;
        resizePtySessionLogged(sessionId, Math.max(term.cols, 2), Math.max(term.rows, 2));

        // GAP 1: register the remote agent session so the dashboard Agents
        // surface sees it (server/agent/path context, no secrets).
        if (projectId) {
          upsertRemoteAgentSession({
            projectId,
            projectName: projectName ?? server.name,
            serverId: server.id,
            serverName: server.name,
            agentId: agent.id,
            agentName: agent.displayName,
            remotePath,
            ptySessionId: sessionId,
          });
          markRemoteAgentSessionRunning(sessionId);
        }

        const gate = createAttachGate((bytes) => {
          term.write(bytes);
        });
        const unlistenData = await onPtySessionData(sessionId, (bytes, offset) => {
          if (cancelled) return;
          gate.push(offset, bytes);
        });
        disposers.push(() => void unlistenData());

        const unlistenExit = await onPtySessionExit(sessionId, (exitCode) => {
          if (cancelled) return;
          setExited(true);
          // GAP 1: the remote agent session ended � update the registry.
          if (projectId) markRemoteAgentSessionStopped(sessionId);
          term.writeln(
            `\r\n\x1b[2m[${agent.displayName} session ended — exit code ${exitCode}]\x1b[0m`
          );
        });
        disposers.push(() => void unlistenExit());

        const attach = await attachPtySession(sessionId);
        if (cancelled) return;
        if (attach.buffer.length > 0) {
          term.write(attach.buffer);
        }
        gate.open(attach.endOffset);

        // GAP 1: sync the registry with the ACTUAL pty state — an app
        // restart kills backend ptys, so a persisted 'running' entry must
        // flip to 'stopped' when the pty is dead on re-attach.
        if (projectId) {
          if (attach.alive) {
            markRemoteAgentSessionRunning(sessionId);
          } else {
            markRemoteAgentSessionStopped(sessionId);
          }
        }

        if (!attach.alive && attach.exitCode !== null) {
          setExited(true);
        }
      } catch (err) {
        if (!cancelled) {
          const msg = formatCommandError(asCommandError(err));
          term.writeln(`\r\n\x1b[31mFailed to start ${agent.displayName}: ${msg}\x1b[0m`);
          logger.error('[RemoteAgentTerminal] agent spawn failed', {
            server: server.name,
            agent: agent.id,
            error: msg,
          });
        }
      }
    })();

    const resizeObserver = new ResizeObserver(() => {
      if (cancelled) return;
      safeFit();
      if (sessionOpened) {
        resizePtySessionLogged(sessionId, term.cols, term.rows);
      }
    });
    resizeObserver.observe(container);

    return () => {
      cancelled = true;
      resizeObserver.disconnect();
      void detachPtySession(sessionId);
      for (const dispose of disposers) {
        try {
          dispose();
        } catch {
          /* best-effort */
        }
      }
      term.dispose();
      termRef.current = null;
    };
  }, [agent, server, remotePath, sessionId, projectId, projectName, ensureHostKeyAccepted]);

  const handleReconnect = async () => {
    try {
      await killPtySession(sessionId);
    } catch {
      // Session may already be dead — ignore
    }
    setExited(false);
    // Force re-mount by bumping the ref — the effect will re-run because
    // sessionId changes when we re-open.
    mountedRef.current = false;
    setTimeout(() => {
      mountedRef.current = true;
    }, 0);
  };

  return (
    <div className="ssh-remote-terminal">
      <div className="ssh-remote-terminal-header">
        <Button variant="ghost" size="sm" onClick={onBack}>
          ← Back
        </Button>
        <span className="ssh-remote-terminal-title">
          {agent.displayName} — {server.username}@{server.host}:{remotePath}
        </span>
        {exited && (
          <Button variant="secondary" size="sm" onClick={() => void handleReconnect()}>
            Restart
          </Button>
        )}
      </div>
      <div ref={containerRef} className="ssh-remote-terminal-body" />
      {hostKeyModal}
    </div>
  );
}

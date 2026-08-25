/**
 * RemoteTerminal — an interactive terminal that runs over SSH on a remote
 * VPS. Reuses the same xterm.js + backend-owned PTY infrastructure as the
 * main Terminal and BuildTerminal components.
 *
 * Spawns `ssh` in a `pty_session` with keepalive and host-key verification.
 * The PTY lifecycle (resize, input, scrollback replay) works identically to
 * a local terminal — the only difference is the spawn command.
 *
 * @module components/ssh/RemoteTerminal
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
import { buildSshTerminalArgs, type SshServer } from '../../lib/ssh';
import { Button } from '../primitives/Button';
import '@xterm/xterm/css/xterm.css';

interface RemoteTerminalProps {
  server: SshServer;
  onBack: () => void;
}

export function RemoteTerminal({ server, onBack }: RemoteTerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XTerm | null>(null);
  const sessionIdRef = useRef<string>(`ssh-terminal-${server.id}`);
  const [exited, setExited] = useState(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let cancelled = false;
    let sessionOpened = false;
    const disposers: Array<() => void> = [];

    const sessionId = sessionIdRef.current;

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

    const sshArgs = buildSshTerminalArgs(server);

    term.writeln(`\x1b[2mConnecting to ${server.username}@${server.host}...\x1b[0m`);

    void (async () => {
      try {
        await openPtySession({
          sessionId,
          command: 'ssh',
          args: sshArgs,
          cwd: null,
          env: {},
          cols: Math.max(term.cols, 2),
          rows: Math.max(term.rows, 2),
          projectPath: null,
        });
        sessionOpened = true;
        if (cancelled) return;
        resizePtySessionLogged(sessionId, Math.max(term.cols, 2), Math.max(term.rows, 2));

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
          term.writeln(`\r\n\x1b[2m[SSH session ended — exit code ${exitCode}]\x1b[0m`);
        });
        disposers.push(() => void unlistenExit());

        const attach = await attachPtySession(sessionId);
        if (cancelled) return;
        if (attach.buffer.length > 0) {
          term.write(attach.buffer);
        }
        gate.open(attach.endOffset);

        if (!attach.alive && attach.exitCode !== null) {
          setExited(true);
        }
      } catch (err) {
        if (!cancelled) {
          const msg = formatCommandError(asCommandError(err));
          term.writeln(`\r\n\x1b[31mFailed to connect: ${msg}\x1b[0m`);
          logger.error('[RemoteTerminal] SSH session failed', {
            server: server.name,
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
  }, [server]);

  const handleReconnect = async () => {
    const sessionId = sessionIdRef.current;
    try {
      await killPtySession(sessionId);
    } catch {
      // Session may already be dead — ignore
    }
    setExited(false);
    // Force re-mount by changing the key — the parent should handle this
    // via a state bump. For now, reload the component.
    window.location.reload();
  };

  return (
    <div className="ssh-remote-terminal">
      <div className="ssh-remote-terminal-header">
        <Button variant="ghost" size="sm" onClick={onBack}>
          ← Back
        </Button>
        <span className="ssh-remote-terminal-title">
          {server.name} — {server.username}@{server.host}
        </span>
        {exited && (
          <Button variant="secondary" size="sm" onClick={() => void handleReconnect()}>
            Reconnect
          </Button>
        )}
      </div>
      <div ref={containerRef} className="ssh-remote-terminal-body" />
    </div>
  );
}

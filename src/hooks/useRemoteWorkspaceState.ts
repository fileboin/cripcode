/**
 * Per-remote-project persistent workspace state (tab + path), keyed
 * `remote:{projectId}` — the same keying pattern the terminal tabs use for
 * local projects. Closing/reopening a remote project restores the last
 * active tab and folder.
 *
 * @module hooks/useRemoteWorkspaceState
 */

import { useCallback, useState } from 'react';

export interface RemoteWorkspaceState {
  tab: string;
  path: string;
}

const STORAGE_PREFIX = 'remote-workspace-state:';

function readState(projectId: string): Partial<RemoteWorkspaceState> {
  try {
    const raw = localStorage.getItem(STORAGE_PREFIX + projectId);
    return raw ? (JSON.parse(raw) as Partial<RemoteWorkspaceState>) : {};
  } catch {
    return {};
  }
}

/**
 * Returns the persisted state snapshot for this remote project plus stable
 * `persist` helpers — callers keep their existing useState semantics and
 * call `persistTab` / `persistPath` (and `persist` for both) whenever the
 * workspace state changes.
 */
export function useRemoteWorkspaceState(projectId: string | undefined, fallbackPath = '/home') {
  const key = STORAGE_PREFIX + (projectId ?? 'unknown');
  const [initial] = useState(() => readState(projectId ?? 'unknown'));
  const state = {
    tab: initial.tab ?? 'files',
    path: initial.path ?? fallbackPath,
  };

  const persist = useCallback(
    (patch: Partial<RemoteWorkspaceState>) => {
      try {
        const current = readState(projectId ?? 'unknown');
        localStorage.setItem(
          key,
          JSON.stringify({
            ...current,
            ...patch,
            tab: patch.tab ?? current.tab ?? 'files',
            path: patch.path ?? current.path ?? fallbackPath,
          })
        );
      } catch {
        /* persistence is best-effort */
      }
    },
    [key, projectId, fallbackPath]
  );

  const persistTab = useCallback((tab: string) => persist({ tab }), [persist]);
  const persistPath = useCallback((path: string) => persist({ path }), [persist]);

  return { tab: state.tab, path: state.path, persist, persistTab, persistPath };
}

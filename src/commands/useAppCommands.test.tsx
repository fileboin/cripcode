/**
 * Android gating for app-level palette commands.
 *
 * Desktop-only entries (local-folder import, reveal in file manager, updater
 * check, attached libraries) must not be offered on Android, where their
 * backing commands don't exist. Everything else stays visible.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import type { ReactNode } from 'react';
import { ModalProvider } from '../contexts/ModalContext';
import { _reset, getSnapshot } from './registry';
import { matchesContext, type PaletteCtx } from './types';
import { useAppCommands } from './useAppCommands';
import { getDashboardProjects, type Project } from '../lib/project';
import { checkIdeAvailability } from '../lib/ide';

const androidState = vi.hoisted(() => ({ value: false }));

vi.mock('../lib/setup', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/setup')>();
  return { ...actual, isAndroid: () => androidState.value };
});

vi.mock('../lib/project', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/project')>();
  return { ...actual, getDashboardProjects: vi.fn() };
});

vi.mock('../lib/ide', () => ({
  checkIdeAvailability: vi.fn(),
  openInIde: vi.fn(),
  openInFinder: vi.fn(),
}));

const DESKTOP_ONLY_HOME = [
  'project.import.local',
  'settings.checkUpdates',
  'modal.attachedLibraries',
];
const DESKTOP_ONLY_PROJECT = ['ide.finder'];

const project: Project = { name: 'demo', path: '/tmp/demo', thumbnail: null };

const ctx = (kind: 'home' | 'project'): PaletteCtx => ({ kind, currentProjectName: project.name });

const baseParams = {
  currentProject: project,
  pinnedPaths: [] as string[],
  handleSelectProject: vi.fn(),
  handleBackToProjects: vi.fn(),
  handleCreateProject: vi.fn(),
  handleImportProject: vi.fn(),
  handleImportLocalFolder: vi.fn(),
  handleGitHubConnect: vi.fn(),
  handleRestartDevServer: vi.fn(),
  isEducationMode: false,
  setIsEducationMode: vi.fn(),
  showToast: vi.fn(),
};

function renderCommands() {
  return renderHook(() => useAppCommands(baseParams), {
    wrapper: ({ children }: { children: ReactNode }) => <ModalProvider>{children}</ModalProvider>,
  });
}

function commandById(id: string) {
  const command = getSnapshot().find((cmd) => cmd.id === id);
  if (!command) throw new Error(`command not registered: ${id}`);
  return command;
}

describe('useAppCommands Android gating', () => {
  beforeEach(() => {
    _reset();
    androidState.value = false;
    vi.mocked(checkIdeAvailability).mockResolvedValue({ vscode: false, cursor: false });
    vi.mocked(getDashboardProjects).mockResolvedValue([]);
  });

  it('filters desktop-only commands out on Android', async () => {
    androidState.value = true;
    renderCommands();
    await act(async () => {});

    for (const id of DESKTOP_ONLY_HOME) {
      expect(matchesContext(commandById(id).when, ctx('home'))).toBe(false);
    }
    for (const id of DESKTOP_ONLY_PROJECT) {
      expect(matchesContext(commandById(id).when, ctx('project'))).toBe(false);
    }
    expect(matchesContext(commandById('modal.help').when, ctx('home'))).toBe(true);
  });

  it('keeps desktop-only commands available on desktop', async () => {
    renderCommands();
    await act(async () => {});

    for (const id of DESKTOP_ONLY_HOME) {
      expect(matchesContext(commandById(id).when, ctx('home'))).toBe(true);
    }
    for (const id of DESKTOP_ONLY_PROJECT) {
      expect(matchesContext(commandById(id).when, ctx('project'))).toBe(true);
    }
  });
});

import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  listRemoteProjects,
  addRemoteProject,
  removeRemoteProject,
  markRemoteProjectOpened,
} from './remoteProjects';

const sampleProject = {
  id: 'abc-123',
  name: 'My App',
  serverId: 'server-1',
  remotePath: '/home/user/my-app',
  createdAt: 1724600000000,
  lastOpened: null,
};

describe('listRemoteProjects', () => {
  it('returns the project list from the backend', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_remote_projects') return [sampleProject];
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(listRemoteProjects()).resolves.toEqual([sampleProject]);
  });

  it('returns an empty array when no projects are registered', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_remote_projects') return [];
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(listRemoteProjects()).resolves.toEqual([]);
  });
});

describe('addRemoteProject', () => {
  it('calls add_remote_project with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'add_remote_project') {
        expect(args).toMatchObject({
          name: 'My App',
          serverId: 'server-1',
          remotePath: '/home/user/my-app',
        });
        return sampleProject;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const result = await addRemoteProject('My App', 'server-1', '/home/user/my-app');
    expect(result.name).toBe('My App');
  });
});

describe('removeRemoteProject', () => {
  it('calls remove_remote_project with the id', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'remove_remote_project') {
        expect(args).toMatchObject({ id: 'abc-123' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(removeRemoteProject('abc-123')).resolves.toBeUndefined();
  });
});

describe('markRemoteProjectOpened', () => {
  it('calls mark_remote_project_opened with the id', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'mark_remote_project_opened') {
        expect(args).toMatchObject({ id: 'abc-123' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(markRemoteProjectOpened('abc-123')).resolves.toBeUndefined();
  });
});

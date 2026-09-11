import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  listRemoteProjects,
  addRemoteProject,
  removeRemoteProject,
  markRemoteProjectOpened,
  setRemoteProjectLocalLink,
  findRemoteProjectForLocalPath,
  type RemoteProject,
} from './remoteProjects';

const sampleProject = {
  id: 'abc-123',
  name: 'My App',
  serverId: 'server-1',
  remotePath: '/home/user/my-app',
  localProjectPath: null,
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
          localProjectPath: '/local/repo',
        });
        return { ...sampleProject, localProjectPath: '/local/repo' };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const result = await addRemoteProject('My App', 'server-1', '/home/user/my-app', '/local/repo');
    expect(result.localProjectPath).toBe('/local/repo');
  });

  it('defaults localProjectPath to null', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'add_remote_project') {
        expect(args).toMatchObject({ localProjectPath: null });
        return sampleProject;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(addRemoteProject('My App', 'server-1', '/home/user/my-app')).resolves.toEqual(
      sampleProject
    );
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

describe('setRemoteProjectLocalLink', () => {
  it('calls set_remote_project_local_link with the link', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'set_remote_project_local_link') {
        expect(args).toMatchObject({ id: 'abc-123', localProjectPath: '/local/repo' });
        return { ...sampleProject, localProjectPath: '/local/repo' };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const result = await setRemoteProjectLocalLink('abc-123', '/local/repo');
    expect(result.localProjectPath).toBe('/local/repo');
  });

  it('unlinks with null', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'set_remote_project_local_link') {
        expect(args).toMatchObject({ id: 'abc-123', localProjectPath: null });
        return sampleProject;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(setRemoteProjectLocalLink('abc-123', null)).resolves.toEqual(sampleProject);
  });
});

describe('findRemoteProjectForLocalPath', () => {
  const linked: RemoteProject = {
    ...sampleProject,
    id: 'linked',
    localProjectPath: '/repos/bro',
  };
  const other: RemoteProject = {
    ...sampleProject,
    id: 'other',
    remotePath: '/root/ollama',
    localProjectPath: null,
  };

  it('finds the remote project linked to the local path', () => {
    expect(findRemoteProjectForLocalPath([other, linked], '/repos/bro')?.id).toBe('linked');
  });

  it('returns null when no project is linked', () => {
    expect(findRemoteProjectForLocalPath([other], '/repos/bro')).toBeNull();
  });

  it('returns null for an empty local path', () => {
    expect(findRemoteProjectForLocalPath([linked], '')).toBeNull();
    expect(findRemoteProjectForLocalPath([linked], null)).toBeNull();
    expect(findRemoteProjectForLocalPath([linked], undefined)).toBeNull();
  });

  it('returns null for an empty project list', () => {
    expect(findRemoteProjectForLocalPath([], '/repos/bro')).toBeNull();
  });
});

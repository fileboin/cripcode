import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  remoteGitStatus,
  remoteGitCurrentBranch,
  remoteGitListBranches,
  remoteGitChangedFiles,
  remoteGitCommit,
  remoteGitPull,
  remoteGitPush,
  remoteGitDiff,
} from './remoteGit';

describe('remoteGitStatus', () => {
  it('returns true when there are changes', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_status') return true;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(remoteGitStatus('server-id', '/home/user/app')).resolves.toBe(true);
  });

  it('returns false when clean', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_status') return false;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(remoteGitStatus('server-id', '/home/user/app')).resolves.toBe(false);
  });
});

describe('remoteGitCurrentBranch', () => {
  it('returns the branch name', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_current_branch') return 'main';
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(remoteGitCurrentBranch('server-id', '/home/user/app')).resolves.toBe('main');
  });

  it('returns null when no branch', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_current_branch') return null;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(remoteGitCurrentBranch('server-id', '/home/user/app')).resolves.toBeNull();
  });
});

describe('remoteGitListBranches', () => {
  it('returns branch list', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_list_branches') {
        return [
          {
            name: 'main',
            isCurrent: true,
            isRemote: false,
            lastCommitDate: 1724600000000,
            lastCommitAuthor: 'Alice',
          },
        ];
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const branches = await remoteGitListBranches('server-id', '/home/user/app');
    expect(branches).toHaveLength(1);
    expect(branches[0].name).toBe('main');
    expect(branches[0].isCurrent).toBe(true);
  });
});

describe('remoteGitChangedFiles', () => {
  it('returns changed files', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_changed_files') {
        return [{ path: 'src/app.ts', status: 'modified' }];
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const files = await remoteGitChangedFiles('server-id', '/home/user/app');
    expect(files).toHaveLength(1);
    expect(files[0].path).toBe('src/app.ts');
  });
});

describe('remoteGitCommit', () => {
  it('returns true when committed', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_commit') return true;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      remoteGitCommit('server-id', '/home/user/app', 'fix: update something')
    ).resolves.toBe(true);
  });
});

describe('remoteGitPull', () => {
  it('resolves on success', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_pull') return null;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(remoteGitPull('server-id', '/home/user/app')).resolves.toBeUndefined();
  });
});

describe('remoteGitPush', () => {
  it('resolves on success', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_push') return null;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(remoteGitPush('server-id', '/home/user/app', 'main')).resolves.toBeUndefined();
  });
});

describe('remoteGitDiff', () => {
  it('returns diff info', async () => {
    mockIPC((cmd) => {
      if (cmd === 'remote_git_diff') {
        return {
          file_path: 'src/app.ts',
          is_new_file: false,
          is_deleted: false,
          is_binary: false,
          content: '+new line',
          additions: 1,
          deletions: 0,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const diff = await remoteGitDiff('server-id', '/home/user/app', 'src/app.ts');
    expect(diff.filePath).toBe('src/app.ts');
    expect(diff.additions).toBe(1);
  });
});

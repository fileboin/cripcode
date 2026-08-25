import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  listRemoteFiles,
  readRemoteFile,
  saveRemoteFile,
  createRemoteDirectory,
  deleteRemoteFile,
  renameRemoteFile,
} from './remoteFiles';

describe('listRemoteFiles', () => {
  it('returns file entries from the backend', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_remote_files') {
        return [
          { name: 'src', path: 'src', is_directory: true, size: 0 },
          { name: 'package.json', path: 'package.json', is_directory: false, size: 234 },
        ];
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const result = await listRemoteFiles('server-id', '/home/user/myproject');
    expect(result).toHaveLength(2);
    expect(result[0].name).toBe('src');
    expect(result[0].isDirectory).toBe(true);
    expect(result[1].name).toBe('package.json');
    expect(result[1].isDirectory).toBe(false);
    expect(result[1].size).toBe(234);
  });

  it('returns an empty array when the directory is empty', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_remote_files') return [];
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(listRemoteFiles('server-id', '/empty')).resolves.toEqual([]);
  });
});

describe('readRemoteFile', () => {
  it('returns file content from the backend', async () => {
    mockIPC((cmd) => {
      if (cmd === 'read_remote_file') {
        return {
          content: 'console.log("hello");',
          is_binary: false,
          is_truncated: false,
          size: 22,
          language: 'javascript',
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const result = await readRemoteFile('server-id', '/home/user/app.js');
    expect(result.content).toBe('console.log("hello");');
    expect(result.isBinary).toBe(false);
    expect(result.language).toBe('javascript');
    expect(result.size).toBe(22);
  });

  it('detects binary files', async () => {
    mockIPC((cmd) => {
      if (cmd === 'read_remote_file') {
        return { content: '', is_binary: true, is_truncated: false, size: 1024, language: '' };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const result = await readRemoteFile('server-id', '/home/user/image.png');
    expect(result.isBinary).toBe(true);
    expect(result.content).toBe('');
  });
});

describe('saveRemoteFile', () => {
  it('calls save_remote_file with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'save_remote_file') {
        expect(args).toMatchObject({
          serverId: 'server-id',
          filePath: '/home/user/app.js',
          content: 'new content',
        });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      saveRemoteFile('server-id', '/home/user/app.js', 'new content')
    ).resolves.toBeUndefined();
  });
});

describe('createRemoteDirectory', () => {
  it('calls create_remote_directory with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'create_remote_directory') {
        expect(args).toMatchObject({ serverId: 'server-id', dirPath: '/home/user/newdir' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(createRemoteDirectory('server-id', '/home/user/newdir')).resolves.toBeUndefined();
  });
});

describe('deleteRemoteFile', () => {
  it('calls delete_remote_file with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'delete_remote_file') {
        expect(args).toMatchObject({ serverId: 'server-id', path: '/home/user/oldfile' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(deleteRemoteFile('server-id', '/home/user/oldfile')).resolves.toBeUndefined();
  });
});

describe('renameRemoteFile', () => {
  it('calls rename_remote_file with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'rename_remote_file') {
        expect(args).toMatchObject({
          serverId: 'server-id',
          oldPath: '/home/user/old.js',
          newPath: '/home/user/new.js',
        });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      renameRemoteFile('server-id', '/home/user/old.js', '/home/user/new.js')
    ).resolves.toBeUndefined();
  });
});

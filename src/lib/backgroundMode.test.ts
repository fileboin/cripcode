import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  getBackgroundMode,
  setBackgroundMode,
  reportActiveTaskCount,
  isPreventingSleep,
} from './backgroundMode';

describe('getBackgroundMode', () => {
  it('returns the current mode (default smart)', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_background_mode') return 'smart';
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(getBackgroundMode()).resolves.toBe('smart');
  });

  it('returns always_on when set', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_background_mode') return 'always_on';
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(getBackgroundMode()).resolves.toBe('always_on');
  });
});

describe('setBackgroundMode', () => {
  it('calls set_background_mode with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'set_background_mode') {
        expect(args).toMatchObject({ mode: 'always_on' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(setBackgroundMode('always_on')).resolves.toBeUndefined();
  });

  it('passes off mode', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'set_background_mode') {
        expect(args).toMatchObject({ mode: 'off' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(setBackgroundMode('off')).resolves.toBeUndefined();
  });
});

describe('reportActiveTaskCount', () => {
  it('passes the count to backend', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'report_active_task_count') {
        expect(args).toMatchObject({ count: 3 });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(reportActiveTaskCount(3)).resolves.toBeUndefined();
  });

  it('passes zero count', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'report_active_task_count') {
        expect(args).toMatchObject({ count: 0 });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(reportActiveTaskCount(0)).resolves.toBeUndefined();
  });
});

describe('isPreventingSleep', () => {
  it('returns true when preventing sleep', async () => {
    mockIPC((cmd) => {
      if (cmd === 'is_preventing_sleep') return true;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(isPreventingSleep()).resolves.toBe(true);
  });

  it('returns false when not preventing sleep', async () => {
    mockIPC((cmd) => {
      if (cmd === 'is_preventing_sleep') return false;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(isPreventingSleep()).resolves.toBe(false);
  });
});

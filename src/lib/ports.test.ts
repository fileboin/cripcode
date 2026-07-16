/**
 * Tests for dev-server port helpers.
 *
 * `extractAnnouncedPort` is the ground-truth reader for which port a dev
 * server actually bound (frameworks auto-increment when their requested port
 * is taken) — false negatives mean the preview trusts a wrong port, false
 * positives mean random log lines move the preview. Both directions matter.
 */

import { describe, it, expect } from 'vitest';
import {
  extractAnnouncedPort,
  preferredPortForProject,
  DEV_PORT_RANGE_START,
  DEV_PORT_RANGE_SIZE,
} from './ports';

describe('preferredPortForProject', () => {
  it('is deterministic for the same path', () => {
    const path = '/Users/test/ShipStudio/my-app';
    expect(preferredPortForProject(path)).toBe(preferredPortForProject(path));
  });

  it('stays within the assigned range', () => {
    const paths = [
      '/Users/test/ShipStudio/a',
      '/Users/test/ShipStudio/b',
      '/Users/other/Projects/deeply/nested/project-name',
      '',
    ];
    for (const p of paths) {
      const port = preferredPortForProject(p);
      expect(port).toBeGreaterThanOrEqual(DEV_PORT_RANGE_START);
      expect(port).toBeLessThan(DEV_PORT_RANGE_START + DEV_PORT_RANGE_SIZE);
    }
  });

  it('spreads distinct projects to distinct ports (typical case)', () => {
    expect(preferredPortForProject('/Users/test/ShipStudio/storefront')).not.toBe(
      preferredPortForProject('/Users/test/ShipStudio/dashboard')
    );
  });
});

describe('extractAnnouncedPort', () => {
  it('reads the Next.js 13+ banner', () => {
    expect(extractAnnouncedPort('   - Local:        http://localhost:3000')).toBe(3000);
  });

  it('reads the Vite banner (trailing slash)', () => {
    expect(extractAnnouncedPort('  ➜  Local:   http://localhost:5173/')).toBe(5173);
  });

  it('reads the Astro banner (no colon after Local)', () => {
    expect(extractAnnouncedPort('┃ Local    http://localhost:4321/')).toBe(4321);
  });

  it('reads the CRA / webpack-dev-server banner', () => {
    expect(extractAnnouncedPort('  Local:            http://localhost:3000')).toBe(3000);
  });

  it('reads the Next.js ≤12 "started server" banner', () => {
    expect(
      extractAnnouncedPort('ready - started server on 0.0.0.0:3000, url: http://localhost:3001')
    ).toBe(3001);
  });

  it('accepts 127.0.0.1 and 0.0.0.0 hosts', () => {
    expect(extractAnnouncedPort('Local:   http://127.0.0.1:8080/')).toBe(8080);
    expect(extractAnnouncedPort('Local:   http://0.0.0.0:4000')).toBe(4000);
  });

  it('ignores localhost URLs without a Local announcement label', () => {
    expect(extractAnnouncedPort('proxying request to http://localhost:8080')).toBeNull();
    expect(extractAnnouncedPort('connect to http://localhost:9000 for devtools')).toBeNull();
    expect(extractAnnouncedPort('Last request was GET / 200')).toBeNull();
  });

  it('ignores the Network (LAN) URL line', () => {
    expect(extractAnnouncedPort('  ➜  Network: http://192.168.1.5:5173/')).toBeNull();
  });

  it('does not treat "localhost" itself as the Local label', () => {
    // "local" is a prefix of "localhost" — the separator requirement must
    // keep a bare URL from matching as its own announcement.
    expect(extractAnnouncedPort('http://localhost:3000')).toBeNull();
  });

  it('rejects out-of-range ports', () => {
    expect(extractAnnouncedPort('Local: http://localhost:99999')).toBeNull();
  });

  it('returns null for empty and unrelated lines', () => {
    expect(extractAnnouncedPort('')).toBeNull();
    expect(extractAnnouncedPort('webpack compiled successfully')).toBeNull();
  });
});

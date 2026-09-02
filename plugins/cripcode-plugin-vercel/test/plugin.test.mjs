import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';

const elements = [];
const fakeReact = {
  createElement(type, props, ...children) {
    const element = { type, props, children };
    elements.push(element);
    return element;
  },
  useContext() {
    return globalThis.window.__testContext;
  },
  useEffect() {},
  useState(value) {
    return [value, () => {}];
  },
};

globalThis.window = {
  __SHIPSTUDIO_REACT__: fakeReact,
  __SHIPSTUDIO_PLUGIN_CONTEXT__: {
    project: { path: '/tmp/project' },
    shell: { exec: async () => ({ exit_code: 0, stdout: 'vercel 1.0.0', stderr: '' }) },
    actions: { showToast() {}, openTerminal: async () => 0 },
  },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'Linux', platform: 'Linux' },
});

const manifest = JSON.parse(await readFile(new URL('../plugin.json', import.meta.url), 'utf8'));
const bundle = await import('../dist/index.js');

test('manifest satisfies the install contract', () => {
  assert.equal(manifest.id, 'vercel');
  assert.equal(manifest.name, 'Vercel');
  assert.deepEqual(manifest.slots, ['toolbar']);
  assert.equal(manifest.api_version, 1);
});

test('bundle exports the Vercel toolbar slot', () => {
  assert.equal(bundle.name, 'Vercel');
  assert.equal(typeof bundle.slots.toolbar, 'function');
});

test('toolbar renders through the host context', () => {
  const element = bundle.slots.toolbar();
  assert.equal(element.type, 'button');
  assert.match(element.props.className, /toolbar-icon-btn/);
  assert.equal(element.props.disabled, true);
  assert.equal(elements.length > 0, true);
});

import { describe, it, expect } from 'vitest';
import { TEMPLATES } from './useProjectCreation';

/**
 * The 11 built-in templates ship as bundled local ZIPs under
 * `src-tauri/resources/templates/<id>.zip` and are resolved by the backend
 * `create_project_from_bundled_template` command. No template may reference an
 * external GitHub repository anymore.
 */
const BUNDLED_TEMPLATE_IDS = [
  'nextjs-basic',
  'nextjs-plain-css',
  'astro-html',
  'astro-basic',
  'sveltekit-basic',
  'html-basic',
  'expo-mobile',
  'react-native-mobile',
  'flutter-mobile',
  'shopify-theme',
  'eve-agent',
];

describe('TEMPLATES (local bundled source)', () => {
  it('defines 11 built-in templates plus blank', () => {
    expect(TEMPLATES).toHaveLength(12);
    expect(TEMPLATES.find((t) => t.id === 'blank')).toBeDefined();
  });

  it('no template references an external repository URL', () => {
    for (const template of TEMPLATES) {
      expect('repo' in template).toBe(false);
    }
  });

  it('every built-in template has an id that maps to a bundled zip', () => {
    for (const id of BUNDLED_TEMPLATE_IDS) {
      const template = TEMPLATES.find((t) => t.id === id);
      expect(template, `missing template ${id}`).toBeDefined();
    }
  });

  it('skipInstall templates still resolve to a bundled zip (no npm needed)', () => {
    const skipInstall = TEMPLATES.filter((t) => t.skipInstall);
    expect(skipInstall.map((t) => t.id)).toEqual(
      expect.arrayContaining(['html-basic', 'flutter-mobile', 'shopify-theme'])
    );
    for (const template of skipInstall) {
      expect(BUNDLED_TEMPLATE_IDS).toContain(template.id);
    }
  });
});

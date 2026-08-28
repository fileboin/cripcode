import { afterEach, describe, expect, it, vi } from 'vitest';
import { fetchPluginRegistry, VERCEL_PLUGIN_REPO } from './plugins';

const CRIPCODE_REGISTRY_URL =
  'https://raw.githubusercontent.com/fileboin/cripcode-plugin-registry/main/registry.json';

describe('plugin registry configuration', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('loads the CripCode registry and parses its Vercel entry', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(
        JSON.stringify({
          version: 1,
          plugins: [
            {
              id: 'vercel',
              name: 'Vercel',
              description: 'Deploy projects to Vercel from CripCode.',
              repo: VERCEL_PLUGIN_REPO,
              author: 'CripCode',
              category: 'deployment',
              icon: 'vercel',
            },
          ],
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } }
      )
    );

    const entries = await fetchPluginRegistry();

    expect(fetchMock).toHaveBeenCalledWith(CRIPCODE_REGISTRY_URL);
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      id: 'vercel',
      repo: 'https://github.com/fileboin/cripcode-plugin-vercel',
      author: 'CripCode',
      category: 'deployment',
    });
    expect(entries[0].repo).not.toContain('ship-studio');
  });
});

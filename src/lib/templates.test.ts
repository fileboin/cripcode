import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  fetchCommunityTemplates,
  fetchTemplateDetails,
  parseTemplateDetailsResponse,
  parseTemplateListResponse,
} from './templates';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

const template = {
  id: 'template-1',
  name: 'Starter',
  description: 'A starter project',
  author: 'CripCode',
  category: 'marketing',
  framework: 'Astro',
  thumbnail: null,
  version: '1.0.0',
  download: { url: 'https://storage.example/template.zip', size_bytes: 1234 },
  created_at: '2026-08-29T00:00:00Z',
  updated_at: '2026-08-29T00:00:00Z',
};

describe('CripCode template API adapter', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('fetches and validates a template list', async () => {
    invokeMock.mockResolvedValue(JSON.stringify({ templates: [template], total: 1 }));

    await expect(
      fetchCommunityTemplates({ search: 'starter', category: 'web', framework: 'Astro', limit: 1 })
    ).resolves.toEqual({
      templates: [template],
      total: 1,
    });
    expect(invokeMock).toHaveBeenCalledWith('fetch_community_templates', {
      search: 'starter',
      category: 'web',
      framework: 'Astro',
      limit: 1,
    });
  });

  it('fetches and validates template details', async () => {
    invokeMock.mockResolvedValue(JSON.stringify(template));

    await expect(fetchTemplateDetails('template-1')).resolves.toEqual(template);
    expect(invokeMock).toHaveBeenCalledWith('fetch_template_details', { id: 'template-1' });
  });

  it('accepts an empty result', () => {
    expect(parseTemplateListResponse('{"templates":[],"total":0}')).toEqual({
      templates: [],
      total: 0,
    });
  });

  it('rejects malformed JSON and response envelopes', () => {
    expect(() => parseTemplateListResponse('not-json')).toThrow('expected JSON');
    expect(() => parseTemplateListResponse('{"items":[]}')).toThrow('templates must be an array');
  });

  it('rejects invalid template metadata', () => {
    const invalid = { ...template, author: '' };
    expect(() => parseTemplateDetailsResponse(JSON.stringify(invalid))).toThrow(
      'author must be a non-empty string'
    );
  });

  it('propagates an unavailable API error', async () => {
    invokeMock.mockRejectedValue(new Error('template API unavailable'));

    await expect(fetchCommunityTemplates()).rejects.toThrow('template API unavailable');
  });
});

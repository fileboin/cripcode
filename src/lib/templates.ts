/**
 * CripCode-native Community Templates contract and Tauri adapter.
 *
 * The backend owns the configured API base URL. This module only validates the
 * response contract and keeps the UI independent from transport details.
 */

import { invoke } from '@tauri-apps/api/core';

export interface TemplateDownloadInfo {
  url: string | null;
  size_bytes: number | null;
}

export interface CommunityTemplate {
  id: string;
  name: string;
  description: string;
  author: string;
  category: string;
  framework: string;
  thumbnail: string | null;
  version: string;
  download: TemplateDownloadInfo;
  created_at: string;
  updated_at: string;
}

export interface TemplateListResponse {
  templates: CommunityTemplate[];
  total: number;
}

export interface TemplateListParams {
  search?: string;
  category?: string;
  framework?: string;
  sort?: string;
  limit?: number;
  offset?: number;
}

function configuredApiBase(): string | undefined {
  const value: unknown = import.meta.env.VITE_CRIPCODE_TEMPLATES_API_BASE_URL as unknown;
  return typeof value === 'string' && value.trim() !== '' ? value.trim() : undefined;
}

export function templateDownloadUrl(template: CommunityTemplate): string {
  if (!template.download.url) {
    throw new Error(`Template "${template.name}" has no downloadable archive`);
  }
  return template.download.url;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function requiredString(value: unknown, field: string): string {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`Invalid template metadata: ${field} must be a non-empty string`);
  }
  return value;
}

function nullableString(value: unknown, field: string): string | null {
  if (value === null) return null;
  return requiredString(value, field);
}

function nullableNonNegativeNumber(value: unknown, field: string): number | null {
  if (value === null) return null;
  if (
    typeof value !== 'number' ||
    !Number.isInteger(value) ||
    !Number.isFinite(value) ||
    value < 0
  ) {
    throw new Error(`Invalid template metadata: ${field} must be a non-negative integer or null`);
  }
  return value;
}

function parseTemplate(value: unknown, index?: number): CommunityTemplate {
  const prefix = index === undefined ? 'template' : `templates[${index}]`;
  if (!isRecord(value)) {
    throw new Error(`Invalid template metadata: ${prefix} must be an object`);
  }

  if (!isRecord(value.download)) {
    throw new Error(`Invalid template metadata: ${prefix}.download must be an object`);
  }

  return {
    id: requiredString(value.id, `${prefix}.id`),
    name: requiredString(value.name, `${prefix}.name`),
    description: requiredString(value.description, `${prefix}.description`),
    author: requiredString(value.author, `${prefix}.author`),
    category: requiredString(value.category, `${prefix}.category`),
    framework: requiredString(value.framework, `${prefix}.framework`),
    thumbnail: nullableString(value.thumbnail, `${prefix}.thumbnail`),
    version: requiredString(value.version, `${prefix}.version`),
    download: {
      url: nullableString(value.download.url, `${prefix}.download.url`),
      size_bytes: nullableNonNegativeNumber(
        value.download.size_bytes,
        `${prefix}.download.size_bytes`
      ),
    },
    created_at: requiredString(value.created_at, `${prefix}.created_at`),
    updated_at: requiredString(value.updated_at, `${prefix}.updated_at`),
  };
}

export function parseTemplateListResponse(raw: string): TemplateListResponse {
  let value: unknown;
  try {
    value = JSON.parse(raw) as unknown;
  } catch {
    throw new Error('Invalid template response: expected JSON');
  }

  if (!isRecord(value) || !Array.isArray(value.templates)) {
    throw new Error('Invalid template response: templates must be an array');
  }
  if (typeof value.total !== 'number' || !Number.isInteger(value.total) || value.total < 0) {
    throw new Error('Invalid template response: total must be a non-negative integer');
  }

  return {
    templates: value.templates.map((template, index) => parseTemplate(template, index)),
    total: value.total,
  };
}

export function parseTemplateDetailsResponse(raw: string): CommunityTemplate {
  let value: unknown;
  try {
    value = JSON.parse(raw) as unknown;
  } catch {
    throw new Error('Invalid template response: expected JSON');
  }
  return parseTemplate(value);
}

export async function fetchCommunityTemplates(
  params: TemplateListParams = {}
): Promise<TemplateListResponse> {
  const args: Record<string, unknown> = { ...params };
  const apiBaseUrl = configuredApiBase();
  if (apiBaseUrl) args.apiBaseUrl = apiBaseUrl;
  const raw = await invoke<string>('fetch_community_templates', args);
  return parseTemplateListResponse(raw);
}

export async function fetchTemplateDetails(id: string): Promise<CommunityTemplate> {
  const args: Record<string, unknown> = { id };
  const apiBaseUrl = configuredApiBase();
  if (apiBaseUrl) args.apiBaseUrl = apiBaseUrl;
  const raw = await invoke<string>('fetch_template_details', args);
  return parseTemplateDetailsResponse(raw);
}

export async function downloadTemplateZip(url: string): Promise<string> {
  return invoke<string>('download_template_zip', { url });
}

/**
 * CreateProject tests — "Start from Template" tab.
 *
 * Covers the split between bundled starters and the community template
 * gallery. The gallery is backed by the CripCode adapter and may be empty
 * while the API is unavailable or not configured.
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { CreateProject } from './CreateProject';

// useProjectCreation attaches Tauri drag-drop listeners on mount; stub `listen`
// so the jsdom render doesn't reach for real event IPC.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('../../lib/templates', () => ({
  fetchCommunityTemplates: vi.fn().mockResolvedValue({ templates: [], total: 0 }),
  downloadTemplateZip: vi.fn(),
  templateDownloadUrl: vi.fn(),
}));

describe('CreateProject template tabs', () => {
  it('renders local built-in templates under Start from Scratch', async () => {
    render(<CreateProject onComplete={vi.fn()} onCancel={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Start from Scratch' })).toBeInTheDocument();
    expect(await screen.findByText('Next.js (Tailwind)')).toBeInTheDocument();
    expect(screen.getByText('Shopify Theme')).toBeInTheDocument();
    expect(screen.getByText('Eve Agent')).toBeInTheDocument();
  });

  it('renders the CripCode community gallery under Start from Template', async () => {
    render(<CreateProject onComplete={vi.fn()} onCancel={vi.fn()} />);

    fireEvent.click(screen.getByRole('button', { name: 'Start from Template' }));

    expect(await screen.findByPlaceholderText('Search community templates...')).toBeInTheDocument();
    expect(screen.getByText('No templates found')).toBeInTheDocument();
  });
});

/**
 * CreateProject tests — "Start from Template" tab.
 *
 * Regression: the "Start from Template" tab used to render the community
 * template gallery (TemplateGallery → fetch_community_templates), whose backend
 * endpoint is neutralized (`TEMPLATES_API_URL = ""`), so it always showed
 * "No templates found". The tab must now render the same local built-in
 * `TEMPLATES` grid that "Start from Scratch" uses.
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { CreateProject } from './CreateProject';

// useProjectCreation attaches Tauri drag-drop listeners on mount; stub `listen`
// so the jsdom render doesn't reach for real event IPC.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

describe('CreateProject — Start from Template tab', () => {
  it('renders the local built-in templates instead of the community gallery', () => {
    render(<CreateProject onComplete={vi.fn()} onCancel={vi.fn()} />);

    fireEvent.click(screen.getByRole('button', { name: 'Start from Template' }));

    // Local built-in templates are visible under "Start from Template".
    expect(screen.getByText('Next.js (Tailwind)')).toBeInTheDocument();
    expect(screen.getByText('Shopify Theme')).toBeInTheDocument();
    expect(screen.getByText('Eve Agent')).toBeInTheDocument();

    // The community gallery (source of "No templates found") is no longer rendered.
    expect(screen.queryByText('No templates found')).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText('Search community templates...')).not.toBeInTheDocument();
  });
});

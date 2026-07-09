/**
 * Tests for the onboarding mode router: agent-led is the default, the classic
 * escape hatch is pinned in view at all times, and the choice persists across
 * restarts via localStorage.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { OnboardingRouter } from './OnboardingRouter';

vi.mock('./OnboardingScreen', () => ({
  OnboardingScreen: () => <div data-testid="classic-screen" />,
}));
vi.mock('./agent-led/AgentOnboardingScreen', () => ({
  AgentOnboardingScreen: () => <div data-testid="agent-screen" />,
}));
vi.mock('../../lib/analytics', () => ({
  trackEvent: vi.fn(() => Promise.resolve()),
  trackPageview: vi.fn(),
}));

describe('OnboardingRouter', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('defaults to the agent-led experience', () => {
    render(<OnboardingRouter onComplete={vi.fn()} />);
    expect(screen.getByTestId('agent-screen')).toBeInTheDocument();
    expect(screen.queryByTestId('classic-screen')).not.toBeInTheDocument();
  });

  it('always shows the classic escape hatch in agent mode', () => {
    render(<OnboardingRouter onComplete={vi.fn()} />);
    expect(screen.getByRole('button', { name: 'Try classic onboarding' })).toBeInTheDocument();
  });

  it('switches to classic and persists the choice', () => {
    render(<OnboardingRouter onComplete={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Try classic onboarding' }));
    expect(screen.getByTestId('classic-screen')).toBeInTheDocument();
    expect(localStorage.getItem('shipstudio.onboardingMode')).toBe('classic');
  });

  it('restores a persisted classic choice on mount and can switch back', () => {
    localStorage.setItem('shipstudio.onboardingMode', 'classic');
    render(<OnboardingRouter onComplete={vi.fn()} />);
    expect(screen.getByTestId('classic-screen')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Try agent-guided setup' }));
    expect(screen.getByTestId('agent-screen')).toBeInTheDocument();
    expect(localStorage.getItem('shipstudio.onboardingMode')).toBe('agent');
  });

  it('treats unknown stored values as the agent default', () => {
    localStorage.setItem('shipstudio.onboardingMode', 'garbage');
    render(<OnboardingRouter onComplete={vi.fn()} />);
    expect(screen.getByTestId('agent-screen')).toBeInTheDocument();
  });
});

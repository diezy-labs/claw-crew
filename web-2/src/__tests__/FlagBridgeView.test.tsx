import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { FlagBridgeView } from '../components/features/FlagBridgeView';
import { useFleetStore } from '../store/fleetStore';

describe('FlagBridgeView Component', () => {
  it('renders Flag Bridge title, control room subtitle, and executive promise', () => {
    render(<FlagBridgeView />);
    expect(screen.getByText('Flag Bridge')).toBeInTheDocument();
    expect(screen.getByText(/Quartermaster Control Room/i)).toBeInTheDocument();
    expect(screen.getByText(/See, steer, and decide across your Fleet/i)).toBeInTheDocument();
  });

  it('renders control room tabs and executive briefing cards', () => {
    render(<FlagBridgeView />);
    expect(screen.getByRole('button', { name: /^Overview$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Briefings$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Ship Reports$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Decisions/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Treasury$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Health$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Strategy$/ })).toBeInTheDocument();

    expect(screen.getByText('Executive Briefing')).toBeInTheDocument();
    expect(screen.getByText(/3 items need attention/i)).toBeInTheDocument();
    expect(screen.getByText('Fleet Pulse')).toBeInTheDocument();
    expect(screen.getByText('Active Voyages')).toBeInTheDocument();
    expect(screen.getByText('Quartermaster Recommendations')).toBeInTheDocument();
    expect(screen.getByText('Recent Outcomes')).toBeInTheDocument();
  });

  it('allows switching to Briefings tab and navigating with Ask QM', () => {
    render(<FlagBridgeView />);
    const briefingsTab = screen.getByRole('button', { name: /^Briefings$/ });
    fireEvent.click(briefingsTab);

    expect(screen.getByText(/Daily Briefing/i)).toBeInTheDocument();
    expect(screen.getByText(/Developer Ship:/i)).toBeInTheDocument();

    const askQmBtn = screen.getByRole('button', { name: /Ask QM/i });
    fireEvent.click(askQmBtn);
    expect(useFleetStore.getState().activeTab).toBe('quarterdeck');
  });
});

import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, beforeEach } from 'vitest';
import { SettingsView } from '../components/features/SettingsView';
import { useFleetStore } from '../store/fleetStore';

describe('SettingsView Component', () => {
  beforeEach(() => {
    useFleetStore.setState({
      activeSettingsCategory: 'profile'
    });
  });

  it('renders settings categories and profile view by default', () => {
    render(<SettingsView />);

    expect(screen.getByRole('heading', { name: /^settings$/i })).toBeInTheDocument();
    expect(screen.getAllByText('Pirate King Profile').length).toBeGreaterThan(0);
    expect(screen.getByText('Appearance & Language')).toBeInTheDocument();
    expect(screen.getByText('Defaults & Preferences')).toBeInTheDocument();
  });

  it('allows switching to Appearance category', () => {
    render(<SettingsView />);

    const appearanceBtn = screen.getByRole('button', { name: /Appearance & Language/i });
    fireEvent.click(appearanceBtn);

    expect(useFleetStore.getState().activeSettingsCategory).toBe('appearance');
  });

  it('allows filtering categories via search bar', () => {
    render(<SettingsView />);

    const searchInput = screen.getByPlaceholderText(/search settings/i);
    fireEvent.change(searchInput, { target: { value: 'Developer' } });

    expect(screen.getByText('Developer Preferences')).toBeInTheDocument();
  });
});

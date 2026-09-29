import React from 'react';
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { QuestsView } from '../components/features/QuestsView';

describe('QuestsView Component', () => {
  it('renders Quests workspace header and project rail', () => {
    render(<QuestsView />);
    expect(screen.getByText('Quests')).toBeInTheDocument();
    expect(screen.getByText(/Workspace & Project SOPs/i)).toBeInTheDocument();
    expect(screen.getByText(/Workspaces/i)).toBeInTheDocument();
    expect(screen.getByText(/Initiative Projects/i)).toBeInTheDocument();
  });

  it('renders subtabs and map studio mode controls', () => {
    render(<QuestsView />);
    expect(screen.getByText('Active Quests')).toBeInTheDocument();
    expect(screen.getByText('Planned / Drafts')).toBeInTheDocument();
    expect(screen.getByText('Guided Map (Steps)')).toBeInTheDocument();
    expect(screen.getByText('Advanced Studio (Nodes)')).toBeInTheDocument();
  });

  it('renders new quest launcher button', () => {
    render(<QuestsView />);
    expect(screen.getByText(/New Quest/i)).toBeInTheDocument();
  });
});

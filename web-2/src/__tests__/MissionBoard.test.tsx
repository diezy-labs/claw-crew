import React from 'react';
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { MissionBoard } from '../components/features/MissionBoard';

describe('MissionBoard Component', () => {
  it('renders Kan-ban columns: Backlog, Ready to Sail, Underway, Awaiting Captain, Treasures Claimed', () => {
    render(<MissionBoard />);
    expect(screen.getByText('Backlog')).toBeInTheDocument();
    expect(screen.getByText('Ready to Sail')).toBeInTheDocument();
    expect(screen.getByText('Underway')).toBeInTheDocument();
    expect(screen.getByText('Awaiting Captain')).toBeInTheDocument();
    expect(screen.getByText('Treasures Claimed')).toBeInTheDocument();
  });

  it('renders filter controls and new quest button', () => {
    render(<MissionBoard />);
    expect(screen.getByPlaceholderText(/Filter quests.../i)).toBeInTheDocument();
    expect(screen.getByText(/New Quest/i)).toBeInTheDocument();
  });
});

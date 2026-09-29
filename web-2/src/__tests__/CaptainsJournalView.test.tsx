import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { CaptainsJournalView } from '../components/features/CaptainsJournalView';

describe('CaptainsJournalView Component', () => {
  it('renders Captain’s Journal header and private tag', () => {
    render(<CaptainsJournalView />);
    expect(screen.getByText(/Captain’s Journal/i)).toBeInTheDocument();
    expect(screen.getByText(/Private & Exploratory/i)).toBeInTheDocument();
    expect(screen.getByText(/Your private conversations, notes/i)).toBeInTheDocument();
  });

  it('renders session list with pinned and recent sessions', () => {
    render(<CaptainsJournalView />);
    expect(screen.getByText(/Pinned Discussions/i)).toBeInTheDocument();
    expect(screen.getByText(/Recent Sessions/i)).toBeInTheDocument();
    expect(screen.getAllByText(/Product Direction & Multi-Ship Capacity/i)[0]).toBeInTheDocument();
  });

  it('renders journal composer and conversion action buttons', () => {
    render(<CaptainsJournalView />);
    expect(screen.getByText(/Save as Artifact/i)).toBeInTheDocument();
    expect(screen.getByText(/Create Quest/i)).toBeInTheDocument();
    const input = screen.getByPlaceholderText(/Write private notes or consult with Quartermaster/i);
    expect(input).toBeInTheDocument();
  });
});

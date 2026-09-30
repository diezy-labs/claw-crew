import React from 'react';
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { ApprovalsView } from '../components/features/ApprovalsView';

describe('ApprovalsView Component', () => {
  it('renders Captain’s Approval header and review queue', () => {
    render(<ApprovalsView />);
    expect(screen.getByText(/Captain’s Approval/i)).toBeInTheDocument();
    expect(screen.getByText(/Pending Verification/i)).toBeInTheDocument();
  });

  it('renders pending approval items with target and action details', () => {
    render(<ApprovalsView />);
    const approveButtons = screen.queryAllByText(/Approve & Sign Action/i);
    const rejectButtons = screen.queryAllByText(/Reject Action/i);

    expect(approveButtons.length).toBeGreaterThan(0);
    expect(rejectButtons.length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Why Now\?/i)[0]).toBeInTheDocument();
    expect(screen.getAllByText(/Exact Side Effect & Scope Boundary/i)[0]).toBeInTheDocument();
  });
});

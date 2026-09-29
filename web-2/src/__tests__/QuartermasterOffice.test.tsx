import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { QuartermasterOffice } from '../components/features/QuartermasterOffice';

describe('QuartermasterOffice Component', () => {
  it('renders executive desk greeting and quick starts', () => {
    render(<QuartermasterOffice />);
    expect(screen.getAllByText(/Good morning, Pirate King/i)[0]).toBeInTheDocument();
    expect(screen.getByText(/QUARTERMASTER OFFICE — EXECUTIVE DESK/i)).toBeInTheDocument();
    expect(screen.getByText(/Prepare Release v1.4/i)).toBeInTheDocument();
    expect(screen.getByText(/Triage CI Teardown Hang/i)).toBeInTheDocument();
  });

  it('renders briefing summary cards', () => {
    render(<QuartermasterOffice />);
    expect(screen.getByText(/Executive Briefing Summary/i)).toBeInTheDocument();
    expect(screen.getByText(/1 Release Blocker/i)).toBeInTheDocument();
    expect(screen.getByText(/3 Artifacts Ready/i)).toBeInTheDocument();
  });

  it('allows typing and submitting a command in input field', () => {
    render(<QuartermasterOffice />);
    const input = screen.getByPlaceholderText(/Ask Quartermaster, propose a goal/i);
    expect(input).toBeInTheDocument();

    fireEvent.change(input, { target: { value: 'Audit our CI pipeline' } });
    expect((input as HTMLInputElement).value).toBe('Audit our CI pipeline');
  });
});

import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { QuarterdeckView } from '../components/features/QuarterdeckView';

describe('QuarterdeckView Component', () => {
  it('renders Quarterdeck command console and executive greeting', () => {
    render(<QuarterdeckView />);
    expect(screen.getByText(/QUARTERDECK — QUARTERMASTER COMMAND CONSOLE/i)).toBeInTheDocument();
    expect(screen.getByText(/Good evening, Pirate King/i)).toBeInTheDocument();
  });

  it('renders decision cards and ship reports in conversational flow', () => {
    render(<QuarterdeckView />);
    expect(screen.getByText(/Owner Decision Needed/i)).toBeInTheDocument();
    expect(screen.getByText(/Approve GitHub Issue Draft\?/i)).toBeInTheDocument();
    expect(screen.getByText(/Developer Ship Report/i)).toBeInTheDocument();
  });

  it('renders Quartermaster composer and context buttons', () => {
    render(<QuarterdeckView />);
    expect(screen.getByText(/\+ Workspace/i)).toBeInTheDocument();
    expect(screen.getByText(/\+ Project/i)).toBeInTheDocument();
    expect(screen.getByText(/\+ Developer Ship/i)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/Ask Quartermaster.../i)).toBeInTheDocument();
  });

  it('allows user to select AI model and adjust effort from low to high', () => {
    render(<QuarterdeckView />);

    // Check default model and effort display using getAllByText
    expect(screen.getAllByText('Claude 3.7 Sonnet').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText(/High/i).length).toBeGreaterThanOrEqual(1);

    // Open Effort selector and select Low effort
    const effortBtn = screen.getByTitle(/Thinking \/ Reasoning Effort/i);
    fireEvent.click(effortBtn);

    const lowEffortBtn = screen.getByText('Low Effort');
    fireEvent.click(lowEffortBtn);

    // Effort should now show low
    expect(screen.getAllByText(/Low/i).length).toBeGreaterThanOrEqual(1);

    // Open Model selector and select Gemini 2.5 Pro
    const modelBtn = screen.getByTitle('Select AI Model');
    fireEvent.click(modelBtn);

    const geminiOption = screen.getByText('Gemini 2.5 Pro');
    fireEvent.click(geminiOption);

    // Model name should now be updated to Gemini 2.5 Pro
    expect(screen.getAllByText('Gemini 2.5 Pro').length).toBeGreaterThanOrEqual(1);
  });
});

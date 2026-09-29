import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, beforeEach } from 'vitest';
import { AppSidebar } from '../components/layout/AppSidebar';
import { useFleetStore } from '../store/fleetStore';

describe('AppSidebar Navigation Component', () => {
  beforeEach(() => {
    useFleetStore.setState({
      activeTab: 'quarterdeck',
      isSidebarCollapsed: false,
      isMobileSidebarOpen: false,
      isAnchorDropped: false
    });
  });

  it('renders all four domain navigation sections and core surfaces', () => {
    render(<AppSidebar />);

    // Domain sections
    expect(screen.getByText('COMMAND')).toBeInTheDocument();
    expect(screen.getByText('FLEET')).toBeInTheDocument();
    expect(screen.getByText('OPERATIONS')).toBeInTheDocument();
    expect(screen.getByText('CONTROL')).toBeInTheDocument();

    // Command items
    expect(screen.getByText('Quarterdeck')).toBeInTheDocument();
    expect(screen.getByText('Quests')).toBeInTheDocument();
    expect(screen.getByText('Captain’s Journal')).toBeInTheDocument();

    // Fleet items
    expect(screen.getByText('Mission Board')).toBeInTheDocument();
    expect(screen.getByText('Ships')).toBeInTheDocument();
    expect(screen.getByText('Crew Members')).toBeInTheDocument();
    expect(screen.getByText('Artifacts')).toBeInTheDocument();
    expect(screen.getByText('Captain’s Approval')).toBeInTheDocument();

    // Operations items
    expect(screen.getByText('Treasury')).toBeInTheDocument();
    expect(screen.getByText('Logbook')).toBeInTheDocument();
    expect(screen.getByText('Harbor')).toBeInTheDocument();

    // Control items
    expect(screen.getByText('Fleet Code')).toBeInTheDocument();
    expect(screen.getByText('Crow’s Nest')).toBeInTheDocument();
    expect(screen.getByText('Shipyard')).toBeInTheDocument();

    // Settings
    expect(screen.getByText('Settings')).toBeInTheDocument();
  });

  it('navigates when clicking a menu item', () => {
    render(<AppSidebar />);

    const treasuryBtn = screen.getByText('Treasury');
    fireEvent.click(treasuryBtn);

    expect(useFleetStore.getState().activeTab).toBe('treasury');
  });

  it('displays Drop Anchor modal and toggles anchor state without window.alert', () => {
    render(<AppSidebar />);

    const anchorBtn = screen.getByText('Drop Anchor (Pause All)');
    fireEvent.click(anchorBtn);

    // Modal should be visible
    expect(screen.getByText('Drop Anchor (Emergency Pause All)?')).toBeInTheDocument();

    const confirmBtn = screen.getByText('Confirm Emergency Halt');
    fireEvent.click(confirmBtn);

    expect(useFleetStore.getState().isAnchorDropped).toBe(true);
  });
});

import React, { Component, ErrorInfo, ReactNode } from 'react';
import { AppSidebar } from './AppSidebar';
import { Navbar } from '../common/Navbar';
import { RightRail } from './RightRail';
import { CommandPalette } from './CommandPalette';
import { RemoteAccessModal } from '../features/RemoteAccessModal';
import { useFleetStore } from '../../store/fleetStore';
import { useSimulation } from '../../hooks/useSimulation';

import { QuarterdeckView } from '../features/QuarterdeckView';
import { RealmView } from '../features/RealmView';
import { FlagBridgeView } from '../features/FlagBridgeView';
import { QuestsView } from '../features/QuestsView';
import { CaptainsJournalView } from '../features/CaptainsJournalView';
import { QuartermasterOffice } from '../features/QuartermasterOffice';
import { MissionBoard } from '../features/MissionBoard';
import { ShipsView } from '../features/ShipsView';
import { SquadsView } from '../features/SquadsView';
import { CrewView } from '../features/CrewView';
import { ArtifactsView } from '../features/ArtifactsView';
import { ApprovalsView } from '../features/ApprovalsView';
import { TreasuryView } from '../features/TreasuryView';
import { LogbookView } from '../features/LogbookView';
import { HarborView } from '../features/HarborView';
import { TrainingOfficerView } from '../features/TrainingOfficerView';
import { FleetCodeView } from '../features/FleetCodeView';
import { CrowsNestView } from '../features/CrowsNestView';
import { EngineRoomView } from '../features/EngineRoomView';
import { ShipyardView } from '../features/ShipyardView';
import { SettingsView } from '../features/SettingsView';

interface ErrorBoundaryProps {
  children: ReactNode;
  activeTab: string;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

class ViewErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Error rendering Fleet View:', error, errorInfo);
  }

  componentDidUpdate(prevProps: ErrorBoundaryProps) {
    if (prevProps.activeTab !== this.props.activeTab && this.state.hasError) {
      this.setState({ hasError: false, error: null });
    }
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex-1 flex flex-col items-center justify-center p-6 text-center space-y-4">
          <div className="w-12 h-12 rounded-xl bg-amber-500/10 border border-amber-500/30 flex items-center justify-center text-amber-500 text-xl font-bold font-mono">
            ⚠
          </div>
          <div className="space-y-1">
            <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
              Fleet View Recovered
            </h3>
            <p className="text-xs text-neutral-400 max-w-md">
              {this.state.error?.message || 'An unexpected state occurred while rendering this surface.'}
            </p>
          </div>
          <button
            onClick={() => this.setState({ hasError: false, error: null })}
            className="px-3 py-1.5 rounded-lg bg-teal-600 hover:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold transition-colors cursor-pointer"
          >
            Reload Surface
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}

export const AppShell: React.FC = () => {
  React.useEffect(() => {
    useFleetStore.getState().fetchRealData();
  }, []);
  const {
    activeTab,
    setActiveTab,
    toggleSidebarCollapsed,
    isRemoteAccessModalOpen,
    setRemoteAccessModalOpen
  } = useFleetStore();
  useSimulation();

  React.useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // ⌘, or Ctrl+, opens Settings
      if ((e.metaKey || e.ctrlKey) && e.key === ',') {
        e.preventDefault();
        setActiveTab('settings');
      }
      // ⌘B or Ctrl+B toggles sidebar collapse
      if ((e.metaKey || e.ctrlKey) && (e.key === 'b' || e.key === 'B')) {
        e.preventDefault();
        toggleSidebarCollapsed();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [setActiveTab, toggleSidebarCollapsed]);

  const renderActiveView = () => {
    switch (activeTab) {
      case 'quarterdeck':
      case 'quartermaster':
        return <QuarterdeckView />;
      case 'realm':
        return <RealmView />;
      case 'flag-bridge':
        return <FlagBridgeView />;
      case 'quests':
        return <QuestsView />;
      case 'captains-journal':
        return <CaptainsJournalView />;
      case 'mission-board':
        return <MissionBoard />;
      case 'ships':
        return <ShipsView />;
      case 'squads':
        return <SquadsView />;
      case 'crew':
        return <CrewView />;
      case 'artifacts':
        return <ArtifactsView />;
      case 'approvals':
        return <ApprovalsView />;
      case 'treasury':
        return <TreasuryView />;
      case 'logbook':
        return <LogbookView />;
      case 'harbor':
        return <HarborView />;
      case 'training-officer':
        return <TrainingOfficerView />;
      case 'fleet-code':
        return <FleetCodeView />;
      case 'crows-nest':
        return <CrowsNestView />;
      case 'engine-room':
        return <EngineRoomView />;
      case 'shipyard':
        return <ShipyardView />;
      case 'settings':
        return <SettingsView />;
      default:
        return <QuartermasterOffice />;
    }
  };

  return (
    <div className="flex h-screen h-[100dvh] w-screen overflow-hidden bg-[var(--bg-canvas)] text-[var(--text-primary)]">
      {/* Sidebar */}
      <AppSidebar />

      {/* Main Content Area */}
      <div className="flex-1 flex flex-col min-w-0 h-full overflow-hidden">
        {/* Standard General Navbar */}
        <Navbar />

        {/* Dynamic Canvas + Optional Right Rail */}
        <div className="flex-1 flex overflow-hidden">
          <main className="flex-1 overflow-hidden flex flex-col">
            <ViewErrorBoundary activeTab={activeTab}>
              <div key={activeTab} className="flex-1 flex flex-col overflow-hidden animate-view-fade-in">
                {renderActiveView()}
              </div>
            </ViewErrorBoundary>
          </main>

          {/* Right Rail Fleet Pulse (Shown on wide screens) */}
          <RightRail />
        </div>
      </div>

      {/* Global Command Palette */}
      <CommandPalette />

      {/* Global Remote Access & Multi-Platform QR Modal */}
      <RemoteAccessModal
        isOpen={isRemoteAccessModalOpen}
        onClose={() => setRemoteAccessModalOpen(false)}
      />
    </div>
  );
};


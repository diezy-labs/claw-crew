import React, { Suspense, lazy } from 'react';
import { AppSidebar } from './AppSidebar';
import { ContextBar } from './ContextBar';
import { RightRail } from './RightRail';
import { CommandPalette } from './CommandPalette';
import { useFleetStore } from '../../store/fleetStore';
import { useSimulation } from '../../hooks/useSimulation';

// Lazy loading feature views for optimal performance and initial bundle size
const QuarterdeckView = lazy(() =>
  import('../features/QuarterdeckView').then((m) => ({ default: m.QuarterdeckView }))
);
const RealmView = lazy(() =>
  import('../features/RealmView').then((m) => ({ default: m.RealmView }))
);
const FlagBridgeView = lazy(() =>
  import('../features/FlagBridgeView').then((m) => ({ default: m.FlagBridgeView }))
);
const QuestsView = lazy(() =>
  import('../features/QuestsView').then((m) => ({ default: m.QuestsView }))
);
const CaptainsJournalView = lazy(() =>
  import('../features/CaptainsJournalView').then((m) => ({ default: m.CaptainsJournalView }))
);
const QuartermasterOffice = lazy(() =>
  import('../features/QuartermasterOffice').then((m) => ({ default: m.QuartermasterOffice }))
);
const MissionBoard = lazy(() =>
  import('../features/MissionBoard').then((m) => ({ default: m.MissionBoard }))
);
const ShipsView = lazy(() =>
  import('../features/ShipsView').then((m) => ({ default: m.ShipsView }))
);
const CrewView = lazy(() =>
  import('../features/CrewView').then((m) => ({ default: m.CrewView }))
);
const ArtifactsView = lazy(() =>
  import('../features/ArtifactsView').then((m) => ({ default: m.ArtifactsView }))
);
const ApprovalsView = lazy(() =>
  import('../features/ApprovalsView').then((m) => ({ default: m.ApprovalsView }))
);
const TreasuryView = lazy(() =>
  import('../features/TreasuryView').then((m) => ({ default: m.TreasuryView }))
);
const LogbookView = lazy(() =>
  import('../features/LogbookView').then((m) => ({ default: m.LogbookView }))
);
const HarborView = lazy(() =>
  import('../features/HarborView').then((m) => ({ default: m.HarborView }))
);
const FleetCodeView = lazy(() =>
  import('../features/FleetCodeView').then((m) => ({ default: m.FleetCodeView }))
);
const CrowsNestView = lazy(() =>
  import('../features/CrowsNestView').then((m) => ({ default: m.CrowsNestView }))
);
const ShipyardView = lazy(() =>
  import('../features/ShipyardView').then((m) => ({ default: m.ShipyardView }))
);
const SettingsView = lazy(() =>
  import('../features/SettingsView').then((m) => ({ default: m.SettingsView }))
);

export const AppShell: React.FC = () => {
  const { activeTab, setActiveTab, toggleSidebarCollapsed } = useFleetStore();
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
      case 'fleet-code':
        return <FleetCodeView />;
      case 'crows-nest':
        return <CrowsNestView />;
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
        {/* Context Breadcrumb Top Bar */}
        <ContextBar />

        {/* Dynamic Canvas + Optional Right Rail */}
        <div className="flex-1 flex overflow-hidden">
          <main className="flex-1 overflow-hidden flex flex-col">
            <Suspense
              fallback={
                <div className="flex-1 flex items-center justify-center p-8 text-neutral-400 font-mono text-xs">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-teal-500 animate-ping" />
                    <span>Loading Fleet surface...</span>
                  </div>
                </div>
              }
            >
              <div key={activeTab} className="flex-1 flex flex-col overflow-hidden animate-view-fade-in">
                {renderActiveView()}
              </div>
            </Suspense>
          </main>

          {/* Right Rail Fleet Pulse (Shown on wide screens) */}
          <RightRail />
        </div>
      </div>

      {/* Global Command Palette */}
      <CommandPalette />
    </div>
  );
};

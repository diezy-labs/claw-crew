import React, { useState } from 'react';
import {
  Ship,
  Users,
  Compass,
  Coins,
  Shield,
  FileText,
  AlertTriangle,
  Play,
  ArrowRight,
  CheckCircle2,
  Lock,
  Plus,
  Sparkles,
  Check,
  Layers
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { PageHeader } from '../common/PageHeader';
import { PageStickyNav } from '../common/PageStickyNav';
import { SubMenuScroller } from '../common/SubMenuScroller';

const AVAILABLE_SQUADS = [
  {
    name: 'Core Engineering Squad',
    crewCount: 3,
    description: 'Autonomous AST refactoring, API integration, and architectural evolution.'
  },
  {
    name: 'Quality & Test Assurance Squad',
    crewCount: 2,
    description: 'Deterministic regression testing, socket timeout triage, and test suite automation.'
  },
  {
    name: 'Security & Policy Guard Squad',
    crewCount: 2,
    description: 'Credential rotation verification, risk gate enforcement, and secret leak scanning.'
  },
  {
    name: 'Release Delivery & SRE Squad',
    crewCount: 3,
    description: 'CI/CD pipeline staging, release tag coordination, and runtime health telemetry.'
  }
];

export const ShipsView: React.FC = () => {
  const { ships, crew, quests, artifacts, setActiveTab, createQuest, createShip } = useFleetStore();
  const [selectedShipId, setSelectedShipId] = useState<string>('ship-dev');
  const [isCraftShipOpen, setIsCraftShipOpen] = useState(false);

  // Craft ship form state
  const [shipName, setShipName] = useState('');
  const [navigatorName, setNavigatorName] = useState('');
  const [tagline, setTagline] = useState('');
  const [selectedSquads, setSelectedSquads] = useState<string[]>([AVAILABLE_SQUADS[0].name, AVAILABLE_SQUADS[1].name]);
  const [selectedCrewIds, setSelectedCrewIds] = useState<string[]>(['crew-repo-analyst', 'crew-eng-planner']);

  const selectedShip = ships.find((s) => s.id === selectedShipId) || ships[0];
  const shipCrew = crew.filter((c) => c.shipId === selectedShip.id);
  const shipQuests = quests.filter((q) => q.assignedShipId === selectedShip.id || q.suggestedShipId === selectedShip.id);
  const shipArtifacts = artifacts.filter((a) => a.shipId === selectedShip.id);

  const toggleSquad = (squadName: string) => {
    setSelectedSquads((prev) =>
      prev.includes(squadName) ? prev.filter((s) => s !== squadName) : [...prev, squadName]
    );
  };

  const toggleCrew = (crewId: string) => {
    setSelectedCrewIds((prev) =>
      prev.includes(crewId) ? prev.filter((id) => id !== crewId) : [...prev, crewId]
    );
  };

  const handleCraftShipSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!shipName.trim()) return;

    const newShipId = createShip({
      name: shipName.trim(),
      navigatorName: navigatorName.trim() || 'Orchestrator Navigator',
      tagline: tagline.trim() || `Vessel operating ${selectedSquads.join(', ')} with ${selectedCrewIds.length} specialist seats.`,
      homeScope: 'engineering',
      crewIds: selectedCrewIds,
      charter: {
        purpose: tagline.trim() || 'Continuous mission execution across assigned engineering squads.',
        acceptedQuestTypes: ['repository_health', 'ci_triage', 'feature_delivery', 'release_readiness'],
        crewAuthority: 'Autonomous reading and staging. Impactful external writes require Captain’s Approval.',
        prohibitedActions: ['Direct production deploys without Captain sign-off'],
        budgetPerVoyageUSD: 2.00,
        monthlyBudgetUSD: 35.00,
        memorySharing: 'ship_scoped'
      }
    });

    setSelectedShipId(newShipId);
    setIsCraftShipOpen(false);
    // Reset form
    setShipName('');
    setNavigatorName('');
    setTagline('');
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 gap-6 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Standard Reusable PageHeader */}
      <PageHeader
        icon={<Ship className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Ships & Squads"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            {ships.length} Persistent Teams
          </span>
        }
        description="A Ship is an operational home for a persistent specialist AI team with its own Charter, Navigator, and memory."
        actions={
          <button
            onClick={() => setIsCraftShipOpen(true)}
            className="flex items-center gap-1 sm:gap-1.5 px-2.5 sm:px-3.5 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer shadow-xs shrink-0"
          >
            <Plus className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">+ Craft a Ship</span>
            <span className="sm:hidden">Craft</span>
          </button>
        }
      />

      {/* Floating Sticky Sub-Tabs with Navigation Arrows (< >) */}
      <PageStickyNav>
        <SubMenuScroller className="gap-2" containerClassName="w-full">
          {ships.map((ship) => {
            const isSelected = ship.id === selectedShip.id;
            return (
              <button
                key={ship.id}
                onClick={() => setSelectedShipId(ship.id)}
                className={`flex items-center gap-2 px-3.5 py-2 rounded-xl text-xs font-medium transition-all shrink-0 cursor-pointer ${
                  isSelected
                    ? 'bg-teal-500/10 dark:bg-teal-500/15 text-teal-700 dark:text-teal-300 font-semibold border border-teal-500/30 shadow-2xs'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white border border-neutral-200 dark:border-neutral-800 bg-white/60 dark:bg-[#181a1d] hover:border-neutral-300 dark:hover:border-neutral-700'
                }`}
              >
                <Ship className={`w-3.5 h-3.5 ${isSelected ? 'text-teal-600 dark:text-teal-400' : 'text-neutral-400'}`} />
                <span className="whitespace-nowrap">{ship.name}</span>
                <span className={`text-[10px] font-mono px-1.5 py-0.2 rounded ${isSelected ? 'bg-teal-500/20 text-teal-800 dark:text-teal-200' : 'bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400'}`}>
                  {ship.crewIds.length} Crew
                </span>
              </button>
            );
          })}
        </SubMenuScroller>
      </PageStickyNav>

      {/* Selected Ship Showcase */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left Column: Navigator & Status */}
        <div className="flex flex-col gap-5 lg:col-span-2">
          {/* Navigator Briefing Box */}
          <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Compass className="w-4 h-4 text-teal-500" />
                <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                  Navigator Briefing · {selectedShip.navigatorName}
                </span>
              </div>
              <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-emerald-500/20 text-emerald-500 font-semibold">
                ACTIVE
              </span>
            </div>

            <p className="text-xs text-neutral-800 dark:text-neutral-200 leading-relaxed font-medium">
              &ldquo;Currently overseeing {shipQuests.length} assigned Quests. QA &amp; Risk Reviewer has isolated the CI teardown bug, and repository health metrics are pristine. Waiting on Captain&rsquo;s Approval before submitting the GitHub draft issue.&rdquo;
            </p>

            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={() => setActiveTab('mission-board')}
                className="px-2.5 py-1 text-xs rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-700 dark:text-neutral-300 hover:border-teal-500 transition-colors cursor-pointer"
              >
                Inspect Quests →
              </button>
              <button
                onClick={() => setActiveTab('approvals')}
                className="px-2.5 py-1 text-xs rounded-lg bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/30 hover:bg-amber-500/20 transition-colors cursor-pointer"
              >
                Review Blocker Approval
              </button>
            </div>
          </div>

          {/* Crew Specialists Roster */}
          <div className="flex flex-col gap-2.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                Assigned Specialist Crew ({shipCrew.length} / 5 Berths)
              </span>
              <button
                onClick={() => setActiveTab('crew')}
                className="text-xs text-teal-600 dark:text-teal-400 hover:underline cursor-pointer"
              >
                Manage Crew →
              </button>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              {shipCrew.map((member) => (
                <div
                  key={member.id}
                  className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex flex-col gap-2 text-xs"
                >
                  <div className="flex items-start justify-between">
                    <div>
                      <div className="font-semibold text-neutral-900 dark:text-neutral-100">
                        {member.name}
                      </div>
                      <div className="text-[11px] text-neutral-500 dark:text-neutral-400">
                        {member.role}
                      </div>
                    </div>
                    <span className="text-[10px] font-mono px-1 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-400">
                      {member.authority}
                    </span>
                  </div>

                  <p className="text-[11px] text-neutral-500 dark:text-neutral-400 line-clamp-2">
                    {member.purpose}
                  </p>

                  <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-between text-[10px] text-neutral-400 font-mono">
                    <span>{member.modelProfile.split(' ')[0]}</span>
                    <span>${member.costLast30Days.toFixed(2)} cost</span>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* Active Quests & Deliverables */}
          <div className="flex flex-col gap-2.5">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Active Quests Underway
            </span>

            <div className="flex flex-col gap-2">
              {shipQuests.map((q) => (
                <div
                  key={q.id}
                  className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex items-center justify-between gap-3 text-xs"
                >
                  <div>
                    <div className="font-semibold text-neutral-900 dark:text-neutral-100">
                      {q.title}
                    </div>
                    <div className="text-[11px] text-neutral-500 dark:text-neutral-400">
                      {q.objective}
                    </div>
                  </div>
                  <span className="text-[10px] font-mono px-2 py-1 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 uppercase shrink-0">
                    {q.status}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Right Column: Readable Ship Charter */}
        <div className="flex flex-col gap-4">
          <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-[#15171a] flex flex-col gap-4 text-xs">
            <div className="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3">
              <span className="font-bold text-neutral-900 dark:text-neutral-100 text-sm">
                Ship Charter
              </span>
              <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-teal-500/20 text-teal-600 dark:text-teal-400">
                LIVING MANIFEST
              </span>
            </div>

            <div className="space-y-1">
              <span className="text-[11px] font-semibold text-neutral-500 dark:text-neutral-400 uppercase tracking-wider">
                What this Ship does
              </span>
              <p className="text-neutral-800 dark:text-neutral-200 leading-relaxed">
                {selectedShip.charter.purpose}
              </p>
            </div>

            <div className="space-y-1">
              <span className="text-[11px] font-semibold text-neutral-500 dark:text-neutral-400 uppercase tracking-wider">
                Accepted Quest Types
              </span>
              <div className="flex flex-wrap gap-1">
                {selectedShip.charter.acceptedQuestTypes.map((qt) => (
                  <span
                    key={qt}
                    className="px-2 py-0.5 rounded bg-neutral-200 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 font-mono text-[10px]"
                  >
                    {qt}
                  </span>
                ))}
              </div>
            </div>

            <div className="space-y-1">
              <span className="text-[11px] font-semibold text-neutral-500 dark:text-neutral-400 uppercase tracking-wider">
                Crew Authority Boundary
              </span>
              <p className="text-neutral-700 dark:text-neutral-300 leading-relaxed">
                {selectedShip.charter.crewAuthority}
              </p>
            </div>

            <div className="space-y-1">
              <span className="text-[11px] font-semibold text-rose-500 uppercase tracking-wider flex items-center gap-1">
                <Lock className="w-3 h-3" />
                Prohibited Actions
              </span>
              <ul className="space-y-1 text-neutral-600 dark:text-neutral-400 list-disc list-inside">
                {selectedShip.charter.prohibitedActions.map((pa, idx) => (
                  <li key={idx} className="line-clamp-1">{pa}</li>
                ))}
              </ul>
            </div>

            <div className="pt-2 border-t border-neutral-200 dark:border-neutral-800 grid grid-cols-2 gap-2 font-mono">
              <div>
                <span className="text-[10px] text-neutral-400 block">Voyage Cap</span>
                <span className="font-semibold text-neutral-800 dark:text-neutral-200">
                  ${selectedShip.charter.budgetPerVoyageUSD.toFixed(2)}
                </span>
              </div>
              <div>
                <span className="text-[10px] text-neutral-400 block">Monthly Budget</span>
                <span className="font-semibold text-teal-600 dark:text-teal-400">
                  ${selectedShip.charter.monthlyBudgetUSD.toFixed(2)}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Craft a Ship Modal */}
      {isCraftShipOpen && (
        <div
          onClick={() => setIsCraftShipOpen(false)}
          className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-150 cursor-pointer"
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-2xl rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1e] shadow-2xl p-5 sm:p-6 space-y-5 max-h-[90vh] overflow-y-auto scrollbar-none cursor-default"
          >
            {/* Modal Header without X */}
            <div className="flex items-center gap-3 border-b border-neutral-200 dark:border-neutral-800 pb-4">
              <div className="w-10 h-10 rounded-xl bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center border border-teal-500/20 shrink-0">
                <Ship className="w-5 h-5" />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <h3 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                    Craft a Ship — Commission Vessel
                  </h3>
                  <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-teal-500/15 text-teal-600 dark:text-teal-400 border border-teal-500/30">
                    Squads &amp; Crew
                  </span>
                </div>
                <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Commission an operational container for multiple specialist squads and assigned AI crew members.
                </p>
              </div>
            </div>

            {/* Form */}
            <form onSubmit={handleCraftShipSubmit} className="space-y-4 text-xs">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3.5">
                <div>
                  <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                    Ship Vessel Name
                  </label>
                  <input
                    type="text"
                    required
                    value={shipName}
                    onChange={(e) => setShipName(e.target.value)}
                    placeholder="e.g. Velocity SRE Frigate"
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                  />
                </div>

                <div>
                  <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                    Navigator AI (Orchestrator)
                  </label>
                  <input
                    type="text"
                    required
                    value={navigatorName}
                    onChange={(e) => setNavigatorName(e.target.value)}
                    placeholder="e.g. Atlas (Orchestrator)"
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                  />
                </div>
              </div>

              <div>
                <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1">
                  Vessel Purpose &amp; Mission Charter
                </label>
                <textarea
                  rows={2}
                  value={tagline}
                  onChange={(e) => setTagline(e.target.value)}
                  placeholder="e.g. Dedicated container for microservice performance testing, canary auditing, and automated rollback."
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 resize-none"
                />
              </div>

              <div>
                <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1.5">
                  1. Form Specialist Squads (Multi-Squad Capacity)
                </label>
                <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mb-2">
                  Select which squads this vessel will house. Each squad focuses on a distinct operational charter.
                </p>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                  {AVAILABLE_SQUADS.map((sq) => {
                    const isSelected = selectedSquads.includes(sq.name);
                    return (
                      <div
                        key={sq.name}
                        onClick={() => toggleSquad(sq.name)}
                        className={`p-2.5 rounded-lg border cursor-pointer transition-all flex items-start gap-2.5 ${
                          isSelected
                            ? 'border-teal-500 bg-teal-50/20 dark:bg-teal-950/20 ring-1 ring-teal-500/50'
                            : 'border-neutral-200 dark:border-neutral-800 hover:border-neutral-300 dark:hover:border-neutral-700'
                        }`}
                      >
                        <div className={`w-4 h-4 rounded mt-0.5 flex items-center justify-center shrink-0 border ${
                          isSelected ? 'bg-teal-500 border-teal-500 text-white' : 'border-neutral-300 dark:border-neutral-700'
                        }`}>
                          {isSelected && <Check className="w-3 h-3" />}
                        </div>
                        <div>
                          <div className="font-semibold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                            <span>{sq.name}</span>
                            <span className="text-[10px] font-mono px-1 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-500">
                              {sq.crewCount} berths
                            </span>
                          </div>
                          <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5 leading-snug">
                            {sq.description}
                          </p>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>

              <div>
                <label className="block text-neutral-700 dark:text-neutral-300 font-semibold mb-1.5">
                  2. Assign Specialist Crew Members to Ship ({selectedCrewIds.length} Selected)
                </label>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 max-h-44 overflow-y-auto pr-1 scrollbar-none">
                  {crew.map((member) => {
                    const isSelected = selectedCrewIds.includes(member.id);
                    return (
                      <div
                        key={member.id}
                        onClick={() => toggleCrew(member.id)}
                        className={`p-2 rounded-lg border cursor-pointer transition-all flex items-center justify-between ${
                          isSelected
                            ? 'border-teal-500 bg-teal-50/20 dark:bg-teal-950/20'
                            : 'border-neutral-200 dark:border-neutral-800 hover:border-neutral-300 dark:hover:border-neutral-700'
                        }`}
                      >
                        <div className="flex items-center gap-2 min-w-0">
                          <div className="w-6 h-6 rounded-md bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center text-xs font-bold shrink-0">
                            {member.avatar || member.name[0]}
                          </div>
                          <div className="truncate">
                            <span className="font-medium text-neutral-900 dark:text-neutral-100 block truncate">
                              {member.name}
                            </span>
                            <span className="text-[10px] text-neutral-400 block truncate">
                              {member.role}
                            </span>
                          </div>
                        </div>
                        <div className={`w-4 h-4 rounded flex items-center justify-center shrink-0 border ${
                          isSelected ? 'bg-teal-500 border-teal-500 text-white' : 'border-neutral-300 dark:border-neutral-700'
                        }`}>
                          {isSelected && <Check className="w-3 h-3" />}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>

              <div className="flex items-center justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
                <button
                  type="button"
                  onClick={() => setIsCraftShipOpen(false)}
                  className="px-3.5 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-700 dark:text-neutral-300 text-xs font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-4 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer shadow-xs flex items-center gap-1.5"
                >
                  <Ship className="w-3.5 h-3.5" />
                  <span>Commission Ship</span>
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
};

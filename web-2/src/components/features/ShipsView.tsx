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
  Layers,
  ShieldCheck,
  ChevronDown
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { PageHeaderNav } from '../common/PageHeaderNav';
import { Button } from '../common/Button';
import { Modal } from '../common/Modal';
import { ItemCard } from '../common/ItemCard';
import { CardPopover } from '../common/CardPopover';
import { Dropdown, SelectDropdown } from '../common/Dropdown';
import { CrewMember, Squad } from '../../types';

export const ShipsView: React.FC = () => {
  const {
    ships,
    squads,
    crew,
    quests,
    artifacts,
    setActiveTab,
    createShip,
    updateSquad
  } = useFleetStore();

  const [selectedShipId, setSelectedShipId] = useState<string>('ship-dev');
  const [isManualModalOpen, setIsManualModalOpen] = useState(false);
  const [isAiModalOpen, setIsAiModalOpen] = useState(false);
  const [inspectingCrew, setInspectingCrew] = useState<CrewMember | null>(null);
  const [toastMessage, setToastMessage] = useState<string | null>(null);

  // Manual Craft Form State
  const [shipName, setShipName] = useState('');
  const [navigatorName, setNavigatorName] = useState('');
  const [tagline, setTagline] = useState('');
  const [homeScope, setHomeScope] = useState('engineering');
  const [selectedSquadIds, setSelectedSquadIds] = useState<string[]>([]);
  const [selectedCrewIds, setSelectedCrewIds] = useState<string[]>([]);
  const [monthlyBudgetUSD, setMonthlyBudgetUSD] = useState(30.0);

  // AI Gen State
  const [aiPrompt, setAiPrompt] = useState('');
  const [isGeneratingAiShip, setIsGeneratingAiShip] = useState(false);
  const [proposedAiShip, setProposedAiShip] = useState<{
    name: string;
    tagline: string;
    navigatorName: string;
    homeScope: string;
    monthlyBudgetUSD: number;
    suggestedSquads: string[];
    charterPurpose: string;
  } | null>(null);

  const showToast = (msg: string) => {
    setToastMessage(msg);
    setTimeout(() => setToastMessage(null), 3000);
  };

  const selectedShip = ships.find((s) => s.id === selectedShipId) || ships[0];
  const departmentSquads = squads.filter(
    (sq) => sq.shipId === selectedShip.id || selectedShip.squadIds?.includes(sq.id)
  );
  const shipCrew = crew.filter((c) => c.shipId === selectedShip.id);
  const shipQuests = quests.filter(
    (q) => q.assignedShipId === selectedShip.id || q.suggestedShipId === selectedShip.id
  );

  const handleOpenManualCraft = () => {
    setShipName('');
    setNavigatorName('');
    setTagline('');
    setHomeScope('engineering');
    setSelectedSquadIds([]);
    setSelectedCrewIds([]);
    setMonthlyBudgetUSD(30.0);
    setIsManualModalOpen(true);
  };

  const handleOpenAiCraft = () => {
    setAiPrompt('');
    setProposedAiShip(null);
    setIsAiModalOpen(true);
  };

  const handleToggleSquadSelection = (squadId: string) => {
    setSelectedSquadIds((prev) =>
      prev.includes(squadId) ? prev.filter((id) => id !== squadId) : [...prev, squadId]
    );
  };

  const handleToggleCrewSelection = (crewId: string) => {
    setSelectedCrewIds((prev) =>
      prev.includes(crewId) ? prev.filter((id) => id !== crewId) : [...prev, crewId]
    );
  };

  const handleSaveManualShip = () => {
    if (!shipName.trim()) return;

    const newShipId = createShip({
      name: shipName.trim(),
      navigatorName: navigatorName.trim() || 'Orion Navigator',
      tagline: tagline.trim() || 'Autonomous department vessel governing tactical squads.',
      homeScope,
      squadIds: selectedSquadIds,
      crewIds: selectedCrewIds,
      charter: {
        purpose: tagline.trim() || 'High-impact department mission execution.',
        acceptedQuestTypes: ['repository_health', 'ci_triage', 'feature_delivery', 'release_readiness'],
        crewAuthority: 'Autonomous reading and staging. Impactful external writes require Captain’s Approval.',
        prohibitedActions: ['Direct production deploys without Captain sign-off'],
        budgetPerVoyageUSD: 2.0,
        monthlyBudgetUSD,
        memorySharing: 'ship_scoped'
      }
    });

    // Update parent ship on selected squads
    selectedSquadIds.forEach((sqId) => {
      updateSquad(sqId, { shipId: newShipId });
    });

    setSelectedShipId(newShipId);
    setIsManualModalOpen(false);
    showToast(`Crafted new Department Ship: ${shipName}`);
  };

  const handleGenerateAiShip = () => {
    if (!aiPrompt.trim()) return;
    setIsGeneratingAiShip(true);

    setTimeout(() => {
      setIsGeneratingAiShip(false);
      const lower = aiPrompt.toLowerCase();
      let derivedName = 'Data Science & Intelligence Vessel';
      let derivedTagline = 'Department overseeing quantitative modeling, data pipelines, and telemetry synthesis.';
      let derivedScope = 'research';
      let derivedNavigator = 'Polaris (Data Lead)';
      let derivedSquads = ['Squad Analytics Engine', 'Squad Model Benchmarking'];

      if (lower.includes('security') || lower.includes('compliance')) {
        derivedName = 'Security & Fleet Governance Vessel';
        derivedTagline = 'Department responsible for zero-trust authorization, credential rotation, and compliance.';
        derivedScope = 'operations';
        derivedNavigator = 'Aegis (Security Officer)';
        derivedSquads = ['Squad Security Guard', 'Squad Policy Auditor'];
      } else if (lower.includes('infra') || lower.includes('cloud') || lower.includes('devops')) {
        derivedName = 'Cloud & Infrastructure Vessel';
        derivedTagline = 'Department provisioning container runtimes, Kubernetes clusters, and edge services.';
        derivedScope = 'engineering';
        derivedNavigator = 'Atlas (Infra Navigator)';
        derivedSquads = ['Squad SRE & Delivery', 'Squad Cluster Ops'];
      }

      setProposedAiShip({
        name: derivedName,
        tagline: derivedTagline,
        navigatorName: derivedNavigator,
        homeScope: derivedScope,
        monthlyBudgetUSD: 45.0,
        suggestedSquads: derivedSquads,
        charterPurpose: derivedTagline
      });
    }, 900);
  };

  const handleSaveAiShip = () => {
    if (!proposedAiShip) return;

    const newShipId = createShip({
      name: proposedAiShip.name,
      navigatorName: proposedAiShip.navigatorName,
      tagline: proposedAiShip.tagline,
      homeScope: proposedAiShip.homeScope,
      squadIds: [],
      crewIds: [],
      charter: {
        purpose: proposedAiShip.charterPurpose,
        acceptedQuestTypes: ['architecture_decision', 'ci_triage', 'feature_delivery'],
        crewAuthority: 'Autonomous reading and staging. Impactful external writes require Captain’s Approval.',
        prohibitedActions: ['Direct production deploys without Captain sign-off'],
        budgetPerVoyageUSD: 2.5,
        monthlyBudgetUSD: proposedAiShip.monthlyBudgetUSD,
        memorySharing: 'ship_scoped'
      }
    });

    setSelectedShipId(newShipId);
    setIsAiModalOpen(false);
    showToast(`Quartermaster crafted ${proposedAiShip.name}!`);
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto px-4 sm:px-6 pt-0 pb-6 space-y-4 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Toast Alert */}
      {toastMessage && (
        <div className="fixed top-4 right-4 z-50 px-4 py-2 rounded-xl bg-teal-600 text-white text-xs font-semibold shadow-lg flex items-center gap-2 animate-in fade-in duration-200">
          <CheckCircle2 className="w-4 h-4" />
          <span>{toastMessage}</span>
        </div>
      )}

      {/* Standard Reusable PageHeader with Integrated Chips */}
      <PageHeaderNav
        icon={<Ship className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Ships"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            {ships.length} Departments
          </span>
        }
        description="Departments of the fleet. Each Ship represents a department managing multiple Squads and Crew executing high-impact voyages."
        actions={
          <Dropdown
            title="Craft Ship Options"
            align="right"
            menuWidth="w-72"
            items={[
              {
                id: 'manual',
                label: 'Manual Department Crafting',
                description: 'Define ship name, charter, budget, and map squads manually'
              },
              {
                id: 'ai',
                label: 'Ask Quartermaster (AI Gen)',
                description: 'Quartermaster drafts department scope, navigator, and charter'
              }
            ]}
            onSelect={(id) => {
              if (id === 'manual') handleOpenManualCraft();
              else handleOpenAiCraft();
            }}
            trigger={
              <Button
                variant="primary"
                size="sm"
                icon={<Ship className="w-3.5 h-3.5" />}
                shortLabel="Craft Ship"
                title="Craft Department Ship"
              >
                Craft Ship
              </Button>
            }
          />
        }
        chips={{
          items: ships.map((s) => ({
            id: s.id,
            label: s.name,
            count: `${squads.filter((sq) => sq.shipId === s.id).length} Squads · ${s.crewIds.length} Crew`,
            icon: <Ship className="w-3.5 h-3.5 opacity-70" />
          })),
          selectedId: selectedShip.id,
          onSelect: setSelectedShipId,
          variant: 'subtle'
        }}
      />

      {/* Selected Ship Showcase */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left Column: Department Squads & Crew */}
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
                ACTIVE DEPARTMENT
              </span>
            </div>

            <p className="text-xs text-neutral-800 dark:text-neutral-200 leading-relaxed font-medium">
              &ldquo;Currently orchestrating {departmentSquads.length} tactical Squads and {shipCrew.length} specialist Crew across {shipQuests.length} assigned Quests. All operations follow our department charter with strict budget limits.&rdquo;
            </p>

            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={() => setActiveTab('mission-board')}
                className="px-2.5 py-1 text-xs rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-700 dark:text-neutral-300 hover:border-teal-500 transition-colors cursor-pointer"
              >
                Inspect Quests →
              </button>
              <button
                onClick={() => setActiveTab('squads')}
                className="px-2.5 py-1 text-xs rounded-lg bg-teal-500/10 text-teal-600 dark:text-teal-400 border border-teal-500/30 hover:bg-teal-500/20 transition-colors cursor-pointer"
              >
                Manage Squads →
              </button>
            </div>
          </div>

          {/* Department Squads Section (Key update for user request 3!) */}
          <div className="flex flex-col gap-2.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 flex items-center gap-1.5">
                <ShieldCheck className="w-3.5 h-3.5 text-teal-500" />
                Department Squads ({departmentSquads.length})
              </span>
              <button
                onClick={() => setActiveTab('squads')}
                className="text-xs text-teal-600 dark:text-teal-400 hover:underline cursor-pointer"
              >
                View all Squads →
              </button>
            </div>

            {departmentSquads.length === 0 ? (
              <div className="p-4 text-center rounded-xl border border-dashed border-neutral-300 dark:border-neutral-800 text-xs text-neutral-500 bg-white/50 dark:bg-[#15171a]/50">
                No tactical squads mapped to this department ship yet. Create a squad under the Squad menu.
              </div>
            ) : (
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {departmentSquads.map((sq) => {
                  const sqMembers = crew.filter((c) => sq.crewIds.includes(c.id));
                  return (
                    <ItemCard
                      key={sq.id}
                      compact
                      onClick={() => setActiveTab('squads')}
                      title={sq.name}
                      subtitle={`${sqMembers.length} Specialists`}
                      badge={
                        <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-teal-500/10 text-teal-600 dark:text-teal-400 font-bold">
                          Squad
                        </span>
                      }
                      description={sq.purpose}
                      descriptionClamp={2}
                      footer={
                        <div className="flex items-center gap-1 text-[10px] text-neutral-400 truncate">
                          <span>Members:</span>
                          <span className="font-semibold text-neutral-700 dark:text-neutral-300 truncate">
                            {sqMembers.map((m) => m.name).join(', ') || 'None'}
                          </span>
                        </div>
                      }
                    />
                  );
                })}
              </div>
            )}
          </div>

          {/* Assigned Crew Members */}
          <div className="flex flex-col gap-2.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 flex items-center gap-1.5">
                <Users className="w-3.5 h-3.5 text-teal-500" />
                Department Specialists ({shipCrew.length})
              </span>
              <button
                onClick={() => setActiveTab('crew')}
                className="text-xs text-teal-600 dark:text-teal-400 hover:underline cursor-pointer"
              >
                Manage Crew Roster →
              </button>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              {shipCrew.map((member) => {
                const memberSquad = squads.find((sq) => sq.id === member.squadId);
                return (
                  <ItemCard
                    key={member.id}
                    compact
                    selected={inspectingCrew?.id === member.id}
                    onClick={() => setInspectingCrew(member)}
                    title={member.name}
                    subtitle={member.role}
                    badge={
                      <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-500 font-semibold">
                        {memberSquad ? memberSquad.name : 'Independent'}
                      </span>
                    }
                    description={member.purpose}
                    descriptionClamp={2}
                    footer={
                      <div className="flex items-center justify-between text-[10px] text-neutral-400 font-mono">
                        <span>{member.modelProfile.split(' ')[0]}</span>
                        <span className="text-teal-600 dark:text-teal-400 font-semibold">
                          ${member.costLast30Days.toFixed(2)} cost
                        </span>
                      </div>
                    }
                  />
                );
              })}
            </div>
          </div>
        </div>

        {/* Right Column: Readable Ship Charter & Budget */}
        <div className="flex flex-col gap-4">
          <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-[#15171a] flex flex-col gap-4 text-xs">
            <div className="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3">
              <span className="font-bold text-neutral-900 dark:text-neutral-100 text-sm">
                Department Charter
              </span>
              <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-teal-500/20 text-teal-600 dark:text-teal-400">
                LIVING MANIFEST
              </span>
            </div>

            <div className="space-y-1">
              <span className="text-[11px] font-semibold text-neutral-500 dark:text-neutral-400 uppercase tracking-wider">
                What this Department does
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
                <span className="text-[10px] text-neutral-400 block">Monthly Hard Cap</span>
                <span className="font-semibold text-neutral-800 dark:text-neutral-200">
                  ${selectedShip.charter.monthlyBudgetUSD.toFixed(2)}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* MODAL 1: MANUAL CRAFT SHIP */}
      <Modal
        isOpen={isManualModalOpen}
        onClose={() => setIsManualModalOpen(false)}
        title="Craft Ship (Manual Department Configuration)"
        description="Commission a new Department Vessel to host specialist Squads and Crew."
        maxWidth="xl"
      >
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Ship / Department Name*
            </label>
            <input
              type="text"
              value={shipName}
              onChange={(e) => setShipName(e.target.value)}
              placeholder="e.g. Platform Infrastructure Ship, Growth &amp; Content Ship"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Navigator Name
              </label>
              <input
                type="text"
                value={navigatorName}
                onChange={(e) => setNavigatorName(e.target.value)}
                placeholder="e.g. Horizon Navigator"
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Department Scope
              </label>
              <SelectDropdown
                value={homeScope}
                onChange={setHomeScope}
                options={[
                  { value: 'engineering', label: 'Engineering' },
                  { value: 'marketing', label: 'Marketing' },
                  { value: 'research', label: 'Research & Intelligence' },
                  { value: 'operations', label: 'Operations' }
                ]}
              />
            </div>
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Department Mandate &amp; Tagline*
            </label>
            <textarea
              rows={2}
              value={tagline}
              onChange={(e) => setTagline(e.target.value)}
              placeholder="Describe the overarching mission of this department ship..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none resize-none font-mono"
            />
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Assign Tactical Squads ({selectedSquadIds.length} chosen)
            </label>
            <div className="max-h-36 overflow-y-auto rounded-xl border border-neutral-200 dark:border-neutral-800 p-2 space-y-1.5 bg-neutral-50 dark:bg-neutral-950">
              {squads.map((sq) => {
                const isSelected = selectedSquadIds.includes(sq.id);
                return (
                  <div
                    key={sq.id}
                    onClick={() => handleToggleSquadSelection(sq.id)}
                    className={`p-2 rounded-lg border text-xs flex items-center justify-between cursor-pointer transition-colors ${
                      isSelected
                        ? 'border-teal-500 bg-teal-500/10 text-teal-900 dark:text-teal-200 font-semibold'
                        : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-neutral-700 dark:text-neutral-300'
                    }`}
                  >
                    <span>{sq.name} ({sq.crewIds.length} members)</span>
                    {isSelected && <Check className="w-4 h-4 text-teal-500 shrink-0" />}
                  </div>
                );
              })}
            </div>
          </div>

          <div className="flex justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
            <Button variant="ghost" size="sm" onClick={() => setIsManualModalOpen(false)}>
              Cancel
            </Button>
            <Button variant="primary" size="sm" onClick={handleSaveManualShip}>
              Craft Department Ship
            </Button>
          </div>
        </div>
      </Modal>

      {/* MODAL 2: QUARTERMASTER GEN AI CRAFT SHIP */}
      <Modal
        isOpen={isAiModalOpen}
        onClose={() => setIsAiModalOpen(false)}
        title="Consult Quartermaster (AI Department Crafting)"
        description="Describe your department mission. Quartermaster will design the Ship charter, navigator, and budget boundaries."
        maxWidth="2xl"
      >
        <div className="space-y-4">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Describe the department requirements:
            </label>
            <textarea
              rows={3}
              value={aiPrompt}
              onChange={(e) => setAiPrompt(e.target.value)}
              placeholder="e.g. We need a Dedicated Data Science & Machine Learning Department to run model evaluations, automate benchmarking quests, and manage telemetry pipelines..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500 resize-none font-mono"
            />
          </div>

          <div className="flex justify-end">
            <Button
              variant="primary"
              size="sm"
              disabled={!aiPrompt.trim() || isGeneratingAiShip}
              onClick={handleGenerateAiShip}
              icon={<Sparkles className="w-3.5 h-3.5" />}
            >
              {isGeneratingAiShip ? 'Quartermaster is drafting department vessel…' : 'Synthesize Ship'}
            </Button>
          </div>

          {/* AI Output Preview and Edit */}
          {proposedAiShip && (
            <div className="p-4 rounded-xl border border-teal-500/30 bg-teal-500/5 space-y-3 animate-in fade-in duration-200">
              <div className="flex items-center justify-between">
                <span className="text-xs font-bold text-teal-600 dark:text-teal-400 uppercase tracking-wider flex items-center gap-1.5">
                  <CheckCircle2 className="w-3.5 h-3.5" />
                  Proposed Department Ship Configuration
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/20 text-teal-500 font-semibold">
                  Review &amp; Edit
                </span>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                    Ship Name
                  </label>
                  <input
                    type="text"
                    value={proposedAiShip.name}
                    onChange={(e) => setProposedAiShip({ ...proposedAiShip, name: e.target.value })}
                    className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-bold"
                  />
                </div>
                <div>
                  <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                    Navigator Name
                  </label>
                  <input
                    type="text"
                    value={proposedAiShip.navigatorName}
                    onChange={(e) => setProposedAiShip({ ...proposedAiShip, navigatorName: e.target.value })}
                    className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-bold"
                  />
                </div>
              </div>

              <div>
                <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                  Department Mandate &amp; Charter Purpose
                </label>
                <textarea
                  rows={2}
                  value={proposedAiShip.charterPurpose}
                  onChange={(e) => setProposedAiShip({ ...proposedAiShip, charterPurpose: e.target.value, tagline: e.target.value })}
                  className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-mono resize-none"
                />
              </div>

              <div className="pt-2 flex justify-end gap-2 border-t border-teal-500/20">
                <Button variant="ghost" size="sm" onClick={() => setProposedAiShip(null)}>
                  Discard
                </Button>
                <Button variant="primary" size="sm" onClick={handleSaveAiShip} icon={<Check className="w-3.5 h-3.5" />}>
                  Save &amp; Craft Ship
                </Button>
              </div>
            </div>
          )}
        </div>
      </Modal>
    </div>
  );
};

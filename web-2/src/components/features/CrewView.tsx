import React, { useState } from 'react';
import {
  Users,
  Sparkles,
  Shield,
  Wrench,
  Brain,
  Coins,
  Ship,
  CheckCircle2,
  ArrowRight,
  PlusCircle,
  HelpCircle,
  Filter
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { CrewMember } from '../../types';
import { PageHeader } from '../common/PageHeader';
import { PageStickyNav } from '../common/PageStickyNav';
import { SubMenuScroller } from '../common/SubMenuScroller';
import { Modal } from '../common/Modal';
import { ItemCard } from '../common/ItemCard';
import { CardPopover } from '../common/CardPopover';

export const CrewView: React.FC = () => {
  const { crew, ships, addCrewMember, updateCrewMember, setActiveTab, createQuest } = useFleetStore();

  const [selectedShipFilter, setSelectedShipFilter] = useState('all');
  const [search, setSearch] = useState('');
  const [isWizardOpen, setIsWizardOpen] = useState(false);
  const [wizardStep, setWizardStep] = useState(1);
  const [selectedTemplate, setSelectedTemplate] = useState('dev');

  // New crew draft in wizard
  const [wizardRoleName, setWizardRoleName] = useState('Refactor Specialist');
  const [wizardPurpose, setWizardPurpose] = useState('Analyze technical debt and generate isolated AST diff refactors.');

  // Editing existing crew modal state
  const [editingCrew, setEditingCrew] = useState<CrewMember | null>(null);
  const [editForm, setEditForm] = useState<Partial<CrewMember>>({});

  const filteredCrew = crew.filter((c) => {
    const matchShip = selectedShipFilter === 'all' || c.shipId === selectedShipFilter;
    const matchSearch =
      c.name.toLowerCase().includes(search.toLowerCase()) ||
      c.role.toLowerCase().includes(search.toLowerCase()) ||
      c.purpose.toLowerCase().includes(search.toLowerCase()) ||
      c.tools.some((t) => t.toLowerCase().includes(search.toLowerCase()));
    return matchShip && matchSearch;
  });

  const handleFinishWizard = () => {
    addCrewMember({
      name: wizardRoleName,
      shipId: 'ship-dev',
      role: 'AST Refactor Engineer',
      purpose: wizardPurpose,
      skills: ['Code Modernization', 'Dead Code Pruning', 'Test Generation'],
      tools: ['ast_tool', 'local_filesystem'],
      modelProfile: 'Claude 3.7 Sonnet',
      authority: 'draft_only',
      memoryScope: 'ship',
      status: 'active',
      lastVoyage: 'Just recruited',
      costLast30Days: 0.00
    });

    createQuest({
      title: 'Initial Refactor Audit for feat/enhance-agent-phase',
      objective: 'Run newly assigned specialist against the code engine directory to identify modernization targets.',
      suggestedShipId: 'ship-dev',
      priority: 'medium'
    });

    setIsWizardOpen(false);
    setWizardStep(1);
    setActiveTab('mission-board');
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-4 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Reusable Standard Header */}
      <PageHeader
        icon={<Users className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Crew Members"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            {crew.length} / 15 Berths Active
          </span>
        }
        description="Persistent AI specialists with scoped tools, memory, artifact contracts, and authority."
        search={{
          value: search,
          onChange: setSearch,
          placeholder: 'Search crew by role, tools...'
        }}
        actions={
          <button
            onClick={() => setIsWizardOpen(true)}
            className="flex items-center gap-1 sm:gap-1.5 px-2.5 sm:px-3.5 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer shadow-xs shrink-0"
          >
            <Sparkles className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">Make Me a Squad</span>
            <span className="sm:hidden">New Squad</span>
          </button>
        }
      />

      {/* Floating Sticky Sub-Tabs with Navigation Arrows (< >) */}
      <PageStickyNav>
        <SubMenuScroller className="gap-2" containerClassName="w-full">
          <button
            onClick={() => setSelectedShipFilter('all')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium transition-all shrink-0 cursor-pointer ${
              selectedShipFilter === 'all'
                ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-white font-semibold shadow-2xs'
                : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
            }`}
          >
            <span>All Ships</span>
            <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-200/80 dark:bg-neutral-700/80 text-neutral-700 dark:text-neutral-300">
              {crew.length}
            </span>
          </button>
          {ships.map((ship) => {
            const shipCrewCount = crew.filter((c) => c.shipId === ship.id).length;
            const isSelected = selectedShipFilter === ship.id;
            return (
              <button
                key={ship.id}
                onClick={() => setSelectedShipFilter(ship.id)}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium transition-all shrink-0 cursor-pointer ${
                  isSelected
                    ? 'bg-teal-500/10 dark:bg-teal-500/15 text-teal-700 dark:text-teal-300 font-semibold border border-teal-500/30'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white border border-neutral-200 dark:border-neutral-800 bg-white/60 dark:bg-[#181a1d]'
                }`}
              >
                <Ship className="w-3.5 h-3.5 text-neutral-400" />
                <span className="whitespace-nowrap">{ship.name}</span>
                <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400">
                  {shipCrewCount}
                </span>
              </button>
            );
          })}
        </SubMenuScroller>
      </PageStickyNav>

      {/* Crew Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {filteredCrew.map((member) => {
          const ship = ships.find((s) => s.id === member.shipId);
          return (
            <ItemCard
              key={member.id}
              selected={editingCrew?.id === member.id}
              onClick={() => {
                setEditingCrew(member);
                setEditForm({
                  name: member.name,
                  role: member.role,
                  purpose: member.purpose,
                  skills: member.skills,
                  modelProfile: member.modelProfile,
                  authority: member.authority,
                  memoryScope: member.memoryScope,
                  status: member.status
                });
              }}
              title={member.name}
              subtitle={member.role}
              badge={
                <span
                  className={`text-[9px] font-mono px-1.5 py-0.2 rounded uppercase font-semibold ${
                    member.authority === 'read_only'
                      ? 'bg-blue-500/20 text-blue-500'
                      : member.authority === 'draft_only'
                      ? 'bg-amber-500/20 text-amber-500'
                      : 'bg-teal-500/20 text-teal-500'
                  }`}
                >
                  {member.authority.replace('_', ' ')}
                </span>
              }
              description={member.purpose}
              tags={member.skills}
              footer={
                <div className="space-y-2">
                  <div className="grid grid-cols-2 gap-2 text-[10px] font-mono text-neutral-500">
                    <div>
                      <span className="text-neutral-400 block">Model Profile</span>
                      <span className="text-neutral-800 dark:text-neutral-200 truncate block">
                        {member.modelProfile}
                      </span>
                    </div>
                    <div>
                      <span className="text-neutral-400 block">Memory Boundary</span>
                      <span className="text-neutral-800 dark:text-neutral-200 block capitalize">
                        {member.memoryScope} Scoped
                      </span>
                    </div>
                  </div>
                  <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-between text-[11px] font-mono">
                    <span className="text-neutral-400">
                      Ship: {ship?.name.replace(' Ship', '') || 'Developer'}
                    </span>
                    <span className="text-teal-600 dark:text-teal-400 font-semibold">
                      ${member.costLast30Days.toFixed(2)}/mo
                    </span>
                  </div>
                </div>
              }
            />
          );
        })}
      </div>

      {/* Make Me a Squad Wizard Modal */}
      <Modal
        isOpen={isWizardOpen}
        onClose={() => setIsWizardOpen(false)}
        maxWidth="xl"
        icon={<Sparkles className="w-4 h-4 text-amber-500" />}
        title="Make Me a Squad — AI Team Blueprint"
        subtitle={`Step ${wizardStep} of 3: Formulate a persistent specialist squad without manual agent wiring.`}
        footer={
          <div className="flex items-center justify-between w-full">
            <button
              type="button"
              onClick={() => {
                if (wizardStep > 1) {
                  setWizardStep(wizardStep - 1);
                } else {
                  setIsWizardOpen(false);
                }
              }}
              className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-xs font-medium cursor-pointer"
            >
              {wizardStep === 1 ? 'Cancel' : 'Back'}
            </button>

            {wizardStep < 3 ? (
              <button
                type="button"
                onClick={() => setWizardStep(wizardStep + 1)}
                className="px-4 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold hover:opacity-90 cursor-pointer"
              >
                Continue
              </button>
            ) : (
              <button
                type="button"
                onClick={handleFinishWizard}
                className="px-4 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold hover:opacity-90 cursor-pointer"
              >
                <span className="hidden sm:inline">Recruit &amp; Launch First Quest</span>
                <span className="sm:hidden">Recruit &amp; Launch</span>
              </button>
            )}
          </div>
        }
      >
        <div className="space-y-4">

            {/* Step 1: Choose Intent */}
            {wizardStep === 1 && (
              <div className="space-y-3">
                <span className="font-semibold text-neutral-800 dark:text-neutral-200 block">
                  Select a team template or define a custom specialty:
                </span>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div
                    onClick={() => setSelectedTemplate('dev')}
                    className={`p-3 rounded-lg border cursor-pointer transition-all ${
                      selectedTemplate === 'dev'
                        ? 'border-teal-500 bg-teal-50/20 dark:bg-teal-950/20 ring-1 ring-teal-500'
                        : 'border-neutral-200 dark:border-neutral-800 hover:border-neutral-400'
                    }`}
                  >
                    <div className="font-semibold text-neutral-900 dark:text-neutral-100">
                      Developer Refactor Squad
                    </div>
                    <div className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-1">
                      AST analysis, code modernization, dead code detection, and regression testing.
                    </div>
                  </div>

                  <div
                    onClick={() => setSelectedTemplate('qa')}
                    className={`p-3 rounded-lg border cursor-pointer transition-all ${
                      selectedTemplate === 'qa'
                        ? 'border-teal-500 bg-teal-50/20 dark:bg-teal-950/20 ring-1 ring-teal-500'
                        : 'border-neutral-200 dark:border-neutral-800 hover:border-neutral-400'
                    }`}
                  >
                    <div className="font-semibold text-neutral-900 dark:text-neutral-100">
                      Security &amp; Vulnerability Squad
                    </div>
                    <div className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-1">
                      Dependency CVE audit, secret leak scanning, and policy sandbox compliance.
                    </div>
                  </div>
                </div>

                <div className="pt-2">
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    Specialist Role Name
                  </label>
                  <input
                    type="text"
                    value={wizardRoleName}
                    onChange={(e) => setWizardRoleName(e.target.value)}
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                  />
                </div>

                <div>
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    Role Purpose &amp; Boundaries
                  </label>
                  <textarea
                    rows={2}
                    value={wizardPurpose}
                    onChange={(e) => setWizardPurpose(e.target.value)}
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500"
                  />
                </div>
              </div>
            )}

            {/* Step 2: Policy & Authority */}
            {wizardStep === 2 && (
              <div className="space-y-3">
                <span className="font-semibold text-neutral-800 dark:text-neutral-200 block">
                  Define authority, safety boundaries, and model profile:
                </span>

                <div className="p-3 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900/40 space-y-2">
                  <div className="flex items-center justify-between font-semibold">
                    <span>Authority Class</span>
                    <span className="text-teal-600 font-mono">DRAFT ONLY</span>
                  </div>
                  <p className="text-neutral-500 dark:text-neutral-400 text-[11px]">
                    This specialist can propose patches and draft pull requests, but cannot write directly to master or trigger external webhooks without Captain&rsquo;s Approval.
                  </p>
                </div>

                <div className="grid grid-cols-2 gap-3 font-mono">
                  <div className="p-2.5 rounded border border-neutral-200 dark:border-neutral-800">
                    <span className="text-[10px] text-neutral-400 block">Model Profile</span>
                    <span className="font-medium text-neutral-800 dark:text-neutral-200">
                      Claude 3.7 Sonnet
                    </span>
                  </div>
                  <div className="p-2.5 rounded border border-neutral-200 dark:border-neutral-800">
                    <span className="text-[10px] text-neutral-400 block">Memory Scope</span>
                    <span className="font-medium text-neutral-800 dark:text-neutral-200">
                      Ship-Scoped
                    </span>
                  </div>
                </div>
              </div>
            )}

            {/* Step 3: Confirmation */}
            {wizardStep === 3 && (
              <div className="space-y-3">
                <div className="p-4 rounded-lg bg-teal-50/60 dark:bg-teal-950/30 border border-teal-500/30 space-y-2 text-center">
                  <CheckCircle2 className="w-6 h-6 text-teal-500 mx-auto" />
                  <div className="font-bold text-neutral-900 dark:text-neutral-100">
                    Squad Member Ready to Recruit
                  </div>
                  <p className="text-neutral-600 dark:text-neutral-400 text-xs">
                    &ldquo;{wizardRoleName}&rdquo; will be assigned to Developer Delivery Ship with 1 initial audit Quest placed on the Mission Board.
                  </p>
                </div>
              </div>
            )}

        </div>
      </Modal>

      {/* Specialist Edit & Details Popover / Drawer */}
      <CardPopover
        isOpen={Boolean(editingCrew)}
        onClose={() => setEditingCrew(null)}
        variant="sheet-right"
        drawerWidth="sm:w-[560px]"
        icon={<Users className="w-4 h-4 text-teal-500" />}
        title={
          editingCrew && (
            <div className="flex items-center gap-2">
              <span className="w-2.5 h-2.5 rounded-full bg-teal-500 animate-pulse" />
              <span>Specialist: {editingCrew.name}</span>
            </div>
          )
        }
        badge={
          editingCrew && (
            <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/10 text-teal-600 dark:text-teal-400 font-bold uppercase">
              {editingCrew.authority.replace('_', ' ')}
            </span>
          )
        }
        subtitle="Manage specialist identity, LLM profile, scoped tools, and authority boundaries."
        footer={
          <>
            <button
              type="button"
              onClick={() => setEditingCrew(null)}
              className="px-3.5 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-xs font-medium cursor-pointer"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => {
                if (editingCrew) {
                  updateCrewMember(editingCrew.id, editForm);
                  setEditingCrew(null);
                }
              }}
              className="px-4 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 transition-opacity cursor-pointer shadow-xs"
            >
              <span className="hidden sm:inline">Save Specialist</span>
              <span className="sm:hidden">Save</span>
            </button>
          </>
        }
      >
        <div className="space-y-4">
              <div>
                <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                  Specialist Name
                </label>
                <input
                  type="text"
                  value={editForm.name || ''}
                  onChange={(e) => setEditForm((prev) => ({ ...prev, name: e.target.value }))}
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 text-xs"
                />
              </div>

              <div>
                <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                  Role / Title
                </label>
                <input
                  type="text"
                  value={editForm.role || ''}
                  onChange={(e) => setEditForm((prev) => ({ ...prev, role: e.target.value }))}
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 text-xs"
                />
              </div>

              <div>
                <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                  Operational Purpose & Mandate
                </label>
                <textarea
                  rows={2}
                  value={editForm.purpose || ''}
                  onChange={(e) => setEditForm((prev) => ({ ...prev, purpose: e.target.value }))}
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 text-xs resize-none"
                />
              </div>

              <div>
                <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                  Specialist Skills (comma separated)
                </label>
                <input
                  type="text"
                  value={editForm.skills?.join(', ') || ''}
                  onChange={(e) =>
                    setEditForm((prev) => ({
                      ...prev,
                      skills: e.target.value
                        .split(',')
                        .map((s) => s.trim())
                        .filter(Boolean)
                    }))
                  }
                  className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 text-xs"
                />
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    AI Model Profile
                  </label>
                  <select
                    value={editForm.modelProfile || ''}
                    onChange={(e) => setEditForm((prev) => ({ ...prev, modelProfile: e.target.value }))}
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 text-xs"
                  >
                    <option value="Claude 3.7 Sonnet (Reasoning)">Claude 3.7 Sonnet (Reasoning)</option>
                    <option value="Claude 3.7 Sonnet">Claude 3.7 Sonnet</option>
                    <option value="Gemini 2.5 Pro">Gemini 2.5 Pro</option>
                    <option value="Gemini 2.5 Flash">Gemini 2.5 Flash</option>
                    <option value="GPT-4o">GPT-4o</option>
                  </select>
                </div>

                <div>
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    Authority Level
                  </label>
                  <select
                    value={editForm.authority || 'draft_only'}
                    onChange={(e) => setEditForm((prev) => ({ ...prev, authority: e.target.value as any }))}
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 text-xs"
                  >
                    <option value="read_only">Read Only (Inspection)</option>
                    <option value="draft_only">Draft Only (Requires Approval)</option>
                    <option value="full_autonomous">Full Autonomous (Authorized)</option>
                  </select>
                </div>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    Memory Scope
                  </label>
                  <select
                    value={editForm.memoryScope || 'ship'}
                    onChange={(e) => setEditForm((prev) => ({ ...prev, memoryScope: e.target.value as any }))}
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 text-xs"
                  >
                    <option value="ship">Ship Scoped</option>
                    <option value="fleet">Fleet Wide</option>
                    <option value="private">Private Scoped</option>
                  </select>
                </div>

                <div>
                  <label className="block font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                    Active Status
                  </label>
                  <select
                    value={editForm.status || 'active'}
                    onChange={(e) => setEditForm((prev) => ({ ...prev, status: e.target.value as any }))}
                    className="w-full px-3 py-2 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 text-xs"
                  >
                    <option value="active">Active</option>
                    <option value="standby">Standby</option>
                    <option value="busy">Busy / On Voyage</option>
                  </select>
                </div>
              </div>
            </div>
          </CardPopover>
    </div>
  );
};

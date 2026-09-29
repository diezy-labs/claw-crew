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

export const CrewView: React.FC = () => {
  const { crew, ships, addCrewMember, setActiveTab, createQuest } = useFleetStore();

  const [selectedShipFilter, setSelectedShipFilter] = useState('all');
  const [isWizardOpen, setIsWizardOpen] = useState(false);
  const [wizardStep, setWizardStep] = useState(1);
  const [selectedTemplate, setSelectedTemplate] = useState('dev');

  // New crew draft in wizard
  const [wizardRoleName, setWizardRoleName] = useState('Refactor Specialist');
  const [wizardPurpose, setWizardPurpose] = useState('Analyze technical debt and generate isolated AST diff refactors.');

  const filteredCrew = crew.filter((c) => {
    return selectedShipFilter === 'all' || c.shipId === selectedShipFilter;
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
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-6xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
              Crew Members
            </h1>
            <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
              {crew.length} / 15 Berths Active
            </span>
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
            Persistent AI specialists with scoped tools, memory, artifact contracts, and authority.
          </p>
        </div>

        <div className="flex items-center gap-2 self-start">
          <select
            value={selectedShipFilter}
            onChange={(e) => setSelectedShipFilter(e.target.value)}
            className="text-xs px-2.5 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 text-neutral-800 dark:text-neutral-200 focus:outline-none cursor-pointer"
          >
            <option value="all">All Ships</option>
            {ships.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>

          <button
            onClick={() => setIsWizardOpen(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 transition-opacity"
          >
            <Sparkles className="w-3.5 h-3.5" />
            <span>Make Me a Squad</span>
          </button>
        </div>
      </div>

      {/* Crew Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {filteredCrew.map((member) => {
          const ship = ships.find((s) => s.id === member.shipId);
          return (
            <div
              key={member.id}
              className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3.5 text-xs shadow-xs hover:border-teal-500/40 transition-colors"
            >
              {/* Header */}
              <div className="flex items-start justify-between">
                <div>
                  <h3 className="font-bold text-neutral-900 dark:text-neutral-100 text-sm">
                    {member.name}
                  </h3>
                  <div className="text-[11px] text-neutral-500 dark:text-neutral-400">
                    {member.role}
                  </div>
                </div>
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
              </div>

              {/* Purpose */}
              <p className="text-neutral-600 dark:text-neutral-400 leading-relaxed line-clamp-2">
                {member.purpose}
              </p>

              {/* Skills badges */}
              <div className="space-y-1">
                <span className="text-[10px] font-mono text-neutral-400 uppercase tracking-wider">
                  Specialist Skills
                </span>
                <div className="flex flex-wrap gap-1">
                  {member.skills.map((skill, idx) => (
                    <span
                      key={idx}
                      className="px-2 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 text-[10px] font-mono"
                    >
                      {skill}
                    </span>
                  ))}
                </div>
              </div>

              {/* Model & Tool scope */}
              <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800/80 grid grid-cols-2 gap-2 text-[10px] font-mono text-neutral-500">
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

              {/* Footer status */}
              <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-between text-[11px] font-mono">
                <span className="text-neutral-400">
                  Ship: {ship?.name.replace(' Ship', '') || 'Developer'}
                </span>
                <span className="text-teal-600 dark:text-teal-400 font-semibold">
                  ${member.costLast30Days.toFixed(2)}/mo
                </span>
              </div>
            </div>
          );
        })}
      </div>

      {/* Make Me a Squad Wizard Modal */}
      {isWizardOpen && (
        <div
          onClick={() => setIsWizardOpen(false)}
          className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs cursor-pointer"
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-xl rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-2xl p-6 space-y-5 text-xs cursor-default"
          >
            {/* Header */}
            <div className="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3">
              <div>
                <div className="flex items-center gap-2">
                  <Sparkles className="w-4 h-4 text-amber-500" />
                  <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                    Make Me a Squad — AI Team Blueprint
                  </h3>
                </div>
                <p className="text-[11px] text-neutral-400 mt-0.5">
                  Step {wizardStep} of 3: Formulate a persistent specialist squad without manual agent wiring.
                </p>
              </div>
            </div>

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

            {/* Wizard Navigation Footer */}
            <div className="pt-3 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-between">
              {wizardStep > 1 ? (
                <button
                  type="button"
                  onClick={() => setWizardStep(wizardStep - 1)}
                  className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                >
                  Back
                </button>
              ) : (
                <div />
              )}

              {wizardStep < 3 ? (
                <button
                  type="button"
                  onClick={() => setWizardStep(wizardStep + 1)}
                  className="px-4 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold hover:opacity-90"
                >
                  Continue
                </button>
              ) : (
                <button
                  type="button"
                  onClick={handleFinishWizard}
                  className="px-4 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-semibold hover:opacity-90"
                >
                  Recruit &amp; Launch First Quest
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

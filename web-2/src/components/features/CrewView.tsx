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
  Plus,
  HelpCircle,
  Filter,
  ShieldCheck,
  Compass,
  Cpu,
  Edit2,
  Check
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { CrewMember } from '../../types';
import { PageHeaderNav } from '../common/PageHeaderNav';
import { Button } from '../common/Button';
import { Modal } from '../common/Modal';
import { ItemCard } from '../common/ItemCard';
import { Dropdown, SelectDropdown } from '../common/Dropdown';

export const CrewView: React.FC = () => {
  const {
    crew,
    ships,
    squads,
    trainingSkills,
    steeringDirectives,
    addCrewMember,
    updateCrewMember,
    setActiveTab
  } = useFleetStore();

  const [selectedSquadFilter, setSelectedSquadFilter] = useState('all');
  const [search, setSearch] = useState('');
  const [isManualModalOpen, setIsManualModalOpen] = useState(false);
  const [isAiModalOpen, setIsAiModalOpen] = useState(false);
  const [editingCrew, setEditingCrew] = useState<CrewMember | null>(null);
  const [toastMessage, setToastMessage] = useState<string | null>(null);

  // Manual Form State
  const [manualForm, setManualForm] = useState({
    name: '',
    role: '',
    purpose: '',
    shipId: ships[0]?.id || 'ship-dev',
    squadId: 'none',
    modelProfile: 'Claude 3.7 Sonnet (Reasoning)',
    selectedSkill: trainingSkills[0]?.name || 'Incident Briefing Officer',
    selectedSteering: steeringDirectives[0]?.guidance || 'Cite approved sources and separate evidence from inference.',
    authority: 'draft_only' as 'read_only' | 'draft_only' | 'gated_write',
    memoryScope: 'ship' as 'crew' | 'ship' | 'workspace'
  });

  // AI Gen State
  const [aiPrompt, setAiPrompt] = useState('');
  const [isGeneratingAiCrew, setIsGeneratingAiCrew] = useState(false);
  const [proposedAiCrew, setProposedAiCrew] = useState<{
    name: string;
    role: string;
    purpose: string;
    shipId: string;
    squadId: string;
    modelProfile: string;
    skills: string[];
    steering: string;
    authority: 'read_only' | 'draft_only' | 'gated_write';
    memoryScope: 'crew' | 'ship' | 'workspace';
  } | null>(null);

  const showToast = (msg: string) => {
    setToastMessage(msg);
    setTimeout(() => setToastMessage(null), 3000);
  };

  // Filter crew by squad mapping
  const filteredCrew = crew.filter((c) => {
    let matchSquad = true;
    if (selectedSquadFilter === 'non-squad') {
      matchSquad = !c.squadId || c.squadId === 'none';
    } else if (selectedSquadFilter !== 'all') {
      matchSquad = c.squadId === selectedSquadFilter;
    }

    const matchSearch =
      c.name.toLowerCase().includes(search.toLowerCase()) ||
      c.role.toLowerCase().includes(search.toLowerCase()) ||
      c.purpose.toLowerCase().includes(search.toLowerCase()) ||
      c.skills.some((sk) => sk.toLowerCase().includes(search.toLowerCase()));

    return matchSquad && matchSearch;
  });

  const handleOpenManualAdd = () => {
    setManualForm({
      name: '',
      role: '',
      purpose: '',
      shipId: ships[0]?.id || 'ship-dev',
      squadId: 'none',
      modelProfile: 'Claude 3.7 Sonnet (Reasoning)',
      selectedSkill: trainingSkills[0]?.name || 'Incident Briefing Officer',
      selectedSteering: steeringDirectives[0]?.guidance || 'Cite approved sources and separate evidence from inference.',
      authority: 'draft_only',
      memoryScope: 'ship'
    });
    setIsManualModalOpen(true);
  };

  const handleSaveManualCrew = () => {
    if (!manualForm.name.trim() || !manualForm.role.trim()) return;

    const assignedSquad = squads.find((s) => s.id === manualForm.squadId);
    const assignedShipId = assignedSquad?.shipId || manualForm.shipId;

    addCrewMember({
      name: manualForm.name,
      role: manualForm.role,
      purpose: manualForm.purpose || `Specialist performing ${manualForm.role} tasks.`,
      shipId: assignedShipId,
      squadId: manualForm.squadId === 'none' ? undefined : manualForm.squadId,
      modelProfile: manualForm.modelProfile,
      skills: [manualForm.selectedSkill],
      steering: manualForm.selectedSteering,
      tools: ['local_filesystem', 'task_runner'],
      authority: manualForm.authority,
      memoryScope: manualForm.memoryScope,
      status: 'active',
      costLast30Days: 0.0
    });

    setIsManualModalOpen(false);
    showToast(`Added specialist: ${manualForm.name}`);
  };

  const handleGenerateAiCrew = () => {
    if (!aiPrompt.trim()) return;
    setIsGeneratingAiCrew(true);

    setTimeout(() => {
      setIsGeneratingAiCrew(false);
      const lower = aiPrompt.toLowerCase();
      let derivedName = 'Telemetry & Performance Auditor';
      let derivedRole = 'Concurrency & Query Profiler';
      let derivedPurpose = 'Diagnose hot database queries, analyze latency anomalies, and profile memory footprint.';
      let derivedModel = 'Claude 3.7 Sonnet (Reasoning)';
      let derivedSquad = squads[0]?.id || 'none';
      let derivedSkill = trainingSkills[2]?.name || 'Fleet Status Analyst';
      let derivedSteering = steeringDirectives[1]?.guidance || 'Prioritize severity classification before proposing remediation.';

      if (lower.includes('security') || lower.includes('auth') || lower.includes('policy')) {
        derivedName = 'Vulnerability & Policy Sentry';
        derivedRole = 'Access Boundary & Secret Auditor';
        derivedPurpose = 'Enforces zero-trust token inspection and scans repositories for secret exposures.';
        derivedSkill = trainingSkills[1]?.name || 'Cargo Manifest Reviewer';
        derivedSteering = 'Protect confidential operational data at all failure boundaries.';
      } else if (lower.includes('market') || lower.includes('copy') || lower.includes('brand')) {
        derivedName = 'Audience Intelligence Strategist';
        derivedRole = 'Market Signal & Positioning Specialist';
        derivedPurpose = 'Analyzes competitive launches and formulates high-signal narrative alignment.';
        derivedModel = 'Claude 3.5 Sonnet';
        derivedSquad = squads.find((s) => s.name.toLowerCase().includes('market'))?.id || 'none';
        derivedSkill = trainingSkills[3]?.name || 'Research Briefing Officer';
        derivedSteering = 'Use concise, brand-consistent language for user-facing copy.';
      }

      setProposedAiCrew({
        name: derivedName,
        role: derivedRole,
        purpose: derivedPurpose,
        shipId: ships[0]?.id || 'ship-dev',
        squadId: derivedSquad,
        modelProfile: derivedModel,
        skills: [derivedSkill],
        steering: derivedSteering,
        authority: 'draft_only',
        memoryScope: 'ship'
      });
    }, 850);
  };

  const handleSaveAiCrew = () => {
    if (!proposedAiCrew) return;

    addCrewMember({
      name: proposedAiCrew.name,
      role: proposedAiCrew.role,
      purpose: proposedAiCrew.purpose,
      shipId: proposedAiCrew.shipId,
      squadId: proposedAiCrew.squadId === 'none' ? undefined : proposedAiCrew.squadId,
      modelProfile: proposedAiCrew.modelProfile,
      skills: proposedAiCrew.skills,
      steering: proposedAiCrew.steering,
      tools: ['local_filesystem', 'task_runner'],
      authority: proposedAiCrew.authority,
      memoryScope: proposedAiCrew.memoryScope,
      status: 'active',
      costLast30Days: 0.0
    });

    setIsAiModalOpen(false);
    showToast(`Quartermaster commissioned ${proposedAiCrew.name}!`);
  };

  const handleSaveEditCrew = () => {
    if (!editingCrew) return;
    updateCrewMember(editingCrew.id, {
      ...editingCrew
    });
    setEditingCrew(null);
    showToast(`Updated ${editingCrew.name}`);
  };

  // Squad options for SelectDropdown
  const squadOptions = [
    { value: 'none', label: 'No Squad (Non-Squad)' },
    ...squads.map((sq) => ({ value: sq.id, label: sq.name }))
  ];

  // Skill options from Training Officer
  const skillOptions = trainingSkills.map((sk) => ({ value: sk.name, label: sk.name }));

  // Steering options from Training Officer
  const steeringOptions = steeringDirectives.map((sd) => ({
    value: sd.guidance,
    label: `${sd.name}: ${sd.guidance.substring(0, 48)}...`
  }));

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto px-4 sm:px-6 pt-0 pb-6 space-y-3 sm:space-y-4 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Toast Alert */}
      {toastMessage && (
        <div className="fixed top-4 right-4 z-50 px-4 py-2 rounded-xl bg-teal-600 text-white text-xs font-semibold shadow-lg flex items-center gap-2 animate-in fade-in duration-200">
          <CheckCircle2 className="w-4 h-4" />
          <span>{toastMessage}</span>
        </div>
      )}

      {/* Reusable General Header with Squad-mapped Chips and Add Crew button */}
      <PageHeaderNav
        icon={<Users className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Crew Members"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            {crew.length} / 15 Berths Active
          </span>
        }
        description="Persistent AI specialists mapped into squads, with defined model profiles, skills, and steering."
        search={{
          value: search,
          onChange: setSearch,
          placeholder: 'Search crew by role, skills...'
        }}
        actions={
          <Dropdown
            title="Add Crew Options"
            align="right"
            menuWidth="w-72"
            items={[
              {
                id: 'manual',
                label: 'Manual Specialist Creation',
                description: 'Define crew name, AI model, squad, training skill, and steering'
              },
              {
                id: 'ai',
                label: 'Ask Quartermaster (AI Gen)',
                description: 'Quartermaster maps optimal specialist profile from your description'
              }
            ]}
            onSelect={(id) => {
              if (id === 'manual') handleOpenManualAdd();
              else {
                setAiPrompt('');
                setProposedAiCrew(null);
                setIsAiModalOpen(true);
              }
            }}
            trigger={
              <Button
                variant="primary"
                size="sm"
                icon={<Plus className="w-3.5 h-3.5" />}
                shortLabel="Add Crew"
                title="Add New Crew Member"
              >
                Add Crew
              </Button>
            }
          />
        }
        chips={{
          items: [
            { id: 'all', label: 'All Squads', count: crew.length },
            {
              id: 'non-squad',
              label: 'Non-Squad',
              count: crew.filter((c) => !c.squadId || c.squadId === 'none').length
            },
            ...squads.map((sq) => ({
              id: sq.id,
              label: sq.name,
              count: crew.filter((c) => c.squadId === sq.id).length
            }))
          ],
          selectedId: selectedSquadFilter,
          onSelect: setSelectedSquadFilter,
          variant: 'pills'
        }}
      />

      {/* Crew Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {filteredCrew.map((member) => {
          const ship = ships.find((s) => s.id === member.shipId);
          const squadObj = squads.find((sq) => sq.id === member.squadId);
          return (
            <ItemCard
              key={member.id}
              selected={editingCrew?.id === member.id}
              onClick={() => setEditingCrew(member)}
              title={member.name}
              subtitle={member.role}
              badge={
                <div className="flex items-center gap-1">
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
                  <span className="text-[9px] font-mono px-1.5 py-0.2 rounded bg-neutral-200 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300 font-bold">
                    {squadObj ? squadObj.name : 'Non-Squad'}
                  </span>
                </div>
              }
              description={member.purpose}
              tags={member.skills}
              footer={
                <div className="space-y-2">
                  {member.steering && (
                    <div className="p-1.5 rounded-lg bg-neutral-100 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 text-[10px] text-neutral-700 dark:text-neutral-300 font-mono line-clamp-1 flex items-center gap-1">
                      <Compass className="w-3 h-3 text-teal-500 shrink-0" />
                      <span className="truncate">Steering: {member.steering}</span>
                    </div>
                  )}

                  <div className="grid grid-cols-2 gap-2 text-[10px] font-mono text-neutral-500">
                    <div>
                      <span className="text-neutral-400 block">Model Profile</span>
                      <span className="text-neutral-800 dark:text-neutral-200 truncate block">
                        {member.modelProfile}
                      </span>
                    </div>
                    <div>
                      <span className="text-neutral-400 block">Department Ship</span>
                      <span className="text-neutral-800 dark:text-neutral-200 block truncate">
                        {ship?.name || 'Developer Delivery'}
                      </span>
                    </div>
                  </div>
                  <div className="pt-2 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-between text-[11px] font-mono">
                    <span className="text-neutral-400">
                      Squad: {squadObj?.name || 'Unassigned'}
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

      {/* MODAL 1: MANUAL ADD CREW */}
      <Modal
        isOpen={isManualModalOpen}
        onClose={() => setIsManualModalOpen(false)}
        title="Add Crew Specialist (Manual)"
        description="Configure specialist identity, model, squad assignment, training skills, and steering directives."
        maxWidth="xl"
      >
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Specialist Name*
            </label>
            <input
              type="text"
              value={manualForm.name}
              onChange={(e) => setManualForm({ ...manualForm, name: e.target.value })}
              placeholder="e.g. Infrastructure Sentry, Code Modernization Engineer"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Role / Title*
            </label>
            <input
              type="text"
              value={manualForm.role}
              onChange={(e) => setManualForm({ ...manualForm, role: e.target.value })}
              placeholder="e.g. Static Analysis &amp; Vulnerability Auditor"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Operational Purpose*
            </label>
            <textarea
              rows={2}
              value={manualForm.purpose}
              onChange={(e) => setManualForm({ ...manualForm, purpose: e.target.value })}
              placeholder="Describe the operational mandate and core duties of this specialist..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none resize-none font-mono"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                AI Model Profile
              </label>
              <SelectDropdown
                value={manualForm.modelProfile}
                onChange={(v) => setManualForm({ ...manualForm, modelProfile: v })}
                options={[
                  'Claude 3.7 Sonnet (Reasoning)',
                  'Claude 3.7 Sonnet',
                  'Gemini 2.5 Pro (Balanced)',
                  'Gemini 2.5 Flash',
                  'Claude 3.5 Haiku (Fast & Precise)',
                  'DeepSeek R1 / Local Ollama'
                ]}
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Squad Mapping (Default: No Squad)
              </label>
              <SelectDropdown
                value={manualForm.squadId}
                onChange={(v) => setManualForm({ ...manualForm, squadId: v })}
                options={squadOptions}
              />
            </div>
          </div>

          {/* Training Skills Dropdown (from Training Officer) */}
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1 flex items-center justify-between">
              <span>Training Skill (from Training Officer)</span>
              <button
                type="button"
                onClick={() => {
                  setIsManualModalOpen(false);
                  setActiveTab('training-officer');
                }}
                className="text-[10px] text-teal-600 dark:text-teal-400 hover:underline"
              >
                Configure Skills →
              </button>
            </label>
            <SelectDropdown
              value={manualForm.selectedSkill}
              onChange={(v) => setManualForm({ ...manualForm, selectedSkill: v })}
              options={skillOptions.length > 0 ? skillOptions : ['Incident Briefing Officer', 'Cargo Manifest Reviewer']}
            />
          </div>

          {/* Steering Dropdown (from Training Officer) */}
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1 flex items-center justify-between">
              <span>Steering Directive (from Training Officer)</span>
              <button
                type="button"
                onClick={() => {
                  setIsManualModalOpen(false);
                  setActiveTab('training-officer');
                }}
                className="text-[10px] text-teal-600 dark:text-teal-400 hover:underline"
              >
                Configure Steering →
              </button>
            </label>
            <SelectDropdown
              value={manualForm.selectedSteering}
              onChange={(v) => setManualForm({ ...manualForm, selectedSteering: v })}
              options={steeringOptions.length > 0 ? steeringOptions : ['Cite approved sources and separate evidence from inference.']}
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Authority Level
              </label>
              <SelectDropdown
                value={manualForm.authority}
                onChange={(v) => setManualForm({ ...manualForm, authority: v as any })}
                options={[
                  { value: 'read_only', label: 'Read Only (Inspect & Audit)' },
                  { value: 'draft_only', label: 'Draft Only (Requires Approval)' },
                  { value: 'gated_write', label: 'Gated Write (Autonomous Staging)' }
                ]}
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Memory Scope
              </label>
              <SelectDropdown
                value={manualForm.memoryScope}
                onChange={(v) => setManualForm({ ...manualForm, memoryScope: v as any })}
                options={[
                  { value: 'ship', label: 'Ship Scoped' },
                  { value: 'workspace', label: 'Workspace Scoped' },
                  { value: 'crew', label: 'Isolated Crew' }
                ]}
              />
            </div>
          </div>

          <div className="flex justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
            <Button variant="ghost" size="sm" onClick={() => setIsManualModalOpen(false)}>
              Cancel
            </Button>
            <Button variant="primary" size="sm" onClick={handleSaveManualCrew}>
              Add Crew Member
            </Button>
          </div>
        </div>
      </Modal>

      {/* MODAL 2: QUARTERMASTER GEN AI ADD CREW */}
      <Modal
        isOpen={isAiModalOpen}
        onClose={() => setIsAiModalOpen(false)}
        title="Consult Quartermaster (AI Specialist Crafting)"
        description="Describe your specialist requirements. Quartermaster will determine optimal role, model, skills, and steering."
        maxWidth="2xl"
      >
        <div className="space-y-4">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Describe the specialist you need:
            </label>
            <textarea
              rows={3}
              value={aiPrompt}
              onChange={(e) => setAiPrompt(e.target.value)}
              placeholder="e.g. We need an autonomous Vulnerability Pen-tester to audit Go concurrency locks, verify secret hygiene, and scan incoming dependency advisories..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500 resize-none font-mono"
            />
          </div>

          <div className="flex justify-end">
            <Button
              variant="primary"
              size="sm"
              disabled={!aiPrompt.trim() || isGeneratingAiCrew}
              onClick={handleGenerateAiCrew}
              icon={<Sparkles className="w-3.5 h-3.5" />}
            >
              {isGeneratingAiCrew ? 'Quartermaster is drafting specialist profile…' : 'Synthesize Specialist'}
            </Button>
          </div>

          {/* AI Output Preview and Edit */}
          {proposedAiCrew && (
            <div className="p-4 rounded-xl border border-teal-500/30 bg-teal-500/5 space-y-3 animate-in fade-in duration-200">
              <div className="flex items-center justify-between">
                <span className="text-xs font-bold text-teal-600 dark:text-teal-400 uppercase tracking-wider flex items-center gap-1.5">
                  <CheckCircle2 className="w-3.5 h-3.5" />
                  Quartermaster Proposed Specialist
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/20 text-teal-500 font-semibold">
                  Review &amp; Edit Before Saving
                </span>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                    Specialist Name
                  </label>
                  <input
                    type="text"
                    value={proposedAiCrew.name}
                    onChange={(e) => setProposedAiCrew({ ...proposedAiCrew, name: e.target.value })}
                    className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-bold"
                  />
                </div>
                <div>
                  <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                    Role / Mandate
                  </label>
                  <input
                    type="text"
                    value={proposedAiCrew.role}
                    onChange={(e) => setProposedAiCrew({ ...proposedAiCrew, role: e.target.value })}
                    className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-bold"
                  />
                </div>
              </div>

              <div>
                <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                  Purpose Statement
                </label>
                <textarea
                  rows={2}
                  value={proposedAiCrew.purpose}
                  onChange={(e) => setProposedAiCrew({ ...proposedAiCrew, purpose: e.target.value })}
                  className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-mono resize-none"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                    Assigned Squad
                  </label>
                  <SelectDropdown
                    value={proposedAiCrew.squadId}
                    onChange={(v) => setProposedAiCrew({ ...proposedAiCrew, squadId: v })}
                    options={squadOptions}
                  />
                </div>
                <div>
                  <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                    AI Model Profile
                  </label>
                  <SelectDropdown
                    value={proposedAiCrew.modelProfile}
                    onChange={(v) => setProposedAiCrew({ ...proposedAiCrew, modelProfile: v })}
                    options={[
                      'Claude 3.7 Sonnet (Reasoning)',
                      'Claude 3.7 Sonnet',
                      'Gemini 2.5 Pro (Balanced)',
                      'Gemini 2.5 Flash',
                      'Claude 3.5 Haiku (Fast & Precise)',
                      'DeepSeek R1 / Local Ollama'
                    ]}
                  />
                </div>
              </div>

              <div>
                <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                  Assigned Steering Directive
                </label>
                <input
                  type="text"
                  value={proposedAiCrew.steering}
                  onChange={(e) => setProposedAiCrew({ ...proposedAiCrew, steering: e.target.value })}
                  className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-mono"
                />
              </div>

              <div className="pt-2 flex justify-end gap-2 border-t border-teal-500/20">
                <Button variant="ghost" size="sm" onClick={() => setProposedAiCrew(null)}>
                  Discard
                </Button>
                <Button variant="primary" size="sm" onClick={handleSaveAiCrew} icon={<Check className="w-3.5 h-3.5" />}>
                  Save &amp; Add Crew
                </Button>
              </div>
            </div>
          )}
        </div>
      </Modal>

      {/* EDIT EXISTING CREW MODAL */}
      {editingCrew && (
        <Modal
          isOpen={Boolean(editingCrew)}
          onClose={() => setEditingCrew(null)}
          title={`Edit Specialist: ${editingCrew.name}`}
          description="Update specialist identity, squad mapping, training skills, and steering boundaries."
          maxWidth="xl"
        >
          <div className="space-y-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Specialist Name
              </label>
              <input
                type="text"
                value={editingCrew.name}
                onChange={(e) => setEditingCrew({ ...editingCrew, name: e.target.value })}
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs font-semibold"
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Role / Title
              </label>
              <input
                type="text"
                value={editingCrew.role}
                onChange={(e) => setEditingCrew({ ...editingCrew, role: e.target.value })}
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs font-semibold"
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                  Squad Assignment
                </label>
                <SelectDropdown
                  value={editingCrew.squadId || 'none'}
                  onChange={(v) => setEditingCrew({ ...editingCrew, squadId: v === 'none' ? undefined : v })}
                  options={squadOptions}
                />
              </div>

              <div>
                <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                  AI Model Profile
                </label>
                <SelectDropdown
                  value={editingCrew.modelProfile}
                  onChange={(v) => setEditingCrew({ ...editingCrew, modelProfile: v })}
                  options={[
                    'Claude 3.7 Sonnet (Reasoning)',
                    'Claude 3.7 Sonnet',
                    'Gemini 2.5 Pro (Balanced)',
                    'Gemini 2.5 Flash',
                    'Claude 3.5 Haiku (Fast & Precise)',
                    'DeepSeek R1 / Local Ollama'
                  ]}
                />
              </div>
            </div>

            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Steering Directive
              </label>
              <input
                type="text"
                value={editingCrew.steering || ''}
                onChange={(e) => setEditingCrew({ ...editingCrew, steering: e.target.value })}
                placeholder="Operational guidance for this specialist..."
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs font-mono"
              />
            </div>

            <div className="flex justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
              <Button variant="ghost" size="sm" onClick={() => setEditingCrew(null)}>
                Cancel
              </Button>
              <Button variant="primary" size="sm" onClick={handleSaveEditCrew}>
                Save Changes
              </Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
};

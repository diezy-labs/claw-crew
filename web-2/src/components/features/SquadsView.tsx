import React, { useState } from 'react';
import {
  ShieldCheck,
  Users,
  Sparkles,
  Plus,
  Search,
  Ship,
  Edit2,
  Trash2,
  UserPlus,
  Compass,
  CheckCircle2,
  AlertTriangle,
  Layers,
  ChevronDown,
  ArrowRight,
  Shield,
  Cpu,
  Brain,
  Check,
  X,
  UserX
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { Squad, CrewMember } from '../../types';
import { PageHeaderNav } from '../common/PageHeaderNav';
import { Button } from '../common/Button';
import { Modal } from '../common/Modal';
import { Dropdown, SelectDropdown } from '../common/Dropdown';
import { ItemCard } from '../common/ItemCard';

export const SquadsView: React.FC = () => {
  const {
    squads,
    crew,
    ships,
    trainingSkills,
    steeringDirectives,
    addSquad,
    updateSquad,
    deleteSquad,
    addCrewMember,
    updateCrewMember
  } = useFleetStore();

  // Selected chip: 'non-squad' (default), squad id, or 'all'
  const [selectedChip, setSelectedChip] = useState<string>('non-squad');
  const [search, setSearch] = useState('');

  // Modals
  const [isManualModalOpen, setIsManualModalOpen] = useState(false);
  const [isAiModalOpen, setIsAiModalOpen] = useState(false);
  const [editingSquad, setEditingSquad] = useState<Squad | null>(null);
  const [deleteConfirmation, setDeleteConfirmation] = useState<Squad | null>(null);
  const [toastMessage, setToastMessage] = useState<string | null>(null);

  // Manual Form State
  const [squadName, setSquadName] = useState('');
  const [squadShipId, setSquadShipId] = useState<string>(ships[0]?.id || 'ship-dev');
  const [squadPurpose, setSquadPurpose] = useState('');
  const [selectedCrewIds, setSelectedCrewIds] = useState<string[]>([]);

  // Gen AI Quartermaster State
  const [aiPrompt, setAiPrompt] = useState('');
  const [isGeneratingAiSquad, setIsGeneratingAiSquad] = useState(false);
  const [proposedAiSquad, setProposedAiSquad] = useState<{
    squadName: string;
    shipId: string;
    purpose: string;
    crewMembers: Array<{
      name: string;
      role: string;
      purpose: string;
      modelProfile: string;
      skills: string[];
      steering: string;
      authority: 'read_only' | 'draft_only' | 'gated_write';
    }>;
  } | null>(null);

  const showToast = (msg: string) => {
    setToastMessage(msg);
    setTimeout(() => setToastMessage(null), 3000);
  };

  const nonSquadCrew = crew.filter((c) => !c.squadId || c.squadId === 'none');

  // Open Manual Modal
  const handleOpenManualCreate = () => {
    setEditingSquad(null);
    setSquadName('');
    setSquadShipId(ships[0]?.id || 'ship-dev');
    setSquadPurpose('');
    setSelectedCrewIds([]);
    setIsManualModalOpen(true);
  };

  const handleOpenManualEdit = (sq: Squad) => {
    setEditingSquad(sq);
    setSquadName(sq.name);
    setSquadShipId(sq.shipId || ships[0]?.id || 'ship-dev');
    setSquadPurpose(sq.purpose);
    setSelectedCrewIds([...sq.crewIds]);
    setIsManualModalOpen(true);
  };

  const handleSaveManualSquad = () => {
    if (!squadName.trim() || !squadPurpose.trim()) return;

    if (editingSquad) {
      updateSquad(editingSquad.id, {
        name: squadName,
        shipId: squadShipId,
        purpose: squadPurpose,
        crewIds: selectedCrewIds
      });
      showToast(`Updated squad: ${squadName}`);
    } else {
      const newId = addSquad({
        name: squadName,
        shipId: squadShipId,
        purpose: squadPurpose,
        crewIds: selectedCrewIds,
        status: 'active'
      });
      setSelectedChip(newId);
      showToast(`Commissioned new squad: ${squadName}`);
    }
    setIsManualModalOpen(false);
  };

  // Open AI Quartermaster Modal
  const handleOpenAiModal = () => {
    setAiPrompt('');
    setProposedAiSquad(null);
    setIsAiModalOpen(true);
  };

  const handleGenerateAiSquad = () => {
    if (!aiPrompt.trim()) return;
    setIsGeneratingAiSquad(true);

    setTimeout(() => {
      setIsGeneratingAiSquad(false);
      // Synthesize intelligent squad mapping with skills and steering
      const lower = aiPrompt.toLowerCase();
      let derivedName = 'Squad Performance & Reliability';
      let derivedPurpose = 'System benchmark analysis, query optimization, and latency regression prevention.';
      let derivedShip = ships[0]?.id || 'ship-dev';

      if (lower.includes('market') || lower.includes('growth') || lower.includes('content')) {
        derivedName = 'Squad Growth & Acquisition';
        derivedPurpose = 'Execute multi-channel audience discovery, copywriting campaigns, and competitive benchmarking.';
        derivedShip = ships.find((s) => s.id === 'ship-market')?.id || derivedShip;
      } else if (lower.includes('security') || lower.includes('audit') || lower.includes('policy')) {
        derivedName = 'Squad Security Sentinel';
        derivedPurpose = 'Conduct repository vulnerability audits, access gate enforcement, and secret protection.';
      }

      setProposedAiSquad({
        squadName: derivedName,
        shipId: derivedShip,
        purpose: derivedPurpose,
        crewMembers: [
          {
            name: `${derivedName.replace('Squad ', '')} Lead`,
            role: 'Principal Systems Strategist',
            purpose: 'Formulates phased sprint execution plans and dependency charts.',
            modelProfile: 'Claude 3.7 Sonnet (Reasoning)',
            skills: ['AST Parsing', 'Dependency Graphing', 'Architecture Breakdown'],
            steering: 'Cite approved sources and separate evidence from inference.',
            authority: 'draft_only'
          },
          {
            name: `${derivedName.replace('Squad ', '')} Specialist`,
            role: 'Tactical Execution & Quality Officer',
            purpose: 'Audits edge cases, executes verification scripts, and prepares evidence logs.',
            modelProfile: 'Gemini 2.5 Pro (Balanced)',
            skills: ['Regression Hunting', 'Incident Briefing Officer', 'Knowledge Base Navigator'],
            steering: 'Return structured output for fleet workflows.',
            authority: 'gated_write'
          }
        ]
      });
    }, 900);
  };

  const handleSaveAiSquad = () => {
    if (!proposedAiSquad) return;

    // 1. Create the crew members proposed by Quartermaster
    const createdCrewIds: string[] = [];
    proposedAiSquad.crewMembers.forEach((c) => {
      const newCrewId = 'crew-' + Date.now() + '-' + Math.random().toString(36).substring(2, 6);
      addCrewMember({
        name: c.name,
        shipId: proposedAiSquad.shipId,
        role: c.role,
        purpose: c.purpose,
        skills: c.skills,
        steering: c.steering,
        tools: ['local_filesystem', 'task_runner'],
        modelProfile: c.modelProfile,
        authority: c.authority,
        memoryScope: 'ship',
        status: 'active',
        costLast30Days: 0.00
      });
      createdCrewIds.push(newCrewId);
    });

    // 2. Create the Squad
    const newSquadId = addSquad({
      name: proposedAiSquad.squadName,
      shipId: proposedAiSquad.shipId,
      purpose: proposedAiSquad.purpose,
      crewIds: createdCrewIds,
      status: 'active'
    });

    setIsAiModalOpen(false);
    setSelectedChip(newSquadId);
    showToast(`Quartermaster commissioned ${proposedAiSquad.squadName} with ${createdCrewIds.length} specialists!`);
  };

  // Toggle crew member selection in manual modal
  const handleToggleCrewSelection = (crewId: string) => {
    setSelectedCrewIds((prev) =>
      prev.includes(crewId) ? prev.filter((id) => id !== crewId) : [...prev, crewId]
    );
  };

  // Assign a non-squad crew directly to an existing squad
  const handleAssignNonSquadToSquad = (crewMember: CrewMember, targetSquadId: string) => {
    const targetSquad = squads.find((s) => s.id === targetSquadId);
    if (!targetSquad) return;
    updateCrewMember(crewMember.id, {
      squadId: targetSquadId,
      shipId: targetSquad.shipId || crewMember.shipId
    });
    updateSquad(targetSquadId, {
      crewIds: Array.from(new Set([...targetSquad.crewIds, crewMember.id]))
    });
    showToast(`Assigned ${crewMember.name} to ${targetSquad.name}`);
  };

  const handleDeleteSquadConfirmed = () => {
    if (!deleteConfirmation) return;
    deleteSquad(deleteConfirmation.id);
    showToast(`Disbanded squad: ${deleteConfirmation.name}`);
    setDeleteConfirmation(null);
    setSelectedChip('non-squad');
  };

  // Build chips according to user specification:
  // Default first chip is Non-Squad, followed by Squads, then All Squads
  const chipItems = [
    {
      id: 'non-squad',
      label: 'Non-Squad',
      count: nonSquadCrew.length
    },
    ...squads.map((sq) => ({
      id: sq.id,
      label: sq.name,
      count: sq.crewIds.length
    })),
    {
      id: 'all',
      label: 'All Squads',
      count: squads.length
    }
  ];

  const activeSquad = squads.find((sq) => sq.id === selectedChip);
  const activeSquadCrew = activeSquad ? crew.filter((c) => activeSquad.crewIds.includes(c.id)) : [];

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto px-4 sm:px-6 pt-0 pb-6 space-y-4 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Toast Alert */}
      {toastMessage && (
        <div className="fixed top-4 right-4 z-50 px-4 py-2 rounded-xl bg-teal-600 text-white text-xs font-semibold shadow-lg flex items-center gap-2 animate-in fade-in duration-200">
          <CheckCircle2 className="w-4 h-4" />
          <span>{toastMessage}</span>
        </div>
      )}

      {/* Page Header with New Squad 2-Option Action */}
      <PageHeaderNav
        icon={<ShieldCheck className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Squad"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            {squads.length} Tactical Squads
          </span>
        }
        description="Cross-functional AI teams mapped within Department Ships executing coordinated goals."
        search={{
          value: search,
          onChange: setSearch,
          placeholder: 'Search squads or crew members...'
        }}
        actions={
          <Dropdown
            title="New Squad Options"
            align="right"
            menuWidth="w-72"
            items={[
              {
                id: 'manual',
                label: 'Manual Squad Mapping',
                description: 'Define squad name, parent ship, and choose from crew roster'
              },
              {
                id: 'ai',
                label: 'Ask Quartermaster (AI Gen)',
                description: 'Quartermaster drafts squad size, skills, and steering from description'
              }
            ]}
            onSelect={(id) => {
              if (id === 'manual') handleOpenManualCreate();
              else handleOpenAiModal();
            }}
            trigger={
              <Button
                variant="primary"
                size="sm"
                icon={<Plus className="w-3.5 h-3.5" />}
                shortLabel="+ Squad"
                title="Create New Squad"
              >
                New Squad
              </Button>
            }
          />
        }
        chips={{
          items: chipItems,
          selectedId: selectedChip,
          onSelect: setSelectedChip,
          variant: 'pills'
        }}
      />

      {/* VIEW 1: NON-SQUAD CREW MEMBERS (DEFAULT CHIP) */}
      {selectedChip === 'non-squad' && (
        <div className="space-y-4">
          <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex items-center justify-between gap-3 shadow-xs">
            <div className="flex items-center gap-3">
              <div className="w-9 h-9 rounded-lg bg-amber-500/10 text-amber-500 flex items-center justify-center shrink-0">
                <UserX className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-xs sm:text-sm font-bold text-neutral-900 dark:text-neutral-100">
                  Unassigned Specialists ({nonSquadCrew.length})
                </h3>
                <p className="text-xs text-neutral-500 dark:text-neutral-400">
                  These crew members operate independently and have not yet been assigned to any cross-functional squad.
                </p>
              </div>
            </div>

            {nonSquadCrew.length > 0 && (
              <Button variant="outline" size="sm" onClick={handleOpenManualCreate} icon={<Plus className="w-3 h-3" />}>
                Form Squad from Crew
              </Button>
            )}
          </div>

          {nonSquadCrew.length === 0 ? (
            <div className="p-8 text-center rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-800 space-y-2 bg-white/50 dark:bg-[#15171a]/50">
              <CheckCircle2 className="w-8 h-8 mx-auto text-emerald-500" />
              <p className="text-xs font-semibold text-neutral-800 dark:text-neutral-200">
                All specialists are actively mapped into squads!
              </p>
              <p className="text-xs text-neutral-500">
                Every crew member in the fleet has an assigned tactical squad.
              </p>
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3.5">
              {nonSquadCrew.map((member) => (
                <ItemCard
                  key={member.id}
                  title={member.name}
                  subtitle={member.role}
                  badge={
                    <span className="text-[10px] font-mono px-2 py-0.5 rounded-full font-bold uppercase bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20">
                      Unassigned
                    </span>
                  }
                  footer={
                    <div className="text-[11px] text-neutral-400 font-mono mt-1 flex items-center gap-1.5">
                      <Cpu className="w-3 h-3 text-teal-500" />
                      <span className="truncate">{member.modelProfile}</span>
                    </div>
                  }
                  actions={
                    <Dropdown
                      title="Assign to Squad"
                      align="right"
                      menuWidth="w-56"
                      items={squads.map((sq) => ({
                        id: sq.id,
                        label: sq.name,
                        description: `Ship: ${ships.find((s) => s.id === sq.shipId)?.name || 'General'}`
                      }))}
                      onSelect={(sqId) => handleAssignNonSquadToSquad(member, sqId)}
                      trigger={
                        <Button
                          variant="outline"
                          size="xs"
                          icon={<UserPlus className="w-3 h-3 text-teal-500" />}
                          title="Assign to Squad"
                        >
                          Assign
                        </Button>
                      }
                    />
                  }
                >
                  <p className="text-xs text-neutral-600 dark:text-neutral-300 line-clamp-2 mb-2">
                    {member.purpose}
                  </p>
                  <div className="flex flex-wrap gap-1">
                    {member.skills?.slice(0, 3).map((sk) => (
                      <span
                        key={sk}
                        className="text-[10px] px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300 font-mono"
                      >
                        {sk}
                      </span>
                    ))}
                  </div>
                </ItemCard>
              ))}
            </div>
          )}
        </div>
      )}

      {/* VIEW 2: SPECIFIC SQUAD SELECTED */}
      {activeSquad && (
        <div className="space-y-4">
          {/* Squad Showcase Card */}
          <div className="p-4 sm:p-5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] shadow-xs space-y-3">
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="flex items-center gap-2 mb-1">
                  <h2 className="text-base sm:text-lg font-bold text-neutral-900 dark:text-neutral-100">
                    {activeSquad.name}
                  </h2>
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded-full font-bold uppercase bg-teal-500/10 text-teal-600 dark:text-teal-400 border border-teal-500/20">
                    {activeSquad.crewIds.length} Members
                  </span>
                </div>
                <p className="text-xs text-neutral-600 dark:text-neutral-300 leading-relaxed max-w-2xl">
                  {activeSquad.purpose}
                </p>
                <div className="flex items-center gap-3 text-xs text-neutral-400 mt-2 font-mono">
                  <span className="flex items-center gap-1">
                    <Ship className="w-3.5 h-3.5 text-teal-500" />
                    Department: {ships.find((s) => s.id === activeSquad.shipId)?.name || 'Fleet Core'}
                  </span>
                </div>
              </div>

              <div className="flex items-center gap-1.5 shrink-0">
                <Button
                  variant="outline"
                  size="sm"
                  icon={<Edit2 className="w-3.5 h-3.5" />}
                  onClick={() => handleOpenManualEdit(activeSquad)}
                  title="Edit Squad"
                >
                  Edit Squad
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<Trash2 className="w-3.5 h-3.5 text-rose-500" />}
                  onClick={() => setDeleteConfirmation(activeSquad)}
                  title="Disband Squad"
                />
              </div>
            </div>
          </div>

          {/* Members of this Squad */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <h3 className="text-xs font-bold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                Squad Specialists Roster
              </h3>
              <Button variant="outline" size="xs" onClick={() => handleOpenManualEdit(activeSquad)} icon={<UserPlus className="w-3 h-3" />}>
                Manage Crew
              </Button>
            </div>

            {activeSquadCrew.length === 0 ? (
              <div className="p-6 text-center rounded-xl border border-dashed border-neutral-300 dark:border-neutral-800 space-y-2 bg-white/50 dark:bg-[#15171a]/50">
                <p className="text-xs text-neutral-500">No specialists currently assigned to this squad.</p>
                <Button variant="primary" size="xs" onClick={() => handleOpenManualEdit(activeSquad)}>
                  Assign Crew Members
                </Button>
              </div>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3.5">
                {activeSquadCrew.map((member) => (
                  <ItemCard
                    key={member.id}
                    title={member.name}
                    subtitle={member.role}
                    badge={
                      <span className="text-[10px] font-mono px-2 py-0.5 rounded-full font-bold uppercase bg-teal-500/10 text-teal-600 dark:text-teal-400 border border-teal-500/20">
                        {member.authority.replace('_', ' ')}
                      </span>
                    }
                    footer={
                      <div className="text-[11px] text-neutral-400 font-mono mt-1 flex items-center gap-1.5">
                        <Cpu className="w-3 h-3 text-teal-500" />
                        <span className="truncate">{member.modelProfile}</span>
                      </div>
                    }
                  >
                    <p className="text-xs text-neutral-600 dark:text-neutral-300 line-clamp-2 mb-2">
                      {member.purpose}
                    </p>

                    {member.steering && (
                      <div className="p-2 rounded-lg bg-neutral-100 dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 text-[11px] text-neutral-700 dark:text-neutral-300 mb-2 font-mono flex items-start gap-1">
                        <Compass className="w-3 h-3 text-teal-500 shrink-0 mt-0.5" />
                        <span className="line-clamp-2">Steering: {member.steering}</span>
                      </div>
                    )}

                    <div className="flex flex-wrap gap-1">
                      {member.skills?.map((sk) => (
                        <span
                          key={sk}
                          className="text-[10px] px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300 font-mono"
                        >
                          {sk}
                        </span>
                      ))}
                    </div>
                  </ItemCard>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {/* VIEW 3: ALL SQUADS OVERVIEW */}
      {selectedChip === 'all' && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {squads.map((sq) => {
            const mappedCrew = crew.filter((c) => sq.crewIds.includes(c.id));
            const shipObj = ships.find((s) => s.id === sq.shipId);
            return (
              <ItemCard
                key={sq.id}
                title={sq.name}
                subtitle={sq.purpose}
                badge={
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded-full font-bold uppercase bg-teal-500/10 text-teal-600 dark:text-teal-400 border border-teal-500/20">
                    {mappedCrew.length} Specialists
                  </span>
                }
                footer={
                  <div className="text-[11px] text-neutral-400 font-mono mt-1 flex items-center gap-1.5">
                    <Ship className="w-3 h-3 text-teal-500" />
                    <span>Department: {shipObj?.name || 'General Fleet'}</span>
                  </div>
                }
                actions={
                  <Button variant="outline" size="xs" onClick={() => setSelectedChip(sq.id)}>
                    View Squad
                  </Button>
                }
              >
                <div className="space-y-1.5 pt-1">
                  <span className="text-[10px] font-mono text-neutral-400 uppercase tracking-wider block">
                    Mapped Specialists:
                  </span>
                  <div className="flex flex-wrap gap-1.5">
                    {mappedCrew.map((c) => (
                      <span
                        key={c.id}
                        className="text-xs px-2 py-0.5 rounded-md bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 font-medium"
                      >
                        {c.name}
                      </span>
                    ))}
                  </div>
                </div>
              </ItemCard>
            );
          })}
        </div>
      )}

      {/* MODAL 1: MANUAL SQUAD MAPPING */}
      <Modal
        isOpen={isManualModalOpen}
        onClose={() => setIsManualModalOpen(false)}
        title={editingSquad ? 'Edit Squad' : 'New Squad (Manual Mapping)'}
        description="Configure squad name, department vessel, and assign specialists from the crew roster."
        maxWidth="xl"
      >
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Squad Name*
            </label>
            <input
              type="text"
              value={squadName}
              onChange={(e) => setSquadName(e.target.value)}
              placeholder="e.g. Squad Developer, Squad Marketing, Squad Engineer"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Parent Ship (Department)*
            </label>
            <SelectDropdown
              value={squadShipId}
              onChange={setSquadShipId}
              options={ships.map((s) => ({ value: s.id, label: s.name }))}
            />
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Squad Purpose &amp; Mandate*
            </label>
            <textarea
              rows={2}
              value={squadPurpose}
              onChange={(e) => setSquadPurpose(e.target.value)}
              placeholder="Define the primary operational objective of this squad..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none resize-none font-mono"
            />
          </div>

          <div>
            <div className="flex items-center justify-between mb-1.5">
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300">
                Select Crew Members ({selectedCrewIds.length} chosen)
              </label>
            </div>
            <div className="max-h-48 overflow-y-auto rounded-xl border border-neutral-200 dark:border-neutral-800 p-2 space-y-1.5 bg-neutral-50 dark:bg-neutral-950">
              {crew.map((member) => {
                const isSelected = selectedCrewIds.includes(member.id);
                return (
                  <div
                    key={member.id}
                    onClick={() => handleToggleCrewSelection(member.id)}
                    className={`p-2 rounded-lg border text-xs flex items-center justify-between cursor-pointer transition-colors ${
                      isSelected
                        ? 'border-teal-500 bg-teal-500/10 text-teal-900 dark:text-teal-200 font-semibold'
                        : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-neutral-700 dark:text-neutral-300 hover:border-neutral-300'
                    }`}
                  >
                    <div>
                      <span className="block font-bold">{member.name}</span>
                      <span className="text-[10px] text-neutral-400">{member.role}</span>
                    </div>
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
            <Button variant="primary" size="sm" onClick={handleSaveManualSquad}>
              {editingSquad ? 'Update Squad' : 'Save Squad'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* MODAL 2: QUARTERMASTER GEN AI SQUAD GENERATOR */}
      <Modal
        isOpen={isAiModalOpen}
        onClose={() => setIsAiModalOpen(false)}
        title="Consult Quartermaster (Gen AI Squad Crafting)"
        description="Describe your squad requirements. Quartermaster will determine optimal squad sizing, crew roles, skills, and steering."
        maxWidth="2xl"
      >
        <div className="space-y-4">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Describe your team needs &amp; objective:
            </label>
            <textarea
              rows={3}
              value={aiPrompt}
              onChange={(e) => setAiPrompt(e.target.value)}
              placeholder="e.g. Build an autonomous Performance Optimization Squad to audit DB query latency, generate regression tests, and enforce concise benchmark reports..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500 resize-none font-mono"
            />
          </div>

          <div className="flex justify-end">
            <Button
              variant="primary"
              size="sm"
              disabled={!aiPrompt.trim() || isGeneratingAiSquad}
              onClick={handleGenerateAiSquad}
              icon={<Sparkles className="w-3.5 h-3.5" />}
            >
              {isGeneratingAiSquad ? 'Quartermaster is drafting squad configuration…' : 'Synthesize Squad'}
            </Button>
          </div>

          {/* AI Output / Reviewable Sandbox */}
          {proposedAiSquad && (
            <div className="p-4 rounded-xl border border-teal-500/30 bg-teal-500/5 space-y-3 animate-in fade-in duration-200">
              <div className="flex items-center justify-between">
                <span className="text-xs font-bold text-teal-600 dark:text-teal-400 uppercase tracking-wider flex items-center gap-1.5">
                  <CheckCircle2 className="w-3.5 h-3.5" />
                  Quartermaster Proposed Squad Structure
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/20 text-teal-500 font-semibold">
                  Review &amp; Edit Before Saving
                </span>
              </div>

              <div className="space-y-2">
                <div>
                  <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                    Squad Name
                  </label>
                  <input
                    type="text"
                    value={proposedAiSquad.squadName}
                    onChange={(e) => setProposedAiSquad({ ...proposedAiSquad, squadName: e.target.value })}
                    className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-bold"
                  />
                </div>

                <div>
                  <label className="block text-[11px] font-bold text-neutral-700 dark:text-neutral-300 mb-0.5">
                    Squad Mandate
                  </label>
                  <textarea
                    rows={2}
                    value={proposedAiSquad.purpose}
                    onChange={(e) => setProposedAiSquad({ ...proposedAiSquad, purpose: e.target.value })}
                    className="w-full px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-xs font-mono resize-none"
                  />
                </div>
              </div>

              {/* Proposed Crew Members List */}
              <div className="space-y-2 pt-2 border-t border-teal-500/20">
                <span className="text-[11px] font-bold text-neutral-900 dark:text-neutral-100 uppercase tracking-wider">
                  Proposed Specialists ({proposedAiSquad.crewMembers.length})
                </span>
                <div className="space-y-2 max-h-56 overflow-y-auto pr-1">
                  {proposedAiSquad.crewMembers.map((cr, idx) => (
                    <div
                      key={idx}
                      className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs space-y-1.5"
                    >
                      <div className="flex items-center justify-between">
                        <span className="font-bold text-neutral-900 dark:text-neutral-100">{cr.name}</span>
                        <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-200 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300">
                          {cr.modelProfile}
                        </span>
                      </div>
                      <p className="text-[11px] text-neutral-500">{cr.role} · {cr.purpose}</p>
                      <div className="p-1.5 rounded bg-neutral-50 dark:bg-neutral-950 font-mono text-[10px] text-teal-600 dark:text-teal-400">
                        Steering: {cr.steering}
                      </div>
                      <div className="flex flex-wrap gap-1">
                        {cr.skills.map((sk) => (
                          <span key={sk} className="text-[9px] px-1.5 py-0.5 rounded bg-teal-500/10 text-teal-600 dark:text-teal-400">
                            {sk}
                          </span>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              <div className="pt-2 flex justify-end gap-2 border-t border-teal-500/20">
                <Button variant="ghost" size="sm" onClick={() => setProposedAiSquad(null)}>
                  Discard
                </Button>
                <Button variant="primary" size="sm" onClick={handleSaveAiSquad} icon={<Check className="w-3.5 h-3.5" />}>
                  Save &amp; Commission Squad
                </Button>
              </div>
            </div>
          )}
        </div>
      </Modal>

      {/* DISBAND SQUAD MODAL */}
      {deleteConfirmation && (
        <Modal
          isOpen={Boolean(deleteConfirmation)}
          onClose={() => setDeleteConfirmation(null)}
          title={`Disband ${deleteConfirmation.name}?`}
          description="Specialists in this squad will become unassigned Non-Squad crew members."
          maxWidth="md"
        >
          <div className="space-y-4">
            <p className="text-xs text-neutral-600 dark:text-neutral-300 leading-relaxed">
              Are you sure you want to disband this squad? No crew members will be deleted; they will simply return to the Non-Squad pool.
            </p>
            <div className="flex justify-end gap-2">
              <Button variant="ghost" size="sm" onClick={() => setDeleteConfirmation(null)}>
                Cancel
              </Button>
              <Button variant="danger" size="sm" onClick={handleDeleteSquadConfirmed}>
                Disband Squad
              </Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
};

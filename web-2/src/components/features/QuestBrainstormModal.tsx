import React, { useState } from 'react';
import {
  Sparkles,
  Map,
  Ship as ShipIcon,
  Users,
  Lightbulb,
  Plus,
  Trash2,
  CheckCircle2,
  Shield,
  Coins,
  MessageSquare,
  ArrowRight,
  Flame,
  Layers,
  Wand2
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { Quest, MapStep } from '../../types';

interface QuestBrainstormModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSendToChat?: (brief: string) => void;
}

interface BrainstormIdea {
  title: string;
  objective: string;
  shipId: string;
  crewIds: string[];
  priority: 'low' | 'medium' | 'high' | 'urgent';
  budgetUSD: number;
  steps: MapStep[];
  deliverables: string[];
}

const BRAINSTORM_IDEAS: BrainstormIdea[] = [
  {
    title: 'Integration Test Suite Teardown Stabilization',
    objective: 'Investigate socket listener timeout during test container teardown in claw-crew, eliminate 12s delay, and prepare CI patch.',
    shipId: 'ship-dev',
    crewIds: ['crew-repo-analyst', 'crew-qa-reviewer'],
    priority: 'urgent',
    budgetUSD: 1.80,
    deliverables: ['CI Triage Report #105', 'SetReadDeadline PR Patch'],
    steps: [
      { stepNumber: 1, title: 'Analyze Go runtime stack dumps from GitHub Actions runners', assignedCrewId: 'crew-repo-analyst', status: 'pending', outputArtifactType: 'goroutine-dump-analysis' },
      { stepNumber: 2, title: 'Reproduce socket read block in isolated test container', assignedCrewId: 'crew-qa-reviewer', status: 'pending', outputArtifactType: 'reproduction-log' },
      { stepNumber: 3, title: 'Draft SetReadDeadline timeout patch and run regression suite', assignedCrewId: 'crew-repo-analyst', status: 'pending', outputArtifactType: 'patch-draft' },
      { stepNumber: 4, title: 'Compile Captain’s Approval package for branch merge', assignedCrewId: 'crew-qa-reviewer', status: 'pending', outputArtifactType: 'approval-request' }
    ]
  },
  {
    title: 'Cryptographic ActionDigest & Tool Receipt Audit',
    objective: 'Audit Landlock syscall bounds and SHA-256 tool receipt verification before write-level actions are permitted.',
    shipId: 'ship-dev',
    crewIds: ['crew-repo-analyst', 'crew-eng-planner'],
    priority: 'high',
    budgetUSD: 2.20,
    deliverables: ['Security Boundary Verification Brief', 'ActionDigest Schema v2'],
    steps: [
      { stepNumber: 1, title: 'Map tool dispatch pathways across Tauri host & Go engine', assignedCrewId: 'crew-eng-planner', status: 'pending', outputArtifactType: 'architecture-diagram' },
      { stepNumber: 2, title: 'Verify deterministic hashing on tool parameters and credentials', assignedCrewId: 'crew-repo-analyst', status: 'pending', outputArtifactType: 'hash-verification' },
      { stepNumber: 3, title: 'Synthesize sandboxing compliance report for Captain review', assignedCrewId: 'crew-eng-planner', status: 'pending', outputArtifactType: 'audit-report' }
    ]
  },
  {
    title: 'BYOK Multi-Provider Cost & Latency Benchmark',
    objective: 'Benchmark token usage and response latency across Ollama local models vs Gemini 2.5 and Claude 3.7 for everyday repo scanning.',
    shipId: 'ship-research',
    crewIds: ['crew-researcher', 'crew-evaluator'],
    priority: 'medium',
    budgetUSD: 1.40,
    deliverables: ['Model Routing Cost Matrix', 'Recommended Provider Allocation'],
    steps: [
      { stepNumber: 1, title: 'Execute uniform 500-line AST parsing query across 4 models', assignedCrewId: 'crew-researcher', status: 'pending', outputArtifactType: 'benchmark-raw' },
      { stepNumber: 2, title: 'Calculate cost per 1M tokens vs response time', assignedCrewId: 'crew-evaluator', status: 'pending', outputArtifactType: 'cost-ledger' },
      { stepNumber: 3, title: 'Formulate automatic model routing policy for Harbor', assignedCrewId: 'crew-researcher', status: 'pending', outputArtifactType: 'routing-policy' }
    ]
  },
  {
    title: 'Developer Quickstart & CLI Onboarding Guide',
    objective: 'Design high-converting setup documentation and interactive terminal tour for new Fleet engineers.',
    shipId: 'ship-market',
    crewIds: ['crew-market-analyst', 'crew-brand-reviewer'],
    priority: 'medium',
    budgetUSD: 1.20,
    deliverables: ['Interactive CLI Guide', '5-Minute Quickstart SOP'],
    steps: [
      { stepNumber: 1, title: 'Review common setup friction points in issue tracker', assignedCrewId: 'crew-market-analyst', status: 'pending', outputArtifactType: 'friction-analysis' },
      { stepNumber: 2, title: 'Write conversational step-by-step terminal walkthrough', assignedCrewId: 'crew-brand-reviewer', status: 'pending', outputArtifactType: 'guide-markdown' }
    ]
  }
];

export const QuestBrainstormModal: React.FC<QuestBrainstormModalProps> = ({
  isOpen,
  onClose,
  onSendToChat
}) => {
  const {
    ships,
    crew,
    selectedWorkspace,
    selectedProject,
    createQuest,
    addNotification,
    setActiveTab,
    setSelectedQuestId
  } = useFleetStore();

  const [title, setTitle] = useState('New Strategic Quest');
  const [objective, setObjective] = useState('');
  const [selectedShipId, setSelectedShipId] = useState(ships[0]?.id || 'ship-dev');
  const [priority, setPriority] = useState<'low' | 'medium' | 'high' | 'urgent'>('high');
  const [budgetUSD, setBudgetUSD] = useState<number>(2.00);
  const [steps, setSteps] = useState<MapStep[]>([
    { stepNumber: 1, title: 'Explore & diagnose code/system bounds', status: 'pending' },
    { stepNumber: 2, title: 'Formulate execution plan & generate reviewable artifact', status: 'pending' },
    { stepNumber: 3, title: 'Submit for Captain’s Approval if write action required', status: 'pending' }
  ]);
  const [deliverables, setDeliverables] = useState<string[]>(['Technical Brief', 'Implementation Plan']);
  const [newStepText, setNewStepText] = useState('');
  const [isGenerating, setIsGenerating] = useState(false);
  const [feedbackMsg, setFeedbackMsg] = useState<string | null>(null);

  if (!isOpen) return null;

  const currentShip = ships.find((s) => s.id === selectedShipId) || ships[0];
  const shipCrew = crew.filter((c) => c.shipId === selectedShipId);

  const applyBrainstormIdea = (idea: BrainstormIdea) => {
    setTitle(idea.title);
    setObjective(idea.objective);
    setSelectedShipId(idea.shipId);
    setPriority(idea.priority);
    setBudgetUSD(idea.budgetUSD);
    setSteps(idea.steps);
    setDeliverables(idea.deliverables);
  };

  const handleAiBrainstorm = () => {
    if (!objective.trim() && !title.trim()) {
      setFeedbackMsg('Please enter a goal or topic to brainstorm.');
      return;
    }

    setIsGenerating(true);
    setFeedbackMsg('Quartermaster is synthesizing workflow milestones...');

    setTimeout(() => {
      setIsGenerating(false);
      setFeedbackMsg(null);

      // Enhance title and steps dynamically based on keywords
      const objLower = (objective + ' ' + title).toLowerCase();
      let generatedSteps: MapStep[] = [];
      let generatedDeliverables: string[] = [];

      if (objLower.includes('test') || objLower.includes('ci') || objLower.includes('bug')) {
        generatedSteps = [
          { stepNumber: 1, title: 'AST Static Analysis & Failure Trace Extraction', status: 'pending', outputArtifactType: 'trace-analysis' },
          { stepNumber: 2, title: 'Containerized Sandbox Reproduction & Test Isolation', status: 'pending', outputArtifactType: 'reproduction-log' },
          { stepNumber: 3, title: 'Produce Verified Bug Fix Patch & Regression Check', status: 'pending', outputArtifactType: 'diff-patch' },
          { stepNumber: 4, title: 'Package Artifact for Captain’s Approval Gate', status: 'pending', outputArtifactType: 'approval-request' }
        ];
        generatedDeliverables = ['CI Triage Report', 'Remediation Pull Request'];
        setSelectedShipId('ship-dev');
      } else if (objLower.includes('market') || objLower.includes('copy') || objLower.includes('launch') || objLower.includes('user')) {
        generatedSteps = [
          { stepNumber: 1, title: 'Analyze Target Audience Signals & Competitive Positioning', status: 'pending', outputArtifactType: 'market-intel' },
          { stepNumber: 2, title: 'Draft High-Conversion Narrative Copy & Benefit Matrix', status: 'pending', outputArtifactType: 'draft-copy' },
          { stepNumber: 3, title: 'Brand Alignment Review with Strict Human Verification Gate', status: 'pending', outputArtifactType: 'editorial-review' }
        ];
        generatedDeliverables = ['Positioning Brief', 'Launch Messaging Guide'];
        setSelectedShipId('ship-market');
      } else {
        generatedSteps = [
          { stepNumber: 1, title: 'Discover & Map Core Architecture Dependencies', status: 'pending', outputArtifactType: 'dependency-map' },
          { stepNumber: 2, title: 'Synthesize Trade-Off Matrix & Feasibility Model', status: 'pending', outputArtifactType: 'tradeoff-matrix' },
          { stepNumber: 3, title: 'Draft Actionable Roadmap with Resource Budgeting', status: 'pending', outputArtifactType: 'roadmap-brief' },
          { stepNumber: 4, title: 'Compile Final Executive Deliverable for Review', status: 'pending', outputArtifactType: 'decision-brief' }
        ];
        generatedDeliverables = ['Architecture Decision Record', 'Feasibility Brief'];
      }

      setSteps(generatedSteps);
      setDeliverables(generatedDeliverables);
    }, 700);
  };

  const addCustomStep = () => {
    if (!newStepText.trim()) return;
    setSteps([
      ...steps,
      {
        stepNumber: steps.length + 1,
        title: newStepText.trim(),
        status: 'pending'
      }
    ]);
    setNewStepText('');
  };

  const removeStep = (index: number) => {
    const updated = steps.filter((_, idx) => idx !== index).map((s, idx) => ({
      ...s,
      stepNumber: idx + 1
    }));
    setSteps(updated);
  };

  const handleLaunchQuest = () => {
    const questTitle = title.trim() || 'Untitled Quest';
    const questObjective = objective.trim() || 'Execute bounded strategic objective.';

    createQuest({
      title: questTitle,
      objective: questObjective,
      workspaceId: selectedWorkspace,
      projectId: selectedProject,
      priority,
      status: 'underway',
      suggestedShipId: selectedShipId,
      assignedShipId: selectedShipId,
      budgetLimitUSD: budgetUSD,
      estimatedCostUSD: 0,
      mapSteps: steps,
      requiredArtifacts: deliverables,
      activeVoyageProgress: 10
    });

    addNotification({
      title: `Quest Launched: ${questTitle}`,
      description: `Dispatched to ${currentShip.name} under ${priority.toUpperCase()} priority.`,
      type: 'quest',
      actionLinkTab: 'mission-board'
    });

    onClose();
  };

  const handleDiscussInChat = () => {
    const brief = `I just brainstormed a Quest with the following specifications:
- **Title**: ${title || 'New Quest'}
- **Ship**: ${currentShip.name}
- **Priority**: ${priority.toUpperCase()} | Budget: $${budgetUSD.toFixed(2)}
- **Objective**: ${objective || 'Explore high-leverage objective'}
- **Key Milestones**:
${steps.map((s) => `  ${s.stepNumber}. ${s.title}`).join('\n')}
- **Expected Artifacts**: ${deliverables.join(', ')}

Quartermaster, please review this draft. Do you see any risks or opportunities to tighten the scope before we dispatch the voyage?`;

    if (onSendToChat) {
      onSendToChat(brief);
    }
    onClose();
  };

  return (
    <div
      onClick={onClose}
      className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-150 cursor-pointer"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="bg-white dark:bg-[#16181b] border border-neutral-200 dark:border-neutral-800 rounded-2xl w-full max-w-3xl shadow-2xl flex flex-col max-h-[92vh] overflow-hidden cursor-default"
      >
        {/* Modal Header */}
        <div className="p-4 sm:p-5 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between bg-neutral-50/60 dark:bg-neutral-900/40">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-teal-500/10 text-teal-600 dark:text-teal-400 flex items-center justify-center border border-teal-500/20">
              <Lightbulb className="w-5 h-5" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h3 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                  Quest Brainstorming Studio
                </h3>
                <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-teal-500/15 text-teal-600 dark:text-teal-400 border border-teal-500/30">
                  Interactive Strategy
                </span>
              </div>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                Formulate clear objectives, design SOP milestones, and assign specialist Crew before dispatch.
              </p>
            </div>
          </div>
        </div>

        {/* Modal Body (Scrollable) */}
        <div className="flex-1 overflow-y-auto p-4 sm:p-6 space-y-6">
          {/* 1. Quick Inspiration Seeds */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 flex items-center gap-1.5 uppercase tracking-wider font-mono">
                <Flame className="w-3.5 h-3.5 text-amber-500" />
                Strategic Inspiration Seeds
              </span>
              <span className="text-[11px] text-neutral-400">Click to pre-fill</span>
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
              {BRAINSTORM_IDEAS.map((idea) => (
                <button
                  key={idea.title}
                  onClick={() => applyBrainstormIdea(idea)}
                  className="text-left p-2.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/30 hover:border-teal-500/50 hover:bg-teal-50/30 dark:hover:bg-teal-950/20 transition-all cursor-pointer group"
                >
                  <div className="text-xs font-semibold text-neutral-800 dark:text-neutral-200 group-hover:text-teal-600 dark:group-hover:text-teal-400 truncate">
                    {idea.title}
                  </div>
                  <div className="text-[11px] text-neutral-500 dark:text-neutral-400 line-clamp-1 mt-0.5">
                    {idea.objective}
                  </div>
                  <div className="flex items-center gap-2 mt-1.5 text-[10px] font-mono text-neutral-400">
                    <span>${idea.budgetUSD.toFixed(2)} cap</span>
                    <span>&middot;</span>
                    <span className="uppercase text-teal-600 dark:text-teal-400">{idea.priority}</span>
                  </div>
                </button>
              ))}
            </div>
          </div>

          {/* 2. Quest Title & Objective */}
          <div className="space-y-3 p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/30 dark:bg-neutral-900/20">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Quest Title
              </label>
              <input
                type="text"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="e.g. CI Socket Teardown Race Condition Fix"
                className="w-full text-xs px-3 py-2 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-[#181a1d] text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-teal-500 font-medium"
              />
            </div>

            <div>
              <div className="flex items-center justify-between mb-1">
                <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300">
                  Strategic Objective & Scope
                </label>
                <button
                  type="button"
                  onClick={handleAiBrainstorm}
                  disabled={isGenerating}
                  className="text-xs text-teal-600 dark:text-teal-400 hover:underline flex items-center gap-1 font-medium cursor-pointer"
                >
                  <Wand2 className="w-3.5 h-3.5 text-teal-500" />
                  <span className="hidden sm:inline">{isGenerating ? 'Synthesizing...' : 'Synthesize SOP with AI'}</span>
                  <span className="sm:hidden">{isGenerating ? 'Synthesizing...' : 'AI SOP'}</span>
                </button>
              </div>
              <textarea
                rows={3}
                value={objective}
                onChange={(e) => setObjective(e.target.value)}
                placeholder="What high-leverage outcome do you want to accomplish? What constraints or acceptance criteria must be respected?"
                className="w-full text-xs px-3 py-2 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-[#181a1d] text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-teal-500"
              />
              {feedbackMsg && (
                <div className="text-[11px] text-teal-600 dark:text-teal-400 font-mono mt-1 flex items-center gap-1.5">
                  <span className="w-1.5 h-1.5 rounded-full bg-teal-500 animate-pulse" />
                  {feedbackMsg}
                </div>
              )}
            </div>
          </div>

          {/* 3. Ship Container & Specialist Team */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1.5 flex items-center gap-1.5">
                <ShipIcon className="w-3.5 h-3.5 text-teal-500" />
                Assigned Ship Container
              </label>
              <select
                value={selectedShipId}
                onChange={(e) => setSelectedShipId(e.target.value)}
                className="w-full text-xs px-3 py-2 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-[#181a1d] text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-teal-500 font-medium cursor-pointer"
              >
                {ships.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} ({s.homeScope})
                  </option>
                ))}
              </select>
              <div className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-1 line-clamp-1">
                {currentShip.tagline}
              </div>
            </div>

            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1.5 flex items-center gap-1.5">
                <Users className="w-3.5 h-3.5 text-teal-500" />
                Crew Specialists Available ({shipCrew.length})
              </label>
              <div className="flex flex-wrap gap-1.5">
                {shipCrew.map((c) => (
                  <span
                    key={c.id}
                    className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-[11px] bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 font-medium border border-neutral-200 dark:border-neutral-700/60"
                  >
                    <span className="w-1.5 h-1.5 rounded-full bg-teal-500" />
                    {c.name}
                  </span>
                ))}
              </div>
            </div>
          </div>

          {/* 4. Milestone Roadmap (MapSteps) */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 flex items-center gap-1.5 uppercase tracking-wider font-mono">
                <Map className="w-3.5 h-3.5 text-teal-500" />
                Milestone Workflow Map ({steps.length} Steps)
              </span>
              <span className="text-[11px] text-neutral-400">Sequential SOP execution</span>
            </div>

            <div className="space-y-2">
              {steps.map((step, idx) => (
                <div
                  key={idx}
                  className="flex items-center justify-between p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1d] text-xs gap-2"
                >
                  <div className="flex items-center gap-2.5 min-w-0">
                    <span className="w-5 h-5 rounded-full bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 font-mono text-[10px] flex items-center justify-center shrink-0 font-bold">
                      {step.stepNumber}
                    </span>
                    <span className="font-medium text-neutral-800 dark:text-neutral-200 truncate">
                      {step.title}
                    </span>
                  </div>
                  {step.outputArtifactType && (
                    <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400 shrink-0 hidden sm:inline">
                      artifact: {step.outputArtifactType}
                    </span>
                  )}
                  <button
                    onClick={() => removeStep(idx)}
                    className="p-1 text-neutral-400 hover:text-red-500 transition-colors shrink-0 cursor-pointer"
                    title="Remove step"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
              ))}

              {/* Add Custom Step input */}
              <div className="flex items-center gap-2 mt-2">
                <input
                  type="text"
                  value={newStepText}
                  onChange={(e) => setNewStepText(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      addCustomStep();
                    }
                  }}
                  placeholder="Add custom milestone step..."
                  className="flex-1 text-xs px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181a1d] text-neutral-800 dark:text-neutral-200 focus:outline-none focus:ring-1 focus:ring-teal-500"
                />
                <button
                  onClick={addCustomStep}
                  className="px-3 py-1.5 rounded-lg bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 text-xs font-semibold text-neutral-700 dark:text-neutral-200 flex items-center gap-1 cursor-pointer"
                >
                  <Plus className="w-3.5 h-3.5" />
                  <span>Add Step</span>
                </button>
              </div>
            </div>
          </div>

          {/* 5. Budget, Priority, and Safeguards */}
          <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/30 grid grid-cols-1 sm:grid-cols-3 gap-3">
            <div>
              <label className="block text-[11px] font-semibold text-neutral-600 dark:text-neutral-400 mb-1">
                Priority
              </label>
              <select
                value={priority}
                onChange={(e) => setPriority(e.target.value as any)}
                className="w-full text-xs px-2.5 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-[#181a1d] text-neutral-900 dark:text-neutral-100 uppercase font-mono font-medium cursor-pointer"
              >
                <option value="low">Low</option>
                <option value="medium">Medium</option>
                <option value="high">High</option>
                <option value="urgent">Urgent</option>
              </select>
            </div>

            <div>
              <label className="block text-[11px] font-semibold text-neutral-600 dark:text-neutral-400 mb-1 flex items-center gap-1">
                <Coins className="w-3 h-3 text-amber-500" />
                Voyage Budget Cap
              </label>
              <div className="flex items-center gap-1.5">
                <span className="text-xs font-mono text-neutral-500">$</span>
                <input
                  type="number"
                  step="0.10"
                  min="0.20"
                  max="25.00"
                  value={budgetUSD}
                  onChange={(e) => setBudgetUSD(parseFloat(e.target.value) || 1.0)}
                  className="w-full text-xs px-2 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-[#181a1d] text-neutral-900 dark:text-neutral-100 font-mono"
                />
              </div>
            </div>

            <div>
              <label className="block text-[11px] font-semibold text-neutral-600 dark:text-neutral-400 mb-1 flex items-center gap-1">
                <Shield className="w-3 h-3 text-teal-500" />
                Execution Policy
              </label>
              <div className="text-xs font-medium text-neutral-800 dark:text-neutral-200 mt-1">
                Read-first with Gated Write
              </div>
              <div className="text-[10px] text-neutral-400 font-mono mt-0.5">
                Captain approval required
              </div>
            </div>
          </div>
        </div>

        {/* Modal Footer Actions */}
        <div className="p-4 sm:p-5 border-t border-neutral-200 dark:border-neutral-800 bg-neutral-50/60 dark:bg-neutral-900/40 flex flex-col sm:flex-row items-center justify-between gap-3">
          <div className="text-xs text-neutral-500 dark:text-neutral-400 text-center sm:text-left">
            <span>Project: </span>
            <span className="font-semibold text-neutral-800 dark:text-neutral-200">{selectedProject}</span>
          </div>

          <div className="flex items-center gap-2.5 w-full sm:w-auto justify-end">
            <button
              onClick={handleDiscussInChat}
              className="flex-1 sm:flex-none flex items-center justify-center gap-1.5 px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-[#181a1d] text-neutral-700 dark:text-neutral-200 text-xs font-semibold hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-all cursor-pointer"
              title="Discuss this brainstormed draft in Quarterdeck chat"
            >
              <MessageSquare className="w-3.5 h-3.5 text-teal-500" />
              <span className="hidden sm:inline">Discuss in Chat</span>
              <span className="sm:hidden">Discuss</span>
            </button>

            <button
              onClick={handleLaunchQuest}
              className="flex-1 sm:flex-none flex items-center justify-center gap-1.5 px-4 py-2 rounded-xl bg-teal-600 dark:bg-teal-500 hover:bg-teal-500 dark:hover:bg-teal-400 text-white dark:text-neutral-950 text-xs font-bold transition-all shadow-md active:scale-[0.98] cursor-pointer"
            >
              <Sparkles className="w-3.5 h-3.5" />
              <span className="hidden sm:inline">Launch Quest to Fleet</span>
              <span className="sm:hidden">Launch</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

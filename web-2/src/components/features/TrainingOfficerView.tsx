import React, { useState } from 'react';
import {
  GraduationCap,
  Sparkles,
  Plus,
  Search,
  Filter,
  Eye,
  Edit2,
  Trash2,
  Copy,
  Pause,
  Play,
  RotateCcw,
  CheckCircle2,
  AlertTriangle,
  Layers,
  ChevronDown,
  Terminal,
  Anchor,
  Compass,
  ArrowRight,
  ShieldAlert,
  Sliders,
  Check,
  X,
  Workflow
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import {
  TrainingSkill,
  GlobalSteering,
  SteeringDirective,
  TrainingHook,
  TrainingConfigStatus
} from '../../types';
import { PageHeaderNav } from '../common/PageHeaderNav';
import { Button } from '../common/Button';
import { Modal } from '../common/Modal';
import { Dropdown, SelectDropdown } from '../common/Dropdown';
import { ItemCard } from '../common/ItemCard';
import { CardPopover } from '../common/CardPopover';

export const TrainingOfficerView: React.FC = () => {
  const {
    trainingSkills,
    globalSteering,
    steeringDirectives,
    trainingHooks,
    addTrainingSkill,
    updateTrainingSkill,
    deleteTrainingSkill,
    addGlobalSteering,
    updateGlobalSteering,
    deleteGlobalSteering,
    addSteeringDirective,
    updateSteeringDirective,
    deleteSteeringDirective,
    addTrainingHook,
    updateTrainingHook,
    deleteTrainingHook
  } = useFleetStore();

  const [activeTab, setActiveTab] = useState<'skills' | 'global' | 'steering' | 'hooks'>('skills');
  const [searchQuery, setSearchQuery] = useState('');
  const [statusFilter, setStatusFilter] = useState<'all' | 'active' | 'draft' | 'disabled'>('all');

  // Mentor AI Modal State
  const [isMentorOpen, setIsMentorOpen] = useState(false);
  const [mentorContext, setMentorContext] = useState<'skills' | 'global' | 'steering' | 'hooks'>('skills');
  const [mentorPrompt, setMentorPrompt] = useState('');
  const [isGeneratingDraft, setIsGeneratingDraft] = useState(false);
  const [generatedDraft, setGeneratedDraft] = useState<any | null>(null);

  // Forms and Modals
  const [modalType, setModalType] = useState<'skill' | 'global' | 'steering' | 'hook' | null>(null);
  const [editingItem, setEditingItem] = useState<any | null>(null);
  const [viewingDetailItem, setViewingDetailItem] = useState<{ type: string; data: any } | null>(null);
  const [deleteConfirmation, setDeleteConfirmation] = useState<{ type: string; item: any } | null>(null);
  const [deleteInputText, setDeleteInputText] = useState('');
  const [toastMessage, setToastMessage] = useState<string | null>(null);

  // Form states for Skill
  const [skillForm, setSkillForm] = useState({
    name: '',
    purpose: '',
    instructions: '',
    inputSchemaText: '',
    outputFormat: 'Structured JSON',
    accessScopeText: 'fleet-wide',
    status: 'active' as TrainingConfigStatus
  });

  // Form states for Global Steering (Orders)
  const [globalForm, setGlobalForm] = useState({
    name: '',
    directive: '',
    priority: 'standard' as 'critical' | 'high' | 'standard',
    enforcement: 'required' as 'required' | 'advisory',
    appliesToText: 'engineering, release_readiness, fleet-wide',
    conflictHandling: 'Takes precedence over task velocity.',
    status: 'active' as TrainingConfigStatus
  });

  // Form states for Steering (Courses)
  const [steeringForm, setSteeringForm] = useState({
    name: '',
    targetType: 'role' as 'skill' | 'workflow' | 'workspace' | 'role',
    targetId: 'brand_reviewer',
    guidance: '',
    priority: 1,
    overridePolicy: 'inherit' as 'inherit' | 'override' | 'append',
    status: 'active' as TrainingConfigStatus
  });

  // Form states for Hook
  const [hookForm, setHookForm] = useState({
    name: '',
    triggerEvent: 'skill_created',
    actionType: 'validate_and_audit',
    conditionsText: '{"autoValidate": true}',
    actionConfigText: '{"logAudit": true}',
    executionMode: 'automatic' as 'automatic' | 'approval_required' | 'simulation',
    failureHandling: 'notify' as 'retry' | 'notify' | 'queue_for_review' | 'stop',
    status: 'active' as TrainingConfigStatus
  });

  const showToast = (msg: string) => {
    setToastMessage(msg);
    setTimeout(() => setToastMessage(null), 3000);
  };

  const handleOpenMentor = (tabContext?: 'skills' | 'global' | 'steering' | 'hooks') => {
    setMentorContext(tabContext || activeTab);
    setMentorPrompt('');
    setGeneratedDraft(null);
    setIsMentorOpen(true);
  };

  const handleGenerateMentorDraft = () => {
    if (!mentorPrompt.trim()) return;
    setIsGeneratingDraft(true);

    setTimeout(() => {
      setIsGeneratingDraft(false);
      if (mentorContext === 'skills') {
        setGeneratedDraft({
          type: 'skill',
          title: 'Code Security & Dependency Sentry',
          summary: 'Automated static analysis and vulnerability gate for incoming pull requests.',
          formValues: {
            name: 'Security & Dependency Sentry',
            purpose: 'Scans new dependencies and parses lockfiles for known CVE advisories before deployment.',
            instructions: 'Inspect lockfiles, verify SHA-256 digests against advisory feeds, and fail validation on high/critical vulnerabilities.',
            outputFormat: 'Security Audit Report Markdown',
            accessScopeText: 'engineering, release_readiness',
            status: 'draft'
          },
          assumptions: ['Assumes access to local or remote package advisory database.', 'Operates under read-only permissions.'],
          risks: 'May slow down high-velocity merges if strict blocking is enabled without an approval override.',
          validationChecklist: ['Verify schema compliance', 'Confirm permission boundaries', 'Test on sample repository pull request']
        });
      } else if (mentorContext === 'global') {
        setGeneratedDraft({
          type: 'global',
          title: 'Enforce Verified Source Citations',
          summary: 'Fleet-wide standing order to mandate commit and benchmark citations.',
          formValues: {
            name: 'Mandatory Factual Citations',
            directive: 'All operational claims and architectural conclusions must cite verifiable commit hashes, official documentation, or benchmark metrics.',
            priority: 'high',
            enforcement: 'required',
            appliesToText: 'engineering, release_readiness, fleet-wide',
            conflictHandling: 'Overrides unsupported assertions; ungrounded statements must be flagged as speculative.',
            status: 'draft'
          },
          assumptions: ['Applies across all specialist vessels and crews in the fleet.'],
          risks: 'Conversational brainstorming might require explicit labeling as speculative to avoid triggering rejections.',
          validationChecklist: ['Review fleet compliance score', 'Audit log impact check']
        });
      } else if (mentorContext === 'steering') {
        setGeneratedDraft({
          type: 'steering',
          title: 'Frontend Typography & Layout Steering',
          summary: 'Specific design discipline course correction for creative and UI roles.',
          formValues: {
            name: 'UI Typography & Zero-Pill Discipline',
            targetType: 'role',
            targetId: 'Brand & Tone Reviewer',
            guidance: 'Enforce clean typographic hierarchy, avoid excessive pill borders, and ensure contrast ratios adhere to modern dark UI standards.',
            priority: 1,
            overridePolicy: 'append',
            status: 'draft'
          },
          assumptions: ['Governs brand and UI reviewers.'],
          risks: 'None. Provides advisory aesthetic boundaries.',
          validationChecklist: ['Confirm target role exists', 'Verify override policy']
        });
      } else {
        setGeneratedDraft({
          type: 'hook',
          title: 'CI Teardown Failure Alert Hook',
          summary: 'Automated alert trigger when tests fail on repository health quests.',
          formValues: {
            name: 'CI Teardown Failure Monitor',
            triggerEvent: 'quest_test_failed',
            actionType: 'notify_and_draft_approval',
            conditionsText: '{"severity": "high", "consecutiveFailures": 2}',
            actionConfigText: '{"channel": "quarterdeck", "autoOpenGate": true}',
            executionMode: 'approval_required',
            failureHandling: 'notify',
            status: 'draft'
          },
          assumptions: ['Triggered by mission test runner telemetry.'],
          risks: 'May create duplicate notifications if flaky tests re-run automatically.',
          validationChecklist: ['Simulate sample trigger signal', 'Verify notification receiver']
        });
      }
    }, 800);
  };

  const handleApplyMentorDraft = () => {
    if (!generatedDraft) return;
    setIsMentorOpen(false);

    if (generatedDraft.type === 'skill') {
      setSkillForm({
        ...skillForm,
        ...generatedDraft.formValues
      });
      setEditingItem(null);
      setModalType('skill');
      setActiveTab('skills');
    } else if (generatedDraft.type === 'global') {
      setGlobalForm({
        ...globalForm,
        ...generatedDraft.formValues
      });
      setEditingItem(null);
      setModalType('global');
      setActiveTab('global');
    } else if (generatedDraft.type === 'steering') {
      setSteeringForm({
        ...steeringForm,
        ...generatedDraft.formValues
      });
      setEditingItem(null);
      setModalType('steering');
      setActiveTab('steering');
    } else if (generatedDraft.type === 'hook') {
      setHookForm({
        ...hookForm,
        ...generatedDraft.formValues
      });
      setEditingItem(null);
      setModalType('hook');
      setActiveTab('hooks');
    }
    showToast('Mentor draft applied to form. Review and submit to save.');
  };

  // Open Form Helpers
  const handleOpenAdd = (type: 'skill' | 'global' | 'steering' | 'hook') => {
    setEditingItem(null);
    if (type === 'skill') {
      setSkillForm({
        name: '',
        purpose: '',
        instructions: '',
        inputSchemaText: '',
        outputFormat: 'Structured JSON',
        accessScopeText: 'fleet-wide',
        status: 'active'
      });
    } else if (type === 'global') {
      setGlobalForm({
        name: '',
        directive: '',
        priority: 'standard',
        enforcement: 'required',
        appliesToText: 'engineering, release_readiness, fleet-wide',
        conflictHandling: 'Takes precedence over task velocity.',
        status: 'active'
      });
    } else if (type === 'steering') {
      setSteeringForm({
        name: '',
        targetType: 'role',
        targetId: 'brand_reviewer',
        guidance: '',
        priority: 1,
        overridePolicy: 'inherit',
        status: 'active'
      });
    } else if (type === 'hook') {
      setHookForm({
        name: '',
        triggerEvent: 'skill_created',
        actionType: 'validate_and_audit',
        conditionsText: '{"autoValidate": true}',
        actionConfigText: '{"logAudit": true}',
        executionMode: 'automatic',
        failureHandling: 'notify',
        status: 'active'
      });
    }
    setModalType(type);
  };

  const handleOpenEdit = (type: 'skill' | 'global' | 'steering' | 'hook', item: any) => {
    setEditingItem(item);
    if (type === 'skill') {
      setSkillForm({
        name: item.name,
        purpose: item.purpose,
        instructions: item.instructions,
        inputSchemaText: item.inputSchema ? JSON.stringify(item.inputSchema) : '',
        outputFormat: item.outputFormat || 'Structured JSON',
        accessScopeText: (item.accessScope || []).join(', '),
        status: item.status
      });
    } else if (type === 'global') {
      setGlobalForm({
        name: item.name,
        directive: item.directive,
        priority: item.priority,
        enforcement: item.enforcement,
        appliesToText: (item.appliesTo || []).join(', '),
        conflictHandling: item.conflictHandling || '',
        status: item.status
      });
    } else if (type === 'steering') {
      setSteeringForm({
        name: item.name,
        targetType: item.targetType,
        targetId: item.targetId,
        guidance: item.guidance,
        priority: item.priority,
        overridePolicy: item.overridePolicy,
        status: item.status
      });
    } else if (type === 'hook') {
      setHookForm({
        name: item.name,
        triggerEvent: item.triggerEvent,
        actionType: item.actionType,
        conditionsText: item.conditions ? JSON.stringify(item.conditions) : '',
        actionConfigText: item.actionConfig ? JSON.stringify(item.actionConfig) : '',
        executionMode: item.executionMode,
        failureHandling: item.failureHandling,
        status: item.status
      });
    }
    setModalType(type);
  };

  // Submit Handlers
  const handleSaveSkill = (asDraft = false) => {
    if (!skillForm.name.trim() || !skillForm.purpose.trim()) return;
    const scopes = skillForm.accessScopeText.split(',').map((s) => s.trim()).filter(Boolean);
    const statusVal = asDraft ? 'draft' : skillForm.status;

    if (editingItem) {
      updateTrainingSkill(editingItem.id, {
        name: skillForm.name,
        purpose: skillForm.purpose,
        instructions: skillForm.instructions,
        outputFormat: skillForm.outputFormat,
        accessScope: scopes,
        status: statusVal
      });
      showToast(`Updated Skill: ${skillForm.name}`);
    } else {
      addTrainingSkill({
        name: skillForm.name,
        purpose: skillForm.purpose,
        instructions: skillForm.instructions,
        outputFormat: skillForm.outputFormat,
        accessScope: scopes,
        status: statusVal,
        createdBy: 'Operator',
        updatedBy: 'Operator'
      });
      showToast(`Added Skill: ${skillForm.name}`);
    }
    setModalType(null);
  };

  const handleSaveGlobal = (asDraft = false) => {
    if (!globalForm.name.trim() || !globalForm.directive.trim()) return;
    const applies = globalForm.appliesToText.split(',').map((s) => s.trim()).filter(Boolean);
    const statusVal = asDraft ? 'draft' : globalForm.status;

    if (editingItem) {
      updateGlobalSteering(editingItem.id, {
        name: globalForm.name,
        directive: globalForm.directive,
        priority: globalForm.priority,
        enforcement: globalForm.enforcement,
        appliesTo: applies,
        conflictHandling: globalForm.conflictHandling,
        status: statusVal
      });
      showToast(`Updated Standing Order: ${globalForm.name}`);
    } else {
      addGlobalSteering({
        name: globalForm.name,
        directive: globalForm.directive,
        priority: globalForm.priority,
        enforcement: globalForm.enforcement,
        appliesTo: applies,
        conflictHandling: globalForm.conflictHandling,
        status: statusVal,
        createdBy: 'Admiralty Desk',
        updatedBy: 'Operator'
      });
      showToast(`Issued Standing Order: ${globalForm.name}`);
    }
    setModalType(null);
  };

  const handleSaveSteering = (asDraft = false) => {
    if (!steeringForm.name.trim() || !steeringForm.guidance.trim()) return;
    const statusVal = asDraft ? 'draft' : steeringForm.status;

    if (editingItem) {
      updateSteeringDirective(editingItem.id, {
        name: steeringForm.name,
        targetType: steeringForm.targetType,
        targetId: steeringForm.targetId,
        guidance: steeringForm.guidance,
        priority: steeringForm.priority,
        overridePolicy: steeringForm.overridePolicy,
        status: statusVal
      });
      showToast(`Updated Steering Course: ${steeringForm.name}`);
    } else {
      addSteeringDirective({
        name: steeringForm.name,
        targetType: steeringForm.targetType,
        targetId: steeringForm.targetId,
        guidance: steeringForm.guidance,
        priority: steeringForm.priority,
        overridePolicy: steeringForm.overridePolicy,
        status: statusVal,
        createdBy: 'Operator',
        updatedBy: 'Operator'
      });
      showToast(`Set Course: ${steeringForm.name}`);
    }
    setModalType(null);
  };

  const handleSaveHook = (asDraft = false) => {
    if (!hookForm.name.trim() || !hookForm.triggerEvent.trim()) return;
    const statusVal = asDraft ? 'draft' : hookForm.status;

    let parsedConditions = {};
    try {
      if (hookForm.conditionsText.trim()) parsedConditions = JSON.parse(hookForm.conditionsText);
    } catch {
      // fallback
    }

    let parsedConfig = {};
    try {
      if (hookForm.actionConfigText.trim()) parsedConfig = JSON.parse(hookForm.actionConfigText);
    } catch {
      // fallback
    }

    if (editingItem) {
      updateTrainingHook(editingItem.id, {
        name: hookForm.name,
        triggerEvent: hookForm.triggerEvent,
        actionType: hookForm.actionType,
        conditions: parsedConditions,
        actionConfig: parsedConfig,
        executionMode: hookForm.executionMode,
        failureHandling: hookForm.failureHandling,
        status: statusVal
      });
      showToast(`Updated Hook: ${hookForm.name}`);
    } else {
      addTrainingHook({
        name: hookForm.name,
        triggerEvent: hookForm.triggerEvent,
        actionType: hookForm.actionType,
        conditions: parsedConditions,
        actionConfig: parsedConfig,
        executionMode: hookForm.executionMode,
        failureHandling: hookForm.failureHandling,
        status: statusVal,
        createdBy: 'System Architect',
        updatedBy: 'Operator'
      });
      showToast(`Rigged Hook: ${hookForm.name}`);
    }
    setModalType(null);
  };

  const handleDeleteConfirmed = () => {
    if (!deleteConfirmation) return;
    const { type, item } = deleteConfirmation;
    if (type === 'skill') deleteTrainingSkill(item.id);
    else if (type === 'global') deleteGlobalSteering(item.id);
    else if (type === 'steering') deleteSteeringDirective(item.id);
    else if (type === 'hook') deleteTrainingHook(item.id);

    showToast(`Removed directive: ${item.name}`);
    setDeleteConfirmation(null);
    setDeleteInputText('');
  };

  // Filtered Lists
  const filteredSkills = trainingSkills.filter((s) => {
    const matchesSearch = s.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      s.purpose.toLowerCase().includes(searchQuery.toLowerCase());
    const matchesStatus = statusFilter === 'all' || s.status === statusFilter;
    return matchesSearch && matchesStatus;
  });

  const filteredGlobal = globalSteering.filter((g) => {
    const matchesSearch = g.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      g.directive.toLowerCase().includes(searchQuery.toLowerCase());
    const matchesStatus = statusFilter === 'all' || g.status === statusFilter;
    return matchesSearch && matchesStatus;
  });

  const filteredSteering = steeringDirectives.filter((st) => {
    const matchesSearch = st.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      st.guidance.toLowerCase().includes(searchQuery.toLowerCase()) ||
      st.targetId.toLowerCase().includes(searchQuery.toLowerCase());
    const matchesStatus = statusFilter === 'all' || st.status === statusFilter;
    return matchesSearch && matchesStatus;
  });

  const filteredHooks = trainingHooks.filter((h) => {
    const matchesSearch = h.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      h.triggerEvent.toLowerCase().includes(searchQuery.toLowerCase()) ||
      h.actionType.toLowerCase().includes(searchQuery.toLowerCase());
    const matchesStatus = statusFilter === 'all' || h.status === statusFilter;
    return matchesSearch && matchesStatus;
  });

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto px-4 sm:px-6 pt-0 pb-6 space-y-4 max-w-6xl mx-auto w-full animate-view-fade-in scrollbar-none">
      {/* Toast Alert */}
      {toastMessage && (
        <div className="fixed top-4 right-4 z-50 px-4 py-2 rounded-xl bg-teal-600 text-white text-xs font-semibold shadow-lg flex items-center gap-2 animate-in fade-in duration-200">
          <CheckCircle2 className="w-4 h-4" />
          <span>{toastMessage}</span>
        </div>
      )}

      {/* Header with Title, Mentor AI Action, and Add Directive Menu */}
      <PageHeaderNav
        icon={<GraduationCap className="w-4 h-4 text-teal-500 shrink-0" />}
        title="Training Officer"
        badge={
          <span className="hidden sm:inline-flex text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
            Intelligence Operator
          </span>
        }
        description="Train, steer, and automate the fleet’s intelligence."
        search={{
          value: searchQuery,
          onChange: setSearchQuery,
          placeholder: 'Search directives, rules, hooks...'
        }}
        actions={
          <div className="flex items-center gap-2">
            {/* Primary AI Action: Mentor */}
            <Button
              variant="outline"
              size="sm"
              icon={<Sparkles className="w-3.5 h-3.5 text-teal-500" />}
              onClick={() => handleOpenMentor()}
              title="Consult Mentor AI"
            >
              Mentor
            </Button>

            {/* Primary Creation Action: Add Directive Dropdown */}
            <Dropdown
              title="Add Directive"
              align="right"
              menuWidth="w-56"
              items={[
                { id: 'skill', label: 'Add Skill', description: 'Define reusable AI capability' },
                { id: 'global', label: 'Issue Order', description: 'Fleet-wide baseline standing order' },
                { id: 'steering', label: 'Set Course', description: 'Scoped steering for role or workflow' },
                { id: 'hook', label: 'Rig Hook', description: 'Event-driven signal automation' }
              ]}
              onSelect={(id) => handleOpenAdd(id as any)}
              trigger={
                <Button
                  variant="primary"
                  size="sm"
                  icon={<Plus className="w-3.5 h-3.5" />}
                  shortLabel="Add"
                  title="Add New Directive"
                >
                  Add Directive
                </Button>
              }
            />
          </div>
        }
        chips={{
          items: [
            { id: 'skills', label: 'Skills', count: trainingSkills.length },
            { id: 'global', label: 'Orders', count: globalSteering.length },
            { id: 'steering', label: 'Courses', count: steeringDirectives.length },
            { id: 'hooks', label: 'Hooks', count: trainingHooks.length }
          ],
          selectedId: activeTab,
          onSelect: (id) => setActiveTab(id as any),
          variant: 'pills'
        }}
      />

      {/* Secondary Status Filter & Counter Toolbar */}
      <div className="flex items-center justify-between gap-3 text-xs text-neutral-500 px-1">
        <div className="flex items-center gap-2">
          <span>Filter:</span>
          {(['all', 'active', 'draft', 'disabled'] as const).map((st) => (
            <button
              key={st}
              type="button"
              onClick={() => setStatusFilter(st)}
              className={`px-2 py-0.5 rounded-lg text-xs capitalize cursor-pointer transition-colors ${statusFilter === st
                ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-semibold'
                : 'hover:text-neutral-800 dark:hover:text-neutral-200'
                }`}
            >
              {st}
            </button>
          ))}
        </div>
        <span className="font-mono text-[11px]">
          {activeTab === 'skills' && `${filteredSkills.length} Skills`}
          {activeTab === 'global' && `${filteredGlobal.length} Standing Orders`}
          {activeTab === 'steering' && `${filteredSteering.length} Active Courses`}
          {activeTab === 'hooks' && `${filteredHooks.length} Event Hooks`}
        </span>
      </div>

      {/* TAB 1: SKILLS LIST */}
      {activeTab === 'skills' && (
        <div className="space-y-3">
          {filteredSkills.length === 0 ? (
            <div className="p-8 text-center rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-800 space-y-3 bg-white/50 dark:bg-[#15171a]/50">
              <Compass className="w-8 h-8 mx-auto text-neutral-400" />
              <p className="text-xs text-neutral-500 max-w-sm mx-auto">
                No skills aboard yet. Add a capability to expand the fleet’s reach.
              </p>
              <Button variant="primary" size="sm" onClick={() => handleOpenAdd('skill')} icon={<Plus className="w-3.5 h-3.5" />}>
                Add Skill
              </Button>
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
              {filteredSkills.map((skill) => (
                <ItemCard
                  key={skill.id}
                  title={skill.name}
                  subtitle={skill.purpose}
                  badge={
                    <span
                      className={`text-[10px] font-mono px-2 py-0.5 rounded-full font-bold uppercase ${skill.status === 'active'
                        ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20'
                        : skill.status === 'draft'
                          ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20'
                          : 'bg-neutral-500/10 text-neutral-500 border border-neutral-500/20'
                        }`}
                    >
                      {skill.status}
                    </span>
                  }
                  meta={
                    <div className="flex items-center gap-2 text-[11px] text-neutral-400 font-mono">
                      <span>Scope: {skill.accessScope?.join(', ') || 'fleet-wide'}</span>
                      <span>·</span>
                      <span>v{skill.version}</span>
                    </div>
                  }
                  actions={
                    <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={<Edit2 className="w-3.5 h-3.5" />}
                        onClick={() => handleOpenEdit('skill', skill)}
                        title="Edit Skill"
                      />
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={<Trash2 className="w-3.5 h-3.5 text-rose-500" />}
                        onClick={() => setDeleteConfirmation({ type: 'skill', item: skill })}
                        title="Delete Skill"
                      />
                    </div>
                  }
                  onClick={() => setViewingDetailItem({ type: 'skill', data: skill })}
                >
                  <div className="p-3 bg-neutral-50 dark:bg-neutral-900/60 rounded-xl text-xs text-neutral-700 dark:text-neutral-300 font-mono line-clamp-2">
                    {skill.instructions}
                  </div>
                </ItemCard>
              ))}
            </div>
          )}
        </div>
      )}

      {/* TAB 2: STANDING ORDERS */}
      {activeTab === 'global' && (
        <div className="space-y-3">
          {filteredGlobal.length === 0 ? (
            <div className="p-8 text-center rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-800 space-y-3 bg-white/50 dark:bg-[#15171a]/50">
              <ShieldAlert className="w-8 h-8 mx-auto text-neutral-400" />
              <p className="text-xs text-neutral-500 max-w-sm mx-auto">
                No standing orders have been issued. Establish baseline guidance for every vessel.
              </p>
              <Button variant="primary" size="sm" onClick={() => handleOpenAdd('global')} icon={<Plus className="w-3.5 h-3.5" />}>
                Issue Order
              </Button>
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
              {filteredGlobal.map((order) => (
                <ItemCard
                  key={order.id}
                  title={order.name}
                  subtitle={order.directive}
                  badge={
                    <div className="flex items-center gap-1.5">
                      <span
                        className={`text-[10px] font-mono px-2 py-0.5 rounded-full font-bold uppercase ${order.priority === 'critical'
                          ? 'bg-rose-500/10 text-rose-600 dark:text-rose-400 border border-rose-500/20'
                          : order.priority === 'high'
                            ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20'
                            : 'bg-sky-500/10 text-sky-600 dark:text-sky-400 border border-sky-500/20'
                          }`}
                      >
                        {order.priority}
                      </span>
                      <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-neutral-200 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300">
                        {order.enforcement}
                      </span>
                    </div>
                  }
                  meta={
                    <div className="flex items-center gap-2 text-[11px] text-neutral-400 font-mono">
                      <span>Applies to: {order.appliesTo?.join(', ')}</span>
                      <span>·</span>
                      <span>v{order.version}</span>
                    </div>
                  }
                  actions={
                    <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={<Edit2 className="w-3.5 h-3.5" />}
                        onClick={() => handleOpenEdit('global', order)}
                        title="Edit Order"
                      />
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={<Trash2 className="w-3.5 h-3.5 text-rose-500" />}
                        onClick={() => setDeleteConfirmation({ type: 'global', item: order })}
                        title="Delete Order"
                      />
                    </div>
                  }
                  onClick={() => setViewingDetailItem({ type: 'global', data: order })}
                >
                  {order.conflictHandling && (
                    <div className="text-[11px] text-neutral-500 italic">
                      Conflict Rule: {order.conflictHandling}
                    </div>
                  )}
                </ItemCard>
              ))}
            </div>
          )}
        </div>
      )}

      {/* TAB 3: COURSES (STEERING) */}
      {activeTab === 'steering' && (
        <div className="space-y-3">
          {filteredSteering.length === 0 ? (
            <div className="p-8 text-center rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-800 space-y-3 bg-white/50 dark:bg-[#15171a]/50">
              <Compass className="w-8 h-8 mx-auto text-neutral-400" />
              <p className="text-xs text-neutral-500 max-w-sm mx-auto">
                No course corrections are configured. Add scoped guidance where the fleet needs it.
              </p>
              <Button variant="primary" size="sm" onClick={() => handleOpenAdd('steering')} icon={<Plus className="w-3.5 h-3.5" />}>
                Set Course
              </Button>
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
              {filteredSteering.map((dir) => (
                <ItemCard
                  key={dir.id}
                  title={dir.name}
                  subtitle={dir.guidance}
                  badge={
                    <span className="text-[10px] font-mono px-2 py-0.5 rounded-full font-bold uppercase bg-teal-500/10 text-teal-600 dark:text-teal-400 border border-teal-500/20">
                      {dir.targetType}: {dir.targetId}
                    </span>
                  }
                  meta={
                    <div className="flex items-center gap-2 text-[11px] text-neutral-400 font-mono">
                      <span>Priority: #{dir.priority}</span>
                      <span>·</span>
                      <span>Policy: {dir.overridePolicy}</span>
                      <span>·</span>
                      <span>v{dir.version}</span>
                    </div>
                  }
                  actions={
                    <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={<Edit2 className="w-3.5 h-3.5" />}
                        onClick={() => handleOpenEdit('steering', dir)}
                        title="Edit Course"
                      />
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={<Trash2 className="w-3.5 h-3.5 text-rose-500" />}
                        onClick={() => setDeleteConfirmation({ type: 'steering', item: dir })}
                        title="Delete Course"
                      />
                    </div>
                  }
                  onClick={() => setViewingDetailItem({ type: 'steering', data: dir })}
                />
              ))}
            </div>
          )}
        </div>
      )}

      {/* TAB 4: HOOKS */}
      {activeTab === 'hooks' && (
        <div className="space-y-3">
          {filteredHooks.length === 0 ? (
            <div className="p-8 text-center rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-800 space-y-3 bg-white/50 dark:bg-[#15171a]/50">
              <Workflow className="w-8 h-8 mx-auto text-neutral-400" />
              <p className="text-xs text-neutral-500 max-w-sm mx-auto">
                No signals are rigged. Create a hook to automate the fleet’s response.
              </p>
              <Button variant="primary" size="sm" onClick={() => handleOpenAdd('hook')} icon={<Plus className="w-3.5 h-3.5" />}>
                Rig Hook
              </Button>
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
              {filteredHooks.map((hook) => (
                <ItemCard
                  key={hook.id}
                  title={hook.name}
                  subtitle={`Trigger: ${hook.triggerEvent} → Action: ${hook.actionType}`}
                  badge={
                    <span
                      className={`text-[10px] font-mono px-2 py-0.5 rounded-full font-bold uppercase ${hook.status === 'active'
                        ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20'
                        : hook.status === 'paused'
                          ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20'
                          : 'bg-rose-500/10 text-rose-600 dark:text-rose-400 border border-rose-500/20'
                        }`}
                    >
                      {hook.status}
                    </span>
                  }
                  meta={
                    <div className="flex items-center gap-2 text-[11px] text-neutral-400 font-mono">
                      <span>Mode: {hook.executionMode}</span>
                      <span>·</span>
                      <span>Last: {hook.lastRunAt || 'Never'} ({hook.lastRunStatus || 'idle'})</span>
                    </div>
                  }
                  actions={
                    <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={hook.status === 'paused' ? <Play className="w-3.5 h-3.5" /> : <Pause className="w-3.5 h-3.5" />}
                        onClick={() => {
                          updateTrainingHook(hook.id, { status: hook.status === 'paused' ? 'active' : 'paused' });
                          showToast(hook.status === 'paused' ? `Resumed Hook: ${hook.name}` : `Paused Hook: ${hook.name}`);
                        }}
                        title={hook.status === 'paused' ? 'Resume Hook' : 'Pause Hook'}
                      />
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={<Edit2 className="w-3.5 h-3.5" />}
                        onClick={() => handleOpenEdit('hook', hook)}
                        title="Edit Hook"
                      />
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={<Trash2 className="w-3.5 h-3.5 text-rose-500" />}
                        onClick={() => setDeleteConfirmation({ type: 'hook', item: hook })}
                        title="Delete Hook"
                      />
                    </div>
                  }
                  onClick={() => setViewingDetailItem({ type: 'hook', data: hook })}
                />
              ))}
            </div>
          )}
        </div>
      )}

      {/* MENTOR AI ASSISTANT MODAL */}
      <Modal
        isOpen={isMentorOpen}
        onClose={() => setIsMentorOpen(false)}
        title="Mentor"
        description="Consult the fleet’s training officer."
        maxWidth="max-w-2xl"
      >
        <div className="space-y-4">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Configuration Target Context
            </label>
            <div className="grid grid-cols-4 gap-2">
              {[
                { id: 'skills', label: 'Skill' },
                { id: 'global', label: 'Global Steering' },
                { id: 'steering', label: 'Steering' },
                { id: 'hooks', label: 'Hook' }
              ].map((c) => (
                <button
                  key={c.id}
                  type="button"
                  onClick={() => setMentorContext(c.id as any)}
                  className={`px-2.5 py-1.5 rounded-lg border text-xs font-semibold transition-all cursor-pointer ${mentorContext === c.id
                    ? 'border-teal-500 bg-teal-500/10 text-teal-600 dark:text-teal-400'
                    : 'border-neutral-200 dark:border-neutral-800 text-neutral-600 dark:text-neutral-400 hover:border-neutral-300'
                    }`}
                >
                  {c.label}
                </button>
              ))}
            </div>
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              What do you want to configure?
            </label>
            <textarea
              rows={3}
              value={mentorPrompt}
              onChange={(e) => setMentorPrompt(e.target.value)}
              placeholder="Describe the objective, target, constraints, and expected outcome..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-1 focus:ring-teal-500 resize-none font-mono"
            />
          </div>

          {/* Prompt Starters */}
          <div className="flex flex-wrap gap-1.5">
            {[
              'Draft a new skill',
              'Create a fleet-wide order',
              'Add guidance for a specific workflow',
              'Automate an event with a hook'
            ].map((p) => (
              <button
                key={p}
                type="button"
                onClick={() => setMentorPrompt(p)}
                className="px-2 py-1 rounded-md bg-neutral-100 dark:bg-neutral-800 text-[11px] text-neutral-600 dark:text-neutral-300 hover:text-teal-600 dark:hover:text-teal-400 cursor-pointer transition-colors"
              >
                + {p}
              </button>
            ))}
          </div>

          <div className="flex justify-end">
            <Button
              variant="primary"
              size="sm"
              disabled={!mentorPrompt.trim() || isGeneratingDraft}
              onClick={handleGenerateMentorDraft}
              icon={<Sparkles className="w-3.5 h-3.5" />}
            >
              {isGeneratingDraft ? 'Reviewing the chart and preparing a directive…' : 'Generate Draft'}
            </Button>
          </div>

          {/* Generated Draft Output Container */}
          {generatedDraft && (
            <div className="p-4 rounded-xl border border-teal-500/30 bg-teal-500/5 space-y-3 animate-in fade-in duration-200">
              <div className="flex items-center justify-between">
                <span className="text-xs font-bold text-teal-600 dark:text-teal-400 uppercase tracking-wider flex items-center gap-1.5">
                  <CheckCircle2 className="w-3.5 h-3.5" />
                  Recommended Configuration
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-teal-500/20 text-teal-500 font-semibold">
                  Ready for Review
                </span>
              </div>

              <div className="space-y-1.5 text-xs text-neutral-800 dark:text-neutral-200">
                <p className="font-semibold text-neutral-900 dark:text-neutral-100">{generatedDraft.title}</p>
                <p className="text-neutral-600 dark:text-neutral-400">{generatedDraft.summary}</p>
              </div>

              {generatedDraft.risks && (
                <div className="p-2 rounded-lg bg-amber-500/10 border border-amber-500/20 text-[11px] text-amber-700 dark:text-amber-300 flex items-start gap-1.5">
                  <AlertTriangle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
                  <span>{generatedDraft.risks}</span>
                </div>
              )}

              <div className="pt-2 flex justify-end gap-2 border-t border-teal-500/20">
                <Button variant="ghost" size="sm" onClick={() => setGeneratedDraft(null)}>
                  Discard
                </Button>
                <Button variant="primary" size="sm" onClick={handleApplyMentorDraft} icon={<Check className="w-3.5 h-3.5" />}>
                  Apply Draft
                </Button>
              </div>
            </div>
          )}
        </div>
      </Modal>

      {/* CREATE / EDIT MODAL FOR SKILL */}
      <Modal
        isOpen={modalType === 'skill'}
        onClose={() => setModalType(null)}
        title={editingItem ? 'Update Skill' : 'Add Skill'}
        description="Define a reusable capability for specialist models, tools, or workflows."
        maxWidth="max-w-xl"
      >
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Skill Name*
            </label>
            <input
              type="text"
              value={skillForm.name}
              onChange={(e) => setSkillForm({ ...skillForm, name: e.target.value })}
              placeholder="e.g. Incident Briefing Officer"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Purpose*
            </label>
            <input
              type="text"
              value={skillForm.purpose}
              onChange={(e) => setSkillForm({ ...skillForm, purpose: e.target.value })}
              placeholder="Describe what this skill is responsible for"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Status
              </label>
              <SelectDropdown
                value={skillForm.status}
                onChange={(v) => setSkillForm({ ...skillForm, status: v as TrainingConfigStatus })}
                options={[
                  { value: 'active', label: 'Active' },
                  { value: 'draft', label: 'Draft' },
                  { value: 'disabled', label: 'Disabled' }
                ]}
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Access Scope (comma separated)
              </label>
              <input
                type="text"
                value={skillForm.accessScopeText}
                onChange={(e) => setSkillForm({ ...skillForm, accessScopeText: e.target.value })}
                placeholder="e.g. engineering, fleet-wide, operations"
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
              />
            </div>
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Output Format (optional)
            </label>
            <input
              type="text"
              value={skillForm.outputFormat}
              onChange={(e) => setSkillForm({ ...skillForm, outputFormat: e.target.value })}
              placeholder="e.g. Structured JSON, Markdown Report"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          {/* Instruction Box at the VERY BOTTOM */}
          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300">
                Instructions &amp; Operating Procedure*
              </label>
              <span className="text-[10px] text-neutral-400 font-mono">Detailed system prompt &amp; guidelines</span>
            </div>
            <textarea
              rows={6}
              value={skillForm.instructions}
              onChange={(e) => setSkillForm({ ...skillForm, instructions: e.target.value })}
              placeholder="Define the operating procedure, detailed instructions, and constraints for this skill..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none resize-y min-h-[140px] font-mono leading-relaxed"
            />
          </div>

          <div className="flex justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
            <Button variant="ghost" size="sm" onClick={() => handleSaveSkill(true)}>
              Save Draft
            </Button>
            <Button variant="primary" size="sm" onClick={() => handleSaveSkill(false)}>
              {editingItem ? 'Update Skill' : 'Add Skill'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* CREATE / EDIT MODAL FOR ORDERS (GLOBAL STEERING) */}
      <Modal
        isOpen={modalType === 'global'}
        onClose={() => setModalType(null)}
        title={editingItem ? 'Update Order' : 'Issue Order'}
        description="Manage high-priority baseline instructions that apply across the fleet."
        maxWidth="max-w-xl"
      >
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Directive Title*
            </label>
            <input
              type="text"
              value={globalForm.name}
              onChange={(e) => setGlobalForm({ ...globalForm, name: e.target.value })}
              placeholder="e.g. Return structured output for fleet workflows"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Priority*
              </label>
              <SelectDropdown
                value={globalForm.priority}
                onChange={(v) => setGlobalForm({ ...globalForm, priority: v as any })}
                options={[
                  { value: 'critical', label: 'Critical' },
                  { value: 'high', label: 'High' },
                  { value: 'standard', label: 'Standard' }
                ]}
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Enforcement Mode*
              </label>
              <SelectDropdown
                value={globalForm.enforcement}
                onChange={(v) => setGlobalForm({ ...globalForm, enforcement: v as any })}
                options={[
                  { value: 'required', label: 'Required (Mandatory)' },
                  { value: 'advisory', label: 'Advisory (Guidance)' }
                ]}
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Applies To (comma separated)
              </label>
              <input
                type="text"
                value={globalForm.appliesToText}
                onChange={(e) => setGlobalForm({ ...globalForm, appliesToText: e.target.value })}
                placeholder="e.g. workflows, artifacts, engineering"
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Status
              </label>
              <SelectDropdown
                value={globalForm.status}
                onChange={(v) => setGlobalForm({ ...globalForm, status: v as TrainingConfigStatus })}
                options={[
                  { value: 'active', label: 'Active' },
                  { value: 'draft', label: 'Draft' },
                  { value: 'disabled', label: 'Disabled' }
                ]}
              />
            </div>
          </div>

          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Conflict Handling
            </label>
            <input
              type="text"
              value={globalForm.conflictHandling}
              onChange={(e) => setGlobalForm({ ...globalForm, conflictHandling: e.target.value })}
              placeholder="e.g. Applies unless conversational narrative output is explicitly requested"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          {/* Standing Order Instruction Box at the VERY BOTTOM */}
          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300">
                Standing Order*
              </label>
              <span className="text-[10px] text-neutral-400 font-mono">Fleet-wide directive prompt</span>
            </div>
            <textarea
              rows={6}
              value={globalForm.directive}
              onChange={(e) => setGlobalForm({ ...globalForm, directive: e.target.value })}
              placeholder="All deliverables intended for downstream automated consumption must adhere to typed JSON or standardized Markdown schema..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none resize-y min-h-[140px] font-mono leading-relaxed"
            />
          </div>

          <div className="flex justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
            <Button variant="ghost" size="sm" onClick={() => handleSaveGlobal(true)}>
              Save Draft
            </Button>
            <Button variant="primary" size="sm" onClick={() => handleSaveGlobal(false)}>
              {editingItem ? 'Update Order' : 'Issue Order'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* CREATE / EDIT MODAL FOR COURSES (STEERING) */}
      <Modal
        isOpen={modalType === 'steering'}
        onClose={() => setModalType(null)}
        title={editingItem ? 'Update Course' : 'Set Course'}
        description="Manage guidance that applies to a defined target such as a role, skill, workflow, or workspace."
        maxWidth="max-w-xl"
      >
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Course Name*
            </label>
            <input
              type="text"
              value={steeringForm.name}
              onChange={(e) => setSteeringForm({ ...steeringForm, name: e.target.value })}
              placeholder="e.g. Creative Assistant Brand Alignment"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Target Type*
              </label>
              <SelectDropdown
                value={steeringForm.targetType}
                onChange={(v) => setSteeringForm({ ...steeringForm, targetType: v as any })}
                options={[
                  { value: 'role', label: 'Role' },
                  { value: 'skill', label: 'Skill' },
                  { value: 'workflow', label: 'Workflow' },
                  { value: 'workspace', label: 'Workspace' }
                ]}
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Target Identifier*
              </label>
              <input
                type="text"
                value={steeringForm.targetId}
                onChange={(e) => setSteeringForm({ ...steeringForm, targetId: e.target.value })}
                placeholder="e.g. brand_reviewer"
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
              />
            </div>
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Override Policy
              </label>
              <SelectDropdown
                value={steeringForm.overridePolicy}
                onChange={(v) => setSteeringForm({ ...steeringForm, overridePolicy: v as any })}
                options={[
                  { value: 'inherit', label: 'Inherit' },
                  { value: 'override', label: 'Override' },
                  { value: 'append', label: 'Append' }
                ]}
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Priority Rank
              </label>
              <input
                type="number"
                value={steeringForm.priority}
                onChange={(e) => setSteeringForm({ ...steeringForm, priority: parseInt(e.target.value) || 1 })}
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Status
              </label>
              <SelectDropdown
                value={steeringForm.status}
                onChange={(v) => setSteeringForm({ ...steeringForm, status: v as TrainingConfigStatus })}
                options={[
                  { value: 'active', label: 'Active' },
                  { value: 'draft', label: 'Draft' },
                  { value: 'disabled', label: 'Disabled' }
                ]}
              />
            </div>
          </div>

          {/* Guidance Instruction Box at the VERY BOTTOM */}
          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300">
                Course Guidance &amp; Instructions*
              </label>
              <span className="text-[10px] text-neutral-400 font-mono">Scoped prompt directive</span>
            </div>
            <textarea
              rows={6}
              value={steeringForm.guidance}
              onChange={(e) => setSteeringForm({ ...steeringForm, guidance: e.target.value })}
              placeholder="Write the specific guidance, behavioral rules, or steering instructions..."
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none resize-y min-h-[140px] font-mono leading-relaxed"
            />
          </div>

          <div className="flex justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
            <Button variant="ghost" size="sm" onClick={() => handleSaveSteering(true)}>
              Save Draft
            </Button>
            <Button variant="primary" size="sm" onClick={() => handleSaveSteering(false)}>
              {editingItem ? 'Update Course' : 'Set Course'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* CREATE / EDIT MODAL FOR HOOK */}
      <Modal
        isOpen={modalType === 'hook'}
        onClose={() => setModalType(null)}
        title={editingItem ? 'Update Hook' : 'Rig Hook'}
        description="Configure event-driven automation triggered by signals and conditions."
        maxWidth="max-w-xl"
      >
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
              Hook Name*
            </label>
            <input
              type="text"
              value={hookForm.name}
              onChange={(e) => setHookForm({ ...hookForm, name: e.target.value })}
              placeholder="e.g. Policy Violation Router"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Trigger Event*
              </label>
              <input
                type="text"
                value={hookForm.triggerEvent}
                onChange={(e) => setHookForm({ ...hookForm, triggerEvent: e.target.value })}
                placeholder="e.g. policy_validation_failed"
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none font-mono"
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Action Type*
              </label>
              <input
                type="text"
                value={hookForm.actionType}
                onChange={(e) => setHookForm({ ...hookForm, actionType: e.target.value })}
                placeholder="e.g. route_to_review_queue"
                className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none font-mono"
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Execution Mode
              </label>
              <SelectDropdown
                value={hookForm.executionMode}
                onChange={(v) => setHookForm({ ...hookForm, executionMode: v as any })}
                options={[
                  { value: 'automatic', label: 'Automatic' },
                  { value: 'approval_required', label: 'Require Approval' },
                  { value: 'simulation', label: 'Simulation' }
                ]}
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-neutral-700 dark:text-neutral-300 mb-1">
                Failure Handling
              </label>
              <SelectDropdown
                value={hookForm.failureHandling}
                onChange={(v) => setHookForm({ ...hookForm, failureHandling: v as any })}
                options={[
                  { value: 'notify', label: 'Notify' },
                  { value: 'retry', label: 'Retry' },
                  { value: 'queue_for_review', label: 'Queue for Review' },
                  { value: 'stop', label: 'Stop' }
                ]}
              />
            </div>
          </div>

          <div className="flex justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
            <Button variant="ghost" size="sm" onClick={() => handleSaveHook(true)}>
              Save Draft
            </Button>
            <Button variant="primary" size="sm" onClick={() => handleSaveHook(false)}>
              {editingItem ? 'Update Hook' : 'Rig Hook'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* DETAIL MODAL */}
      {viewingDetailItem && (
        <Modal
          isOpen={Boolean(viewingDetailItem)}
          onClose={() => setViewingDetailItem(null)}
          title={viewingDetailItem.data.name}
          description={`Directive Type: ${viewingDetailItem.type.toUpperCase()}`}
          maxWidth="max-w-xl"
        >
          <div className="space-y-3 text-xs">
            <div className="p-3 rounded-xl bg-neutral-50 dark:bg-neutral-900 space-y-1 font-mono">
              <span className="text-neutral-400 block text-[10px]">DIRECTIVE ID &amp; AUDIT TRAIL</span>
              <p>ID: {viewingDetailItem.data.id}</p>
              <p>Version: v{viewingDetailItem.data.version}</p>
              <p>Created: {viewingDetailItem.data.createdAt} by {viewingDetailItem.data.createdBy || 'Operator'}</p>
              <p>Updated: {viewingDetailItem.data.updatedAt} by {viewingDetailItem.data.updatedBy || 'Operator'}</p>
            </div>

            <div className="space-y-1">
              <span className="font-semibold text-neutral-700 dark:text-neutral-300">Instructions / Guidance:</span>
              <p className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950 font-mono leading-relaxed">
                {viewingDetailItem.data.instructions || viewingDetailItem.data.directive || viewingDetailItem.data.guidance || 'No raw text'}
              </p>
            </div>

            <div className="flex justify-end pt-2">
              <Button variant="outline" size="sm" onClick={() => setViewingDetailItem(null)}>
                Close
              </Button>
            </div>
          </div>
        </Modal>
      )}

      {/* DELETE / STAND DOWN CONFIRMATION MODAL */}
      {deleteConfirmation && (
        <Modal
          isOpen={Boolean(deleteConfirmation)}
          onClose={() => setDeleteConfirmation(null)}
          title={`Delete ${deleteConfirmation.item.name}?`}
          description="This action permanently removes the configuration and cannot be undone."
          maxWidth="max-w-md"
        >
          <div className="space-y-4">
            <p className="text-xs text-neutral-600 dark:text-neutral-300">
              Type <strong className="font-mono text-rose-500">DELETE</strong> to confirm permanent deletion of this directive.
            </p>
            <input
              type="text"
              value={deleteInputText}
              onChange={(e) => setDeleteInputText(e.target.value)}
              placeholder="DELETE"
              className="w-full px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs font-mono focus:outline-none"
            />
            <div className="flex justify-end gap-2">
              <Button variant="ghost" size="sm" onClick={() => setDeleteConfirmation(null)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                size="sm"
                disabled={deleteInputText !== 'DELETE'}
                onClick={handleDeleteConfirmed}
              >
                Delete Permanently
              </Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
};

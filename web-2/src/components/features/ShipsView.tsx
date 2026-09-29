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
  Plus
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';

export const ShipsView: React.FC = () => {
  const { ships, crew, quests, artifacts, setActiveTab, createQuest } = useFleetStore();
  const [selectedShipId, setSelectedShipId] = useState<string>('ship-dev');

  const selectedShip = ships.find((s) => s.id === selectedShipId) || ships[0];
  const shipCrew = crew.filter((c) => c.shipId === selectedShip.id);
  const shipQuests = quests.filter((q) => q.assignedShipId === selectedShip.id || q.suggestedShipId === selectedShip.id);
  const shipArtifacts = artifacts.filter((a) => a.shipId === selectedShip.id);

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-y-auto p-4 sm:p-6 space-y-6 max-w-6xl mx-auto w-full animate-view-fade-in">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-xl sm:text-2xl font-bold tracking-tight text-neutral-900 dark:text-neutral-100">
              Ships &amp; Squads
            </h1>
            <span className="text-xs font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10">
              {ships.length} Persistent Teams
            </span>
          </div>
          <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
            A Ship is an operational home for a persistent specialist AI team with its own Charter, Navigator, and memory.
          </p>
        </div>

        <button
          onClick={() => setActiveTab('crew')}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 text-xs font-semibold hover:opacity-90 transition-opacity self-start"
        >
          <Plus className="w-3.5 h-3.5" />
          <span>Make Me a Squad</span>
        </button>
      </div>

      {/* Ships Switcher Tabs */}
      <div className="flex gap-2 overflow-x-auto pb-1 border-b border-neutral-200 dark:border-neutral-800">
        {ships.map((ship) => {
          const isSelected = ship.id === selectedShip.id;
          return (
            <button
              key={ship.id}
              onClick={() => setSelectedShipId(ship.id)}
              className={`flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-colors shrink-0 ${
                isSelected
                  ? 'bg-neutral-100 dark:bg-neutral-800 text-teal-700 dark:text-teal-300 font-semibold border border-neutral-300 dark:border-neutral-700'
                  : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white'
              }`}
            >
              <Ship className={`w-3.5 h-3.5 ${isSelected ? 'text-teal-600 dark:text-teal-400' : 'text-neutral-400'}`} />
              <span>{ship.name}</span>
              <span className="text-[10px] font-mono px-1 py-0.2 rounded bg-neutral-200 dark:bg-neutral-700 text-neutral-700 dark:text-neutral-300">
                {ship.crewIds.length} Crew
              </span>
            </button>
          );
        })}
      </div>

      {/* Selected Ship Showcase */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left Column: Navigator & Status */}
        <div className="space-y-4 lg:col-span-2">
          {/* Navigator Briefing Box */}
          <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3">
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
                className="px-2.5 py-1 text-xs rounded border border-neutral-200 dark:border-neutral-700 text-neutral-700 dark:text-neutral-300 hover:border-teal-500 transition-colors"
              >
                Inspect Quests →
              </button>
              <button
                onClick={() => setActiveTab('approvals')}
                className="px-2.5 py-1 text-xs rounded bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/30 hover:bg-amber-500/20 transition-colors"
              >
                Review Blocker Approval
              </button>
            </div>
          </div>

          {/* Crew Specialists Roster */}
          <div className="space-y-2.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                Assigned Specialist Crew ({shipCrew.length} / 5 Berths)
              </span>
              <button
                onClick={() => setActiveTab('crew')}
                className="text-xs text-teal-600 dark:text-teal-400 hover:underline"
              >
                Manage Crew →
              </button>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              {shipCrew.map((member) => (
                <div
                  key={member.id}
                  className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-2 text-xs"
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
          <div className="space-y-2.5">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
              Active Quests Underway
            </span>

            <div className="space-y-2">
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
        <div className="space-y-4">
          <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-[#15171a] space-y-4 text-xs">
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
    </div>
  );
};

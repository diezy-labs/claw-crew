import React, { useState, useEffect, useMemo, useCallback } from 'react';
import {
  Mic,
  MicOff,
  Volume2,
  VolumeX,
  Compass,
  Ship as ShipIcon,
  Send,
  Check,
  ChevronDown,
  Bell,
  Flame,
  MessageSquare
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';
import { retroAudio } from '../../utils/retroAudio';
import { RealmCanvas, DeckCharacter, CharacterState } from './RealmCanvas';
import { Button } from '../common/Button';
import { Dropdown } from '../common/Dropdown';
import { ToolButton } from '../common/ToolButton';
import { ChatboxSparksEffect } from '../common/ChatboxSparksEffect';
import { getSparksConfig, SparksConfig } from '../../utils/sparksEngine';

export const RealmView: React.FC = () => {
  const { ships, crew } = useFleetStore();

  // Selection Scopes: 'quartermaster' | 'ship:ship-dev' | 'squad:core-dev' | 'crew:crew-repo-analyst'
  const [selectedScope, setSelectedScope] = useState<string>('quartermaster');
  const [isScopeMenuOpen, setIsScopeMenuOpen] = useState(false);

  // Audio / Voice State
  const [isVoiceActive, setIsVoiceActive] = useState(false);
  const [isMuted, setIsMuted] = useState(false);
  const [transcript, setTranscript] = useState('');
  const [textInput, setTextInput] = useState('');
  const [audioLevel, setAudioLevel] = useState<number[]>(Array(16).fill(10));
  const [isThinkingGlobal, setIsThinkingGlobal] = useState(false);
  const [thoughtStage, setThoughtStage] = useState('');

  // Captain Player Character Coordinates
  const [playerPos, setPlayerPos] = useState({ x: 50, y: 70 });
  const [playerFacing, setPlayerFacing] = useState<'left' | 'right' | 'up' | 'down'>('up');
  const [playerSpeech, setPlayerSpeech] = useState<string | null>(null);
  const [isPlayerMoving, setIsPlayerMoving] = useState(false);

  // Deck Props Interaction States
  const [isSteeringHelm, setIsSteeringHelm] = useState(false);
  const [bellRinging, setBellRinging] = useState(false);
  const [cannonSmokes, setCannonSmokes] = useState<{ id: number; side: 'port' | 'starboard'; x: number; y: number }[]>([]);

  // Full Log Drawer
  const [isLogOpen, setIsLogOpen] = useState(false);
  const [sparksConfig, setSparksConfig] = useState<SparksConfig>(() => getSparksConfig());

  useEffect(() => {
    const handleSparksUpdated = (e: any) => {
      if (e.detail) {
        setSparksConfig(e.detail);
      } else {
        setSparksConfig(getSparksConfig());
      }
    };
    window.addEventListener('galleon:sparks-updated', handleSparksUpdated);
    return () => window.removeEventListener('galleon:sparks-updated', handleSparksUpdated);
  }, []);

  const [dialogueHistory, setDialogueHistory] = useState<{
    id: string;
    speaker: string;
    text: string;
    time: string;
    isCaptain: boolean;
  }[]>([
    {
      id: 'init-1',
      speaker: 'Quartermaster',
      text: 'Ahoy, Pirate King! Welcome to the Sovereign Quarterdeck. Speak or type your orders—the deck is primed.',
      time: 'Just now',
      isCaptain: false
    }
  ]);

  // Office & Station Coordinates
  const OFFICE_DOOR = { x: 80, y: 32 };
  const HELM_POS = { x: 50, y: 24 };

  // Generate Deck NPCs based on current selected scope
  const deckCharacters = useMemo<DeckCharacter[]>(() => {
    if (selectedScope === 'quartermaster') {
      return [
        {
          id: 'npc-qm',
          name: 'Quartermaster',
          role: 'Fleet Executive & Chief of Staff',
          avatarLetter: 'Q',
          coatColor: '#EAB308', // Gold
          hatColor: '#1E293B',
          hairColor: '#FFFFFF',
          hasHat: true,
          hasBicorne: true,
          x: 58,
          y: 36,
          facing: 'left',
          state: 'idle'
        }
      ];
    }

    if (selectedScope.startsWith('ship:')) {
      const shipId = selectedScope.split(':')[1];
      const targetShip = ships.find((s) => s.id === shipId) || ships[0];
      if (!targetShip) return [];

      const targetCrewIds = targetShip.crewIds || [];
      const shipCrew = crew.filter((c) => targetCrewIds.includes(c.id));

      const colors = ['#14B8A6', '#3B82F6', '#8B5CF6', '#F59E0B'];
      const hats = ['#0F766E', '#1D4ED8', '#6D28D9', '#B45309'];

      const list: DeckCharacter[] = [
        {
          id: `nav-${targetShip.id}`,
          name: targetShip.navigatorName || 'Ship Navigator',
          role: 'Ship Navigator & Orchestrator',
          avatarLetter: 'N',
          coatColor: '#0D9488',
          hatColor: '#134E4A',
          hairColor: '#D97706',
          hasHat: true,
          hasBicorne: true,
          x: 48,
          y: 36,
          facing: 'down',
          state: 'idle'
        }
      ];

      // Spawn crew members in pleasant arc around deck
      shipCrew.forEach((member, idx) => {
        const angle = (idx / Math.max(1, shipCrew.length - 1)) * Math.PI;
        const radiusX = 22;
        const radiusY = 14;
        const posX = 50 + Math.cos(angle - Math.PI / 2) * radiusX + (idx % 2 === 0 ? -4 : 4);
        const posY = 52 + Math.sin(angle - Math.PI / 2) * radiusY;

        list.push({
          id: member.id,
          name: member.name,
          role: member.role,
          avatarLetter: member.name?.[0] || 'C',
          coatColor: colors[idx % colors.length],
          hatColor: hats[idx % hats.length],
          hairColor: idx % 2 === 0 ? '#451A03' : '#78350F',
          hasHat: idx % 2 === 0,
          x: Math.max(20, Math.min(80, posX)),
          y: Math.max(38, Math.min(74, posY)),
          facing: posX > 50 ? 'left' : 'right',
          state: 'idle'
        });
      });

      return list;
    }

    if (selectedScope.startsWith('crew:')) {
      const crewId = selectedScope.split(':')[1];
      const singleCrew = crew.find((c) => c.id === crewId) || crew[0];
      if (!singleCrew) return [];
      return [
        {
          id: singleCrew.id,
          name: singleCrew.name,
          role: singleCrew.role,
          avatarLetter: singleCrew.name?.[0] || 'C',
          coatColor: '#0EA5E9',
          hatColor: '#0369A1',
          hairColor: '#78350F',
          hasHat: true,
          x: 54,
          y: 45,
          facing: 'left',
          state: 'idle'
        }
      ];
    }

    return [];
  }, [selectedScope, ships, crew]);

  // Dynamic NPC state tracker
  const [npcList, setNpcList] = useState<DeckCharacter[]>(deckCharacters);

  useEffect(() => {
    setNpcList(deckCharacters);
  }, [deckCharacters]);

  // Audio Waveform Visualization Loop
  useEffect(() => {
    const timer = setInterval(() => {
      if (isVoiceActive) {
        setAudioLevel(
          Array(16)
            .fill(0)
            .map(() => 15 + Math.random() * 80)
        );
      } else {
        setAudioLevel(Array(16).fill(10));
      }
    }, 150);
    return () => clearInterval(timer);
  }, [isVoiceActive]);

  // Manual Player Locomotion (Keyboard WASD / Arrows)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (['INPUT', 'TEXTAREA'].includes((e.target as HTMLElement).tagName)) {
        return;
      }

      const step = 3;
      let newX = playerPos.x;
      let newY = playerPos.y;
      let moved = false;

      if (e.key === 'ArrowUp' || e.key === 'w' || e.key === 'W') {
        newY = Math.max(28, playerPos.y - step);
        setPlayerFacing('up');
        moved = true;
      } else if (e.key === 'ArrowDown' || e.key === 's' || e.key === 'S') {
        newY = Math.min(84, playerPos.y + step);
        setPlayerFacing('down');
        moved = true;
      } else if (e.key === 'ArrowLeft' || e.key === 'a' || e.key === 'A') {
        newX = Math.max(14, playerPos.x - step);
        setPlayerFacing('left');
        moved = true;
      } else if (e.key === 'ArrowRight' || e.key === 'd' || e.key === 'D') {
        newX = Math.min(86, playerPos.x + step);
        setPlayerFacing('right');
        moved = true;
      }

      if (moved) {
        setPlayerPos({ x: newX, y: newY });
        setIsPlayerMoving(true);
        retroAudio.playFootstep();

        const distHelm = Math.hypot(newX - HELM_POS.x, newY - HELM_POS.y);
        setIsSteeringHelm(distHelm < 7);
      }
    };

    const handleKeyUp = () => {
      setIsPlayerMoving(false);
    };

    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('keyup', handleKeyUp);
    };
  }, [playerPos, HELM_POS.x, HELM_POS.y]);

  // Move Player via D-Pad or Click
  const movePlayerByDirection = (direction: 'up' | 'down' | 'left' | 'right') => {
    const step = 4;
    let newX = playerPos.x;
    let newY = playerPos.y;
    if (direction === 'up') newY = Math.max(28, playerPos.y - step);
    if (direction === 'down') newY = Math.min(84, playerPos.y + step);
    if (direction === 'left') newX = Math.max(14, playerPos.x - step);
    if (direction === 'right') newX = Math.min(86, playerPos.x + step);

    setPlayerFacing(direction);
    setPlayerPos({ x: newX, y: newY });
    retroAudio.playFootstep();

    const distHelm = Math.hypot(newX - HELM_POS.x, newY - HELM_POS.y);
    setIsSteeringHelm(distHelm < 7);
  };

  const handleDeckClick = (xPercent: number, yPercent: number) => {
    const boundedX = Math.max(14, Math.min(86, xPercent));
    const boundedY = Math.max(28, Math.min(84, yPercent));

    if (boundedX > playerPos.x) setPlayerFacing('right');
    else if (boundedX < playerPos.x) setPlayerFacing('left');
    else if (boundedY < playerPos.y) setPlayerFacing('up');
    else setPlayerFacing('down');

    setPlayerPos({ x: boundedX, y: boundedY });
    retroAudio.playFootstep();

    const distHelm = Math.hypot(boundedX - HELM_POS.x, boundedY - HELM_POS.y);
    setIsSteeringHelm(distHelm < 7);
  };

  // Ring the Brass Bell
  const handleBellClick = () => {
    setBellRinging(true);
    retroAudio.playBell();
    setTimeout(() => setBellRinging(false), 800);
  };

  // Fire Cannon
  const handleCannonClick = (side: 'port' | 'starboard') => {
    retroAudio.playCannon();
    const id = Date.now();
    const x = side === 'port' ? 10 : 90;
    const y = 58;
    setCannonSmokes((prev) => [...prev, { id, side, x, y }]);
    setTimeout(() => {
      setCannonSmokes((prev) => prev.filter((c) => c.id !== id));
    }, 1200);
  };

  // Dispatch Command to Deck Crew
  const dispatchCommand = useCallback(
    (commandText: string) => {
      if (!commandText.trim()) return;

      retroAudio.playBleep(620);
      setPlayerSpeech(commandText);
      setTimeout(() => setPlayerSpeech(null), 5500);

      const timeStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      setDialogueHistory((prev) => [
        ...prev,
        {
          id: 'msg-' + Date.now(),
          speaker: 'Captain (Pirate King)',
          text: commandText,
          time: timeStr,
          isCaptain: true
        }
      ]);

      // Trigger Thinking Simulation on NPCs
      setIsThinkingGlobal(true);
      setThoughtStage('Synthesizing intent with Fleet memory scope...');
      retroAudio.playThinkingChime();

      setNpcList((prev) =>
        prev.map((npc) => ({
          ...npc,
          state: npc.state === 'in_office' ? 'in_office' : 'thinking',
          thoughtText: 'Deliberating operational options...'
        }))
      );

      const isHeavy =
        commandText.toLowerCase().includes('heavy') ||
        commandText.toLowerCase().includes('audit') ||
        commandText.toLowerCase().includes('full scan') ||
        commandText.toLowerCase().includes('ci triage') ||
        commandText.toLowerCase().includes('benchmark');

      setTimeout(() => {
        setIsThinkingGlobal(false);

        setNpcList((prev) => {
          if (prev.length === 0) return prev;

          // If heavy task requested: Send crew member into office!
          if (isHeavy) {
            const candidate = prev.find((n) => n.state !== 'in_office') || prev[0];
            retroAudio.playOfficeDoor();

            const departureReply = `Aye Captain! Off to the Chart Room to execute "${commandText}". I'll have the full artifact logged soon.`;
            retroAudio.speakText(departureReply, candidate.role);

            setDialogueHistory((hist) => [
              ...hist,
              {
                id: 'resp-' + Date.now(),
                speaker: candidate.name,
                text: departureReply,
                time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
                isCaptain: false
              }
            ]);

            return prev.map((n) => {
              if (n.id === candidate.id) {
                return {
                  ...n,
                  state: 'in_office',
                  x: OFFICE_DOOR.x,
                  y: OFFICE_DOOR.y,
                  activeSpeech: departureReply,
                  officeTask: {
                    title: commandText,
                    progress: 15,
                    startedAt: 'Just now'
                  }
                };
              }
              return { ...n, state: 'idle', thoughtText: undefined };
            });
          }

          // Ordinary Conversational Response
          const speaker = prev[0];
          let reply = '';
          if (speaker.name === 'Quartermaster') {
            reply = `Directives noted, Captain. Current fleet cost is healthy ($6.23 / $60.00 cap). Mission Board has 1 pending triage. Ready to chart course!`;
          } else if (speaker.role.includes('Navigator')) {
            reply = `Aye Captain! Specialist crew standing by at stations. Bounded tools ready for next voyage.`;
          } else {
            reply = `Understood, Captain. Memory scope verified under read-first policy. Standing by for specific bounded Quest execution.`;
          }

          retroAudio.playBleep(540);
          retroAudio.speakText(reply, speaker.role);

          setDialogueHistory((hist) => [
            ...hist,
            {
              id: 'resp-' + Date.now(),
              speaker: speaker.name,
              text: reply,
              time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
              isCaptain: false
            }
          ]);

          return prev.map((n) => {
            if (n.id === speaker.id) {
              return {
                ...n,
                state: 'speaking',
                activeSpeech: reply,
                thoughtText: undefined
              };
            }
            return { ...n, state: 'idle', thoughtText: undefined };
          });
        });
      }, 1800);
    },
    [OFFICE_DOOR.x, OFFICE_DOOR.y]
  );

  // Toggle Voice Recognition
  const toggleVoiceMode = () => {
    if (isVoiceActive) {
      retroAudio.stopSpeechRecognition();
      setIsVoiceActive(false);
      setTranscript('');
    } else {
      const started = retroAudio.startSpeechRecognition(
        (text, isFinal) => {
          setTranscript(text);
          if (isFinal) {
            dispatchCommand(text);
            setTranscript('');
          }
        },
        () => {
          const simulatedPrompts = [
            'Quartermaster, what is our current fleet cost and active voyages?',
            'Horizon, prepare a comprehensive repository health audit.',
            'QA Reviewer, triage the recent PR timeout and verify risk tiers.',
            'Give me an executive briefing for the release launch.'
          ];
          const chosen = simulatedPrompts[Math.floor(Math.random() * simulatedPrompts.length)];
          setTranscript(chosen);
          setTimeout(() => {
            dispatchCommand(chosen);
            setTranscript('');
            setIsVoiceActive(false);
          }, 1200);
        },
        () => {
          setIsVoiceActive(false);
        }
      );

      if (started) {
        setIsVoiceActive(true);
        retroAudio.playBell();
      } else {
        setIsVoiceActive(true);
        setTimeout(() => {
          const fallback = 'Quartermaster, give me our latest fleet status and cost.';
          setTranscript(fallback);
          setTimeout(() => {
            dispatchCommand(fallback);
            setTranscript('');
            setIsVoiceActive(false);
          }, 1400);
        }, 800);
      }
    }
  };

  // Recall from Office
  const recallFromOffice = (npcId: string) => {
    retroAudio.playOfficeDoor();
    setNpcList((prev) =>
      prev.map((n) => {
        if (n.id === npcId) {
          return {
            ...n,
            state: 'idle',
            x: 55,
            y: 48,
            officeTask: undefined,
            activeSpeech: 'Returned to quarterdeck from chart room!'
          };
        }
        return n;
      })
    );
  };

  return (
    <div className="flex-1 flex flex-col h-[calc(100vh-3.5rem)] overflow-hidden bg-neutral-950 font-sans select-none relative animate-view-fade-in">
      {/* 1. TOP BAR: CONVERSATION PARTICIPANT SELECTOR & CONTROLS */}
      <div className="px-3 sm:px-4 py-2 bg-neutral-900 border-b border-neutral-800 flex flex-wrap items-center justify-between gap-2.5 shrink-0 z-30 min-w-0">
        {/* Left: Participant Scope Dropdown & Deck Indicator */}
        <div className="flex items-center gap-2 shrink-0">
          <Dropdown
            title="Deck Conversational Scope"
            menuWidth="w-72"
            groups={[
              {
                group: 'EXECUTIVE',
                items: [
                  {
                    id: 'quartermaster',
                    label: 'Quartermaster',
                    description: 'AI Executive (1-on-1 helm)',
                    icon: <span className="text-base">👑</span>
                  }
                ]
              },
              {
                group: 'SHIPS (Entire Crew on Deck)',
                items: ships.map((s) => ({
                  id: `ship:${s.id}`,
                  label: s.name,
                  description: `${s.crewIds.length + 1} characters on deck`,
                  icon: <ShipIcon className="w-3.5 h-3.5 text-teal-400" />
                }))
              },
              {
                group: 'PRIVATE SPECIALIST BRIEFING',
                items: crew.slice(0, 4).map((c) => ({
                  id: `crew:${c.id}`,
                  label: c.name,
                  description: c.role || 'Crew specialist',
                  icon: <span className="text-base">👤</span>
                }))
              }
            ]}
            selectedId={selectedScope}
            onSelect={(id) => {
              setSelectedScope(id);
              retroAudio.playBell();
            }}
            trigger={
              <button
                type="button"
                className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-neutral-800 hover:bg-neutral-750 border border-neutral-700 text-xs font-semibold text-neutral-100 transition-colors shadow-xs cursor-pointer"
              >
                <Compass className="w-3.5 h-3.5 text-teal-400" />
                <span className="text-neutral-400 font-normal hidden sm:inline">Deck Roster:</span>
                <span className="text-teal-400 font-bold truncate max-w-[130px] sm:max-w-none">
                  {selectedScope === 'quartermaster'
                    ? '👑 Quartermaster'
                    : selectedScope.startsWith('ship:')
                    ? `⚓ ${ships.find((s) => s.id === (selectedScope.split(':')[1] || ''))?.name || 'Ship'}`
                    : `👤 ${crew.find((c) => c.id === (selectedScope.split(':')[1] || ''))?.name || 'Crew'}`}
                </span>
                <ChevronDown className="w-3 h-3 text-neutral-400" />
              </button>
            }
          />

          {/* Quick Deck Actions */}
          <div className="hidden sm:flex items-center gap-1.5">
            <ToolButton
              onClick={handleBellClick}
              icon={<Bell className="w-3.5 h-3.5 text-amber-400" />}
              label="Ring Bell"
              shortLabel="Bell"
              active={bellRinging}
              variant={bellRinging ? 'amber' : 'default'}
              size="xs"
              title="Ring ship's bell"
            />

            <ToolButton
              onClick={() => handleCannonClick('port')}
              icon={<Flame className="w-3.5 h-3.5 text-red-400" />}
              label="Fire Cannon"
              shortLabel="Cannon"
              size="xs"
              title="Fire port cannon"
            />
          </div>
        </div>

        {/* Right: Audio Waveform & Voice Mode Controls */}
        <div className="flex items-center gap-1.5 sm:gap-2 shrink-0 ml-auto">
          {/* 8-bit Oscilloscope Audio Frequency Bars */}
          <div className="hidden sm:flex items-center gap-0.5 px-2 py-1 rounded-md bg-neutral-950 border border-neutral-800 h-8">
            {audioLevel.map((lvl, i) => (
              <div
                key={i}
                className="w-1 rounded-xs transition-all duration-75"
                style={{
                  height: `${Math.max(4, lvl * 0.22)}px`,
                  backgroundColor: isVoiceActive
                    ? i % 2 === 0
                      ? '#2DD4BF' // teal
                      : '#38BDF8' // sky
                    : '#525252'
                }}
              />
            ))}
          </div>

          {/* Mute Toggle */}
          <ToolButton
            onClick={() => {
              const nextMute = !isMuted;
              setIsMuted(nextMute);
              retroAudio.setMuted(nextMute);
            }}
            icon={isMuted ? <VolumeX className="w-4 h-4" /> : <Volume2 className="w-4 h-4" />}
            size="sm"
            variant={isMuted ? 'danger' : 'default'}
            title={isMuted ? 'Unmute Audio & Voice' : 'Mute Audio & Voice'}
          />

          {/* Main Voice Toggle Button with shortLabel for Mobile */}
          <Button
            variant={isVoiceActive ? 'danger' : 'primary'}
            size="sm"
            onClick={toggleVoiceMode}
            icon={isVoiceActive ? <MicOff className="w-3.5 h-3.5" /> : <Mic className="w-3.5 h-3.5" />}
            shortLabel={isVoiceActive ? 'Rec' : 'Voice'}
            className={isVoiceActive ? 'animate-pulse ring-2 ring-red-500/40 shadow-md' : 'shadow-md'}
          >
            {isVoiceActive ? 'Listening...' : 'Voice Mode'}
          </Button>

          {/* Transcript / Dialogue Log Toggle */}
          <ToolButton
            onClick={() => setIsLogOpen(!isLogOpen)}
            icon={<MessageSquare className="w-4 h-4" />}
            size="sm"
            active={isLogOpen}
            variant={isLogOpen ? 'primary' : 'default'}
            title="Dialogue Log & History"
          />
        </div>
      </div>

      {/* 2. MAIN REALM CANVAS COMPONENT: 8-BIT QUARTERDECK WITH WORK/THINK INDICATORS */}
      <div className="flex-1 relative overflow-hidden">
        <RealmCanvas
          characters={npcList}
          playerPos={playerPos}
          playerFacing={playerFacing}
          isPlayerMoving={isPlayerMoving}
          isSteeringHelm={isSteeringHelm}
          bellRinging={bellRinging}
          cannonSmokes={cannonSmokes}
          isThinkingGlobal={isThinkingGlobal}
          thoughtStage={thoughtStage}
          playerSpeech={playerSpeech}
          onDeckClick={handleDeckClick}
          onHelmClick={() => {
            setIsSteeringHelm(!isSteeringHelm);
            retroAudio.playBell();
          }}
          onBellClick={handleBellClick}
          onCannonClick={handleCannonClick}
          onOfficeClick={() => {
            dispatchCommand('Check progress in the Chart Room Office.');
          }}
          onCharacterClick={(charId) => {
            const char = npcList.find((c) => c.id === charId);
            if (char) {
              dispatchCommand(`Conferring directly with ${char.name}.`);
            }
          }}
          onRecallFromOffice={recallFromOffice}
        />

        {/* Movement Hint Badge */}
        <div className="absolute top-4 left-4 z-20 pointer-events-none bg-neutral-950/70 border border-neutral-700/80 px-2.5 py-1.5 rounded-lg text-[10px] font-mono text-neutral-400 backdrop-blur-xs hidden sm:block">
          <span className="text-teal-400 font-bold">WASD / Arrow Keys</span> or Click Deck to Walk &bull; Click Prop to Interact
        </div>

        {/* On-Screen Retro D-Pad Controller */}
        <div
          onClick={(e) => e.stopPropagation()}
          className="absolute bottom-4 left-4 z-30 opacity-70 hover:opacity-100 transition-opacity bg-neutral-900/85 p-2 rounded-xl border border-neutral-700/80 backdrop-blur-xs flex flex-col items-center gap-1 shadow-2xl"
        >
          <button
            type="button"
            onClick={() => movePlayerByDirection('up')}
            className="w-7 h-7 bg-neutral-800 hover:bg-teal-500 hover:text-neutral-950 border border-neutral-600 rounded flex items-center justify-center font-bold text-xs text-neutral-200 cursor-pointer active:scale-95 shadow-xs"
            title="Move Up (W)"
          >
            ▲
          </button>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => movePlayerByDirection('left')}
              className="w-7 h-7 bg-neutral-800 hover:bg-teal-500 hover:text-neutral-950 border border-neutral-600 rounded flex items-center justify-center font-bold text-xs text-neutral-200 cursor-pointer active:scale-95 shadow-xs"
              title="Move Left (A)"
            >
              ◀
            </button>
            <div className="w-4 h-4 rounded-full bg-amber-400/30 border border-amber-400/50" />
            <button
              type="button"
              onClick={() => movePlayerByDirection('right')}
              className="w-7 h-7 bg-neutral-800 hover:bg-teal-500 hover:text-neutral-950 border border-neutral-600 rounded flex items-center justify-center font-bold text-xs text-neutral-200 cursor-pointer active:scale-95 shadow-xs"
              title="Move Right (D)"
            >
              ▶
            </button>
          </div>
          <button
            type="button"
            onClick={() => movePlayerByDirection('down')}
            className="w-7 h-7 bg-neutral-800 hover:bg-teal-500 hover:text-neutral-950 border border-neutral-600 rounded flex items-center justify-center font-bold text-xs text-neutral-200 cursor-pointer active:scale-95 shadow-xs"
            title="Move Down (S)"
          >
            ▼
          </button>
        </div>
      </div>

      {/* 3. BOTTOM VOICE INPUT & QUICK COMMAND COMPOSER */}
      <div className="p-3 bg-neutral-900 border-t border-neutral-800 shrink-0 z-30">
        <div className="max-w-4xl mx-auto space-y-2">
          {/* Quick Command Action Pills */}
          <div className="flex items-center gap-1.5 overflow-x-auto scrollbar-none py-0.5 text-xs shrink-0 max-w-full">
            <span className="text-[10px] font-mono text-neutral-500 uppercase tracking-wider shrink-0">Quick Commands:</span>
            <ToolButton
              onClick={() => dispatchCommand('Quartermaster, give me our latest fleet status, budget and active voyages.')}
              label="📊 Fleet Status"
              size="xs"
              hideLabelOnMobile={false}
            />
            <ToolButton
              onClick={() => dispatchCommand('Horizon, run a comprehensive repository health and CI triage audit.')}
              label="🛠️ Delegate Heavy Audit"
              size="xs"
              hideLabelOnMobile={false}
              variant="primary"
            />
            <ToolButton
              onClick={() => dispatchCommand('Review current pull request diffs and security risk tiers.')}
              label="🛡️ Risk Review"
              size="xs"
              hideLabelOnMobile={false}
            />
            <ToolButton
              onClick={() => dispatchCommand('What are our active quests and deliverables?')}
              label="🗺️ Quest Roster"
              size="xs"
              hideLabelOnMobile={false}
            />
          </div>

          {/* Voice Transcript / Text Input Bar */}
          <div className="flex items-center gap-2">
            <ChatboxSparksEffect config={sparksConfig} className="flex-1">
              <div className="relative">
                <input
                  type="text"
                  value={transcript || textInput}
                  onChange={(e) => setTextInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      dispatchCommand(textInput);
                      setTextInput('');
                    }
                  }}
                  placeholder={
                    isVoiceActive
                      ? 'Listening... speak to the Quarterdeck crew (e.g. "Prepare release package")'
                      : 'Click mic or type command to speak on deck (WASD / Click to move Captain)...'
                  }
                  className="w-full bg-neutral-950 border border-neutral-700 rounded-xl px-4 py-2.5 text-xs text-neutral-100 placeholder:text-neutral-500 focus:outline-none focus:border-teal-500 font-mono shadow-inner transition-colors"
                />
                {transcript && (
                  <span className="absolute right-3 top-2.5 text-[10px] text-teal-400 font-mono animate-pulse">
                    VOICE DETECTED
                  </span>
                )}
              </div>
            </ChatboxSparksEffect>

            {/* Mic Toggle Button */}
            <Button
              variant={isVoiceActive ? 'danger' : 'secondary'}
              size="sm"
              onClick={toggleVoiceMode}
              icon={isVoiceActive ? <MicOff className="w-4 h-4" /> : <Mic className="w-4 h-4 text-teal-400" />}
              title={isVoiceActive ? 'Stop Voice Recording' : 'Start Voice Mode'}
              className={isVoiceActive ? 'animate-pulse px-2.5' : 'px-2.5'}
            />

            {/* Send Button */}
            <Button
              variant="primary"
              size="sm"
              onClick={() => {
                dispatchCommand(textInput);
                setTextInput('');
              }}
              disabled={!textInput.trim() && !transcript.trim()}
              icon={<Send className="w-3.5 h-3.5" />}
              shortLabel="Send"
              className="px-4"
            >
              Command
            </Button>
          </div>
        </div>
      </div>

      {/* 4. EXPANDABLE DIALOGUE LOG DRAWER */}
      {isLogOpen && (
        <div className="absolute right-0 top-12 bottom-0 w-80 sm:w-96 bg-neutral-900 border-l border-neutral-800 z-50 flex flex-col shadow-2xl animate-slide-in-right">
          <div className="px-4 py-3 border-b border-neutral-800 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <MessageSquare className="w-4 h-4 text-teal-400" />
              <span className="text-xs font-bold text-neutral-100">Deck Dialogue & Voice Log</span>
            </div>
            <button
              onClick={() => setIsLogOpen(false)}
              className="text-xs text-neutral-400 hover:text-white cursor-pointer px-1.5 py-0.5"
            >
              ✕
            </button>
          </div>

          <div className="flex-1 overflow-y-auto p-4 space-y-3 font-mono text-xs">
            {dialogueHistory.map((entry) => (
              <div
                key={entry.id}
                className={`p-2.5 rounded-lg border leading-relaxed ${
                  entry.isCaptain
                    ? 'bg-teal-500/10 border-teal-500/30 text-teal-200 ml-4'
                    : 'bg-neutral-800/80 border-neutral-700 text-neutral-200 mr-4'
                }`}
              >
                <div className="flex items-center justify-between text-[10px] text-neutral-400 mb-1">
                  <span className="font-bold text-neutral-300">{entry.speaker}</span>
                  <span>{entry.time}</span>
                </div>
                <div>{entry.text}</div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

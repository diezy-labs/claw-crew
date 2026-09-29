import React, { useRef, useEffect, useState, useCallback } from 'react';
import { Sparkles, Brain, DoorOpen, Play, Check } from 'lucide-react';

export type CharacterState = 'idle' | 'walking' | 'thinking' | 'speaking' | 'in_office';

export interface DeckCharacter {
  id: string;
  name: string;
  role: string;
  avatarLetter: string;
  coatColor: string;
  hatColor: string;
  hairColor: string;
  hasHat: boolean;
  hasBicorne?: boolean;
  x: number; // 0 - 100 percentage
  y: number; // 0 - 100 percentage
  targetX?: number;
  targetY?: number;
  facing: 'left' | 'right' | 'up' | 'down';
  state: CharacterState;
  thoughtText?: string;
  activeSpeech?: string;
  officeTask?: {
    title: string;
    progress: number;
    startedAt: string;
  };
}

interface RealmCanvasProps {
  characters: DeckCharacter[];
  playerPos: { x: number; y: number };
  playerFacing: 'left' | 'right' | 'up' | 'down';
  isPlayerMoving: boolean;
  isSteeringHelm: boolean;
  bellRinging: boolean;
  cannonSmokes: { id: number; side: 'port' | 'starboard'; x: number; y: number }[];
  isThinkingGlobal: boolean;
  thoughtStage: string;
  playerSpeech: string | null;
  onDeckClick: (xPercent: number, yPercent: number) => void;
  onHelmClick: () => void;
  onBellClick: () => void;
  onCannonClick: (side: 'port' | 'starboard') => void;
  onOfficeClick: () => void;
  onCharacterClick: (charId: string) => void;
  onRecallFromOffice: (charId: string) => void;
}

export const RealmCanvas: React.FC<RealmCanvasProps> = ({
  characters,
  playerPos,
  playerFacing,
  isPlayerMoving,
  isSteeringHelm,
  bellRinging,
  cannonSmokes,
  isThinkingGlobal,
  thoughtStage,
  playerSpeech,
  onDeckClick,
  onHelmClick,
  onBellClick,
  onCannonClick,
  onOfficeClick,
  onCharacterClick,
  onRecallFromOffice
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Animation frame state
  const [animTick, setAnimTick] = useState(0);
  const [targetIndicator, setTargetIndicator] = useState<{ x: number; y: number } | null>(null);

  // Loop for ocean waves, flickering lanterns, character walk cycles
  useEffect(() => {
    let animId: number;
    let lastTime = performance.now();

    const loop = (currentTime: number) => {
      if (currentTime - lastTime > 80) {
        setAnimTick((t) => (t + 1) % 1000);
        lastTime = currentTime;
      }
      animId = requestAnimationFrame(loop);
    };

    animId = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(animId);
  }, []);

  // Canvas drawing routine
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Enable crisp pixel art rendering
    ctx.imageSmoothingEnabled = false;

    const width = canvas.width;
    const height = canvas.height;

    // Clear canvas
    ctx.clearRect(0, 0, width, height);

    // 1. SKY & DISTANT HORIZON
    const skyGradient = ctx.createLinearGradient(0, 0, 0, height * 0.28);
    skyGradient.addColorStop(0, '#091528');
    skyGradient.addColorStop(0.6, '#0f294a');
    skyGradient.addColorStop(1, '#1b4b73');
    ctx.fillStyle = skyGradient;
    ctx.fillRect(0, 0, width, height * 0.28);

    // Distant Stars (8-bit twinkle)
    const stars = [
      { x: 0.1, y: 0.05 },
      { x: 0.25, y: 0.08 },
      { x: 0.45, y: 0.04 },
      { x: 0.65, y: 0.07 },
      { x: 0.82, y: 0.03 },
      { x: 0.92, y: 0.09 }
    ];
    stars.forEach((star, idx) => {
      const twinkle = (animTick + idx * 3) % 4 === 0;
      ctx.fillStyle = twinkle ? '#ffffff' : '#7dd3fc';
      ctx.fillRect(star.x * width, star.y * height, 2, 2);
    });

    // Distant 8-bit Clouds (Drifting)
    const cloudOffset = (animTick * 0.6) % width;
    ctx.fillStyle = 'rgba(186, 230, 253, 0.25)';
    [0.1, 0.5, 0.85].forEach((baseX) => {
      const cx = (baseX * width + cloudOffset) % (width + 120) - 60;
      ctx.fillRect(cx, height * 0.08, 64, 12);
      ctx.fillRect(cx + 8, height * 0.06, 44, 8);
      ctx.fillRect(cx + 18, height * 0.04, 24, 6);
    });

    // 2. OCEAN WAVES WITH MOVING PIXEL FOAM
    const oceanHeight = height * 0.12;
    const oceanTop = height * 0.16;
    ctx.fillStyle = '#0e3a63';
    ctx.fillRect(0, oceanTop, width, oceanHeight);

    // Wave foam lines
    const waveStep = (animTick * 2) % 32;
    ctx.fillStyle = '#38bdf8';
    for (let x = -32; x < width + 32; x += 16) {
      const waveX = x + waveStep;
      const waveY = oceanTop + 8 + Math.sin((x + animTick * 4) * 0.05) * 4;
      ctx.fillRect(waveX, waveY, 10, 2);
      ctx.fillStyle = '#f0f9ff';
      ctx.fillRect(waveX + 2, waveY - 1, 6, 1);
      ctx.fillStyle = '#38bdf8';
    }

    // 3. GALLEON QUARTERDECK STRUCTURE
    const deckLeft = width * 0.06;
    const deckRight = width * 0.94;
    const deckWidth = deckRight - deckLeft;
    const deckTop = height * 0.26;
    const deckBottom = height;

    // Stern Railing along top
    ctx.fillStyle = '#2d1810';
    ctx.fillRect(deckLeft, deckTop - 8, deckWidth, 8);
    ctx.fillStyle = '#4a2818';
    ctx.fillRect(deckLeft, deckTop - 12, deckWidth, 4);

    // Railing balusters
    const balusterSpacing = deckWidth / 24;
    for (let i = 0; i <= 24; i++) {
      const bx = deckLeft + i * balusterSpacing;
      ctx.fillStyle = '#3e2214';
      ctx.fillRect(bx, deckTop - 14, 3, 14);
      ctx.fillStyle = '#6b3e26';
      ctx.fillRect(bx + 1, deckTop - 14, 1, 14);
    }

    // Main Oak Planks Deck Flooring
    const numPlanks = 16;
    const plankHeight = (deckBottom - deckTop) / numPlanks;

    for (let i = 0; i < numPlanks; i++) {
      const py = deckTop + i * plankHeight;
      // Alternate plank wood shades
      ctx.fillStyle = i % 2 === 0 ? '#8b5a2b' : '#7c4f24';
      ctx.fillRect(deckLeft, py, deckWidth, plankHeight);

      // Plank separation seam line
      ctx.fillStyle = '#4a2e16';
      ctx.fillRect(deckLeft, py, deckWidth, 1.5);

      // Wood grain & nails (pegs)
      ctx.fillStyle = '#38220f';
      ctx.fillRect(deckLeft + 12, py + plankHeight * 0.5, 2, 2);
      ctx.fillRect(deckLeft + deckWidth * 0.35, py + plankHeight * 0.5, 2, 2);
      ctx.fillRect(deckLeft + deckWidth * 0.65, py + plankHeight * 0.5, 2, 2);
      ctx.fillRect(deckRight - 16, py + plankHeight * 0.5, 2, 2);

      // Plank grain highlight
      ctx.fillStyle = 'rgba(255, 255, 255, 0.05)';
      ctx.fillRect(deckLeft + 20, py + 2, deckWidth - 40, 1);
    }

    // Outer Bulwarks (Left & Right Wooden Walls)
    ctx.fillStyle = '#3a1f11';
    ctx.fillRect(deckLeft - 10, deckTop, 10, deckBottom - deckTop);
    ctx.fillRect(deckRight, deckTop, 10, deckBottom - deckTop);

    ctx.fillStyle = '#57301c';
    ctx.fillRect(deckLeft - 6, deckTop, 4, deckBottom - deckTop);
    ctx.fillRect(deckRight + 2, deckTop, 4, deckBottom - deckTop);

    // 4. DECK PROPS & STATIONS

    // A. Port & Starboard Cannons
    const cannonY = deckTop + (deckBottom - deckTop) * 0.45;
    // Port Cannon
    ctx.fillStyle = '#1c1917';
    ctx.fillRect(deckLeft + 4, cannonY - 8, 20, 16);
    ctx.fillStyle = '#44403c';
    ctx.fillRect(deckLeft - 8, cannonY - 4, 16, 8); // barrel
    ctx.fillStyle = '#78350f';
    ctx.fillRect(deckLeft + 8, cannonY + 6, 14, 6); // carriage wheels

    // Starboard Cannon
    ctx.fillStyle = '#1c1917';
    ctx.fillRect(deckRight - 24, cannonY - 8, 20, 16);
    ctx.fillStyle = '#44403c';
    ctx.fillRect(deckRight - 8, cannonY - 4, 16, 8); // barrel
    ctx.fillStyle = '#78350f';
    ctx.fillRect(deckRight - 22, cannonY + 6, 14, 6); // carriage wheels

    // B. Ship Helm (Steering Wheel) on Raised Platform
    const helmX = width * 0.5;
    const helmY = deckTop + 24;

    // Helm Stand
    ctx.fillStyle = '#451a03';
    ctx.fillRect(helmX - 5, helmY + 4, 10, 22);

    // Helm Wheel (Spinning when active)
    ctx.save();
    ctx.translate(helmX, helmY);
    if (isSteeringHelm) {
      ctx.rotate((animTick * 0.08) % (Math.PI * 2));
    }
    // Outer Wheel Ring
    ctx.strokeStyle = '#d97706';
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.arc(0, 0, 16, 0, Math.PI * 2);
    ctx.stroke();

    // Spokes
    ctx.strokeStyle = '#b45309';
    ctx.lineWidth = 2;
    for (let s = 0; s < 8; s++) {
      const angle = (s * Math.PI) / 4;
      ctx.beginPath();
      ctx.moveTo(0, 0);
      ctx.lineTo(Math.cos(angle) * 20, Math.sin(angle) * 20);
      ctx.stroke();
    }
    // Brass Hub
    ctx.fillStyle = '#f59e0b';
    ctx.beginPath();
    ctx.arc(0, 0, 4, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    // C. Ship's Brass Bell
    const bellX = width * 0.26;
    const bellY = deckTop + 14;
    ctx.fillStyle = '#78350f';
    ctx.fillRect(bellX - 2, bellY - 8, 4, 8); // post

    ctx.save();
    ctx.translate(bellX, bellY);
    if (bellRinging) {
      ctx.rotate(Math.sin(animTick * 0.8) * 0.35);
    }
    // Brass Bell Shape
    ctx.fillStyle = '#fbbf24';
    ctx.beginPath();
    ctx.moveTo(-6, 0);
    ctx.lineTo(6, 0);
    ctx.lineTo(8, 12);
    ctx.lineTo(-8, 12);
    ctx.closePath();
    ctx.fill();
    ctx.strokeStyle = '#d97706';
    ctx.lineWidth = 1;
    ctx.stroke();
    // Clapper
    ctx.fillStyle = '#78350f';
    ctx.fillRect(-1.5, 12, 3, 4);
    ctx.restore();

    // D. Navigator's Chart Table
    const tableX = width * 0.32;
    const tableY = deckTop + (deckBottom - deckTop) * 0.32;
    ctx.fillStyle = '#3e2214';
    ctx.fillRect(tableX - 28, tableY - 14, 56, 28);
    ctx.fillStyle = '#5c331c';
    ctx.fillRect(tableX - 26, tableY - 12, 52, 24);

    // Rolled Nautical Map Parchment
    ctx.fillStyle = '#fef3c7';
    ctx.fillRect(tableX - 20, tableY - 8, 32, 16);
    ctx.fillStyle = '#d97706';
    ctx.fillRect(tableX - 16, tableY - 4, 18, 1);
    ctx.fillRect(tableX - 16, tableY, 22, 1);
    ctx.fillRect(tableX - 16, tableY + 4, 12, 1);

    // Glowing Table Oil Lamp
    const lampX = tableX + 18;
    const lampY = tableY - 2;
    const lampFlicker = Math.sin(animTick * 0.5) * 1.5;
    ctx.fillStyle = 'rgba(251, 191, 36, 0.25)';
    ctx.beginPath();
    ctx.arc(lampX, lampY, 14 + lampFlicker, 0, Math.PI * 2);
    ctx.fill();

    ctx.fillStyle = '#78350f';
    ctx.fillRect(lampX - 3, lampY - 2, 6, 6);
    ctx.fillStyle = '#fef08a';
    ctx.fillRect(lampX - 2, lampY - 6, 4, 4);

    // E. Captain's Cabin / Specialist Office (Top Right)
    const officeX = width * 0.76;
    const officeY = deckTop + 4;
    const officeW = width * 0.16;
    const officeH = height * 0.24;

    // Office Outer Wooden Cabin Wall
    ctx.fillStyle = '#26130b';
    ctx.fillRect(officeX, officeY, officeW, officeH);
    ctx.strokeStyle = '#4a2818';
    ctx.lineWidth = 2;
    ctx.strokeRect(officeX, officeY, officeW, officeH);

    // Office Roof Trim
    ctx.fillStyle = '#3a1e12';
    ctx.fillRect(officeX - 4, officeY - 6, officeW + 8, 6);

    // Office Doorway
    const doorX = officeX + 8;
    const doorY = officeY + officeH - 36;
    const hasWorkerInOffice = characters.some((c) => c.state === 'in_office');

    ctx.fillStyle = hasWorkerInOffice ? '#451a03' : '#140a05';
    ctx.fillRect(doorX, doorY, 22, 34);
    ctx.strokeStyle = '#78350f';
    ctx.strokeRect(doorX, doorY, 22, 34);

    // Brass doorknob
    ctx.fillStyle = '#fbbf24';
    ctx.fillRect(doorX + 17, doorY + 16, 3, 3);

    // Warm Window Porthole / Light
    const winX = officeX + officeW - 24;
    const winY = officeY + 12;
    ctx.fillStyle = hasWorkerInOffice ? '#fef08a' : '#451a03';
    ctx.fillRect(winX, winY, 14, 14);
    ctx.strokeStyle = '#b45309';
    ctx.strokeRect(winX, winY, 14, 14);

    if (hasWorkerInOffice) {
      // Glow from office window
      ctx.fillStyle = 'rgba(253, 224, 71, 0.2)';
      ctx.beginPath();
      ctx.arc(winX + 7, winY + 7, 18, 0, Math.PI * 2);
      ctx.fill();

      // Silhouette of worker at desk
      ctx.fillStyle = '#1c1917';
      ctx.fillRect(winX + 3, winY + 5, 8, 7);
    }

    // Office Signboard
    ctx.fillStyle = '#1c1917';
    ctx.fillRect(officeX + 4, officeY + 4, officeW - 8, 10);
    ctx.fillStyle = '#fbbf24';
    ctx.font = '7px monospace';
    ctx.fillText('CHART ROOM', officeX + 8, officeY + 11);

    // F. Target Indicator (when player clicks deck to walk)
    if (targetIndicator) {
      ctx.strokeStyle = '#2dd4bf';
      ctx.lineWidth = 1.5;
      const targetSize = 6 + (animTick % 4);
      ctx.strokeRect(targetIndicator.x - targetSize / 2, targetIndicator.y - targetSize / 2, targetSize, targetSize);
    }
  }, [
    animTick,
    characters,
    isSteeringHelm,
    bellRinging,
    targetIndicator
  ]);

  // Click on canvas to move player or interact with objects
  const handleCanvasClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const clickX = e.clientX - rect.left;
    const clickY = e.clientY - rect.top;

    const xPercent = (clickX / rect.width) * 100;
    const yPercent = (clickY / rect.height) * 100;

    // Check click on Helm
    if (xPercent >= 45 && xPercent <= 55 && yPercent >= 16 && yPercent <= 32) {
      onHelmClick();
      return;
    }

    // Check click on Bell
    if (xPercent >= 22 && xPercent <= 30 && yPercent >= 18 && yPercent <= 30) {
      onBellClick();
      return;
    }

    // Check click on Office
    if (xPercent >= 74 && xPercent <= 92 && yPercent >= 12 && yPercent <= 42) {
      onOfficeClick();
      return;
    }

    // Check click on Cannons
    if (xPercent <= 16 && yPercent >= 40 && yPercent <= 65) {
      onCannonClick('port');
      return;
    }
    if (xPercent >= 84 && yPercent >= 40 && yPercent <= 65) {
      onCannonClick('starboard');
      return;
    }

    // Set target walk indicator
    setTargetIndicator({ x: clickX, y: clickY });
    setTimeout(() => setTargetIndicator(null), 1200);

    onDeckClick(xPercent, yPercent);
  };

  return (
    <div
      ref={containerRef}
      className="relative w-full h-full overflow-hidden select-none bg-[#0a192f] flex items-center justify-center"
      style={{ imageRendering: 'pixelated' }}
    >
      {/* HTML5 Canvas for Galleon Quarterdeck & Background Rendering */}
      <canvas
        ref={canvasRef}
        width={960}
        height={540}
        onClick={handleCanvasClick}
        className="w-full h-full max-w-full max-h-full object-contain cursor-crosshair"
      />

      {/* 5. INTERACTIVE 8-BIT CHARACTER SPRITES (HTML/SVG DOM OVERLAY FOR CRISP RESPONSIVENESS) */}
      <div className="absolute inset-0 pointer-events-none">
        {/* NPCs on Deck */}
        {characters
          .filter((c) => c.state !== 'in_office')
          .map((npc) => {
            // Smart clamp speech bubble so it never goes off-screen
            const bubbleLeftAlign = npc.x < 25;
            const bubbleRightAlign = npc.x > 75;

            return (
              <div
                key={npc.id}
                onClick={(e) => {
                  e.stopPropagation();
                  onCharacterClick(npc.id);
                }}
                className="absolute z-20 pointer-events-auto cursor-pointer transition-all duration-300"
                style={{
                  left: `${npc.x}%`,
                  top: `${npc.y}%`,
                  transform: 'translate(-50%, -50%)'
                }}
              >
                {/* Responsive Chat Bubble (Bubble Chat) */}
                {npc.activeSpeech && (
                  <div
                    className={`absolute bottom-full mb-3 z-50 animate-bounce-subtle pointer-events-none ${
                      bubbleLeftAlign
                        ? 'left-0'
                        : bubbleRightAlign
                        ? 'right-0'
                        : 'left-1/2 -translate-x-1/2'
                    } w-64 max-w-[280px] sm:max-w-xs`}
                  >
                    <div className="bg-neutral-900 border-2 border-neutral-950 p-2.5 rounded-lg shadow-[4px_4px_0px_rgba(0,0,0,0.6)] text-neutral-100 text-xs font-mono leading-relaxed relative">
                      <div className="flex items-center justify-between text-[10px] font-bold text-teal-400 mb-1 border-b border-neutral-800 pb-0.5">
                        <span>{npc.name}</span>
                        <span className="text-[8px] text-neutral-400 font-normal uppercase">{npc.role}</span>
                      </div>
                      <div className="text-neutral-200">{npc.activeSpeech}</div>
                      {/* Triangle Pointer Tail */}
                      <div
                        className={`absolute top-full w-0 h-0 border-x-6 border-x-transparent border-t-6 border-t-neutral-950 ${
                          bubbleLeftAlign
                            ? 'left-4'
                            : bubbleRightAlign
                            ? 'right-4'
                            : 'left-1/2 -translate-x-1/2'
                        }`}
                      />
                    </div>
                  </div>
                )}

                {/* 8-Bit Thought Status Indicator (💭) */}
                {npc.state === 'thinking' && (
                  <div className="absolute bottom-full mb-2 left-1/2 -translate-x-1/2 z-40 flex flex-col items-center pointer-events-none animate-pulse">
                    <div className="bg-amber-400 border-2 border-neutral-950 px-2.5 py-0.5 rounded-full text-[10px] font-mono font-bold text-neutral-950 flex items-center gap-1 shadow-[2px_2px_0px_rgba(0,0,0,0.5)]">
                      <span>💭</span>
                      <span>Thinking...</span>
                    </div>
                    <div className="w-2 h-2 rounded-full bg-amber-400 border border-neutral-950 mt-0.5" />
                    <div className="w-1 h-1 rounded-full bg-amber-400 border border-neutral-950 mt-0.5" />
                  </div>
                )}

                {/* Character 8-Bit Pixel Sprite */}
                <div className="flex flex-col items-center group">
                  {/* Nameplate */}
                  <span className="text-[9px] font-mono font-bold text-neutral-300 bg-neutral-950/85 px-1.5 py-0.5 rounded-xs mb-1 border border-neutral-800 group-hover:border-teal-400 shadow-xs whitespace-nowrap">
                    {npc.name}
                  </span>

                  {/* 8-Bit Body */}
                  <div className="relative w-8 h-10 flex flex-col items-center">
                    {/* Hat */}
                    {npc.hasHat && (
                      <div
                        className="w-7 h-3 rounded-t-xs border border-neutral-950 shadow-xs"
                        style={{ backgroundColor: npc.hatColor }}
                      >
                        {npc.hasBicorne && <div className="w-2 h-1 bg-amber-400 mx-auto -mt-0.5 rounded-xs" />}
                      </div>
                    )}

                    {/* Head / Face */}
                    <div className="w-5 h-3.5 bg-[#FFCC80] border border-neutral-950 flex items-center justify-around px-0.5">
                      <div className="w-1 h-1 bg-neutral-950 rounded-full" />
                      <div className="w-1 h-1 bg-neutral-950 rounded-full" />
                    </div>

                    {/* Coat */}
                    <div
                      className="w-6 h-4 border border-neutral-950 flex items-center justify-center relative shadow-xs"
                      style={{ backgroundColor: npc.coatColor }}
                    >
                      <div className="w-0.5 h-3 bg-amber-300" />
                    </div>

                    {/* Legs / Boots */}
                    <div className="w-4 h-2 flex justify-between">
                      <div className="w-1.5 h-2 bg-neutral-900 border border-neutral-950" />
                      <div className="w-1.5 h-2 bg-neutral-900 border border-neutral-950" />
                    </div>
                  </div>
                </div>
              </div>
            );
          })}

        {/* Player (Captain / Pirate King) */}
        <div
          className="absolute z-25 pointer-events-auto transition-all duration-100"
          style={{
            left: `${playerPos.x}%`,
            top: `${playerPos.y}%`,
            transform: 'translate(-50%, -50%)'
          }}
        >
          {/* Captain Speech Bubble */}
          {playerSpeech && (
            <div className="absolute bottom-full mb-3 left-1/2 -translate-x-1/2 w-64 max-w-[280px] sm:max-w-xs z-50 pointer-events-none animate-bounce-subtle">
              <div className="bg-teal-500 text-neutral-950 border-2 border-neutral-950 p-2.5 rounded-lg shadow-[4px_4px_0px_rgba(0,0,0,0.6)] text-xs font-mono font-semibold leading-relaxed relative">
                <div className="font-bold text-[10px] text-neutral-900 mb-0.5 uppercase tracking-wider">
                  Captain (Pirate King)
                </div>
                <div>{playerSpeech}</div>
                <div className="absolute top-full left-1/2 -translate-x-1/2 w-0 h-0 border-x-6 border-x-transparent border-t-6 border-t-neutral-950" />
              </div>
            </div>
          )}

          {/* Captain 8-Bit Pixel Sprite */}
          <div className="flex flex-col items-center">
            <span className="text-[9px] font-mono font-bold text-amber-300 bg-neutral-950/90 px-1.5 py-0.5 rounded-xs mb-1 border border-amber-500/50 shadow-xs">
              Captain
            </span>

            <div
              className={`relative w-8 h-10 flex flex-col items-center ${
                isPlayerMoving ? 'animate-bounce-short' : ''
              }`}
            >
              {/* Captain's Tricorn Hat with Gold Feather */}
              <div className="w-8 h-3 bg-[#1E293B] border border-neutral-950 rounded-t-xs relative flex items-center justify-center">
                <div className="w-2 h-1 bg-amber-400 absolute -top-1 -right-0.5 rounded-full" />
              </div>

              {/* Face & Pirate Beard */}
              <div className="w-5 h-3.5 bg-[#FFCC80] border border-neutral-950 flex flex-col items-center justify-between">
                <div className="w-full flex justify-around px-0.5 pt-0.5">
                  <div className="w-1 h-1 bg-neutral-950 rounded-full" />
                  <div className="w-1 h-1 bg-neutral-950 rounded-full" />
                </div>
                <div className="w-3 h-1 bg-[#4A2E16] rounded-xs" />
              </div>

              {/* Regal Pirate King Coat with Gold Trimming */}
              <div className="w-7 h-4 bg-[#7F1D1D] border border-neutral-950 flex items-center justify-center relative shadow-md">
                <div className="w-1 h-full bg-amber-400" />
                <div className="w-1 h-1 bg-amber-300 absolute left-1" />
                <div className="w-1 h-1 bg-amber-300 absolute right-1" />
              </div>

              {/* Boots */}
              <div className="w-5 h-2 flex justify-between">
                <div className="w-2 h-2 bg-[#1C1917] border border-neutral-950" />
                <div className="w-2 h-2 bg-[#1C1917] border border-neutral-950" />
              </div>
            </div>
          </div>
        </div>

        {/* 6. WORK STATUS INDICATOR: ACTIVE CREW IN OFFICE (CHART ROOM) */}
        {characters.some((c) => c.state === 'in_office') && (
          <div className="absolute top-[32%] right-[5%] w-52 sm:w-56 bg-neutral-900/95 border-2 border-teal-500/60 rounded-xl p-2.5 shadow-2xl backdrop-blur-md z-40 animate-fade-in pointer-events-auto">
            <div className="flex items-center justify-between text-[11px] font-mono text-teal-400 font-bold border-b border-neutral-800 pb-1 mb-1.5">
              <span className="flex items-center gap-1.5">
                <span className="w-2 h-2 rounded-full bg-teal-400 animate-ping" />
                <span>IN OFFICE: VOYAGE IN PROGRESS</span>
              </span>
            </div>

            {characters
              .filter((c) => c.state === 'in_office')
              .map((busy) => (
                <div key={busy.id} className="space-y-1.5">
                  <div className="flex items-center justify-between text-[11px]">
                    <span className="font-bold text-neutral-200">{busy.name}</span>
                    <button
                      onClick={() => onRecallFromOffice(busy.id)}
                      className="px-2 py-0.5 rounded bg-neutral-800 hover:bg-teal-500 hover:text-neutral-950 text-[10px] text-teal-300 font-mono transition-colors cursor-pointer border border-teal-500/30"
                    >
                      Recall to Deck
                    </button>
                  </div>
                  <div className="text-[10px] text-neutral-400 truncate">
                    {busy.officeTask?.title || 'Heavy Codebase Audit & CI Triage'}
                  </div>
                  <div className="w-full bg-neutral-800 rounded-full h-1.5 overflow-hidden">
                    <div className="bg-teal-400 h-full w-3/4 animate-pulse" />
                  </div>
                </div>
              ))}
          </div>
        )}

        {/* Floating Global Thinking Pill Banner */}
        {isThinkingGlobal && (
          <div className="absolute top-4 left-1/2 -translate-x-1/2 z-40 bg-neutral-900/95 border border-amber-500/60 text-amber-300 px-4 py-2 rounded-xl backdrop-blur-md shadow-2xl flex items-center gap-2 text-xs font-mono animate-pulse">
            <Brain className="w-4 h-4 text-amber-400" />
            <span>{thoughtStage}</span>
          </div>
        )}
      </div>
    </div>
  );
};

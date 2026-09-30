# Realm (Voice Mode) — Design Plan & Architecture Specification

> **Subsystem:** Fleet AI Sovereign Command Layer (`web-2`)  
> **Route / Submenu:** `Command -> Realm` (Directly below `Quarterdeck`)  
> **Document Version:** `1.0.0-phase2`  
> **Grounding:** Aligned with `docs/fundamental/01-vision-and-thesis.md` through `08-strategy-and-metrics.md`

---

## 1. Executive Summary & Narrative Grounding

In `docs/fundamental`, Fleet AI establishes a clear operating thesis:
* **Pirate King (The User):** Sovereign owner and captain of the Fleet.
* **Quartermaster:** The personal AI executive and chief of staff who understands Owner intent, coordinates Ships, receives reports, and escalates critical decisions.
* **Ships & Squads:** Persistent operational containers for specialist AI agents (Developer Delivery Ship, Marketing Launch Ship, Research Ship).
* **Crew Specialists:** Dedicated agents with bounded authorities, distinct skills, and verifiable artifact deliverables.

While the primary **Quarterdeck** provides a high-density, text-first executive command console, **Realm** is the official **Voice Mode** of the Quarterdeck. It transports the Captain directly onto the physical quarterdeck of a sovereign galleon ship rendered in an authentic **8-bit retro pixel art** aesthetic.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       COMMAND -> REALM (VOICE MODE)                         │
│                                                                             │
│  [Captain / Pirate King] ◄───(Duplex Audio / 8-bit Speech)───► [Crew / QM]  │
│             │                                                       │       │
│      (WASD Locomotion)                                         (Behaviors)  │
│             │                                                       │       │
│             ▼                                                       ▼       │
│      Walk on Wooden Deck                               - Idle / Attention   │
│      Point-and-Click Move                              - Thinking (💭 8-bit)│
│      Interact with Objects                             - Heavy Task -> Office│
│      (Helm, Map Table, Cannon, Bell)                   - Speech Bubbles     │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Core Feature Requirements

### 2.1 Scope-Adaptive Crew Deck Population
The number of characters present on the quarterdeck strictly reflects the conversational counterpart selected by the Captain:
1. **Quartermaster (1-on-1):** Only the Captain and Quartermaster occupy the deck near the captain's wheel and navigation ledger.
2. **Specialist Ship (Team):** The Navigator and all crew members assigned to that vessel spawn on the deck (e.g. Developer Ship = Horizon, Repo Analyst, Eng Planner, QA Reviewer).
3. **Specialist Squad (Squad Roster):** Focused working crew members on deck.
4. **Individual Crew Specialist (1-on-1):** Private bounded briefing between the Captain and an individual specialist (e.g. QA & Risk Reviewer).

### 2.2 Character Movement & Locomotion
* **Captain (Player Character):**
  * Keyboard: `W`, `A`, `S`, `D` or Arrow keys.
  * Mouse / Touch: Click anywhere on the deck to pathfind/move to that coordinate.
  * Touch/Gamepad: On-screen retro D-pad controller.
  * 4-directional 8-bit pixel sprites (Facing Down, Up, Left, Right) with walking step cycles.
* **Deck Boundaries:**
  * Boundaries prevent characters from walking off the ship rails into the ocean.
  * Interactive zones: Helm (Steering wheel), Chart Table, Cannon battery, Ship's Bell, Captain's Cabin / Office.

### 2.3 Conversational Partner & Voice Mode
* **Voice Input (STT):**
  * Integrated Web Speech API (`webkitSpeechRecognition` / `SpeechRecognition`) with push-to-talk, continuous hands-free voice mode, or voice-activity detection.
  * Oscilloscope retro 8-bit audio visualizer showing sound waves when user speaks.
  * Fallback quick-chat keyboard input for noisy environments.
* **Voice Output (TTS):**
  * Web Speech Synthesis (`window.speechSynthesis`) allows crew members and Quartermaster to speak back to the Captain with character-tuned pitches.
  * Retro sound effects generated dynamically using the Web Audio API (`AudioContext`): 8-bit text typewriter chimes, nautical bell clangs, wood creaks, and command confirmation pings.
* **Speech Bubbles (Bubble Chat):**
  * Pixelated dialog bubbles pop up directly above the active speaker's sprite.
  * Typewriter text animation pacing dialogue naturally.

### 2.4 Thinking State Visualization
* When the agent receives a prompt and initiates reasoning/tool-calling:
  * Character stops moving and switches to the `thinking` animation pose.
  * A retro 8-bit thinking cloud (`💭`) or animated `... / ???` appears above their head.
  * Live status indicator displays their current internal reasoning stage (e.g., *"Evaluating commit history..."*, *"Analyzing CI failure logs..."*).

### 2.5 Heavy Task Execution (Walking to the Office)
* When a crew member is assigned a long-running, compute-heavy task (e.g., full repository audit, running test suites, multi-step quest execution):
  * Character announces in bubble chat: *"Aye Captain! Off to the chart room to execute this voyage."*
  * The crew member physically walks across the deck towards the **Captain's Cabin / Specialist Office Door**.
  * The character enters the office, the cabin door swings shut, and an illuminated lantern/status badge shows `[IN OFFICE - VOYAGE IN PROGRESS]`.
  * The Captain can walk up to the office window/door to inspect current live progress, view stdout logs, or summon them back to deck.

---

## 3. Visual & Aesthetic Design Specifications

### 3.1 8-Bit Pixel Art Style Guide
* **Color Palette:**
  * Deck Oak Planks: `#6B4423`, `#8B5A2B`, `#A06836`, `#4A2E16`
  * Ocean Deep & Crest: `#0F2027`, `#203A43`, `#2C5364`, `#38BDF8`
  * Brass & Lantern Glow: `#F59E0B`, `#FBBF24`, `#FEF3C7`
  * Captain & Quartermaster Accents: Regal Gold (`#EAB308`), Sovereign Teal (`#14B8A6`), Navy (`#1E293B`)
* **Deck Elements:**
  * Quarterdeck raised stern helm with Captain's Wheel.
  * Wooden balustrade / ship railings with rope rigging.
  * Starboard and Port deck cannons.
  * Chart table with nautical scrolls, sextant, and oil lamp.
  * Animated ocean backdrop with moving pixel waves, distant islands, and swaying ship motion.
  * Cabin office door with warm lantern lighting and brass door handle.
* **Sprite Dimensions:**
  * Standard 8-bit grid: Characters rendered at 32x32 pixel matrix scaled with crisp pixel rendering (`image-rendering: pixelated`).

---

## 4. State Management & Component Hierarchy

```
RealmView (Top-level view in Command category)
├── RealmTopBar (Target selector: QM vs Ship vs Squad, Voice toggle, Mute, Help)
├── RealmCanvas (8-bit HTML5 Canvas + DOM Overlay)
│   ├── OceanBackdrop (Parallax animated ocean waves & sky)
│   ├── GalleonQuarterdeck (Deck planks, railings, helm, chart table, office door)
│   ├── InteractiveProps (Helm, Bell, Cannons, Chest, Office Cabin)
│   ├── CharacterSprites (Player + NPCs with idle/walk/think animations)
│   ├── SpeechBubbleOverlay (Pixel-art chat bubbles tethered to character coords)
│   └── AudioWaveformOverlay (Oscilloscope voice activity visualizer)
├── RealmControls (WASD keys, D-pad, Quick Action Buttons, Mic button)
└── RealmLogDrawer (Expandable transcript of all spoken dialogue and quest dispatches)
```

---

## 5. Implementation Roadmap

1. **Type Definitions & Navigation Tab**:
   * Add `'realm'` to `NavigationTab` in `src/types/index.ts`.
   * Add `'Realm'` menu entry in `src/components/layout/AppSidebar.tsx` under `COMMAND` directly below `Quarterdeck`.
2. **Audio & Speech Engine (`src/utils/audioEngine.ts`)**:
   * Synthesize 8-bit retro sound effects using Web Audio API (typewriter, bell, footsteps, thinking chime).
   * SpeechRecognition & SpeechSynthesis bindings with automatic fallback.
3. **8-Bit Quarterdeck & Character Engine (`src/components/features/realm/...`)**:
   * Canvas / SVG pixel art renderer with collision detection, deck zones, and office door.
   * Player controller with WASD, Arrow keys, point-and-click, and mobile D-pad.
   * NPC behavior state machine (`idle`, `walking`, `thinking`, `speaking`, `in_office`).
   * Speech bubble overlay with auto-dismiss and voice transcript pairing.
4. **AppShell Routing**:
   * Mount `RealmView` inside `src/components/layout/AppShell.tsx`.
5. **Compilation, Lint, & Verification**:
   * Verify TypeScript compilation, clean builds, and seamless navigation transitions.

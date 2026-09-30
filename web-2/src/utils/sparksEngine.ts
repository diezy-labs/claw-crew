// Quarterdeck Chatbox Sparks & Ambient Effects Engine

export type SparkEffectStyle = 'constellation' | 'cyber-neon' | 'sovereign-aura' | 'pirate-embers';
export type SparkIntensity = 'subtle' | 'balanced' | 'vivid';

export interface SparksConfig {
  enabled: boolean;
  style: SparkEffectStyle;
  color: string;
  intensity: SparkIntensity;
}

export const SPARK_STYLE_OPTIONS: { id: SparkEffectStyle; label: string; description: string; icon: string }[] = [
  {
    id: 'constellation',
    label: 'Constellation Sparks',
    description: 'Glittering celestial star particles floating dynamically around chatbox edges',
    icon: '✨'
  },
  {
    id: 'cyber-neon',
    label: 'Cyber Neon Pulse',
    description: 'Futuristic electric border perimeter sweep with animated high-tech gradient',
    icon: '⚡'
  },
  {
    id: 'sovereign-aura',
    label: 'Sovereign Aura',
    description: 'Smooth, deep harmonic breathing glow radiating from the toolbox boundary',
    icon: '🌊'
  },
  {
    id: 'pirate-embers',
    label: 'Pirate Cannon Embers',
    description: 'Flickering golden sparks and fiery embers drifting upward from the command deck',
    icon: '🔥'
  }
];

export const SPARK_COLOR_PRESETS = [
  { name: 'Sovereign Teal', hex: '#14b8a6' },
  { name: 'Pirate Gold', hex: '#f59e0b' },
  { name: 'Ghost Amethyst', hex: '#a855f7' },
  { name: 'Corsair Crimson', hex: '#f43f5e' },
  { name: 'Electric Cyan', hex: '#06b6d4' },
  { name: 'Emerald Sea', hex: '#10b981' }
];

export const DEFAULT_SPARKS_CONFIG: SparksConfig = {
  enabled: true,
  style: 'constellation',
  color: '#14b8a6',
  intensity: 'balanced'
};

const STORAGE_KEY = 'galleon_quarterdeck_sparks';

export function getSparksConfig(): SparksConfig {
  if (typeof localStorage === 'undefined') return DEFAULT_SPARKS_CONFIG;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      return { ...DEFAULT_SPARKS_CONFIG, ...JSON.parse(raw) };
    }
  } catch (e) {}
  return DEFAULT_SPARKS_CONFIG;
}

export function saveSparksConfig(config: SparksConfig): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
    window.dispatchEvent(new CustomEvent('galleon:sparks-updated', { detail: config }));
  } catch (e) {}
}

// Galleon Font Customization Engine

export interface FontOption {
  id: string;
  name: string;
  category: 'sans' | 'mono';
  fontFamily: string;
  description: string;
}

export const PRIMARY_FONT_OPTIONS: FontOption[] = [
  {
    id: 'plus-jakarta',
    name: 'Plus Jakarta Sans',
    category: 'sans',
    fontFamily: "'Plus Jakarta Sans', system-ui, -apple-system, sans-serif",
    description: 'Default Galleon maritime grotesk — friendly, balanced & modern'
  },
  {
    id: 'inter',
    name: 'Inter',
    category: 'sans',
    fontFamily: "'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
    description: 'Clean, neutral engineered UI font favored by developer tools'
  },
  {
    id: 'outfit',
    name: 'Outfit',
    category: 'sans',
    fontFamily: "'Outfit', -apple-system, BlinkMacSystemFont, sans-serif",
    description: 'Geometric, crisp headings and high readability'
  },
  {
    id: 'space-grotesk',
    name: 'Space Grotesk',
    category: 'sans',
    fontFamily: "'Space Grotesk', system-ui, -apple-system, sans-serif",
    description: 'Tech-forward futuristic typography with distinctive personality'
  },
  {
    id: 'fira-sans',
    name: 'Fira Sans',
    category: 'sans',
    fontFamily: "'Fira Sans', -apple-system, BlinkMacSystemFont, sans-serif",
    description: 'Humanist sans with tall x-height for clear reading'
  },
  {
    id: 'jetbrains-mono',
    name: 'JetBrains Mono (Full Monospace)',
    category: 'mono',
    fontFamily: "'JetBrains Mono', ui-monospace, monospace",
    description: 'Cyberpunk & hacker aesthetic — all UI elements in code font'
  },
  {
    id: 'system',
    name: 'System Native UI',
    category: 'sans',
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
    description: 'OS native font rendering (San Francisco / Segoe UI / Roboto)'
  }
];

export const MONO_FONT_OPTIONS: FontOption[] = [
  {
    id: 'jetbrains-mono',
    name: 'JetBrains Mono',
    category: 'mono',
    fontFamily: "'JetBrains Mono', ui-monospace, SFMono-Regular, monospace",
    description: 'Crisp code typography with tabular numerals'
  },
  {
    id: 'fira-code',
    name: 'Fira Code',
    category: 'mono',
    fontFamily: "'Fira Code', ui-monospace, SFMono-Regular, monospace",
    description: 'Iconic developer font with ligature support'
  },
  {
    id: 'source-code-pro',
    name: 'Source Code Pro',
    category: 'mono',
    fontFamily: "'Source Code Pro', ui-monospace, monospace",
    description: 'Balanced proportions designed specifically for IDE terminals'
  }
];

export interface FontSettings {
  primaryFontId: string;
  primaryFontFamily: string;
  monoFontId: string;
  monoFontFamily: string;
  fontScale: 'compact' | 'normal' | 'comfortable';
}

export const DEFAULT_FONT_SETTINGS: FontSettings = {
  primaryFontId: 'plus-jakarta',
  primaryFontFamily: PRIMARY_FONT_OPTIONS[0].fontFamily,
  monoFontId: 'jetbrains-mono',
  monoFontFamily: MONO_FONT_OPTIONS[0].fontFamily,
  fontScale: 'normal'
};

const SCALE_MAP: Record<'compact' | 'normal' | 'comfortable', string> = {
  compact: '92%',
  normal: '100%',
  comfortable: '106%'
};

/**
 * Applies font settings to DOM and CSS custom properties
 */
export function applyFontSettings(settings: FontSettings) {
  if (typeof document === 'undefined') return;

  try {
    localStorage.setItem('galleon_font_settings', JSON.stringify(settings));
  } catch (e) {}

  const root = document.documentElement;
  root.style.setProperty('--font-primary', settings.primaryFontFamily);
  root.style.setProperty('--font-mono', settings.monoFontFamily);
  root.style.fontSize = SCALE_MAP[settings.fontScale] || '100%';

  document.body.style.fontFamily = settings.primaryFontFamily;

  // Update or inject font style tag
  const styleId = 'galleon-custom-font-styles';
  let styleEl = document.getElementById(styleId) as HTMLStyleElement | null;
  if (!styleEl) {
    styleEl = document.createElement('style');
    styleEl.id = styleId;
    document.head.appendChild(styleEl);
  }

  styleEl.innerHTML = `
    body, html {
      font-family: ${settings.primaryFontFamily} !important;
    }
    code, kbd, samp, pre, .font-mono {
      font-family: ${settings.monoFontFamily} !important;
    }
  `;
}

/**
 * Loads and initializes saved font settings on app boot
 */
export function initSavedFonts(): FontSettings {
  if (typeof localStorage === 'undefined') return DEFAULT_FONT_SETTINGS;

  try {
    const saved = localStorage.getItem('galleon_font_settings');
    if (saved) {
      const parsed = JSON.parse(saved) as FontSettings;
      applyFontSettings(parsed);
      return parsed;
    }
  } catch (e) {}

  return DEFAULT_FONT_SETTINGS;
}

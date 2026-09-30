// VS Code & Custom Theme Engine for Galleon Fleet

export interface CustomThemeDefinition {
  id: string;
  name: string;
  type: 'dark' | 'light';
  source: 'preset' | 'vscode' | 'custom';
  bgCanvas: string;
  bgSurface: string;
  bgElevated: string;
  borderSubtle: string;
  textPrimary: string;
  textSecondary: string;
  textMuted: string;
  brandPrimary: string;
  accentSea: string;
}

export function hexToRgb(hex: string): string {
  let clean = hex.replace('#', '').trim();
  if (clean.length === 3) {
    clean = clean.split('').map((c) => c + c).join('');
  }
  if (clean.length >= 6) {
    const r = parseInt(clean.substring(0, 2), 16) || 0;
    const g = parseInt(clean.substring(2, 4), 16) || 0;
    const b = parseInt(clean.substring(4, 6), 16) || 0;
    return `${r}, ${g}, ${b}`;
  }
  return '13, 148, 136'; // fallback teal
}

// Built-in VS Code & Galleon Themes
export const PRESET_THEMES: CustomThemeDefinition[] = [
  {
    id: 'galleon-default',
    name: 'Galleon Sovereign Teal (Default)',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#121315',
    bgSurface: '#191b1f',
    bgElevated: '#22252a',
    borderSubtle: '#2c3036',
    textPrimary: '#f3f4f1',
    textSecondary: '#a7adb5',
    textMuted: '#747c86',
    brandPrimary: '#2dd4bf',
    accentSea: '#66c7c5'
  },
  {
    id: 'dracula',
    name: 'Dracula Official (VS Code)',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#282a36',
    bgSurface: '#21222c',
    bgElevated: '#343746',
    borderSubtle: '#44475a',
    textPrimary: '#f8f8f2',
    textSecondary: '#bd93f9',
    textMuted: '#6272a4',
    brandPrimary: '#bd93f9', // Dracula Purple
    accentSea: '#50fa7b'     // Dracula Green
  },
  {
    id: 'one-dark-pro',
    name: 'One Dark Pro (VS Code / Atom)',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#282c34',
    bgSurface: '#21252b',
    bgElevated: '#2c313a',
    borderSubtle: '#3e4451',
    textPrimary: '#abb2bf',
    textSecondary: '#e5c07b',
    textMuted: '#5c6370',
    brandPrimary: '#61afef', // One Dark Blue
    accentSea: '#98c379'     // One Dark Green
  },
  {
    id: 'tokyo-night',
    name: 'Tokyo Night Storm (VS Code)',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#24283b',
    bgSurface: '#1f2335',
    bgElevated: '#292e42',
    borderSubtle: '#3b4261',
    textPrimary: '#c0caf5',
    textSecondary: '#7aa2f7',
    textMuted: '#565f89',
    brandPrimary: '#7aa2f7', // Tokyo Blue
    accentSea: '#bb9af7'     // Tokyo Purple
  },
  {
    id: 'github-dark',
    name: 'GitHub Dark Default',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#0d1117',
    bgSurface: '#161b22',
    bgElevated: '#21262d',
    borderSubtle: '#30363d',
    textPrimary: '#c9d1d9',
    textSecondary: '#8b949e',
    textMuted: '#6e7681',
    brandPrimary: '#58a6ff', // GitHub Blue
    accentSea: '#3fb950'     // GitHub Green
  },
  {
    id: 'catppuccin-mocha',
    name: 'Catppuccin Mocha (VS Code)',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#1e1e2e',
    bgSurface: '#181825',
    bgElevated: '#313244',
    borderSubtle: '#45475a',
    textPrimary: '#cdd6f4',
    textSecondary: '#a6adc8',
    textMuted: '#6c7086',
    brandPrimary: '#cba6f7', // Mauve
    accentSea: '#89b4fa'     // Sapphire
  },
  {
    id: 'nord',
    name: 'Nord Frost (Arctic VS Code)',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#2e3440',
    bgSurface: '#3b4252',
    bgElevated: '#434c5e',
    borderSubtle: '#4c566a',
    textPrimary: '#eceff4',
    textSecondary: '#d8dee9',
    textMuted: '#e5e9f0',
    brandPrimary: '#88c0d0', // Frost Blue
    accentSea: '#81a1c1'
  },
  {
    id: 'monokai-pro',
    name: 'Monokai Pro (VS Code)',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#2d2a2e',
    bgSurface: '#221f22',
    bgElevated: '#363337',
    borderSubtle: '#49464b',
    textPrimary: '#fcfcfa',
    textSecondary: '#ffd866',
    textMuted: '#727072',
    brandPrimary: '#ffd866', // Monokai Amber/Yellow
    accentSea: '#a9dc76'     // Monokai Green
  },
  {
    id: 'solarized-dark',
    name: 'Solarized Dark',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#002b36',
    bgSurface: '#073642',
    bgElevated: '#0a4250',
    borderSubtle: '#0e5264',
    textPrimary: '#93a1a1',
    textSecondary: '#268bd2',
    textMuted: '#586e75',
    brandPrimary: '#268bd2', // Solarized Blue
    accentSea: '#2aa198'     // Cyan
  },
  {
    id: 'cyberpunk-neon',
    name: 'Cyberpunk 2077 Night City',
    type: 'dark',
    source: 'preset',
    bgCanvas: '#05070d',
    bgSurface: '#0b101d',
    bgElevated: '#131b2e',
    borderSubtle: '#1d2a45',
    textPrimary: '#e0f7fa',
    textSecondary: '#fdf500',
    textMuted: '#738a9c',
    brandPrimary: '#fcee0a', // Cyberpunk Yellow
    accentSea: '#00ff9f'     // Neon Green
  }
];

// Normalize 8-character hex (with alpha) to 6-character hex
function normalizeHex(color: string | undefined): string | null {
  if (!color || typeof color !== 'string') return null;
  const match = color.trim().match(/^#([0-9a-fA-F]{3,8})/);
  if (!match) return null;
  let hex = match[1];
  if (hex.length === 3 || hex.length === 4) {
    hex = hex.substring(0, 3).split('').map((c) => c + c).join('');
  } else if (hex.length === 8) {
    hex = hex.substring(0, 6);
  }
  return '#' + hex;
}

// Lighten/Darken hex color utility
function adjustHex(hex: string, percent: number): string {
  const clean = hex.replace('#', '');
  const num = parseInt(clean, 16);
  let r = (num >> 16) + Math.round(255 * (percent / 100));
  let g = ((num >> 8) & 0x00ff) + Math.round(255 * (percent / 100));
  let b = (num & 0x0000ff) + Math.round(255 * (percent / 100));

  r = Math.min(255, Math.max(0, r));
  g = Math.min(255, Math.max(0, g));
  b = Math.min(255, Math.max(0, b));

  return `#${((1 << 24) + (r << 16) + (g << 8) + b).toString(16).slice(1)}`;
}

/**
 * Intelligent VS Code Theme JSON Parser
 * Supports:
 * - VS Code Extension Theme JSON (with "colors" object)
 * - settings.json "workbench.colorCustomizations"
 * - Flat key-value colors object
 */
export function parseVSCodeTheme(
  jsonText: string,
  nameFallback = 'Custom VS Code Theme'
): CustomThemeDefinition | null {
  try {
    const raw = JSON.parse(jsonText);
    const colors: Record<string, string> =
      raw.colors || raw['workbench.colorCustomizations'] || raw;

    if (!colors || typeof colors !== 'object') {
      return null;
    }

    const themeName = raw.name || raw.label || nameFallback;
    const isLight = raw.type === 'light';

    // 1. Detect Background (Canvas)
    const bgCanvasRaw =
      normalizeHex(colors['editor.background']) ||
      normalizeHex(colors['terminal.background']) ||
      normalizeHex(colors['sideBar.background']) ||
      normalizeHex(colors['activityBar.background']) ||
      (isLight ? '#ffffff' : '#1e1e1e');

    // 2. Detect Surface / Sidebar
    const bgSurfaceRaw =
      normalizeHex(colors['sideBar.background']) ||
      normalizeHex(colors['activityBar.background']) ||
      normalizeHex(colors['panel.background']) ||
      normalizeHex(colors['tab.activeBackground']) ||
      adjustHex(bgCanvasRaw, isLight ? -4 : 6);

    // 3. Detect Elevated / Active Panels
    const bgElevatedRaw =
      normalizeHex(colors['activityBar.background']) ||
      normalizeHex(colors['editorGroupHeader.tabsBackground']) ||
      normalizeHex(colors['menu.background']) ||
      adjustHex(bgSurfaceRaw, isLight ? -6 : 8);

    // 4. Detect Border
    const borderRaw =
      normalizeHex(colors['sideBar.border']) ||
      normalizeHex(colors['panel.border']) ||
      normalizeHex(colors['editorGroup.border']) ||
      normalizeHex(colors['activityBar.border']) ||
      normalizeHex(colors['focusBorder']) ||
      adjustHex(bgCanvasRaw, isLight ? -14 : 14);

    // 5. Detect Text Primary & Muted
    const textPrimaryRaw =
      normalizeHex(colors['editor.foreground']) ||
      normalizeHex(colors['foreground']) ||
      (isLight ? '#1f2328' : '#e6edf3');

    const textMutedRaw =
      normalizeHex(colors['editorLineNumber.foreground']) ||
      normalizeHex(colors['descriptionForeground']) ||
      normalizeHex(colors['sideBarTitle.foreground']) ||
      adjustHex(textPrimaryRaw, isLight ? 40 : -40);

    // 6. Detect Accent / Brand Primary
    const brandPrimaryRaw =
      normalizeHex(colors['button.background']) ||
      normalizeHex(colors['focusBorder']) ||
      normalizeHex(colors['activityBarBadge.background']) ||
      normalizeHex(colors['statusBar.background']) ||
      normalizeHex(colors['textLink.foreground']) ||
      normalizeHex(colors['editorCursor.foreground']) ||
      normalizeHex(colors['terminal.ansiCyan']) ||
      (isLight ? '#0969da' : '#2dd4bf');

    const accentSeaRaw =
      normalizeHex(colors['textLink.activeForeground']) ||
      normalizeHex(colors['terminal.ansiGreen']) ||
      normalizeHex(colors['gitDecoration.addedResourceForeground']) ||
      brandPrimaryRaw;

    return {
      id: `vscode-custom-${Date.now()}`,
      name: themeName,
      type: isLight ? 'light' : 'dark',
      source: 'vscode',
      bgCanvas: bgCanvasRaw,
      bgSurface: bgSurfaceRaw,
      bgElevated: bgElevatedRaw,
      borderSubtle: borderRaw,
      textPrimary: textPrimaryRaw,
      textSecondary: textMutedRaw,
      textMuted: textMutedRaw,
      brandPrimary: brandPrimaryRaw,
      accentSea: accentSeaRaw
    };
  } catch (err) {
    return null;
  }
}

/**
 * Apply the theme definition to document.documentElement
 * Injects CSS variables and a custom style override tag
 */
export function applyTheme(theme: CustomThemeDefinition | null) {
  if (typeof document === 'undefined') return;

  const styleId = 'galleon-custom-theme-styles';
  let styleEl = document.getElementById(styleId) as HTMLStyleElement | null;

  if (!theme || theme.id === 'galleon-default') {
    // Reset to default theme
    document.documentElement.removeAttribute('data-custom-theme');
    if (styleEl && styleEl.parentNode) {
      styleEl.parentNode.removeChild(styleEl);
    }
    localStorage.removeItem('galleon_custom_theme');
    return;
  }

  // Save active theme
  localStorage.setItem('galleon_custom_theme', JSON.stringify(theme));
  document.documentElement.setAttribute('data-custom-theme', 'true');
  document.documentElement.classList.toggle('dark', theme.type === 'dark');

  const rgb = hexToRgb(theme.brandPrimary);

  // Set CSS variables directly
  const root = document.documentElement;
  root.style.setProperty('--bg-canvas', theme.bgCanvas);
  root.style.setProperty('--bg-surface', theme.bgSurface);
  root.style.setProperty('--bg-elevated', theme.bgElevated);
  root.style.setProperty('--border-subtle', theme.borderSubtle);
  root.style.setProperty('--text-primary', theme.textPrimary);
  root.style.setProperty('--text-secondary', theme.textSecondary);
  root.style.setProperty('--text-muted', theme.textMuted);
  root.style.setProperty('--brand-primary', theme.brandPrimary);
  root.style.setProperty('--brand-primary-rgb', rgb);
  root.style.setProperty('--accent-sea', theme.accentSea);

  // Inject CSS rules for high-fidelity app-wide styling
  if (!styleEl) {
    styleEl = document.createElement('style');
    styleEl.id = styleId;
    document.head.appendChild(styleEl);
  }

  styleEl.innerHTML = `
    [data-custom-theme="true"] {
      --bg-canvas: ${theme.bgCanvas} !important;
      --bg-surface: ${theme.bgSurface} !important;
      --bg-elevated: ${theme.bgElevated} !important;
      --border-subtle: ${theme.borderSubtle} !important;
      --text-primary: ${theme.textPrimary} !important;
      --text-secondary: ${theme.textSecondary} !important;
      --text-muted: ${theme.textMuted} !important;
      --brand-primary: ${theme.brandPrimary} !important;
      --brand-primary-rgb: ${rgb} !important;
      --accent-sea: ${theme.accentSea} !important;
    }
    
    [data-custom-theme="true"] body,
    [data-custom-theme="true"] .dark\\:bg-neutral-950,
    [data-custom-theme="true"] .dark\\:bg-\\[\\#0e1013\\],
    [data-custom-theme="true"] .bg-neutral-950 {
      background-color: var(--bg-canvas) !important;
    }

    [data-custom-theme="true"] .dark\\:bg-\\[\\#191b1f\\],
    [data-custom-theme="true"] .dark\\:bg-\\[\\#141619\\],
    [data-custom-theme="true"] .dark\\:bg-\\[\\#181a1d\\],
    [data-custom-theme="true"] .dark\\:bg-\\[\\#111315\\],
    [data-custom-theme="true"] .dark\\:bg-neutral-900,
    [data-custom-theme="true"] .dark\\:bg-neutral-900\\/60,
    [data-custom-theme="true"] .bg-white {
      background-color: var(--bg-surface) !important;
    }

    [data-custom-theme="true"] .dark\\:border-neutral-800,
    [data-custom-theme="true"] .dark\\:border-neutral-700,
    [data-custom-theme="true"] .border-neutral-200 {
      border-color: var(--border-subtle) !important;
    }

    [data-custom-theme="true"] .bg-teal-600,
    [data-custom-theme="true"] .dark\\:bg-teal-500 {
      background-color: var(--brand-primary) !important;
      color: ${theme.type === 'dark' ? '#000000' : '#ffffff'} !important;
    }

    [data-custom-theme="true"] .text-teal-600,
    [data-custom-theme="true"] .dark\\:text-teal-400,
    [data-custom-theme="true"] .dark\\:text-teal-300,
    [data-custom-theme="true"] .text-teal-700,
    [data-custom-theme="true"] .text-teal-500 {
      color: var(--brand-primary) !important;
    }

    [data-custom-theme="true"] .border-teal-500,
    [data-custom-theme="true"] .dark\\:border-teal-500\\/30,
    [data-custom-theme="true"] .border-teal-600 {
      border-color: var(--brand-primary) !important;
    }

    [data-custom-theme="true"] .bg-teal-500\\/10,
    [data-custom-theme="true"] .dark\\:bg-teal-500\\/20,
    [data-custom-theme="true"] .dark\\:bg-teal-500\\/15 {
      background-color: rgba(var(--brand-primary-rgb), 0.16) !important;
      color: var(--brand-primary) !important;
    }

    [data-custom-theme="true"] .ring-teal-500,
    [data-custom-theme="true"] .ring-teal-500\\/20 {
      --tw-ring-color: rgba(var(--brand-primary-rgb), 0.3) !important;
    }

    /* Active chips always have pure white typography and icons */
    [data-custom-theme="true"] .active-theme-chip,
    [data-custom-theme="true"] .active-theme-chip *,
    [data-custom-theme="true"] button.active-theme-chip span {
      color: #ffffff !important;
    }
  `;
}

/**
 * Initializes the saved theme on app load
 */
export function initSavedTheme(): CustomThemeDefinition | null {
  if (typeof localStorage === 'undefined') return null;
  try {
    const saved = localStorage.getItem('galleon_custom_theme');
    if (saved) {
      const parsed = JSON.parse(saved) as CustomThemeDefinition;
      applyTheme(parsed);
      return parsed;
    }
  } catch (e) {}
  return null;
}

/**
 * Generates a VS Code workbench.colorCustomizations JSON snippet
 * from the active theme for export back into VS Code
 */
export function exportToVSCodeJSON(theme: CustomThemeDefinition): string {
  const vsCodeConfig = {
    "workbench.colorTheme": theme.name,
    "workbench.colorCustomizations": {
      "editor.background": theme.bgCanvas,
      "sideBar.background": theme.bgSurface,
      "activityBar.background": theme.bgElevated,
      "editor.foreground": theme.textPrimary,
      "editorLineNumber.foreground": theme.textMuted,
      "button.background": theme.brandPrimary,
      "focusBorder": theme.brandPrimary,
      "statusBar.background": theme.bgSurface,
      "sideBar.border": theme.borderSubtle,
      "panel.border": theme.borderSubtle
    }
  };
  return JSON.stringify(vsCodeConfig, null, 2);
}

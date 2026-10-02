import React, { useState, useEffect, useId } from 'react';
import {
  Palette,
  Code2,
  Upload,
  Check,
  RotateCcw,
  Copy,
  Sparkles,
  Sliders,
  CheckCircle2,
  FileCode,
  AlertCircle,
  Type,
  Baseline
} from 'lucide-react';
import {
  CustomThemeDefinition,
  PRESET_THEMES,
  parseVSCodeTheme,
  applyTheme,
  initSavedTheme,
  exportToVSCodeJSON
} from '../../utils/themeEngine';
import {
  PRIMARY_FONT_OPTIONS,
  MONO_FONT_OPTIONS,
  FontSettings,
  DEFAULT_FONT_SETTINGS,
  applyFontSettings,
  initSavedFonts
} from '../../utils/fontEngine';
import { ToolButton } from '../common/ToolButton';
import { HeaderToolbar } from '../common/HeaderToolbar';
import {
  SparksConfig,
  SPARK_STYLE_OPTIONS,
  SPARK_COLOR_PRESETS,
  DEFAULT_SPARKS_CONFIG,
  getSparksConfig,
  saveSparksConfig
} from '../../utils/sparksEngine';
import { ChatboxSparksEffect } from '../common/ChatboxSparksEffect';

const QUICK_ACCENTS = [
  { name: 'Sovereign Teal', hex: '#14b8a6' },
  { name: 'Emerald Sea', hex: '#10b981' },
  { name: 'Abyssal Sapphire', hex: '#3b82f6' },
  { name: 'Pirate Gold', hex: '#f59e0b' },
  { name: 'Ghost Amethyst', hex: '#8b5cf6' },
  { name: 'Corsair Crimson', hex: '#f43f5e' },
  { name: 'Electric Cyan', hex: '#06b6d4' },
  { name: 'Sunset Orange', hex: '#f97316' }
];

const SAMPLE_VSCODE_SNIPPETS: Record<string, string> = {
  dracula: JSON.stringify(
    {
      name: "Dracula Official",
      type: "dark",
      colors: {
        "editor.background": "#282a36",
        "sideBar.background": "#21222c",
        "activityBar.background": "#343746",
        "editor.foreground": "#f8f8f2",
        "button.background": "#bd93f9",
        "focusBorder": "#bd93f9",
        "statusBar.background": "#191a21",
        "sideBar.border": "#44475a"
      }
    },
    null,
    2
  ),
  tokyo: JSON.stringify(
    {
      name: "Tokyo Night",
      type: "dark",
      colors: {
        "editor.background": "#24283b",
        "sideBar.background": "#1f2335",
        "activityBar.background": "#292e42",
        "editor.foreground": "#c0caf5",
        "button.background": "#7aa2f7",
        "focusBorder": "#7aa2f7",
        "statusBar.background": "#1f2335",
        "sideBar.border": "#3b4261"
      }
    },
    null,
    2
  ),
  onedark: JSON.stringify(
    {
      "workbench.colorTheme": "One Dark Pro",
      "workbench.colorCustomizations": {
        "editor.background": "#282c34",
        "sideBar.background": "#21252b",
        "activityBar.background": "#2c313a",
        "editor.foreground": "#abb2bf",
        "button.background": "#61afef",
        "focusBorder": "#61afef",
        "sideBar.border": "#3e4451"
      }
    },
    null,
    2
  )
};

export const ThemeCustomizer: React.FC = () => {
  const [activeTheme, setActiveTheme] = useState<CustomThemeDefinition>(() => {
    return initSavedTheme() || PRESET_THEMES[0];
  });

  const [activeTab, setActiveTab] = useState<'presets' | 'custom' | 'vscode' | 'fonts' | 'sparks'>('presets');
  const [fontSettings, setFontSettings] = useState<FontSettings>(() => initSavedFonts());
  const [sparksConfig, setSparksConfig] = useState<SparksConfig>(() => getSparksConfig());
  const [vsCodeJsonInput, setVsCodeJsonInput] = useState('');
  const [parsedPreview, setParsedPreview] = useState<CustomThemeDefinition | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);
  const [copiedToast, setCopiedToast] = useState(false);
  const [applySuccessToast, setApplySuccessToast] = useState(false);
  const fileInputId = useId();

  useEffect(() => {
    const loaded = initSavedTheme();
    if (loaded) {
      setActiveTheme(loaded);
    }
    const loadedFonts = initSavedFonts();
    if (loadedFonts) {
      setFontSettings(loadedFonts);
    }
    setSparksConfig(getSparksConfig());
  }, []);

  // Handle parsing user input in real-time
  useEffect(() => {
    if (!vsCodeJsonInput.trim()) {
      setParsedPreview(null);
      setParseError(null);
      return;
    }
    const parsed = parseVSCodeTheme(vsCodeJsonInput);
    if (parsed) {
      setParsedPreview(parsed);
      setParseError(null);
    } else {
      setParsedPreview(null);
      setParseError('Could not parse VS Code theme JSON. Ensure it contains a valid "colors" or "workbench.colorCustomizations" block.');
    }
  }, [vsCodeJsonInput]);

  const handleSelectPreset = (preset: CustomThemeDefinition) => {
    setActiveTheme(preset);
    applyTheme(preset);
    showSuccessToast();
  };

  const handleApplyImportedVSCode = () => {
    if (!parsedPreview) return;
    setActiveTheme(parsedPreview);
    applyTheme(parsedPreview);
    showSuccessToast();
  };

  const handleCustomColorChange = (key: keyof CustomThemeDefinition, value: string) => {
    const updated: CustomThemeDefinition = {
      ...activeTheme,
      id: `custom-${Date.now()}`,
      name: 'Custom User Theme',
      source: 'custom',
      [key]: value
    };
    if (key === 'brandPrimary') {
      updated.accentSea = value;
    }
    setActiveTheme(updated);
    applyTheme(updated);
  };

  const handleFontChange = (patch: Partial<FontSettings>) => {
    const updated: FontSettings = { ...fontSettings, ...patch };
    setFontSettings(updated);
    applyFontSettings(updated);
    showSuccessToast();
  };

  const handleSparksChange = (patch: Partial<SparksConfig>) => {
    const updated: SparksConfig = { ...sparksConfig, ...patch };
    setSparksConfig(updated);
    saveSparksConfig(updated);
    showSuccessToast();
  };

  const handleResetDefaults = () => {
    const defaultTheme = PRESET_THEMES[0];
    setActiveTheme(defaultTheme);
    applyTheme(null);
    setFontSettings(DEFAULT_FONT_SETTINGS);
    applyFontSettings(DEFAULT_FONT_SETTINGS);
    setSparksConfig(DEFAULT_SPARKS_CONFIG);
    saveSparksConfig(DEFAULT_SPARKS_CONFIG);
    showSuccessToast();
  };

  const handleCopyVSCodeConfig = () => {
    const json = exportToVSCodeJSON(activeTheme);
    navigator.clipboard.writeText(json);
    setCopiedToast(true);
    setTimeout(() => setCopiedToast(false), 2500);
  };

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (event) => {
      const content = event.target?.result as string;
      if (content) {
        setVsCodeJsonInput(content);
        setActiveTab('vscode');
      }
    };
    reader.readAsText(file);
  };

  const showSuccessToast = () => {
    setApplySuccessToast(true);
    setTimeout(() => setApplySuccessToast(false), 2500);
  };

  return (
    <div className="space-y-6">
      {/* Sub-header Navigation Tabs with Responsive Wrapping */}
      <HeaderToolbar
        leftContent={
          <div className="flex items-center gap-1 p-1 rounded-xl bg-neutral-100 dark:bg-neutral-900 border border-neutral-200/80 dark:border-neutral-800/80 text-xs shrink-0 max-w-full overflow-x-auto scrollbar-none">
            <button
              type="button"
              onClick={() => setActiveTab('presets')}
              className={`px-3 py-1.5 rounded-lg font-semibold transition-all cursor-pointer flex items-center gap-1.5 shrink-0 whitespace-nowrap ${
                activeTab === 'presets'
                  ? 'bg-white dark:bg-[#191b1f] text-teal-600 dark:text-teal-400 shadow-xs'
                  : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
              }`}
            >
              <Palette className="w-3.5 h-3.5" />
              <span>Theme Presets</span>
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('vscode')}
              className={`px-3 py-1.5 rounded-lg font-semibold transition-all cursor-pointer flex items-center gap-1.5 shrink-0 whitespace-nowrap ${
                activeTab === 'vscode'
                  ? 'bg-white dark:bg-[#191b1f] text-teal-600 dark:text-teal-400 shadow-xs'
                  : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
              }`}
            >
              <Code2 className="w-3.5 h-3.5" />
              <span>VS Code Theme Importer</span>
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('custom')}
              className={`px-3 py-1.5 rounded-lg font-semibold transition-all cursor-pointer flex items-center gap-1.5 shrink-0 whitespace-nowrap ${
                activeTab === 'custom'
                  ? 'bg-white dark:bg-[#191b1f] text-teal-600 dark:text-teal-400 shadow-xs'
                  : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
              }`}
            >
              <Sliders className="w-3.5 h-3.5" />
              <span>Custom Color Studio</span>
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('fonts')}
              className={`px-3 py-1.5 rounded-lg font-semibold transition-all cursor-pointer flex items-center gap-1.5 shrink-0 whitespace-nowrap ${
                activeTab === 'fonts'
                  ? 'bg-white dark:bg-[#191b1f] text-teal-600 dark:text-teal-400 shadow-xs'
                  : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
              }`}
            >
              <Type className="w-3.5 h-3.5" />
              <span>Typography &amp; Fonts</span>
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('sparks')}
              className={`px-3 py-1.5 rounded-lg font-semibold transition-all cursor-pointer flex items-center gap-1.5 shrink-0 whitespace-nowrap ${
                activeTab === 'sparks'
                  ? 'bg-white dark:bg-[#191b1f] text-teal-600 dark:text-teal-400 shadow-xs'
                  : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
              }`}
            >
              <Sparkles className="w-3.5 h-3.5" />
              <span>Deck Sparks &amp; Effects</span>
            </button>
          </div>
        }
        rightContent={
          <div className="flex items-center gap-1.5 shrink-0">
            <ToolButton
              icon={copiedToast ? <Check className="w-3.5 h-3.5 text-emerald-500" /> : <Copy className="w-3.5 h-3.5" />}
              label={copiedToast ? 'Copied!' : 'Export to VS Code'}
              shortLabel={copiedToast ? 'Copied' : 'Export'}
              onClick={handleCopyVSCodeConfig}
              title="Copy active color scheme as VS Code settings JSON"
            />
            <ToolButton
              icon={<RotateCcw className="w-3.5 h-3.5" />}
              label="Reset Defaults"
              shortLabel="Reset"
              onClick={handleResetDefaults}
              title="Reset to Galleon Sovereign Teal & typography defaults"
            />
          </div>
        }
      />

      {/* Success Notification Banner */}
      {applySuccessToast && (
        <div className="p-3 rounded-lg bg-teal-500/10 border border-teal-500/30 text-teal-600 dark:text-teal-400 text-xs font-semibold flex items-center justify-between animate-view-fade-in">
          <div className="flex items-center gap-2">
            <CheckCircle2 className="w-4 h-4 text-teal-500 shrink-0" />
            <span>Theme applied successfully! Color tokens updated across all views.</span>
          </div>
          <span className="text-[11px] font-mono text-neutral-400">Active: {activeTheme.name}</span>
        </div>
      )}

      {/* TAB 1: PRESET THEMES GALLERY */}
      {activeTab === 'presets' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                Popular Developer &amp; VS Code Themes
              </h3>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                Click any theme to apply its exact palette, contrast curves, and syntax accent colors.
              </p>
            </div>
            <span className="text-[11px] font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10 shrink-0">
              {PRESET_THEMES.length} Presets Available
            </span>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {PRESET_THEMES.map((theme) => {
              const isSelected = activeTheme.id === theme.id || activeTheme.name === theme.name;
              return (
                <div
                  key={theme.id}
                  onClick={() => handleSelectPreset(theme)}
                  className={`p-4 rounded-xl border transition-all cursor-pointer relative overflow-hidden group ${
                    isSelected
                      ? 'border-teal-500 ring-2 ring-teal-500/20 shadow-md bg-neutral-50/80 dark:bg-neutral-900/80'
                      : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-neutral-300 dark:hover:border-neutral-700 hover:shadow-xs'
                  }`}
                  style={{
                    backgroundColor: theme.bgSurface,
                    borderColor: isSelected ? theme.brandPrimary : theme.borderSubtle
                  }}
                >
                  <div className="flex items-center justify-between mb-3">
                    <span
                      className="font-bold text-xs truncate max-w-[170px]"
                      style={{ color: theme.textPrimary }}
                    >
                      {theme.name}
                    </span>
                    {isSelected ? (
                      <span
                        className="text-[10px] font-mono font-bold px-2 py-0.5 rounded-full flex items-center gap-1 shadow-2xs"
                        style={{
                          backgroundColor: theme.brandPrimary,
                          color: theme.type === 'dark' ? '#000000' : '#ffffff'
                        }}
                      >
                        <Check className="w-3 h-3" />
                        Active
                      </span>
                    ) : (
                      <span
                        className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-black/20 text-neutral-400"
                        style={{ color: theme.textMuted }}
                      >
                        VS Code
                      </span>
                    )}
                  </div>

                  {/* Swatches strip */}
                  <div className="space-y-1.5">
                    <div className="flex h-5 rounded-md overflow-hidden border border-black/20">
                      <div className="flex-1" style={{ backgroundColor: theme.bgCanvas }} title="Canvas Background" />
                      <div className="flex-1" style={{ backgroundColor: theme.bgSurface }} title="Card Surface" />
                      <div className="flex-1" style={{ backgroundColor: theme.borderSubtle }} title="Border Color" />
                      <div className="flex-1" style={{ backgroundColor: theme.brandPrimary }} title="Primary Accent" />
                      <div className="flex-1" style={{ backgroundColor: theme.accentSea }} title="Secondary Accent" />
                    </div>

                    <div className="flex items-center justify-between text-[10px] font-mono pt-1" style={{ color: theme.textMuted }}>
                      <span>Accent: <strong style={{ color: theme.brandPrimary }}>{theme.brandPrimary}</strong></span>
                      <span>Canvas: {theme.bgCanvas}</span>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* TAB 2: VS CODE THEME JSON IMPORTER */}
      {activeTab === 'vscode' && (
        <div className="space-y-5">
          <div className="p-4 rounded-xl border border-teal-500/20 bg-teal-500/5 dark:bg-teal-950/20 text-xs space-y-2">
            <div className="flex items-center gap-2 font-bold text-teal-800 dark:text-teal-300">
              <Sparkles className="w-4 h-4 text-teal-500" />
              <span>Direct VS Code Theme Parser</span>
            </div>
            <p className="text-neutral-600 dark:text-neutral-400 leading-relaxed">
              Export your VS Code theme or copy your <code className="px-1.5 py-0.5 rounded bg-neutral-200 dark:bg-neutral-800 font-mono text-teal-600 dark:text-teal-300">workbench.colorCustomizations</code> from <code className="px-1.5 py-0.5 rounded bg-neutral-200 dark:bg-neutral-800 font-mono">settings.json</code>.
              Galleon Fleet will intelligently extract all UI tokens, including backgrounds, surface elevation, accents, borders, and foreground typography.
            </p>
          </div>

          {/* Quick Sample Buttons */}
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span className="text-neutral-500 dark:text-neutral-400 font-medium">Quick Samples:</span>
            <button
              type="button"
              onClick={() => setVsCodeJsonInput(SAMPLE_VSCODE_SNIPPETS.dracula)}
              className="px-2.5 py-1 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:bg-neutral-100 dark:hover:bg-neutral-800 font-mono text-[11px] cursor-pointer"
            >
              Dracula Sample
            </button>
            <button
              type="button"
              onClick={() => setVsCodeJsonInput(SAMPLE_VSCODE_SNIPPETS.tokyo)}
              className="px-2.5 py-1 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:bg-neutral-100 dark:hover:bg-neutral-800 font-mono text-[11px] cursor-pointer"
            >
              Tokyo Night Sample
            </button>
            <button
              type="button"
              onClick={() => setVsCodeJsonInput(SAMPLE_VSCODE_SNIPPETS.onedark)}
              className="px-2.5 py-1 rounded-lg border border-neutral-200 dark:border-neutral-800 hover:bg-neutral-100 dark:hover:bg-neutral-800 font-mono text-[11px] cursor-pointer"
            >
              One Dark Pro Sample
            </button>

            <div className="ml-auto">
              <label
                htmlFor={fileInputId}
                className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-neutral-200 dark:bg-neutral-800 hover:bg-neutral-300 dark:hover:bg-neutral-700 text-neutral-800 dark:text-neutral-200 font-medium text-xs cursor-pointer transition-colors"
              >
                <Upload className="w-3.5 h-3.5" />
                <span>Upload .json file</span>
                <input
                  id={fileInputId}
                  type="file"
                  accept=".json,application/json"
                  onChange={handleFileUpload}
                  className="hidden"
                />
              </label>
            </div>
          </div>

          {/* JSON Textarea */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between text-xs">
              <label className="font-semibold text-neutral-800 dark:text-neutral-200 flex items-center gap-1.5">
                <FileCode className="w-3.5 h-3.5 text-neutral-500" />
                <span>Paste VS Code Theme JSON</span>
              </label>
              <span className="font-mono text-[11px] text-neutral-400">JSON Format</span>
            </div>
            <textarea
              rows={9}
              value={vsCodeJsonInput}
              onChange={(e) => setVsCodeJsonInput(e.target.value)}
              placeholder={`Paste VS Code theme JSON or settings snippet here...\n\nExample:\n{\n  "workbench.colorCustomizations": {\n    "editor.background": "#1e1e2e",\n    "sideBar.background": "#181825",\n    "editor.foreground": "#cdd6f4",\n    "button.background": "#cba6f7"\n  }\n}`}
              className="w-full p-3 font-mono text-xs rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-teal-500 resize-y"
            />
          </div>

          {/* Parser Error State */}
          {parseError && (
            <div className="p-3 rounded-lg bg-rose-500/10 border border-rose-500/30 text-rose-600 dark:text-rose-400 text-xs flex items-center gap-2">
              <AlertCircle className="w-4 h-4 shrink-0" />
              <span>{parseError}</span>
            </div>
          )}

          {/* Parsed Preview Card */}
          {parsedPreview && (
            <div className="p-4 rounded-xl border border-teal-500/40 bg-white dark:bg-[#191b1f] shadow-sm space-y-4 animate-view-fade-in">
              <div className="flex items-center justify-between">
                <div>
                  <span className="text-[10px] font-mono text-teal-600 dark:text-teal-400 uppercase tracking-wider block">
                    Detected Valid VS Code Theme
                  </span>
                  <h4 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                    {parsedPreview.name}
                  </h4>
                </div>

                <button
                  type="button"
                  onClick={handleApplyImportedVSCode}
                  className="px-4 py-2 rounded-lg bg-teal-600 dark:bg-teal-500 text-white dark:text-neutral-950 font-bold text-xs hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer flex items-center gap-1.5 shadow-xs"
                >
                  <Check className="w-3.5 h-3.5" />
                  <span>Apply This VS Code Theme</span>
                </button>
              </div>

              {/* Color swatches breakdown */}
              <div className="grid grid-cols-2 sm:grid-cols-5 gap-2 text-xs font-mono">
                <div className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 space-y-1.5" style={{ backgroundColor: parsedPreview.bgCanvas }}>
                  <span className="text-[10px] block" style={{ color: parsedPreview.textMuted }}>Canvas</span>
                  <span className="font-bold block" style={{ color: parsedPreview.textPrimary }}>{parsedPreview.bgCanvas}</span>
                </div>
                <div className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 space-y-1.5" style={{ backgroundColor: parsedPreview.bgSurface }}>
                  <span className="text-[10px] block" style={{ color: parsedPreview.textMuted }}>Surface</span>
                  <span className="font-bold block" style={{ color: parsedPreview.textPrimary }}>{parsedPreview.bgSurface}</span>
                </div>
                <div className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 space-y-1.5" style={{ backgroundColor: parsedPreview.bgElevated }}>
                  <span className="text-[10px] block" style={{ color: parsedPreview.textMuted }}>Elevated</span>
                  <span className="font-bold block" style={{ color: parsedPreview.textPrimary }}>{parsedPreview.bgElevated}</span>
                </div>
                <div className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 space-y-1.5" style={{ backgroundColor: parsedPreview.bgSurface }}>
                  <span className="text-[10px] block" style={{ color: parsedPreview.textMuted }}>Accent</span>
                  <span className="font-bold block" style={{ color: parsedPreview.brandPrimary }}>{parsedPreview.brandPrimary}</span>
                </div>
                <div className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 space-y-1.5" style={{ backgroundColor: parsedPreview.bgSurface }}>
                  <span className="text-[10px] block" style={{ color: parsedPreview.textMuted }}>Border</span>
                  <span className="font-bold block" style={{ color: parsedPreview.textPrimary }}>{parsedPreview.borderSubtle}</span>
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* TAB 3: CUSTOM COLOR STUDIO */}
      {activeTab === 'custom' && (
        <div className="space-y-6">
          {/* Quick Accent Selector */}
          <div>
            <label className="block font-semibold text-neutral-800 dark:text-neutral-200 mb-2 text-xs">
              Quick Brand Accent Swatches
            </label>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2.5">
              {QUICK_ACCENTS.map((acc) => {
                const isSelected = activeTheme.brandPrimary.toLowerCase() === acc.hex.toLowerCase();
                return (
                  <button
                    key={acc.hex}
                    type="button"
                    onClick={() => handleCustomColorChange('brandPrimary', acc.hex)}
                    className={`p-2.5 rounded-xl border flex items-center gap-2.5 transition-all cursor-pointer text-xs ${
                      isSelected
                        ? 'border-neutral-900 dark:border-white shadow-xs bg-neutral-100 dark:bg-neutral-800 font-bold'
                        : 'border-neutral-200 dark:border-neutral-800 hover:border-neutral-300 dark:hover:border-neutral-700 bg-white dark:bg-[#191b1f]'
                    }`}
                  >
                    <span
                      className="w-4 h-4 rounded-full shrink-0 border border-black/20 shadow-2xs"
                      style={{ backgroundColor: acc.hex }}
                    />
                    <span className="truncate">{acc.name}</span>
                    {isSelected && <Check className="w-3.5 h-3.5 ml-auto text-neutral-900 dark:text-white" />}
                  </button>
                );
              })}
            </div>
          </div>

          {/* Granular Color Controls */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            {/* Primary Accent Color */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100 block">
                    Brand Accent Color
                  </span>
                  <span className="text-[11px] text-neutral-500">
                    Buttons, highlights, active tabs &amp; icons
                  </span>
                </div>
                <div
                  className="w-7 h-7 rounded-lg border border-black/20 shadow-xs"
                  style={{ backgroundColor: activeTheme.brandPrimary }}
                />
              </div>

              <div className="flex items-center gap-2">
                <input
                  type="color"
                  value={activeTheme.brandPrimary}
                  onChange={(e) => handleCustomColorChange('brandPrimary', e.target.value)}
                  className="w-8 h-8 rounded border border-neutral-300 dark:border-neutral-700 cursor-pointer p-0 bg-transparent"
                />
                <input
                  type="text"
                  value={activeTheme.brandPrimary}
                  onChange={(e) => handleCustomColorChange('brandPrimary', e.target.value)}
                  className="flex-1 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 font-mono text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
                />
              </div>
            </div>

            {/* Canvas Background Color */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100 block">
                    Canvas Background
                  </span>
                  <span className="text-[11px] text-neutral-500">
                    Deep viewport &amp; main application backing
                  </span>
                </div>
                <div
                  className="w-7 h-7 rounded-lg border border-black/20 shadow-xs"
                  style={{ backgroundColor: activeTheme.bgCanvas }}
                />
              </div>

              <div className="flex items-center gap-2">
                <input
                  type="color"
                  value={activeTheme.bgCanvas}
                  onChange={(e) => handleCustomColorChange('bgCanvas', e.target.value)}
                  className="w-8 h-8 rounded border border-neutral-300 dark:border-neutral-700 cursor-pointer p-0 bg-transparent"
                />
                <input
                  type="text"
                  value={activeTheme.bgCanvas}
                  onChange={(e) => handleCustomColorChange('bgCanvas', e.target.value)}
                  className="flex-1 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 font-mono text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
                />
              </div>
            </div>

            {/* Surface Card Background Color */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100 block">
                    Surface / Card Background
                  </span>
                  <span className="text-[11px] text-neutral-500">
                    Containers, modallayers, cards &amp; sidebar
                  </span>
                </div>
                <div
                  className="w-7 h-7 rounded-lg border border-black/20 shadow-xs"
                  style={{ backgroundColor: activeTheme.bgSurface }}
                />
              </div>

              <div className="flex items-center gap-2">
                <input
                  type="color"
                  value={activeTheme.bgSurface}
                  onChange={(e) => handleCustomColorChange('bgSurface', e.target.value)}
                  className="w-8 h-8 rounded border border-neutral-300 dark:border-neutral-700 cursor-pointer p-0 bg-transparent"
                />
                <input
                  type="text"
                  value={activeTheme.bgSurface}
                  onChange={(e) => handleCustomColorChange('bgSurface', e.target.value)}
                  className="flex-1 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 font-mono text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
                />
              </div>
            </div>

            {/* Border Subtle Color */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100 block">
                    Border &amp; Divider Color
                  </span>
                  <span className="text-[11px] text-neutral-500">
                    Card lines, dividers &amp; subheaders
                  </span>
                </div>
                <div
                  className="w-7 h-7 rounded-lg border border-black/20 shadow-xs"
                  style={{ backgroundColor: activeTheme.borderSubtle }}
                />
              </div>

              <div className="flex items-center gap-2">
                <input
                  type="color"
                  value={activeTheme.borderSubtle}
                  onChange={(e) => handleCustomColorChange('borderSubtle', e.target.value)}
                  className="w-8 h-8 rounded border border-neutral-300 dark:border-neutral-700 cursor-pointer p-0 bg-transparent"
                />
                <input
                  type="text"
                  value={activeTheme.borderSubtle}
                  onChange={(e) => handleCustomColorChange('borderSubtle', e.target.value)}
                  className="flex-1 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 font-mono text-xs text-neutral-900 dark:text-neutral-100 focus:outline-none"
                />
              </div>
            </div>
          </div>
        </div>
      )}

      {/* TAB 5: QUARTERDECK CHATBOX SPARKS & AMBIENT EFFECTS */}
      {activeTab === 'sparks' && (
        <div className="space-y-6 animate-view-fade-in">
          {/* Section 1: Master ON/OFF Switch */}
          <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] flex items-center justify-between gap-4">
            <div>
              <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                <Sparkles className="w-4 h-4 text-teal-500" />
                <span>Quarterdeck Chatbox Sparks &amp; Ambient Glow</span>
              </h3>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                Enable or disable dynamic glowing aura and particle sparkles around the Sovereign Quarterdeck command bar.
              </p>
            </div>

            <button
              type="button"
              onClick={() => handleSparksChange({ enabled: !sparksConfig.enabled })}
              className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none ${
                sparksConfig.enabled ? 'bg-teal-500' : 'bg-neutral-300 dark:bg-neutral-700'
              }`}
            >
              <span
                className={`pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out ${
                  sparksConfig.enabled ? 'translate-x-5' : 'translate-x-0'
                }`}
              />
            </button>
          </div>

          {/* Section 2: Effect Style Selection Cards */}
          <div className="space-y-3">
            <div>
              <h4 className="text-xs font-bold text-neutral-900 dark:text-neutral-100 uppercase tracking-wider">
                Visual Effect Style
              </h4>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                Choose the animation behavior and particle formation surrounding the toolbox.
              </p>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              {SPARK_STYLE_OPTIONS.map((styleOpt) => {
                const isSelected = sparksConfig.style === styleOpt.id;
                return (
                  <div
                    key={styleOpt.id}
                    onClick={() => handleSparksChange({ style: styleOpt.id })}
                    className={`p-3.5 rounded-xl border transition-all cursor-pointer relative ${
                      isSelected
                        ? 'border-teal-500 ring-2 ring-teal-500/20 bg-neutral-50 dark:bg-neutral-900/90 shadow-sm'
                        : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-neutral-300 dark:hover:border-neutral-700'
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1.5">
                      <div className="flex items-center gap-2">
                        <span className="text-lg">{styleOpt.icon}</span>
                        <span className="font-bold text-xs text-neutral-900 dark:text-neutral-100">
                          {styleOpt.label}
                        </span>
                      </div>
                      {isSelected && (
                        <span className="text-[10px] font-mono font-bold px-2 py-0.5 rounded-full bg-teal-500 text-neutral-950 flex items-center gap-1">
                          <Check className="w-3 h-3" />
                          Selected
                        </span>
                      )}
                    </div>
                    <p className="text-[11px] text-neutral-500 line-clamp-2">
                      {styleOpt.description}
                    </p>
                  </div>
                );
              })}
            </div>
          </div>

          {/* Section 3: Color Customizer */}
          <div className="space-y-3">
            <div>
              <h4 className="text-xs font-bold text-neutral-900 dark:text-neutral-100 uppercase tracking-wider">
                Effect Accent Color
              </h4>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                Select from maritime color presets or choose a custom hex color for particles and glow.
              </p>
            </div>

            <div className="flex flex-wrap items-center gap-2.5">
              {SPARK_COLOR_PRESETS.map((colorPreset) => {
                const isSelected = sparksConfig.color.toLowerCase() === colorPreset.hex.toLowerCase();
                return (
                  <button
                    key={colorPreset.hex}
                    type="button"
                    onClick={() => handleSparksChange({ color: colorPreset.hex })}
                    className={`px-3 py-1.5 rounded-xl border text-xs font-medium flex items-center gap-2 transition-all cursor-pointer ${
                      isSelected
                        ? 'border-teal-500 bg-neutral-100 dark:bg-neutral-800 font-bold shadow-xs'
                        : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-neutral-300'
                    }`}
                  >
                    <span
                      className="w-3.5 h-3.5 rounded-full shrink-0 shadow-2xs border border-black/20"
                      style={{ backgroundColor: colorPreset.hex }}
                    />
                    <span>{colorPreset.name}</span>
                    {isSelected && <Check className="w-3 h-3 text-teal-500" />}
                  </button>
                );
              })}

              {/* Custom Color Input */}
              <div className="flex items-center gap-1.5 px-2 py-1 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f]">
                <input
                  type="color"
                  value={sparksConfig.color}
                  onChange={(e) => handleSparksChange({ color: e.target.value })}
                  className="w-6 h-6 rounded cursor-pointer p-0 bg-transparent border-0"
                />
                <input
                  type="text"
                  value={sparksConfig.color}
                  onChange={(e) => handleSparksChange({ color: e.target.value })}
                  className="w-20 font-mono text-xs bg-transparent text-neutral-900 dark:text-neutral-100 focus:outline-none"
                />
              </div>
            </div>
          </div>

          {/* Section 4: Intensity Scaling */}
          <div className="space-y-3">
            <div>
              <h4 className="text-xs font-bold text-neutral-900 dark:text-neutral-100 uppercase tracking-wider">
                Effect Glow &amp; Particle Intensity
              </h4>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                Control particle opacity, halo brightness, and glow visibility.
              </p>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              {[
                { id: 'subtle', label: 'Subtle', desc: 'Discreet ambient accent, low particle distraction' },
                { id: 'balanced', label: 'Balanced', desc: 'Harmonious glowing boundary with steady pulses' },
                { id: 'vivid', label: 'Vivid Cyber', desc: 'Bright luminous halo with high particle impact' }
              ].map((lvl) => {
                const isSelected = sparksConfig.intensity === lvl.id;
                return (
                  <button
                    key={lvl.id}
                    type="button"
                    onClick={() => handleSparksChange({ intensity: lvl.id as any })}
                    className={`p-3 rounded-xl border text-left transition-all cursor-pointer ${
                      isSelected
                        ? 'border-teal-500 bg-teal-500/10 text-teal-700 dark:text-teal-300 font-semibold shadow-xs'
                        : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-neutral-300'
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1">
                      <span className="text-xs font-bold">{lvl.label}</span>
                      {isSelected && <Check className="w-3.5 h-3.5 text-teal-500" />}
                    </div>
                    <p className="text-[11px] text-neutral-500">{lvl.desc}</p>
                  </button>
                );
              })}
            </div>
          </div>

          {/* Section 5: Interactive Live Preview Sandbox */}
          <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-900 space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-[10px] font-mono text-neutral-400 uppercase tracking-wider">
                Live Quarterdeck Toolbox Preview
              </span>
              <span className="text-[10px] font-mono text-teal-400">
                {sparksConfig.enabled ? `${sparksConfig.style.toUpperCase()} ACTIVE` : 'EFFECTS DISABLED'}
              </span>
            </div>

            <ChatboxSparksEffect config={sparksConfig}>
              <div className="relative">
                <input
                  type="text"
                  readOnly
                  value="Quartermaster, prepare fleet release packages for deployment..."
                  className="w-full bg-neutral-950 border border-neutral-700 rounded-xl px-4 py-2.5 text-xs text-neutral-100 placeholder:text-neutral-500 font-mono shadow-inner cursor-default"
                />
                <span
                  className="absolute right-3 top-2.5 text-[10px] font-mono px-2 py-0.5 rounded font-bold"
                  style={{
                    backgroundColor: `${sparksConfig.color}25`,
                    color: sparksConfig.color
                  }}
                >
                  DECK PRIMED
                </span>
              </div>
            </ChatboxSparksEffect>
          </div>
        </div>
      )}

      {activeTab === 'fonts' && (
        <div className="space-y-6 animate-view-fade-in">
          {/* Section 1: Primary UI Font Family */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                  <Type className="w-4 h-4 text-teal-500" />
                  <span>Primary UI Font Family</span>
                </h3>
                <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                  Applied across navigation, buttons, titles, cards, and modal dialogs.
                </p>
              </div>
              <span className="text-[11px] font-mono text-teal-600 dark:text-teal-400 px-2 py-0.5 rounded bg-teal-500/10 shrink-0">
                {PRIMARY_FONT_OPTIONS.find((f) => f.id === fontSettings.primaryFontId)?.name || 'Plus Jakarta Sans'}
              </span>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
              {PRIMARY_FONT_OPTIONS.map((font) => {
                const isSelected = fontSettings.primaryFontId === font.id;
                return (
                  <div
                    key={font.id}
                    onClick={() =>
                      handleFontChange({
                        primaryFontId: font.id,
                        primaryFontFamily: font.fontFamily
                      })
                    }
                    className={`p-3.5 rounded-xl border transition-all cursor-pointer relative ${
                      isSelected
                        ? 'border-teal-500 ring-2 ring-teal-500/20 bg-neutral-50 dark:bg-neutral-900/90 shadow-sm'
                        : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-neutral-300 dark:hover:border-neutral-700'
                    }`}
                  >
                    <div className="flex items-center justify-between mb-2">
                      <span className="font-bold text-xs text-neutral-900 dark:text-neutral-100 truncate">
                        {font.name}
                      </span>
                      {isSelected ? (
                        <span className="text-[10px] font-mono font-bold px-2 py-0.5 rounded-full bg-teal-500 text-neutral-950 flex items-center gap-1">
                          <Check className="w-3 h-3" />
                          Active
                        </span>
                      ) : (
                        <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-400 uppercase">
                          {font.category}
                        </span>
                      )}
                    </div>

                    <div
                      style={{ fontFamily: font.fontFamily }}
                      className="text-sm font-semibold text-neutral-800 dark:text-neutral-200 truncate py-1"
                    >
                      Sovereign Fleet AI 0123
                    </div>

                    <p className="text-[11px] text-neutral-500 line-clamp-1 mt-1">
                      {font.description}
                    </p>
                  </div>
                );
              })}
            </div>
          </div>

          {/* Section 2: Monospace & Code Font Family */}
          <div className="space-y-3 pt-2">
            <div>
              <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                <Baseline className="w-4 h-4 text-teal-500" />
                <span>Code &amp; Terminal Monospace Font</span>
              </h3>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                Applied to code snippets, Git hashes, diffs, JSON payloads, and terminal outputs.
              </p>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              {MONO_FONT_OPTIONS.map((font) => {
                const isSelected = fontSettings.monoFontId === font.id;
                return (
                  <div
                    key={font.id}
                    onClick={() =>
                      handleFontChange({
                        monoFontId: font.id,
                        monoFontFamily: font.fontFamily
                      })
                    }
                    className={`p-3.5 rounded-xl border transition-all cursor-pointer relative ${
                      isSelected
                        ? 'border-teal-500 ring-2 ring-teal-500/20 bg-neutral-50 dark:bg-neutral-900/90 shadow-sm'
                        : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-neutral-300 dark:hover:border-neutral-700'
                    }`}
                  >
                    <div className="flex items-center justify-between mb-2">
                      <span className="font-bold text-xs text-neutral-900 dark:text-neutral-100">
                        {font.name}
                      </span>
                      {isSelected && (
                        <span className="text-[10px] font-mono font-bold px-2 py-0.5 rounded-full bg-teal-500 text-neutral-950 flex items-center gap-1">
                          <Check className="w-3 h-3" />
                          Active
                        </span>
                      )}
                    </div>

                    <div
                      style={{ fontFamily: font.fontFamily }}
                      className="text-xs text-teal-600 dark:text-teal-400 font-mono py-1 truncate"
                    >
                      const run = await fleet.triage();
                    </div>

                    <p className="text-[11px] text-neutral-500 line-clamp-1 mt-1">
                      {font.description}
                    </p>
                  </div>
                );
              })}
            </div>
          </div>

          {/* Section 3: Interface Typography Scale */}
          <div className="space-y-3 pt-2">
            <div>
              <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                Interface Typography Scaling
              </h3>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                Proportionally adjusts root typography size for optimal density on your display.
              </p>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              {[
                { id: 'compact', label: 'Compact Scale', scale: '92%', hint: 'Tighter lines, maximized data visibility' },
                { id: 'normal', label: 'Default Scale', scale: '100%', hint: 'Balanced typography for standard displays' },
                { id: 'comfortable', label: 'Comfortable Scale', scale: '106%', hint: 'Relaxed reading height on larger monitors' }
              ].map((opt) => {
                const isSelected = fontSettings.fontScale === opt.id;
                return (
                  <button
                    key={opt.id}
                    type="button"
                    onClick={() => handleFontChange({ fontScale: opt.id as any })}
                    className={`p-3 rounded-xl border text-left transition-all cursor-pointer ${
                      isSelected
                        ? 'border-teal-500 bg-teal-500/10 text-teal-700 dark:text-teal-300 font-semibold shadow-xs'
                        : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] hover:border-neutral-300'
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1">
                      <span className="text-xs font-bold">{opt.label}</span>
                      <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-200 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300">
                        {opt.scale}
                      </span>
                    </div>
                    <p className="text-[11px] text-neutral-500 line-clamp-1">{opt.hint}</p>
                  </button>
                );
              })}
            </div>
          </div>

          {/* Section 4: Live Typography Preview Sandbox */}
          <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#191b1f] space-y-2">
            <span className="text-[10px] font-mono text-neutral-400 uppercase tracking-wider block">
              Active Typography Render Sample
            </span>
            <h4
              style={{ fontFamily: fontSettings.primaryFontFamily }}
              className="text-base sm:text-lg font-bold text-neutral-900 dark:text-neutral-100"
            >
              The pirate sovereign commands the fleet with immutable decentralized intelligence.
            </h4>
            <p
              style={{ fontFamily: fontSettings.primaryFontFamily }}
              className="text-xs text-neutral-600 dark:text-neutral-300 leading-relaxed"
            >
              Autonomous crew specialists deliberate and draft pull requests. Captain reviews high-risk gates before submitting deliverables.
            </p>
            <div
              style={{ fontFamily: fontSettings.monoFontFamily }}
              className="p-2.5 rounded-lg bg-neutral-100 dark:bg-neutral-950 font-mono text-xs text-teal-600 dark:text-teal-400 border border-neutral-200 dark:border-neutral-800"
            >
              &gt; galleon-fleet status --ship="Black Pearl" --quests=active
            </div>
          </div>
        </div>
      )}


      {/* LIVE INTERACTIVE THEME PREVIEW CARD */}
      <div className="pt-2">
        <span className="text-xs font-bold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 block mb-2">
          Live Theme Preview Simulation
        </span>

        <div
          className="p-5 rounded-2xl border transition-all space-y-4"
          style={{
            backgroundColor: activeTheme.bgSurface,
            borderColor: activeTheme.borderSubtle
          }}
        >
          <div className="flex items-center justify-between border-b pb-3" style={{ borderColor: activeTheme.borderSubtle }}>
            <div className="flex items-center gap-2.5">
              <div
                className="w-8 h-8 rounded-lg flex items-center justify-center font-bold text-xs"
                style={{
                  backgroundColor: `${activeTheme.brandPrimary}25`,
                  color: activeTheme.brandPrimary
                }}
              >
                GF
              </div>
              <div>
                <h4 className="font-bold text-sm" style={{ color: activeTheme.textPrimary }}>
                  Flagship Orchestration &bull; {activeTheme.name}
                </h4>
                <p className="text-xs" style={{ color: activeTheme.textSecondary }}>
                  Verified Sovereign Workspace &bull; Local-First
                </p>
              </div>
            </div>

            <span
              className="text-[10px] font-mono px-2 py-0.5 rounded font-bold uppercase"
              style={{
                backgroundColor: `${activeTheme.brandPrimary}25`,
                color: activeTheme.brandPrimary
              }}
            >
              Voyage Underway
            </span>
          </div>

          <p className="text-xs leading-relaxed" style={{ color: activeTheme.textPrimary }}>
            Quartermaster is actively deliberating with assigned Crew specialists. Artifact deliverables will be saved to your local workspace vault upon Captain approval.
          </p>

          <div className="flex items-center gap-2.5 pt-1">
            <button
              type="button"
              className="px-4 py-2 rounded-lg font-bold text-xs shadow-xs transition-opacity hover:opacity-90"
              style={{
                backgroundColor: activeTheme.brandPrimary,
                color: activeTheme.type === 'dark' ? '#000000' : '#ffffff'
              }}
            >
              Confirm Voyage Action
            </button>
            <button
              type="button"
              className="px-3.5 py-2 rounded-lg font-medium text-xs border"
              style={{
                borderColor: activeTheme.borderSubtle,
                color: activeTheme.textSecondary
              }}
            >
              Review Scope
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

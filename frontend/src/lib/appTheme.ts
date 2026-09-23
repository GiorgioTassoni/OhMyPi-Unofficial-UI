/** App-owned themes. OMP's `theme.dark` setting controls its own terminal UI, not this shell. */
export const APP_THEMES = [
  { id: "midnight-violet", name: "Midnight Violet", description: "The original deep-plum palette" },
  { id: "graphite-amber", name: "Graphite Amber", description: "Charcoal with warm gold" },
  { id: "deep-ocean", name: "Deep Ocean", description: "Midnight navy with clear blue" },
  { id: "forest-mint", name: "Forest Mint", description: "Evergreen charcoal with mint" },
  { id: "espresso-coral", name: "Espresso Coral", description: "Warm near-black with coral" },
  { id: "catppuccin-macchiato", name: "Catppuccin Macchiato", description: "The official soothing pastel palette" },
] as const;

export type AppTheme = (typeof APP_THEMES)[number]["id"];
export const DEFAULT_APP_THEME: AppTheme = "midnight-violet";
const STORAGE_KEY = "omp:app-theme";

export function parseAppTheme(value: string | null): AppTheme {
  return APP_THEMES.find((theme) => theme.id === value)?.id ?? DEFAULT_APP_THEME;
}

export function readAppTheme(): AppTheme {
  try {
    return parseAppTheme(localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_APP_THEME;
  }
}

export function setAppTheme(theme: AppTheme): void {
  document.documentElement.dataset.ompTheme = theme;
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Keep the live selection if browser storage is unavailable.
  }
}

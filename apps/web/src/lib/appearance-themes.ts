import { APPEARANCE_PALETTES, appearancePalette, type AppearancePalette, type AppearanceSeries } from './appearance-catalog';
import { normalizeAppearance, type AppearancePreferences } from './appearance-state';

export interface AppearanceTheme {
  readonly id: string;
  readonly name: string;
  readonly nameEn: string;
  readonly series: AppearanceSeries;
  readonly day: AppearancePalette;
  readonly night: AppearancePalette;
}

const originalPairs = [
  ['classic', 'classic-light', 'classic-dark', '经典', 'Classic'],
  ['mist', 'mist', 'nocturne', '晨雾', 'Morning mist'],
  ['pearl', 'pearl', 'plum', '珠光', 'Pearl'],
  ['iris', 'iris', 'plum', '鸢尾', 'Iris'],
  ['sand', 'sand', 'forest', '燕麦', 'Oat sand'],
  ['nocturne', 'pearl', 'nocturne', '夜曲', 'Nocturne'],
  ['waterlilies', 'waterlilies', 'pond', '莲池', 'Water lilies'],
  ['garden', 'garden', 'forest', '花园', 'Garden'],
  ['celadon', 'celadon', 'moon-jade', '青瓷', 'Celadon'],
  ['indigo', 'indigo-paper', 'ink-indigo', '藍染', 'Indigo'],
  ['clay', 'clay-paper', 'volcanic-clay', '陶土', 'Clay'],
  ['canyon-classic', 'canyon-paper', 'redwood-dusk', '峡谷与红杉', 'Canyon and redwood'],
  ['glacier-classic', 'glacier-paper', 'polar-night', '冰川与极夜', 'Glacier and polar night'],
  ['turquoise', 'turquoise-paper', 'bosphorus-night', '松石', 'Turquoise'],
  ['sahel', 'sahel-sand', 'highland-dusk', '萨赫勒', 'Sahel'],
] as const;

function theme(id: string, dayId: string, nightId: string, name: string, nameEn: string): AppearanceTheme {
  const day = appearancePalette(dayId);
  const night = appearancePalette(nightId);
  if (day?.period !== 'light' || night?.period !== 'dark') throw new Error(`Invalid theme pair: ${id}`);
  return { id, name, nameEn, series: day.series, day, night };
}

export const APPEARANCE_THEMES: readonly AppearanceTheme[] = [
  ...originalPairs.map(([id, day, night, name, nameEn]) => theme(id, day, night, name, nameEn)),
  ...APPEARANCE_PALETTES.filter(palette => palette.period === 'light' && palette.pair !== undefined).map(day => {
    const night = APPEARANCE_PALETTES.find(palette => palette.period === 'dark' && palette.pair === day.pair);
    if (!night || !day.pair) throw new Error(`Missing night palette: ${day.id}`);
    return theme(day.pair, day.id, night.id, day.name, day.nameEn);
  }),
];

export function selectedAppearanceTheme(value: Pick<AppearancePreferences, 'lightPalette' | 'darkPalette'>): AppearanceTheme | undefined {
  return APPEARANCE_THEMES.find(theme => theme.day.id === value.lightPalette && theme.night.id === value.darkPalette);
}

/** One state update sets the complete pair; fonts, icons and color mode stay independent. */
export function applyAppearanceTheme(current: AppearancePreferences, id: string): AppearancePreferences {
  const next = APPEARANCE_THEMES.find(theme => theme.id === id);
  if (!next) return current;
  return normalizeAppearance({ ...current, lightPalette: next.day.id, darkPalette: next.night.id, paletteSeries: next.series, presetId: null });
}

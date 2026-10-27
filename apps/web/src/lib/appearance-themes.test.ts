import { describe, expect, it } from 'vitest';
import { APPEARANCE_PALETTES } from './appearance-catalog';
import { DEFAULT_APPEARANCE, resolvedPaletteId } from './appearance-state';
import { APPEARANCE_THEMES, applyAppearanceTheme, selectedAppearanceTheme } from './appearance-themes';

describe('paired appearance themes', () => {
  it('covers every palette with valid day/night collections and unique IDs', () => {
    expect(new Set(APPEARANCE_THEMES.map(theme => theme.id)).size).toBe(APPEARANCE_THEMES.length);
    for (const palette of APPEARANCE_PALETTES) expect(APPEARANCE_THEMES.some(theme => theme.day.id === palette.id || theme.night.id === palette.id)).toBe(true);
    for (const theme of APPEARANCE_THEMES) { expect(theme.day.period).toBe('light'); expect(theme.night.period).toBe('dark'); }
  });
  it('applies both Palace palettes in one result and preserves personal font/icon choices', () => {
    const previous = { ...DEFAULT_APPEARANCE, uiFont: 'literary' as const, codeFont: 'menlo' as const, brandStyle: 'simple' as const, bookTitleStyle: 'literary' as const, iconStyle: 'fine' as const };
    const next = applyAppearanceTheme(previous, 'palace');
    expect(next).toMatchObject({ ...previous, lightPalette: 'palace-day', darkPalette: 'palace-night', paletteSeries: 'china', presetId: null });
    expect(selectedAppearanceTheme(next)?.id).toBe('palace');
    expect(resolvedPaletteId(next, 'light')).toBe('palace-day');
    expect(resolvedPaletteId(next, 'dark')).toBe('palace-night');
    expect(resolvedPaletteId(next, 'high-contrast')).toBeNull();
  });
  it('selects the companion night palette for regional/music themes and leaves custom stored pairs intact', () => {
    for (const id of ['nanjing', 'ginkgo', 'faun', 'moonlight', 'rach-2', 'paganini']) expect(selectedAppearanceTheme(applyAppearanceTheme(DEFAULT_APPEARANCE, id))?.id).toBe(id);
    const custom = { ...DEFAULT_APPEARANCE, lightPalette: 'palace-day', darkPalette: 'moonlight-night' };
    expect(selectedAppearanceTheme(custom)).toBeUndefined();
    expect(applyAppearanceTheme(custom, 'unknown')).toBe(custom);
  });
});

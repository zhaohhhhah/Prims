import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

import { createPdfAppearanceStore, DEFAULT_PDF_APPEARANCE, normalizePdfAppearance, pdfAppearanceFilter, pdfAppearanceKey } from './pdf-appearance';

describe('PDF reading appearance', () => {
  it('restores exact original and uses neutral soft/night brightness without colour inversion', () => {
    expect(pdfAppearanceFilter(DEFAULT_PDF_APPEARANCE)).toBe('none');
    expect(pdfAppearanceFilter({ mode: 'soft', brightness: 100 })).toBe('brightness(0.95) contrast(0.96)');
    expect(pdfAppearanceFilter({ mode: 'night', brightness: 100 })).toBe('brightness(0.72) contrast(0.96)');
    expect(pdfAppearanceFilter({ mode: 'original', brightness: 120 })).toBe('brightness(1.2) contrast(1)');
    for (const mode of ['original', 'soft', 'night'] as const) {
      expect(pdfAppearanceFilter({ mode, brightness: 100 })).not.toMatch(/invert|hue-rotate|sepia|saturate/);
    }
  });

  it('bounds persisted values and tolerates missing/corrupt options', () => {
    expect(normalizePdfAppearance(null)).toBe(DEFAULT_PDF_APPEARANCE);
    expect(normalizePdfAppearance({ mode: 'unknown', brightness: 999 })).toEqual({ mode: 'original', brightness: 120 });
    expect(normalizePdfAppearance({ mode: 'soft', brightness: Number.NaN })).toEqual({ mode: 'soft', brightness: 100 });
    expect(normalizePdfAppearance({ mode: 'night', brightness: -1 })).toEqual({ mode: 'night', brightness: 60 });
  });

  it('isolates accounts, restores preferences, and never persists anonymous state', () => {
    const data = new Map<string, string>();
    const storage = { getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => { data.set(key, value); } };
    const first = createPdfAppearanceStore(() => storage);
    first.set('user-a', { mode: 'night', brightness: 85 });
    first.set(null, { mode: 'soft', brightness: 90 });
    expect(first.read('user-b')).toEqual(DEFAULT_PDF_APPEARANCE);
    expect(first.read('user-a')).toEqual({ mode: 'night', brightness: 85 });
    expect(data.size).toBe(1);
    const reload = createPdfAppearanceStore(() => storage);
    expect(reload.read('user-a')).toEqual(first.read('user-a'));
    expect(reload.read(null)).toEqual(DEFAULT_PDF_APPEARANCE);
    storage.setItem(pdfAppearanceKey('user-a'), JSON.stringify({ mode: 'soft', brightness: 99 }));
    first.refresh('user-a');
    expect(first.read('user-a')).toEqual({ mode: 'soft', brightness: 99 });
  });

  it('handles SSR and blocked localStorage while keeping the active in-memory preference usable', () => {
    for (const storage of [() => null, () => { throw new Error('blocked'); }]) {
      const store = createPdfAppearanceStore(storage);
      expect(store.read('a')).toEqual(DEFAULT_PDF_APPEARANCE);
      expect(() => store.set('a', { mode: 'soft', brightness: 93 })).not.toThrow();
      expect(store.read('a')).toEqual({ mode: 'soft', brightness: 93 });
    }
  });

  it('syncs storage events only to the corresponding account and handles reset/logout switching', () => {
    const store = createPdfAppearanceStore(() => null);
    let a = 0;
    let b = 0;
    const stop = store.subscribe('a', () => { a += 1; });
    store.subscribe('b', () => { b += 1; });
    store.read('a'); store.read('b');
    store.storageChanged(pdfAppearanceKey('a'), JSON.stringify({ mode: 'night', brightness: 80 }));
    expect(store.read('a').mode).toBe('night');
    expect(store.read('b')).toEqual(DEFAULT_PDF_APPEARANCE);
    expect([a, b]).toEqual([1, 0]);
    store.storageChanged(pdfAppearanceKey('a'), null);
    expect(store.read('a')).toEqual(DEFAULT_PDF_APPEARANCE);
    stop();
    store.storageChanged(null, null);
    expect([a, b]).toEqual([2, 1]);
  });

  it('ignores malformed external events without replacing valid active preferences', () => {
    const store = createPdfAppearanceStore(() => null);
    store.set('a', { mode: 'night', brightness: 90 });
    store.storageChanged('another-setting', '{}');
    store.storageChanged(pdfAppearanceKey('a'), '{bad');
    expect(store.read('a')).toEqual({ mode: 'night', brightness: 90 });
  });

  it('filters only canvas wrapper, preserves text/SyncTeX and original download path, disables filter for print', () => {
    const source = readFileSync(new URL('../components/PDFPreview/PDFPreview.tsx', import.meta.url), 'utf8');
    expect(source).toContain('className="prims-pdf-canvas-surface" style={{ filter: appearanceFilter }}');
    expect(source).toMatch(/<\/div>\s*<div\s*ref=\{textLayerRef\}/);
    expect(source).toContain('@media print { .prims-pdf-canvas-surface { filter: none !important; } }');
    expect(source).toContain("downloadCompiledPdf(url, downloadFilename ?? 'document')");
    expect(source).toContain('viewport.convertToPdfPoint(vx, vy)');
    expect(source).not.toMatch(/toDataURL|toBlob|putImageData/);
    const host = readFileSync(new URL('../pages/project/PreviewPanel.tsx', import.meta.url), 'utf8');
    const header = host.slice(host.indexOf('role="tablist"'), host.indexOf('<div className="min-h-0 flex-1">'));
    expect(header).toContain('<PDFReadingAppearance appearance={appearance} onChange={setAppearance} />');
  });
});

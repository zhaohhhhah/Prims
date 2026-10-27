/** Display preferences only: never change PDF bytes or PDF.js rendering coordinates. */
export type PdfReadingMode = 'original' | 'soft' | 'night';

export interface PdfAppearance {
  readonly mode: PdfReadingMode;
  readonly brightness: number;
}

export const DEFAULT_PDF_APPEARANCE: PdfAppearance = Object.freeze({
  mode: 'original',
  brightness: 100,
});

export function normalizePdfAppearance(value: unknown): PdfAppearance {
  if (typeof value !== 'object' || value === null) return DEFAULT_PDF_APPEARANCE;
  const item = value as Record<string, unknown>;
  const mode = item['mode'] === 'soft' || item['mode'] === 'night' ? item['mode'] : 'original';
  const raw = item['brightness'];
  const brightness = typeof raw === 'number' && Number.isFinite(raw)
    ? Math.max(60, Math.min(120, Math.round(raw)))
    : 100;
  return { mode, brightness };
}

export function pdfAppearanceKey(userId: string): string {
  return `prims:pdf-appearance:v1:${encodeURIComponent(userId)}`;
}

export function pdfAppearanceFilter(value: PdfAppearance): string {
  const { mode, brightness } = normalizePdfAppearance(value);
  if (mode === 'original' && brightness === 100) return 'none';
  // Neutral luminance/contrast only: no inversion, hue rotation or colour remapping.
  const base = mode === 'night' ? 0.72 : mode === 'soft' ? 0.95 : 1;
  const luminance = Math.round(base * brightness) / 100;
  const contrast = mode === 'original' ? 1 : 0.96;
  return `brightness(${luminance}) contrast(${contrast})`;
}

type PreferenceStorage = Pick<Storage, 'getItem' | 'setItem'>;

/** Injectable storage keeps SSR, blocked storage, user switching and cross-tab updates testable. */
export function createPdfAppearanceStore(storage: () => PreferenceStorage | null) {
  const values = new Map<string, PdfAppearance>();
  const listeners = new Map<string, Set<() => void>>();
  const identity = (userId: string | null) => userId === null ? 'guest' : `user:${userId}`;
  const notify = (key: string) => { listeners.get(key)?.forEach((listener) => listener()); };

  function read(userId: string | null): PdfAppearance {
    const key = identity(userId);
    const cached = values.get(key);
    if (cached !== undefined) return cached;
    let next = DEFAULT_PDF_APPEARANCE;
    if (userId !== null) {
      try {
        const raw = storage()?.getItem(pdfAppearanceKey(userId));
        if (raw !== null && raw !== undefined) next = normalizePdfAppearance(JSON.parse(raw));
      } catch { /* unavailable, blocked, or malformed storage: safe original appearance */ }
    }
    values.set(key, next);
    return next;
  }

  return {
    read,
    refresh(userId: string | null) {
      if (userId === null) return;
      try {
        const target = storage();
        if (target === null) return;
        const raw = target.getItem(pdfAppearanceKey(userId));
        const next = raw === null ? DEFAULT_PDF_APPEARANCE : normalizePdfAppearance(JSON.parse(raw));
        const previous = values.get(identity(userId));
        if (previous?.mode === next.mode && previous.brightness === next.brightness) return;
        values.set(identity(userId), next);
        notify(identity(userId));
      } catch { /* keep the working in-memory state when storage is blocked */ }
    },
    set(userId: string | null, value: PdfAppearance) {
      const next = normalizePdfAppearance(value);
      const key = identity(userId);
      values.set(key, next);
      if (userId !== null) {
        try { storage()?.setItem(pdfAppearanceKey(userId), JSON.stringify(next)); } catch { /* memory-only preference still works */ }
      }
      notify(key);
    },
    subscribe(userId: string | null, listener: () => void) {
      const key = identity(userId);
      const group = listeners.get(key) ?? new Set<() => void>();
      group.add(listener);
      listeners.set(key, group);
      return () => {
        group.delete(listener);
        if (group.size === 0) listeners.delete(key);
      };
    },
    storageChanged(key: string | null, raw: string | null) {
      if (key === null) {
        for (const [id] of values) {
          if (!id.startsWith('user:')) continue;
          values.set(id, DEFAULT_PDF_APPEARANCE);
          notify(id);
        }
        return;
      }
      const prefix = 'prims:pdf-appearance:v1:';
      if (!key.startsWith(prefix)) return;
      try {
        const userId = decodeURIComponent(key.slice(prefix.length));
        const next = raw === null ? DEFAULT_PDF_APPEARANCE : normalizePdfAppearance(JSON.parse(raw));
        values.set(identity(userId), next);
        notify(identity(userId));
      } catch { /* unrelated or corrupt storage events must not break the viewer */ }
    },
  };
}

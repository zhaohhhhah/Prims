import { useCallback, useEffect, useSyncExternalStore } from 'react';

import { createPdfAppearanceStore, DEFAULT_PDF_APPEARANCE, pdfAppearanceFilter } from '../lib/pdf-appearance';

import type { PdfAppearance } from '../lib/pdf-appearance';

const store = createPdfAppearanceStore(() => typeof window === 'undefined' ? null : window.localStorage);

export function usePdfAppearance(userId: string | null) {
  const subscribe = useCallback((listener: () => void) => {
    const unsubscribe = store.subscribe(userId, listener);
    store.refresh(userId); // Pick up changes made while this viewer/account was unmounted.
    const onStorage = (event: StorageEvent) => {
      // Ignore sessionStorage and react only to browser-local preferences.
      try {
        if (event.storageArea !== null && event.storageArea !== window.localStorage) return;
      } catch { return; }
      store.storageChanged(event.key, event.newValue);
    };
    if (typeof window !== 'undefined') window.addEventListener('storage', onStorage);
    return () => {
      unsubscribe();
      if (typeof window !== 'undefined') window.removeEventListener('storage', onStorage);
    };
  }, [userId]);
  const getSnapshot = useCallback(() => store.read(userId), [userId]);
  const appearance = useSyncExternalStore(subscribe, getSnapshot, () => DEFAULT_PDF_APPEARANCE);
  const setAppearance = useCallback((next: PdfAppearance) => store.set(userId, next), [userId]);
  const filter = pdfAppearanceFilter(appearance);

  useEffect(() => {
    if (typeof document !== 'undefined') document.documentElement.style.setProperty('--prims-pdf-filter', filter);
  }, [filter]);

  return { appearance, setAppearance, filter };
}

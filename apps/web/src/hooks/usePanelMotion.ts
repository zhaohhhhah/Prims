import { useCallback, useEffect, useRef, type RefObject } from 'react';
import { flushSync } from 'react-dom';
import { PanelMotion, type PanelMotionOptions } from '../lib/panel-motion';

export function usePanelMotion(host: RefObject<HTMLElement>) {
  const motion = useRef<PanelMotion | null>(null);
  if (!motion.current) motion.current = new PanelMotion();
  useEffect(() => {
    const manager = motion.current!;
    const down = (event: PointerEvent) => {
      const target = event.target as Element | null;
      // A nested group's size also changes when the outer sidebar is dragged.
      if (target && typeof target.closest === 'function' && target.closest('[data-panel-resize-handle-id], [data-panel-resize-handle], [role="separator"]')) manager.setDragging(true);
    };
    const keyboardResize = (event: KeyboardEvent) => {
      const target = event.target as Element | null;
      if (target && typeof target.closest === 'function' && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key) && target.closest('[data-panel-resize-handle-id], [data-panel-resize-handle], [role="separator"]')) manager.cancel(true);
    };
    const up = () => manager.setDragging(false);
    const resize = () => manager.cancel(true);
    const media = window.matchMedia('(prefers-reduced-motion: reduce)');
    const preference = () => { if (media.matches) manager.cancel(true); };
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('keydown', keyboardResize, true);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', up);
    window.addEventListener('blur', up);
    window.addEventListener('resize', resize);
    media.addEventListener('change', preference);
    return () => {
      window.removeEventListener('pointerdown', down, true);
      window.removeEventListener('keydown', keyboardResize, true);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', up);
      window.removeEventListener('blur', up);
      window.removeEventListener('resize', resize);
      media.removeEventListener('change', preference);
      manager.cancel(false);
    };
  }, [host]);
  return useCallback((action: () => void, options: PanelMotionOptions) => {
    motion.current!.play(host.current, () => flushSync(action), options);
  }, [host]);
}

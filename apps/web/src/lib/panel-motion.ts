export type PanelMotionPhase = 'open' | 'close' | 'switch';
export interface PanelMotionOptions { phase: PanelMotionPhase; surface?: string; edge?: 'left' | 'right'; after?: () => void }
interface ActiveMotion { animations: Animation[]; timer: ReturnType<typeof setTimeout> | null; clear: () => void; finish: () => void }
const DURATION = 200;
function groupIn(host: HTMLElement | null): HTMLElement | null {
  return host?.matches('[data-panel-group]') ? host : host?.querySelector<HTMLElement>('[data-panel-group]') ?? null;
}
function panels(group: HTMLElement | null): HTMLElement[] {
  return group ? Array.from(group.children).filter(element => element.matches('[data-panel]')) as HTMLElement[] : [];
}
function grows(elements: readonly HTMLElement[], committed = false): Map<HTMLElement, number> {
  return new Map(elements.map(element => {
    // Read computed geometry only for the visible starting point. During a
    // reversal the old Web Animation may still be reflected by computed style
    // even after React has committed a different imperative panel target.
    const inline = committed ? Number.parseFloat(element.style?.flexGrow ?? '') : Number.NaN;
    return [element, Number.isFinite(inline) ? inline : Number.parseFloat(getComputedStyle(element).flexGrow) || 0] as const;
  }));
}

/** Only button-triggered operations enter here. The panel library still owns final layout. */
export class PanelMotion {
  private active: ActiveMotion | null = null;
  private dragging = false;
  constructor(private readonly reduced: () => boolean = () => typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches) {}
  setDragging(value: boolean): void { this.dragging = value; if (value) this.cancel(true); }
  cancel(complete = false): void {
    const current = this.active;
    if (!current) return;
    this.active = null;
    if (current.timer !== null) clearTimeout(current.timer);
    for (const animation of current.animations) animation.cancel();
    current.clear();
    if (complete) current.finish();
  }
  play(host: HTMLElement | null, commit: () => void, options: PanelMotionOptions): void {
    const beforeGroup = groupIn(host);
    const before = grows(panels(beforeGroup));
    const previousSurface = options.surface ? host?.querySelector<HTMLElement>(options.surface) : null;
    // Read the current animated geometry before cancelling: rapid reversals start
    // where the user is looking, rather than jumping to the old inline target.
    this.cancel(true);
    commit();
    const group = groupIn(host);
    const elements = panels(group);
    const after = grows(elements, true);
    const surface = options.surface ? host?.querySelector<HTMLElement>(options.surface) ?? previousSurface : null;
    const canAnimate = !this.dragging && !this.reduced() && elements.some(element => typeof element.animate === 'function');
    if (!canAnimate || !group) { options.after?.(); return; }
    const animations: Animation[] = [];
    const animate = (element: HTMLElement, frames: Keyframe[], duration = DURATION) => {
      if (typeof element.animate !== 'function') return;
      animations.push(element.animate(frames, { duration, easing: 'cubic-bezier(0.2, 0.65, 0.3, 1)', fill: 'both' }));
    };
    for (const element of elements) {
      const from = before.get(element) ?? 0, to = after.get(element) ?? 0;
      if (Math.abs(from - to) > 0.001) animate(element, [{ flexGrow: String(from) }, { flexGrow: String(to) }]);
    }
    if (surface) {
      const offset = options.edge === 'left' ? -4 : 4;
      animate(surface, options.phase === 'close'
        ? [{ opacity: 1, transform: 'translateX(0)' }, { opacity: 0.2, transform: `translateX(${offset}px)` }]
        : [{ opacity: 0.35, transform: `translateX(${offset}px)` }, { opacity: 1, transform: 'translateX(0)' }], options.phase === 'switch' ? 180 : DURATION);
    }
    if (!animations.length) { options.after?.(); return; }
    group.setAttribute('data-panel-motion', options.phase);
    let completed = false;
    const finish = () => {
      if (completed) return;
      completed = true;
      group.removeAttribute('data-panel-motion');
      options.after?.();
    };
    const current: ActiveMotion = { animations, timer: null, clear: () => group.removeAttribute('data-panel-motion'), finish };
    this.active = current;
    const settle = () => {
      if (this.active !== current) return;
      this.active = null;
      if (current.timer !== null) clearTimeout(current.timer);
      // The imperative panel target is already final. Remove animation effects
      // so future pointer drags respond immediately to the library's inline styles.
      for (const animation of animations) animation.cancel();
      finish();
    };
    void Promise.allSettled(animations.map(animation => animation.finished)).then(settle);
    current.timer = setTimeout(settle, DURATION + 60);
  }
}

/** A middle preview returns its size to the editor, not to a collapsed inspector. */
export function previewPanelLayout(layout: readonly number[], open: boolean, previousSize = 50): number[] | null {
  if (layout.length !== 3 || layout.some(size => !Number.isFinite(size))) return null;
  const [editor = 0, preview = 0, inspector = 0] = layout;
  if (!open) return [editor + preview, 0, inspector];
  const size = Math.min(Math.max(20, previousSize), 80 - inspector);
  return [100 - inspector - size, size, inspector];
}

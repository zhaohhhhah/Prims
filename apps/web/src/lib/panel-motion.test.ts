import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PanelMotion, previewPanelLayout } from './panel-motion';
class Effect {
  cancelled = false;
  resolve!: (value: Animation) => void;
  finished = new Promise<Animation>(resolve => { this.resolve = resolve; });
  cancel() { this.cancelled = true; }
  finish() { this.resolve(this as unknown as Animation); }
}
class Panel {
  grow: number;
  visual: number | null = null;
  effects: { frames: Keyframe[]; options: KeyframeAnimationOptions; effect: Effect }[] = [];
  constructor(grow: number) { this.grow = grow; }
  matches(selector: string) { return selector === '[data-panel]'; }
  animate(frames: Keyframe[], options: KeyframeAnimationOptions) { const effect = new Effect(); this.effects.push({ frames, options, effect }); return effect as unknown as Animation; }
}
function fixture() {
  const editor = new Panel(50), preview = new Panel(50), inspector = new Panel(0), content = new Panel(0);
  const attributes = new Map<string, string>();
  const group = { children: [editor, preview, inspector], matches: (selector: string) => selector === '[data-panel-group]', setAttribute: (key: string, value: string) => attributes.set(key, value), removeAttribute: (key: string) => attributes.delete(key), querySelector: () => null };
  const host = { matches: () => false, querySelector: (selector: string) => selector === '[data-panel-group]' ? group : content };
  return { host: host as unknown as HTMLElement, editor, preview, inspector, content, attributes };
}
describe('real resizable panel geometry motion', () => {
  beforeEach(() => { vi.useFakeTimers(); vi.stubGlobal('getComputedStyle', (element: Panel) => ({ flexGrow: String(element.visual ?? element.grow) })); });
  afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
  it('animates the actual panel flex-grow values and commits immediately', () => {
    const f = fixture(), manager = new PanelMotion(() => false), after = vi.fn();
    manager.play(f.host, () => { f.editor.grow = 75; f.preview.grow = 25; }, { phase: 'close', surface: '.preview', after });
    expect(f.editor.grow).toBe(75);
    expect(f.editor.effects[0]?.frames).toEqual([{ flexGrow: '50' }, { flexGrow: '75' }]);
    expect(f.preview.effects[0]?.frames).toEqual([{ flexGrow: '50' }, { flexGrow: '25' }]);
    expect(f.attributes.get('data-panel-motion')).toBe('close');
    expect(after).not.toHaveBeenCalled();
    vi.advanceTimersByTime(260);
    expect(after).toHaveBeenCalledTimes(1);
    expect(f.attributes.has('data-panel-motion')).toBe(false);
    expect(f.editor.effects[0]?.effect.cancelled).toBe(true);
  });
  it('uses the committed inline target when computed style still reflects the previous animation', () => {
    const f = fixture(), manager = new PanelMotion(() => false);
    // Real react-resizable-panels commits inline flexGrow. Computed style is
    // still zero at this instant because the old collapsed effect is visible.
    f.editor.grow = 0;
    manager.play(f.host, () => { Object.assign(f.editor, { style: { flexGrow: '18' } }); }, { phase: 'open' });
    expect(f.editor.effects[0]?.frames).toEqual([{ flexGrow: '0' }, { flexGrow: '18' }]);
    manager.cancel();
  });
  it('opens the permanently mounted inspector from size zero without replacing panel nodes', () => {
    const f = fixture(), manager = new PanelMotion(() => false), editor = f.editor;
    manager.play(f.host, () => { f.editor.grow = 37.5; f.preview.grow = 37.5; f.inspector.grow = 25; }, { phase: 'open', surface: '.inspector' });
    expect(f.inspector.effects[0]?.frames).toEqual([{ flexGrow: '0' }, { flexGrow: '25' }]);
    expect(f.editor).toBe(editor);
    expect(f.content.effects[0]?.options.duration).toBe(200);
    manager.cancel();
  });
  it('retains exit content until completion, then cleans up once', async () => {
    const f = fixture(), manager = new PanelMotion(() => false); let mounted = true;
    manager.play(f.host, () => { f.preview.grow = 0; f.editor.grow = 100; }, { phase: 'close', surface: '.preview', after: () => { mounted = false; } });
    expect(mounted).toBe(true);
    [...f.editor.effects, ...f.preview.effects, ...f.content.effects].forEach(item => item.effect.finish());
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    expect(mounted).toBe(false);
  });
  it('uses the current visual size for a rapid reversal and ignores old completion promises', async () => {
    const f = fixture(), manager = new PanelMotion(() => false);
    manager.play(f.host, () => { f.editor.grow = 100; f.preview.grow = 0; }, { phase: 'close' });
    const old = f.editor.effects[0]!.effect;
    f.editor.visual = 72; f.preview.visual = 28;
    manager.play(f.host, () => { f.editor.visual = null; f.preview.visual = null; f.editor.grow = 50; f.preview.grow = 50; }, { phase: 'open' });
    expect(f.editor.effects[1]?.frames).toEqual([{ flexGrow: '72' }, { flexGrow: '50' }]);
    old.finish(); await Promise.resolve(); await Promise.resolve();
    expect(f.attributes.get('data-panel-motion')).toBe('open');
    manager.cancel();
  });
  it('cancels before dragging and never animates layout operations during a drag', () => {
    const f = fixture(), manager = new PanelMotion(() => false), after = vi.fn();
    manager.play(f.host, () => { f.editor.grow = 60; }, { phase: 'open', after });
    manager.setDragging(true);
    expect(f.editor.effects[0]?.effect.cancelled).toBe(true);
    expect(after).toHaveBeenCalledTimes(1);
    manager.play(f.host, () => { f.editor.grow = 65; }, { phase: 'open' });
    expect(f.editor.effects).toHaveLength(1);
    expect(f.editor.grow).toBe(65);
  });
  it('reduced motion applies final layout and cleanup synchronously without animation', () => {
    const f = fixture(), manager = new PanelMotion(() => true), after = vi.fn();
    manager.play(f.host, () => { f.editor.grow = 100; f.preview.grow = 0; }, { phase: 'close', after });
    expect(f.editor.grow).toBe(100); expect(after).toHaveBeenCalledOnce(); expect(f.editor.effects).toHaveLength(0);
  });
  it('unmount cancellation clears timers and suppresses delayed state writes', () => {
    const f = fixture(), manager = new PanelMotion(() => false), after = vi.fn();
    manager.play(f.host, () => { f.editor.grow = 70; }, { phase: 'open', after });
    manager.cancel(false); vi.advanceTimersByTime(1000);
    expect(after).not.toHaveBeenCalled();
    expect(f.attributes.has('data-panel-motion')).toBe(false);
  });
});

describe('preview layout beside a permanently registered inspector', () => {
  it('collapses the middle PDF into the editor instead of overflowing the inspector max size', () => {
    expect(previewPanelLayout([50, 50, 0], false)).toEqual([100, 0, 0]);
    expect(previewPanelLayout([40, 35, 25], false)).toEqual([75, 0, 25]);
  });
  it('restores the previous PDF width while retaining inspector size and editor minimum', () => {
    expect(previewPanelLayout([100, 0, 0], true, 50)).toEqual([50, 50, 0]);
    expect(previewPanelLayout([75, 0, 25], true, 35)).toEqual([40, 35, 25]);
    expect(previewPanelLayout([55, 0, 45], true, 50)).toEqual([20, 35, 45]);
    expect(previewPanelLayout([], true)).toBeNull();
  });
});

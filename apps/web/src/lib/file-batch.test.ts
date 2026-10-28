import { describe, expect, it } from 'vitest';
import { planFileMoves, runFileBatch } from './file-batch';
import { registerFileOperationGuard, withFileOperationGuard } from './file-operation-guard';

describe('file batch plans', () => {
  const files = [{ id: 'a', path: 'chapters/one.tex' }, { id: 'b', path: 'chapters/nested/two.tex' }, { id: 'c', path: 'figures/two.png' }];
  it('preserves nested paths relative to the common selected parent', () => {
    expect(planFileMoves(files.slice(0, 2), files, [], 'archive').map(item => item.newPath)).toEqual(['archive/one.tex', 'archive/nested/two.tex']);
  });
  it('preserves separate root folders and allows moving to the root', () => {
    expect(planFileMoves(files, files, [], 'backup').map(item => item.newPath)).toEqual(['backup/chapters/one.tex', 'backup/chapters/nested/two.tex', 'backup/figures/two.png']);
    expect(planFileMoves([files[0]!], files, [], '').map(item => item.newPath)).toEqual(['one.tex']);
  });
  it('rejects traversal and absolute paths without touching any files', () => {
    for (const path of ['../oops', '/', '/outside', 'a/../b', 'a\\b', '.git', 'a//b', 'a:b']) expect(() => planFileMoves([files[0]!], files, [], path)).toThrow();
  });
  it('rejects file and directory destination collisions', () => {
    expect(() => planFileMoves([files[0]!], [...files, { id: 'd', path: 'archive/one.tex' }], [], 'archive')).toThrow();
    expect(() => planFileMoves([files[0]!], files, ['archive/one.tex'], 'archive')).toThrow();
    expect(() => planFileMoves([files[0]!], [...files, { id: 'd', path: 'archive' }], [], 'archive')).toThrow();
  });
  it('does not send no-op moves to the server', () => {
    expect(planFileMoves(files.slice(0, 2), files, [], 'chapters')).toEqual([]);
  });
  it('reports all successes and failures so only failed ids can be retried', async () => {
    const calls: string[] = [];
    const results = await runFileBatch(files, async file => { calls.push(file.id); if (file.id === 'b') throw { body: { message: 'read-only' } }; });
    expect(calls).toEqual(['a', 'b', 'c']);
    expect(results.map(item => item.ok)).toEqual([true, false, true]);
    expect(results[1]?.error).toBe('read-only');
  });
});

describe('editor save barrier', () => {
  it('flushes before executing and releases even if the file API fails', async () => {
    const calls: string[] = [];
    const unregister = registerFileOperationGuard('fixture', async write => { expect(write).toBe(true); calls.push('save'); return () => calls.push('release'); });
    await expect(withFileOperationGuard('fixture', true, async () => { calls.push('delete'); throw new Error('offline'); })).rejects.toThrow('offline');
    expect(calls).toEqual(['save', 'delete', 'release']); unregister();
  });
  it('does not mutate if the editor cannot save or is not registered', async () => {
    let touched = false;
    const unregister = registerFileOperationGuard('fixture', async () => { throw new Error('save failed'); });
    await expect(withFileOperationGuard('fixture', true, async () => { touched = true; })).rejects.toThrow('save failed');
    expect(touched).toBe(false); unregister();
    await expect(withFileOperationGuard('fixture', true, async () => {})).rejects.toThrow('Editor is not ready');
  });
});

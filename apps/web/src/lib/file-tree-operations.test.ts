import { describe, expect, it, vi } from 'vitest';
import { claimFileTreeDrag, mutateFolder } from './file-tree-operations';

const files = [{ id: 'one', path: 'chapters/one.tex' }, { id: 'two', path: 'chapters/nested/two.tex' }];
function operation() {
  return { path: 'chapters', expected: files, kind: 'rename' as 'rename' | 'delete', destination: 'archive', listFiles: vi.fn(async () => files), listDirectories: vi.fn(async () => [] as string[]), checkpoint: vi.fn(async () => {}), rename: vi.fn(async (_file: typeof files[number], _path: string) => {}), remove: vi.fn(async (_file: typeof files[number]) => {}), refresh: vi.fn(async () => {}) };
}

describe('folder drag ownership', () => {
  it('claims a nested drop once and leaves unrelated browser drags alone', () => {
    const event = { dataTransfer: { types: ['application/x-scribe-file'] }, preventDefault: vi.fn(), stopPropagation: vi.fn() };
    expect(claimFileTreeDrag(event, 'application/x-scribe-file')).toBe(true);
    expect(event.stopPropagation).toHaveBeenCalledOnce();
    expect(event.preventDefault).toHaveBeenCalledOnce();
    event.dataTransfer.types = ['Files'];
    expect(claimFileTreeDrag(event, 'application/x-scribe-file')).toBe(false);
    expect(event.stopPropagation).toHaveBeenCalledOnce();
  });
});

describe('folder operations', () => {
  it('keeps nested paths, checkpoints before writes, and reports partial failures', async () => {
    const io = operation(); const calls: string[] = [];
    io.checkpoint.mockImplementation(async () => { calls.push('checkpoint'); });
    io.rename.mockImplementation(async (file, path) => { calls.push(path); if (file.id === 'one') throw { body: { message: 'offline' } }; });
    const result = await mutateFolder(io);
    expect(calls).toEqual(['checkpoint', 'archive/one.tex', 'archive/nested/two.tex']);
    expect(result.map(item => item.ok)).toEqual([false, true]);
    expect(result[0]?.error).toBe('offline');
    expect(io.refresh).toHaveBeenCalledOnce();
  });
  it('refreshes after deletion failure and does not repeat successful operations', async () => {
    const io = operation(); io.kind = 'delete';
    io.remove.mockRejectedValueOnce(new Error('offline'));
    expect((await mutateFolder(io)).map(item => item.ok)).toEqual([false, true]);
    expect(io.remove).toHaveBeenCalledTimes(2); expect(io.refresh).toHaveBeenCalledOnce();
  });
  it('does not write when a destination collides or points inside the source', async () => {
    for (const destination of ['chapters/nested', '../escape', 'archive']) {
      const io = operation(); io.destination = destination;
      io.listDirectories.mockResolvedValue(['archive/one.tex']);
      await expect(mutateFolder(io)).rejects.toThrow();
      expect(io.checkpoint).not.toHaveBeenCalled(); expect(io.rename).not.toHaveBeenCalled();
      expect(io.refresh).toHaveBeenCalledOnce();
    }
  });
  it('does not write after a concurrent subtree change or failed checkpoint', async () => {
    const io = operation(); io.listFiles.mockResolvedValue([...files, { id: 'new', path: 'chapters/new.tex' }]);
    await expect(mutateFolder(io)).rejects.toThrow('Folder changed');
    expect(io.rename).not.toHaveBeenCalled();
    const failed = operation(); failed.checkpoint.mockRejectedValue(new Error('cannot save'));
    await expect(mutateFolder(failed)).rejects.toThrow('cannot save');
    expect(failed.rename).not.toHaveBeenCalled(); expect(failed.refresh).toHaveBeenCalledOnce();
  });
});

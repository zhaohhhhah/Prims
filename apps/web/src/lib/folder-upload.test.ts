import { describe, expect, it } from 'vitest';
import { planFolderImport } from './folder-import';
import { planFolderUpload } from './folder-upload';

const file = (path: string) => ({ name: path.split('/').pop()!, size: 2, webkitRelativePath: path, arrayBuffer: async () => new Uint8Array([0, 255]).buffer });
describe('folder uploads inside a project', () => {
  it('retains the selected folder and all nested paths through the shared packager', () => {
    const plan = planFolderUpload([file('paper/main.tex'), file('paper/figures/a.png')]);
    expect(plan.name).toBe('paper');
    expect(planFolderImport(plan.files).files.map(entry => entry.path)).toEqual(['paper/main.tex', 'paper/figures/a.png']);
  });
  it('skips metadata while preserving useful dotfiles and raw binary data', async () => {
    const plan = planFolderUpload([file('paper/.DS_Store'), file('paper/.git/config'), file('paper/.latexmkrc'), file('paper/a.png')]);
    expect(plan.skippedMetadata).toBe(2);
    expect(planFolderImport(plan.files).files.map(entry => entry.path)).toEqual(['paper/.latexmkrc', 'paper/a.png']);
    expect(new Uint8Array(await plan.files[1]!.arrayBuffer())).toEqual(new Uint8Array([0, 255]));
  });
  it('rejects mixed roots, traversal and empty directories before any upload', () => {
    expect(() => planFolderUpload([file('one/a.tex'), file('two/b.tex')])).toThrow();
    expect(() => planFolderUpload([file('paper/../a.tex')])).toThrow();
    expect(() => planFolderUpload([])).toThrow();
  });
});

import { unzipSync } from 'fflate';
import { describe, expect, it } from 'vitest';

import { type FolderFile, packageFolderImport, planFolderImport } from './folder-import';

function picked(path: string, body = 'fixture'): FolderFile {
  const bytes = new TextEncoder().encode(body);
  return { name: path.split('/').at(-1)!, webkitRelativePath: path, size: bytes.length, arrayBuffer: async () => bytes.buffer };
}

describe('project folder import', () => {
  it('removes only the selected root and preserves nested filenames and useful hidden files', () => {
    const plan = planFolderImport([picked('Paper/main.tex'), picked('Paper/chapters/引言.tex'), picked('Paper/.latexmkrc')]);
    expect(plan.name).toBe('Paper');
    expect(plan.files.map(item => item.path)).toEqual(['main.tex', 'chapters/引言.tex', '.latexmkrc']);
  });
  it('omits disclosed OS metadata and git files without dropping general hidden files', () => {
    const paths = ['Paper/main.tex', 'Paper/.DS_Store', 'Paper/__MACOSX/a', 'Paper/figures/._a.png', 'Paper/.git/config', 'Paper/Thumbs.db', 'Paper/desktop.ini', 'Paper/.notes'];
    const plan = planFolderImport(paths.map(path => picked(path)));
    expect(plan.skippedMetadata).toBe(6);
    expect(plan.files.map(item => item.path)).toEqual(['main.tex', '.notes']);
  });
  it('rejects unavailable browser paths, mixed roots, traversal, duplicates and file-directory collisions', () => {
    for (const paths of [['main.tex'], ['A/a.tex', 'B/b.tex'], ['Paper/../a.tex'], ['Paper/C:/a'], ['Paper/a\\b'], ['Paper/a.tex', 'Paper/a.tex'], ['Paper/a', 'Paper/a/b.tex']]) {
      expect(() => planFolderImport(paths.map(path => picked(path)))).toThrow();
    }
    expect(() => planFolderImport([])).toThrow();
    expect(() => planFolderImport([picked('Paper/.DS_Store')])).toThrow('没有可导入');
  });
  it('enforces file, total and entry quotas before reading file contents', () => {
    expect(() => planFolderImport([{ ...picked('Paper/a'), size: 8 * 1024 * 1024 + 1 }])).toThrow('8MiB');
    expect(() => planFolderImport(Array.from({ length: 5 }, (_, index) => ({ ...picked(`Paper/${index}`), size: 8 * 1024 * 1024 })))).toThrow('32MiB');
    expect(() => planFolderImport(Array.from({ length: 257 }, (_, index) => picked(`Paper/${index}`)))).toThrow('256');
  });
  it('creates a real ZIP with unchanged binary bytes and no extra top-level folder', async () => {
    const binary = new Uint8Array([0, 255, 1, 13, 10]);
    const result = await packageFolderImport([picked('Paper/main.tex', '\\documentclass{article}'), { ...picked('Paper/figures/test.png'), size: binary.length, arrayBuffer: async () => binary.buffer }]);
    const files = unzipSync(new Uint8Array(await result.archive.arrayBuffer()));
    expect(result.archive.name).toBe('Paper.zip');
    expect(Object.keys(files).sort()).toEqual(['figures/test.png', 'main.tex']);
    expect(files['figures/test.png']).toEqual(binary);
    expect(new TextDecoder().decode(files['main.tex'])).toBe('\\documentclass{article}');
  });
  it('does not upload anything and reports an interrupted local file read', async () => {
    await expect(packageFolderImport([{ ...picked('Paper/main.tex'), arrayBuffer: async () => { throw new Error('file is unavailable'); } }])).rejects.toThrow('file is unavailable');
    await expect(packageFolderImport([{ ...picked('Paper/main.tex'), size: 100 }])).rejects.toThrow('重新选择');
  });
});

import { unzipSync } from 'fflate';
import { describe, expect, it } from 'vitest';
import { classifyProjectImport, packageProjectImport } from './project-file-import';

describe('unified project import', () => {
  it('recognizes ZIP automatically and refuses ambiguous mixed archives', () => {
    expect(classifyProjectImport([{ name: 'Paper.ZIP' }])).toBe('zip');
    expect(classifyProjectImport([{ name: 'main.tex' }, { name: 'refs.bib' }])).toBe('files');
    expect(() => classifyProjectImport([{ name: 'a.zip' }, { name: 'main.tex' }])).toThrow('单独');
    expect(() => classifyProjectImport([{ name: 'a.zip' }, { name: 'b.zip' }])).toThrow('单独');
    expect(() => classifyProjectImport([{ name: 'a.7z' }])).toThrow('目前支持 ZIP');
  });
  it('reuses a single ZIP without unpacking or altering it', async () => {
    const zip = new File(['fixture'], 'Paper.ZIP');
    const result = await packageProjectImport([zip]);
    expect(result.archive).toBe(zip);
    expect(result.name).toBe('Paper');
  });
  it('packages single or multiple files with original names and exact content', async () => {
    const result = await packageProjectImport([new File(['@article{}'], 'refs.bib'), new File(['hello'], 'paper.tex')]);
    expect(result.name).toBe('paper');
    const entries = unzipSync(new Uint8Array(await result.archive.arrayBuffer()));
    expect(Object.keys(entries)).toEqual(['refs.bib', 'paper.tex']);
    expect(new TextDecoder().decode(entries['refs.bib'])).toBe('@article{}');
    expect(Object.keys(unzipSync(new Uint8Array(await (await packageProjectImport([new File(['one'], 'only.tex')])).archive.arrayBuffer())))).toEqual(['only.tex']);
  });
  it('retains relative directory paths and rejects duplicate ordinary filenames', async () => {
    const nested = new File(['nested'], 'part.tex');
    Object.defineProperty(nested, 'webkitRelativePath', { value: 'chapters/part.tex' });
    const result = await packageProjectImport([new File(['main'], 'main.tex'), nested]);
    expect(Object.keys(unzipSync(new Uint8Array(await result.archive.arrayBuffer())))).toEqual(['main.tex', 'chapters/part.tex']);
    await expect(packageProjectImport([new File(['a'], 'main.tex'), new File(['b'], 'main.tex')])).rejects.toThrow('重名');
  });
});

import { describe, expect, it } from 'vitest';
import { buildCompileAssistantRequest, compileProjectPath, MAX_COMPILE_ASSIST_PROMPT, sanitizeCompileMessage } from './compile-assist';
const base = { projectId: 'project-a', jobProjectId: 'project-a', status: 'error', knownFilePaths: ['main.tex', 'chapters/方法.tex'], entries: [] };
describe('compile assistant snapshot', () => {
 it('accepts only the current failed job and never includes another project snapshot', () => {
  expect(buildCompileAssistantRequest({ ...base, jobProjectId: 'project-b' })).toBeNull();
  expect(buildCompileAssistantRequest({ ...base, status: 'success' })).toBeNull();
  expect(buildCompileAssistantRequest({ ...base, status: 'running' })).toBeNull();
 });
 it('prefills an analysis request with a known relative tex diagnostic and no auto-send/apply', () => {
  const value = buildCompileAssistantRequest({ ...base, compileId: 'job-current', entries: [{ level: 'error', file: './chapters/方法.tex', line: 42, message: 'Undefined control sequence.', raw: 'RAW_SECRET_DO_NOT_SEND' }] });
  expect(value).toMatchObject({ projectId: 'project-a', source: 'compile', dock: true, diagnostic: { filePath: 'chapters/方法.tex', line: 42 } });
  expect(value?.prompt).toContain('BEGIN_COMPILE_DIAGNOSTICS'); expect(value?.prompt).toContain('手动重新编译');
  expect(JSON.stringify(value)).not.toContain('RAW_SECRET_DO_NOT_SEND'); expect(value).not.toHaveProperty('autoSend');
 });
 it('does not infer unknown, absolute or traversal targets; strips system paths and credentials', () => {
  expect(compileProjectPath('../main.tex', base.knownFilePaths)).toBeUndefined(); expect(compileProjectPath('/tmp/main.tex', base.knownFilePaths)).toBeUndefined();
  const value = buildCompileAssistantRequest({ ...base, entries: [{ level: 'error', file: '/srv/private/other/main.tex', line: 8, message: 'Error at /srv/private/other/main.tex\nAuthorization: Bearer never-print-this\nPATH=/private/bin\nSee https://host.invalid/?key=never-print-this' }] });
  expect(value?.diagnostic).toBeUndefined(); const text = value?.prompt ?? ''; expect(text).not.toContain('/srv/private/'); expect(text).not.toContain('never-print-this'); expect(text).not.toContain('/private/bin');
  expect(sanitizeCompileMessage('main.tex:8: undefined command')).toContain('main.tex:8');
 });
 it('can recover a known relative file:line prefix and caps the complete prompt', () => {
  const value = buildCompileAssistantRequest({ ...base, entries: [{ level: 'error', message: 'main.tex:17: undefined command' }, ...Array.from({ length: 1000 }, () => ({ level: 'info' as const, message: '大量编译输出'.repeat(500) }))] });
  expect(value?.diagnostic).toMatchObject({ filePath: 'main.tex', line: 17 }); expect(value?.prompt?.length).toBeLessThanOrEqual(MAX_COMPILE_ASSIST_PROMPT); expect(value?.prompt).toContain('"truncated": true');
 });
});

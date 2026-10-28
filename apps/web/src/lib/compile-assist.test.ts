import { describe, expect, it } from 'vitest';
import { buildCompileAssistantRequest, canRequestCompileAssistant, compileProjectPath, MAX_COMPILE_ASSIST_PROMPT, sanitizeCompileMessage } from './compile-assist';
const base = { projectId: 'project-a', jobProjectId: 'project-a', status: 'error', compileId: 'job-current', mainFile: 'main.tex', engine: 'xelatex', knownFilePaths: ['main.tex', 'chapters/方法.tex'], entries: [] };
describe('compile assistant snapshot', () => {
 it('accepts only the current finished job and never includes another project snapshot', () => {
  expect(buildCompileAssistantRequest({ ...base, jobProjectId: 'project-b' })).toBeNull();
  expect(buildCompileAssistantRequest({ ...base, status: 'queued' })).toBeNull();
  expect(buildCompileAssistantRequest({ ...base, compileId: undefined })).toBeNull();
  expect(canRequestCompileAssistant({ ...base, status: 'idle' })).toBe(false);
  expect(buildCompileAssistantRequest({ ...base, status: 'running' })).toBeNull();
 });
 it('prefills a repair request with a known relative tex diagnostic and review flow', () => {
  const value = buildCompileAssistantRequest({ ...base, compileId: 'job-current', entries: [{ level: 'error', file: './chapters/方法.tex', line: 42, message: 'Undefined control sequence.', raw: 'RAW_SECRET_DO_NOT_SEND' }] });
  expect(value).toMatchObject({ projectId: 'project-a', source: 'compile', dock: true, autoSend: true, compile: { jobId: 'job-current', mainFile: 'main.tex', engine: 'xelatex' }, diagnostic: { filePath: 'chapters/方法.tex', line: 42 } });
  expect(value?.prompt).toContain('请立即诊断'); expect(value?.prompt).toContain('直接提交'); expect(value?.prompt).not.toContain('没有修改建议也没关系'); expect(value?.prompt).toContain('BEGIN_COMPILE_DIAGNOSTICS'); expect(value?.prompt).toContain('手动重新编译');
  expect(JSON.stringify(value)).not.toContain('RAW_SECRET_DO_NOT_SEND'); expect(value?.autoSend).toBe(true); expect(value?.prompt).toContain('read_compile_log'); expect(value?.prompt).toContain('jobId'); expect(value?.prompt).toContain('用户接受后应用');
 });
 it('can diagnose a successful job with visible lint warnings using its actual engine', () => {
  const value = buildCompileAssistantRequest({ ...base, status: 'success', engine: 'lualatex', entries: [{ level: 'warning', source: 'chktex', message: 'Visible lint warning' }] });
  expect(value?.compile).toMatchObject({ jobId: 'job-current', mainFile: 'main.tex', engine: 'lualatex' });
  expect(value?.prompt).toContain('Visible lint warning'); expect(value?.prompt).toContain('success');
 });
 it('does not infer unknown, absolute or traversal targets; strips system paths and credentials', () => {
  expect(compileProjectPath('../main.tex', base.knownFilePaths)).toBeUndefined(); expect(compileProjectPath('/tmp/main.tex', base.knownFilePaths)).toBeUndefined();
  const value = buildCompileAssistantRequest({ ...base, entries: [{ level: 'error', file: '/srv/private/other/main.tex', line: 8, message: 'Error at /srv/private/other/main.tex\nAuthorization: Bearer never-print-this\nPATH=/private/bin\nSee https://host.invalid/?key=never-print-this' }] });
  expect(value?.diagnostic).toBeUndefined(); const text = value?.prompt ?? ''; expect(text).not.toContain('/srv/'); expect(text).not.toContain('never-print-this'); expect(text).not.toContain('/private/bin');
  expect(sanitizeCompileMessage('main.tex:8: undefined command')).toContain('main.tex:8');
 });
 it('can recover a known relative file:line prefix and caps the complete prompt', () => {
  const value = buildCompileAssistantRequest({ ...base, entries: [{ level: 'error', message: 'main.tex:17: undefined command' }, ...Array.from({ length: 1000 }, () => ({ level: 'info' as const, message: '大量编译输出'.repeat(500) }))] });
  expect(value?.diagnostic).toMatchObject({ filePath: 'main.tex', line: 17 }); expect(value?.prompt?.length).toBeLessThanOrEqual(MAX_COMPILE_ASSIST_PROMPT); expect(value?.prompt).toContain('"truncated": true');
 });
});

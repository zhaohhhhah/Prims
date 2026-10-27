import { describe, expect, it, vi } from 'vitest';
import { AgentValidationError, MEMORY_MAX_CHARACTERS, readSkillFile, refreshMemoryDraft, SKILL_EXAMPLE, SKILL_MAX_BYTES, validateMemoryContent, validateSkillContent } from './agent-library-validation';
describe('Agent library text boundaries and memory CAS draft preservation', () => {
  it('accepts the example YAML frontmatter and never interprets skill instructions', () => {
    expect(validateSkillContent(SKILL_EXAMPLE)).toBe(SKILL_EXAMPLE);
    const content = SKILL_EXAMPLE + '\n```sh\necho this-is-text-only\n```';
    expect(validateSkillContent(content)).toBe(content);
    expect(() => validateSkillContent('---\nname: missing-description\n---\nbody')).toThrow(AgentValidationError);
  });
  it('enforces the UTF-8 byte boundary, including multibyte text', () => {
    const prefix = '---\nname: bounded\ndescription: Test\n---\n';
    expect(validateSkillContent(prefix + 'a'.repeat(SKILL_MAX_BYTES - new TextEncoder().encode(prefix).length))).toHaveLength(SKILL_MAX_BYTES);
    expect(() => validateSkillContent(prefix + '中'.repeat(SKILL_MAX_BYTES / 3))).toThrow('skillTooLarge');
  });
  it('rejects oversized and non-Markdown files before reading', async () => {
    const text = vi.fn(async () => SKILL_EXAMPLE);
    await expect(readSkillFile({ name: 'SKILL.md', size: SKILL_MAX_BYTES + 1, text })).rejects.toThrow('skillTooLarge');
    await expect(readSkillFile({ name: 'setup.sh', size: 20, text })).rejects.toThrow('skillFileType');
    expect(text).not.toHaveBeenCalled();
  });
  it('returns a static read error and validates file contents after reading', async () => {
    await expect(readSkillFile({ name: 'SKILL.md', size: 20, text: async () => { throw new Error('private OS path'); } })).rejects.toThrow('skillReadFailed');
    await expect(readSkillFile({ name: 'SKILL.MD', size: 20, text: async () => SKILL_EXAMPLE })).resolves.toBe(SKILL_EXAMPLE);
  });
  it('counts memory in Unicode characters, with an empty string allowed for CAS clearing', () => {
    expect(validateMemoryContent('😀'.repeat(MEMORY_MAX_CHARACTERS))).toBe('😀'.repeat(MEMORY_MAX_CHARACTERS));
    expect(validateMemoryContent('')).toBe('');
    expect(() => validateMemoryContent('中'.repeat(MEMORY_MAX_CHARACTERS + 1))).toThrow('memoryTooLong');
  });
  it('loads a new CAS revision while preserving the local draft for an explicit later save', () => {
    const draft = { content: 'my unsaved draft', savedContent: 'old server text', revision: 3, conflict: true };
    expect(refreshMemoryDraft(draft, { content: 'new server text', revision: 4 })).toEqual({ ...draft, savedContent: 'new server text', revision: 4, conflict: false });
    expect(refreshMemoryDraft(draft, { content: '', revision: 4 }, false)).toEqual({ content: '', savedContent: '', revision: 4, conflict: false });
  });
  it('never rewinds the revision or accepts a malformed revision', () => {
    const draft = { content: 'mine', savedContent: '', revision: 4, conflict: true };
    expect(refreshMemoryDraft(draft, { content: 'stale', revision: 2 })).toBe(draft);
    expect(() => refreshMemoryDraft(draft, { content: '', revision: -1 })).toThrow('Invalid memory revision');
  });
});

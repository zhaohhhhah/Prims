import { describe, expect, it, vi } from 'vitest';
import { AgentValidationError, MEMORY_MAX_CHARACTERS, readSkillFile, refreshMemoryDraft, SEARCH_ENDPOINTS, SKILL_EXAMPLE, SKILL_MAX_BYTES, validateMemoryContent, validateSearchConfig, validateSkillContent } from './agent-library-validation';
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
describe('independent search configuration', () => {
  it('accepts the two full default endpoints with a separately entered key', () => {
    for (const provider of ['doubao', 'deepseek'] as const) expect(validateSearchConfig(provider, SEARCH_ENDPOINTS[provider], 'new-search-key', null)).toEqual({ provider, endpoint: SEARCH_ENDPOINTS[provider], apiKey: 'new-search-key' });
    expect(() => validateSearchConfig('tavily', 'https://api.example.test/search', 'key', null)).toThrow('searchProviderRequired');
  });
  it('only preserves an empty key for the exact same saved provider and endpoint', () => {
    const saved = { provider: 'doubao', endpoint: SEARCH_ENDPOINTS.doubao, hasApiKey: true };
    expect(validateSearchConfig('doubao', saved.endpoint, '', saved).apiKey).toBe('');
    expect(() => validateSearchConfig('deepseek', SEARCH_ENDPOINTS.deepseek, '', saved)).toThrow('searchKeyRequired');
    expect(() => validateSearchConfig('doubao', 'https://search.example.test/full/path', '', saved)).toThrow('searchKeyRequired');
    expect(() => validateSearchConfig('doubao', saved.endpoint, '', { ...saved, hasApiKey: false })).toThrow('searchKeyRequired');
  });
  it('rejects incomplete URLs, insecure protocols, credentials and fragments', () => {
    for (const endpoint of ['api.example.test/search', 'http://api.example.test/search', 'https://name:pass@api.example.test/search', 'https://api.example.test/search#fragment', 'https://api.example.test/search?api_key=secret', 'https://api.example.test/search?region=global']) expect(() => validateSearchConfig('doubao', endpoint, 'new-key', null)).toThrow('searchEndpointInvalid');
    expect(validateSearchConfig('doubao', 'https://search.example.test/custom/full/path', 'new-key', null).endpoint).toBe('https://search.example.test/custom/full/path');
    expect(validateSearchConfig('doubao', '', 'new-key', null).endpoint).toBe(SEARCH_ENDPOINTS.doubao);
  });
});

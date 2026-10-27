export const SKILL_MAX_BYTES = 48 * 1024;
export const MEMORY_MAX_CHARACTERS = 4000;
export type AgentValidationCode = 'skillTooLarge' | 'skillFormat' | 'skillFileType' | 'skillReadFailed' | 'memoryTooLong' | 'searchProviderRequired' | 'searchEndpointInvalid' | 'searchKeyRequired' | 'searchKeyInvalid' | 'searchLimitInvalid';
export class AgentValidationError extends Error {
  constructor(readonly code: AgentValidationCode) { super(code); }
}
export function validateSkillContent(content: string): string {
  if (new TextEncoder().encode(content).length > SKILL_MAX_BYTES) throw new AgentValidationError('skillTooLarge');
  const frontmatter = /^\uFEFF?---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(content)?.[1];
  if (!frontmatter || !/^name:\s*\S/m.test(frontmatter) || !/^description:\s*\S/m.test(frontmatter) || content.includes('\0')) throw new AgentValidationError('skillFormat');
  return content;
}
export async function readSkillFile(file: Pick<File, 'name' | 'size' | 'text'>): Promise<string> {
  if (!file.name.toLowerCase().endsWith('.md')) throw new AgentValidationError('skillFileType');
  if (file.size > SKILL_MAX_BYTES) throw new AgentValidationError('skillTooLarge');
  let content: string;
  try { content = await file.text(); } catch { throw new AgentValidationError('skillReadFailed'); }
  return validateSkillContent(content);
}
export function validateMemoryContent(content: string): string {
  if (Array.from(content).length > MEMORY_MAX_CHARACTERS) throw new AgentValidationError('memoryTooLong');
  return content;
}
export interface MemoryDraftState { content: string; savedContent: string; revision: number; conflict: boolean }
/** Refresh the CAS base without silently replacing a user's draft or submitting it. */
export function refreshMemoryDraft(current: MemoryDraftState | undefined, latest: { content: string; revision: number }, keepDraft = true): MemoryDraftState {
  if (!Number.isSafeInteger(latest.revision) || latest.revision < 0) throw new Error('Invalid memory revision.');
  if (current && latest.revision < current.revision) return current;
  return { content: keepDraft && current ? current.content : latest.content, savedContent: latest.content, revision: latest.revision, conflict: false };
}
export type SearchProvider = 'doubao' | 'deepseek' | 'searxng';
export const SEARCH_ENDPOINTS: Record<SearchProvider, string> = {
  doubao: 'https://open.feedcoopapi.com/search_api/web_search',
  deepseek: 'https://api.deepseek.com/v1/tools/search',
  searxng: '',
};
export function validateSearchConfig(provider: string, endpoint: string, apiKey: string, saved: { provider: string; endpoint: string; hasApiKey: boolean } | null, clearApiKey = false): { provider: SearchProvider; endpoint: string; apiKey: string; clearApiKey?: boolean } {
  if (provider !== 'doubao' && provider !== 'deepseek' && provider !== 'searxng') throw new AgentValidationError('searchProviderRequired');
  const address = endpoint.trim() || SEARCH_ENDPOINTS[provider];
  let parsed: URL;
  try { parsed = new URL(address); } catch { throw new AgentValidationError('searchEndpointInvalid'); }
  if (address.length > 2048 || parsed.protocol !== 'https:' || parsed.username || parsed.password || parsed.hash || /[\x00-\x20\x7f]/.test(address) || parsed.search) throw new AgentValidationError('searchEndpointInvalid');
  if (apiKey.length > 16384 || /[\x00-\x1f\x7f]/.test(apiKey)) throw new AgentValidationError('searchKeyInvalid');
  const key = apiKey.trim();
  if (clearApiKey && (provider !== 'searxng' || key)) throw new AgentValidationError('searchKeyInvalid');
  const canonical = parsed.toString();
  if (provider !== 'searxng' && !key && !(saved?.hasApiKey && saved.provider === provider && saved.endpoint === canonical)) throw new AgentValidationError('searchKeyRequired');
  return { provider, endpoint: canonical, apiKey: key, ...(clearApiKey ? { clearApiKey: true } : {}) };
}
export const SKILL_EXAMPLE = `---
name: academic-writing
description: Improve academic clarity while preserving citations and LaTeX commands.
---

# Academic writing

Read the relevant project text before suggesting edits.
Preserve citations, equations, labels and LaTeX commands.
Explain substantial changes briefly. Submit edits for the author's review.
`;

export function validateSearchLimit(value: string): number {
  if (!/^\d+$/.test(value.trim())) throw new AgentValidationError('searchLimitInvalid');
  const limit = Number(value);
  if (!Number.isSafeInteger(limit) || limit < 0 || limit > 4294967295) throw new AgentValidationError('searchLimitInvalid');
  return limit;
}

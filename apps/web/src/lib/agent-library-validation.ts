export const SKILL_MAX_BYTES = 48 * 1024;
export const MEMORY_MAX_CHARACTERS = 4000;
export type AgentValidationCode = 'skillTooLarge' | 'skillFormat' | 'skillFileType' | 'skillReadFailed' | 'memoryTooLong';
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
export const SKILL_EXAMPLE = `---
name: academic-writing
description: Improve academic clarity while preserving citations and LaTeX commands.
---

# Academic writing

Read the relevant project text before suggesting edits.
Preserve citations, equations, labels and LaTeX commands.
Explain substantial changes briefly. Submit edits for the author's review.
`;

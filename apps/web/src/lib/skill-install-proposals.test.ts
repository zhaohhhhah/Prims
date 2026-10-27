import { describe, expect, it } from 'vitest';
import { parseSkillInstallProposal, restoreSkillInstallCards } from './skill-install-proposals';
const id = '12345678-1234-1234-1234-123456789abc';
const doc = '---\nname: careful-writing\ndescription: Preserve citations.\n---\nRead the source before proposing edits.\n';
const proposal = { id, name: 'careful-writing', description: 'Preserve citations.', content: doc, source: null, status: 'pending_review', installed: false, untrusted: true, permissionsGranted: false };
const row = (raw: unknown) => ({ role: 'action' as const, raw: JSON.stringify(raw) });
const candidate = () => row({ kind: 'prims.skill_proposal', requestId: 'request-a', proposal });
describe('confirmation-only skill proposal history', () => {
  it('parses a complete preview without claiming installation', () => { const result = parseSkillInstallProposal(proposal); expect(result.installed).toBe(false); expect(result.permissionsGranted).toBe(false); });
  it('rejects inferred installs, privileges, incomplete content and unsafe links', () => {
    for (const patch of [{ installed: true }, { permissionsGranted: true }, { content: 'not a skill' }, { source: 'https://evil.test/SKILL.md' }, { source: 'https://raw.githubusercontent.com/a/b/main/SKILL.md?token=x' }]) expect(() => parseSkillInstallProposal({ ...proposal, ...patch })).toThrow();
  });
  it('restores pending cards as data only, without executing any installation', () => { const cards = restoreSkillInstallCards([candidate()]); expect(cards).toHaveLength(1); expect(cards[0]?.state).toBe('pending'); });
  it('deduplicates complete events and does not replace the first content by a reused ID', () => { expect(restoreSkillInstallCards([candidate(), candidate(), row({ kind: 'prims.skill_proposal', requestId: 'request-a', proposal: { ...proposal, description: 'changed metadata' } })])).toHaveLength(1); expect(restoreSkillInstallCards([candidate(), candidate()])[0]?.proposal.description).toBe('Preserve citations.'); });
  it('restores rejection across refresh and never resurrects it from a final duplicate', () => { const decision = row({ kind: 'prims.skill_decision', requestId: 'request-a', proposalId: id, state: 'rejected' }); expect(restoreSkillInstallCards([candidate(), decision, candidate()])[0]?.state).toBe('rejected'); });
  it('keeps decisions per proposal rather than installing an entire batch', () => { const second = row({ kind: 'prims.skill_proposal', requestId: 'request-a', proposal: { ...proposal, id: '87654321-1234-1234-1234-123456789abc' } }); const decision = row({ kind: 'prims.skill_decision', requestId: 'request-a', proposalId: id, state: 'installed', skillId: id }); const cards = restoreSkillInstallCards([candidate(), second, decision]); expect(cards.map(card => card.state)).toEqual(['installed', 'pending']); });
  it('never promotes model prose or malformed history into an actionable card', () => { expect(restoreSkillInstallCards([{ role: 'assistant', raw: JSON.stringify({ kind: 'prims.skill_proposal', proposal }) }, { role: 'action', raw: '{"kind":' }])).toEqual([]); });
});

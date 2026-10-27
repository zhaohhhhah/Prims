import type { ProjectAIMessage } from './project-ai-context';
import { validateSkillContent } from './agent-library-validation';
export interface SkillInstallProposal { id: string; name: string; description: string; content: string; source: string | null; status: 'pending_review'; installed: false; untrusted: true; permissionsGranted: false }
export interface SkillInstallCard { requestId: string; proposal: SkillInstallProposal; state: 'pending' | 'installed' | 'rejected'; skillId?: string }
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export function parseSkillInstallProposal(value: unknown): SkillInstallProposal {
  if (!value || typeof value !== 'object') throw new Error('技能安装建议格式无效');
  const item = value as Record<string, unknown>;
  if (typeof item.id !== 'string' || !uuid.test(item.id) || typeof item.name !== 'string' || item.name.length > 64 || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(item.name) || typeof item.description !== 'string' || !item.description.trim() || Array.from(item.description).length > 1024 || typeof item.content !== 'string' || item.status !== 'pending_review' || item.installed !== false || item.permissionsGranted !== false || item.untrusted !== true) throw new Error('技能安装建议缺少完整的待确认信息');
  validateSkillContent(item.content);
  let source: string | null = null;
  if (item.source !== null && item.source !== undefined) {
    if (typeof item.source !== 'string' || item.source.length > 2048 || item.source.includes('%')) throw new Error('技能来源无效');
    const address = new URL(item.source);
    if (address.protocol !== 'https:' || address.hostname !== 'raw.githubusercontent.com' || address.port && address.port !== '443' || address.username || address.password || address.search || address.hash || !address.pathname.endsWith('/SKILL.md')) throw new Error('技能来源必须是公开GitHub文件');
    source = item.source;
  }
  return { id: item.id, name: item.name, description: item.description, content: item.content, source, status: 'pending_review', installed: false, untrusted: true, permissionsGranted: false };
}
export function skillProposalMessage(requestId: string, proposal: SkillInstallProposal): ProjectAIMessage {
  return { id: crypto.randomUUID(), role: 'action', content: `技能安装建议待确认：${proposal.name}。尚未保存、启用或执行。`, raw: JSON.stringify({ kind: 'prims.skill_proposal', requestId, proposal }) };
}
export function skillDecisionMessage(card: SkillInstallCard, state: 'installed' | 'rejected', skillId?: string): ProjectAIMessage {
  return { id: crypto.randomUUID(), role: 'action', content: state === 'installed' ? `用户点击安装后，个人技能 ${card.proposal.name} 已保存并启用。未执行脚本，未授予新权限。` : `用户拒绝安装个人技能 ${card.proposal.name}，没有保存或启用。`, raw: JSON.stringify({ kind: 'prims.skill_decision', requestId: card.requestId, proposalId: card.proposal.id, state, ...(skillId ? { skillId } : {}) }) };
}
/** Historical data can restore a card, never authorize or trigger an installation. */
export function restoreSkillInstallCards(messages: readonly Pick<ProjectAIMessage, 'role' | 'raw'>[]): SkillInstallCard[] {
  const cards = new Map<string, SkillInstallCard>();
  const decisions = new Map<string, { state: 'installed' | 'rejected'; skillId?: string }>();
  for (const message of messages) {
    if (message.role !== 'action' || !message.raw) continue;
    try {
      const raw = JSON.parse(message.raw) as Record<string, unknown>;
      if (raw.kind === 'prims.skill_proposal' && typeof raw.requestId === 'string') {
        const proposal = parseSkillInstallProposal(raw.proposal);
        if (!cards.has(proposal.id)) cards.set(proposal.id, { requestId: raw.requestId, proposal, state: 'pending' });
      } else if (raw.kind === 'prims.skill_decision' && typeof raw.proposalId === 'string' && (raw.state === 'installed' || raw.state === 'rejected')) {
        decisions.set(raw.proposalId, { state: raw.state, ...(typeof raw.skillId === 'string' && uuid.test(raw.skillId) ? { skillId: raw.skillId } : {}) });
      }
    } catch { /* Incomplete or unrelated historical text cannot become an installable proposal. */ }
  }
  return [...cards.values()].slice(-6).map(card => ({ ...card, ...(decisions.get(card.proposal.id) ?? {}) }));
}

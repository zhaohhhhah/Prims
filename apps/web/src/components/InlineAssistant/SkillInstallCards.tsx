import { Button } from '@scribe/ui';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { agentLibraryApi, AgentLibraryError, agentQueryKey } from '../../lib/agent-library';
import { restoreSkillInstallCards, skillDecisionMessage, type SkillInstallCard } from '../../lib/skill-install-proposals';
import type { ProjectAIMessage } from '../../lib/project-ai-context';

interface Props { userId: string; projectId: string; threadId: string; messages: readonly ProjectAIMessage[]; disabled: boolean; onDecision: (message: ProjectAIMessage) => Promise<void> }
export function SkillInstallCards({ userId, projectId, threadId, messages, disabled, onDecision }: Props) {
  const cache = useQueryClient();
  const messageKey = messages.map(message => `${message.id}:${message.raw?.length ?? 0}`).join('|');
  const cards = useMemo(() => restoreSkillInstallCards(messages), [messageKey]);
  const key = `${userId}:${projectId}:${threadId}`;
  const scope = useRef({ key, inFlight: new Set<string>() });
  if (scope.current.key !== key) scope.current = { key, inFlight: new Set<string>() };
  useEffect(() => { scope.current = { key, inFlight: new Set<string>() }; return () => { scope.current = { key: 'closed', inFlight: new Set<string>() }; }; }, [key]);
  const [statuses, setStatuses] = useState<Record<string, 'installed' | 'rejected'>>({});
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  async function decide(card: SkillInstallCard, reject: boolean) {
    const owner = scope.current, id = card.proposal.id;
    if (disabled || owner.inFlight.size || card.state !== 'pending' || statuses[id]) return;
    owner.inFlight.add(id); setBusy(id); setError('');
    try {
      let skillId: string | undefined;
      if (!reject) {
        const preferences = await agentLibraryApi.preferences(userId);
        if (scope.current !== owner) return;
        if (!preferences.skillsEnabled) throw new Error('个人技能已关闭，请先在设置中启用。');
        // This explicit click is the only installation write. No model/tool/history path calls it.
        const saved = await agentLibraryApi.saveSkill(userId, null, card.proposal.content, true);
        skillId = saved.id;
        if (scope.current !== owner) return;
        await cache.invalidateQueries({ queryKey: agentQueryKey(userId, 'skills') });
      }
      if (scope.current !== owner) return;
      const state = reject ? 'rejected' : 'installed';
      setStatuses(current => ({ ...current, [id]: state }));
      await onDecision(skillDecisionMessage(card, state, skillId));
    } catch (failure) {
      if (scope.current === owner) setError(failure instanceof AgentLibraryError && failure.status === 409 ? '同名技能已存在，请在设置中处理。不会覆盖已有技能。' : failure instanceof Error ? failure.message : '技能安装未确认成功，请检查个人技能设置。');
    } finally { owner.inFlight.delete(id); if (scope.current === owner) setBusy(''); }
  }
  if (!cards.length) return null;
  return <section aria-label="技能安装建议" className="space-y-2">
    {cards.map(card => { const state = statuses[card.proposal.id] ?? card.state; return <div key={card.proposal.id} className="space-y-2 rounded-lg border p-3 text-xs">
      <div className="flex flex-wrap items-center justify-between gap-2"><strong className="break-words">{card.proposal.name}</strong><span className="text-muted-foreground">{state === 'installed' ? '已安装' : state === 'rejected' ? '已拒绝' : '待确认安装'}</span></div>
      <p className="line-clamp-2 text-muted-foreground">{card.proposal.description}</p>
      {card.proposal.source && <a className="block truncate text-muted-foreground underline" title={card.proposal.source} href={card.proposal.source} target="_blank" rel="noopener noreferrer">GitHub · SKILL.md</a>}
      <details><summary className="cursor-pointer text-muted-foreground">查看 SKILL.md</summary><pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-words rounded bg-muted/40 p-2 font-mono text-[11px]">{card.proposal.content}</pre></details>
      {state === 'pending' && <div className="flex gap-2"><Button type="button" size="sm" disabled={disabled || !!busy} onClick={() => { void decide(card, false); }}>{busy === card.proposal.id ? '正在处理…' : '安装'}</Button><Button type="button" size="sm" variant="outline" disabled={disabled || !!busy} onClick={() => { void decide(card, true); }}>拒绝</Button></div>}
    </div>; })}
    {error && <p role="alert" className="text-xs text-destructive">{error} <Link to="/settings?tab=agent" className="underline">管理技能</Link></p>}
  </section>;
}

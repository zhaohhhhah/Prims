import { Button } from '@scribe/ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';
import { agentLibraryApi, AgentLibraryError, agentQueryKey, type AgentPreferences } from '../../lib/agent-library';
import { AgentValidationError, readSkillFile, refreshMemoryDraft, SKILL_EXAMPLE, validateMemoryContent, validateSkillContent, type MemoryDraftState } from '../../lib/agent-library-validation';
import { agentLibraryCopy } from '../../locales/agent-library-copy';
import { useAuthStore } from '../../stores/auth';

interface SkillDraft { content: string; enabled: boolean; savedContent: string; savedEnabled: boolean }
type MemoryDraft = MemoryDraftState;
interface LibraryDrafts { skillId: string; projectId: string; skills: Record<string, SkillDraft>; memories: Record<string, MemoryDraft> }
// Drafts survive settings navigation within this browser session, account-isolated.
// They are never put in localStorage, never shared with another account, and never sent until Save.
const draftsByUser = new Map<string, LibraryDrafts>();
const skillDirty = (draft: SkillDraft) => draft.content !== draft.savedContent || draft.enabled !== draft.savedEnabled;
const memoryDirty = (draft: MemoryDraft) => draft.content !== draft.savedContent;
const newSkillDraft = (): SkillDraft => ({ content: SKILL_EXAMPLE, enabled: false, savedContent: SKILL_EXAMPLE, savedEnabled: false });
const inputClass = 'w-full rounded-md border bg-background px-3 py-2 text-sm disabled:opacity-60';

export function AgentSettingsTab() {
  const userId = useAuthStore(state => state.user?.id);
  return userId ? <AgentLibraryPanel key={userId} userId={userId} /> : null;
}

function AgentLibraryPanel({ userId }: { userId: string }) {
  const { i18n } = useTranslation();
  const copy = agentLibraryCopy(i18n?.resolvedLanguage);
  const [params] = useSearchParams();
  const cache = useQueryClient();
  const [drafts, setDrafts] = useState<LibraryDrafts>(() => draftsByUser.get(userId) ?? { skillId: 'new', projectId: params.get('project') ?? '', skills: { new: newSkillDraft() }, memories: {} });
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const upload = useRef<HTMLInputElement>(null);
  const scope = useRef({});
  useEffect(() => { const current = {}; scope.current = current; return () => { scope.current = {}; }; }, []);
  useEffect(() => { draftsByUser.set(userId, drafts); while (draftsByUser.size > 3) draftsByUser.delete(draftsByUser.keys().next().value!); }, [userId, drafts]);
  const dirty = Object.values(drafts.skills).some(skillDirty) || Object.values(drafts.memories).some(memoryDirty);
  useEffect(() => {
    if (!dirty) return;
    const handler = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [dirty]);

  const preferences = useQuery({ queryKey: agentQueryKey(userId, 'preferences'), queryFn: () => agentLibraryApi.preferences(userId), retry: false });
  const capabilities = useQuery({ queryKey: agentQueryKey(userId, 'capabilities'), queryFn: () => agentLibraryApi.capabilities(userId), retry: false, staleTime: 10000 });
  const skills = useQuery({ queryKey: agentQueryKey(userId, 'skills'), queryFn: () => agentLibraryApi.skills(userId), retry: false });
  const projects = useQuery({ queryKey: agentQueryKey(userId, 'projects'), queryFn: () => agentLibraryApi.projects(userId), retry: false });
  const skill = useQuery({ queryKey: agentQueryKey(userId, 'skill', drafts.skillId), queryFn: () => agentLibraryApi.skill(userId, drafts.skillId), enabled: drafts.skillId !== 'new', retry: false });
  const memory = useQuery({ queryKey: agentQueryKey(userId, 'memory', drafts.projectId), queryFn: () => agentLibraryApi.memory(userId, drafts.projectId), enabled: !!drafts.projectId, retry: false });

  useEffect(() => {
    if (!projects.data) return;
    if (!projects.data.some(project => project.id === drafts.projectId)) setDrafts(current => ({ ...current, projectId: projects.data?.[0]?.id ?? '' }));
  }, [projects.data, drafts.projectId]);
  useEffect(() => {
    const value = skill.data;
    if (!value || value.id !== drafts.skillId) return;
    setDrafts(current => {
      const previous = current.skills[value.id];
      if (previous && skillDirty(previous)) return current;
      if (previous?.content === value.content && previous.enabled === value.enabled) return current;
      return { ...current, skills: { ...current.skills, [value.id]: { content: value.content, enabled: value.enabled, savedContent: value.content, savedEnabled: value.enabled } } };
    });
  }, [skill.data, drafts.skillId]);
  useEffect(() => {
    const value = memory.data, project = drafts.projectId;
    if (!value || !project) return;
    setDrafts(current => {
      const previous = current.memories[project];
      if (previous && (memoryDirty(previous) || previous.conflict)) return current;
      if (previous?.revision === value.revision && previous.content === value.content) return current;
      return { ...current, memories: { ...current.memories, [project]: { content: value.content, savedContent: value.content, revision: value.revision, conflict: false } } };
    });
  }, [memory.data, drafts.projectId]);
  const skillDraft = drafts.skills[drafts.skillId];
  const memoryDraft = drafts.memories[drafts.projectId];
  function feedbackReset() { setError(''); setNotice(''); }
  function failureMessage(failure: unknown) { return failure instanceof AgentValidationError ? copy[failure.code] : failure instanceof Error ? failure.message : copy.requestFailed; }
  function changeSkill(patch: Partial<SkillDraft>) { setDrafts(current => { const previous = current.skills[current.skillId]; return previous ? { ...current, skills: { ...current.skills, [current.skillId]: { ...previous, ...patch } } } : current; }); }
  async function togglePreference(key: keyof AgentPreferences) {
    if (busy || !preferences.data) return;
    const owner = scope.current, next = { ...preferences.data, [key]: !preferences.data[key] };
    setBusy('preferences'); feedbackReset();
    try { const value = await agentLibraryApi.setPreferences(userId, next); if (scope.current === owner) { cache.setQueryData(agentQueryKey(userId, 'preferences'), value); setNotice(copy.settingsSaved); void capabilities.refetch(); } }
    catch (failure) { if (scope.current === owner) setError(failureMessage(failure)); }
    finally { if (scope.current === owner) setBusy(''); }
  }
  async function importSkill(file: File) {
    if (busy) return;
    if (drafts.skills.new && skillDirty(drafts.skills.new) && !window.confirm(copy.replaceDraft)) { if (upload.current) upload.current.value = ''; return; }
    const owner = scope.current; setBusy('file'); feedbackReset();
    try { const content = await readSkillFile(file); if (scope.current === owner) setDrafts(current => ({ ...current, skillId: 'new', skills: { ...current.skills, new: { content, enabled: false, savedContent: '', savedEnabled: false } } })); }
    catch (failure) { if (scope.current === owner) setError(failureMessage(failure)); }
    finally { if (scope.current === owner) { setBusy(''); if (upload.current) upload.current.value = ''; } }
  }
  async function saveSkill() {
    if (busy || !skillDraft) return;
    const owner = scope.current, id = drafts.skillId, draft = skillDraft;
    setBusy('skill'); feedbackReset();
    try {
      validateSkillContent(draft.content);
      const value = await agentLibraryApi.saveSkill(userId, id === 'new' ? null : id, draft.content, draft.enabled);
      if (scope.current !== owner) return;
      cache.setQueryData(agentQueryKey(userId, 'skill', value.id), value);
      setDrafts(current => ({ ...current, skillId: value.id, skills: { ...current.skills, ...(id === 'new' ? { new: newSkillDraft() } : {}), [value.id]: { content: value.content, enabled: value.enabled, savedContent: value.content, savedEnabled: value.enabled } } }));
      setNotice(copy.skillSaved); await skills.refetch();
    } catch (failure) { if (scope.current === owner) setError(failureMessage(failure)); }
    finally { if (scope.current === owner) setBusy(''); }
  }
  async function archiveSkill() {
    if (busy || drafts.skillId === 'new' || !window.confirm(copy.archiveConfirm)) return;
    const owner = scope.current, id = drafts.skillId; setBusy('skill'); feedbackReset();
    try { const result = await agentLibraryApi.archiveSkill(userId, id); if (!result.ok) throw new Error(copy.requestFailed); if (scope.current !== owner) return; setDrafts(current => { const next = { ...current.skills }; delete next[id]; return { ...current, skillId: 'new', skills: { ...next, new: current.skills.new ?? newSkillDraft() } }; }); setNotice(copy.archived); await skills.refetch(); }
    catch (failure) { if (scope.current === owner) setError(failureMessage(failure)); }
    finally { if (scope.current === owner) setBusy(''); }
  }
  async function saveMemory(clear = false) {
    if (busy || !memoryDraft || !drafts.projectId || memoryDraft.conflict || (clear && !window.confirm(copy.clearConfirm))) return;
    const owner = scope.current, project = drafts.projectId, content = clear ? '' : memoryDraft.content, revision = memoryDraft.revision;
    setBusy('memory'); feedbackReset();
    try {
      validateMemoryContent(content);
      const value = await agentLibraryApi.saveMemory(userId, project, { content, revision });
      if (scope.current !== owner) return;
      cache.setQueryData(agentQueryKey(userId, 'memory', project), value);
      setDrafts(current => ({ ...current, memories: { ...current.memories, [project]: { content: value.content, savedContent: value.content, revision: value.revision, conflict: false } } }));
      setNotice(clear ? copy.cleared : copy.memorySaved);
    } catch (failure) {
      if (scope.current !== owner) return;
      if (failure instanceof AgentLibraryError && failure.status === 409) { setDrafts(current => ({ ...current, memories: { ...current.memories, [project]: { ...memoryDraft, conflict: true } } })); setError(copy.conflict); }
      else setError(failureMessage(failure));
    } finally { if (scope.current === owner) setBusy(''); }
  }
  async function loadLatestMemory() {
    if (busy || !drafts.projectId) return;
    const owner = scope.current, project = drafts.projectId; setBusy('memory'); feedbackReset();
    try { const value = await agentLibraryApi.memory(userId, project); if (scope.current !== owner) return; cache.setQueryData(agentQueryKey(userId, 'memory', project), value); setDrafts(current => ({ ...current, memories: { ...current.memories, [project]: refreshMemoryDraft(current.memories[project], value) } })); setNotice(copy.latestLoaded); }
    catch (failure) { if (scope.current === owner) setError(failureMessage(failure)); }
    finally { if (scope.current === owner) setBusy(''); }
  }
  const capabilityText = (available: boolean | undefined) => capabilities.isPending ? copy.checking : available === true ? copy.ready : copy.unavailable;
  const retryLine = (failure: Error | null, retry: () => void) => failure ? <p role="alert" className="text-sm text-destructive">{failure.message || copy.loadFailed} <button type="button" className="underline" onClick={retry}>{copy.retry}</button></p> : null;
  return <div className="space-y-6">
    <section className="space-y-3" aria-label={copy.capabilities}>
      <h3 className="text-sm font-medium">{copy.capabilities}</h3>
      <p className="rounded-md border bg-muted/30 p-3 text-sm leading-relaxed">{copy.boundary}</p>
      <p className="text-xs text-muted-foreground">{copy.defaultModel}</p>
      <div className="grid gap-2 text-sm sm:grid-cols-2">
        <p>{copy.worker} · <span className="font-medium">{capabilityText(capabilities.data?.diagnostics?.ready)}</span></p>
        <p>{copy.search} · <span className="font-medium">{capabilityText(capabilities.data?.search?.available)}</span></p>
      </div>
      <p className="text-xs text-muted-foreground">{copy.diagnosticsScope}</p>
      <p className="text-xs text-muted-foreground">{copy.searchScope}</p>
      {retryLine(capabilities.error, () => { void capabilities.refetch(); })}
      <div className="space-y-2">{(['memoryEnabled', 'skillsEnabled', 'diagnosticsEnabled', 'searchEnabled'] as const).map(key => <label key={key} className="flex items-center justify-between gap-3 rounded-md border px-3 py-2 text-sm"><span>{copy[key]}</span><input type="checkbox" checked={preferences.data?.[key] ?? false} disabled={!!busy || !preferences.data || preferences.isError} onChange={() => { void togglePreference(key); }} /></label>)}</div>
      {retryLine(preferences.error, () => { void preferences.refetch(); })}
    </section>
    <section className="space-y-3 border-t pt-5" aria-label={copy.skills}>
      <div className="flex flex-wrap items-center justify-between gap-2"><h3 className="text-sm font-medium">{copy.skills}</h3><div className="flex gap-2"><Button type="button" size="sm" variant="outline" disabled={!!busy} onClick={() => { feedbackReset(); setDrafts(current => ({ ...current, skillId: 'new', skills: { ...current.skills, new: current.skills.new ?? newSkillDraft() } })); }}>{copy.newSkill}</Button><Button type="button" size="sm" variant="outline" disabled={!!busy} onClick={() => upload.current?.click()}>{busy === 'file' ? copy.fileReading : copy.importSkill}</Button></div></div>
      <input ref={upload} type="file" accept=".md,text/markdown" className="hidden" aria-label={copy.importSkill} onChange={event => { const file = event.target.files?.[0]; if (file) void importSkill(file); }} />
      <p className="text-xs leading-relaxed text-muted-foreground">{copy.skillHint}</p>
      <select aria-label={copy.skills} className={inputClass} value={drafts.skillId} disabled={!!busy} onChange={event => { feedbackReset(); setDrafts(current => ({ ...current, skillId: event.target.value })); }}><option value="new">{copy.newSkill}{drafts.skills.new && skillDirty(drafts.skills.new) ? ' *' : ''}</option>{skills.data?.skills.map(item => <option key={item.id} value={item.id}>{item.name} · {item.enabled ? copy.enabled : copy.disabled}{drafts.skills[item.id] && skillDirty(drafts.skills[item.id]!) ? ' *' : ''}</option>)}</select>
      {drafts.skillId !== 'new' && skill.data?.description && <p className="text-xs text-muted-foreground">{skill.data.description}</p>}
      {skills.data?.skills.length === 0 && <p className="text-xs text-muted-foreground">{copy.noSkills}</p>}
      {retryLine(skills.error, () => { void skills.refetch(); })}
      {drafts.skillId !== 'new' && retryLine(skill.error, () => { void skill.refetch(); })}
      {skillDraft ? <><label className="block space-y-1 text-sm"><span>{copy.content}</span><textarea className={`${inputClass} min-h-56 font-mono text-xs leading-relaxed`} value={skillDraft.content} disabled={!!busy} onChange={event => changeSkill({ content: event.target.value })} spellCheck={false} /></label><label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={skillDraft.enabled} disabled={!!busy} onChange={event => changeSkill({ enabled: event.target.checked })} />{copy.enableSkill}</label><div className="flex flex-wrap items-center gap-2"><Button type="button" size="sm" disabled={!!busy} onClick={() => { void saveSkill(); }}>{busy === 'skill' ? copy.saving : copy.save}</Button>{drafts.skillId !== 'new' && <Button type="button" variant="outline" size="sm" disabled={!!busy} onClick={() => { void archiveSkill(); }}>{copy.archive}</Button>}{skillDirty(skillDraft) && <span className="text-xs text-muted-foreground">{copy.unsaved}</span>}</div></> : skill.isPending ? <p className="text-sm text-muted-foreground">{copy.loading}</p> : null}
    </section>
    <section className="space-y-3 border-t pt-5" aria-label={copy.memory}>
      <h3 className="text-sm font-medium">{copy.memory}</h3><p className="text-xs text-muted-foreground">{copy.memoryHint}</p>
      <select aria-label={copy.project} className={inputClass} value={drafts.projectId} disabled={!!busy || projects.isPending} onChange={event => { feedbackReset(); setDrafts(current => ({ ...current, projectId: event.target.value })); }}><option value="">{copy.chooseProject}</option>{projects.data?.map(project => <option key={project.id} value={project.id}>{project.name}{drafts.memories[project.id] && memoryDirty(drafts.memories[project.id]!) ? ' *' : ''}</option>)}</select>
      {projects.data?.length === 0 && <p className="text-sm text-muted-foreground">{copy.noProjects}</p>}{retryLine(projects.error, () => { void projects.refetch(); })}
      {drafts.projectId && retryLine(memory.error, () => { void memory.refetch(); })}
      {drafts.projectId && memoryDraft ? <><textarea aria-label={copy.memory} className={`${inputClass} min-h-36 leading-relaxed`} value={memoryDraft.content} disabled={!!busy} onChange={event => { const content = event.target.value; setDrafts(current => ({ ...current, memories: { ...current.memories, [current.projectId]: { ...memoryDraft, content } } })); }} /><div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground"><span>{copy.revision} {memoryDraft.revision}</span><span>{Array.from(memoryDraft.content).length.toLocaleString()} / 4,000</span></div>{memoryDraft.conflict && <p role="alert" className="text-sm text-destructive">{copy.conflict}</p>}<div className="flex flex-wrap items-center gap-2"><Button type="button" size="sm" disabled={!!busy || memoryDraft.conflict} onClick={() => { void saveMemory(); }}>{busy === 'memory' ? copy.saving : copy.save}</Button><Button type="button" variant="outline" size="sm" disabled={!!busy || memoryDraft.conflict} onClick={() => { void saveMemory(true); }}>{copy.clear}</Button>{memoryDraft.conflict && <Button type="button" variant="outline" size="sm" disabled={!!busy} onClick={() => { void loadLatestMemory(); }}>{copy.loadLatest}</Button>}{memoryDirty(memoryDraft) && <span className="text-xs text-muted-foreground">{copy.unsaved}</span>}</div>{memoryDirty(memoryDraft) && <details className="text-xs text-muted-foreground"><summary className="cursor-pointer">{copy.latestContent}</summary><pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap rounded border p-2 font-sans">{memoryDraft.savedContent || '—'}</pre></details>}</> : drafts.projectId && memory.isPending ? <p className="text-sm text-muted-foreground">{copy.loading}</p> : null}
    </section>
    {error && <p role="alert" className="rounded-md border border-destructive/30 p-3 text-sm text-destructive">{error}</p>}
    {notice && <p role="status" className="text-sm text-muted-foreground">{notice}</p>}
  </div>;
}

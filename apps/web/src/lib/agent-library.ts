import type { Project } from '@scribe/shared';
import { api } from './api';
import { API_URL, supabase } from './supabase';
import { useAuthStore } from '../stores/auth';
export interface AgentPreferences { memoryEnabled: boolean; skillsEnabled: boolean; diagnosticsEnabled: boolean; searchEnabled: boolean }
export interface AgentSkill { id: string; name: string; description: string; enabled: boolean; updatedAt: string }
export interface AgentSkillDetail extends AgentSkill { content: string; untrusted: boolean; permissionsGranted: boolean }
export interface AgentMemory { content: string; revision: number }
export interface AgentSearchConfig { provider: 'doubao' | 'deepseek'; endpoint: string; hasApiKey: boolean; updatedAt: string }
export interface AgentSearchState { config: AgentSearchConfig | null; fallback: { provider: 'deepseek'; available: boolean } }
export interface AgentCapabilities {
  limits?: { modelRounds?: number; requestSeconds?: number; readCalls?: number; diagnosticCalls?: number };
  skills?: { available: boolean }; memory?: { available: boolean };
  diagnostics?: { available: boolean; configured?: boolean; ready?: boolean; health?: { ready: boolean; status: 'ready' | 'unavailable' | 'not_configured' }; tasks?: string[]; reason?: string };
  search?: { available: boolean; provider?: string; scope?: string; reason?: string };
  preferences?: AgentPreferences;
}
export class AgentLibraryError extends Error { constructor(readonly status: number, message: string) { super(message); } }
export const agentQueryKey = (userId: string, kind: string, resource = '') => ['agent-library', userId, kind, resource] as const;
function checkOwner(userId: string) { if (useAuthStore.getState().user?.id !== userId) throw new AgentLibraryError(401, 'Account changed. Reopen Agent settings.'); }
async function request<T>(userId: string, path: string, method = 'GET', body?: unknown): Promise<T> {
  checkOwner(userId);
  const { data: { session } } = await supabase.auth.getSession();
  if (!session || session.user.id !== userId) throw new AgentLibraryError(401, 'Sign in again to continue.');
  const response = await fetch(`${API_URL}${path}`, { method, cache: 'no-store', headers: { Authorization: `Bearer ${session.access_token}`, 'Content-Type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  checkOwner(userId);
  if (response.status === 204) return undefined as T;
  let value: unknown;
  try { value = await response.json(); } catch { throw new AgentLibraryError(response.status, `Agent service returned an invalid response (${response.status}).`); }
  checkOwner(userId);
  if (!response.ok) { const error = value && typeof value === 'object' ? value as { message?: string; error?: { message?: string } } : {}; throw new AgentLibraryError(response.status, error.message ?? error.error?.message ?? `Agent request failed (${response.status}).`); }
  return value as T;
}
export const agentLibraryApi = {
  searchConfig: (userId: string) => request<AgentSearchState>(userId, '/api/agent/search-config'),
  saveSearchConfig: (userId: string, body: { provider: 'doubao' | 'deepseek'; endpoint: string; apiKey: string }) => request<AgentSearchState>(userId, '/api/agent/search-config', 'PUT', body),
  clearSearchConfig: (userId: string) => request<{ ok: boolean }>(userId, '/api/agent/search-config', 'DELETE'),
  preferences: (userId: string) => request<AgentPreferences>(userId, '/api/agent/preferences'),
  setPreferences: (userId: string, body: AgentPreferences) => request<AgentPreferences>(userId, '/api/agent/preferences', 'PUT', body),
  capabilities: (userId: string) => request<AgentCapabilities>(userId, '/api/agent/capabilities'),
  skills: (userId: string) => request<{ skills: AgentSkill[] }>(userId, '/api/agent/skills'),
  skill: (userId: string, id: string) => request<AgentSkillDetail>(userId, `/api/agent/skills/${encodeURIComponent(id)}`),
  saveSkill: (userId: string, id: string | null, content: string, enabled: boolean) => request<AgentSkillDetail>(userId, `/api/agent/skills${id ? '/' + encodeURIComponent(id) : ''}`, id ? 'PUT' : 'POST', { content, enabled }),
  archiveSkill: (userId: string, id: string) => request<{ ok: boolean }>(userId, `/api/agent/skills/${encodeURIComponent(id)}`, 'DELETE'),
  memory: (userId: string, project: string) => request<AgentMemory>(userId, `/api/projects/${encodeURIComponent(project)}/agent/memory`),
  saveMemory: (userId: string, project: string, value: AgentMemory) => request<AgentMemory>(userId, `/api/projects/${encodeURIComponent(project)}/agent/memory`, 'PUT', value),
  projects: async (userId: string): Promise<Project[]> => { checkOwner(userId); const projects = await api.projects.list(); checkOwner(userId); return projects; },
};

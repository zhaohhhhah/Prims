import { API_URL, supabase } from './supabase';

export interface RegistrationInvite {
  id: string;
  hint: string;
  label: string;
  maxUses: number;
  usedCount: number;
  expiresAt: string;
  revokedAt: string | null;
  createdAt: string;
}
export interface SiteAccount {
  id: string;
  email: string | null;
  displayName: string | null;
  createdAt: string;
  lastSignInAt: string | null;
  isAdmin: boolean;
}
async function request<T>(path: string, body?: unknown): Promise<T> {
  const { data } = await supabase.auth.getSession();
  if (!data.session) throw new Error('请先登录');
  const response = await fetch(`${API_URL}/api/admin${path}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { Authorization: `Bearer ${data.session.access_token}`, 'Content-Type': 'application/json' },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const result = (await response.json()) as T & { message?: string };
  if (!response.ok) throw new Error(result.message ?? `请求失败 (${response.status})`);
  return result;
}
export const adminApi = {
  me: () => request<{ isAdmin: boolean }>('/me'),
  users: (q = '') => request<SiteAccount[]>(`/users?q=${encodeURIComponent(q)}`),
  deletionPreview: (id: string) => request<UserDeletionImpact>(`/users/${id}/deletion-preview`),
  deleteUser: (id: string, confirmationEmail: string) => request<UserDeleted>(`/users/${id}/delete`, { confirmationEmail, permanentlyDeleteOwnedProjects: true }),
  deletionStatus: (id: string) => request<UserDeletionStatus>(`/user-deletions/${id}`),
  invites: () => request<RegistrationInvite[]>('/registration-invites'),
  create: (body: { label: string; maxUses: number; expiresDays: number }) =>
    request<{ code: string; invite: RegistrationInvite }>('/registration-invites', body),
  revoke: (id: string) => request<RegistrationInvite>(`/registration-invites/${id}/revoke`, {}),
};

export interface UserDeletionImpact {
  userId: string; email: string; ownedProjects: { id: string; name: string }[];
  attachmentCount: number; joinedProjectCount: number; activeCompileCount: number;
  canDelete: boolean; blockedReason: string | null;
}
export interface UserDeleted { deletionId: string; deleted: boolean; ownedProjectCount: number; cleanupStatus: string }
export interface UserDeletionStatus { deletionId: string; status: 'pending' | 'cleaned'; pendingScopes: number; lastCheckedAt: string | null }

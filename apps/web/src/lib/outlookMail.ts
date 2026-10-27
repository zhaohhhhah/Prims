import { supabase } from './supabase';

export interface OutlookMailConfig {
  clientId: string; clientSecretSet: boolean; mailbox: string | null;
  connected: boolean; enabled: boolean; needsReauth: boolean;
  lastError: string | null; redirectUri: string; scopes: string;
  tokenExpiresAt: number | null; hookConfigured: boolean; hookAvailable: boolean; requireConfirmation: boolean;
}
async function request<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const { data: { session } } = await supabase.auth.getSession();
  if (!session) throw new Error('请重新登录');
  const response = await fetch(`/api/admin/mail/outlook${path}`, {
    method, credentials: 'same-origin', cache: 'no-store',
    headers: { Authorization: `Bearer ${session.access_token}`, ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const data: unknown = await response.json();
  if (!response.ok) {
    const value = data as { error?: { message?: string }; message?: string };
    throw new Error(value.error?.message ?? value.message ?? '邮件操作失败');
  }
  return data as T;
}
export const outlookMailApi = {
  config: () => request<OutlookMailConfig>(''),
  save: (body: { clientId: string; clientSecret?: string; enabled: boolean }) => request<OutlookMailConfig>('', 'PUT', body),
  connect: () => request<{ authorizationUrl: string }>('/connect', 'POST'),
  disconnect: () => request<OutlookMailConfig>('/disconnect', 'POST'),
  hook: (enabled: boolean, requireConfirmation: boolean) => request<OutlookMailConfig>('/hook-config', 'POST', { enabled, requireConfirmation }),
  test: (recipient: string) => request<{ message: string }>('/test', 'POST', { recipient }),
};

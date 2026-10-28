import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';
const fixture = vi.hoisted(() => ({ target: null as string | null, email: '', ack: false, allowed: true, self: false, stateIndex: 0 }));
vi.mock('react', async () => {
  const actual = await vi.importActual<typeof import('react')>('react');
  return { ...actual, useState: () => [[ '', fixture.target, fixture.email, fixture.ack, false, null ][fixture.stateIndex++], vi.fn()] };
});
vi.mock('@scribe/ui', () => ({ Button: ({ children, variant: _v, size: _s, ...props }: any) => <button {...props}>{children}</button>, Input: (props: any) => <input {...props} />, Label: (props: any) => <label {...props} /> }));
vi.mock('../lib/admin', () => ({ adminApi: { users: vi.fn(), deletionPreview: vi.fn(), deleteUser: vi.fn(), deletionStatus: vi.fn() } }));
vi.mock('@tanstack/react-query', () => ({ useQueryClient: () => ({ invalidateQueries: vi.fn() }), useQuery: ({ queryKey }: any) => ({ isPending: false, isError: false, data: queryKey[0] === 'site-accounts' ? [{ id: fixture.self ? 'actor' : 'target', email: 'target@example.test', displayName: 'Target', isAdmin: false }] : queryKey[0] === 'user-deletion-preview' ? { email: 'target@example.test', ownedProjects: [{ id: 'owned', name: 'Owned only' }], attachmentCount: 4, joinedProjectCount: 2, canDelete: fixture.allowed, blockedReason: fixture.allowed ? null : '正在编译' } : undefined }) }));
import { AdminUserManagement } from './AdminUserManagement';
import { adminApi } from '../lib/admin';
function html() { fixture.stateIndex = 0; return renderToStaticMarkup(<AdminUserManagement userId="actor" />); }
function deleteButton(markup: string) { return markup.match(/<button[^>]*>永久删除账号及项目<\/button>/)?.[0] ?? ''; }
describe('permanent user deletion confirmation', () => {
 beforeEach(() => { Object.assign(fixture, { target: 'target', email: '', ack: false, allowed: true, self: false, stateIndex: 0 }); vi.clearAllMocks(); });
 it('shows exact owned impact and explicitly preserves other owners projects', () => { const page = html(); expect(page).toContain('Owned only'); expect(page).toContain('4 个存储附件'); expect(page).toContain('其他 2 个项目会保留'); expect(deleteButton(page)).toContain('disabled'); expect(adminApi.deleteUser).not.toHaveBeenCalled(); });
 it('requires both exact target email and explicit acknowledgement', () => { fixture.email = 'target@example.test'; expect(deleteButton(html())).toContain('disabled'); fixture.ack = true; expect(deleteButton(html())).not.toContain('disabled'); fixture.email = 'other@example.test'; expect(deleteButton(html())).toContain('disabled'); });
 it('keeps the action blocked while compile or server guard blocks deletion', () => { fixture.email = 'target@example.test'; fixture.ack = true; fixture.allowed = false; const page = html(); expect(deleteButton(page)).toContain('disabled'); expect(page).toContain('正在编译'); });
 it('has no confirmation dialog until an account is selected', () => { fixture.target = null; expect(html()).not.toContain('role="dialog"'); });
 it('disables selecting the currently logged in administrator', () => { fixture.self = true; expect(html()).toMatch(/<button[^>]*disabled=""[^>]*>当前账号<\/button>/); });
});

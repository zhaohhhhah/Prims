import { Button, Input, Label } from '@scribe/ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';
import { adminApi } from '../lib/admin';

const date = (value: string | null) => value ? new Date(value).toLocaleString() : '—';
export function AdminUserManagement({ userId }: { userId: string }) {
  const cache = useQueryClient();
  const [search, setSearch] = useState('');
  const [target, setTarget] = useState<string | null>(null);
  const [confirmation, setConfirmation] = useState('');
  const [acknowledged, setAcknowledged] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deletionId, setDeletionId] = useState<string | null>(null);
  const accounts = useQuery({ queryKey: ['site-accounts', userId, search], queryFn: () => adminApi.users(search) });
  const preview = useQuery({ queryKey: ['user-deletion-preview', userId, target], queryFn: () => adminApi.deletionPreview(target!), enabled: !!target, staleTime: 0 });
  const cleanup = useQuery({ queryKey: ['user-deletion-cleanup', userId, deletionId], queryFn: () => adminApi.deletionStatus(deletionId!), enabled: !!deletionId, refetchInterval: query => query.state.data?.status === 'cleaned' ? false : 2500 });
  const impact = preview.data;
  const ready = impact?.canDelete === true && confirmation.trim() === impact.email && acknowledged && !deleting;
  async function remove() {
    if (!target || !ready) return;
    setDeleting(true);
    try {
      const result = await adminApi.deleteUser(target, confirmation.trim());
      setDeletionId(result.deletionId); setTarget(null); setConfirmation(''); setAcknowledged(false);
      await cache.invalidateQueries({ queryKey: ['site-accounts', userId] });
      toast.success('账号及其名下项目已永久删除，正在清理附件');
    } catch (error) {
      toast.error(error instanceof Error ? error.message : '删除失败，请重试');
      await preview.refetch();
    } finally { setDeleting(false); }
  }
  function choose(id: string | null) { setTarget(id); setConfirmation(''); setAcknowledged(false); }
  return <section className="space-y-4">
    <div className="flex flex-wrap items-center justify-between gap-3"><h2 className="text-lg font-medium">用户管理 · 账号列表</h2><Input aria-label="搜索用户邮箱或名称" placeholder="搜索邮箱或名称" value={search} maxLength={200} onChange={event => setSearch(event.target.value)} className="max-w-sm" /></div>
    <p className="text-sm text-muted-foreground">永久删除账号会同时删除其名下项目和个人 AI 设置及对话。其参与的其他人的项目会保留。</p>
    {deletionId && <div role="status" className="rounded-md border p-3 text-sm">{cleanup.data?.status === 'cleaned' ? '账号、项目及附件清理已完成。' : cleanup.isError ? '账号已删除，暂时无法读取附件清理进度；后台会持续重试。' : '账号及项目已删除，附件正在后台清理；失败时自动重试。'}</div>}
    {accounts.isPending ? <p>加载中…</p> : accounts.isError ? <p role="alert" className="text-destructive">{accounts.error.message}</p> : <div className="overflow-x-auto rounded-lg border"><table className="w-full whitespace-nowrap text-left text-sm"><thead className="bg-muted"><tr>{['邮箱', '名称', '权限', '注册时间', '最近登录', '操作'].map(text => <th className="p-3 font-medium" key={text}>{text}</th>)}</tr></thead><tbody>
      {accounts.data?.length === 0 && <tr><td colSpan={6} className="p-4 text-muted-foreground">没有匹配的用户。</td></tr>}
      {accounts.data?.map(account => <tr className="border-t" key={account.id}><td className="p-3">{account.email}</td><td className="p-3">{account.displayName || '—'}</td><td className="p-3">{account.isAdmin ? '管理员' : '成员'}</td><td className="p-3">{date(account.createdAt)}</td><td className="p-3">{date(account.lastSignInAt)}</td><td className="p-3"><Button size="sm" variant="outline" disabled={account.id === userId || deleting} onClick={() => choose(account.id)}>{account.id === userId ? '当前账号' : '永久删除…'}</Button></td></tr>)}
    </tbody></table></div>}
    {target && <div role="dialog" aria-modal="true" aria-labelledby="delete-account-title" className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"><div className="max-h-[90vh] w-full max-w-xl space-y-4 overflow-y-auto rounded-lg border bg-background p-6 shadow-xl">
      <h3 id="delete-account-title" className="text-lg font-semibold">永久删除账号及其名下项目</h3>
      {preview.isPending ? <p>正在核对删除影响…</p> : preview.isError ? <p role="alert" className="text-destructive">{preview.error.message}</p> : impact && <>
        <p className="break-all text-sm">目标账号：<strong>{impact.email}</strong></p>
        <p className="text-sm">将删除 {impact.ownedProjects.length} 个名下项目、{impact.attachmentCount} 个存储附件，以及个人设置、AI 连接和对话。此操作无法撤销。</p>
        {impact.ownedProjects.length > 0 && <ul className="max-h-40 list-disc overflow-y-auto pl-5 text-sm">{impact.ownedProjects.map(project => <li key={project.id}>{project.name}</li>)}</ul>}
        <p className="text-sm text-muted-foreground">其参与的其他 {impact.joinedProjectCount} 个项目会保留，移除此账号的成员身份及个人关联记录。常规运维备份按原有保留策略管理。</p>
        {impact.blockedReason && <p role="alert" className="text-destructive">{impact.blockedReason}</p>}
        <div className="space-y-2"><Label htmlFor="delete-user-email">输入目标邮箱以确认</Label><Input id="delete-user-email" autoComplete="off" value={confirmation} onChange={event => setConfirmation(event.target.value)} disabled={deleting || !impact.canDelete} /></div>
        <label className="flex items-start gap-2 text-sm"><input type="checkbox" checked={acknowledged} onChange={event => setAcknowledged(event.target.checked)} disabled={deleting || !impact.canDelete} /><span>我确认永久删除该账号及其名下所有项目、附件和个人 AI 数据。</span></label>
      </>}
      <div className="flex justify-end gap-3"><Button variant="outline" onClick={() => choose(null)} disabled={deleting}>取消</Button><Button variant="destructive" disabled={!ready} onClick={() => { void remove(); }}>{deleting ? '正在删除…' : '永久删除账号及项目'}</Button></div>
    </div></div>}
  </section>;
}

import { Button, Input, Label } from '@scribe/ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { toast } from 'sonner';

import { SiteMailSettings } from '../components/SiteMailSettings';

import { useDocumentTitle } from '../hooks/useDocumentTitle';
import { adminApi, type RegistrationInvite } from '../lib/admin';
import { useAuthStore } from '../stores/auth';

const date = (value: string | null) => value ? new Date(value).toLocaleString() : '—';
function status(invite: RegistrationInvite) {
  if (invite.revokedAt) return '已停用';
  if (new Date(invite.expiresAt).getTime() <= Date.now()) return '已过期';
  if (invite.usedCount >= invite.maxUses) return '已用完';
  return '可使用';
}
export function AdminPage({ embedded = false }: { embedded?: boolean } = {}) {
  useDocumentTitle(embedded ? '设置 · 站点管理' : '站点管理');
  const userId = useAuthStore((state) => state.user?.id);
  const client = useQueryClient();
  const access = useQuery({ queryKey: ['admin-access', userId], queryFn: adminApi.me, enabled: !!userId });
  const enabled = access.data?.isAdmin === true;
  const invites = useQuery({ queryKey: ['registration-invites', userId], queryFn: adminApi.invites, enabled });
  const accounts = useQuery({ queryKey: ['site-accounts', userId], queryFn: adminApi.users, enabled });
  const [label, setLabel] = useState('');
  const [maxUses, setMaxUses] = useState(1);
  const [expiresDays, setExpiresDays] = useState(7);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [revoking, setRevoking] = useState<string | null>(null);
  async function create(event: FormEvent) {
    event.preventDefault(); setBusy(true);
    try {
      const created = await adminApi.create({ label, maxUses, expiresDays });
      setCode(created.code);
      await client.invalidateQueries({ queryKey: ['registration-invites', userId] });
      toast.success('邀请码已生成');
    } catch (error) { toast.error(error instanceof Error ? error.message : '生成失败'); }
    finally { setBusy(false); }
  }
  async function revoke(id: string) {
    setRevoking(id);
    try {
      await adminApi.revoke(id);
      await client.invalidateQueries({ queryKey: ['registration-invites', userId] });
      toast.success('邀请码已停用');
    } catch (error) { toast.error(error instanceof Error ? error.message : '停用失败'); }
    finally { setRevoking(null); }
  }
  if (access.isPending) return <p className="p-6">正在检查管理权限…</p>;
  if (access.isError) return <p role="alert" className="p-6 text-destructive">无法检查权限：{access.error.message}</p>;
  if (!enabled) return <p className="p-6">仅管理员可访问。</p>;
  return (
    <div className={embedded ? "space-y-6" : "mx-auto max-w-6xl space-y-8 p-4 sm:p-8"}>
      {!embedded && <header><h1 className="text-2xl font-semibold">站点管理</h1><p className="mt-2 text-sm text-muted-foreground">新账号需要邀请码。已有账号可直接登录。</p></header>}
      <SiteMailSettings />
      <section className="space-y-4 rounded-lg border p-5">
        <h2 className="text-lg font-medium">生成邀请码</h2>
        <form onSubmit={(event) => { void create(event); }} className="flex flex-wrap items-end gap-4">
          <div className="min-w-48 flex-1 space-y-2"><Label htmlFor="invite-label">备注</Label><Input id="invite-label" maxLength={80} value={label} onChange={(event) => setLabel(event.target.value)} placeholder="例如：同学或团队" /></div>
          <div className="w-32 space-y-2"><Label htmlFor="invite-uses">可用次数</Label><Input id="invite-uses" type="number" required min={1} max={10000} value={maxUses} onChange={(event) => setMaxUses(Number(event.target.value))} /></div>
          <div className="w-32 space-y-2"><Label htmlFor="invite-days">有效天数</Label><Input id="invite-days" type="number" required min={1} max={365} value={expiresDays} onChange={(event) => setExpiresDays(Number(event.target.value))} /></div>
          <Button type="submit" disabled={busy}>{busy ? '生成中…' : '生成邀请码'}</Button>
        </form>
        {code && <div className="space-y-2 rounded-md bg-muted p-4"><Label htmlFor="new-invite">新邀请码（完整内容仅本次显示，请保存）</Label><div className="flex flex-wrap gap-2"><Input id="new-invite" className="min-w-64 flex-1 font-mono" readOnly value={code} /><Button variant="outline" onClick={() => { void navigator.clipboard.writeText(code).then(() => toast.success('已复制')).catch(() => toast.error('请手动选择并复制邀请码')); }}>复制</Button></div></div>}
      </section>
      <section className="space-y-4"><h2 className="text-lg font-medium">邀请码</h2>
        {invites.isPending ? <p>加载中…</p> : invites.isError ? <p role="alert" className="text-destructive">{invites.error.message}</p> : <div className="overflow-x-auto rounded-lg border"><table className="w-full whitespace-nowrap text-left text-sm"><thead className="bg-muted"><tr>{['识别码 / 备注', '状态', '已用 / 总次数', '到期时间', '操作'].map((text) => <th className="p-3 font-medium" key={text}>{text}</th>)}</tr></thead><tbody>{invites.data?.length === 0 && <tr><td colSpan={5} className="p-4 text-muted-foreground">尚未生成邀请码。</td></tr>}{invites.data?.map((invite) => <tr className="border-t" key={invite.id}><td className="p-3"><div className="font-mono">{invite.hint}</div><div className="text-muted-foreground">{invite.label || '无备注'}</div></td><td className="p-3">{status(invite)}</td><td className="p-3">{invite.usedCount} / {invite.maxUses}</td><td className="p-3">{date(invite.expiresAt)}</td><td className="p-3"><Button size="sm" variant="outline" disabled={status(invite) !== '可使用' || revoking !== null} onClick={() => { void revoke(invite.id); }}>{revoking === invite.id ? '停用中…' : '停用'}</Button></td></tr>)}</tbody></table></div>}
      </section>
      <section className="space-y-4"><h2 className="text-lg font-medium">账号列表</h2>
        {accounts.isPending ? <p>加载中…</p> : accounts.isError ? <p role="alert" className="text-destructive">{accounts.error.message}</p> : <div className="overflow-x-auto rounded-lg border"><table className="w-full whitespace-nowrap text-left text-sm"><thead className="bg-muted"><tr>{['邮箱', '名称', '权限', '注册时间', '最近登录'].map((text) => <th className="p-3 font-medium" key={text}>{text}</th>)}</tr></thead><tbody>{accounts.data?.map((account) => <tr className="border-t" key={account.id}><td className="p-3">{account.email}</td><td className="p-3">{account.displayName || '—'}</td><td className="p-3">{account.isAdmin ? '管理员' : '成员'}</td><td className="p-3">{date(account.createdAt)}</td><td className="p-3">{date(account.lastSignInAt)}</td></tr>)}</tbody></table></div>}
      </section>
    </div>
  );
}

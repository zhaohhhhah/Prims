import { Button, Input, Label } from '@scribe/ui';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState, type FormEvent } from 'react';
import { toast } from 'sonner';

import { smtpMailApi, type SMTPDraft as MailDraft } from '../lib/smtpMail';
import { useAuthStore } from '../stores/auth';

const defaults: MailDraft = { enabled: false, host: 'smtp.qq.com', port: 465, username: '', password: '', senderEmail: '', senderName: 'Prims', requireConfirmation: false };
export function SMTPMailSettings() {
  const userId = useAuthStore((state) => state.user?.id);
  const query = useQuery({ queryKey: ['mail-config', userId], queryFn: smtpMailApi.config });
  const [draft, setDraft] = useState<MailDraft>(defaults);
  const [recipient, setRecipient] = useState('');
  const [busy, setBusy] = useState<'save' | 'test' | null>(null);
  const [message, setMessage] = useState('');
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    if (!query.data || dirty) return;
    const { passwordSet: _passwordSet, updatedAt: _updatedAt, activeChannel: _activeChannel, ...config } = query.data;
    setDraft({ ...config, host: config.host || 'smtp.qq.com', port: config.host ? config.port : 465, password: '', clearPassword: false });
  }, [query.data, dirty]);
  function change<K extends keyof MailDraft>(key: K, value: MailDraft[K]) {
    setDirty(true);setMessage('');setDraft((old) => ({ ...old, [key]: value }));
  }
  async function save(event: FormEvent) {
    event.preventDefault();setBusy('save');setMessage('');
    try {
      const result = await smtpMailApi.save(draft);
      const { passwordSet: _passwordSet, updatedAt: _updatedAt, activeChannel: _activeChannel, ...config } = result;
      setDraft({ ...config, host: config.host || 'smtp.qq.com', port: config.host ? config.port : 465, password: '', clearPassword: false });setDirty(false);
      await query.refetch();setMessage('邮件配置已保存并生效。');toast.success('邮件配置已保存');
    } catch (error) { setMessage(error instanceof Error ? error.message : '保存失败'); }
    finally { setBusy(null); }
  }
  async function test() {
    setBusy('test');setMessage('');
    try { setMessage((await smtpMailApi.test({ ...draft, recipient })).message); }
    catch (error) { setMessage(error instanceof Error ? error.message : '测试失败'); }
    finally { setBusy(null); }
  }
  if (query.isPending) return <section className="rounded-lg border p-5">邮件配置加载中…</section>;
  if (query.isError) return <section className="rounded-lg border p-5"><p role="alert" className="text-destructive">{query.error.message}</p><Button variant="outline" onClick={() => { void query.refetch(); }}>重试</Button></section>;
  return <section className="space-y-5 rounded-lg border p-5">
    <div><h2 className="text-lg font-medium">SMTP 发信 · QQ 邮箱等</h2><p className="mt-1 text-sm text-muted-foreground">SMTP 用于密码重置、邮件登录和注册验证。配置邮箱供应商的发信服务，无需 IMAP。</p></div>
    <form onSubmit={(event) => { void save(event); }} className="space-y-4">
      <div className="rounded-md bg-muted p-3 text-sm">当前发信通道：{query.data?.activeChannel === 'smtp' ? 'SMTP' : query.data?.activeChannel === 'outlook' ? 'Microsoft Outlook' : '尚未启用'}。启用 SMTP 后会停用 Outlook Hook，两套配置保留。</div>
      <div className="flex flex-wrap gap-2"><Button type="button" size="sm" variant="outline" onClick={() => { change('host', 'smtp.qq.com');change('port', 465); }}>QQ 邮箱预设</Button><p className="text-xs text-muted-foreground">在 QQ 邮箱设置 → 账号中开启 SMTP 并生成授权码。填写完整 QQ 邮箱及授权码，非 QQ 登录密码。</p></div>
      <Label className="flex items-center gap-2"><input type="checkbox" checked={draft.enabled} onChange={(event) => { change('enabled', event.target.checked);if (!event.target.checked) change('requireConfirmation', false); }} />启用邮件发送</Label>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-2"><Label htmlFor="smtp-host">SMTP 服务器</Label><Input id="smtp-host" autoComplete="off" placeholder="smtp.example.com" value={draft.host} onChange={(event) => change('host', event.target.value)} maxLength={253} required={draft.enabled} /></div>
        <div className="space-y-2"><Label htmlFor="smtp-port">端口</Label><Input id="smtp-port" type="number" min={1} max={65535} value={draft.port} onChange={(event) => change('port', Number(event.target.value))} required /><p className="text-xs text-muted-foreground">465 使用 SSL/TLS；587 及其他端口使用 STARTTLS，校验服务器证书。</p></div>
        <div className="space-y-2"><Label htmlFor="smtp-username">SMTP 账号</Label><Input id="smtp-username" autoComplete="off" placeholder="123456@qq.com" value={draft.username} onChange={(event) => { change('username', event.target.value);if (!draft.senderEmail || draft.senderEmail === draft.username) change('senderEmail', event.target.value); }} maxLength={320} /></div>
        <div className="space-y-2"><Label htmlFor="smtp-password">密码／邮箱授权码</Label><Input id="smtp-password" type="password" autoComplete="new-password" value={draft.password ?? ''} onChange={(event) => { change('password', event.target.value);change('clearPassword', false); }} placeholder={query.data?.passwordSet ? '已保存，留空保持原授权码' : '填写 SMTP 密码或邮箱授权码'} maxLength={4096} /><p className="text-xs text-muted-foreground">保存后不回显。QQ、163 等邮箱通常需要单独申请 SMTP 授权码。</p>{query.data?.passwordSet && <Label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={draft.clearPassword ?? false} onChange={(event) => { change('clearPassword', event.target.checked);if (event.target.checked) change('password', ''); }} />清除已保存的授权码</Label>}</div>
        <div className="space-y-2"><Label htmlFor="smtp-sender">发件邮箱</Label><Input id="smtp-sender" type="email" placeholder="123456@qq.com" value={draft.senderEmail} onChange={(event) => change('senderEmail', event.target.value)} maxLength={320} required={draft.enabled} /></div>
        <div className="space-y-2"><Label htmlFor="smtp-name">发件人名称</Label><Input id="smtp-name" value={draft.senderName} onChange={(event) => change('senderName', event.target.value)} maxLength={80} /></div>
      </div>
      <div className="space-y-2"><Label className="flex items-center gap-2"><input type="checkbox" disabled={!draft.enabled} checked={draft.requireConfirmation} onChange={(event) => change('requireConfirmation', event.target.checked)} />新注册账号需要验证邮箱</Label><p className="text-xs text-muted-foreground">邀请码仍然必填，已有账号照常使用。启用前请先发送测试邮件确认能收到。</p></div>
      <div className="flex flex-wrap items-center gap-3"><Button type="submit" disabled={busy !== null}>{busy === 'save' ? '正在验证并应用…' : '保存并应用'}</Button><span className="text-xs text-muted-foreground">应用时仅重新加载认证服务，编辑与编译服务保持运行。</span></div>
    </form>
    <div className="flex flex-wrap items-end gap-3 border-t pt-4"><div className="min-w-56 flex-1 space-y-2"><Label htmlFor="smtp-test">测试收件邮箱</Label><Input id="smtp-test" type="email" placeholder="填写你能收信的邮箱" value={recipient} onChange={(event) => setRecipient(event.target.value)} /></div><Button variant="outline" disabled={busy !== null || !recipient} onClick={() => { void test(); }}>{busy === 'test' ? '发送中…' : '发送测试邮件'}</Button></div>
    <p className="text-xs text-muted-foreground">测试使用当前填写的配置，不会自动保存；成功代表 SMTP 服务器已接受邮件，最终送达请检查邮箱。</p>
    {message && <p role="status" className="rounded-md bg-muted p-3 text-sm">{message}</p>}
  </section>;
}

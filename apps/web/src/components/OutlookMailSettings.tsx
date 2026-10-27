import { Button, Input, Label } from '@scribe/ui';
import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { outlookMailApi, type OutlookMailConfig } from '../lib/outlookMail';

export function OutlookMailSettings() {
  const [config, setConfig] = useState<OutlookMailConfig | null>(null);
  const [clientId, setClientId] = useState('');
  const [secret, setSecret] = useState('');
  const [enabled, setEnabled] = useState(false);
  const [recipient, setRecipient] = useState('');
  const [hookEnabled, setHookEnabled] = useState(false);
  const [confirmation, setConfirmation] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  function adopt(value: OutlookMailConfig) { setConfig(value); setClientId(value.clientId); setEnabled(value.enabled); setSecret(''); setHookEnabled(value.hookConfigured); setConfirmation(value.requireConfirmation); }
  useEffect(() => {
    let active = true;
    void outlookMailApi.config().then(value => { if (active) adopt(value); }).catch((e: unknown) => { if (active) setError(e instanceof Error ? e.message : '加载邮件设置失败'); });
    const status = new URLSearchParams(window.location.search).get('outlook');
    if (status === 'connected') toast.success('Outlook 已连接。请先发送测试邮件，再启用发信。');
    if (status === 'failed') toast.error('Outlook 授权未完成，请检查应用设置后重新连接。已有登录方式保持可用。');
    return () => { active = false; };
  }, []);
  async function run(action: () => Promise<void>) {
    setBusy(true); setError('');
    try { await action(); } catch (e) { setError(e instanceof Error ? e.message : '操作失败'); }
    finally { setBusy(false); }
  }
  const unsupportedCallback = config !== null && new URL(config.redirectUri).hostname.split('.').some(part => part.startsWith('xn--'));
  const dirty = config !== null && (clientId.trim() !== config.clientId || secret !== '' || enabled !== config.enabled);
  return <section className="space-y-4 rounded-lg border p-5" aria-labelledby="outlook-mail-title">
    <h2 id="outlook-mail-title" className="text-lg font-medium">站点邮件 · 个人 Outlook</h2>
    <p className="text-sm text-muted-foreground">用一个管理员授权的 Outlook 邮箱发送验证及找回密码邮件。每个人自己的 AI API 配置独立保留。</p>
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    {!config ? <p className="text-sm">{error ? '邮件配置暂未就绪。' : '正在加载…'}</p> : <>
      <div className="rounded-md bg-muted p-4 text-sm space-y-2">
        <p>状态：{config.needsReauth ? '授权已失效，需要重新连接' : config.connected ? `已连接 ${config.mailbox ?? 'Outlook'}` : '等待配置与授权'} · {config.enabled ? '已允许发信' : '发信关闭'}</p>
        <p>认证邮件接线：{config.hookConfigured ? '已配置 Hook' : '尚未启用 Hook（需服务器管理员安装接线后生效）'}。现有账号可继续密码登录。</p>
        {config.lastError && <p className="text-destructive">{config.lastError}</p>}
      </div>
      {unsupportedCallback && <p role="status" className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-sm">邮件接入暂缓：Microsoft Entra 不支持中文域名（包括其 punycode 形式）作为 OAuth 回调地址。需要先配置一个纯英文、非 IDN 域名；当前连接按钮已停用。AI 共写和现有密码登录可继续使用。</p>}
      <details className="text-sm space-y-3"><summary className="cursor-pointer font-medium">第一次配置：注册微软应用</summary>
        <p className="mt-3">需要能访问 Microsoft Entra 租户，并具有应用注册权限；只有个人 Outlook 账号不一定能创建应用。<a href="https://entra.microsoft.com/" target="_blank" rel="noreferrer" className="underline">打开 Entra 管理中心</a>，注册应用时选择“个人 Microsoft 帐户”或“任何组织目录及个人帐户”。</p>
        <p>添加 Web 平台（不是 SPA），使用管理员配置的纯英文、非 IDN 回调地址（当前中文域名地址不能登记）。创建 Client Secret，保存它的“值”，再在下方填写。</p>
        <Label htmlFor="outlook-callback">当前回调 URI{unsupportedCallback ? "（待更换英文域名）" : ""}</Label><Input id="outlook-callback" readOnly value={config.redirectUri} className="font-mono text-xs" />
        <p>最小授权范围：<code className="break-all">{config.scopes}</code>。不申请读取邮件权限。应用 Secret 与令牌使用服务器现有密钥加密，不会回显。</p>
      </details>
      <form className="space-y-4" onSubmit={e => { e.preventDefault(); void run(async () => { adopt(await outlookMailApi.save({ clientId: clientId.trim(), ...(secret ? { clientSecret: secret } : {}), enabled })); toast.success('邮件配置已保存'); }); }}>
        <div className="space-y-2"><Label htmlFor="outlook-client-id">Application (client) ID</Label><Input id="outlook-client-id" value={clientId} onChange={e => setClientId(e.target.value)} maxLength={40} placeholder="微软应用的 UUID" autoComplete="off" /></div>
        <div className="space-y-2"><Label htmlFor="outlook-secret">Client Secret</Label><Input id="outlook-secret" type="password" value={secret} onChange={e => setSecret(e.target.value)} autoComplete="new-password" maxLength={4096} placeholder={config.clientSecretSet ? '已保存，留空保留；修改 Client ID 后需重新填写' : '填写 Secret 的值'} /></div>
        <Label className="flex gap-2 items-center"><input type="checkbox" checked={enabled} disabled={!config.connected} onChange={e => setEnabled(e.target.checked)} />允许 Outlook 发信</Label>
        <p className="text-xs text-muted-foreground">此开关控制邮件通道，不会自动要求现有用户验证邮箱。正式接入认证 Hook 后，注册及找回密码邮件才会经此发送。</p>
        <Button type="submit" disabled={busy || !dirty}>保存配置</Button>
      </form>
      <div className="flex flex-wrap gap-2"><Button variant="outline" disabled={busy || dirty || unsupportedCallback || !config.clientId || !config.clientSecretSet} onClick={() => { void run(async () => { const result = await outlookMailApi.connect(); window.location.assign(result.authorizationUrl); }); }}>{config.connected ? '重新授权 Outlook' : '连接并授权 Outlook'}</Button>
        {config.connected && <Button variant="outline" disabled={busy} onClick={() => { void run(async () => { adopt(await outlookMailApi.disconnect()); toast.success('已断开邮箱连接'); }); }}>断开连接并清除本站令牌</Button>}
      </div>
      {dirty && <p className="text-xs text-muted-foreground">保存配置后才能授权。更换 Client ID 会断开原邮箱。</p>}
      {config.hookAvailable && <form className="space-y-3 border-t pt-4" onSubmit={e => { e.preventDefault(); void run(async () => { adopt(await outlookMailApi.hook(hookEnabled, confirmation)); toast.success('认证邮件接线已更新'); }); }}>
        <h3 className="font-medium">认证邮件接线</h3>
        <Label className="flex gap-2 items-center"><input type="checkbox" checked={hookEnabled} onChange={e => { setHookEnabled(e.target.checked); if (!e.target.checked) setConfirmation(false); }} disabled={busy || (!config.enabled && !config.hookConfigured)} />将注册／找回密码邮件交给 Outlook</Label>
        <Label className="flex gap-2 items-center"><input type="checkbox" checked={confirmation} onChange={e => setConfirmation(e.target.checked)} disabled={!hookEnabled || busy} />新注册账号必须验证邮箱（默认关闭）</Label>
        <p className="text-xs text-muted-foreground">请先验证测试邮件能收到。应用此设置会仅重载认证服务，已有项目和密码保持不变。</p>
        <Button variant="outline" type="submit" disabled={busy || (hookEnabled === config.hookConfigured && confirmation === config.requireConfirmation)}>应用认证邮件接线</Button>
      </form>}
      <form className="space-y-2 border-t pt-4" onSubmit={e => { e.preventDefault(); void run(async () => { const value = await outlookMailApi.test(recipient.trim()); toast.success(value.message); }); }}>
        <Label htmlFor="outlook-recipient">测试收件地址</Label><div className="flex flex-wrap gap-2"><Input id="outlook-recipient" type="email" required value={recipient} onChange={e => setRecipient(e.target.value)} placeholder="由管理员指定收件人" className="min-w-48 flex-1" /><Button type="submit" variant="outline" disabled={busy || !config.connected || dirty || !recipient.trim()}>发送一封测试邮件</Button></div>
        <p className="text-xs text-muted-foreground">点击后会实际发送。微软接受请求后，请检查收件箱及垃圾邮件；接受请求不代表最终投递成功。</p>
      </form>
    </>}
  </section>;
}

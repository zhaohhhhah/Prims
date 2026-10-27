import { Button, Input, Label } from '@scribe/ui';
import { useEffect, useState, type FormEvent } from 'react';
import { Link } from 'react-router-dom';
import { supabase } from '../../lib/supabase';
import { AuthCardLayout } from './AuthCardLayout';

export function ResetPasswordPage() {
  const [ready, setReady] = useState(false);
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  useEffect(() => { let active = true; void supabase.auth.getSession().then(({ data: { session } }) => { if (!active) return; if (session) setReady(true); else setError('重置链接已失效，请重新申请。'); }); return () => { active = false; }; }, []);
  async function submit(event: FormEvent) {
    event.preventDefault(); setError('');
    if (password.length < 8) { setError('密码至少需要 8 位。'); return; }
    if (password !== confirm) { setError('两次输入的密码不一致。'); return; }
    setBusy(true);
    try { const { error: failure } = await supabase.auth.updateUser({ password }); if (failure) throw failure; setPassword(''); setConfirm(''); setDone(true); }
    catch (failure) { setError(failure instanceof Error ? failure.message : '密码更新失败'); }
    finally { setBusy(false); }
  }
  return <AuthCardLayout title="设置新密码" footer={<Link to={done ? '/dashboard' : '/login'} className="text-primary underline">{done ? '返回项目' : '返回登录'}</Link>}>
    {error && <p role="alert" className="mb-4 text-sm text-destructive">{error}</p>}
    {done ? <p>密码已更新，请保存好新密码。</p> : ready ? <form onSubmit={e => { void submit(e); }} className="space-y-4">
      <div className="space-y-2"><Label htmlFor="new-password">新密码</Label><Input id="new-password" type="password" required minLength={8} autoComplete="new-password" value={password} onChange={e => setPassword(e.target.value)} /></div>
      <div className="space-y-2"><Label htmlFor="confirm-password">再次输入新密码</Label><Input id="confirm-password" type="password" required minLength={8} autoComplete="new-password" value={confirm} onChange={e => setConfirm(e.target.value)} /></div>
      <Button type="submit" disabled={busy} className="w-full">{busy ? '正在更新…' : '保存新密码'}</Button>
    </form> : !error && <p>正在检查重置链接…</p>}
  </AuthCardLayout>;
}

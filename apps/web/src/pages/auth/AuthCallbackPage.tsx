import { Button } from '@scribe/ui';
import { Loader2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';
import { completeAuthCallback } from '../../lib/auth-callback';
import { supabase } from '../../lib/supabase';
import { AuthCardLayout } from './AuthCardLayout';
export function AuthCallbackPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [href] = useState(() => window.location.href);
  const pending = useRef<Promise<string> | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    if (!pending.current) {
      // Remove emailed proof from history/referrers; retain it only in this one-shot request.
      window.history.replaceState(window.history.state, '', window.location.pathname);
      pending.current = completeAuthCallback(supabase.auth, href);
    }
    void pending.current.then(destination => { if (active) void navigate(destination, { replace: true }); })
      .catch(error => { if (active) setErrorMessage(error instanceof Error ? error.message : '验证未完成，请重新申请链接。'); });
    return () => { active = false; };
  }, [href, navigate]);
  return <AuthCardLayout title={t('auth.verifyingSession')}>
    {errorMessage === null ? <div className="flex items-center justify-center py-8"><Loader2 className="h-8 w-8 animate-spin text-muted-foreground" aria-hidden="true"/><span className="sr-only">{t('common.loading')}</span></div> : <div className="space-y-4"><p role="alert" className="text-sm text-destructive">{errorMessage}</p><Button asChild className="w-full"><Link to="/forgot">重新申请重置邮件</Link></Button><Link to="/login" className="block text-center text-sm underline">返回登录</Link></div>}
  </AuthCardLayout>;
}

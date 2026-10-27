import { zodResolver } from '@hookform/resolvers/zod';
import { Button, Input, Label } from '@scribe/ui';
import { Loader2 } from 'lucide-react';
import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import { z } from 'zod';

import { supabase } from '../../lib/supabase';

import { AuthCardLayout } from './AuthCardLayout';

const schema = z.object({ email: z.string().email() });
type Values = z.infer<typeof schema>;

export function ForgotPasswordPage() {
  const { t } = useTranslation();
  const [sent, setSent] = useState(false);
  const [sendError, setSendError] = useState('');
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { email: '' },
  });

  const onSubmit = async (values: Values) => {
    setSendError('');
    try {
      const { error } = await supabase.auth.resetPasswordForEmail(values.email, {
        redirectTo: `${window.location.origin}/auth/callback?next=/auth/reset-password`,
      });
      if (error) throw error;
      setSent(true);
    } catch (error) { setSendError(error instanceof Error ? error.message : '重置邮件发送失败，请稍后重试'); }
  };

  return (
    <AuthCardLayout
      title={t('auth.forgotPassword')}
      footer={
        <Link to="/login" className="font-medium text-primary hover:underline">
          {t('common.back')}
        </Link>
      }
    >
      {sendError && <p role="alert" className="mb-4 text-sm text-destructive">{sendError}</p>}
      {sent ? (
        <p className="text-center text-sm text-muted-foreground">{t('auth.passwordResetSent')}</p>
      ) : (
        <form
          onSubmit={form.handleSubmit((values) => {
            void onSubmit(values);
          })}
          className="space-y-4"
          noValidate
        >
          <div className="space-y-2">
            <Label htmlFor="email">{t('auth.email')}</Label>
            <Input id="email" type="email" autoComplete="email" {...form.register('email')} />
            {form.formState.errors.email !== undefined && (
              <p className="text-xs text-destructive">{form.formState.errors.email.message}</p>
            )}
          </div>
          <Button type="submit" className="w-full" disabled={form.formState.isSubmitting}>
            {form.formState.isSubmitting && (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            )}
            {t('common.continue')}
          </Button>
        </form>
      )}
    </AuthCardLayout>
  );
}

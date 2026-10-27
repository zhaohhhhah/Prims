import {
  Button,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@scribe/ui';
import { Loader2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';

import { SUPPORTED_LANGUAGES } from '../../i18n';
import { API_URL, supabase } from '../../lib/supabase';
import { prepareAvatar } from '../../lib/avatar';
import { UserAvatar } from '../../components/UserAvatar';
import { useQueryClient } from '@tanstack/react-query';
import { useAuthStore } from '../../stores/auth';

export function ProfileSettingsTab() {
  const { t, i18n } = useTranslation();
  const user = useAuthStore((s) => s.user);
  const [displayName, setDisplayName] = useState<string>('');
  const [avatarUrl, setAvatarUrl] = useState<string>('');
  const picker = useRef<HTMLInputElement>(null);
  const queryClient = useQueryClient();
  const [uploading, setUploading] = useState(false);
  const zh = i18n.language.startsWith('zh');
  const [saving, setSaving] = useState<boolean>(false);

  useEffect(() => {
    const current = useAuthStore.getState().user;
    if (current === null) return;
    const meta = current.user_metadata;
    setDisplayName(typeof meta.display_name === 'string' ? meta.display_name : '');
    setAvatarUrl(typeof meta.avatar_url === 'string' ? meta.avatar_url : '');
  }, [user?.id]);

  async function changeAvatar(file: File | null) {
    if (!user || uploading) return;
    setUploading(true);
    try {
      const body = file ? await prepareAvatar(file) : undefined;
      const { data: { session } } = await supabase.auth.getSession();
      if (!session) throw new Error(zh ? '请重新登录' : 'Please sign in again');
      const response = await fetch(`${API_URL}/api/profile/avatar`, {
        method: file ? 'POST' : 'DELETE',
        headers: { Authorization: `Bearer ${session.access_token}`, ...(file ? { 'Content-Type': 'image/png' } : {}) },
        body: body ?? null,
      });
      if (!response.ok) throw new Error(zh ? '头像保存失败，请重试' : 'Could not save avatar. Please retry.');
      const result = file ? await response.json() as { avatarUrl: string } : { avatarUrl: '' };
      setAvatarUrl(result.avatarUrl);
      // Refresh the signed-in user's metadata without replacing any other profile or API settings.
      const { data } = await supabase.auth.getUser();
      if (data.user) useAuthStore.setState({ user: data.user });
      await queryClient.invalidateQueries({ queryKey: ['user-avatar'] });
      toast.success(zh ? '头像已保存' : 'Avatar saved');
    } catch (error) {
      const code = error instanceof Error ? error.message : '';
      toast.error(code === 'size' ? (zh ? '图片不能超过 8 MB' : 'Image must be under 8 MB') :
        code === 'type' || code === 'image' ? (zh ? '请选择有效的 JPG、PNG 或 WebP 图片' : 'Choose a valid JPG, PNG or WebP image') :
        code || (zh ? '无法读取图片，请换一张图片' : 'Could not read this image. Try another photo.'));
    } finally { setUploading(false); if (picker.current) picker.current.value = ''; }
  }

  async function save() {
    if (user === null) return;
    setSaving(true);
    try {
      const { error: authError } = await supabase.auth.updateUser({
        data: { display_name: displayName, avatar_url: avatarUrl },
      });
      if (authError !== null) throw authError;
      const { error: profileError } = await supabase
        .from('users')
        .update({ display_name: displayName, avatar_url: avatarUrl })
        .eq('id', user.id);
      if (profileError !== null) throw profileError;
      toast.success(t('settings.profile.saved'));
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t('errors.generic'));
    } finally {
      setSaving(false);
    }
  }

  return (
    <form
      className="space-y-5"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div className="space-y-2">
        <Label htmlFor="profile-email">{t('settings.profile.email')}</Label>
        <Input id="profile-email" value={user?.email ?? ''} disabled readOnly />
        <p className="text-xs text-muted-foreground">{t('settings.profile.emailHint')}</p>
      </div>
      <div className="space-y-2">
        <Label htmlFor="profile-name">{t('settings.profile.displayName')}</Label>
        <Input
          id="profile-name"
          value={displayName}
          onChange={(e) => {
            setDisplayName(e.target.value);
          }}
          placeholder={t('settings.profile.displayNamePlaceholder')}
        />
      </div>
      <div className="space-y-3">
        <Label>{zh ? '头像' : 'Avatar'}</Label>
        <div className="flex items-center gap-4">
          {user && <UserAvatar userId={user.id} name={displayName || user.email || '?'} avatarUrl={avatarUrl} className="h-16 w-16 text-xl" />}
          <div className="space-y-2">
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" disabled={uploading || saving} onClick={() => picker.current?.click()}>
                {uploading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}{zh ? '上传头像' : 'Upload photo'}
              </Button>
              <Button type="button" variant="ghost" disabled={uploading || saving || !avatarUrl} onClick={() => { void changeAvatar(null); }}>{zh ? '使用默认头像' : 'Use initials'}</Button>
            </div>
            <p className="text-xs text-muted-foreground">{zh ? 'JPG、PNG、WebP，最大 8 MB。默认使用彩色圆圈与姓名首字。' : 'JPG, PNG or WebP, up to 8 MB. Initials are shown by default.'}</p>
            <input ref={picker} type="file" accept="image/jpeg,image/png,image/webp" className="sr-only" aria-label={zh ? '选择头像图片' : 'Choose profile photo'} onChange={event => { const file = event.target.files?.[0]; if (file) void changeAvatar(file); }} />
          </div>
        </div>
      </div>
      <div className="space-y-2">
        <Label htmlFor="profile-language">{t('settings.language')}</Label>
        <Select
          value={i18n.language}
          onValueChange={(next) => {
            void i18n.changeLanguage(next);
          }}
        >
          <SelectTrigger id="profile-language" className="w-full max-w-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SUPPORTED_LANGUAGES.map((lang) => (
              <SelectItem key={lang.code} value={lang.code}>
                {lang.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">{t('settings.languageHint')}</p>
      </div>
      <Button type="submit" disabled={saving || uploading} className="gap-1.5">
        {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" /> : null}
        {t('common.save')}
      </Button>
    </form>
  );
}

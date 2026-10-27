import { Avatar, AvatarFallback, AvatarImage } from '@scribe/ui';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { avatarColor, avatarInitials } from '../lib/avatar';
import { API_URL, supabase } from '../lib/supabase';
import { useAuthStore } from '../stores/auth';

export function UserAvatar({ userId, name, avatarUrl, className = '', title }: {
  userId: string; name: string; avatarUrl?: string; className?: string; title?: string;
}) {
  const viewer = useAuthStore(state => state.user?.id);
  const ownAvatar = useAuthStore(state => state.user?.id === userId ? state.user.user_metadata.avatar_url as unknown : undefined);
  const version = typeof ownAvatar === 'string' ? ownAvatar : avatarUrl;
  const remote = typeof version === 'string' && /^https?:\/\//i.test(version) ? version : undefined;
  const photo = useQuery({
    queryKey: ['user-avatar', viewer, userId, version ?? ''],
    enabled: !!viewer && !!userId && !remote,
    staleTime: 60_000, gcTime: 5 * 60_000, retry: false,
    queryFn: async ({ signal }) => {
      const { data: { session } } = await supabase.auth.getSession();
      if (!session) return null;
      const response = await fetch(`${API_URL}/api/users/${encodeURIComponent(userId)}/avatar`, {
        headers: { Authorization: `Bearer ${session.access_token}` }, signal,
      });
      if (!response.ok || !response.headers.get('content-type')?.startsWith('image/png')) return null;
      return response.blob();
    },
  });
  const [image, setImage] = useState<string>();
  useEffect(() => {
    if (!photo.data) { setImage(undefined); return; }
    const url = URL.createObjectURL(photo.data); setImage(url);
    return () => URL.revokeObjectURL(url);
  }, [photo.data]);
  return <Avatar className={`shrink-0 ${className}`} title={title ?? name} aria-label={name}>
    <AvatarImage src={remote ?? image} alt="" className="object-cover" referrerPolicy="no-referrer" />
    <AvatarFallback className="font-semibold text-white" style={{ backgroundColor: avatarColor(userId || name), color: '#fff' }}>
      {avatarInitials(name)}
    </AvatarFallback>
  </Avatar>;
}

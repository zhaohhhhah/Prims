begin;
create table if not exists public.scribe_ai_profile_preferences(
 user_id uuid primary key references auth.users(id) on delete cascade,
 default_profile_id uuid references public.scribe_ai_model_profiles(id) on delete set null,
 default_model text check(default_model is null or char_length(default_model)<=256),
 legacy_profile_id uuid references public.scribe_ai_model_profiles(id) on delete set null,
 legacy_migrated_at timestamptz,
 migration_status text not null default 'pending',updated_at timestamptz not null default now()
);
alter table public.scribe_ai_profile_preferences enable row level security;
revoke all on public.scribe_ai_profile_preferences from public,anon,authenticated,service_role;
commit;

-- Each release creates a separate private project; old welcome markers stay untouched.
begin;
create table if not exists public.prims_tutorial_releases (
  user_id uuid not null references auth.users(id) on delete cascade,
  release_id text not null,
  project_id uuid references public.projects(id) on delete set null,
  created_at timestamptz not null default now(),
  primary key (user_id, release_id)
);
alter table public.prims_tutorial_releases enable row level security;
revoke all on public.prims_tutorial_releases from public, anon, authenticated, service_role;
comment on table public.prims_tutorial_releases is 'Per-account tutorial release markers. Deleting a tutorial leaves its marker so the same release is not recreated.';
commit;

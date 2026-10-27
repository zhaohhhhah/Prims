-- Prims extension; apply after the upstream schema migrations. See README.md.
begin;
create table if not exists public.scribe_welcome_projects (
  user_id uuid primary key references auth.users(id) on delete cascade,
  project_id uuid references public.projects(id) on delete set null,
  version integer not null default 1,
  created_at timestamptz not null default now()
);
alter table public.scribe_welcome_projects enable row level security;
revoke all on public.scribe_welcome_projects from public, anon, authenticated, service_role;
comment on table public.scribe_welcome_projects is 'Once-per-account welcome seed; keep the marker when a user deletes the project.';
commit;

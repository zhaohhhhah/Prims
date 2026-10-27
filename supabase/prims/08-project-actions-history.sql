begin;
create table if not exists public.scribe_project_directories(
 project_id uuid not null references public.projects(id) on delete cascade,
 path text not null check(octet_length(path)<=512),
 created_by uuid references auth.users(id) on delete set null,
 created_at timestamptz not null default now(),primary key(project_id,path)
);
create table if not exists public.scribe_project_action_plans(
 id uuid primary key,user_id uuid not null references auth.users(id) on delete cascade,
 project_id uuid not null references public.projects(id) on delete cascade,
 items jsonb not null,created_at timestamptz not null default now(),expires_at timestamptz not null
);
create index if not exists scribe_project_action_plans_owner on public.scribe_project_action_plans(user_id,project_id,created_at desc);
create table if not exists public.scribe_project_version_state(
 project_id uuid primary key references public.projects(id) on delete cascade,
 content_hash text not null,last_version_id uuid references public.project_versions(id) on delete set null,
 last_auto_at timestamptz,updated_at timestamptz not null default now()
);
alter table public.scribe_project_directories enable row level security;
alter table public.scribe_project_action_plans enable row level security;
alter table public.scribe_project_version_state enable row level security;
revoke all on public.scribe_project_directories,public.scribe_project_action_plans,public.scribe_project_version_state from public,anon,authenticated,service_role;
commit;
-- Enforce one logical namespace even when ordinary file APIs race with AI confirmations.
begin;
create or replace function public.scribe_guard_project_path() returns trigger language plpgsql set search_path=public as $$
begin
 perform pg_advisory_xact_lock(hashtextextended('prims-paths:'||new.project_id::text,4078243));
 if tg_table_name='project_files' then
  if exists(select 1 from public.project_files f where f.project_id=new.project_id and f.id<>new.id and (f.path=new.path or starts_with(new.path,f.path||'/') or starts_with(f.path,new.path||'/')))
    or exists(select 1 from public.scribe_project_directories d where d.project_id=new.project_id and (d.path=new.path or starts_with(d.path,new.path||'/'))) then
    raise unique_violation using message='Project path is already occupied';
  end if;
 else
  if exists(select 1 from public.project_files f where f.project_id=new.project_id and (f.path=new.path or starts_with(new.path,f.path||'/') or starts_with(f.path,new.path||'/'))) then
    raise unique_violation using message='Project directory path is already occupied';
  end if;
 end if;
 return new;
end $$;
drop trigger if exists scribe_guard_file_path on public.project_files;
create trigger scribe_guard_file_path before insert or update of path on public.project_files for each row execute function public.scribe_guard_project_path();
drop trigger if exists scribe_guard_directory_path on public.scribe_project_directories;
create trigger scribe_guard_directory_path before insert or update of path on public.scribe_project_directories for each row execute function public.scribe_guard_project_path();
commit;

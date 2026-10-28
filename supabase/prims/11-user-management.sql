-- Private, durable storage cleanup scopes survive hard deletion of an Auth user.
begin;
create table if not exists public.prims_user_deletions (
 id uuid primary key default gen_random_uuid(), target_user_id uuid not null,
 requested_by uuid not null, project_count integer not null, created_at timestamptz not null default now()
);
create table if not exists public.prims_user_deletion_scopes (
 id uuid primary key default gen_random_uuid(), deletion_id uuid not null references public.prims_user_deletions(id) on delete cascade,
 bucket text not null check(bucket in ('project-files','compile-artifacts','version-snapshots')),
 kind text not null check(kind in ('project','avatar')), scope_id uuid not null,
 status text not null default 'pending' check(status in ('pending','cleaned')),
 attempts integer not null default 0, next_attempt_at timestamptz not null default now(),
 lease_id uuid, lease_until timestamptz, last_error text, last_checked_at timestamptz,
 unique(deletion_id,bucket,kind,scope_id),
 check(kind<>'avatar' or bucket='project-files')
);
create table if not exists public.prims_user_deletion_objects (
 scope_id uuid not null references public.prims_user_deletion_scopes(id) on delete cascade,
 object_key text not null, primary key(scope_id,object_key)
);
create index if not exists prims_user_deletion_due on public.prims_user_deletion_scopes(next_attempt_at);
alter table public.prims_user_deletions enable row level security;
alter table public.prims_user_deletion_scopes enable row level security;
alter table public.prims_user_deletion_objects enable row level security;
revoke all on public.prims_user_deletions,public.prims_user_deletion_scopes,public.prims_user_deletion_objects from public,anon,authenticated,service_role;
comment on table public.prims_user_deletion_scopes is 'Private exact owned namespaces; periodically rechecked after completion to catch uploads already in flight at deletion. No email or credential retained.';
-- Collaboration snapshots use text doc IDs instead of a project FK. Prevent
-- already-in-flight persistence from recreating snapshots after project deletion.
create or replace function public.prims_yjs_live_project()
returns trigger language plpgsql security definer set search_path=pg_catalog as $$
begin
 perform id from public.projects where id=split_part(new.doc_id,'/',1)::uuid for key share;
 if not found then raise exception 'Collaboration project no longer exists' using errcode='23503';end if;
 return new;
end;$$;
revoke all on function public.prims_yjs_live_project() from public,anon,authenticated,service_role;
drop trigger if exists prims_yjs_live_project on public.yjs_updates;
create trigger prims_yjs_live_project before insert or update on public.yjs_updates for each row execute function public.prims_yjs_live_project();
-- A signed JWT alone does not prove its account still exists. This no-argument
-- function checks only the caller identity, including direct Storage requests.
create or replace function public.current_active_account()
returns boolean language sql stable security definer set search_path=pg_catalog as $$
 select exists(
  select 1 from auth.users
  where id=auth.uid() and deleted_at is null
    and (banned_until is null or banned_until<=now())
 );
$$;
revoke all on function public.current_active_account() from public,anon,authenticated,service_role;
grant execute on function public.current_active_account() to authenticated;
drop policy if exists prims_storage_active_account on storage.objects;
create policy prims_storage_active_account on storage.objects as restrictive for all to authenticated
 using(public.current_active_account()) with check(public.current_active_account());
commit;

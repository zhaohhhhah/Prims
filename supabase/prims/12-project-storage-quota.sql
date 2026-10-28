begin;
alter table public.projects add column if not exists source_bytes bigint not null default 0;
drop trigger if exists prims_protect_project_storage_counter on public.projects;
-- Lock tables while reconciling once; never count PDFs, versions or avatars.
lock table public.projects in share row exclusive mode;
lock table public.project_files in share row exclusive mode;
-- Preserve historical edit times during reconciliation only. The table lock
-- excludes concurrent writes; rollback restores trigger state on any error.
alter table public.projects disable trigger projects_set_updated_at;
update public.projects p set source_bytes=coalesce((select sum(f.size_bytes) from public.project_files f where f.project_id=p.id),0);
alter table public.projects enable trigger projects_set_updated_at;
create or replace function public.prims_project_storage_quota()
returns trigger language plpgsql security definer set search_path=pg_catalog as $$
declare project uuid; delta bigint; changed uuid;
begin
 if tg_op='UPDATE' and new.project_id<>old.project_id then
  raise exception 'Moving files between projects is not supported' using errcode='23514';
 end if;
 if tg_op='DELETE' then project:=old.project_id;delta:=-old.size_bytes;
 elsif tg_op='INSERT' then project:=new.project_id;delta:=new.size_bytes;
 else project:=new.project_id;delta:=new.size_bytes-old.size_bytes;end if;
 if delta<>0 then
  update public.projects set source_bytes=greatest(0,source_bytes+delta)
   where id=project and (delta<=0 or source_bytes+delta<=524288000) returning id into changed;
  if changed is null and tg_op<>'DELETE' then
   raise exception 'Project source files and attachments exceed 500MiB quota' using errcode='P5001';
  end if;
 end if;
 if tg_op='DELETE' then return old;else return new;end if;
end;$$;
revoke all on function public.prims_project_storage_quota() from public,anon,authenticated,service_role;
drop trigger if exists prims_project_storage_quota on public.project_files;
create trigger prims_project_storage_quota before insert or update or delete on public.project_files for each row execute function public.prims_project_storage_quota();
-- The counter is server-maintained even for direct authenticated PostgREST calls.
create or replace function public.prims_protect_project_storage_counter()
returns trigger language plpgsql set search_path=pg_catalog as $$
begin
 if tg_op='INSERT' then new.source_bytes:=0;
 elsif pg_trigger_depth()<2 and new.source_bytes is distinct from old.source_bytes then
  raise exception 'Project storage counter is server managed' using errcode='42501';
 end if;
 return new;
end;$$;
revoke all on function public.prims_protect_project_storage_counter() from public,anon,authenticated,service_role;
drop trigger if exists prims_protect_project_storage_counter on public.projects;
create trigger prims_protect_project_storage_counter before insert or update on public.projects for each row execute function public.prims_protect_project_storage_counter();
-- Legacy authenticated Storage writes skip metadata/quota. Limit only source
-- namespaces; the backend service role bypasses RLS and avatars are excluded.
drop policy if exists prims_project_upload_via_backend on storage.objects;
create policy prims_project_upload_via_backend on storage.objects as restrictive for insert to authenticated
 with check(bucket_id<>'project-files' or split_part(name,'/',1)='avatars');
drop policy if exists prims_project_overwrite_via_backend on storage.objects;
create policy prims_project_overwrite_via_backend on storage.objects as restrictive for update to authenticated
 using(bucket_id<>'project-files' or split_part(name,'/',1)='avatars')
 with check(bucket_id<>'project-files' or split_part(name,'/',1)='avatars');
-- Metadata writes must use the same quota-aware backend; otherwise an
-- authenticated client could forge size_bytes or delete metadata to free quota
-- while retaining the physical blob. Read policies remain untouched.
drop policy if exists prims_file_create_via_backend on public.project_files;
create policy prims_file_create_via_backend on public.project_files as restrictive for insert to authenticated with check(false);
drop policy if exists prims_file_update_via_backend on public.project_files;
create policy prims_file_update_via_backend on public.project_files as restrictive for update to authenticated using(false) with check(false);
drop policy if exists prims_file_delete_via_backend on public.project_files;
create policy prims_file_delete_via_backend on public.project_files as restrictive for delete to authenticated using(false);
commit;

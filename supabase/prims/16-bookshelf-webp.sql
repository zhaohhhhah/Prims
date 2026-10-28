-- Additive private WebP state. Legacy PDF fields and old clients remain valid.
begin;
alter table public.prims_pdf_previews
 add column if not exists webp_status text not null default 'queued',
 add column if not exists webp_profile text not null default 'webp-v1',
 add column if not exists page_count integer,
 add column if not exists pages jsonb not null default '[]'::jsonb,
 add column if not exists webp_manifest_key text,
 add column if not exists batch_size integer not null default 8,
 add column if not exists queued_at timestamptz not null default now(),
 add column if not exists reader_until timestamptz,
 add column if not exists reader_since timestamptz,
 add column if not exists editor_until timestamptz,
 add column if not exists editor_since timestamptz;
alter table public.prims_pdf_previews drop constraint if exists prims_webp_state;
alter table public.prims_pdf_previews add constraint prims_webp_state check(
 webp_status in ('queued','ready','failed') and batch_size between 1 and 8
 and (page_count is null or page_count between 1 and 2048)
 and jsonb_typeof(pages)='array' and jsonb_array_length(pages)<=2048
 and (webp_status<>'ready' or (page_count is not null and jsonb_array_length(pages)=page_count and webp_manifest_key is not null))
);
create table if not exists public.prims_webp_cleanup (
 project_id uuid not null, source_job_id uuid not null,
 retry_after timestamptz not null default now(), primary key(project_id,source_job_id)
);
alter table public.prims_webp_cleanup enable row level security;
revoke all on public.prims_webp_cleanup from public,anon,authenticated,service_role;
-- Add formats only; never make the private artifact bucket public.
update storage.buckets set allowed_mime_types=(select array_agg(distinct value) from unnest(allowed_mime_types||array['image/webp','application/json']) value)
 where id='compile-artifacts' and allowed_mime_types is not null;
-- Reuse the existing durable namespace collector on permanent project deletion.
-- Its periodic recheck also catches uploads already in flight at deletion.
create or replace function public.prims_queue_deleted_project_storage()
returns trigger language plpgsql security definer set search_path=pg_catalog as $$
declare event uuid;
begin
 if not exists(select 1 from public.prims_user_deletion_scopes where kind='project' and scope_id=old.id) then
  insert into public.prims_user_deletions(target_user_id,requested_by,project_count) values(old.owner_id,old.owner_id,1) returning id into event;
  insert into public.prims_user_deletion_scopes(deletion_id,bucket,kind,scope_id)
   select event,bucket,'project',old.id from unnest(array['project-files','compile-artifacts','version-snapshots']) bucket;
 end if;
 return old;
end;$$;
revoke all on function public.prims_queue_deleted_project_storage() from public,anon,authenticated,service_role;
drop trigger if exists prims_queue_deleted_project_storage on public.projects;
create trigger prims_queue_deleted_project_storage before delete on public.projects for each row execute function public.prims_queue_deleted_project_storage();
comment on column public.prims_pdf_previews.pages is 'Private completed WebP batches only; not exposed until full source-job manifest is ready.';
commit;

-- Private background derivatives; original compile PDFs and editor/download APIs are unchanged.
begin;
create table if not exists public.prims_pdf_previews (
 project_id uuid primary key references public.projects(id) on delete cascade,
 source_job_id uuid not null references public.compile_jobs(id) on delete cascade,
 status text not null check(status in ('queued','running','ready','failed')),
 artifact_key text,
 attempts integer not null default 0,
 retry_after timestamptz not null default now(),
 updated_at timestamptz not null default now(),
 constraint prims_pdf_previews_ready check(status<>'ready' or artifact_key is not null)
);
alter table public.prims_pdf_previews enable row level security;
revoke all on public.prims_pdf_previews from public,anon,authenticated,service_role;
comment on table public.prims_pdf_previews is 'Privileged app-only bookshelf PDF derivatives, keyed to exact successful compile; member checks happen before signing.';
commit;

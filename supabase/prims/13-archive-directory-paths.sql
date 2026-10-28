-- Permit a directory to contain existing files during ZIP import.
-- Keep the shared project namespace lock and reject file/dir identity or a
-- directory nested beneath an existing file. Do not rewrite historical 08.
begin;
CREATE OR REPLACE FUNCTION public.scribe_guard_project_path()
 RETURNS trigger
 LANGUAGE plpgsql
 SET search_path TO 'public'
AS $function$
begin
 perform pg_advisory_xact_lock(hashtextextended('prims-paths:'||new.project_id::text,4078243));
 if tg_table_name='project_files' then
  if exists(select 1 from public.project_files f where f.project_id=new.project_id and f.id<>new.id and (f.path=new.path or starts_with(new.path,f.path||'/') or starts_with(f.path,new.path||'/')))
    or exists(select 1 from public.scribe_project_directories d where d.project_id=new.project_id and (d.path=new.path or starts_with(d.path,new.path||'/'))) then
    raise unique_violation using message='Project path is already occupied';
  end if;
 else
  if exists(select 1 from public.project_files f where f.project_id=new.project_id and (f.path=new.path or starts_with(new.path,f.path||'/'))) then
    raise unique_violation using message='Project directory path is already occupied';
  end if;
 end if;
 return new;
end $function$;
commit;

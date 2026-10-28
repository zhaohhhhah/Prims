-- Keep historical compiler enum values readable while all new execution uses native TeX.
begin;
lock table public.projects in share row exclusive mode;
-- Preference migration must not make every document appear freshly edited.
alter table public.projects disable trigger projects_set_updated_at;
update public.projects set compiler='xelatex' where compiler='tectonic';
alter table public.projects enable trigger projects_set_updated_at;
alter table public.projects alter column compiler set default 'xelatex';
create or replace function public.prims_native_project_compiler()
returns trigger language plpgsql set search_path=pg_catalog as $$
begin
 if new.compiler::text='tectonic' then new.compiler:='xelatex'; end if;
 return new;
end;
$$;
drop trigger if exists prims_native_project_compiler on public.projects;
create trigger prims_native_project_compiler before insert or update of compiler on public.projects
for each row execute function public.prims_native_project_compiler();
commit;

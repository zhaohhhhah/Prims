-- Explicit operator-only administrator assignment. This file uses psql variables.
-- Usage: psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -v admin_email='you@example.com' -f 05-create-admin.sql
-- Register this exact intended owner account BEFORE installing 01-registration.sql.
-- Re-running for the same account is safe. No arbitrary profile metadata can grant this role.
\set ON_ERROR_STOP on
begin;
select set_config('prims.bootstrap_admin_email', btrim(:'admin_email'), true) as bootstrap_admin_email \gset

do $$
declare
  requested text := current_setting('prims.bootstrap_admin_email', true);
  candidates uuid[];
  target uuid;
begin
  if requested is null or requested = '' or length(requested) > 320 then
    raise exception 'Supply an explicit admin_email psql variable for an existing account';
  end if;
  select array_agg(id) into candidates
    from auth.users
   where email = requested and deleted_at is null;
  if coalesce(array_length(candidates, 1), 0) <> 1 then
    raise exception 'Expected exactly one active account matching admin_email exactly';
  end if;
  target := candidates[1];
  insert into public.scribe_site_admins(user_id) values(target)
    on conflict(user_id) do nothing;
end;
$$;
commit;
\unset bootstrap_admin_email

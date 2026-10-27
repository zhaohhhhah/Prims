-- Prims extension; apply after the upstream schema migrations. See README.md.
begin;
create table if not exists public.scribe_site_admins (
  user_id uuid primary key references auth.users(id) on delete cascade,
  created_at timestamptz not null default now()
);
create table if not exists public.scribe_registration_invites (
  id uuid primary key default gen_random_uuid(),
  code_hash text not null unique,
  hint text not null,
  label text not null default '',
  max_uses integer not null check (max_uses between 1 and 10000),
  used_count integer not null default 0 check (used_count >= 0),
  expires_at timestamptz not null,
  revoked_at timestamptz,
  created_at timestamptz not null default now(),
  last_used_at timestamptz,
  created_by uuid references auth.users(id) on delete set null
);
alter table public.scribe_site_admins enable row level security;
alter table public.scribe_registration_invites enable row level security;
revoke all on public.scribe_site_admins from public, anon, authenticated, service_role;
revoke all on public.scribe_registration_invites from public, anon, authenticated, service_role;

-- This trigger is the final enforcement boundary, including direct Auth API calls.
-- The counter is consumed in the SAME transaction as auth.users insertion.
create or replace function public.scribe_consume_registration_invite()
returns trigger language plpgsql security definer set search_path = pg_catalog as $$
declare
  supplied text;
  matched uuid;
begin
  -- Auth may update metadata after INSERT. Never retain the invitation in a profile.
  if tg_op = 'UPDATE' then
    new.raw_user_meta_data := coalesce(new.raw_user_meta_data, '{}'::jsonb) - 'invitation_code';
    return new;
  end if;
  supplied := upper(btrim(coalesce(new.raw_user_meta_data->>'invitation_code', '')));
  if length(supplied) < 16 or length(supplied) > 128 then
    raise exception 'A valid invitation code is required' using errcode = 'P0001';
  end if;
  update public.scribe_registration_invites
     set used_count = used_count + 1, last_used_at = now()
   where code_hash = encode(sha256(convert_to(supplied, 'UTF8')), 'hex')
     and revoked_at is null and expires_at > now() and used_count < max_uses
   returning id into matched;
  if matched is null then
    raise exception 'Invitation code is invalid, expired, revoked or exhausted' using errcode = 'P0001';
  end if;
  new.raw_user_meta_data := coalesce(new.raw_user_meta_data, '{}'::jsonb) - 'invitation_code';
  return new;
end;
$$;
revoke all on function public.scribe_consume_registration_invite() from public, anon, authenticated, service_role;
drop trigger if exists scribe_require_registration_invite on auth.users;
create trigger scribe_require_registration_invite
before insert or update of raw_user_meta_data on auth.users
for each row execute function public.scribe_consume_registration_invite();

-- Friendly validation errors before the transactional trigger. No counter update here.
create or replace function public.scribe_check_registration_invite(event jsonb)
returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
declare supplied text;
begin
  supplied := upper(btrim(coalesce(event->'user'->'user_metadata'->>'invitation_code',
    event->'user'->'raw_user_meta_data'->>'invitation_code', '')));
  if not exists (
    select 1 from public.scribe_registration_invites
    where code_hash = encode(sha256(convert_to(supplied, 'UTF8')), 'hex')
      and revoked_at is null and expires_at > now() and used_count < max_uses
  ) then
    return jsonb_build_object('error',jsonb_build_object('http_code',403,
      'message','邀请码无效、已过期或已用完，请联系管理员。'));
  end if;
  return '{}'::jsonb;
end;
$$;
revoke all on function public.scribe_check_registration_invite(jsonb) from public, anon, authenticated, service_role;
grant execute on function public.scribe_check_registration_invite(jsonb) to supabase_auth_admin;
-- Administrator assignment is a separate, explicit operator step: 05-create-admin.sql.
commit;

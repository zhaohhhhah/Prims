-- Apply after 01-registration.sql, before enabling email-confirmed signup.
-- GoTrue remains the sole issuer/verifier of OTPs. This table contains no OTP or password.
begin;
create table if not exists public.scribe_pending_registration_invites (
  user_id uuid primary key references auth.users(id) on delete cascade deferrable initially deferred,
  invite_id uuid not null references public.scribe_registration_invites(id),
  created_at timestamptz not null default now(),
  last_sent_at timestamptz
);
alter table public.scribe_pending_registration_invites add column if not exists last_sent_at timestamptz;
alter table public.scribe_pending_registration_invites enable row level security;
revoke all on public.scribe_pending_registration_invites from public, anon, authenticated, service_role;

create or replace function public.scribe_consume_registration_invite()
returns trigger language plpgsql security definer set search_path = pg_catalog as $$
declare supplied text; matched uuid; pending uuid;
begin
  supplied := upper(btrim(coalesce(new.raw_user_meta_data->>'invitation_code', '')));
  -- Refuse already-confirmed INSERTs. GoTrue must keep AUTOCONFIRM=false;
  -- only GoTrue can distinguish valid OTP verification from auto-confirmation.
  if tg_op = 'INSERT' and new.email_confirmed_at is not null then
    raise exception 'Email confirmation is required for registration' using errcode = 'P0001';
  end if;
  if tg_op = 'INSERT' or (supplied <> '' and old.email_confirmed_at is null) then
    if length(supplied) < 16 or length(supplied) > 128 then
      raise exception 'A valid invitation code is required' using errcode = 'P0001';
    end if;
    select id into matched from public.scribe_registration_invites
      where code_hash = encode(sha256(convert_to(supplied, 'UTF8')), 'hex')
        and revoked_at is null and expires_at > clock_timestamp() and used_count < max_uses;
    if matched is null then
      raise exception 'Invitation code is invalid, expired, revoked or exhausted' using errcode = 'P0001';
    end if;
    -- Deferred FK permits this BEFORE INSERT row while keeping the link private.
    insert into public.scribe_pending_registration_invites(user_id,invite_id)
      values(new.id,matched)
      on conflict(user_id) do update set invite_id=excluded.invite_id,created_at=now();
  end if;
  -- Defense in depth after GoTrue sends: a newly stored signup challenge must
  -- still have a valid binding. The audit trigger below is the PRE-send gate.
  if tg_op = 'UPDATE' and new.email_confirmed_at is null
    and coalesce(new.confirmation_token, '') <> ''
    and (new.confirmation_token is distinct from old.confirmation_token
      or new.confirmation_sent_at > old.confirmation_sent_at) then
    if not exists(select 1 from public.scribe_pending_registration_invites p
      join public.scribe_registration_invites i on i.id=p.invite_id
      where p.user_id=new.id and i.revoked_at is null and i.expires_at>clock_timestamp() and i.used_count<i.max_uses) then
      raise exception 'A valid pending invitation is required to issue a signup challenge' using errcode = 'P0001';
    end if;
  end if;
  if tg_op = 'UPDATE' and old.email_confirmed_at is null and new.email_confirmed_at is not null then
    select invite_id into pending from public.scribe_pending_registration_invites
      where user_id=new.id for update;
    if pending is not null then
      -- Require an issued signup challenge before confirming a new account.
      -- This also rejects immediate AUTOCONFIRM on a fresh signup. It cannot
      -- replace the GoTrue configuration for previously pending users.
      if coalesce(old.confirmation_token, '') = '' or old.confirmation_sent_at is null then
        raise exception 'Email confirmation challenge is required for registration' using errcode = 'P0001';
      end if;
      update public.scribe_registration_invites
        set used_count=used_count+1,last_used_at=now()
        where id=pending and revoked_at is null and expires_at>clock_timestamp() and used_count<max_uses
        returning id into matched;
      if matched is null then
        raise exception 'Invitation code is invalid, expired, revoked or exhausted; contact the administrator to repair the pending registration' using errcode = 'P0001';
      end if;
      delete from public.scribe_pending_registration_invites where user_id=new.id;
    end if;
    -- Pre-migration accounts already consumed their invite at INSERT; no second charge.
  end if;
  new.raw_user_meta_data := coalesce(new.raw_user_meta_data,'{}'::jsonb) - 'invitation_code';
  return new;
end;
$$;
revoke all on function public.scribe_consume_registration_invite() from public, anon, authenticated, service_role;
drop trigger if exists scribe_require_registration_invite on auth.users;
create trigger scribe_require_registration_invite
before insert or update of raw_user_meta_data,email_confirmed_at,confirmation_token,confirmation_sent_at on auth.users
for each row execute function public.scribe_consume_registration_invite();
-- GoTrue also copies signup metadata into auth.identities. Keep the raw invite
-- out of both metadata stores, including subsequent identity updates.
create or replace function public.scribe_strip_identity_registration_invite()
returns trigger language plpgsql set search_path = pg_catalog as $$
begin
  new.identity_data := coalesce(new.identity_data, '{}'::jsonb) - 'invitation_code';
  return new;
end;
$$;
revoke all on function public.scribe_strip_identity_registration_invite() from public, anon, authenticated, service_role;
drop trigger if exists scribe_strip_identity_registration_invite on auth.identities;
create trigger scribe_strip_identity_registration_invite
before insert or update of identity_data on auth.identities
for each row execute function public.scribe_strip_identity_registration_invite();
-- GoTrue v2.197.0 logs user_confirmation_requested INSIDE the same signup /
-- resend transaction and BEFORE sendConfirmation sends SMTP or the email hook.
-- GOTRUE_AUDIT_LOG_DISABLE_POSTGRES=false is a required deployment invariant.
-- A users-token-update trigger alone is too late: this GoTrue version sends
-- mail first, then updates confirmation_token / confirmation_sent_at.
create or replace function public.scribe_gate_registration_mail()
returns trigger language plpgsql security definer set search_path = pg_catalog as $$
declare actor uuid; confirmed timestamptz; pending uuid; last_sent timestamptz; valid uuid;
begin
  if new.payload->>'action' is distinct from 'user_confirmation_requested' then return new; end if;
  actor := (new.payload->>'actor_id')::uuid;
  -- Consistent lock order: auth user, pending link, invitation. Verification
  -- already holds the auth user row before its BEFORE UPDATE trigger runs.
  select email_confirmed_at into confirmed from auth.users where id=actor for update;
  if not found then raise exception 'Signup account is missing' using errcode='P0001'; end if;
  if confirmed is not null then return new; end if;
  select invite_id,last_sent_at into pending,last_sent
    from public.scribe_pending_registration_invites where user_id=actor for update;
  if pending is null then
    raise exception 'A valid pending invitation is required before sending signup mail' using errcode='P0001';
  end if;
  select id into valid from public.scribe_registration_invites
    where id=pending and revoked_at is null and expires_at>clock_timestamp() and used_count<max_uses for share;
  if valid is null then
    raise exception 'Invitation code is invalid, expired, revoked or exhausted' using errcode='P0001';
  end if;
  if last_sent is not null and last_sent>clock_timestamp()-interval '60 seconds' then
    raise exception 'Registration email rate limit: wait 60 seconds before retrying' using errcode='P0001';
  end if;
  -- This lock/claim persists through GoTrue sending. Any SMTP, hook or Auth
  -- transaction failure rolls it back, so failed delivery cannot stick a user.
  update public.scribe_pending_registration_invites set last_sent_at=clock_timestamp() where user_id=actor;
  return new;
end;
$$;
revoke all on function public.scribe_gate_registration_mail() from public, anon, authenticated, service_role;
drop trigger if exists scribe_gate_registration_mail on auth.audit_log_entries;
create trigger scribe_gate_registration_mail before insert on auth.audit_log_entries
for each row execute function public.scribe_gate_registration_mail();
commit;

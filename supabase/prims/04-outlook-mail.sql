-- Prims extension; apply after the upstream schema migrations. See README.md.
begin;
create table if not exists public.scribe_outlook_mail (
 singleton boolean primary key default true check(singleton),
 client_id text not null default '', client_secret_cipher text,
 mailbox text, enabled boolean not null default false,
 access_cipher text, refresh_cipher text, expires_at bigint,
 needs_reauth boolean not null default false, last_error text,
 version bigint not null default 1, updated_at timestamptz not null default now()
);
insert into public.scribe_outlook_mail(singleton) values(true) on conflict do nothing;
create table if not exists public.scribe_outlook_oauth_states (
 state_hash text primary key, cookie_hash text not null, user_id uuid not null references auth.users(id) on delete cascade,
 verifier_cipher text not null, nonce text not null, config_version bigint not null,
 expires_at timestamptz not null
);
create table if not exists public.scribe_outlook_mail_events (
 id text primary key, body_hash text not null, status text not null check(status in('processing','sent','failed')),
 updated_at timestamptz not null default now()
);
alter table public.scribe_outlook_mail enable row level security;
alter table public.scribe_outlook_oauth_states enable row level security;
alter table public.scribe_outlook_mail_events enable row level security;
revoke all on public.scribe_outlook_mail,public.scribe_outlook_oauth_states,public.scribe_outlook_mail_events from public,anon,authenticated,service_role;
commit;

-- Add SearXNG protocol support without changing existing users, endpoints or encrypted keys.
begin;
alter table public.scribe_search_connections drop constraint if exists scribe_search_connections_provider_check;
alter table public.scribe_search_connections add constraint scribe_search_connections_provider_check check(provider in ('doubao','deepseek','searxng'));
create table if not exists public.scribe_search_options (
 user_id uuid primary key references public.users(id) on delete cascade,
 max_calls_per_request bigint not null default 3 check(max_calls_per_request between 0 and 4294967295),
 updated_at timestamptz not null default now()
);
alter table public.scribe_search_options enable row level security;
revoke all on table public.scribe_search_options from public,anon,authenticated;
commit;

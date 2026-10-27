-- User-owned independent search credentials; encrypted using the existing AI CryptoBox.
create table if not exists public.scribe_search_connections (
 user_id uuid primary key references public.users(id) on delete cascade,
 provider text not null check(provider in ('doubao','deepseek')),
 endpoint text not null check(octet_length(endpoint) between 1 and 2048),
 api_key_cipher text not null check(octet_length(api_key_cipher) between 1 and 65536),
 revision bigint not null default 1,
 updated_at timestamptz not null default now()
);
alter table public.scribe_search_connections enable row level security;
revoke all on public.scribe_search_connections from public,anon,authenticated;

begin;
alter table public.scribe_ai_model_profiles add column if not exists context_windows jsonb not null default '{}'::jsonb check(jsonb_typeof(context_windows)='object');
alter table public.scribe_search_options alter column max_calls_per_request set default 0;

create table if not exists public.scribe_ai_usage(
 id uuid primary key,user_id uuid not null references auth.users(id) on delete cascade,
 started_at timestamptz not null,provider text not null,model text not null,purpose text not null,
 input_tokens bigint not null default 0 check(input_tokens>=0),output_tokens bigint not null default 0 check(output_tokens>=0),total_tokens bigint not null default 0 check(total_tokens>=0),cached_tokens bigint not null default 0 check(cached_tokens>=0),
 usage_reported boolean not null,usage_complete boolean not null,status text not null check(status in ('completed','failed','interrupted'))
);
create index if not exists scribe_ai_usage_owner_time on public.scribe_ai_usage(user_id,started_at);
alter table public.scribe_ai_usage enable row level security;
revoke all on public.scribe_ai_usage from public,anon,authenticated,service_role;

commit;

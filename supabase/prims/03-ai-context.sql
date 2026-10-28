-- Prims extension; apply after the upstream schema migrations. See README.md.
begin;
create table if not exists public.scribe_ai_contexts (
  user_id uuid not null references auth.users(id) on delete cascade,
  project_id uuid not null references public.projects(id) on delete cascade,
  thread_id uuid not null default gen_random_uuid(),
  brief text not null default '' check(char_length(brief)<=8000),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key(user_id,project_id)
);
create table if not exists public.scribe_ai_context_threads (
  user_id uuid not null,
  project_id uuid not null,
  id uuid not null,
  title text not null default '新对话' check(char_length(title)<=80),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key(user_id,project_id,id),
  foreign key(user_id,project_id) references public.scribe_ai_contexts(user_id,project_id) on delete cascade
);
alter table public.scribe_ai_context_threads add column if not exists auto_summary text not null default '' check(char_length(auto_summary)<=2000);
alter table public.scribe_ai_context_threads add column if not exists summary_updated_at timestamptz;
alter table public.scribe_ai_context_threads add column if not exists summary_revision bigint not null default 0;
create table if not exists public.scribe_ai_context_messages (
  seq bigserial primary key,
  user_id uuid not null,
  project_id uuid not null,
  thread_id uuid not null,
  id uuid not null,
  role text not null check(role in ('user','assistant','action')),
  content text not null check(char_length(content)<=8388608),
  raw text check(char_length(raw)<=8388608),
  file_path text check(char_length(file_path)<=512),
  selection text check(char_length(selection)<=8388608),
  selection_id text check(char_length(selection_id)<=128),
  created_at timestamptz not null default now(),
  unique(user_id,id),
  foreign key(user_id,project_id) references public.scribe_ai_contexts(user_id,project_id) on delete cascade
);
insert into public.scribe_ai_context_threads(user_id,project_id,id,created_at,updated_at)
select user_id,project_id,thread_id,created_at,updated_at from public.scribe_ai_contexts
on conflict(user_id,project_id,id) do nothing;
insert into public.scribe_ai_context_threads(user_id,project_id,id,created_at,updated_at)
select user_id,project_id,thread_id,min(created_at),max(created_at) from public.scribe_ai_context_messages group by user_id,project_id,thread_id
on conflict(user_id,project_id,id) do nothing;
create index if not exists scribe_ai_context_recent on public.scribe_ai_context_messages(user_id,project_id,thread_id,seq desc);
alter table public.scribe_ai_contexts enable row level security;
alter table public.scribe_ai_context_messages enable row level security;
alter table public.scribe_ai_context_threads enable row level security;
revoke all on public.scribe_ai_contexts,public.scribe_ai_context_messages,public.scribe_ai_context_threads from public,anon,authenticated,service_role;
revoke all on sequence public.scribe_ai_context_messages_seq_seq from public,anon,authenticated,service_role;
comment on table public.scribe_ai_contexts is 'Private writing brief and active conversation, scoped to user and project; new conversation retains prior message rows.';
commit;

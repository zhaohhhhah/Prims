-- Private, user-scoped agent state. App routes enforce membership; no direct client-table grants.
create table if not exists public.scribe_agent_preferences (
 user_id uuid primary key references public.users(id) on delete cascade,
 revision bigint not null default 1,
 memory_enabled boolean not null default true,
 skills_enabled boolean not null default true,
 diagnostics_enabled boolean not null default true,
 search_enabled boolean not null default true,
 updated_at timestamptz not null default now()
);
create table if not exists public.scribe_agent_skills (
 id uuid primary key default gen_random_uuid(),
 user_id uuid not null references public.users(id) on delete cascade,
 name text not null check(length(name) between 1 and 64),
 description text not null check(length(description) between 1 and 1024),
 content text not null check(octet_length(content)<=49152),
 enabled boolean not null default true,
 archived_at timestamptz,
 updated_at timestamptz not null default now()
);
create unique index if not exists scribe_agent_skills_active_name on public.scribe_agent_skills(user_id,name) where archived_at is null;
create table if not exists public.scribe_project_memories (
 user_id uuid not null references public.users(id) on delete cascade,
 project_id uuid not null references public.projects(id) on delete cascade,
 content text not null default '' check(length(content)<=4000),
 revision bigint not null default 0,
 updated_at timestamptz not null default now(),
 primary key(user_id,project_id)
);
alter table public.scribe_agent_preferences enable row level security;
alter table public.scribe_agent_skills enable row level security;
alter table public.scribe_project_memories enable row level security;
revoke all on public.scribe_agent_preferences,public.scribe_agent_skills,public.scribe_project_memories from public,anon,authenticated;

-- Idempotent when upgrading an earlier unpublished candidate.
alter table public.scribe_agent_preferences add column if not exists revision bigint not null default 1;

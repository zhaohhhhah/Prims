begin;
create table if not exists public.scribe_ai_model_profiles (
 id uuid primary key default gen_random_uuid(),
 user_id uuid not null references auth.users(id) on delete cascade,
 name text not null check(char_length(name) between 1 and 80),
 provider text not null check(provider in ('openai','anthropic','gemini','ollama','lmstudio','openai-compatible')),
 model text not null check(char_length(model) between 1 and 256),
 models text[] not null default '{}'::text[] check(cardinality(models)<=100),
 base_url text check(char_length(base_url)<=2048),
 api_key_cipher text,
 updated_at timestamptz not null default now()
);
-- Additive compatibility if the initial single-model profile table was already installed.
alter table public.scribe_ai_model_profiles add column if not exists models text[] not null default '{}'::text[];
alter table public.scribe_ai_model_profiles drop constraint if exists scribe_ai_model_profiles_model_check;
alter table public.scribe_ai_model_profiles add constraint scribe_ai_model_profiles_model_check check(char_length(model) between 1 and 256);
alter table public.scribe_ai_model_profiles drop constraint if exists scribe_ai_model_profiles_models_check;
alter table public.scribe_ai_model_profiles add constraint scribe_ai_model_profiles_models_check check(cardinality(models)<=100);
create index if not exists scribe_ai_model_profiles_owner on public.scribe_ai_model_profiles(user_id,updated_at desc);
alter table public.scribe_ai_model_profiles enable row level security;
revoke all on public.scribe_ai_model_profiles from public,anon,authenticated,service_role;
comment on table public.scribe_ai_model_profiles is 'Private per-user named AI configurations. Original users.ai_config remains the default; request selection never mutates it.';
commit;

-- V4 optional AI summaries. No existing versions/body/label are rewritten.
BEGIN;
CREATE TABLE IF NOT EXISTS public.scribe_version_summary_preferences (
 user_id uuid PRIMARY KEY REFERENCES public.users(id) ON DELETE CASCADE,
 enabled boolean NOT NULL DEFAULT false,
 enabled_since timestamptz,
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS public.scribe_version_ai_summaries (
 version_id uuid PRIMARY KEY REFERENCES public.project_versions(id) ON DELETE CASCADE,
 project_id uuid NOT NULL REFERENCES public.projects(id) ON DELETE CASCADE,
 user_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
 status text NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','succeeded','failed','skipped')),
 summary text,
 error text,
 requested_manually boolean NOT NULL DEFAULT false,
 attempts integer NOT NULL DEFAULT 0,
 request_count integer NOT NULL DEFAULT 0,
 lease_id uuid,
 lease_expires_at timestamptz,
 previous_version_id uuid,
 model text,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK(summary IS NULL OR char_length(summary)<=240)
);
CREATE INDEX IF NOT EXISTS scribe_version_summaries_queue_idx ON public.scribe_version_ai_summaries(created_at) WHERE status='queued';
ALTER TABLE public.scribe_version_summary_preferences ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.scribe_version_ai_summaries ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON public.scribe_version_summary_preferences,public.scribe_version_ai_summaries FROM public,anon,authenticated,service_role;
COMMIT;

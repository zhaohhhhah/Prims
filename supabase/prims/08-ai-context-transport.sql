-- Align persisted private messages with the server's existing 8 Mi-character field boundary.
-- Only relax legacy checks; retain roles, ownership, RLS and location validation unchanged.
BEGIN;
SET LOCAL lock_timeout = '2s';
ALTER TABLE public.scribe_ai_context_messages
  DROP CONSTRAINT IF EXISTS scribe_ai_context_messages_content_check,
  DROP CONSTRAINT IF EXISTS scribe_ai_context_messages_raw_check,
  DROP CONSTRAINT IF EXISTS scribe_ai_context_messages_selection_check;
ALTER TABLE public.scribe_ai_context_messages
  ADD CONSTRAINT scribe_ai_context_messages_content_check CHECK (char_length(content)<=8388608) NOT VALID,
  ADD CONSTRAINT scribe_ai_context_messages_raw_check CHECK (char_length(raw)<=8388608) NOT VALID,
  ADD CONSTRAINT scribe_ai_context_messages_selection_check CHECK (char_length(selection)<=8388608) NOT VALID;
ALTER TABLE public.scribe_ai_context_messages VALIDATE CONSTRAINT scribe_ai_context_messages_content_check;
ALTER TABLE public.scribe_ai_context_messages VALIDATE CONSTRAINT scribe_ai_context_messages_raw_check;
ALTER TABLE public.scribe_ai_context_messages VALIDATE CONSTRAINT scribe_ai_context_messages_selection_check;
COMMIT;

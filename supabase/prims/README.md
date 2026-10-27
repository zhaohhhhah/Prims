# Prims database extensions

These manual SQL files accompany the modified Prims source. They are **not automatically applied** by the upstream migration runner. Existing upstream files in `supabase/migrations` remain unchanged. None of these scripts contains an account ID, invitation code, provider key or deployed database content.

Use a privileged database operator connection (normally `postgres`) and take a backup before applying them. The upstream Supabase roles (`anon`, `authenticated`, `service_role`, `supabase_auth_admin`) and base tables must already exist. In particular, finish **all upstream migrations** first, including `auth.users`, `public.users`, projects, project membership and files. Existing standalone Supabase bootstraps may also need their normal Auth function ownership/role compatibility setup; these extensions do not silently change ownership of Auth's built-in functions.

## Fresh installation order

1. While setup is local/private, create the intended owner's email/password account through your normal Supabase Auth setup. Keep that exact account email for the explicit administrator step. Do this **before** installing the invitation gate; the gate intentionally rejects registrations without an invitation.
2. Finish the upstream migration runner and any bootstrap grants.
3. Apply `01-registration.sql`, `02-welcome.sql`, `03-ai-context.sql`, then `04-outlook-mail.sql`, using `psql -v ON_ERROR_STOP=1 -f <file>` for each.
4. Assign the intended existing account explicitly:

   ```sh
   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -v admin_email='you@example.com' -f supabase/prims/05-create-admin.sql
   ```

   The email is matched exactly against an active Auth account, and precisely one match is required. This script never chooses an administrator based on user count, registration order or editable profile metadata. Reapplying for the same account is safe. The database operator can deliberately authorize another existing account by running it again with that account's exact email.
5. Sign in as that administrator and create invitations in Settings → Site administration. The database trigger enforces invitations even if a caller uses the Auth API directly. Enable the optional friendly Auth pre-registration hook separately if desired:

   ```text
   GOTRUE_HOOK_BEFORE_USER_CREATED_ENABLED=true
   GOTRUE_HOOK_BEFORE_USER_CREATED_URI=pg-functions://postgres/public/scribe_check_registration_invite
   ```

   The hook gives friendlier errors; the transactional trigger remains the final enforcement boundary. Keep the site private until the owner and invitation gate are ready.

For an existing installation with a known administrator, keep existing `scribe_site_admins` rows; `01-registration.sql` does not replace them. Only run the explicit assignment when the operator intends to grant an administrator role. There is no migration that assumes the production instance has one user.

**Privilege ordering matters:** these files enable RLS and revoke public/anon/authenticated/service-role access to their private tables and functions. Apply them **after** any broad `GRANT ALL ON ALL TABLES IN SCHEMA public TO service_role` bootstrap operation. Do not run blanket grants afterwards. If another setup command reapplies blanket grants, reapply these extension files last before serving requests. The app's direct privileged database connection handles these private features; a browser's Supabase session must not query these tables directly.

## Included schema

| File | Objects and dependency |
|---|---|
| `01-registration.sql` | Private administrator and hashed invitation tables; registration validation function and transactional `auth.users` invitation trigger. Requires upstream Auth/roles. |
| `02-welcome.sql` | Per-user welcome-project marker referencing Auth users and projects. Does not create or overwrite project documents. The app seeds only an unseeded user's private template; deleting a welcome project keeps its marker. |
| `03-ai-context.sql` | Per-user/project writing context, retained conversation threads/messages and per-thread automatic summary/revision. Requires Auth users and projects. Backfills missing thread metadata from existing context/messages without copying other users' data. |
| `04-outlook-mail.sql` | Private Outlook configuration, encrypted token/state columns and webhook event deduplication. Creates only an empty disabled singleton configuration; it does not connect a mailbox, enable sending or confirm users. Outlook administration also requires the registration/admin schema. |
| `05-create-admin.sql` | Explicit `psql` operator assignment by an exact existing email. Run after `01-registration.sql`; the intended account must already exist. |

SMTP uses the optional private host mail controller rather than an additional SQL table. Publishing these SQL files alone does not install that controller or configure the deployment's mail transport. Keep runtime secrets, encryption keys, SMTP authorization codes, OAuth tokens and all `.env` files outside the repository. Preserve the app's encryption key across upgrades so existing per-user AI keys and mail credentials remain decryptable.

## Named model configurations

`06-ai-model-profiles.sql` adds the private per-user named AI configuration table. Apply it after the upstream Auth/roles setup and after blanket bootstrap grants, preserving its RLS/revokes. It is additive and does not copy or change the original `users.ai_config` default API configuration or key. The app encrypts profile keys with the existing persistent encryption key. Each request may select an owned profile; missing/default selection continues using the original default.

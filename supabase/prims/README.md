# Prims database extensions

These manual SQL files accompany the modified Prims source. They are **not automatically applied** by the upstream migration runner. Existing upstream files in `supabase/migrations` remain unchanged. None of these scripts contains an account ID, invitation code, provider key or deployed database content.

Use a privileged database operator connection (normally `postgres`) and take a backup before applying them. The upstream Supabase roles (`anon`, `authenticated`, `service_role`, `supabase_auth_admin`) and base tables must already exist. In particular, finish the **base May schema migrations** first, including `auth.users`, `public.users`, projects, project membership, files and versions. The later October model/context migrations depend on `06-ai-model-profiles.sql`; apply that extension before continuing those migrations, as shown below. Existing standalone Supabase bootstraps may also need their normal Auth function ownership/role compatibility setup; these extensions do not silently change ownership of Auth's built-in functions.

## Fresh installation order

1. While setup is local/private, create the intended owner's email/password account through your normal Supabase Auth setup. Keep that exact account email for the explicit administrator step. Do this **before** installing the invitation gate; the gate intentionally rejects registrations without an invitation.
2. Finish the upstream base May migrations (through `20260524000004_comment_suggestions.sql`) and bootstrap grants. Pause before the October model/context migrations until extension `06` exists.
3. Apply `01-registration.sql`, `02-welcome.sql`, `03-ai-context.sql`, then `04-outlook-mail.sql`, using `psql -v ON_ERROR_STOP=1 -f <file>` for each. Continue with the complete ordered extension list below; the explicit administrator step is separate and never part of a wildcard loop.
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

## Complete release execution order

Use full filenames. Two independent files begin with `08-` and two begin with `09-`; these are additive features, not alternative versions. A numeric prefix alone is not a migration identifier. Do not execute `supabase/prims/*.sql` as a batch: `05-create-admin.sql` deliberately requires an operator-supplied account email.

After the upstream schema and compatibility grants are ready, apply these extension files explicitly in order:

1. `01-registration.sql`
2. `02-welcome.sql`
3. `03-ai-context.sql`
4. `04-outlook-mail.sql`
5. `05-create-admin.sql` **only when deliberately assigning an existing administrator**, with the `admin_email` parameter shown above. Existing deployments retain their administrator rows and ordinarily skip this step.
6. `06-ai-model-profiles.sql`. **Now apply the remaining October upstream migrations in timestamp order through the normal migration runner/history before continuing with `07` below**; `20261028000002_model_context_windows.sql` and `20261028000003_remove_model_count_limit.sql` both directly require this private model-profile table.
7. `07-ai-profile-defaults.sql` — requires `06`; private default-model preferences and legacy-migration marker.
8. `08-project-actions-history.sql` — requires upstream files and project versions; private directories, action plans, version state and file/directory namespace checks.
9. `08-ai-context-transport.sql` — requires `03`; aligns stored private-message content/raw/selection field checks with the existing 8 Mi-character transport boundary; no provider/context budget changes.
10. `09-version-ai-summaries.sql` — requires upstream users/projects/versions and `08-project-actions-history.sql`; opt-in per-user summary preferences and leased jobs. Installing SQL alone does not enable model requests; the optional worker remains separately configured.
11. `09-registration-confirmation.sql` — requires `01` and audited GoTrue v2.197.0 behavior. Apply before enabling confirmation; see the transport requirements below.
12. `10-tutorial-releases.sql` — requires `02` and upstream users/projects; per-user tutorial release bookkeeping. SQL alone does not create PDF artifacts or replace existing projects.
13. `11-user-management.sql` — requires upstream users/projects/files/Yjs snapshots and the administrator schema. Adds private durable attachment cleanup scopes, retained after account hard deletion, and prevents collaboration updates from recreating a deleted project's snapshots. Deploy with the matching backend; old signed JWTs are rejected by its database-backed account check and live sockets recheck permissions.
14. `12-project-storage-quota.sql` — requires upstream projects/files/Storage and the quota-aware backend. Atomically reconciles source-file sizes and enforces **524,288,000 bytes (500MiB)** per project for source files plus attachments. Generated compile artifacts, version snapshots and avatars are excluded. Existing oversized projects can shrink; growing them is rejected. It restricts direct authenticated source-object and file-metadata writes to the supported backend API; service-role backend operations, read policies and other Storage buckets retain their behavior. Apply during a coordinated backend rollout so the quota is checked before any physical file overwrite. Raise the deployment's reverse-proxy and Storage upload limits separately.

15. `13-archive-directory-paths.sql` — requires `08-project-actions-history.sql`; forward-only correction of the shared file/directory namespace function. Allows parent directories to contain imported files while preserving exact file/directory and directory-beneath-file collision checks and the per-project advisory lock. It replaces only the function; do not edit/replay historical `08` to repair an existing database.

16. `14-native-compilers.sql` — requires upstream projects and the native TeX backend/worker. Migrates legacy Tectonic project preferences to XeLaTeX without changing creation/edit timestamps, sets the database default, and maps old clients' Tectonic preference writes to XeLaTeX. Historical compile records and enum values remain readable. Apply together with the matching native compile service; no document content changes.

The three formerly published files `07-ai-profile-defaults.sql`, `08-project-actions-history.sql`, and `09-version-ai-summaries.sql` must also be present in a complete checkout; do not mistake a deployment directory that omitted these files for a complete installation bundle.

## Upstream incremental migrations are also required

Keep `supabase/migrations` in its normal versioned runner/history. Beyond the initial May schema, the current release includes `20261027000001_agent_library.sql`, `20261027000002_search_connections.sql`, `20261028000001_searxng_search.sql`, `20261028000002_model_context_windows.sql`, `20261028000003_remove_model_count_limit.sql`, and `20261028000004_refresh_compile_repair_skill.sql`. For a fresh installation, first run the base May migrations, install manual extensions `01`–`06` (with `05` remaining explicit/optional administrator assignment), then resume the October upstream migrations in timestamp order, and finally continue manual extensions `07`–`14`. In particular, do not run `20261028000002_model_context_windows.sql` or `20261028000003_remove_model_count_limit.sql` before `06-ai-model-profiles.sql`. For an existing installation, apply only unapplied migrations for the exact source version, respecting these dependencies and the normal migration history. They provide agent preferences/skills/memory, search configuration and current model/context behavior; the manual extension list does not replace them. Future releases may add further versioned migrations, so consult the checked-out release and its migration history rather than treating this list as permanent.

## Email-confirmed registration deployment requirements

For `09-registration-confirmation.sql`, keep `GOTRUE_AUDIT_LOG_DISABLE_POSTGRES=false`, `GOTRUE_MAILER_AUTOCONFIRM=false`, `GOTRUE_SMTP_MAX_FREQUENCY=60s`, and one consistent `GOTRUE_MAILER_OTP_LENGTH` (the deployed UI supports 6–10 digits). The `user_confirmation_requested` audit insertion is the pre-send invitation/cooldown gate for the audited GoTrue version. Every initial or resent signup code needs a valid pending invitation; the invitation is consumed only by successful confirmation. Failed SMTP delivery rolls back the cooldown claim. Verify these invariants again before changing GoTrue versions.

Configure the persisted QQ SMTP or Outlook transport through the private mail controller; its existing saved credentials stay outside this repository. Enable `enabled=true` and `requireConfirmation=true` only after SQL, templates, Auth env and matching UI are ready. Verify the confirmation template contains GoTrue's `{{ .Token }}` value and is reachable by Auth. The SMTP provider account, authorization code, token encryption key and deployment secrets are never SQL parameters or public source files. A production rollout should pause signup while changing confirmation settings and reopen it only after health/template/config checks; do not restore automatic confirmation as an error fallback.

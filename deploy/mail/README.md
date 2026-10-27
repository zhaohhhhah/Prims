# Optional administrator mail controller

This is a deployment example for self-hosted Prims. It is **not a one-click installer** and is disabled until the operator installs the controller and the administrator explicitly applies a working sender. Do not deploy it to a public interface. Existing password login and projects do not depend on this optional controller.

The application has a SMTP/QQ settings panel and an optional personal Microsoft Outlook OAuth sender. SMTP 465 uses implicit TLS; 587 and other ports require STARTTLS. TLS trust and hostname are checked. The administrator supplies SMTP credentials or personally authorizes Outlook; no real credentials belong in this repository.

## Trusted configuration and prerequisites

Use a Unix host with Python 3, Docker Compose, and an operator-controlled installation of Supabase Auth (fixed compose service name **auth**). The application must use its own private encryption key (`AI_KEY_ENCRYPTION_KEY`) and database connection. Apply `schema.sql` as the database owner to create private Outlook credential/state tables. Never grant these tables to browser-facing database roles.

Copy `operator.env.example` to a private directory, for example `/etc/prims/mail/operator.env`, and adapt it to your installation. Protect the directory as mode 0700 and files as mode 0600. The origin is explicitly configured by `PRIMS_PUBLIC_URL`, never inferred from an HTTP Host header or an administrator JSON field. It must be a HTTPS origin with optional port; userinfo, path prefixes, query strings and fragments are rejected. Outlook OAuth also requires a plain ASCII **non-IDN** domain because Microsoft Entra rejects IDN reply URLs. SMTP can use an HTTPS origin supported by your Auth deployment.

The controller's health and hook URLs may be configured only by this trusted operator environment. They are restricted to literal loopback addresses and fixed endpoint paths. HTTP clients cannot supply URLs, filesystem paths, environment variable names, commands, containers or compose service names.

## Example installation outline

1. Install `mail-control.py` and `mail-control-outlook.py` together in the private deployment directory. Supply your fixed `compose.yaml` there, with an `auth` service whose mail settings are read from `mail.env` (Compose raw env-file mode is appropriate for SMTP authorization codes containing `$`). This example does not create your stack or alter an existing compose file.
2. Create the private `app.env` used by your application and put its `PRIMS_PUBLIC_URL` there. Load the trusted `operator.env` into the controller service environment. Run `python3 mail-control-outlook.py --prepare --base /etc/prims/mail` as the service operator. Preparation generates a private bearer token and Hook signing key without printing them and appends `SCRIBE_MAIL_CONTROL_TOKEN` / `SCRIBE_OUTLOOK_HOOK_SECRET` to `app.env`. It does not restart Auth, send mail or enable confirmation. Make those two values and `PRIMS_PUBLIC_URL` available to the app during your normal controlled app restart; never give the app the Docker socket or write access to these host files.
3. Run the controller with `python3 /etc/prims/mail/mail-control-outlook.py --base /etc/prims/mail` under your process manager. For systemd, use `EnvironmentFile=/etc/prims/mail/operator.env`, a dedicated service user with permission for only this deployment, and a working directory of `/etc/prims/mail`. It binds only to `127.0.0.1:19101`. Do not reverse-proxy this port or expose it through container port mappings.
4. The app's admin routes call the fixed loopback controller using the private bearer token. Configure QQ SMTP (`smtp.qq.com:465`, full mailbox address, generated SMTP authorization code) or another SMTP provider, specify a test recipient, and explicitly send one test before applying. The default is disabled; email confirmation is also disabled. A server-accepted message may still fail final delivery, so verify receipt separately.
5. For Outlook, register an Entra Web application with delegated `openid email offline_access https://graph.microsoft.com/Mail.Send`, using exactly `{PRIMS_PUBLIC_URL}/api/mail/outlook/callback`. Enter client ID/secret in the admin UI and personally authorize the account. Remove access logging for this exact callback path at every reverse-proxy layer so one-time code/state values do not enter URI access logs.

Only one automatic Auth mail transport can be active. Applying SMTP disables the Outlook send hook while retaining encrypted Outlook credentials in the database. Activating Outlook disables the SMTP host while retaining saved SMTP settings and the protected mode-0600 authorization-code file for switching back. The fixed Auth-service recreation checks the configured loopback health endpoint and attempts exact configuration rollback on failure. Other application services are not restarted by a mail-settings apply.

## Files excluded from publication

Keep `operator.env`, `app.env`, `mail.env`, `mail-settings.json`, `mail-control-token`, `smtp-password`, generated keys, certificates and database backups outside the repository. The `.example` file contains placeholders only. Never paste live OAuth secrets, access/refresh tokens or SMTP authorization codes into GitHub issues or logs.

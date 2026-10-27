# Prims

**A self-hosted collaborative LaTeX editor with an AI writing partner.**

Prims is a personal fork of [Scribe](https://github.com/sunnyallana/Scribe),
created by [Sunny Shaban Ali](https://github.com/sunnyallana). It keeps the
upstream React, CodeMirror, Yjs, Rust, Supabase and Tectonic foundation, and
adds a browser-first workflow for writing with your own AI endpoint.

## What's different

- **One project assistant.** Keep it floating or docked, continue a discussion,
  start a new conversation, or return to a previous one.
- **Editable suggestions inside the document.** A selection is a clue to the
  task. The model can suggest several related changes in the current file.
  Review and edit each candidate, apply or reject paragraphs independently,
  and undo accepted changes. Accept/reject controls sit beside each Git-style
  red/green preview. Candidates do not become document content until
  accepted. Ambiguous or conflicting source text blocks application.
- **Project context.** The assistant can list files, search filenames and text,
  and read relevant text windows or supported images through bounded,
  project-authorized tools. A private conversation summary is updated within
  the normal reply rather than a separate summarization request.
- **Conversation without forced edits.** The model can explain uncertainty or
  ask a clarifying question. A useful answer without a proposed change is a
  successful response.
- **Appearance.** Light, dark and automatic system-following modes, with a
  remembered preference.
- **Bring your own API.** Each account configures its own endpoint, API key
  and model. Stored credentials use the server's encryption key. Explicit
  image-capability errors trigger one image-free retry with an optional hint;
  unrelated network, authentication and rate-limit errors remain errors.
- **Chinese UI and onboarding.** Language settings, first-use API setup and a
  private, editable LaTeX welcome project with examples and a tutorial.
- **Invitation administration.** Invite-only registration, an administrator
  page, and private SMTP settings with QQ-compatible TLS presets. Personal
  Outlook OAuth is an optional alternative; only one configured mail
  transport is active at a time.
- **Chinese PDF preview assets.** PDF.js CMaps and standard fonts are served
  from the same application origin, with their license files preserved.

## Run it yourself

This fork needs the upstream application services **and the Prims database
extensions**. Existing Scribe databases must be backed up before migration.

1. Follow the upstream [setup scripts](scripts/README.md) and
   [self-hosting guide](docs/self-hosting.md) to prepare PostgreSQL/Supabase,
   Auth, Storage, Redis, Tectonic and the application environment.
2. Copy `.env.example` to a private `.env`, then fill in your own values.
   Generate and keep a stable `AI_KEY_ENCRYPTION_KEY`; rotating it makes
   existing encrypted provider credentials unreadable.
3. Follow [Prims database setup](supabase/prims/README.md) for the additional
   private tables, invitation gate and explicit administrator assignment.
   Create the initial owner through trusted Supabase Auth administration before
   installing the invitation gate; the Prims signup form already requires an
   invitation. Complete this before normal use; the original migrations alone are not
   enough for this fork.
4. Build the web application and Rust server using the workspace commands
   below. Serve the web application, API and `/supabase` gateway at the same
   public origin. Configure HTTPS in your reverse proxy.
5. Sign in and configure your own model endpoint in Settings. Mail delivery is
   optional and remains off until an administrator configures it. See the
   [optional mail controller](deploy/mail/README.md).

```sh
corepack enable
pnpm install --frozen-lockfile
pnpm exec turbo run build --filter=@scribe/web...

cd servers/rust
cargo test -p scribe-ai -p scribe-server
cargo build --release -p scribe-server
```

The package scope, crate names, storage keys and compatibility environment
variables retain their upstream `scribe` identifiers. The visible application
brand is **Prims**. The added workflow is tested in the web application;
desktop packaging is inherited from upstream and has not been revalidated for
all these additions.

## Tests

```sh
pnpm exec turbo run test --filter=@scribe/editor --filter=@scribe/web
cd servers/rust
cargo test -p scribe-ai -p scribe-server
```

Tests cover anchored edits, candidate changes without document writes,
independent application and guarded undo, cross-target proposal rejection,
bounded context, image-error classification and private context validation.
The deployment used disposable-account integration tests for conversation
isolation/history, read-only project search, image retry limits and mail
permission checks. Those private deployment fixtures and their credentials
are not part of this repository.

## Scope and data

Model tools are read-only; document writes happen through the editor after
acceptance. Multi-part suggestions currently target the active file. Image
context supports PNG, JPEG and WebP; PDF, Office and HEIC extraction are not
implemented by these tools. Project files sent to a model use the signed-in
account's configured provider. Deployers should configure access, backups,
retention and model-provider policies for their own installation.

This repository contains source code and example configuration. It does not
contain a production database, personal documents, provider credentials,
mailbox authorization codes, certificates or private deployment addresses.

## Upstream and license

Based on Scribe commit
[`d68b1996e780d906670830bbb4584c2387125ddf`](https://github.com/sunnyallana/Scribe/commit/d68b1996e780d906670830bbb4584c2387125ddf).
Original author: **Sunny Shaban Ali**. Prims modifications are maintained by
[@zhaohhhhah](https://github.com/zhaohhhhah).

Prims remains **AGPL-3.0-or-later**. See [LICENSE](LICENSE), the preserved
[upstream README](docs/UPSTREAM_README.md), and existing third-party notices.
The upstream history and attribution are retained.

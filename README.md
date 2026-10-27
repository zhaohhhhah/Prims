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
  task. The model can suggest precise changes across authorized project files.
  Review and edit each candidate, apply or reject paragraphs independently,
  and undo accepted changes. Accept/reject controls sit beside each Git-style
  red/green preview. Candidates do not become document content until
  accepted. Ambiguous or conflicting source text blocks application.
- **Project context.** The assistant can list files, search filenames and text,
  and read relevant text windows or supported images through bounded,
  project-authorized tools. A private conversation summary carries writing goals between turns. Long
  tool conversations use the pinned, Apache-licensed Codex local-compaction
  prompt and history-rebuilding method; one bounded same-connection summary
  request replaces old context only after complete success.
- **Conversation without forced edits.** The model can explain uncertainty or
  ask a clarifying question. A useful answer without a proposed change is a
  successful response.
- **Appearance settings.** A separate Appearance section offers 82 palettes,
  day/night choices, regional and impression-inspired series, and six coordinated
  presets. Classic colors and existing text/code fonts remain the initial
  defaults. Presets are opt-in; individual choices can still be adjusted.
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

Model tools inspect project references and submit pending proposals;
document writes happen through the real editor after acceptance. Proposals
can target multiple authorized text files. Exact anchors, overlapping ranges
and concurrent edits are checked before acceptance. Image
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

### Native agent tools and project history

OpenAI-compatible and Anthropic connections use native project tools and return results in the matching wire format. Assistant replies render Markdown, while LaTeX proposals remain source code with format checks. Review and apply changes per file and per paragraph. Version history includes dates and reviewed text restoration; optional AI summaries receive bounded version differences only. Interface URLs are recognized when unambiguous, otherwise choose the protocol explicitly.

### Incremental proposals and agent controls

Small, focused changes are submitted through a pending-proposal tool loop,
following the Codex patch/feedback/continue workflow. Complete proposals
survive later output limits or interruptions, are deduplicated, and remain
individually editable, acceptable, rejectable and undoable. The current file
also offers batch acceptance with source rechecks. Long-line review controls
stay within the visible editor. Selection and docked-composer drafts are
independent while sent messages retain shared conversation context.

The assistant can update a bounded public task plan using the Codex
`update_plan` argument shape. Repeated tool batches without new results or
proposals stop automatically. Progress shows actual public actions and
completed steps, not hidden reasoning. There is one unified assistant menu
entry; handwriting/OCR entry points have been removed from the web UI.

Each account can configure a separate Doubao or DeepSeek search connection
in Agent settings, with its own search endpoint and encrypted API key. Any
configured chat model can call that connection; unrelated chat-model credentials
are not sent to the search endpoint. Without an independent search configuration,
the existing official DeepSeek model connection remains an optional fallback.
Requests retain bounded attempts, attributed results and separate search-provider
billing. The client uses the configured search endpoint rather than fetching
arbitrary result URLs from the application server.

The pinned Codex source, adapted portions and Apache-2.0 notices are documented
in [the compaction origin notice](servers/rust/crates/scribe-server/src/services/vendor/codex_compaction/ORIGIN.md).


### Project import, bookshelf and reading appearance

Create a project from a ZIP archive, choose its main TeX file, or add multiple
files and ZIPs to an existing project. Import preserves relative paths and empty
directories. Name conflicts are checked before writes; existing files are not
silently overwritten. ZIPs are bounded to 20 MiB compressed, 32 MiB expanded,
8 MiB per file and 256 entries. Unsafe paths, links, encrypted archives and
unsupported compression are rejected. An existing-project upload can report
partial success if a later storage operation fails, preserving completed files.

The dashboard offers a bookshelf view using actual compiled first-page PDF
thumbnails. Projects without an available PDF use a plain cover placeholder.
PDF reading controls sit beside the PDF/Log tabs and offer Original, Soft and
Night modes plus brightness. These are screen-only filters: PDF bytes, export,
download and print output keep their original appearance. Reading preferences
are separate from the application theme.

The 82 application palettes include classic, impression-inspired, Chinese and
Japanese traditional-color inspirations, and nature-inspired Mexican, American,
Icelandic, Turkish and African series. These are design references rather than
national or continent-wide color standards. Day and night palettes are remembered
independently; filtering a series does not apply it. Six opt-in presets coordinate
colors, wordmarks, book titles and icon strokes while retaining the selected
text/code fonts. High-contrast mode remains available. Appearance preferences
are local browser settings.

Optional calligraphic wordmark/book-title choices use self-hosted Allura and
Ma Shan Zheng, distributed with their SIL Open Font License texts in
`apps/web/public/fonts/appearance/`. Existing application, editor and third-party
licenses remain in place.


Additional day/night palette pairs draw on Chinese architecture and landscapes,
regional color scenes, and musical moods. These include Forbidden City, Great
Wall, Nanjing, Ginkgo, Debussy-inspired Faun and Moonlight, Sunrise at Sea,
Rachmaninoff I/II/III, Paganini Variations and Tchaikovsky I. Book covers have
independent foreground/background colors; application text and all cover titles
retain at least 4.5:1 contrast. Existing custom appearance choices are preserved.
A PDF theme dropdown and brightness control sit in the preview toolbar, with warm-yellow and
light-green paper modes in addition to Original, Soft and Night. Filters are
display-only and do not change exported/downloaded PDFs.


Appearance now presents theme collections with both day and night previews.
Choosing a collection applies its matching palette pair atomically, without
changing personal fonts, icons or the independent light/dark/system color mode.
Existing custom combinations are retained until a collection is chosen.

### Agent preferences, skills and private memory

Settings now has an independent Agent section for personal tool preferences,
text-only Agent Skills and account-and-project private memory. Import or edit
SKILL.md with YAML name/description (up to 48 KiB), enable/disable skills, or
archive them. Importing instructions does not execute bundled scripts or grant
permissions. Memory is bounded to 4,000 characters; revision checks preserve
unsaved drafts when another session changes it. Existing chat-model connections remain unchanged;
search credentials are managed separately.

Project reads stay membership-scoped. Text and file changes require explicit
acceptance. The assistant has no host-system tool or arbitrary shell/MCP setup.
Optional diagnostics run fixed LaTeX/font tasks on a saved, authorized snapshot
in a fresh resource-limited, network-disabled worker; this does not edit project
files or install system fonts. Health checks distinguish configured services
from workers that are actually ready. Independent Doubao or DeepSeek search settings are shared
across the user's chat models and remain controlled by the personal search
preference. No additional system permissions are granted.

To deploy this addition, apply
`supabase/migrations/20261027000001_agent_library.sql` after the existing Prims
schema. The diagnostic workbench is optional: see
[its installation and isolation guide](servers/workbench/README.md) and public
examples. Application variables are `PRIMS_WORKBENCH_URL` and
`PRIMS_WORKBENCH_TOKEN`; worker/broker examples document
`PRIMS_WORKBENCH_IMAGE`, `PRIMS_WORKBENCH_TEX_CACHE` and
`PRIMS_WORKBENCH_TOKEN_FILE`. Operators supply private values outside source
control, pin approved images and provide an approved public TeX cache. The
application and model do not receive container-runtime access. Existing API
credentials and application data are not copied into disposable workers.

Editor chrome now uses short, reduced-motion-aware tab/panel/status transitions
without delaying operations or animating document layout. Assistant UI omits
redundant tutorial labels while retaining file paths, actual progress, review
controls and errors. The upstream AGPL attribution, adapted Codex notices and
font OFL licenses remain unchanged.


To enable independent search settings, apply
`supabase/migrations/20261027000002_search_connections.sql` after the Agent
library migration. In Settings > Agent, select Doubao or DeepSeek, supply its
complete HTTPS POST endpoint and a search-service API key, then save. Keys are
never returned by the API. An empty password keeps an existing key only for the
same provider and endpoint; changing either requires a new key. Clearing the
independent configuration restores the model-connection fallback. No new
server-wide search credential is required.

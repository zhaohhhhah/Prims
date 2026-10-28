# Prims disposable LaTeX/font workbench

This is a small trusted HTTP-to-Docker adapter, not a new sandbox implementation.
The broker listens only on 127.0.0.1:19340; **all routes require its private Bearer token**.
Do not publish this port or give the app/model the Docker socket. Only the trusted broker
uses the existing local Docker endpoint. The fixed worker image is pinned by image ID.

## Protocol

- `GET /health`: version, real approved-image availability and capabilities. 503 if unavailable.
- `POST /run`: `{jobId:UUID,task:"latex_check"|"font_check"|"list_fonts",path?:relative,files:[{path,contentBase64}]}`.
- `DELETE /runs/:jobId`: mark/stop active job; 202 if found, 404 otherwise. Use this when SSE/client disconnects.
- Success at HTTP level returns `{ok,exitCode,command:string[],output,truncated,timedOut,cancelled,durationMs,fonts?,fontScope?}`.
  A failed LaTeX run is HTTP200 with ok=false, not a broker error. Validation400, duplicate409 when a slot is available, busy429 before reading the body at full capacity, unauthenticated401.
- Inputs ≤256 regular files, each ≤8MiB, decoded total≤32MiB; no symlink type, unsafe paths or conflicting file/directory prefixes.
- `latex_check` requires the saved snapshot's .tex/.ltx/.latex path. It produces an XDV only in its disposable workspace, returning diagnostics, never artifacts or edited project files.
- `font_check` checks one provided font, or all project TTF/OTF/TTC files. Project fonts are temporarily visible to this job's fontconfig; no fonts are installed on the NAS or globally.
- `list_fonts` accepts no project files and lists only worker-provided font families.

Commands are fixed argv: Tectonic `--untrusted --only-cached --outfmt xdv`, `fc-scan`, `fc-list`.
There is no shell tool, package installation, arbitrary argv, environment override, arbitrary URL or file download.
Saved snapshot semantics, membership/editor checks and per-user tool permission are enforced by the app before calling this private service.

## Isolation

Fresh container per job: UID/GID10001, root filesystem readonly, network none, all capabilities dropped,
no-new-privileges, Docker's unchanged default seccomp,512MiB RAM/512MiB RAM+swap,CPU1,pids64,
workspace tmpfs128MiB and tmp tmpfs32MiB, both noexec/nosuid/nodev. Only the operator-approved **public TeX resource cache**
is bind-mounted readonly. It must not contain user documents or credentials. No NAS project mounts, Docker socket,
API/DB keys or broker token are passed to workers. At most two jobs run; commands timeout45s, broker total90s;
finally removes containers. This is ordinary Docker isolation on the operator's existing kernel, not a VM guarantee.
No change to host user namespaces, seccomp or network is required.

## Operator installation (optional, root task handles live integration)

1. Prepare a build-only directory with Dockerfile,worker.py and a trusted Tectonic binary.
   Use a pinned locally approved Python3.11 bookworm base image. The example default is an existing mirror of the official image.
   Build may use existing proxy build args for official Debian packages; proxy values are not source or worker ENV.
2. Build independent image; record image ID and package/version manifest. Do not replace the app image.
3. Generate a **new** random token into a private owner-readable mode0600 file; keep actual env/token out of Git.
4. Copy workbench.env.example to a private env file and replace imageID/cache/tokenFile. Configure Rust app with matching token and loopbackURL.
5. Prefer the separate trusted broker container in `Dockerfile.broker` and `compose.example.yml` with restart-unless-stopped, a private token-file bind and the standard Docker Unix socket. Its512MiB limit accommodates Base64/JSON parsing for two32MiB inputs. Copy the approved compatible Docker CLI into the broker build context; verify `docker --version` inside the image. This CLI may be dynamically linked, so verify ABI compatibility with the chosen base. User-systemd is an optional alternative; `Linger=no` cannot guarantee survival after logout. Do not change linger automatically. Existing app/Auth/DB need no restart for building/testing.
6. Health probe with bearer; run test_worker.py and synthetic sandbox smoke tests first. The source scripts do not auto-install/start a host service.

Readonly cached TeX packages may not include every requested package/font; failures are reported rather than fetching over the network.
Use existing project file upload plus fontspec `Path=...` for persistent project-local font files.
Do not claim that this service installed system fonts or examined unsaved editor buffers.

## Upstream/license provenance

- Docker/container flags: https://docs.docker.com/engine/containers/run/ and https://docs.docker.com/engine/security/seccomp/.
- Tectonic: https://github.com/tectonic-typesetting/tectonic ,0.15.0,MIT; copied from operator's existing trusted runtime binary. See its upstream LICENSE and third-party notices.
- Python: https://github.com/docker-library/python (official image packaging), Python Software Foundation License for interpreter.
- Debian bookworm fontconfig/fonts-dejavu: packaged upstream licenses retained in worker `/usr/share/doc/*/copyright`.
- Thin adapter files follow this repository's existing license; no OpenCode SDK or full agent distribution is vendored.

## Optional shared public font resources

Operators may set **both** `PRIMS_WORKBENCH_FONT_DIR` and `PRIMS_WORKBENCH_FONT_CACHE`.
Both paths must be absolute, without commas or control characters; half-configured pairs
prevent broker startup. Empty values retain the previous worker behavior. These values
come only from trusted deployment environment, never from HTTP/model/job fields.

The daemon binds the font library readonly at `/usr/local/share/fonts/prims-import` and
its prebuilt fontconfig cache readonly at `/var/cache/fontconfig` inside each disposable
worker. The broker itself does not require either host directory to be mounted. Standard
`/etc/fonts/fonts.conf`, already included by the worker's temporary project font configuration,
discovers the additional default font directory. Project-specific uploaded fonts and their
writable cache remain temporary and scoped to the individual job.

Prepare caches with the compatible fontconfig version and these exact container target
paths before deployment. Ensure UID10001 can traverse/read the public library/cache, keep
both readonly at runtime, and test actual family availability/LaTeX use. Do not point them
at user documents, private directories, socket paths or secrets. This does not install or
write fonts on the NAS. Font licensing remains the operator's responsibility; font binaries,
caches and NAS-specific paths must never be copied into the public GitHub repository.
All original worker isolation flags and the fixed operator-pinned worker image remain intact.

## Project compiler pipeline

Prims project settings can save `tectonic`, `pdflatex`, `xelatex` or `lualatex`
and the project's main document. Tectonic remains the default and retains its
existing runner. The other three engines use the existing Rust multi-pass
orchestrator, including BibTeX/Biber and SyncTeX, through a fixed
`scribe-tex-runner` entry point in a disposable offline worker.

Build `Dockerfile.texlive` with the approved local runner and Tectonic binary.
Supply an operator-managed TeX Live tree and prewarmed Lua font database as
readonly resources. Prewarm the trusted Biber runtime in a fixed readonly
`PRIMS_WORKBENCH_BIBER_CACHE` as well; job tmp/workspace remain noexec.
Neither resources nor compiled font caches belong in the
source repository. Use `compose.compiler.example.yml` for a separate loopback
19342 broker with its own private token; the existing diagnostic broker can
stay on 19340. Set the app's private `PRIMS_COMPILE_BROKER_URL` and
`PRIMS_COMPILE_BROKER_TOKEN` only after the worker has passed real engine tests.
The app and model never receive a Docker socket or host command interface.

The protocol adds only `task=latex_compile`, a fixed engine enum, a project main
path and bounded Base64 files. Worker replies contain bounded log, PDF, BBL and
SyncTeX artifacts. Request fields cannot select commands, mounts or environment
variables. Native TeX is launched with shell escape disabled; Lua execution is
contained in the same offline, readonly, credential-free worker. Project input
remains untrusted. Worker cancellation and total timeout remove the disposable
container; closing a log stream does not change the app's existing queue-job
semantics.

The dedicated 19342 instance advertises `latex_compile`, `font_check` and `list_fonts`;
19340 keeps the original diagnostic capabilities. Project-aware AI receives
the saved compiler, and its legacy Tectonic-only check refuses native-engine
projects rather than reporting an incompatible engine as a source error.
TeX resource reads use `openin_any=r` so Lua font loading can read its fixed
readonly TeX data; writes stay `openout_any=p`. No host/private data is mounted.

Font checks accept `fontName` to query an exact compilation font family,
PostScript name or TeX font filename, without requiring a project font upload.
Fontconfig matches are checked exactly and TeX font files are resolved through
the same readonly TeX resources as native compilation. A fallback font is never
reported as an exact match. Project TTF/OTF/TTC checks continue to use `path`;
font names cannot supply paths, commands, environment variables or mounts.


## Native project compiler (500MiB snapshots)

`compose.compiler.example.yml` runs a separate, authenticated, loopback-only broker on
port19342. It reuses the fixed native TeX Live worker image and supports PDFLaTeX,
XeLaTeX and LuaLaTeX. Project compilation does not download TeX packages at runtime.
Keep19340's existing diagnostic clients and their small JSON budgets compatible.

The app streams each storage object into its private workdir (at most four transfers),
then streams only registered project inputs into the compiler. Generated output files
are not added to that list; uploaded PDF figures and project-local fonts are preserved.
The native compile snapshot allows4096 files and500MiB aggregate source bytes, including
individual fonts larger than8MiB. Existing JSON snapshot requests retain the legacy
8MiB/file and32MiB total limits; large native clients use the stream protocol below.

`Content-Type: application/vnd.prims.compile-stream` requires a precise Content-Length.
Body layout: eight ASCII bytes `PRIMSC01`, one big-endian unsigned32-bit metadata byte
length, UTF-8 JSON `{jobId,task:"latex_compile",engine,path,files:[{path,size}]}`, then
exactly each file's bytes in metadata order. Metadata is limited to512KiB; file sizes
and total request length are validated before sandbox startup. Absolute/traversing
paths, links, duplicate paths, directory collisions, truncated and trailing content
are rejected. Sender and broker copy at most64KiB per buffer; no whole-project JSON,
Base64 decode, tar extraction or project mount is needed. Responses retain the existing
bounded artifact JSON protocol (32MiB total output artifacts).

Dedicated compiler budgets: one active job, worker CPU1/memory2GiB/no extra swap,
workspace tmpfs768MiB and temporary cache128MiB, noexec/nosuid/nodev, offline/non-root,
read-only root, cap-drop-all, no-new-privileges and fixed operator-approved readonly
TeX/font resources. The trusted broker alone retains the existing Docker socket;
workers and models receive neither socket nor token. The2GiB limit includes tmpfs pages.
The native runner uses the existing multipass bibliography pipeline and180-second total
job limit. Cancel/disconnect cleanup removes the disposable container. Docker container
startup has a separate bounded timeout; startup failures are reported as infrastructure
failures rather than false LaTeX errors.

Tests: `python3 test_compile_stream.py`, `python3 test_compile_broker_stream.py`, and
existing `test_worker.py`. Test scripts use synthetic data and do not require user keys.
Private deployments, images containing private fonts, documents, tokens and fixture ZIPs
must not be committed. Only copy the public source and configurable deployment examples.


## Direct bookshelf WebP pages

The bookshelf now derives page images directly from the immutable successful compile
PDF. Editor and download APIs retain the original PDF. Dedicated broker `POST
/render-pages?start=1&count=8` accepts bounded `application/pdf` input and returns a
bounded stored ZIP with a typed manifest and at most eight WebP pages. All routes
remain private and Bearer-authenticated. The existing fixed worker must provide
Ghostscript, qpdf, ImageMagick convert/identify and a WebP encoder; no runtime downloads
or model-controlled shell commands are accepted. qpdf reads page count only; no PDF
optimization pass precedes rasterization.

The fixed offline renderer uses144dpi, at most1200x2400 pixels, WebP quality82, CPU1,
nice19/shares64 and the existing768MiB/no-network/read-only/non-root sandbox. Native
compilation preempts within250ms; newly higher-interest previews win at the next
batch boundary. Completed batches persist privately and resume without waiting a
fixed15seconds between successful batches. Timeout batches shrink toward one page;
an irreducible failure preserves the original PDF. Up to2048 pages are supported;
no partial book is advertised as ready.

Apply additive migration16 after15. It preserves legacy PDF columns and adds private
WebP progress, full-manifest publication,90-second reader/editor interests and durable
superseded-page cleanup. Reader interest outranks editor interest, which outranks idle
background work; each class uses first-entry FIFO and heartbeats do not move its place.
Reader and editor expirations are independent. Only inactive background projects wait
for the usual idle interval. Original PDF URLs remain available while work is pending,
failed, disabled or unavailable. Complete images use one batch of300-second Storage
signatures. Permanent project/user removal reuses durable namespace cleanup; stale
job image prefixes are separately reclaimed without touching the original PDF.

Run `python3 test_webp_renderer.py` plus the existing PDF optimizer tests. Configure
`PRIMS_PDF_PREVIEW_WORKER=enabled` only on the intended production app; keep shared-DB
preview apps disabled. No private documents, image fixtures, credentials, token files
or local deployment paths belong in the repository.


### Dual cached pages, profile v2

Each v2 page keeps the existing144dpi/max1200x2400 clear WebP and derives a second
half-width/half-height thumbnail from it. The thumbnail does not rerender the PDF.
Both images must be uploaded and validated before the complete book becomes ready.
The existing page `url/width/height` describe the clear image; optional
`thumbUrl/thumbWidth/thumbHeight` describe its cached thumbnail. Page signatures are
requested together in one bounded batch. ZIP batches contain at most17 entries
(one manifest plus two images per page), with a41MiB response limit.

v2 uses a separate immutable job/profile namespace. Until it is complete, the
original PDF remains the fallback. Current-job v1 page objects are reclaimed only
after the state moves to v2; superseded-job cleanup covers both version prefixes.
Original PDFs are outside those image prefixes and are never removed by migration.
Reader/editor priority, FIFO, Native preemption and sandbox limits remain unchanged.

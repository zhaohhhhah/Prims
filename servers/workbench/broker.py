#!/usr/bin/env python3
"""Loopback-only trusted Docker broker. Do not expose this endpoint publicly."""
import hmac
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import threading
import time
import uuid
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import pdf_optimizer
import webp_renderer
from urllib.parse import urlsplit, parse_qs
from worker import MAX_BODY, validate_job, read_stream_header, STREAM_MAX_BODY, STREAM_MAX_TOTAL

VERSION = '1.0.0'
DOCKER_HOST = os.environ.get('DOCKER_HOST', 'unix:///var/run/docker.sock')
DOCKER = ['/usr/bin/docker', '-H', DOCKER_HOST]
IMAGE = os.environ.get('PRIMS_WORKBENCH_IMAGE', '')
CACHE = os.environ.get('PRIMS_WORKBENCH_TEX_CACHE', '')
SECRET_FILE = os.environ.get('PRIMS_WORKBENCH_TOKEN_FILE', '')
FONT_DIR = os.environ.get('PRIMS_WORKBENCH_FONT_DIR', '')
FONT_CACHE = os.environ.get('PRIMS_WORKBENCH_FONT_CACHE', '')
TEXLIVE = os.environ.get('PRIMS_WORKBENCH_TEXLIVE', '')
TEXMF_CACHE = os.environ.get('PRIMS_WORKBENCH_TEXMF_CACHE', '')
BIBER_CACHE = os.environ.get('PRIMS_WORKBENCH_BIBER_CACHE', '')
COMPILE_ONLY = os.environ.get('PRIMS_WORKBENCH_PORT') == '19342'
SLOT_COUNT = 1 if COMPILE_ONLY else 2
SLOTS = threading.BoundedSemaphore(SLOT_COUNT)
LOCK = threading.Lock()
RUNS = {}
SECRET = ''


def create_timeout_seconds(value=None):
    raw = os.environ.get('PRIMS_WORKBENCH_CREATE_TIMEOUT_SECONDS', '60') if value is None else value
    try:
        seconds = int(raw)
    except (TypeError, ValueError):
        raise ValueError('Sandbox create timeout must be an integer between 5 and 120 seconds') from None
    if not 5 <= seconds <= 120:
        raise ValueError('Sandbox create timeout must be between 5 and 120 seconds')
    return seconds


CREATE_TIMEOUT_SECONDS = create_timeout_seconds()


def stage_log(job, phase, event, started, **fields):
    # Never print subprocess argv/stderr, snapshots, file paths or credentials.
    print(json.dumps({'event': event, 'phase': phase, 'jobId': job['jobId'],
          'elapsedMs': round((time.monotonic() - started) * 1000), **fields}), file=sys.stderr, flush=True)


def docker(args, timeout=8):
    return subprocess.run(DOCKER + args, stdin=subprocess.DEVNULL, capture_output=True,
                          timeout=timeout, check=False)


def remove(name):
    try:
        docker(['rm', '-f', name])
    except (OSError, subprocess.TimeoutExpired):
        pass


def optional_font_mounts():
    """Operator-only fixed resources. No request body can alter mounts."""
    if not FONT_DIR and not FONT_CACHE:
        return []
    if not FONT_DIR or not FONT_CACHE:
        raise ValueError('Configure PRIMS_WORKBENCH_FONT_DIR and PRIMS_WORKBENCH_FONT_CACHE together')
    for value in (FONT_DIR, FONT_CACHE):
        if not Path(value).is_absolute() or ',' in value or any(ord(c) < 32 or ord(c) == 127 for c in value):
            raise ValueError('Font resource paths must be absolute and contain no comma or control character')
    return [
        '--mount', f'type=bind,source={FONT_DIR},target=/usr/local/share/fonts/prims-import,readonly,bind-propagation=rprivate',
        '--mount', f'type=bind,source={FONT_CACHE},target=/var/cache/fontconfig,readonly,bind-propagation=rprivate',
    ]


def container_options(name, compile_job=False):
    options = ['create', '--pull', 'never', '-i', '--name', name, '--label', 'prims.workbench=true',
            '--network', 'none', '--user', '10001:10001', '--read-only',
            '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true',
            '--memory', '2g' if compile_job else '512m', '--memory-swap', '2g' if compile_job else '512m', '--cpus', '1', '--pids-limit', '64',
            '--ulimit', 'nofile=256:256', '--ulimit', 'fsize='+('536870912:536870912' if compile_job else '67108864:67108864'),
            '--tmpfs', '/workspace:rw,nosuid,nodev,noexec,size='+('768m' if compile_job else '128m')+',mode=0700,uid=10001,gid=10001',
            '--tmpfs', '/tmp:rw,nosuid,nodev,noexec,size='+('128m' if compile_job else '32m')+',mode=0700,uid=10001,gid=10001',
            '--mount', f'type=bind,source={CACHE},target=/cache,readonly,bind-propagation=rprivate']
    if TEXLIVE:
        if not Path(TEXLIVE).is_absolute() or ',' in TEXLIVE or any(ord(c)<32 for c in TEXLIVE):raise ValueError('Invalid fixed TeX Live resource')
        options+=['--mount',f'type=bind,source={TEXLIVE},target=/usr/local/texlive,readonly,bind-propagation=rprivate']
    if TEXMF_CACHE:
        if not Path(TEXMF_CACHE).is_absolute() or ',' in TEXMF_CACHE or any(ord(c)<32 for c in TEXMF_CACHE):raise ValueError('Invalid fixed TeX font cache resource')
        options+=['--mount',f'type=bind,source={TEXMF_CACHE},target=/texmf-cache,readonly,bind-propagation=rprivate']
    if BIBER_CACHE:
        if not Path(BIBER_CACHE).is_absolute() or ',' in BIBER_CACHE or any(ord(c)<32 or ord(c)==127 for c in BIBER_CACHE):raise ValueError('Invalid fixed Biber resource')
        options+=['--mount',f'type=bind,source={BIBER_CACHE},target=/biber-cache,readonly,bind-propagation=rprivate']
    return options + optional_font_mounts() + [IMAGE]


def execute_job(job, raw, run, source=None):
    started = time.monotonic()
    run['startedAt'] = started
    run['phase'] = 'docker_create'
    name = run['name']
    proc = None
    try:
        if run['cancel'].is_set():
            return stopped(started, cancelled=True)
        create_limit = min(CREATE_TIMEOUT_SECONDS, 180 if job['task'] == 'latex_compile' else 90)
        try:
            created = docker(container_options(name,job['task']=='latex_compile'), timeout=create_limit)
        except subprocess.TimeoutExpired:
            if run['cancel'].is_set():
                return stopped(started, cancelled=True)
            stage_log(job, 'docker_create', 'sandbox_startup_timeout', started, timeoutSeconds=create_limit)
            return failure('[sandbox_startup_timeout] Docker create exceeded '+str(create_limit)+' seconds; the LaTeX engine was not started. Server container preparation was busy; source and engine settings were not checked.', started)
        if created.returncode != 0:
            stage_log(job, 'docker_create', 'sandbox_startup_failed', started, dockerExitCode=created.returncode)
            return failure('[sandbox_startup_failed] Docker could not create the isolated compiler container; the LaTeX engine was not started.', started)
        stage_log(job, 'docker_create', 'sandbox_created', started)
        run['phase'] = 'docker_start'
        if run['cancel'].is_set():
            return stopped(started, cancelled=True)
        proc = subprocess.Popen(DOCKER + ['start', '-ai', name], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        run['phase'] = 'sandbox_execute'
        holder = {}
        def communicate():
            try:
                if source is None:
                    holder['result'] = proc.communicate(raw)
                else:
                    # The HTTP and Docker pipes are copied in bounded chunks. Never buffer a project.
                    input_error = []
                    input_pipe = proc.stdin
                    def send_input():
                        try:
                            input_pipe.write(raw)
                            remaining = sum(item['size'] for item in job['files'])
                            while remaining:
                                chunk = source.read(min(65536, remaining))
                                if not chunk:
                                    raise ValueError('Incomplete compile snapshot')
                                input_pipe.write(chunk); remaining -= len(chunk)
                        except Exception:
                            input_error.append(True)
                        finally:
                            input_pipe.close()
                    writer = threading.Thread(target=send_input, daemon=True)
                    writer.start()
                    # communicate must not flush the pipe managed by the streaming writer.
                    proc.stdin = None
                    holder['result'] = proc.communicate()
                    writer.join(timeout=3)
                    if input_error or writer.is_alive():
                        holder['error'] = True
            except Exception:
                holder['error'] = True
        thread = threading.Thread(target=communicate, daemon=True)
        thread.start()
        while thread.is_alive():
            if run['cancel'].is_set() or time.monotonic() - started >= (180 if job['task']=='latex_compile' else 90):
                remove(name)
                if proc.poll() is None:
                    proc.kill()
                thread.join(timeout=3)
                return stopped(started, cancelled=run['cancel'].is_set())
            thread.join(timeout=.15)
        if holder.get('error'):
            return failure('Workbench communication failed', started)
        stdout, _stderr = holder.get('result', (b'', b''))
        if proc.returncode != 0 or len(stdout) > (48*1024*1024 if job['task']=='latex_compile' else 192*1024):
            if run['cancel'].is_set():
                return stopped(started, cancelled=True)
            return failure('Workbench sandbox exited without a result', started)
        run['phase'] = 'result_validation'
        try:
            result = json.loads(stdout)
        except (ValueError, TypeError):
            return failure('Workbench returned an invalid result', started)
        allowed = {'ok', 'exitCode', 'command', 'output', 'truncated', 'timedOut', 'cancelled',
                   'durationMs', 'fonts', 'fontScope', 'fontName', 'available', 'fontMatches'}
        if job['task']=='latex_compile':allowed={'ok','exitCode','output','truncated','timedOut','cancelled','durationMs','artifacts'}
        if not isinstance(result, dict) or set(result) - allowed or not isinstance(result.get('output'), str):
            return failure('Workbench returned an invalid result', started)
        # Defence in depth: the worker never receives this broker credential.
        result['output'] = result['output'].replace(SECRET, '[redacted]')
        return result
    finally:
        remove(name)
        if proc is not None and proc.poll() is None:
            proc.kill()
        with LOCK:
            RUNS.pop(job['jobId'], None)
        SLOTS.release()


def failure(message, started):
    return {'ok': False, 'exitCode': 70, 'command': [], 'output': message, 'truncated': False,
            'timedOut': False, 'cancelled': False, 'durationMs': round((time.monotonic() - started) * 1000)}


def stopped(started, cancelled=False):
    result = failure('Workbench job cancelled' if cancelled else 'Workbench exceeded its total time limit', started)
    result.update(exitCode=130 if cancelled else 124, cancelled=cancelled, timedOut=not cancelled)
    return result


class Handler(BaseHTTPRequestHandler):
    server_version = 'PrimsWorkbench/' + VERSION
    def log_message(self, *_args):
        pass

    def respond(self, status, body):
        encoded = json.dumps(body, ensure_ascii=False).encode()
        try:
            self.send_response(status)
            self.send_header('Content-Type', 'application/json; charset=utf-8')
            self.send_header('Content-Length', str(len(encoded)))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            self.wfile.write(encoded)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def authorized(self):
        value = self.headers.get('Authorization', '')
        if not hmac.compare_digest(value, 'Bearer ' + SECRET):
            self.respond(401, {'error': 'Unauthorized'})
            self.close_connection = True
            return False
        return True

    def do_GET(self):
        if not self.authorized():
            return
        if self.path != '/health':
            self.respond(404, {'error': 'Not found'})
            return
        try:
            check = docker(['image', 'inspect', IMAGE, '--format', '{{.Id}}'], timeout=3)
            healthy = check.returncode == 0 and check.stdout.decode().strip() == IMAGE
        except (OSError, subprocess.TimeoutExpired):
            healthy = False
        self.respond(200 if healthy else 503, {'ok': healthy, 'version': VERSION,
            'capabilities': ((['font_check','list_fonts','latex_compile'] if TEXLIVE else []) if COMPILE_ONLY else ['latex_check','font_check','list_fonts']) if healthy else [],
            'pdfPreviewOptimizer': COMPILE_ONLY, 'webpPreviewRenderer': COMPILE_ONLY, 'maxConcurrentPdfPreviews': 1,
            'maxConcurrentJobs': SLOT_COUNT, 'snapshotMaxBytes': 32 * 1024 * 1024,
            'commandTimeoutSeconds': 45, 'jobTimeoutSeconds': 90, 'maxOutputBytes': 65536,
            'sandboxCreateTimeoutSeconds':CREATE_TIMEOUT_SECONDS,'compileJobTimeoutSeconds':180,'compileSnapshotMaxBytes':STREAM_MAX_TOTAL,'compileStreamProtocol':'prims-compile-v1','compileArtifactTotalMaxBytes':32*1024*1024})

    def optimize_pdf(self):
        if not COMPILE_ONLY:
            self.respond(404, {'error': 'Not found'})
            return
        try:
            length = int(self.headers.get('Content-Length', '-1'))
            if self.headers.get('Transfer-Encoding') or not 8 <= length <= pdf_optimizer.MAX_PDF:
                self.respond(413, {'error': 'PDF exceeds preview limit'})
                self.close_connection = True
                return
            if self.headers.get('Content-Type') != 'application/pdf':
                self.respond(400, {'error': 'PDF required'})
                return
            self.connection.settimeout(30)
            raw = self.rfile.read(length)
            if len(raw) != length: raise ValueError('Incomplete PDF')
            def busy():
                with LOCK: return bool(RUNS)
            code, output = pdf_optimizer.optimize(raw, DOCKER, IMAGE, busy)
            self.send_response(code)
            self.send_header('Content-Type', 'application/pdf' if code == 200 else 'text/plain')
            self.send_header('Content-Length', str(len(output)))
            self.end_headers()
            self.wfile.write(output)
        except (OSError, ValueError, subprocess.TimeoutExpired):
            self.close_connection = True

    def render_pages(self):
        if not COMPILE_ONLY:
            self.respond(404, {'error':'Not found'});return
        try:
            parts=urlsplit(self.path);params=parse_qs(parts.query,strict_parsing=True)
            if parts.path!='/render-pages' or set(params)!={'start','count'} or any(len(v)!=1 for v in params.values()):raise ValueError('Invalid range')
            start=int(params['start'][0]);count=int(params['count'][0])
            if not 1<=start<=webp_renderer.MAX_PAGES or not 1<=count<=webp_renderer.MAX_BATCH:raise ValueError('Invalid range')
            length=int(self.headers.get('Content-Length','-1'))
            if self.headers.get('Transfer-Encoding') or not 8<=length<=pdf_optimizer.MAX_PDF:
                self.respond(413,{'error':'PDF exceeds preview limit'});self.close_connection=True;return
            if self.headers.get('Content-Type')!='application/pdf':raise ValueError('PDF required')
            self.connection.settimeout(30);raw=self.rfile.read(length)
            if len(raw)!=length:raise ValueError('Incomplete PDF')
            def busy():
                with LOCK:return bool(RUNS)
            code,output=webp_renderer.render(raw,DOCKER,IMAGE,busy,start,count)
            self.send_response(code);self.send_header('Content-Type','application/zip' if code==200 else 'text/plain');self.send_header('Content-Length',str(len(output)));self.end_headers();self.wfile.write(output)
        except (ValueError,TypeError):
            self.respond(400,{'error':'Invalid page batch'});self.close_connection=True
        except (OSError,subprocess.TimeoutExpired):self.close_connection=True

    def do_POST(self):
        if not self.authorized():
            return
        if urlsplit(self.path).path == '/render-pages':
            self.render_pages()
            return
        if self.path == '/optimize':
            self.optimize_pdf()
            return
        if self.path != '/run':
            self.respond(404, {'error': 'Not found'})
            return
        if self.headers.get('Transfer-Encoding'):
            self.respond(400, {'error': 'Content-Length required'})
            self.close_connection = True
            return
        acquired = False
        try:
            length = int(self.headers.get('Content-Length', '-1'))
            streaming = self.headers.get('Content-Type', '').split(';')[0] == 'application/vnd.prims.compile-stream'
            if length <= 0 or length > (STREAM_MAX_BODY if streaming else MAX_BODY):
                self.respond(413, {'error': 'Request exceeds snapshot limit'})
                self.close_connection = True
                return
            if not SLOTS.acquire(blocking=False):
                self.respond(429, {'error': 'Workbench is busy; snapshot/job capacity reached are accepted'})
                self.close_connection = True
                return
            acquired = True
            self.connection.settimeout(30)
            if streaming:
                if not COMPILE_ONLY:
                    raise ValueError('Streaming is available on compile service only')
                job, raw, _total = read_stream_header(self.rfile, length)
            else:
                raw = self.rfile.read(length)
                if len(raw) != length:
                    raise ValueError('Incomplete request')
                job = json.loads(raw)
                validate_job(job)
            if COMPILE_ONLY and job['task'] not in ('latex_compile','font_check','list_fonts'):
                raise ValueError('Native compile service only accepts compile and font diagnostics')
        except (ValueError, TypeError, KeyError, OSError, RecursionError):
            if acquired:
                SLOTS.release()
            self.respond(400, {'error': 'Invalid workbench snapshot or task'})
            self.close_connection = True
            return
        with LOCK:
            if job['jobId'] in RUNS:
                SLOTS.release()
                self.respond(409, {'error': 'jobId is already running'})
                return
            run = {'name': 'prims-workbench-' + job['jobId'] + '-' + uuid.uuid4().hex[:8],
                   'cancel': threading.Event()}
            RUNS[job['jobId']] = run
        try:
            result = execute_job(job, raw, run, self.rfile if streaming else None)
        except Exception as error:
            started = run.get('startedAt', time.monotonic())
            phase = run.get('phase', 'broker')
            stage_log(job, phase, 'sandbox_infrastructure_error', started, errorClass=type(error).__name__)
            result = failure('[sandbox_infrastructure_error] Isolated compiler infrastructure failed at '+phase+'; inspect the private broker diagnostics before changing source or engine settings.', started)
        self.respond(200, result)

    def do_DELETE(self):
        if not self.authorized():
            return
        if not re.fullmatch(r'/runs/[0-9a-f-]{36}', self.path):
            self.respond(404, {'error': 'Not found'})
            return
        with LOCK:
            run = RUNS.get(self.path.rsplit('/', 1)[1])
            if run:
                run['cancel'].set()
        if run:
            # A pending create is checked again before start; active jobs are stopped by both threads.
            remove(run['name'])
        self.respond(202 if run else 404, {'cancelled': bool(run)})


def main():
    global SECRET
    if not DOCKER_HOST.startswith('unix:///') or any(ord(c)<32 for c in DOCKER_HOST):
        raise SystemExit('Configure a local Unix Docker socket')
    if not re.fullmatch(r'sha256:[0-9a-f]{64}', IMAGE):
        raise SystemExit('Pin PRIMS_WORKBENCH_IMAGE to the approved worker image ID')
    if not CACHE or not Path(CACHE).is_absolute() or ',' in CACHE or any(ord(c)<32 for c in CACHE):
        raise SystemExit('Configure the trusted public TeX cache directory')
    try:
        optional_font_mounts()
    except ValueError as error:
        raise SystemExit(str(error)) from None
    secret_path = Path(SECRET_FILE)
    if not SECRET_FILE or not secret_path.is_file() or stat.S_IMODE(secret_path.stat().st_mode) & 0o077:
        raise SystemExit('A private mode-0600 token file is required')
    SECRET = secret_path.read_text().strip()
    if len(SECRET) < 32 or any(c.isspace() for c in SECRET):
        raise SystemExit('Invalid broker token')
    port=int(os.environ.get('PRIMS_WORKBENCH_PORT','19340'))
    if port not in (19340,19342):raise SystemExit('Use an approved loopback broker port')
    server = ThreadingHTTPServer(('127.0.0.1', port), Handler)
    server.daemon_threads = True
    def shutdown(_signum, _frame):
        with LOCK:
            jobs = list(RUNS.values())
            for run in jobs:
                run['cancel'].set()
        for run in jobs:
            remove(run['name'])
        threading.Thread(target=server.shutdown, daemon=True).start()
    signal.signal(signal.SIGTERM, shutdown)
    signal.signal(signal.SIGINT, shutdown)
    server.serve_forever(poll_interval=.2)
    server.server_close()

if __name__ == '__main__':
    main()

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
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from worker import MAX_BODY, validate_job

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
SLOTS = threading.BoundedSemaphore(2)
LOCK = threading.Lock()
RUNS = {}
SECRET = ''


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
            '--memory', '1g' if compile_job else '512m', '--memory-swap', '1g' if compile_job else '512m', '--cpus', '1', '--pids-limit', '64',
            '--ulimit', 'nofile=256:256', '--ulimit', 'fsize=67108864:67108864',
            '--tmpfs', '/workspace:rw,nosuid,nodev,noexec,size='+('256m' if compile_job else '128m')+',mode=0700,uid=10001,gid=10001',
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


def execute_job(job, raw, run):
    started = time.monotonic()
    name = run['name']
    proc = None
    try:
        if run['cancel'].is_set():
            return stopped(started, cancelled=True)
        created = docker(container_options(name,job['task']=='latex_compile'), timeout=12)
        if created.returncode != 0:
            return failure('Workbench sandbox could not be created', started)
        if run['cancel'].is_set():
            return stopped(started, cancelled=True)
        proc = subprocess.Popen(DOCKER + ['start', '-ai', name], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        holder = {}
        def communicate():
            try:
                holder['result'] = proc.communicate(raw)
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
        try:
            result = json.loads(stdout)
        except (ValueError, TypeError):
            return failure('Workbench returned an invalid result', started)
        allowed = {'ok', 'exitCode', 'command', 'output', 'truncated', 'timedOut', 'cancelled',
                   'durationMs', 'fonts', 'fontScope'}
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
            'capabilities': (([] if COMPILE_ONLY else ['latex_check', 'font_check', 'list_fonts'])+(['latex_compile'] if TEXLIVE else [])) if healthy else [],
            'maxConcurrentJobs': 2, 'snapshotMaxBytes': 32 * 1024 * 1024,
            'commandTimeoutSeconds': 45, 'jobTimeoutSeconds': 90, 'maxOutputBytes': 65536,
            'compileJobTimeoutSeconds':180,'compileSnapshotMaxBytes':32*1024*1024,'compileArtifactTotalMaxBytes':32*1024*1024})

    def do_POST(self):
        if not self.authorized():
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
            if length <= 0 or length > MAX_BODY:
                self.respond(413, {'error': 'Request exceeds snapshot limit'})
                self.close_connection = True
                return
            if not SLOTS.acquire(blocking=False):
                self.respond(429, {'error': 'Workbench is busy; at most two snapshots/jobs are accepted'})
                self.close_connection = True
                return
            acquired = True
            self.connection.settimeout(15)
            raw = self.rfile.read(length)
            if len(raw) != length:
                raise ValueError('Incomplete request')
            job = json.loads(raw)
            validate_job(job)
            if COMPILE_ONLY and job['task'] != 'latex_compile':
                raise ValueError('Dedicated compiler only accepts latex_compile')
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
            result = execute_job(job, raw, run)
        except Exception:
            result = failure('Workbench broker failed safely', time.monotonic())
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

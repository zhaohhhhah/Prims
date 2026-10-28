#!/usr/bin/env python3
"""Fixed-command, disposable LaTeX/font worker. No network or host credentials."""
import base64
import json
import os
from pathlib import Path, PurePosixPath
import re
import selectors
import signal
import subprocess
import sys
import time

VERSION = '1.0.0'
MAX_BODY = 46 * 1024 * 1024
MAX_TOTAL = 32 * 1024 * 1024
MAX_FILE = 8 * 1024 * 1024
MAX_OUTPUT = 64 * 1024
ROOT = Path('/workspace')


def safe_path(value):
    if not isinstance(value, str) or not value or len(value.encode('utf-8')) > 512:
        raise ValueError('Invalid project path')
    if value.startswith('/') or '\\' in value or ':' in value or any(ord(c) < 32 or ord(c) == 127 for c in value):
        raise ValueError('Invalid project path')
    parts = value.split('/')
    if any(p in ('', '.', '..') or p.startswith('-') for p in parts) or parts[0] in ('.output', '.workbench'):
        raise ValueError('Invalid project path')
    if str(PurePosixPath(value)) != value:
        raise ValueError('Invalid project path')
    return value


def validate_job(job):
    if not isinstance(job, dict) or set(job) - {'jobId', 'task', 'path', 'files', 'engine'}:
        raise ValueError('Invalid job fields')
    import uuid
    if not isinstance(job.get('jobId'), str) or str(uuid.UUID(job['jobId'])) != job['jobId']:
        raise ValueError('Invalid jobId')
    if job.get('task') not in ('latex_check', 'font_check', 'list_fonts', 'latex_compile'):
        raise ValueError('Unsupported task')
    if job['task']=='latex_compile':
        if job.get('engine') not in ('pdflatex','xelatex','lualatex'):
            raise ValueError('Unsupported compile engine')
    elif 'engine' in job:
        raise ValueError('Engine is only accepted for compile jobs')
    files = job.get('files', [])
    if not isinstance(files, list) or len(files) > 256:
        raise ValueError('Too many files')
    # Bound aggregate decoded size before decoding any file.
    estimated_total = 0
    for item in files:
        if not isinstance(item, dict) or not isinstance(item.get('contentBase64'), str):
            raise ValueError('Invalid file fields')
        encoded = item['contentBase64']
        padding = len(encoded) - len(encoded.rstrip('='))
        if len(encoded) % 4 or padding > 2 or len(encoded) > (MAX_FILE + 2) // 3 * 4:
            raise ValueError('Invalid base64 or file size')
        estimated = len(encoded) // 4 * 3 - padding
        estimated_total += estimated
        if estimated > MAX_FILE or estimated_total > MAX_TOTAL:
            raise ValueError('Project snapshot exceeds limits')
    seen = set()
    decoded = []
    total = 0
    for item in files:
        if not isinstance(item, dict) or set(item) != {'path', 'contentBase64'}:
            raise ValueError('Invalid file fields')
        path = safe_path(item['path'])
        if path in seen or any(path.startswith(p + '/') or p.startswith(path + '/') for p in seen):
            raise ValueError('Duplicate or colliding paths')
        encoded = item['contentBase64']
        if not isinstance(encoded, str) or len(encoded) > (MAX_FILE + 2) // 3 * 4:
            raise ValueError('File exceeds 8MiB limit')
        try:
            content = base64.b64decode(encoded, validate=True)
        except Exception:
            raise ValueError('Invalid base64') from None
        total += len(content)
        if len(content) > MAX_FILE or total > MAX_TOTAL:
            raise ValueError('Project snapshot exceeds limits')
        seen.add(path)
        decoded.append((path, content))
    path = job.get('path')
    if path is not None:
        safe_path(path)
        if path not in seen:
            raise ValueError('Requested path is not in the snapshot')
    if job['task'] in ('latex_check','latex_compile') and (path is None or not path.lower().endswith(('.tex', '.ltx', '.latex'))):
        raise ValueError('latex_check requires a project LaTeX path')
    if job['task'] == 'font_check' and path is not None and not path.lower().endswith(('.ttf', '.otf', '.ttc')):
        raise ValueError('font_check requires a project font path')
    if job['task'] == 'list_fonts' and (files or path is not None):
        raise ValueError('list_fonts does not accept a project snapshot')
    return decoded


def clean_output(text):
    # Strip terminal escapes/control bytes. Project logs are untrusted data.
    text = re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', text)
    return ''.join(c for c in text if c in '\n\t' or ord(c) >= 32 and ord(c) != 127)


def run_command(argv, deadline, env):
    proc = subprocess.Popen(argv, cwd=ROOT, env=env, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    selector = selectors.DefaultSelector()
    selector.register(proc.stdout, selectors.EVENT_READ)
    output = bytearray()
    truncated = False
    timed_out = False
    try:
        while selector.get_map():
            if time.monotonic() >= deadline:
                timed_out = True
                os.killpg(proc.pid, signal.SIGKILL)
                break
            for key, _ in selector.select(min(.15, max(.001, deadline - time.monotonic()))):
                chunk = os.read(key.fileobj.fileno(), 4096)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                available = MAX_OUTPUT - len(output)
                output.extend(chunk[:available])
                truncated |= len(chunk) > available
        proc.wait(timeout=2)
    finally:
        selector.close()
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait(timeout=2)
        proc.stdout.close()
    return proc.returncode, clean_output(output.decode('utf-8', errors='replace')), truncated, timed_out


def bibliography_failed(log):
    return bool(re.search(r'(?im)(?:\bERRORS:\s*[1-9]\d*|^.*\bERROR\s+-|\bThere (?:was|were) [1-9]\d* error message)',log))


def execute(job):
    started = time.monotonic()
    decoded = validate_job(job)
    for path, content in decoded:
        target = ROOT / path
        target.parent.mkdir(parents=True, exist_ok=True)
        fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'wb') as f:
            f.write(content)
    Path('/tmp/home').mkdir(exist_ok=True)
    config = '/tmp/project-fonts.conf'
    Path(config).write_text('<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd"><fontconfig><include ignore_missing="yes">/etc/fonts/fonts.conf</include><dir>/workspace</dir><cachedir>/tmp/fontconfig</cachedir></fontconfig>')
    env = {'PATH': '/usr/local/bin:/usr/local/texlive/2025/bin/x86_64-linux:/usr/bin:/bin', 'HOME': '/tmp/home', 'TMPDIR': '/tmp',
           'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8', 'TECTONIC_CACHE_DIR': '/cache',
           'XDG_CACHE_HOME': '/cache', 'FONTCONFIG_FILE': config, 'FONTCONFIG_PATH': '/etc/fonts'}
    env.update(TEXMFVAR='/tmp/texmf-var',TEXMFCONFIG='/tmp/texmf-config',TEXMFCACHE='/texmf-cache:/usr/local/texlive/2025/texmf-var:/tmp/texmf-var',openin_any='r',openout_any='p')
    if job['task']=='latex_compile':
        argv=['/usr/local/bin/scribe-tex-runner',job['engine'],job['path']]
        code,output,truncated,timed_out=run_command(argv,started+165,env)
        try:
            parsed=json.loads(output)
            if not isinstance(parsed,dict) or not isinstance(parsed.get('output'),str):raise ValueError('Invalid runner response')
        except (ValueError,TypeError):
            parsed={'ok':False,'exitCode':code or 70,'output':output or 'TeX runner did not produce a result'}
        result={'ok':bool(parsed.get('ok')) and code==0,'exitCode':parsed.get('exitCode',code),'output':clean_output(parsed['output'])[:65536],'truncated':truncated,'timedOut':timed_out,'cancelled':False,'durationMs':round((time.monotonic()-started)*1000),'artifacts':{}}
        total=0;stem=Path(job['path']).stem
        for suffix,limit in [('pdf',24*1024*1024),('synctex.gz',4*1024*1024),('log',1024*1024),('bbl',2*1024*1024)]:
            artifact=ROOT/(stem+'.'+suffix)
            if artifact.is_file() and not artifact.is_symlink():
                size=artifact.stat().st_size
                if size>limit or total+size>32*1024*1024:raise ValueError('Compile artifact exceeds limit')
                data=artifact.read_bytes();total+=len(data)
                if suffix=='pdf' and not data.startswith(b'%PDF'):raise ValueError('Invalid PDF output')
                result['artifacts'][suffix]=base64.b64encode(data).decode('ascii')
        aux=ROOT/(stem+'.aux')
        aux_text=aux.read_bytes()[:1024*1024] if aux.is_file() else b''
        needs_bibliography=b'\\citation{' in aux_text or b'\\abx@aux@cite{' in aux_text
        blg=ROOT/(stem+'.blg')
        if result['ok'] and needs_bibliography and blg.is_file() and bibliography_failed(blg.read_text(errors='replace')[:1024*1024]):
            result.update(ok=False,exitCode=1,output=result['output']+'\nBibliography processing reported errors; PDF is incomplete.')
        if result['ok'] and needs_bibliography and not (ROOT/(stem+'.bbl')).is_file():
            result.update(ok=False,exitCode=1,output=result['output']+'\nBibliography processing did not produce a BBL; PDF is incomplete.')
        if result['ok'] and 'pdf' not in result['artifacts']:result.update(ok=False,exitCode=1,output=result['output']+'\nNo PDF produced')
        return result
    deadline = started + 45
    task = job['task']
    if task == 'latex_check':
        (ROOT / '.output').mkdir()
        commands = [['/usr/local/bin/tectonic', '--untrusted', '--only-cached', '--keep-logs',
                     '--print', '--outfmt', 'xdv', '--outdir', '/workspace/.output', '/workspace/' + job['path']]]
    elif task == 'font_check':
        paths = [job['path']] if job.get('path') else [p for p, _ in decoded if p.lower().endswith(('.ttf', '.otf', '.ttc'))]
        commands = [['/usr/bin/fc-scan', '--format', '%{family}\t%{style}\t%{postscriptname}\n', '/workspace/' + p] for p in paths]
    else:
        commands = [['/usr/bin/fc-list', '--format', '%{family}\t%{style}\n']]
    result = {'ok': True, 'exitCode': 0, 'command': commands[0] if commands else [],
              'output': '', 'truncated': False, 'timedOut': False, 'cancelled': False,
              'durationMs': 0, 'fontScope': 'project-job-only'}
    fonts = []
    for index, argv in enumerate(commands):
        code, output, truncated, timed_out = run_command(argv, deadline, env)
        heading = f'[{index + 1}/{len(commands)}] ' + (argv[-1].removeprefix('/workspace/') if task == 'font_check' else task) + '\n'
        result['output'] += heading + output
        if len(result['output'].encode()) > MAX_OUTPUT:
            result['output'] = result['output'].encode()[:MAX_OUTPUT].decode('utf-8', errors='ignore')
            result['truncated'] = True
        result['truncated'] |= truncated
        result['timedOut'] |= timed_out
        if code != 0:
            result['exitCode'] = code
            result['ok'] = False
        if task in ('font_check', 'list_fonts') and code == 0:
            for line in output.splitlines():
                if line and line not in fonts and len(fonts) < 100:
                    fonts.append(line[:256])
        if timed_out:
            break
    if task == 'font_check' and not commands:
        result['output'] = 'No TTF/OTF/TTC fonts in this project snapshot. Upload a font file to the project, then use fontspec with Path=...; no system fonts were installed.'
    if task in ('font_check', 'list_fonts'):
        result['fonts'] = fonts
    result['durationMs'] = round((time.monotonic() - started) * 1000)
    return result


def main():
    try:
        raw = sys.stdin.buffer.read(MAX_BODY + 1)
        if len(raw) > MAX_BODY:
            raise ValueError('Request too large')
        result = execute(json.loads(raw))
    except (ValueError, TypeError, KeyError) as error:
        result = {'ok': False, 'exitCode': 2, 'command': [], 'output': str(error), 'truncated': False,
                  'timedOut': False, 'cancelled': False, 'durationMs': 0}
    except Exception:
        result = {'ok': False, 'exitCode': 70, 'command': [], 'output': 'Workbench worker failed safely',
                  'truncated': False, 'timedOut': False, 'cancelled': False, 'durationMs': 0}
    sys.stdout.write(json.dumps(result, ensure_ascii=False))

if __name__ == '__main__':
    main()

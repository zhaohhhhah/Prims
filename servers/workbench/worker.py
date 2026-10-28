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
import struct
import shutil

VERSION = '1.0.0'
MAX_BODY = 46 * 1024 * 1024
MAX_TOTAL = 32 * 1024 * 1024
MAX_FILE = 8 * 1024 * 1024
MAX_OUTPUT = 64 * 1024
ROOT = Path('/workspace')


# Compile snapshots use a bounded binary stream; diagnostic JSON remains compatible.
STREAM_MAGIC = b'PRIMSC01'
STREAM_MAX_TOTAL = 500 * 1024 * 1024
STREAM_MAX_META = 512 * 1024
STREAM_MAX_FILES = 4096
STREAM_MAX_BODY = STREAM_MAX_TOTAL + STREAM_MAX_META + 12


def read_exact(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.read(min(65536, size - len(data)))
        if not chunk:
            raise ValueError('Incomplete compile snapshot')
        data.extend(chunk)
    return bytes(data)


def read_stream_header(stream, content_length=None):
    if read_exact(stream, 8) != STREAM_MAGIC:
        raise ValueError('Invalid compile stream')
    size = struct.unpack('>I', read_exact(stream, 4))[0]
    if not 0 < size <= STREAM_MAX_META:
        raise ValueError('Compile metadata exceeds limit')
    header = read_exact(stream, size)
    job = json.loads(header)
    if not isinstance(job, dict) or set(job) != {'jobId', 'task', 'engine', 'path', 'files'} or job['task'] != 'latex_compile':
        raise ValueError('Invalid compile stream fields')
    files = job['files']
    if not isinstance(files, list) or not 0 < len(files) <= STREAM_MAX_FILES:
        raise ValueError('Too many compile files')
    # Reuse all legacy identity, path and engine rules without materializing content.
    shadow = dict(job, files=[])
    seen = set(); total = 0
    for item in files:
        if not isinstance(item, dict) or set(item) != {'path', 'size'} or type(item['size']) is not int or not 0 <= item['size'] <= STREAM_MAX_TOTAL:
            raise ValueError('Invalid compile file size')
        path = safe_path(item['path'])
        if path in seen or any(path.startswith(p+'/') or p.startswith(path+'/') for p in seen):
            raise ValueError('Duplicate or colliding compile paths')
        seen.add(path); total += item['size']
        if total > STREAM_MAX_TOTAL:
            raise ValueError('Compile source exceeds 500MiB limit')
    # validate_job's file-count budget intentionally stays unchanged for legacy diagnostics.
    validate_job(dict(shadow, files=[{'path':job['path'],'contentBase64':''}]))
    if job['path'] not in seen:
        raise ValueError('Main document is absent')
    if content_length is not None and content_length != 12 + size + total:
        raise ValueError('Compile stream length mismatch')
    return job, STREAM_MAGIC + struct.pack('>I', size) + header, total


def materialize_stream(stream, job):
    for item in job['files']:
        target = ROOT / item['path']
        target.parent.mkdir(parents=True, exist_ok=True)
        fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'wb') as output:
            remaining = item['size']
            while remaining:
                chunk = stream.read(min(65536, remaining))
                if not chunk:
                    raise ValueError('Incomplete compile snapshot')
                output.write(chunk); remaining -= len(chunk)
    if stream.read(1):
        raise ValueError('Unexpected trailing compile data')


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


def validate_font_name(value):
    if not isinstance(value, str) or not value.strip() or len(value) > 120:
        raise ValueError('Font name must contain 1-120 characters')
    value = value.strip()
    if value.startswith('-') or any(c in value for c in '/\\:') or any(ord(c)<32 or ord(c)==127 for c in value):
        raise ValueError('Font name cannot contain paths, options or control characters')
    return value


def font_records(text):
    records=[]
    for line in text.splitlines():
        fields=line.split('\t')
        if len(fields)!=3:continue
        family,postscript,path=fields
        records.append({'family':family[:300],'postscriptName':postscript[:300], 'fileName':Path(path).name[:256]})
    return records


def matching_font_records(records, name):
    key=name.casefold(); result=[];seen=set()
    for record in records:
        names=[v.strip().casefold() for field in ('family','postscriptName') for v in record[field].split(',')]
        file=record['fileName']
        if key not in names and key not in (file.casefold(),Path(file).stem.casefold()):continue
        identity=(record['family'],record['postscriptName'],file)
        if identity not in seen:seen.add(identity);result.append(record)
        if len(result)>=20:break
    return result


def named_font_check(name, deadline, env):
    name=validate_font_name(name);records=[];truncated=False;timed_out=False;failed=False
    escaped=''.join('\\'+c if c in '\\-,:=' else c for c in name)
    fmt='%{family}\t%{postscriptname}\t%{file}\n'
    for field in ('family','postscriptname'):
        code,text,cut,timeout=run_command(['/usr/bin/fc-list','--format',fmt,':'+field+'='+escaped],deadline,env)
        records.extend(font_records(text));truncated|=cut;timed_out|=timeout;failed|=code!=0
    matches=matching_font_records(records,name)
    # TeX's font-file database is part of the actual native compile environment;
    # these files need not be registered as OS font families for fontspec/xeCJK.
    kpse=Path('/usr/local/texlive/2025/bin/x86_64-linux/kpsewhich')
    if not matches and kpse.is_file() and time.monotonic()<deadline:
        candidates=[name] if name.lower().endswith(('.otf','.ttf','.ttc')) else [name+'.otf',name+'.ttf',name+'.ttc']
        for candidate in candidates:
            code,text,cut,timeout=run_command([str(kpse),candidate],deadline,env);truncated|=cut;timed_out|=timeout
            if code!=0:continue
            for value in text.splitlines()[:4]:
                path=Path(value)
                if not path.is_absolute() or not path.resolve().is_relative_to(Path('/usr/local/texlive')) or not path.is_file():continue
                scan_code,scan_text,scan_cut,scan_timeout=run_command(['/usr/bin/fc-scan','--format',fmt,str(path)],deadline,env)
                truncated|=scan_cut;timed_out|=scan_timeout
                if scan_code==0:matches.extend(matching_font_records(font_records(scan_text),name))
            if matches:break
    available=True if matches else None if failed or timed_out or truncated else False
    output=('Available in the editor compilation environment: ' if available else 'Compilation font lookup was incomplete: ' if available is None else 'No exact font match in the editor compilation environment: ')+name
    return {'ok':not failed and not timed_out,'exitCode':0 if not failed and not timed_out else 1,'output':output,'fonts':[m['family']+'\t'+m['postscriptName'] for m in matches[:20]],'fontName':name,'available':available,'fontMatches':matches[:20],'fontScope':'compilation','truncated':truncated,'timedOut':timed_out,'cancelled':False}


def validate_job(job):
    if not isinstance(job, dict) or set(job) - {'jobId', 'task', 'path', 'files', 'engine', 'fontName'}:
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
    if 'fontName' in job:
        validate_font_name(job['fontName'])
        if job['task'] != 'font_check' or job.get('path') is not None or job.get('files'):
            raise ValueError('Named font checks cannot select paths or project snapshots')
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


def execute(job, materialized=False):
    started = time.monotonic()
    decoded = [] if materialized else validate_job(job)
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
    if job['task']=='font_check' and job.get('fontName') is not None:
        result=named_font_check(job['fontName'],started+45,env);result['durationMs']=round((time.monotonic()-started)*1000);return result
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
              'durationMs': 0, 'fontScope': 'compilation' if task == 'list_fonts' else 'project-font-file'}
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
        result['output'] = 'No project font files were supplied. Check an existing compilation font with fontName, or use list_fonts; project font uploads are optional.'
    if task in ('font_check', 'list_fonts'):
        result['fonts'] = fonts
    result['durationMs'] = round((time.monotonic() - started) * 1000)
    return result


def main():
    try:
        stream = sys.stdin.buffer
        first = read_exact(stream, 8)
        if first == STREAM_MAGIC:
            class PrefixedStream:
                def __init__(self, prefix, source): self.prefix = prefix; self.source = source
                def read(self, size):
                    if self.prefix:
                        part = self.prefix[:size]; self.prefix = self.prefix[size:]
                        return part
                    return self.source.read(size)
            job, _prefix, _total = read_stream_header(PrefixedStream(first, stream))
            materialize_stream(stream, job)
            result = execute(job, materialized=True)
        else:
            raw = first + stream.read(MAX_BODY + 1 - len(first))
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

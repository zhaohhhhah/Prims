"""Fixed, offline bookshelf PDF derivative. No request-controlled commands/paths."""
import subprocess
import threading
import time
import uuid

MAX_PDF = 64 * 1024 * 1024
SLOT = threading.BoundedSemaphore(1)
# Runs inside the disposable worker only; PDF bytes enter stdin, PDF bytes leave stdout.
WORKER = r'''import json, pathlib, subprocess, sys
root=pathlib.Path('/workspace')
source=root/'source.pdf'
source.write_bytes(sys.stdin.buffer.read(67108865))
if source.stat().st_size>67108864 or not source.read_bytes().startswith(b'%PDF-'): sys.exit(2)
def run(args):
    result=subprocess.run(args,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=60)
    if result.returncode not in (0,3): raise ValueError('PDF operation failed')
    return result.stdout
def geometry(path):
    doc=json.loads(run(['/usr/bin/qpdf','--json','--json-stream-data=none',str(path)]))
    objects=doc['qpdf'][1]
    def value(ref,seen=None):
        seen=set() if seen is None else seen
        while isinstance(ref,str) and 'obj:'+ref in objects:
            if ref in seen: raise ValueError('Circular PDF reference')
            seen.add(ref); ref=objects['obj:'+ref].get('value',{})
        if isinstance(ref,list): return [value(item,set(seen)) for item in ref]
        return ref
    def inherited(page,key,default=None):
        seen=set()
        while isinstance(page,dict):
            if key in page: return value(page[key])
            ref=page.get('/Parent')
            if ref is None or ref in seen: break
            seen.add(ref); page=value(ref)
        return default
    result=[]
    for page in doc['pages']:
        obj=value(page['object'])
        media=inherited(obj,'/MediaBox')
        crop=inherited(obj,'/CropBox',media)
        if not isinstance(media,list) or not isinstance(crop,list): raise ValueError('Page geometry unavailable')
        result.append((media,crop,int(inherited(obj,'/Rotate',0))%360,inherited(obj,'/UserUnit',1)))
    if not result: raise ValueError('No pages')
    return result
original=geometry(source)
linear=root/'linear.pdf'
run(['/usr/bin/qpdf','--linearize','--object-streams=generate','--compress-streams=y','--recompress-flate','--compression-level=9',str(source),str(linear)])
if geometry(linear)!=original: raise ValueError('Lossless geometry changed')
choice=linear
# Raster downsampling is a preview-only candidate; retain lossless copy if validation fails.
try:
    compact=root/'compact.pdf'
    run(['/usr/bin/gs','-q','-dSAFER','-dBATCH','-dNOPAUSE','-sDEVICE=pdfwrite','-dCompatibilityLevel=1.7','-dPDFSETTINGS=/ebook','-dAutoRotatePages=/None','-dPreserveAnnots=true','-dColorImageResolution=144','-dGrayImageResolution=144','-dMonoImageResolution=300','-dDetectDuplicateImages=true','-sOutputFile='+str(compact),str(source)])
    candidate=root/'candidate.pdf'
    if geometry(compact)==original:
        run(['/usr/bin/qpdf','--linearize','--object-streams=generate',str(compact),str(candidate)])
        if geometry(candidate)==original and candidate.stat().st_size<linear.stat().st_size:
            choice=candidate
except (ValueError,KeyError,TypeError,subprocess.TimeoutExpired,OSError): pass
if choice.stat().st_size>min(67108864,source.stat().st_size+131072): choice=source
sys.stdout.buffer.write(choice.read_bytes())
'''


def container_options(image, name):
    return ['create', '--pull', 'never', '-i', '--name', name,
        '--label', 'prims.pdf-preview=true', '--network', 'none',
        '--user', '10001:10001', '--read-only', '--cap-drop', 'ALL',
        '--security-opt', 'no-new-privileges:true', '--memory', '768m',
        '--memory-swap', '768m', '--cpus', '0.35', '--cpu-shares', '64',
        '--pids-limit', '32', '--ulimit', 'nofile=128:128',
        '--ulimit', 'fsize=67108864:67108864',
        '--tmpfs', '/workspace:rw,nosuid,nodev,noexec,size=320m,mode=0700,uid=10001,gid=10001',
        '--tmpfs', '/tmp:rw,nosuid,nodev,noexec,size=32m,mode=0700,uid=10001,gid=10001',
        '--entrypoint', '/usr/bin/nice', image, '-n', '19', '/usr/bin/python3', '-I', '-c', WORKER]


def optimize(raw, docker_command, image, compile_busy):
    """Independent capacity; a foreground compile cancels this work, never waits on it."""
    if not raw.startswith(b'%PDF-') or len(raw)>MAX_PDF:
        return 400, b''
    if compile_busy() or not SLOT.acquire(blocking=False):
        return 429, b''
    name='prims-pdf-preview-'+uuid.uuid4().hex
    proc=None
    try:
        created=subprocess.run(docker_command+container_options(image,name),stdin=subprocess.DEVNULL,
            capture_output=True,timeout=20,check=False)
        if created.returncode: return 503,b''
        proc=subprocess.Popen(docker_command+['start','-a','-i',name],stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
        started=time.monotonic()
        # A single communicate call must finish writing stdin. Repeated timeout calls
        # can abandon an incomplete multi-MiB stdin write and deadlock docker attach.
        done=threading.Event()
        result={}
        def transfer():
            try: result['output']=proc.communicate(input=raw)[0]
            except (OSError,ValueError): result['output']=b''
            finally: done.set()
        thread=threading.Thread(target=transfer,daemon=True)
        thread.start()
        while not done.wait(0.25):
            if compile_busy() or time.monotonic()-started>100:
                return 429,b''
        if compile_busy(): return 429,b''
        output=result.get('output',b'')
        if proc.returncode or len(output)>MAX_PDF or not output.startswith(b'%PDF-'):
            return 422,b''
        return 200,output
    except (OSError,subprocess.TimeoutExpired):
        return 503,b''
    finally:
        try:
            subprocess.run(docker_command+['rm','-f',name],stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10,check=False)
        except (OSError,subprocess.TimeoutExpired): pass
        finally:
            if proc is not None and proc.poll() is None: proc.kill()
            SLOT.release()

"""Fixed offline, resumable bookshelf image batches. No request-controlled commands."""
import subprocess
import threading
import time
import uuid
import pdf_optimizer

MAX_BATCH = 8
MAX_PAGES = 2048
MAX_ARCHIVE = 41 * 1024 * 1024
WORKER = r'''import json,pathlib,struct,subprocess,sys,zipfile
root=pathlib.Path('/workspace');source=root/'source.pdf'
raw=sys.stdin.buffer.read(67108865)
if len(raw)>67108864 or not raw.startswith(b'%PDF-'):sys.exit(2)
source.write_bytes(raw);del raw
start=int(sys.argv[1]);count=int(sys.argv[2])
if not 1<=start<=2048 or not 1<=count<=8:sys.exit(2)
def run(args,timeout=70):
 result=subprocess.run(args,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=timeout)
 if result.returncode not in (0,3):raise ValueError('Image operation failed')
 return result.stdout
try:
 total=int(run(['/usr/bin/qpdf','--show-npages',str(source)],10).strip())
 if not 1<=total<=2048 or start>total:raise ValueError('Invalid page range')
 last=min(total,start+count-1)
 run(['/usr/bin/gs','-q','-dSAFER','-dBATCH','-dNOPAUSE','-sDEVICE=png16m','-dUseCropBox','-r144','-dMaxBitmap=67108864','-dFirstPage='+str(start),'-dLastPage='+str(last),'-sOutputFile=/workspace/page-%06d.png',str(source)])
 pngs=sorted(root.glob('page-*.png'))
 if len(pngs)!=last-start+1:raise ValueError('Incomplete raster batch')
 pages=[];size=0
 for number,png in zip(range(start,last+1),pngs):
  out=root/('page-%06d.webp'%number);thumb=root/('page-%06d-thumb.webp'%number)
  run(['/usr/bin/convert','-limit','memory','128MiB','-limit','map','0','-limit','disk','0','PNG:'+str(png),'-resize','1200x2400>','-strip','-quality','82','-define','webp:method=3','WEBP:'+str(out)],20)
  dims=run(['/usr/bin/identify','-format','%w %h','WEBP:'+str(out)],5).decode().split();width,height=map(int,dims)
  length=out.stat().st_size;size+=length
  if not 0<width<=1200 or not 0<height<=2400 or not 0<length<=4194304 or size>33554432:raise ValueError('Preview image exceeds limit')
  run(['/usr/bin/convert','-limit','memory','128MiB','-limit','map','0','-limit','disk','0','WEBP:'+str(out),'-resize','50%','-strip','-quality','75','-define','webp:method=3','WEBP:'+str(thumb)],20)
  tw,th=map(int,run(['/usr/bin/identify','-format','%w %h','WEBP:'+str(thumb)],5).decode().split());ts=thumb.stat().st_size
  if not 0<tw<=600 or not 0<th<=1200 or abs(tw*2-width)>1 or abs(th*2-height)>1 or not 0<ts<=1048576:raise ValueError('Invalid thumbnail')
  pages.append({'page':number,'width':width,'height':height,'entry':out.name,'size':length,'thumbEntry':thumb.name,'thumbWidth':tw,'thumbHeight':th,'thumbSize':ts});png.unlink()
 manifest={'version':2,'totalPages':total,'startPage':start,'pages':pages}
 with zipfile.ZipFile(sys.stdout.buffer,'w',compression=zipfile.ZIP_STORED) as archive:
  archive.writestr('manifest.json',json.dumps(manifest,separators=(',',':')))
  for page in pages:
   archive.write(root/page['entry'],page['entry']);archive.write(root/page['thumbEntry'],page['thumbEntry'])
except subprocess.TimeoutExpired:sys.exit(124)
except (ValueError,KeyError,TypeError,OSError):sys.exit(2)
'''

def render(raw, docker_command, image, compile_busy, start, count):
    if type(start) is not int or type(count) is not int or not 1 <= start <= MAX_PAGES or not 1 <= count <= MAX_BATCH:
        return 400,b''
    if not raw.startswith(b'%PDF-') or len(raw)>pdf_optimizer.MAX_PDF:return 400,b''
    if compile_busy() or not pdf_optimizer.SLOT.acquire(blocking=False):return 429,b''
    name='prims-webp-preview-'+uuid.uuid4().hex;proc=None
    try:
        options=pdf_optimizer.container_options(image,name)
        options[-1]=WORKER;options.extend([str(start),str(count)])
        created=subprocess.run(docker_command+options,stdin=subprocess.DEVNULL,capture_output=True,timeout=20,check=False)
        if created.returncode:return 503,b''
        proc=subprocess.Popen(docker_command+['start','-a','-i',name],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
        done=threading.Event();result={}
        def transfer():
            try:result['output']=proc.communicate(input=raw)[0]
            except (OSError,ValueError):result['output']=b''
            finally:done.set()
        threading.Thread(target=transfer,daemon=True).start();started=time.monotonic()
        while not done.wait(.25):
            if compile_busy():return 429,b''
            if time.monotonic()-started>100:return 408,b''
        if compile_busy():return 429,b''
        if proc.returncode==124:return 408,b''
        output=result.get('output',b'')
        if proc.returncode or len(output)>MAX_ARCHIVE or not output.startswith(b'PK\x03\x04'):return 422,b''
        return 200,output
    except (OSError,subprocess.TimeoutExpired):return 503,b''
    finally:
        try:subprocess.run(docker_command+['rm','-f',name],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10,check=False)
        except (OSError,subprocess.TimeoutExpired):pass
        finally:
            if proc is not None and proc.poll() is None:proc.kill()
            pdf_optimizer.SLOT.release()

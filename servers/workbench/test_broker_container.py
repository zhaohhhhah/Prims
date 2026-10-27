#!/usr/bin/env python3
"""Ephemeral trusted broker container; 2 x exact32MiB synthetic snapshots."""
import base64
import concurrent.futures
import http.client
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import threading
import time
import uuid

DOCKER=['/usr/bin/docker','-H',os.environ.get('DOCKER_HOST','unix:///var/run/docker.sock')]
WORKER=os.environ['PRIMS_WORKBENCH_IMAGE']
BROKER=os.environ['PRIMS_WORKBENCH_BROKER_IMAGE']
CACHE=os.environ['PRIMS_WORKBENCH_TEX_CACHE']
SOCKET=DOCKER[-1].removeprefix('unix://')
name='prims-workbench-broker-test-'+uuid.uuid4().hex
secret=secrets.token_hex(32)
fd,token_path=tempfile.mkstemp(prefix='workbench-test-token-')
os.fchmod(fd,0o600)
with os.fdopen(fd,'w') as f:f.write(secret)
peak=0
stop=threading.Event()
def cmd(args,**kw):return subprocess.run(DOCKER+args,capture_output=True,check=True,**kw)
def request(value=None,method='POST',path='/run'):
    c=http.client.HTTPConnection('127.0.0.1',19340,timeout=100)
    data=json.dumps(value).encode() if value is not None else None
    c.request(method,path,body=data,headers={'Authorization':'Bearer '+secret,'Content-Type':'application/json'})
    r=c.getresponse();result=json.loads(r.read());status=r.status;c.close();return status,result
try:
    cmd(['run','-d','--name',name,'--network','host','--read-only','--user',f'{os.getuid()}:{os.getgid()}',
         '--group-add',str(os.stat(SOCKET).st_gid),'--cap-drop','ALL','--security-opt','no-new-privileges:true',
         '--memory','512m','--memory-swap','512m','--cpus','1','--pids-limit','64',
         '--tmpfs','/tmp:rw,nosuid,nodev,noexec,size=16m,mode=1777',
         '--mount',f'type=bind,source={SOCKET},target=/var/run/docker.sock',
         '--mount',f'type=bind,source={token_path},target=/run/secrets/workbench-token,readonly',
         '-e','PRIMS_WORKBENCH_TOKEN_FILE=/run/secrets/workbench-token','-e','PRIMS_WORKBENCH_IMAGE='+WORKER,
         '-e','PRIMS_WORKBENCH_TEX_CACHE='+CACHE,BROKER])
    status=0
    for _ in range(30):
        try:
            status,health=request(method='GET',path='/health')
            if status==200:break
        except OSError:pass
        time.sleep(.1)
    if status!=200:
        log=cmd(['logs',name]);diagnostics=(log.stdout+log.stderr).decode()
        raise AssertionError('Broker did not start: '+diagnostics)
    encoded=base64.b64encode(b'X'*(8*1024*1024)).decode()
    def large():
        return {'jobId':str(uuid.uuid4()),'task':'font_check','files':[{'path':f'assets/fixture{i}.bin','contentBase64':encoded}for i in range(4)]}
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as e:
        calls=[e.submit(request,large()) for _ in range(2)]
        results=[f.result(timeout=90) for f in calls]
    assert all(status==200 and result['ok'] for status,result in results),results
    report={'brokerImage':BROKER,'workerImage':WORKER,'twoExact32MiBSnapshots':[{'status':s,'ok':r['ok'],'output':r['output'],'durationMs':r['durationMs']}for s,r in results]}
    # Oversized Content-Length rejected without reading/decoding a body.
    c=http.client.HTTPConnection('127.0.0.1',19340,timeout=5)
    c.putrequest('POST','/run');c.putheader('Authorization','Bearer '+secret);c.putheader('Content-Length',str(47*1024*1024));c.endheaders()
    r=c.getresponse();assert r.status==413;report['oversizeHeaderRejected']=r.status;r.read();c.close()
    # Validate a real project-local font, copied only from this public worker image.
    font=cmd(['run','--rm','--network','none','--entrypoint','cat',WORKER,'/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf']).stdout
    status,result=request({'jobId':str(uuid.uuid4()),'task':'font_check','path':'fonts/Fixture.ttf','files':[{'path':'fonts/Fixture.ttf','contentBase64':base64.b64encode(font).decode()}]})
    assert status==200 and result['ok'] and any('DejaVu Sans' in x for x in result['fonts']);report['projectFont']={'ok':True,'fonts':result['fonts'],'fontScope':result['fontScope']}
    status,result=request({'jobId':str(uuid.uuid4()),'task':'list_fonts','files':[]});assert status==200 and result['ok'];report['brokerRemainsHealthy']=True
    report['ephemeralOnly']=True
    print(json.dumps(report,ensure_ascii=False,indent=2))
finally:
    subprocess.run(DOCKER+['rm','-f',name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    Path(token_path).unlink(missing_ok=True)

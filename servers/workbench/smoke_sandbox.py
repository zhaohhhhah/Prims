#!/usr/bin/env python3
"""Synthetic-only Docker checks; no project/DB/model credentials."""
import base64
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import threading
import time
import uuid

import broker
broker.SECRET=secrets.token_hex(32)

def run(task, content=None, path=None):
    value={'jobId':str(uuid.uuid4()),'task':task,'files':[]}
    if path:
        value['path']=path
        value['files']=[{'path':path,'contentBase64':base64.b64encode(content).decode()}]
    run={'name':'prims-workbench-'+value['jobId']+'-smoke','cancel':threading.Event()}
    assert broker.SLOTS.acquire(False)
    with broker.LOCK:broker.RUNS[value['jobId']]=run
    return broker.execute_job(value,json.dumps(value).encode(),run)

results={}
results['fonts']=run('list_fonts')
results['latex']=run('latex_check',b'\\documentclass{article}\n\\begin{document}\nSynthetic workbench check.\n\\end{document}\n','main.tex')
name='prims-workbench-security-'+uuid.uuid4().hex
options=broker.container_options(name)
# Test-only override; the HTTP API cannot provide an entrypoint/argv.
options=options[:-1]+['--entrypoint','python3',options[-1],'-I','-c',r'''
import json,os,socket
from pathlib import Path
r={'uid':os.getuid(),'noNas':not Path('/host-project-data').exists(),'noSocket':not Path('/var/run/docker.sock').exists(),'workspace':False,'readonly':False,'networkBlocked':False}
try:Path('/workspace/fixture.txt').write_text('synthetic');r['workspace']=True
except OSError:pass
try:Path('/etc/prims-synthetic-probe').write_text('must fail')
except OSError:r['readonly']=True
try:s=socket.create_connection(('1.1.1.1',443),timeout=1);s.close()
except OSError:r['networkBlocked']=True
status=Path('/proc/self/status').read_text()
for key in ('CapEff','NoNewPrivs','Seccomp'):
 r[key]=next(x.split(':',1)[1].strip() for x in status.splitlines() if x.startswith(key+':'))
print(json.dumps(r))
''']
try:
    created=broker.docker(options,12);assert created.returncode==0,created.stderr.decode()
    inspected=broker.docker(['inspect',name,'--format','{{json .HostConfig}}'])
    config=json.loads(inspected.stdout)
    completed=broker.docker(['start','-a',name],10)
    results['isolation']=json.loads(completed.stdout)
    results['limits']={k:config[k] for k in ('ReadonlyRootfs','NetworkMode','Memory','MemorySwap','NanoCpus','PidsLimit','CapDrop','SecurityOpt','Tmpfs')}
finally:broker.remove(name)
assert results['fonts']['ok'],results['fonts']
assert results['isolation']=={'uid':10001,'noNas':True,'noSocket':True,'workspace':True,'readonly':True,'networkBlocked':True,'CapEff':'0000000000000000','NoNewPrivs':'1','Seccomp':'2'},results['isolation']
print(json.dumps(results,ensure_ascii=False,indent=2))

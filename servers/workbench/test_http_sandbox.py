#!/usr/bin/env python3
"""Ephemeral broker + synthetic-only real container cancellation/timeout test."""
import base64
import concurrent.futures
import http.client
import json
import os
import secrets
import threading
import time
import uuid
from http.server import ThreadingHTTPServer
import broker
broker.SECRET=secrets.token_hex(32)
server=ThreadingHTTPServer(('127.0.0.1',19340),broker.Handler)
server.daemon_threads=True
threading.Thread(target=server.serve_forever,daemon=True).start()

def request(method,path,value=None,auth=True):
    connection=http.client.HTTPConnection('127.0.0.1',19340,timeout=100)
    headers={'Content-Type':'application/json'}
    if auth:headers['Authorization']='Bearer '+broker.SECRET
    connection.request(method,path,body=json.dumps(value).encode() if value is not None else None,headers=headers)
    response=connection.getresponse();result=json.loads(response.read());connection.close()
    return response.status,result

def loopjob():
    return {'jobId':str(uuid.uuid4()),'task':'latex_check','path':'main.tex','files':[{'path':'main.tex','contentBase64':base64.b64encode(b'\\documentclass{article}\n\\begin{document}\n\\loop\\iftrue\\repeat\n\\end{document}').decode()}]}

report={}
try:
    status,_=request('GET','/health',auth=False);assert status==401;report['unauthorized']=status
    status,health=request('GET','/health');assert status==200 and health['capabilities'];report['health']=health
    status,_=request('POST','/run',{'jobId':str(uuid.uuid4()),'task':'shell','files':[]});assert status==400;report['invalidTask']=status
    value={'jobId':str(uuid.uuid4()),'task':'latex_check','path':'../escape.tex','files':[{'path':'../escape.tex','contentBase64':''}]}
    status,_=request('POST','/run',value);assert status==400;report['pathEscapeRejected']=status
    first,second=loopjob(),loopjob()
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        a=executor.submit(request,'POST','/run',first)
        single_deadline=time.monotonic()+8
        while len(broker.RUNS)!=1 and time.monotonic()<single_deadline:time.sleep(.05)
        status,_=request('POST','/run',first);assert status==409;report['duplicate']=status
        b=executor.submit(request,'POST','/run',second)
        deadline=time.monotonic()+8
        while len(broker.RUNS)!=2 and time.monotonic()<deadline:time.sleep(.05)
        assert len(broker.RUNS)==2
        status,_=request('POST','/run',{'jobId':str(uuid.uuid4()),'task':'list_fonts','files':[]});assert status==429;report['busy']=status
        # Wait for real containers to be created before cancelling one; second exercises45s hard timeout.
        time.sleep(1)
        status,_=request('DELETE','/runs/'+first['jobId']);assert status==202
        status,result=a.result(timeout=10);assert status==200 and result['cancelled'];report['cancel']=result
        if os.environ.get('WORKBENCH_TEST_QUICK')=='1':
            status,_=request('DELETE','/runs/'+second['jobId']);assert status==202
            status,result=b.result(timeout=10);assert status==200 and result['cancelled'];report['secondCancel']=result
        else:
            status,result=b.result(timeout=55);assert status==200 and result['timedOut'];report['hardTimeout']=result
    assert not broker.RUNS
    remaining=broker.docker(['ps','-aq','--filter','label=prims.workbench=true']).stdout.decode().split()
    assert not remaining,remaining
    report['allSyntheticContainersRemoved']=True
finally:
    with broker.LOCK:
        for run in list(broker.RUNS.values()):run['cancel'].set();broker.remove(run['name'])
    server.shutdown();server.server_close()
print(json.dumps(report,indent=2))

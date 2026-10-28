import importlib.util,io,sys,subprocess,json,os,threading,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parent));os.environ['PRIMS_WORKBENCH_PORT']='19342'
import broker as b
class BoundedSource(io.BytesIO):
 def read(self,size=-1):
  if size<0 or size>65536:raise AssertionError('unbounded source read')
  return super().read(size)
class BrokerStreamTests(unittest.TestCase):
 def setUp(self):
  self.old=(b.docker,b.remove,b.subprocess.Popen)
  self.real_popen=subprocess.Popen
  b.SECRET='mock-secret-with-no-real-credentials'
  b.docker=lambda *a,**k: subprocess.CompletedProcess(a,0,b'id',b'')
  b.remove=lambda *a:None
 def tearDown(self):b.docker,b.remove,b.subprocess.Popen=self.old
 def invoke(self,source,size):
  def fake_popen(*args,**kwargs):
   return self.real_popen([sys.executable,'-c',"import sys,json; n=0\nwhile True:\n c=sys.stdin.buffer.read(65536)\n if not c:break\n n+=len(c)\nprint(json.dumps({'ok':True,'exitCode':0,'output':str(n)}))"],**kwargs)
  b.subprocess.Popen=fake_popen
  job={'jobId':'11111111-1111-4111-8111-111111111111','task':'latex_compile','files':[{'path':'main.tex','size':size}]}
  run={'name':'mock-only','cancel':threading.Event()};b.SLOTS.acquire()
  return b.execute_job(job,b'header',run,source)
 def test_bounded_transfer(self):
  r=self.invoke(BoundedSource(b'x'*(2*1024*1024)),2*1024*1024)
  self.assertTrue(r['ok']);self.assertEqual(r['output'],str(2*1024*1024+6))
 def test_truncated_transfer_fails(self):
  self.assertFalse(self.invoke(BoundedSource(b'x'),2)['ok'])
 def test_cancel_during_create_timeout_remains_cancelled(self):
  run={'name':'mock-only','cancel':threading.Event()}
  def timeout(*args,**kwargs):
   run['cancel'].set();raise subprocess.TimeoutExpired('mock',60)
  b.docker=timeout;b.SLOTS.acquire()
  result=b.execute_job({'jobId':'11111111-1111-4111-8111-111111111111','task':'latex_compile'},b'',run)
  self.assertTrue(result['cancelled']);self.assertEqual(result['exitCode'],130)
 def test_sandbox_budget_and_boundary(self):
  b.IMAGE='sha256:'+'a'*64;b.CACHE='/fixed/cache'
  options=b.container_options('mock',True)
  self.assertIn('2g',options);self.assertIn('none',options);self.assertIn('ALL',options)
  self.assertTrue(any('/workspace:' in x and '768m' in x and 'noexec' in x for x in options))
  self.assertFalse(any('docker.sock' in x for x in options));self.assertEqual(b.SLOT_COUNT,1)
if __name__=='__main__':unittest.main()

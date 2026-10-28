import unittest,sys
from pathlib import Path
from unittest.mock import Mock,patch
sys.path.insert(0,str(Path(__file__).parent));import webp_renderer as webp
class RendererTests(unittest.TestCase):
 def test_invalid_range_and_pdf_fail_before_docker(self):
  with patch.object(webp.subprocess,'run') as run:
   for start,count in [(0,1),(1,0),(1,9),(2049,1),(True,1)]:self.assertEqual(webp.render(b'%PDF-x',[],'',lambda:False,start,count)[0],400)
   self.assertEqual(webp.render(b'bad',[],'',lambda:False,1,1)[0],400);run.assert_not_called()
 def test_shared_capacity_and_native_preemption(self):
  webp.pdf_optimizer.SLOT.acquire()
  try:self.assertEqual(webp.render(b'%PDF-x',[],'',lambda:False,1,1)[0],429)
  finally:webp.pdf_optimizer.SLOT.release()
  self.assertEqual(webp.render(b'%PDF-x',[],'',lambda:True,1,1)[0],429)
 def test_uses_fixed_offline_worker_and_same_isolation(self):
  self.assertNotIn('--linearize',webp.WORKER);self.assertIn('png16m',webp.WORKER);self.assertIn("'-quality','82'",webp.WORKER)
  p=Mock(returncode=0);p.communicate.return_value=(b'PK\x03\x04zip',None);p.poll.return_value=0
  with patch.object(webp.subprocess,'run',return_value=Mock(returncode=0)) as run,patch.object(webp.subprocess,'Popen',return_value=p):
   self.assertEqual(webp.render(b'%PDF-x',['docker'],'fixed',lambda:False,9,8)[0],200)
   args=run.call_args_list[0].args[0];self.assertIn('none',args);self.assertIn('768m',args);self.assertIn('19',args);self.assertIn('--read-only',args);self.assertEqual(args[-2:],['9','8']);self.assertNotIn('--mount',args)
 def test_native_arrival_cancels_and_releases_capacity(self):
  p=Mock(returncode=0);p.communicate.return_value=(b'PK\x03\x04zip',None);p.poll.return_value=None
  with patch.object(webp.subprocess,'run',return_value=Mock(returncode=0)),patch.object(webp.subprocess,'Popen',return_value=p):self.assertEqual(webp.render(b'%PDF-x',[],'fixed',Mock(side_effect=[False,True]),1,8)[0],429)
  p.kill.assert_called_once();self.assertTrue(webp.pdf_optimizer.SLOT.acquire(False));webp.pdf_optimizer.SLOT.release()
 def test_timeout_is_adaptive_not_fake_ready(self):
  p=Mock(returncode=124);p.communicate.return_value=(b'',None);p.poll.return_value=124
  with patch.object(webp.subprocess,'run',return_value=Mock(returncode=0)),patch.object(webp.subprocess,'Popen',return_value=p):self.assertEqual(webp.render(b'%PDF-x',[],'fixed',lambda:False,1,8)[0],408)
if __name__=='__main__':unittest.main()

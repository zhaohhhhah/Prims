import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch, Mock

spec=importlib.util.spec_from_file_location('pdf_optimizer',Path(__file__).with_name('pdf_optimizer.py'))
pdf=importlib.util.module_from_spec(spec);spec.loader.exec_module(pdf)

class OptimizerTests(unittest.TestCase):
    def test_worker_is_non_root_readonly_networkless_low_priority(self):
        args=pdf.container_options('sha256:fixed','fixed-name')
        for value in ['none','10001:10001','--read-only','ALL','no-new-privileges:true','1','64','/usr/bin/nice','19']:
            self.assertIn(value,args)
        self.assertEqual(args[args.index('--cpus')+1],'1')
        self.assertEqual(args[args.index('--cpu-shares')+1],'64')
        self.assertEqual(args[args.index('--memory')+1],'768m')
        self.assertEqual(args[args.index('--memory-swap')+1],'768m')
        self.assertNotIn('--mount',args) # PDF gets stdin, no host paths/credential mounts.
        self.assertIn('--linearize',pdf.WORKER)
        self.assertIn('geometry(candidate)==original',pdf.WORKER)
    def test_bad_pdf_and_busy_never_start_docker(self):
        with patch.object(pdf.subprocess,'run') as run:
            self.assertEqual(pdf.optimize(b'bad',[], '',lambda:False)[0],400)
            self.assertEqual(pdf.optimize(b'%PDF-x',[], '',lambda:True)[0],429)
            run.assert_not_called()
    def test_independent_single_slot_never_queues(self):
        pdf.SLOT.acquire()
        try:
            with patch.object(pdf.subprocess,'run') as run:
                self.assertEqual(pdf.optimize(b'%PDF-x',[], '',lambda:False)[0],429)
                run.assert_not_called()
        finally: pdf.SLOT.release()
    def test_compile_preempts_optimizer_and_cleanup_releases_slot(self):
        proc=Mock(returncode=0);proc.communicate.return_value=(b'%PDF-ok',None);proc.poll.return_value=None
        with patch.object(pdf.subprocess,'run',return_value=Mock(returncode=0)),patch.object(pdf.subprocess,'Popen',return_value=proc):
            self.assertEqual(pdf.optimize(b'%PDF-x',['docker'],'fixed',Mock(side_effect=[False,True]))[0],429)
            proc.kill.assert_called_once()
        self.assertTrue(pdf.SLOT.acquire(blocking=False));pdf.SLOT.release()
    def test_invalid_result_rejected(self):
        proc=Mock(returncode=0);proc.communicate.return_value=(b'not PDF',None);proc.poll.return_value=0
        with patch.object(pdf.subprocess,'run',return_value=Mock(returncode=0)),patch.object(pdf.subprocess,'Popen',return_value=proc):
            self.assertEqual(pdf.optimize(b'%PDF-x',['docker'],'fixed',lambda:False)[0],422)
    def test_cleanup_failure_cannot_leak_capacity(self):
        with patch.object(pdf.subprocess,'run',side_effect=[OSError(),OSError()]):
            self.assertEqual(pdf.optimize(b'%PDF-x',['docker'],'fixed',lambda:False)[0],503)
        self.assertTrue(pdf.SLOT.acquire(blocking=False));pdf.SLOT.release()

if __name__=='__main__':unittest.main()

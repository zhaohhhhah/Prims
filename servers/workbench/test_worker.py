import base64
import tempfile
import sys
import time
from pathlib import Path
from unittest.mock import patch
import worker
import unittest
import uuid
from worker import safe_path, validate_job, clean_output

def job(path='main.tex', content=b'example'):
    return {'jobId':str(uuid.uuid4()),'task':'latex_check','path':path,
            'files':[{'path':path,'contentBase64':base64.b64encode(content).decode()}]}

class WorkerValidation(unittest.TestCase):
    def test_rejects_escape_control_drive_and_cli_paths(self):
        for value in ('/tmp/a','../a','x/../y','x\\y','C:/x','x\ny','a//b','a/./b','-x.tex','.output/a'):
            with self.subTest(value=value), self.assertRaises(ValueError): safe_path(value)
    def test_known_safe_relative_paths(self):
        self.assertEqual(safe_path('章节/一 main.tex'), '章节/一 main.tex')
        self.assertEqual(validate_job(job())[0][0], 'main.tex')
    def test_rejects_duplicate_prefix_and_symlink_fields(self):
        for item in ({'path':'main.tex','contentBase64':''},{'path':'main.tex/a','contentBase64':''},
                     {'path':'a','contentBase64':'','symlink':'/etc/passwd'}):
            value=job();value['files'].append(item)
            with self.assertRaises(ValueError):validate_job(value)
    def test_rejects_unsupported_commands_and_absent_target(self):
        value=job();value['task']='shell'
        with self.assertRaises(ValueError):validate_job(value)
        value=job();value['command']='id'
        with self.assertRaises(ValueError):validate_job(value)
        value=job();value['path']='missing.tex'
        with self.assertRaises(ValueError):validate_job(value)
    def test_font_scope_and_list_contract(self):
        value=job('font.ttf');value['task']='font_check';validate_job(value)
        value['task']='list_fonts'
        with self.assertRaises(ValueError):validate_job(value)
        validate_job({'jobId':str(uuid.uuid4()),'task':'list_fonts','files':[]})
    def test_invalid_base64_and_file_count(self):
        value=job();value['files'][0]['contentBase64']='***'
        with self.assertRaises(ValueError):validate_job(value)
        value=job();value['files']*=257
        with self.assertRaises(ValueError):validate_job(value)
    def test_aggregate_size_is_rejected_before_any_decoding(self):
        value=job();value['files']=[{'path':f'{i}.bin','contentBase64':'A'*(4*((8*1024*1024+2)//3))} for i in range(5)]
        with patch('worker.base64.b64decode',side_effect=AssertionError('must not decode')):
            with self.assertRaises(ValueError):validate_job(value)
    def test_subprocess_output_is_bounded_and_drained(self):
        with tempfile.TemporaryDirectory() as directory,patch('worker.ROOT',Path(directory)):
            code,output,truncated,timed_out=worker.run_command([sys.executable,'-c',"print('x'*200000)"],time.monotonic()+5,{'PATH':'/usr/bin:/bin'})
        self.assertEqual(code,0);self.assertLessEqual(len(output.encode()),65536);self.assertTrue(truncated);self.assertFalse(timed_out)
    def test_terminal_output_sanitized(self):
        self.assertEqual(clean_output('\x1b[31mERROR\x1b[0m\x00\nline'), 'ERROR\nline')
class CompileValidation(unittest.TestCase):
    def test_compile_uses_only_fixed_engines(self):
        for engine in ('pdflatex','xelatex','lualatex'):
            value=job();value.update(task='latex_compile',engine=engine);self.assertEqual(validate_job(value)[0][0],'main.tex')
        for engine in ('tectonic','sh','xelatex -shell-escape',''):
            value=job();value.update(task='latex_compile',engine=engine)
            with self.assertRaises(ValueError):validate_job(value)
    def test_compile_rejects_mounts_env_and_commands(self):
        for key,value in [('command','id'),('env',{'SECRET':'x'}),('mount','/etc')]:
            request=job();request.update(task='latex_compile',engine='xelatex');request[key]=value
            with self.assertRaises(ValueError):validate_job(request)
    def test_diagnostics_do_not_gain_engine_selection(self):
        value=job();value['engine']='lualatex'
        with self.assertRaises(ValueError):validate_job(value)
if __name__=='__main__':unittest.main()

class BibliographyErrorTests(unittest.TestCase):
    def test_helper_errors_are_not_silently_treated_as_success(self):
        for log in ["(There were 2 error messages)", "[2026] ERROR - Cannot find refs.bib", "INFO - ERRORS: 1"]:
            self.assertTrue(worker.bibliography_failed(log))
        for log in ["Warning--I didn't find a database entry for ref1", "INFO - ERRORS: 0", "INFO - WARNINGS: 1"]:
            self.assertFalse(worker.bibliography_failed(log))

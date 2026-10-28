import importlib.util,io,json,struct,tempfile,unittest,os
from pathlib import Path
spec=importlib.util.spec_from_file_location('worker',Path(__file__).with_name('worker.py'));w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w)
def frame(files=None,**changes):
 job={'jobId':'11111111-1111-4111-8111-111111111111','task':'latex_compile','engine':'xelatex','path':'中文/main.tex','files':files or [{'path':'中文/main.tex','size':3}]};job.update(changes);m=json.dumps(job).encode();return w.STREAM_MAGIC+struct.pack('>I',len(m))+m
class StreamTests(unittest.TestCase):
 def test_header_and_materialize(self):
  data=frame()+b'abc'; stream=io.BytesIO(data);job,prefix,total=w.read_stream_header(stream,len(data));self.assertEqual(total,3)
  with tempfile.TemporaryDirectory() as directory:
   old=w.ROOT;w.ROOT=Path(directory)
   try:w.materialize_stream(stream,job);self.assertEqual((w.ROOT/'中文/main.tex').read_bytes(),b'abc')
   finally:w.ROOT=old
 def test_fragmented_header_and_body(self):
  class Tiny(io.BytesIO):
   def read(self,size=-1): return super().read(min(size,1))
  stream=Tiny(frame()+b'abc');job,_,_=w.read_stream_header(stream)
  with tempfile.TemporaryDirectory() as directory:
   old=w.ROOT;w.ROOT=Path(directory)
   try:w.materialize_stream(stream,job);self.assertEqual((w.ROOT/'中文/main.tex').read_bytes(),b'abc')
   finally:w.ROOT=old
 def test_fonts_larger_than_old_limit(self):
  data=frame([{'path':'中文/main.tex','size':3},{'path':'font.ttc','size':22*1024*1024}]);self.assertEqual(w.read_stream_header(io.BytesIO(data))[2],22*1024*1024+3)
 def test_total_boundary(self):
  data=frame([{'path':'中文/main.tex','size':w.STREAM_MAX_TOTAL}]);self.assertEqual(w.read_stream_header(io.BytesIO(data))[2],w.STREAM_MAX_TOTAL)
  with self.assertRaises(ValueError):w.read_stream_header(io.BytesIO(frame([{'path':'中文/main.tex','size':w.STREAM_MAX_TOTAL+1}])))
 def test_bad_paths(self):
  for path in ['../a','/etc/passwd','a\\b','a/../b','-x.tex']:
   with self.assertRaises(ValueError):w.read_stream_header(io.BytesIO(frame([{'path':path,'size':1}],path=path)))
 def test_links_and_extra_fields(self):
  with self.assertRaises(ValueError):w.read_stream_header(io.BytesIO(frame([{'path':'中文/main.tex','size':1,'symlink':'/etc/passwd'}])))
 def test_truncated_header(self):
  with self.assertRaises(ValueError):w.read_stream_header(io.BytesIO(frame()[:-1]))
 def test_wrong_content_length(self):
  with self.assertRaises(ValueError):w.read_stream_header(io.BytesIO(frame()),100)
 def test_truncated_body(self):
  stream=io.BytesIO(frame()+b'a');job,_,_=w.read_stream_header(stream)
  with tempfile.TemporaryDirectory() as directory:
   old=w.ROOT;w.ROOT=Path(directory)
   try:
    with self.assertRaises(ValueError):w.materialize_stream(stream,job)
   finally:w.ROOT=old
 def test_collision(self):
  with self.assertRaises(ValueError):w.read_stream_header(io.BytesIO(frame([{'path':'a','size':0},{'path':'a/main.tex','size':0}],path='a/main.tex')))
 def test_legacy_compat(self):
  self.assertEqual(w.validate_job({'jobId':'11111111-1111-4111-8111-111111111111','task':'latex_compile','engine':'xelatex','path':'main.tex','files':[{'path':'main.tex','contentBase64':'YWJj'}]}),[('main.tex',b'abc')])
if __name__=='__main__':unittest.main()

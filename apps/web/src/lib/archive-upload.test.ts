import {describe,it,expect} from 'vitest';
import {uploadPath,preflightUploadPaths} from './archive-upload';
describe('archive and multi-file upload preflight',()=>{
 it('preserves safe relative directories without inventing timestamps',()=>{expect(preflightUploadPaths(['paper/main.tex','figures/a.png'],['notes.md'])).toEqual(['paper/main.tex','figures/a.png']);expect(uploadPath('./paper/main.tex')).toBe('paper/main.tex');});
 it('rejects ambiguous/unsafe paths and existing or normalized duplicate names',()=>{for(const path of ['/a','../a','a/../b','C:/a','a\\b','.git/config','a//b'])expect(()=>uploadPath(path)).toThrow();expect(()=>preflightUploadPaths(['main.tex'],['main.tex'])).toThrow('已存在');expect(()=>preflightUploadPaths(['main.tex','./main.tex'],[])).toThrow('重名');expect(()=>preflightUploadPaths(['folder','folder/a.tex'],[])).toThrow('冲突');expect(()=>preflightUploadPaths(['folder'],[],['folder'])).toThrow('冲突');});
});

import {vi} from 'vitest';
import {uploadBatch,type BatchTransport} from './archive-upload';
function transport():BatchTransport{return{listFiles:vi.fn(async()=>[]),inspectZip:vi.fn(async()=>({files:[{path:'paper/main.tex'}],directories:[],totalBytes:100})),uploadZip:vi.fn(async()=>({created:[{}],complete:true,failed:null})),uploadFile:vi.fn(async()=>({}))};}
describe('multiple-file transport boundaries',()=>{
 it('checks archive paths and existing paths before any upload',async()=>{const io=transport();io.listFiles=vi.fn(async()=>[{path:'paper/main.tex'}]);await expect(uploadBatch([new File(['zip'],'paper.zip'),new File(['x'],'other.tex')],[],io)).rejects.toThrow('已存在');expect(io.uploadZip).not.toHaveBeenCalled();expect(io.uploadFile).not.toHaveBeenCalled();});
 it('keeps earlier new uploads on a later failure and never deletes or overwrites anything',async()=>{const io=transport();let count=0;io.uploadFile=vi.fn(async()=>{if(++count===2)throw new Error('storage failure');return{};});const result=await uploadBatch([new File(['a'],'a.tex'),new File(['b'],'b.tex'),new File(['c'],'c.tex')],[],io);expect(result).toEqual({uploaded:1,failed:'storage failure'});expect(io.uploadFile).toHaveBeenCalledTimes(2);});
 it('handles multiple files and archives without browser decompression',async()=>{const io=transport();expect(await uploadBatch([new File(['a'],'a.tex'),new File(['zip'],'paper.zip')],[],io)).toEqual({uploaded:2,failed:null});expect(io.inspectZip).toHaveBeenCalledTimes(1);expect(io.uploadZip).toHaveBeenCalledTimes(1);});
});

 it('preflights empty directories across every ZIP before the first write',async()=>{const io=transport();io.inspectZip=vi.fn(async(file:File)=>file.name==='a.zip'?{files:[],directories:['foo'],totalBytes:0}:{files:[{path:'foo'}],directories:[],totalBytes:1});await expect(uploadBatch([new File(['zip'],'a.zip'),new File(['zip'],'b.zip')],[],io)).rejects.toThrow('冲突');expect(io.uploadZip).not.toHaveBeenCalled();expect(()=>preflightUploadPaths([],['foo'],[],['foo/bar'])).toThrow('冲突');expect(preflightUploadPaths(['foo/a.tex'],[],['foo'],['foo'])).toEqual(['foo/a.tex']);});

import { FILE_MAX_BYTES, ZIP_MAX_BYTES, PROJECT_MAX_BYTES } from './archive-upload';
it('uses exactly 500MiB for source and attachment limits and rejects remaining-capacity overflow before upload', async () => {
 expect(FILE_MAX_BYTES).toBe(524288000); expect(ZIP_MAX_BYTES).toBe(PROJECT_MAX_BYTES);
 const io=transport();io.listFiles=vi.fn(async()=>[{path:'existing.bin',sizeBytes:PROJECT_MAX_BYTES-1}]);
 await expect(uploadBatch([new File(['xx'],'new.bin')],[],io)).rejects.toThrow('剩余');expect(io.uploadFile).not.toHaveBeenCalled();
});
it('accepts a file larger than the former 8MiB boundary without reading it in preflight', async()=>{
 const io=transport(); const file=new File(['x'],'large.bin');Object.defineProperty(file,'size',{value:9*1024*1024});
 expect(await uploadBatch([file],[],io)).toEqual({uploaded:1,failed:null});
});

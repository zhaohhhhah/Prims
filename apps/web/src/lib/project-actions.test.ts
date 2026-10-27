import { describe,it,expect } from 'vitest';
import { parseProjectActions } from './project-actions';
describe('project actions are only bounded safe proposals',()=>{
 it('keeps allowed creates, empty text and same-project file moves',()=>{expect(parseProjectActions([{type:'create_directory',path:'sections'},{type:'create_file',path:'sections/intro.tex',content:''},{type:'move_file',path:'old.tex',newPath:'archive/old.tex'}])).toHaveLength(3);});
 it('rejects traversal, absolute paths, unknown writes and duplicate targets',()=>{for(const path of ['../secret','/absolute','a//b','a/../b','C:/x','a\\b','.git/config'])expect(()=>parseProjectActions([{type:'create_file',path,content:'x'}])).toThrow();expect(()=>parseProjectActions([{type:'delete_file',path:'main.tex'}])).toThrow();expect(()=>parseProjectActions([{type:'create_file',path:'a.tex',content:'a'},{type:'create_directory',path:'a.tex'}])).toThrow();});
 it('rejects mutation-shaped extras and caps count/content',()=>{expect(()=>parseProjectActions([{type:'move_file',path:'a.tex',newPath:'b.tex',content:'overwrite'}])).toThrow();expect(()=>parseProjectActions([{type:'create_file',path:'x',content:'a'.repeat(40001)}])).toThrow();expect(()=>parseProjectActions(Array.from({length:13},(_,i)=>({type:'create_directory',path:'d'+i})))).toThrow();});
});

import {describe,expect,it} from 'vitest';
import {assistantRequestTarget} from './assistant-target';
describe('follow-up discussion after a document change',()=>{
 it('keeps a valid selected target for another round without creating a different conversation',()=>{const target={text:'current text',conflict:false,selectionId:'same'};expect(assistantRequestTarget(target,false)).toEqual({target,selection:'current text',readOnly:false});});
 it('permits a stale-anchor follow-up only as discussion and excludes stale selected text',()=>{expect(assistantRequestTarget({text:'outdated text',conflict:true},false)).toEqual({target:null,selection:'',readOnly:true});});
 it('preserves readonly selections for questions while disabling all document writes',()=>{expect(assistantRequestTarget({text:'view only',conflict:false},true)).toMatchObject({selection:'view only',readOnly:true});expect(assistantRequestTarget(null,false)).toEqual({target:null,selection:'',readOnly:true});});
});

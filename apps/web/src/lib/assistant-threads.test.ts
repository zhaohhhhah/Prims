import {describe,expect,it} from 'vitest';
import {sortAssistantThreads} from './assistant-threads';
describe('conversation list ordering',()=>{
 const rows=[{id:'a',createdAt:'2026-10-01',lastMessageAt:'2026-10-02',updatedAt:'2099-01-01'},{id:'b',createdAt:'2026-10-01',lastMessageAt:'2026-10-03',updatedAt:'2026-10-03'},{id:'c',createdAt:'2026-10-04',lastMessageAt:null,updatedAt:'2026-10-04'}];
 it('uses real message time, falls back to creation for an empty chat, and ignores selection metadata',()=>{expect(sortAssistantThreads(rows).map(row=>row.id)).toEqual(['c','b','a']);expect(rows[0]!.id).toBe('a');});
 it('only changes position after an actual new message and has a stable ID tie-break',()=>{expect(sortAssistantThreads(rows.map(row=>row.id==='a'?{...row,lastMessageAt:'2026-10-05'}:row)).map(row=>row.id)).toEqual(['a','c','b']);expect(sortAssistantThreads([{id:'z',updatedAt:'2026-10-01'},{id:'a',updatedAt:'2026-10-01'}]).map(row=>row.id)).toEqual(['a','z']);});
});

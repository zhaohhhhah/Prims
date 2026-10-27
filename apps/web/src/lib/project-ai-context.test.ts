import {describe,expect,it,vi} from 'vitest';
vi.mock('./supabase',()=>({API_URL:'',supabase:{auth:{getSession:vi.fn()}}}));
import {mergeContextMessages,proposalForTarget,type ProjectAIMessage} from './project-ai-context';
describe('shared project assistant state',()=>{
 const saved:ProjectAIMessage[]=[{id:'one',role:'user',content:'We are drafting an introduction',selectionId:'old'},{id:'two',role:'assistant',content:'Keep a clear research question',selectionId:'old'}];
 it('keeps discussion when a new selection begins and deduplicates persisted pending messages',()=>{
  const next:ProjectAIMessage={id:'three',role:'user',content:'Apply this idea here',selectionId:'new'};
  expect(mergeContextMessages(saved,[saved[1]!,next]).map(m=>m.id)).toEqual(['one','two','three']);
 });
 it('does not expose an old proposal after switching selection or file',()=>{
  const proposal={selectionId:'old',filePath:'intro.tex',text:'An earlier draft'};
  expect(proposalForTarget(proposal,'new','intro.tex')).toBeNull();expect(proposalForTarget(proposal,'old','methods.tex')).toBeNull();expect(proposalForTarget(proposal,'old','intro.tex')).toEqual(proposal);expect(proposalForTarget(proposal,undefined,'intro.tex')).toBeNull();
 });
 it('never restores an editable plan from a different conversation',()=>{const proposal={selectionId:'same-selection',filePath:'main.tex',threadId:'old-thread'};expect(proposalForTarget(proposal,'same-selection','main.tex','new-thread')).toBeNull();expect(proposalForTarget(proposal,'same-selection','main.tex','old-thread')).toEqual(proposal);});
});

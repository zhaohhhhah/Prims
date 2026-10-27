import {describe,expect,it} from 'vitest';
import {consumeAssistantDraft,editAssistantDraft,restoreAssistantDraft} from './assistant-drafts';
describe('selection and dock drafts stay independent',()=>{
 it('edits and consumes only the sending surface while both keep shared conversation elsewhere',()=>{const drafts={panel:'Unsent panel text',selection:'Edit this selection'};expect(consumeAssistantDraft(drafts,'selection')).toEqual({panel:'Unsent panel text',selection:''});expect(consumeAssistantDraft(drafts,'panel')).toEqual({panel:'',selection:'Edit this selection'});expect(editAssistantDraft(drafts,'selection','Different')).toEqual({panel:'Unsent panel text',selection:'Different'});});
 it('restores failed input to its source without overwriting newer typing or the other surface',()=>{expect(restoreAssistantDraft({panel:'Keep me',selection:''},'selection','Retry')).toEqual({panel:'Keep me',selection:'Retry'});expect(restoreAssistantDraft({panel:'Keep me',selection:'New idea'},'selection','Old request')).toEqual({panel:'Keep me',selection:'New idea'});});
});

import { EditorState } from '@codemirror/state';
import { describe,expect,it } from 'vitest';
import { mapInlineAnchor } from './inline-assistant';

describe('selection anchor safety',()=>{
  const original={from:6,to:12,text:'target',conflict:false};
  const state=EditorState.create({doc:'beforetargetafter'});
  it('maps unrelated insertions before the selection without conflict',()=>{
    expect(mapInlineAnchor(original,state.changes({from:0,insert:'new '}))).toEqual({...original,from:10,to:16});
  });
  it('preserves edits outside both boundaries',()=>{
    const mapped=mapInlineAnchor(original,state.changes([{from:6,insert:'['},{from:12,insert:']'}]));
    expect(mapped).toEqual({...original,from:7,to:13});
  });
  it('blocks overlapping replacement and insertion inside the selection',()=>{
    expect(mapInlineAnchor(original,state.changes({from:8,to:10,insert:'other'})).conflict).toBe(true);
    expect(mapInlineAnchor(original,state.changes({from:8,insert:'other'})).conflict).toBe(true);
  });
  it('keeps conflict sticky even if later changes restore the text',()=>{
    expect(mapInlineAnchor({...original,conflict:true},state.changes({from:0,insert:'x'})).conflict).toBe(true);
  });
  it('maps deletions crossing the target without invalid offsets',()=>{
    const mapped=mapInlineAnchor(original,state.changes({from:0,to:14}));
    expect(mapped.conflict).toBe(true);expect(mapped.from).toBeLessThanOrEqual(mapped.to);
  });
});

import { StateEffect, Transaction, type TransactionSpec } from '@codemirror/state';
import { EditorView, type ViewUpdate } from '@codemirror/view';
import { afterEach, beforeEach, vi } from 'vitest';
import { createInlineAssistant, locateInlineEdits } from './inline-assistant';
let sequence=0;
beforeEach(()=>{vi.stubGlobal('requestAnimationFrame',()=>1);vi.stubGlobal('cancelAnimationFrame',()=>{});vi.stubGlobal('crypto',{randomUUID:()=>`edit-${++sequence}`});});
afterEach(()=>vi.unstubAllGlobals());
function editor(doc='alpha---beta---gamma',readonly=false){
 const controller=createInlineAssistant();let state=EditorState.create({doc,selection:{anchor:0,head:Math.min(5,doc.length)},extensions:[controller.extension,EditorState.readOnly.of(readonly)]});let writes=0;
 const fake={get state(){return state;},coordsAtPos:()=>null,hasFocus:true,dispatch(spec:Transaction|TransactionSpec){const tr=spec instanceof Transaction?spec:state.update(spec);state=tr.state;if(tr.docChanged)writes++;const update={docChanged:tr.docChanged,changes:tr.changes,state,view:fake,selectionSet:tr.selection!==undefined,geometryChanged:false,viewportChanged:false} as unknown as ViewUpdate;for(const listener of state.facet(EditorView.updateListener))listener(update);}};
 controller.bind(fake as unknown as EditorView);controller.capture();
 return {controller,view:fake,doc:()=>state.doc.toString(),writes:()=>writes,external:(spec:TransactionSpec)=>{fake.dispatch(spec);controller.refresh();}};
}
function token(controller:ReturnType<typeof createInlineAssistant>){const result=controller.beginEdits();if(!result.ok)throw new Error(result.message);return result.token;}
describe('request-snapshot multi edit planning',()=>{
 it('requires exact unique non-overlapping nonempty original text',()=>{
  expect(locateInlineEdits('same same',[{before:'same',after:'x'}])).toHaveProperty('message');
  expect(locateInlineEdits('abcdef',[{before:'abc',after:'A'},{before:'bcd',after:'B'}])).toHaveProperty('message');
  expect(locateInlineEdits('abcdef',[{before:'',after:'x'}])).toHaveProperty('message');
  expect(locateInlineEdits('abcdef',Array.from({length:13},()=>({before:'a',after:'x'})))).toHaveProperty('message');
 });
 it('previews and hand-edits candidates without changing or autosaving the document',()=>{
  const e=editor(),request=token(e.controller);expect(e.controller.prepareEdits(request,[{before:'alpha',after:'ALPHA'},{before:'beta',after:'BETA'}]).ok).toBe(true);
  const plan=e.controller.snapshot().plannedEdits;expect(plan).toHaveLength(2);expect(e.doc()).toBe('alpha---beta---gamma');expect(e.writes()).toBe(0);
  expect(e.controller.updatePreparedEdit(plan[0]!.id,'Hand edited A').ok).toBe(true);expect(e.controller.snapshot().plannedEdits[0]!.after).toBe('Hand edited A');expect(e.writes()).toBe(0);
  expect(e.controller.rejectPreparedEdit(plan[1]!.id).ok).toBe(true);expect(e.controller.snapshot().plannedEdits).toHaveLength(1);expect(e.doc()).toBe('alpha---beta---gamma');e.controller.destroy();
 });
 it('applies one paragraph at a time, maps the remainder, and undoes each whole accepted edit LIFO',()=>{
  const e=editor(),request=token(e.controller);e.controller.prepareEdits(request,[{before:'alpha',after:'A'},{before:'beta',after:'B'}]);let plans=e.controller.snapshot().plannedEdits;const first=plans[0]!.id,second=plans[1]!.id;
  e.controller.updatePreparedEdit(first,'Hand A');expect(e.controller.applyPreparedEdit(first).ok).toBe(true);expect(e.doc()).toBe('Hand A---beta---gamma');expect(e.writes()).toBe(1);
  plans=e.controller.snapshot().plannedEdits;expect(plans).toHaveLength(1);expect(plans[0]!.id).toBe(second);expect(plans[0]!.from).toBe(9);expect(plans[0]!.conflict).toBe(false);
  expect(e.controller.applyPreparedEdit(second).ok).toBe(true);expect(e.doc()).toBe('Hand A---B---gamma');expect(e.controller.snapshot().plannedEdits).toHaveLength(0);
  expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('Hand A---beta---gamma');expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('alpha---beta---gamma');e.controller.destroy();
 });
 it('preserves accurate target text and selection identity when the selected paragraph is replaced',()=>{
  const e=editor();const id=e.controller.snapshot().target!.selectionId;e.controller.prepareEdits(token(e.controller),[{before:'alpha',after:'Whole updated paragraph'}]);e.controller.applyPreparedEdits();expect(e.controller.snapshot().target).toMatchObject({from:0,to:23,text:'Whole updated paragraph',selectionId:id,conflict:false});e.controller.undo();expect(e.controller.snapshot().target).toMatchObject({from:0,to:5,text:'alpha',selectionId:id});e.controller.destroy();
 });
 it('undoes two adjacent paragraph deletions LIFO in their original order',()=>{
  const e=editor('abcdef');e.controller.prepareEdits(token(e.controller),[{before:'abc',after:''},{before:'def',after:''}]);const plans=e.controller.snapshot().plannedEdits;e.controller.applyPreparedEdit(plans[0]!.id);e.controller.applyPreparedEdit(plans[1]!.id);expect(e.doc()).toBe('');expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('def');expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('abcdef');e.controller.destroy();
 });
 it('can apply all ranges atomically and undo the complete accepted batch',()=>{
  const e=editor();e.controller.prepareEdits(token(e.controller),[{before:'alpha',after:'A'},{before:'beta',after:'B'}]);expect(e.controller.applyPreparedEdits().ok).toBe(true);expect(e.writes()).toBe(1);expect(e.doc()).toBe('A---B---gamma');expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('alpha---beta---gamma');e.controller.destroy();
 });
 it('maps unrelated transactions occurring while the model is generating',()=>{
  const e=editor(),request=token(e.controller);e.external({changes:{from:0,insert:'prefix '}});expect(e.controller.prepareEdits(request,[{before:'beta',after:'BETTER'}]).ok).toBe(true);expect(e.controller.snapshot().plannedEdits[0]!.from).toBe(15);expect(e.controller.applyPreparedEdits().ok).toBe(true);expect(e.doc()).toBe('prefix alpha---BETTER---gamma');e.controller.destroy();
 });
 it('rejects overlapping concurrent edits before prepare and invalidates existing plans after prepare',()=>{
  const e=editor(),request=token(e.controller);e.external({changes:{from:9,to:10,insert:'X'}});expect(e.controller.prepareEdits(request,[{before:'beta',after:'B'}]).ok).toBe(false);expect(e.controller.applyPreparedEdits().ok).toBe(false);e.controller.destroy();
  const f=editor();f.controller.prepareEdits(token(f.controller),[{before:'beta',after:'B'}]);f.external({changes:{from:9,to:10,insert:'X'}});expect(f.controller.snapshot().plannedEditsConflict).toBe(true);expect(f.controller.applyPreparedEdits().ok).toBe(false);f.controller.destroy();
 });
 it('refuses unsafe undo after a user or collaborative change inside the applied paragraph',()=>{
  const e=editor();e.controller.prepareEdits(token(e.controller),[{before:'beta',after:'BETTER'}]);e.controller.applyPreparedEdits();e.external({changes:{from:10,insert:'user'}});expect(e.controller.snapshot().canUndo).toBe(false);expect(e.controller.undo().ok).toBe(false);expect(e.doc()).toContain('user');e.controller.destroy();
 });
 it('permits safe undo after unrelated user typing without discarding it',()=>{
  const e=editor();e.controller.prepareEdits(token(e.controller),[{before:'beta',after:'B'}]);e.controller.applyPreparedEdits();e.external({changes:{from:0,insert:'user '}});expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('user alpha---beta---gamma');e.controller.destroy();
 });
 it('keeps explicit legacy selection replacement precise when the same selection occurs elsewhere',()=>{
  const e=editor('alpha alpha'),request=token(e.controller);expect(e.controller.prepareSelectionReplacement(request,'first').ok).toBe(true);expect(e.controller.applyPreparedEdits().ok).toBe(true);expect(e.doc()).toBe('first alpha');expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('alpha alpha');e.controller.destroy();
 });
 it('discard and recapture invalidate tokens without writing; starting another request preserves legal undo',()=>{
  const e=editor(),request=token(e.controller);e.controller.prepareEdits(request,[{before:'alpha',after:'A'}]);e.controller.discardEdits();expect(e.controller.snapshot().plannedEdits).toHaveLength(0);expect(e.controller.prepareEdits(request,[{before:'alpha',after:'A'}]).ok).toBe(false);expect(e.writes()).toBe(0);
  e.controller.prepareEdits(token(e.controller),[{before:'alpha',after:'A'}]);e.controller.applyPreparedEdits();token(e.controller);expect(e.controller.snapshot().canUndo).toBe(true);expect(e.controller.undo().ok).toBe(true);e.controller.destroy();
 });
 it('leaves the legacy apply/undo behavior intact and rejects readonly editing',()=>{
  const e=editor();expect(e.controller.apply('one').ok).toBe(true);expect(e.controller.apply('two').ok).toBe(true);expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('one---beta---gamma');expect(e.controller.undo().ok).toBe(true);expect(e.doc()).toBe('alpha---beta---gamma');e.controller.destroy();
  const f=editor(undefined,true);expect(f.controller.beginEdits().ok).toBe(false);expect(f.controller.apply('x').ok).toBe(false);f.controller.destroy();
 });
});

describe('inline review controls remain intents until React records a decision',()=>{
 it('routes stable IDs to the review listener without changing the document or consuming the plan',()=>{
  const e=editor();e.controller.prepareEdits(token(e.controller),[{before:'alpha',after:'A'},{before:'beta',after:'B'}]);const edits=e.controller.snapshot().plannedEdits;const review=vi.fn();const unsubscribe=e.controller.subscribeReview(review);e.controller.setReviewBlocked(false);
  expect(e.controller.requestReview('accept',edits[0]!.id).ok).toBe(true);expect(review).toHaveBeenLastCalledWith({kind:'accept',editId:edits[0]!.id});
  expect(e.controller.requestReview('reject',edits[1]!.id).ok).toBe(true);expect(review).toHaveBeenLastCalledWith({kind:'reject',editId:edits[1]!.id});expect(e.doc()).toBe('alpha---beta---gamma');expect(e.writes()).toBe(0);expect(e.controller.snapshot().plannedEdits).toHaveLength(2);
  unsubscribe();expect(e.controller.requestReview('accept',edits[0]!.id).ok).toBe(false);expect(review).toHaveBeenCalledTimes(2);e.controller.destroy();
 });
 it('blocks stale IDs and concurrent saving, and rejects unsafe acceptance while still permitting rejection',()=>{
  const e=editor();e.controller.prepareEdits(token(e.controller),[{before:'beta',after:'B'}]);const id=e.controller.snapshot().plannedEdits[0]!.id;const review=vi.fn();e.controller.subscribeReview(review);
  expect(e.controller.requestReview('accept',id).ok).toBe(false);expect(review).not.toHaveBeenCalled();e.controller.setReviewBlocked(false);expect(e.controller.requestReview('accept','expired-id').ok).toBe(false);
  e.external({changes:{from:9,to:10,insert:'X'}});expect(e.controller.requestReview('accept',id).ok).toBe(false);expect(e.controller.requestReview('reject',id).ok).toBe(true);expect(review).toHaveBeenCalledTimes(1);expect(e.doc()).toBe('alpha---bXta---gamma');expect(e.writes()).toBe(1);e.controller.destroy();
 });
 it('rechecks read-only state at the button intent boundary',()=>{
  const e=editor();e.controller.prepareEdits(token(e.controller),[{before:'alpha',after:'A'}]);const id=e.controller.snapshot().plannedEdits[0]!.id;const review=vi.fn();e.controller.subscribeReview(review);e.controller.setReviewBlocked(false);
  e.external({effects:StateEffect.reconfigure.of([e.controller.extension,EditorState.readOnly.of(true)])});expect(e.controller.requestReview('accept',id).ok).toBe(false);expect(e.controller.requestReview('reject',id).ok).toBe(false);expect(review).not.toHaveBeenCalled();expect(e.writes()).toBe(0);e.controller.destroy();
 });
 it('places each candidate on a block below red source lines instead of an inline horizontal widget',()=>{
  const e=editor('alpha\nbeta\ngamma');e.controller.prepareEdits(token(e.controller),[{before:'alpha',after:'A'},{before:'beta',after:'B'}]);const widgets:unknown[]=[];let redLines=0;
  for(const decorations of e.view.state.facet(EditorView.decorations))if(typeof decorations!=='function')decorations.between(0,e.view.state.doc.length,(_from,_to,value)=>{if(value.spec.widget)widgets.push(value.spec);if(value.spec.class==='scribe-inline-removed-line')redLines++;});
  expect(widgets).toHaveLength(2);expect(widgets.every(spec=>(spec as {block?:boolean}).block===true)).toBe(true);expect(redLines).toBe(2);expect(e.writes()).toBe(0);e.controller.destroy();
 });
});

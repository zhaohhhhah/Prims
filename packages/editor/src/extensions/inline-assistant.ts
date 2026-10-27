import { getInlineWritingContext, type InlineWritingContext } from './inline-context';
import { isolateHistory } from '@codemirror/commands';
import { StateEffect, StateField, Transaction, type ChangeDesc, type Extension } from '@codemirror/state';
import { Decoration, EditorView, keymap, WidgetType, type DecorationSet } from '@codemirror/view';

export interface InlineAnchor { readonly from:number; readonly to:number; readonly text:string; readonly conflict:boolean; readonly selectionId?:string }
export interface InlineSelection { readonly from:number; readonly to:number; readonly text:string }
export interface InlinePlannedEdit { readonly id:string; readonly from:number; readonly to:number; readonly before:string; readonly after:string; readonly conflict:boolean }
export interface InlineSnapshot {
 readonly selection:InlineSelection; readonly target:InlineAnchor|null;
 readonly position:{readonly left:number;readonly top:number}|null;
 readonly focusRequest:number; readonly canUndo:boolean; readonly allowAutoOpen:boolean;
 readonly plannedEdits:readonly InlinePlannedEdit[]; readonly plannedEditsConflict:boolean;
}
export type InlineResult={ok:true}|{ok:false;message:string};
export interface InlineReviewIntent {readonly kind:'accept'|'reject';readonly editId:string}
export type InlineEditSessionResult={ok:true;token:string}|{ok:false;message:string};
const MAX_EDITS=12,MAX_BEFORE=16000,MAX_AFTER=24000,MAX_TOTAL=80000,MAX_DOCUMENT=2000000,MAX_CHANGES=512;
const failure=(message:string):InlineResult=>({ok:false,message});
/** Includes remote CRDT transactions; any intersecting change remains a conflict even after restoration. */
export function mapInlineAnchor(anchor:InlineAnchor,changes:ChangeDesc):InlineAnchor {
 let conflict=anchor.conflict;
 changes.iterChangedRanges((from,to)=>{if(from===to?(from>anchor.from&&from<anchor.to)||(anchor.from===anchor.to&&from===anchor.from):from<anchor.to&&to>anchor.from)conflict=true;});
 const from=changes.mapPos(anchor.from,1),to=Math.max(from,changes.mapPos(anchor.to,-1));
 return {...anchor,from,to,conflict};
}
function validateEdits(edits:readonly {before:string;after:string}[]):InlineResult {
 if(!edits.length||edits.length>MAX_EDITS)return failure('一次可预览 1–12 处修改');
 let total=0;
 for(const edit of edits){if(typeof edit.before!=='string'||typeof edit.after!=='string'||!edit.before.trim()||edit.before.length>MAX_BEFORE||edit.after.length>MAX_AFTER)return failure('候选原文不能为空，且每处原文和修改需在长度限制内');total+=edit.before.length+edit.after.length;}
 return total>MAX_TOTAL?failure('本轮候选修改总量过大，请分次处理'):{ok:true};
}
/** Exact matching is against the request-time LOCAL document, never a truncated model window. */
export function locateInlineEdits(document:string,edits:readonly {before:string;after:string}[]):InlinePlannedEdit[]|{message:string} {
 const checked=validateEdits(edits);if(!checked.ok)return {message:checked.message};
 const result:InlinePlannedEdit[]=[];
 for(const edit of edits){const from=document.indexOf(edit.before);if(from<0)return {message:'候选原文在请求时文档中不存在，请重新生成'};if(document.indexOf(edit.before,from+1)!==-1)return {message:'候选原文在文档中出现多次，请让 AI 提供更完整且唯一的原文'};result.push({id:crypto.randomUUID(),from,to:from+edit.before.length,...edit,conflict:false});}
 result.sort((a,b)=>a.from-b.from);
 if(result.some((edit,index)=>index>0&&edit.from<result[index-1]!.to))return {message:'候选修改范围重叠，请重新生成互不重叠的修改'};
 return result;
}
function mapEdit(edit:InlinePlannedEdit,changes:ChangeDesc):InlinePlannedEdit {const mapped=mapInlineAnchor({...edit,text:edit.before},changes);return {...edit,from:mapped.from,to:mapped.to,conflict:mapped.conflict};}
const highlight=StateEffect.define<{from:number;to:number}|null>();
const preview=StateEffect.define<readonly InlinePlannedEdit[]>();
const highlights=StateField.define<DecorationSet>({create:()=>Decoration.none,update(value,tr){let next=value.map(tr.changes);for(const effect of tr.effects)if(effect.is(highlight))next=effect.value&&effect.value.to>effect.value.from?Decoration.set([Decoration.mark({class:'scribe-inline-target'}).range(effect.value.from,effect.value.to)]):Decoration.none;return next;},provide:field=>EditorView.decorations.from(field)});

class CandidateWidget extends WidgetType {
 constructor(readonly edit:InlinePlannedEdit,readonly maxLength:number,readonly isReadOnly:boolean,readonly blocked:boolean,readonly update:(id:string,text:string)=>InlineResult,readonly decide:(kind:'accept'|'reject',id:string)=>InlineResult){super();}
 override eq(other:CandidateWidget){return this.edit.id===other.edit.id&&this.edit.after===other.edit.after&&this.edit.conflict===other.edit.conflict&&this.maxLength===other.maxLength&&this.isReadOnly===other.isReadOnly&&this.blocked===other.blocked;}
 override toDOM(){
  const dom=document.createElement('div');dom.className='scribe-inline-candidate';dom.dataset.editId=this.edit.id;
  const title=document.createElement('div');title.className='scribe-inline-candidate-title';title.textContent='+ 建议修改';dom.append(title);
  const input=document.createElement('textarea');input.setAttribute('aria-label','正文中的候选修改');input.spellcheck=false;
  input.addEventListener('input',()=>{const previous=input.dataset.previewValue??'';const result=this.update(this.edit.id,input.value);if(!result.ok){input.value=previous;input.setCustomValidity(result.message);input.reportValidity();}else input.setCustomValidity('');});
  input.addEventListener('keydown',event=>event.stopPropagation());dom.append(input);
  const controls=document.createElement('div');controls.className='scribe-inline-candidate-controls';
  for(const [kind,label] of [['accept','接受本段'],['reject','拒绝本段']] as const){const button=document.createElement('button');button.type='button';button.dataset.reviewAction=kind;button.textContent=label;button.addEventListener('mousedown',event=>event.preventDefault());button.addEventListener('click',event=>{event.preventDefault();event.stopPropagation();const result=this.decide(kind,this.edit.id);if(!result.ok){input.setCustomValidity(result.message);input.reportValidity();}});controls.append(button);}
  dom.append(controls);this.updateDOM(dom);return dom;
 }
 override updateDOM(dom:HTMLElement){const input=dom.querySelector('textarea')!;if(input.value!==this.edit.after){const start=input.selectionStart,end=input.selectionEnd;input.value=this.edit.after;input.setSelectionRange(Math.min(start,input.value.length),Math.min(end,input.value.length));}input.dataset.previewValue=this.edit.after;input.maxLength=this.maxLength;input.readOnly=this.isReadOnly||this.edit.conflict||this.blocked;input.rows=Math.min(12,Math.max(2,this.edit.after.split('\n').length));for(const button of dom.querySelectorAll<HTMLButtonElement>('button[data-review-action]'))button.disabled=this.isReadOnly||this.blocked||(button.dataset.reviewAction==='accept'&&(this.edit.conflict||this.edit.before===this.edit.after));dom.classList.toggle('scribe-inline-candidate-conflict',this.edit.conflict);return true;}
 override ignoreEvent(){return true;}
}
export interface InlineAssistantController {
 subscribeReview(listener:(intent:InlineReviewIntent)=>void):()=>void;setReviewBlocked(blocked:boolean):void;requestReview(kind:'accept'|'reject',editId:string):InlineResult;
 readonly extension:Extension;bind(view:EditorView):void;subscribe(listener:()=>void):()=>void;snapshot():InlineSnapshot;
 capture():InlineResult;close():void;clearUndo():void;refresh():void;context():{before:string;after:string};documentContext():string;writingContext():InlineWritingContext;
 beginEdits():InlineEditSessionResult;prepareEdits(token:string,edits:readonly {before:string;after:string}[]):InlineResult;prepareSelectionReplacement(token:string,text:string):InlineResult;
 updatePreparedEdit(editId:string,text:string):InlineResult;applyPreparedEdit(editId:string):InlineResult;rejectPreparedEdit(editId:string):InlineResult;applyPreparedEdits():InlineResult;discardEdits():void;revealPreparedEdit(editId:string):InlineResult;focusPreparedEdit(editId:string):InlineResult;
 apply(text:string):InlineResult;undo():InlineResult;destroy():void;
}
type EditSession={token:string;document:string;selection:InlineAnchor|null;changes:ChangeDesc[];invalid:boolean};
type UndoStep={kind:'legacy';before:string}|{kind:'edits';anchors:InlineAnchor[];originals:string[]};
export function createInlineAssistant():InlineAssistantController {
 let view:EditorView|null=null,target:InlineAnchor|null=null,focusRequest=0,ownChange=false,allowAutoOpen=false,destroyed=false,refreshFrame=0;
 let session:EditSession|null=null,planned:InlinePlannedEdit[]=[],reviewBlocked=true;
 const reviewListeners=new Set<(intent:InlineReviewIntent)=>void>();
 const undoStack:UndoStep[]=[],listeners=new Set<()=>void>();
 let snapshot:InlineSnapshot={selection:{from:0,to:0,text:''},target:null,position:null,focusRequest:0,canUndo:false,allowAutoOpen:false,plannedEdits:[],plannedEditsConflict:false};
 function undoValid(step:UndoStep|undefined):boolean {if(!view||view.state.readOnly||!step)return false;if(step.kind==='legacy')return !!target&&!target.conflict&&view.state.sliceDoc(target.from,target.to)===target.text;return step.anchors.every(a=>!a.conflict&&view!.state.sliceDoc(a.from,a.to)===a.text);}
 function refresh(){if(!view||destroyed)return;const range=view.state.selection.main,position=view.coordsAtPos(target?.to??range.to);snapshot={selection:{from:range.from,to:range.to,text:view.state.sliceDoc(range.from,range.to)},target,position:position?{left:position.left,top:position.bottom}:snapshot.position,focusRequest,canUndo:undoValid(undoStack.at(-1)),allowAutoOpen,plannedEdits:planned.map(e=>({...e})),plannedEditsConflict:planned.some(e=>e.conflict)};for(const listener of listeners)listener();}
 function showHighlight(){if(view)view.dispatch({effects:highlight.of(target)});}
 function showPreview(){if(view)view.dispatch({effects:preview.of(planned)});}
 function discard(){session=null;planned=[];showPreview();refresh();}
 function scheduleRefresh(){if(!refreshFrame)refreshFrame=requestAnimationFrame(()=>{refreshFrame=0;refresh();});}
 function valid():InlineResult {if(!view||!target)return failure('请重新选择需要修改的文字');if(view.state.readOnly)return failure('当前文档为只读');if(target.conflict||view.state.sliceDoc(target.from,target.to)!==target.text)return failure('这段原文已发生变化，请重新选区后再生成，避免覆盖新内容');return {ok:true};}
 function write(text:string){if(!view||!target)return;const from=target.from;ownChange=true;try{view.dispatch({changes:{from,to:target.to,insert:text},selection:{anchor:from,head:from+text.length},annotations:[Transaction.userEvent.of('input.inline-ai'),isolateHistory.of('full')]});target={from,to:from+text.length,text,conflict:false,...(target.selectionId?{selectionId:target.selectionId}:{})};showHighlight();refresh();}finally{ownChange=false;}}
 function prepare(token:string,edits:readonly {before:string;after:string}[],selectionOnly=false):InlineResult {
  planned=[];showPreview();
  if(!view||!session||session.token!==token||session.invalid)return failure('请求对应的文档快照已失效，请重新生成');
  if(view.state.readOnly)return failure('当前文档为只读');
  let found:InlinePlannedEdit[]|{message:string};
  if(selectionOnly){const checked=validateEdits(edits);if(!checked.ok)return checked;const anchor=session.selection;if(!anchor||anchor.conflict)return failure('请求时没有有效选区，请重新选择文字');found=[{id:crypto.randomUUID(),from:anchor.from,to:anchor.to,before:anchor.text,after:edits[0]!.after,conflict:false}];}
  else found=locateInlineEdits(session.document,edits);
  if(!Array.isArray(found))return failure(found.message);
  for(const change of session.changes)found=found.map(edit=>mapEdit(edit,change));
  planned=found.map(edit=>({...edit,conflict:edit.conflict||view!.state.sliceDoc(edit.from,edit.to)!==edit.before}));
  showPreview();refresh();return planned.some(e=>e.conflict)?failure('候选原文在生成期间已被修改，冲突段落不能应用'):{ok:true};
 }
 function applyPlans(selected:InlinePlannedEdit[]):InlineResult {
  if(!view||view.state.readOnly)return failure('当前文档为只读或尚未就绪');
  if(!selected.length)return failure('没有待应用的候选修改');
  if(selected.some(e=>e.conflict||view!.state.sliceDoc(e.from,e.to)!==e.before))return failure('候选原文已变化，请拒绝冲突段落后重新生成');
  if(selected.every(e=>e.before===e.after))return failure('候选内容与原文相同');
  const ids=new Set(selected.map(e=>e.id));planned=planned.filter(e=>!ids.has(e.id));session=null;
  selected.sort((a,b)=>a.from-b.from);const specs=selected.map(e=>({from:e.from,to:e.to,insert:e.after}));
  const tr=view.state.update({changes:specs,annotations:[Transaction.userEvent.of('input.inline-ai'),isolateHistory.of('full')]});
  const targetOverlap=target?selected.findIndex(e=>e.from<target!.to&&e.to>target!.from):-1;
  const mappedTarget=target?mapInlineAnchor(target,tr.changes):null;
  let delta=0;const anchors=selected.map(e=>{const from=e.from+delta;delta+=e.after.length-(e.to-e.from);return {from,to:from+e.after.length,text:e.after,conflict:false};});
  ownChange=true;try{view.dispatch(tr);if(mappedTarget)target=targetOverlap>=0?{...anchors[targetOverlap]!,...(target?.selectionId?{selectionId:target.selectionId}:{})}:{...mappedTarget,text:view.state.sliceDoc(mappedTarget.from,mappedTarget.to),conflict:false};undoStack.push({kind:'edits',anchors,originals:selected.map(e=>e.before)});if(undoStack.length>40)undoStack.shift();showHighlight();showPreview();refresh();}finally{ownChange=false;}return {ok:true};
 }
 const previews=StateField.define<DecorationSet>({create:()=>Decoration.none,update(value,tr){let next=value.map(tr.changes);for(const effect of tr.effects)if(effect.is(preview)){const ranges=[];const removedLines=new Set<number>();const total=effect.value.reduce((n,e)=>n+e.before.length+e.after.length,0);for(const edit of effect.value){if(edit.to>edit.from){ranges.push(Decoration.mark({class:edit.conflict?'scribe-inline-before-conflict':'scribe-inline-before'}).range(edit.from,edit.to));const first=tr.state.doc.lineAt(edit.from).number,last=tr.state.doc.lineAt(Math.max(edit.from,edit.to-1)).number;for(let line=first;line<=last;line++)removedLines.add(tr.state.doc.line(line).from);}ranges.push(Decoration.widget({widget:new CandidateWidget(edit,Math.min(MAX_AFTER,MAX_TOTAL-total+edit.after.length),tr.state.readOnly,reviewBlocked,(id,text)=>controller.updatePreparedEdit(id,text),(kind,id)=>controller.requestReview(kind,id)),block:true,side:1}).range(edit.to));}for(const from of removedLines)ranges.push(Decoration.line({class:'scribe-inline-removed-line'}).range(from));next=Decoration.set(ranges,true);}return next;},provide:field=>EditorView.decorations.from(field)});
 const extension:Extension=[highlights,previews,EditorView.baseTheme({
  '.scribe-inline-target':{backgroundColor:'rgba(96,165,250,.16)',borderRadius:'2px'},
  '.scribe-inline-before':{backgroundColor:'var(--prims-inline-delete-bg, rgba(239,68,68,.12))',color:'var(--prims-inline-delete-fg, inherit)',textDecoration:'line-through',textDecorationColor:'var(--prims-inline-delete-line, #dc2626)'},
  '.scribe-inline-removed-line':{backgroundColor:'var(--prims-inline-delete-bg, rgba(239,68,68,.12))',borderLeft:'3px solid var(--prims-inline-delete-line, #dc2626)'},
  '.scribe-inline-before-conflict':{backgroundColor:'var(--prims-inline-conflict-bg, rgba(245,158,11,.2))'},
  '.scribe-inline-candidate':{display:'block',boxSizing:'border-box',width:'100%',minWidth:'0',maxWidth:'100%',margin:'4px 0 8px',padding:'8px 10px',border:'1px solid var(--prims-inline-add-border, #22c55e)',borderLeftWidth:'3px',borderRadius:'3px',backgroundColor:'var(--prims-inline-add-bg, rgba(34,197,94,.10))',color:'var(--prims-inline-add-fg, inherit)',whiteSpace:'normal',overflow:'hidden'},
  '.scribe-inline-candidate-title':{font:'11px sans-serif',color:'var(--prims-inline-add-title, #15803d)',marginBottom:'4px'},
  '.scribe-inline-candidate textarea':{display:'block',boxSizing:'border-box',width:'100%',minWidth:'0',maxWidth:'100%',resize:'vertical',font:'inherit',lineHeight:'1.5',border:'0',outline:'none',background:'transparent',color:'inherit',whiteSpace:'pre-wrap',overflowWrap:'anywhere'},
  '.scribe-inline-candidate-controls':{display:'flex',justifyContent:'flex-end',gap:'6px',marginTop:'6px',font:'12px sans-serif'},
  '.scribe-inline-candidate-controls button':{cursor:'pointer',padding:'4px 8px',border:'1px solid var(--prims-inline-add-border, #22c55e)',borderRadius:'4px',background:'transparent',color:'inherit'},
  '.scribe-inline-candidate-controls button:disabled':{cursor:'default',opacity:'.5'},
  '.scribe-inline-candidate-conflict':{borderColor:'var(--prims-inline-conflict-border, #f59e0b)',backgroundColor:'var(--prims-inline-conflict-bg, rgba(245,158,11,.2))'}
 }),EditorView.updateListener.of(update=>{
  if(update.startState&&update.startState.readOnly!==update.state.readOnly)queueMicrotask(()=>{if(!destroyed)showPreview();});
  if(update.docChanged)allowAutoOpen=false;else if(update.selectionSet)allowAutoOpen=update.view.hasFocus;
  if(update.docChanged){
   if(session){session.changes.push(update.changes);if(session.changes.length>MAX_CHANGES){session.invalid=true;session.changes=[];}}
   planned=planned.map(edit=>{const mapped=mapEdit(edit,update.changes);return {...mapped,conflict:mapped.conflict||update.state.sliceDoc(mapped.from,mapped.to)!==mapped.before};});
   for(const step of undoStack)if(step.kind==='edits')step.anchors=step.anchors.map(a=>{let mapped=mapInlineAnchor(a,update.changes);if(ownChange&&a.from===a.to){let conflict=a.conflict;update.changes.iterChangedRanges((from,to)=>{if(from<a.from&&to>a.from)conflict=true;});const from=update.changes.mapPos(a.from,-1);mapped={...a,from,to:from,conflict};}return {...mapped,conflict:mapped.conflict||update.state.sliceDoc(mapped.from,mapped.to)!==mapped.text};});
   if(target&&!ownChange){target=mapInlineAnchor(target,update.changes);if(update.state.sliceDoc(target.from,target.to)!==target.text)target={...target,conflict:true};}
   // Decorations themselves map in this transaction. Rebuild after dispatch, not recursively inside an update listener.
   if(planned.length)queueMicrotask(()=>{if(!destroyed){showPreview();refresh();}});
  }
  if(update.docChanged||update.selectionSet||update.geometryChanged||update.viewportChanged)scheduleRefresh();
 }),keymap.of([{key:'Mod-k',preventDefault:true,run:()=>{if(!view||view.state.selection.main.empty)return false;if(!target)controller.capture();focusRequest++;refresh();return true;}}])];
 const controller:InlineAssistantController={extension,subscribeReview(listener){reviewListeners.add(listener);return()=>reviewListeners.delete(listener);},setReviewBlocked(blocked){if(reviewBlocked===blocked)return;reviewBlocked=blocked;showPreview();},requestReview(kind,editId){if(!view||view.state.readOnly)return failure('当前文档为只读');if(reviewBlocked)return failure('请等待当前操作完成');const edit=planned.find(item=>item.id===editId);if(!edit)return failure('该候选修改已失效');if(kind==='accept'&&(edit.conflict||view.state.sliceDoc(edit.from,edit.to)!==edit.before))return failure('原文已变化，不能应用该候选');if(!reviewListeners.size)return failure('请先打开 AI 助手处理候选');for(const listener of reviewListeners)listener({kind,editId});return {ok:true};},bind(next){view=next;refresh();},subscribe(listener){listeners.add(listener);return()=>listeners.delete(listener);},snapshot:()=>snapshot,
  capture(){if(!view)return failure('当前文档尚未就绪');const range=view.state.selection.main;if(range.empty)return failure('先选择一段文字，再与 AI 对话');if(range.to-range.from>16000)return failure('一次请选择不超过 16000 字符');discard();undoStack.length=0;target={from:range.from,to:range.to,text:view.state.sliceDoc(range.from,range.to),conflict:false,selectionId:crypto.randomUUID()};showHighlight();refresh();return {ok:true};},
  clearUndo(){undoStack.length=0;refresh();},close(){target=null;undoStack.length=0;discard();showHighlight();refresh();},refresh,
  beginEdits(){if(!view||view.state.readOnly)return {ok:false,message:'当前文档为只读或尚未就绪'};if(view.state.doc.length>MAX_DOCUMENT)return {ok:false,message:'文档过长，请缩小后再生成修改'};planned=[];session={token:crypto.randomUUID(),document:view.state.doc.toString(),selection:target?{...target}:null,changes:[],invalid:false};showPreview();refresh();return {ok:true,token:session.token};},
  prepareEdits:(token,edits)=>prepare(token,edits),prepareSelectionReplacement(token,text){return prepare(token,[{before:session?.selection?.text??'',after:text}],true);},
  updatePreparedEdit(id,text){if(!view||view.state.readOnly)return failure('当前文档为只读');const edit=planned.find(e=>e.id===id);if(!edit)return failure('该候选修改已失效');if(edit.conflict)return failure('原文已变化，不能继续编辑此候选');const checked=validateEdits(planned.map(e=>e.id===id?{...e,after:text}:e));if(!checked.ok)return checked;planned=planned.map(e=>e.id===id?{...e,after:text}:e);showPreview();refresh();return {ok:true};},
  applyPreparedEdit(id){const edit=planned.find(e=>e.id===id);return edit?applyPlans([edit]):failure('该候选修改已失效');},rejectPreparedEdit(id){if(!planned.some(e=>e.id===id))return failure('该候选修改已失效');planned=planned.filter(e=>e.id!==id);showPreview();refresh();return {ok:true};},applyPreparedEdits(){return applyPlans([...planned]);},discardEdits:discard,revealPreparedEdit(id){const edit=planned.find(e=>e.id===id);if(!view||!edit)return failure('该候选修改已失效');view.dispatch({effects:EditorView.scrollIntoView(edit.from,{y:'center'})});return {ok:true};},focusPreparedEdit(id){return this.revealPreparedEdit(id);},
  documentContext(){return this.writingContext().document;},writingContext(){return getInlineWritingContext(view?.state.doc.toString()??'',target?.from??0,target?.to??0);},context(){return view&&target?{before:view.state.sliceDoc(Math.max(0,target.from-1200),target.from),after:view.state.sliceDoc(target.to,Math.min(view.state.doc.length,target.to+1200))}:{before:'',after:''};},
  apply(text){const checked=valid();if(!checked.ok)return checked;if(text.length>100000)return failure('生成结果过长，未应用');discard();undoStack.push({kind:'legacy',before:target!.text});write(text);return {ok:true};},
  undo(){const step=undoStack.at(-1);if(!undoValid(step))return failure('最近应用的原文已变化，不能安全撤销');if(!view||!step)return failure('没有可撤销的 AI 修改');undoStack.pop();if(step.kind==='legacy'){write(step.before);return {ok:true};}const mappedTarget=target;const tr=view.state.update({changes:step.anchors.map((a,i)=>({from:a.from,to:a.to,insert:step.originals[i]!})),annotations:[Transaction.userEvent.of('input.inline-ai.undo'),isolateHistory.of('full')]});const overlap=mappedTarget?step.anchors.findIndex(a=>(a.from<mappedTarget.to&&a.to>mappedTarget.from)||(a.from===a.to&&a.from===mappedTarget.from&&mappedTarget.from===mappedTarget.to)):-1;const nextTarget=mappedTarget?mapInlineAnchor(mappedTarget,tr.changes):null;let offset=0;const restored=step.anchors.map((a,i)=>{const from=a.from+offset;offset+=step.originals[i]!.length-(a.to-a.from);return {from,to:from+step.originals[i]!.length,text:step.originals[i]!,conflict:false};});ownChange=true;try{view.dispatch(tr);if(nextTarget)target=overlap>=0?{...restored[overlap]!,...(target?.selectionId?{selectionId:target.selectionId}:{})}:{...nextTarget,text:view.state.sliceDoc(nextTarget.from,nextTarget.to),conflict:false};showHighlight();showPreview();refresh();}finally{ownChange=false;}return {ok:true};},
  destroy(){destroyed=true;cancelAnimationFrame(refreshFrame);listeners.clear();reviewListeners.clear();view=null;target=null;session=null;planned=[];undoStack.length=0;},
 };return controller;
}

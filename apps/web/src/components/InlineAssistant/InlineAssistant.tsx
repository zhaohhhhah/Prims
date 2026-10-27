import type { InlineAssistantController } from '@scribe/editor';
import { Button, Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from '@scribe/ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import DiffMatchPatch from 'diff-match-patch';
import { Check, Loader2, Move, PanelBottom, PanelRight, RotateCcw, Send, Sparkles, Square, X } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';
import { createPortal } from 'react-dom';
import { Link } from 'react-router-dom';
import { defaultModelChoice, isModelProfileReady, profileModelNames, useModelProfiles } from '../../lib/model-profiles';
import { streamInline } from '../../lib/inline-protocol';
import { parseInlineProposal } from '../../lib/inline-result';
import { mergeContextMessages, projectAIContext, proposalForTarget, type ProjectAIContext, type ProjectAIMessage } from '../../lib/project-ai-context';
import { useAuthStore } from '../../stores/auth';
import { ProjectActionsPreview } from './ProjectActionsPreview';
import { parseProjectActions, type ProjectActionInput } from '../../lib/project-actions';
import { buildVersionReview } from '../../lib/version-review';
import { sortAssistantThreads } from '../../lib/assistant-threads';
import { assistantRequestTarget } from '../../lib/assistant-target';

import { ASSISTANT_OPEN_EVENT, type AssistantOpenDetail } from '../../lib/assistant-events';
import type { AssistantLayout, AssistantLayoutPatch, AssistantMode, AssistantViewport } from '../../lib/assistant-layout';
export { ASSISTANT_OPEN_EVENT, openProjectAssistant } from '../../lib/assistant-events';
type Phase='ready'|'requesting'|'generating'|'preview'|'answered'|'applied'|'rejected'|'undone'|'stopped'|'error';
const labels:Record<Phase,string>={ready:'',requesting:'正在连接你的模型…',generating:'正在整理上下文并生成…',preview:'修改预览已就绪，等待确认',answered:'答复已完成',applied:'修改已应用',rejected:'已拒绝方案',undone:'已撤销这次 AI 修改',stopped:'已停止生成',error:'本次请求未完成'};
const emptySnapshot={selection:{from:0,to:0,text:''},target:null,position:null,focusRequest:0,canUndo:false,allowAutoOpen:false,plannedEdits:[],plannedEditsConflict:false};
type Snapshot=ReturnType<InlineAssistantController['snapshot']>;
type Proposal={selectionId:string;filePath:string;threadId:string};
function Preview({before,after}:{before:string;after:string}) {
 const diffs=useMemo(()=>{const engine=new DiffMatchPatch();engine.Diff_Timeout=.15;const result=engine.diff_main(before,after);engine.diff_cleanupSemantic(result);return result;},[before,after]);
 return <div data-testid="inline-preview"><p className="mb-2 text-[11px] text-muted-foreground">修改预览 · <span className="text-red-600">删除</span> / <span className="text-emerald-700">新增</span></p><pre className="max-h-52 overflow-auto whitespace-pre-wrap break-words rounded-md border bg-muted/25 p-3 font-mono text-xs leading-relaxed" aria-label="选区修改差异">{diffs.map(([kind,text],index)=>kind<0?<del key={index} className="bg-red-100 text-red-900 dark:bg-red-950 dark:text-red-200">{text}</del>:kind>0?<ins key={index} className="bg-emerald-100 text-emerald-950 no-underline dark:bg-emerald-950 dark:text-emerald-200">{text}</ins>:<span key={index}>{text}</span>)}</pre></div>;
}
interface InlineAssistantProps {controller:InlineAssistantController|null;projectId:string;filePath:string;readOnly:boolean;layout:AssistantLayout;effectiveMode:AssistantMode;onLayoutChange:(patch:AssistantLayoutPatch)=>void;viewport:AssistantViewport;rightHost:HTMLElement|null;bottomHost:HTMLElement|null;diagnosticPending:boolean;beforeProjectAction:()=>Promise<void>}
export function InlineAssistant({controller,projectId,filePath,readOnly,layout,effectiveMode,onLayoutChange,viewport,rightHost,bottomHost,diagnosticPending,beforeProjectAction}:InlineAssistantProps) {
 const userId=useAuthStore(state=>state.user?.id);const cache=useQueryClient();
 const owner=`${userId??''}:${projectId}`;const ownerRef=useRef({key:owner,epoch:0});if(ownerRef.current.key!==owner)ownerRef.current={key:owner,epoch:ownerRef.current.epoch+1};
 const contextKey=['project-ai-context',userId,projectId];const threadsKey=['project-ai-threads',userId,projectId];
 const context=useQuery({queryKey:contextKey,queryFn:()=>projectAIContext.get(projectId),enabled:!!userId&&!!projectId,refetchOnWindowFocus:false,staleTime:0});
 const conversations=useQuery({queryKey:threadsKey,queryFn:()=>projectAIContext.threads(projectId),enabled:!!userId&&!!projectId,refetchOnWindowFocus:false});
 const [bubbleOpen,setBubbleOpen]=useState(false);
 const [fileActions,setFileActions]=useState<ProjectActionInput[]>([]);const fileActionSelection=useRef<string|undefined>(undefined);const [actionsHost,setActionsHost]=useState<HTMLDivElement|null>(null);
 const [restoreRequest,setRestoreRequest]=useState<AssistantOpenDetail['restore']|null>(null);
 const [snapshot,setSnapshot]=useState<Snapshot>(controller?.snapshot()??emptySnapshot);
 const [pending,setPending]=useState<ProjectAIMessage[]>([]);const [saving,setSaving]=useState(false);const [saveError,setSaveError]=useState('');
 const [prompt,setPrompt]=useState('');const [proposal,setProposal]=useState<Proposal|null>(null);const [phase,setPhase]=useState<Phase>('ready');
 const [importedText,setImportedText]=useState<string|null>(null);
 const [error,setError]=useState('');const [notice,setNotice]=useState('');const [received,setReceived]=useState(0);const [elapsed,setElapsed]=useState(0);
 const [imageWarning,setImageWarning]=useState(false);const [hideImageWarning,setHideImageWarning]=useState(false);const [activity,setActivity]=useState('');
 const input=useRef<HTMLTextAreaElement>(null);const bubbleInput=useRef<HTMLTextAreaElement>(null);const pointerCleanup=useRef<(()=>void)|null>(null);const thread=useRef<HTMLDivElement>(null);const popup=useRef<HTMLDivElement>(null);
 const abort=useRef<AbortController|null>(null);const generation=useRef(0);const dismissed=useRef('');const lastTarget=useRef('');const previousFocus=useRef(0);
 const busy=phase==='requesting'||phase==='generating';const target=snapshot.target;
 const turns=mergeContextMessages(context.data?.messages??[],pending);
 const visibleTurns=turns.filter(turn=>turn.role!=='action');
 const displayedThreads=useMemo(()=>{const rows=conversations.data?.threads??[];if(!context.data||rows.some(row=>row.id===context.data.threadId))return sortAssistantThreads(rows);const messages=context.data.messages;const last=messages.at(-1)?.createdAt;return sortAssistantThreads([...rows,{id:context.data.threadId,title:messages.find(message=>message.role==='user')?.content.slice(0,60)||'新对话',updatedAt:last??'1970-01-01',createdAt:messages[0]?.createdAt??'1970-01-01',lastMessageAt:last??null,messageCount:messages.length}]);},[conversations.data,context.data]);
 const activeProposal=proposalForTarget(proposal,target?.selectionId,filePath,context.data?.threadId);
 const plannedEdits=activeProposal?snapshot.plannedEdits:[];

 const profiles=useModelProfiles();
 const [modelChoice,setModelChoice]=useState({scope:ownerRef.current,id:'auto'});
 const preferredChoice=defaultModelChoice(profiles.data);
 const requestedChoice=modelChoice.scope===ownerRef.current?modelChoice.id:'auto';
 const selectedModelId=requestedChoice==='auto'?preferredChoice?.value??'':requestedChoice;
 const modelChoices=useMemo(()=>profiles.data?.profiles.flatMap(profile=>profileModelNames(profile).map(model=>({value:JSON.stringify([profile.id,model]),profile,model})))??[],[profiles.data]);
 const selectedChoice=modelChoices.find(choice=>choice.value===selectedModelId);
 const selectedModelReady=!!selectedChoice&&isModelProfileReady(selectedChoice.profile);
 useEffect(()=>{setModelChoice({scope:ownerRef.current,id:'auto'});},[owner]);
 useEffect(()=>{if(requestedChoice!=='auto'&&profiles.isSuccess&&!modelChoices.some(choice=>choice.value===selectedModelId))setModelChoice({scope:ownerRef.current,id:'auto'});},[requestedChoice,selectedModelId,profiles.isSuccess,modelChoices,owner]);
 const signature=`${snapshot.selection.from}:${snapshot.selection.to}:${snapshot.selection.text}`;
 const editable=!!target&&!readOnly;
 const blocked=busy||saving||pending.length>0||!context.data||diagnosticPending;
 useEffect(()=>{if(!controller)return;controller.setReviewBlocked(blocked||readOnly||!activeProposal);return()=>controller.setReviewBlocked(true);},[controller,blocked,readOnly,activeProposal]);
 useEffect(()=>controller?.subscribeReview(intent=>action(intent.kind,intent.editId)));
 useEffect(()=>{setSnapshot(controller?.snapshot()??emptySnapshot);previousFocus.current=0;return controller?.subscribe(()=>setSnapshot(controller.snapshot()));},[controller]);
 useEffect(()=>{generation.current++;abort.current?.abort();setProposal(null);setPhase('ready');setError('');controller?.discardEdits();},[controller,filePath]);
 useEffect(()=>{generation.current++;abort.current?.abort();setPending([]);setSaving(false);setProposal(null);setImportedText(null);setPrompt('');setPhase('ready');setNotice('');setError('');setActivity('');setImageWarning(false);setBubbleOpen(false);setFileActions([]);setRestoreRequest(null);setSaveError('');},[projectId,userId]);
 useEffect(()=>{try{setHideImageWarning(!!userId&&localStorage.getItem(`scribe-ai-image-warning:${userId}`)==='hidden');}catch{setHideImageWarning(false);}},[userId]);
 useEffect(()=>{setProposal(null);setFileActions([]);controller?.discardEdits();},[context.data?.threadId]);
 useEffect(()=>()=>{generation.current++;abort.current?.abort();pointerCleanup.current?.();},[]);
 useEffect(()=>{
   if(diagnosticPending||!controller||!snapshot.selection.text.trim()||!snapshot.allowAutoOpen||snapshot.selection.text.length>16000||dismissed.current===signature)return;
   if(target?.from===snapshot.selection.from&&target?.to===snapshot.selection.to&&target.text===snapshot.selection.text)return;
   const timer=window.setTimeout(()=>{controller.capture();setBubbleOpen(true);},180);return()=>window.clearTimeout(timer);
 },[controller,signature,snapshot.allowAutoOpen,snapshot.selection.text,target,diagnosticPending]);
 useEffect(()=>{const key=`${filePath}:${target?.selectionId??''}`;if(lastTarget.current&&lastTarget.current!==key){generation.current++;abort.current?.abort();const retained=proposalForTarget(proposal,target?.selectionId,filePath,context.data?.threadId);if(!retained){controller?.discardEdits();setPhase('ready');setNotice('已切换到新选区，可以接着聊。');}setProposal(current=>proposalForTarget(current,target?.selectionId,filePath,context.data?.threadId));}lastTarget.current=key;},[filePath,target?.selectionId]);
 useEffect(()=>{if(snapshot.focusRequest!==previousFocus.current){previousFocus.current=snapshot.focusRequest;if(snapshot.target){setBubbleOpen(true);window.setTimeout(()=>bubbleInput.current?.focus(),0);}else{onLayoutChange({visible:true});window.setTimeout(()=>input.current?.focus(),0);}}},[snapshot.focusRequest]);
 useEffect(()=>{if(!busy)return;const start=Date.now();setElapsed(0);const timer=window.setInterval(()=>setElapsed(Math.floor((Date.now()-start)/1000)),1000);return()=>window.clearInterval(timer);},[busy]);
 useEffect(()=>{if(thread.current)thread.current.scrollTop=thread.current.scrollHeight;},[visibleTurns.length]);
 useEffect(()=>{if(!controller||(!layout.visible&&!bubbleOpen))return;const refresh=(event:Event)=>{if(!popup.current?.contains(event.target as Node))controller.refresh();};window.addEventListener('resize',refresh);window.addEventListener('scroll',refresh,true);return()=>{window.removeEventListener('resize',refresh);window.removeEventListener('scroll',refresh,true);};},[controller,layout.visible,bubbleOpen]);
 function useContext(value:ProjectAIContext){cache.setQueryData(contextKey,value);void cache.invalidateQueries({queryKey:threadsKey});}
 async function persist(messages:ProjectAIMessage[],threadId=context.data?.threadId) {
   const requestOwner=ownerRef.current;
   setPending(messages);setSaveError('');
   if(!threadId){setSaveError('讨论尚未连接到项目，请重试加载。');return;}
   if(messages.some(m=>m.content.length>24000||(m.raw?.length??0)>80000)){setSaveError('本轮内容超过保存限制，仍显示在此处；请复制保留，或放弃这轮未保存内容后继续。');return;}
   setSaving(true);
   try {const value=await projectAIContext.append(projectId,threadId,messages);if(ownerRef.current!==requestOwner)return;useContext(value);setPending([]);}catch(caught){if(ownerRef.current===requestOwner)setSaveError(caught instanceof Error?caught.message:'讨论保存失败，请重试');}finally{if(ownerRef.current===requestOwner)setSaving(false);}
 }
 useEffect(()=>{const handle=(event:Event)=>{const detail=(event as CustomEvent<AssistantOpenDetail>).detail??{};if(detail.projectId&&detail.projectId!==projectId)return;onLayoutChange({visible:true});setBubbleOpen(false);const before=controller?.snapshot();if(detail.source==='compile'||detail.source==='restore'){controller?.close();setProposal(null);setBubbleOpen(false);}if(detail.source==='restore'&&detail.restore){setRestoreRequest(detail.restore);setPrompt('');}if(detail.source!=='compile'&&detail.source!=='restore'&&before?.selection.text.trim()&&(before.target?.from!==before.selection.from||before.target?.to!==before.selection.to||before.target.text!==before.selection.text))controller?.capture();if(detail.prompt)setPrompt(detail.prompt);if(detail.ocrText){setImportedText(detail.ocrText);const current=controller?.snapshot().target;setProposal(null);if(current?.selectionId&&!readOnly&&controller&&context.data){const started=controller.beginEdits();if(started.ok){const prepared=controller.prepareSelectionReplacement(started.token,detail.ocrText);if(prepared.ok)setProposal({selectionId:current.selectionId,filePath,threadId:context.data.threadId});else setError(prepared.message);}else setError(started.message);}setPrompt('');setPhase(current&&!readOnly?'preview':'answered');setNotice(current?'图片识别结果已进入修改预览，接受后写入当前选区。':'图片识别结果已进入讨论。选中文字后可继续要求修改。');if(!blocked){setImportedText(null);void persist([{id:crypto.randomUUID(),role:'user',content:'识别上传图片中的公式与文字',filePath},{id:crypto.randomUUID(),role:'assistant',content:detail.ocrText,filePath}]);}}window.setTimeout(()=>input.current?.focus(),0);};window.addEventListener(ASSISTANT_OPEN_EVENT,handle);return()=>window.removeEventListener(ASSISTANT_OPEN_EVENT,handle);});
 function stop(){generation.current++;abort.current?.abort();abort.current=null;setPhase('stopped');}
 function close(){onLayoutChange({visible:false});}
 function closeBubble(){setBubbleOpen(false);dismissed.current=signature;}
 async function switchThread(id?:string){if(blocked)return;const requestOwner=ownerRef.current;setSaving(true);setSaveError('');try{const value=id?await projectAIContext.select(projectId,id):await projectAIContext.newThread(projectId);if(ownerRef.current!==requestOwner)return;useContext(value);setPending([]);setProposal(null);controller?.discardEdits();controller?.clearUndo();setPrompt('');setPhase('ready');setNotice(id?'已切回这段对话，可以继续。':'已开启新对话。');}catch(caught){if(ownerRef.current===requestOwner)setSaveError(caught instanceof Error?caught.message:'切换对话失败');}finally{if(ownerRef.current===requestOwner)setSaving(false);}}
 async function send(instruction=prompt) {
   const requestTarget=assistantRequestTarget(controller?.snapshot().target,readOnly),current=requestTarget.target;if(blocked||!instruction.trim()||!context.data)return;
   const changedTarget=!!controller?.snapshot().target?.conflict;if(changedTarget)controller?.discardEdits();
   if(!selectedModelReady){setError('请选择一个可用模型，或到设置中添加连接并设为默认');return;}
   setBubbleOpen(false);onLayoutChange({visible:true});
   const requestedModelId=selectedChoice!.profile.id;
   const requestedModelName=selectedChoice?.model;
   let editToken:string|undefined;if(current&&!readOnly&&controller){const started=controller.beginEdits();if(!started.ok){setError(started.message);return;}editToken=started.token;}
   const request=new AbortController();abort.current=request;const run=++generation.current;const threadId=context.data.threadId;
   const message:ProjectAIMessage={id:crypto.randomUUID(),role:'user',content:instruction.trim(),filePath,...(current?{selection:current.text,...(current.selectionId?{selectionId:current.selectionId}:{})}:{})};
   setPrompt('');setProposal(null);setFileActions([]);setError('');setNotice(changedTarget?'原文已变化，本轮先讨论。重新选择文字后可继续修改。':'');setReceived(0);setPhase('requesting');setActivity('');setImageWarning(false);
   const nearby=current?controller?.context():undefined;const doc=controller?.writingContext();
   try {
     const raw=await streamInline({feature:'chat',selection:requestTarget.selection,options:{mode:'inline-edit',message:instruction.trim(),projectId,threadId,modelProfileId:requestedModelId,...(requestedModelName?{modelName:requestedModelName}:{}),allowProjectFiles:'true',readOnly:String(requestTarget.readOnly),filePath,...(current?.selectionId?{selectionId:current.selectionId}:{}),...(nearby?{before:nearby.before,after:nearby.after}:{}),...(doc?{currentSection:doc.currentSection,selectionStartLine:String(doc.selectionStartLine),selectionEndLine:String(doc.selectionEndLine),documentOutline:doc.documentOutline,documentWindowStartLine:String(doc.documentWindowStartLine),documentWindowEndLine:String(doc.documentWindowEndLine),currentDocumentTruncated:String(doc.documentTruncated),currentDocument:doc.document}:{})}},request.signal,count=>{if(generation.current===run){setPhase('generating');setReceived(count);}},event=>{if(generation.current!==run)return;if(event.status)setActivity(event.status);if(event.warning?.code==='image_unsupported')setImageWarning(true);if(event.received)setReceived(event.received);if(event.file&&!event.status)setActivity('正在读取项目文件…');});
     if(generation.current!==run)return;const parsed=parseInlineProposal(raw,filePath);setFileActions('actions' in parsed?parseProjectActions(parsed.actions):[]);fileActionSelection.current=current?.selectionId;
     let preparationError='';let hasPlan=false;
     if(current?.selectionId&&!readOnly&&controller&&editToken){
       const prepared=parsed.edits.length?controller.prepareEdits(editToken,parsed.edits):parsed.replacement!==null?controller.prepareSelectionReplacement(editToken,parsed.replacement):null;
       if(prepared){if(prepared.ok){hasPlan=true;setProposal({selectionId:current.selectionId,filePath,threadId});}else preparationError=prepared.message;}
       else controller.discardEdits();
     }
     setError(preparationError);setPhase(hasPlan?'preview':'answered');
     const completed:ProjectAIMessage[]=[message,{id:crypto.randomUUID(),role:'assistant',content:parsed.reply,raw,filePath,...(current?.selectionId?{selectionId:current.selectionId}:{})}];
     if(preparationError)completed.push({id:crypto.randomUUID(),role:'action',content:`修改建议未通过定位校验，文档没有改动：${preparationError}`,filePath,...(current?.selectionId?{selectionId:current.selectionId}:{})});
     await persist(completed,threadId);
   }catch(caught){if(generation.current!==run)return;if(caught instanceof Error&&caught.name==='AbortError')setPhase('stopped');else{setError(caught instanceof Error?caught.message:'生成失败，请重试');setPrompt(instruction);setPhase('error');}}finally{if(generation.current===run)abort.current=null;}
 }
 function action(kind:'accept'|'reject'|'undo',editId?:string) {
   if(blocked||readOnly||!controller)return;
   if(kind!=='undo'&&!proposalForTarget(proposal,controller.snapshot().target?.selectionId,filePath,context.data?.threadId))return;
   const edit=editId?controller.snapshot().plannedEdits.find(item=>item.id===editId):undefined;
   let content='';let result;
   if(kind==='accept'){
     if(!edit||!editable)return;result=controller.applyPreparedEdit(edit.id);
     content=`用户只应用了建议 ${edit.id}（${filePath}，原范围 ${edit.from}–${edit.to}）。原文：${edit.before.slice(0,800)}
应用后的文字：${edit.after.slice(0,800)}
其他候选尚未决定，不能视作已应用。`;
   }else if(kind==='reject'){
     if(!edit)return;result=controller.rejectPreparedEdit(edit.id);
     content=`用户只拒绝了建议 ${edit.id}（${filePath}，范围 ${edit.from}–${edit.to}）。原文保持：${edit.before.slice(0,800)}
拒绝的候选：${edit.after.slice(0,800)}
其他候选尚未决定。`;
   }else{
     result=controller.undo();content=`用户撤销了 ${filePath} 中最近应用的一次 AI 修改；其他待决定的候选保持预览状态。`;
   }
   if(!result.ok){setError(result.message);return;}
   controller.setReviewBlocked(true);
   const remaining=controller.snapshot().plannedEdits.length;
   if(!remaining)setProposal(null);
   setPhase(remaining?'preview':kind==='accept'?'applied':kind==='reject'?'rejected':'undone');setError('');
   setNotice(kind==='accept'?'已应用本段。':kind==='reject'?'已拒绝本段，原文保留。':'已撤销最近应用的修改。');
   void persist([{id:crypto.randomUUID(),role:'action',content,filePath,...(target?.selectionId?{selectionId:target.selectionId}:{})}]);
 }
 function editCandidate(editId:string,text:string){const result=controller?.updatePreparedEdit(editId,text);if(result&&!result.ok)setError(result.message);else setError('');}

 useEffect(()=>{
   if(!restoreRequest||restoreRequest.filePath!==filePath||!controller||!context.data||busy)return;
   if(readOnly){setError('当前项目为只读，不能恢复版本。');setRestoreRequest(null);return;}
   try{if(!controller.matchesDocument(restoreRequest.before))throw new Error('当前文件已变化，请重新生成版本恢复预览。');const edits=buildVersionReview(restoreRequest.before,restoreRequest.after);if(!edits.length){setNotice('当前文件与这个版本相同。');setRestoreRequest(null);return;}const first=edits[0]!;const captured=controller.captureRange(first.from,first.to);if(!captured.ok)throw new Error(captured.message);const started=controller.beginEdits();if(!started.ok)throw new Error(started.message);const prepared=controller.prepareEdits(started.token,edits);if(!prepared.ok)throw new Error(prepared.message);const selected=controller.snapshot().target;if(!selected?.selectionId)throw new Error('版本定位失败');setProposal({selectionId:selected.selectionId,filePath,threadId:context.data.threadId});setPhase('preview');setNotice('版本恢复已进入预览，接受后才写入当前文件。');setError('');}
   catch(failure){setError(failure instanceof Error?failure.message:'无法生成恢复预览');}finally{setRestoreRequest(null);}
 },[restoreRequest,filePath,controller,context.data?.threadId,readOnly,busy]);
 function beginPointer(event:ReactPointerEvent<HTMLElement>,kind:'drag'|'resize'){
   if(effectiveMode!=='float'||(kind==='drag'&&(event.target as HTMLElement).closest('button,select,a,input,[role="combobox"]')))return;
   event.preventDefault();pointerCleanup.current?.();const x=event.clientX,y=event.clientY,initial=layout.floating;
   const move=(next:PointerEvent)=>onLayoutChange({floating:kind==='drag'?{x:initial.x+next.clientX-x,y:initial.y+next.clientY-y}:{width:initial.width+next.clientX-x,height:initial.height+next.clientY-y}});
   const end=()=>{window.removeEventListener('pointermove',move);window.removeEventListener('pointerup',end);window.removeEventListener('pointercancel',end);window.removeEventListener('blur',end);pointerCleanup.current=null;};pointerCleanup.current=end;window.addEventListener('pointermove',move);window.addEventListener('pointerup',end);window.addEventListener('pointercancel',end);window.addEventListener('blur',end);
 }
 const modelPicker=(<Select value={selectedModelId} disabled={busy||saving} onValueChange={value=>setModelChoice({scope:ownerRef.current,id:value})}><SelectTrigger aria-label="本次提问使用的模型" className="h-7 min-w-0 max-w-64 flex-1 px-2 text-xs text-foreground" title={selectedChoice?`${selectedChoice.model} — ${selectedChoice.profile.name}`:'请选择模型'}><SelectValue placeholder="请选择模型">{selectedChoice?`${selectedChoice.model} — ${selectedChoice.profile.name}`:'请选择模型'}</SelectValue></SelectTrigger><SelectContent side="top" align="start" className="max-h-80" onEscapeKeyDown={event=>event.stopPropagation()}>{profiles.data?.profiles.map(profile=><SelectGroup key={profile.id}><SelectLabel className="text-xs text-muted-foreground">{profile.name}{profiles.data?.defaultProfileId===profile.id?' · 默认':''}</SelectLabel>{profileModelNames(profile).map(model=><SelectItem key={model} value={JSON.stringify([profile.id,model])} disabled={!isModelProfileReady(profile)} textValue={model} className="text-xs">{model}</SelectItem>)}</SelectGroup>)}</SelectContent></Select>);
 const panelHost=effectiveMode==='float'?document.body:effectiveMode==='right'?rightHost:bottomHost;
 const panelStyle=effectiveMode==='float'?{left:layout.floating.x,top:layout.floating.y,width:layout.floating.width,height:layout.floating.height}:{width:'100%',height:'100%'};
 const panel=layout.visible&&panelHost?createPortal(<div ref={popup} role={effectiveMode==='float'?'dialog':'region'} aria-label="Prims AI 助手" data-testid="inline-assistant" className={`${effectiveMode==='float'?'fixed z-50 rounded-xl border shadow-2xl':'relative'} flex min-h-0 flex-col overflow-hidden bg-background text-foreground`} style={panelStyle} onKeyDown={event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close();}}}>
  <div className={`flex shrink-0 items-center gap-2 border-b bg-muted/25 px-3 py-2 ${effectiveMode==='float'?'cursor-move':''}`} onPointerDown={event=>beginPointer(event,'drag')}><Sparkles className="h-4 w-4 text-foreground"/><strong className="min-w-0 flex-1 truncate text-sm">Prims 助手</strong><Button size="sm" variant="outline" className="h-7 px-2 text-xs" disabled={blocked} onClick={()=>{void switchThread();}}>新对话</Button><div role="group" aria-label="助手位置" className="flex shrink-0 items-center rounded-md border bg-muted/30 p-0.5">{([{mode:'right',label:'停靠到右侧',Icon:PanelRight},{mode:'bottom',label:'停靠到编辑器底部',Icon:PanelBottom},{mode:'float',label:'自由浮动',Icon:Move}] as const).map(({mode,label,Icon})=><button key={mode} type="button" title={label} aria-label={label} aria-pressed={layout.mode===mode} className={`rounded p-1.5 ${layout.mode===mode?'bg-background text-foreground shadow-sm':'text-muted-foreground hover:bg-accent hover:text-foreground'}`} onClick={()=>onLayoutChange({mode})}><Icon className="h-3.5 w-3.5" aria-hidden="true"/></button>)}</div><button type="button" className="rounded p-1 hover:bg-accent" onClick={close} aria-label="关闭助手"><X className="h-4 w-4"/></button></div>
  <><nav aria-label="项目对话" className="max-h-24 shrink-0 overflow-y-auto border-b p-1.5"><ul className="space-y-1">{displayedThreads.map(item=><li key={item.id}><button type="button" aria-current={item.id===context.data?.threadId?'true':undefined} disabled={blocked} title={item.title||'新对话'} className={`block w-full truncate rounded-md px-2 py-1.5 text-left text-xs disabled:opacity-50 ${item.id===context.data?.threadId?'bg-primary/10 font-medium text-primary':'text-muted-foreground hover:bg-accent hover:text-foreground'}`} onClick={()=>{if(item.id!==context.data?.threadId)void switchThread(item.id);}}>{item.title||'新对话'}</button></li>)}</ul></nav>
   {(phase!=='ready'||diagnosticPending||plannedEdits.some(edit=>edit.conflict))&&<><div role="status" aria-live="polite" className="flex shrink-0 items-center gap-2 px-3 py-2 text-xs">{busy&&<Loader2 className="h-3.5 w-3.5 animate-spin text-blue-500"/>}<span className="flex-1">{diagnosticPending?'正在定位编译问题…':plannedEdits.some(edit=>edit.conflict)?'部分原文已变化，请逐段查看':busy&&activity?activity:labels[phase]}{busy?` · ${elapsed}s${received?` · ${received} 字符`:''}`:''}</span></div></>}
   <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 pb-3">
    <div className="rounded border-l-2 border-blue-400 bg-muted/25 px-2 py-1.5 text-[11px]"><div className="flex items-center gap-2"><strong className="min-w-0 flex-1 truncate">{target?`${filePath} · 定位线索`:`${filePath||'项目'} · 暂无选区，仅讨论`}</strong>{target&&<button type="button" disabled={busy} className="underline" onClick={()=>{dismissed.current=signature;controller?.discardEdits();controller?.close();setProposal(null);}}>只讨论</button>}</div>{target&&<p className="line-clamp-2 whitespace-pre-wrap text-muted-foreground">{target.text.slice(0,240)}</p>}{readOnly&&<p>此文件为只读，可以提问。</p>}</div>
    {notice&&<p className="rounded bg-blue-50 p-2 text-[11px] text-blue-900 dark:bg-blue-950 dark:text-blue-200">{notice}</p>}
    {context.isError&&<p role="alert" className="text-xs text-destructive">项目讨论加载失败。<button type="button" className="underline" onClick={()=>{void context.refetch();}}>重试加载</button></p>}
    {visibleTurns.length?<div ref={thread} className="space-y-3" aria-label="项目对话记录">{context.data?.hasEarlier&&<p className="text-[10px] text-muted-foreground">显示最近的讨论。</p>}{visibleTurns.map(turn=><div key={turn.id} className={`whitespace-pre-wrap break-words rounded-md px-3 py-2 text-xs leading-relaxed ${turn.role==='user'?'ml-5 bg-muted':'mr-2 border'}`}><div className="mb-1 text-[10px] text-muted-foreground">{turn.role==='user'?'你':'AI'}{turn.filePath?` · ${turn.filePath}`:''}</div>{turn.content}</div>)}</div>:null}
    {importedText!==null&&<div className="space-y-2 rounded border p-2 text-xs"><p>图片识别结果（尚未加入讨论）</p><pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words">{importedText}</pre><Button size="sm" variant="outline" disabled={blocked} onClick={()=>{const text=importedText;setImportedText(null);void persist([{id:crypto.randomUUID(),role:'user',content:'识别上传图片中的公式与文字',filePath},{id:crypto.randomUUID(),role:'assistant',content:text,filePath}]);}}>加入当前讨论</Button></div>}
    <div ref={setActionsHost}/>
    {plannedEdits.length>0&&<section className="space-y-3" aria-label="修改建议"><div><strong className="text-xs">建议修改 {plannedEdits.length} 处</strong><p className="mt-1 text-[11px] text-muted-foreground">仅修改当前文件。预览修改，应用后保存。</p></div>{plannedEdits.map((edit,index)=><div key={edit.id} className="space-y-2 rounded-lg border p-2" data-edit-id={edit.id}><div className="text-xs font-medium">第 {index+1} 处</div><p className="line-clamp-2 whitespace-pre-wrap text-xs text-muted-foreground">{edit.before.slice(0,160)}</p><button type="button" className="text-[11px] text-primary underline" onClick={()=>{const result=controller?.revealPreparedEdit(edit.id);if(result&&!result.ok)setError(result.message);}}>定位正文</button><details><summary className="cursor-pointer text-xs text-muted-foreground">查看差异与编辑建议</summary><div className="mt-2"><Preview before={edit.before} after={edit.after}/></div><textarea aria-label={`第 ${index+1} 处建议文本`} rows={4} maxLength={24000} className="mt-2 w-full resize-y rounded border bg-background p-2 font-mono text-xs" value={edit.after} disabled={blocked} onChange={event=>editCandidate(edit.id,event.target.value)}/></details>{edit.conflict&&<p role="alert" className="text-xs text-destructive">原文已变化，无法安全定位。请重新提出修改。</p>}<div className="flex gap-2"><Button size="sm" disabled={blocked||!editable||edit.conflict||edit.before===edit.after} onClick={()=>action('accept',edit.id)}><Check className="mr-1 h-3 w-3"/>应用本段</Button><Button size="sm" variant="outline" disabled={blocked} onClick={()=>action('reject',edit.id)}>拒绝本段</Button></div></div>)}</section>}
    {imageWarning&&!hideImageWarning&&<div role="status" className="rounded bg-amber-50 p-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-200">当前模型不支持图片，已改用文本继续。可换支持图片的模型。<button type="button" className="ml-2 underline" onClick={()=>{setHideImageWarning(true);setImageWarning(false);if(userId)try{localStorage.setItem(`scribe-ai-image-warning:${userId}`,'hidden');}catch{/* Preference is optional. */}}}>不再提示</button></div>}
    {error&&<p role="alert" className="rounded bg-destructive/10 p-2 text-xs text-destructive">{error}</p>}
    {saveError&&<div role="alert" className="rounded bg-amber-50 p-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-200">{saveError}{pending.length>0&&<button type="button" className="ml-2 underline" disabled={saving} onClick={()=>{void persist(pending);}}>重试保存本轮</button>}{pending.length>0&&!saving&&<button type="button" className="ml-2 underline" onClick={()=>{setPending([]);controller?.discardEdits();setProposal(null);setSaveError('');setNotice('已放弃本轮未保存的内容，已保存的讨论保持不变。');}}>放弃本轮未保存内容</button>}</div>}
    {!profiles.isPending&&!selectedModelReady&&<p className="text-xs text-muted-foreground">选择已保存的模型，或到<Link to="/settings" className="text-primary underline">设置</Link>添加模型。</p>}
   </div>
   <div className="shrink-0 space-y-2 border-t p-3"><textarea ref={input} aria-label="向项目 AI 助手提问" placeholder={target?'以选区为线索，描述想怎么修改…':'讨论项目、梳理思路或询问文件内容…'} rows={effectiveMode==='bottom'?2:3} maxLength={16000} value={prompt} onChange={e=>setPrompt(e.target.value)} className="block w-full resize-none rounded-lg border bg-background p-2 text-sm outline-none focus:ring-2 focus:ring-blue-400/50" onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.nativeEvent.isComposing){e.preventDefault();if(!blocked)void send();}}}/><div className="flex flex-wrap items-center gap-2">{busy?<Button size="sm" variant="outline" onClick={stop}><Square className="mr-1 h-3 w-3"/>停止</Button>:<Button size="sm" disabled={blocked||!prompt.trim()||!selectedModelReady} onClick={()=>{void send();}}><Send className="mr-1 h-3 w-3"/>{saving?'正在保存':'发送'}</Button>}{modelPicker}{snapshot.canUndo&&<Button size="sm" variant="outline" disabled={blocked||readOnly} onClick={()=>action('undo')}><RotateCcw className="mr-1 h-3 w-3"/>撤销修改</Button>}<span className="ml-auto text-[10px] text-muted-foreground">Shift+Enter 换行</span></div></div>
  </>
  {effectiveMode==='float'&&<button type="button" aria-label="调整浮窗大小" title="拖动调整大小" className="absolute bottom-0 right-0 h-4 w-4 cursor-nwse-resize text-xs text-muted-foreground" onPointerDown={event=>beginPointer(event,'resize')} onKeyDown={event=>{if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)){event.preventDefault();onLayoutChange({floating:{width:layout.floating.width+(event.key==='ArrowRight'?20:event.key==='ArrowLeft'?-20:0),height:layout.floating.height+(event.key==='ArrowDown'?20:event.key==='ArrowUp'?-20:0)}});}}}>↘</button>}
 </div>,panelHost):null;
 const bubbleWidth=Math.min(360,viewport.width-16),bubbleLeft=Math.max(8,Math.min(snapshot.position?.left??24,viewport.width-bubbleWidth-8)),bubbleTop=Math.max(8,Math.min((snapshot.position?.top??60)+8,viewport.height-180));
 const bubble=bubbleOpen&&target?createPortal(<div role="dialog" aria-label="选区提问" data-testid="selection-prompt" className="fixed z-50 rounded-lg border bg-background p-2 text-foreground shadow-xl" style={{left:bubbleLeft,top:bubbleTop,width:bubbleWidth}} onKeyDown={event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeBubble();}}}><div className="mb-1 flex items-center justify-end text-[11px] text-muted-foreground"><button type="button" aria-label="关闭选区提问" className="rounded p-1 hover:bg-accent" onClick={closeBubble}><X className="h-3.5 w-3.5"/></button></div><textarea ref={bubbleInput} rows={2} maxLength={4000} aria-label="选区修改要求" placeholder="想怎样调整这段内容？" value={prompt} onChange={event=>setPrompt(event.target.value)} className="w-full resize-none rounded border bg-background p-2 text-sm outline-none focus:ring-1 focus:ring-ring" onKeyDown={event=>{if(event.key==='Enter'&&!event.shiftKey&&!event.nativeEvent.isComposing){event.preventDefault();event.stopPropagation();if(!blocked)void send();}}}/><div className="mt-1 flex items-center justify-end gap-2">{!selectedModelReady&&<button type="button" className="mr-auto text-[11px] underline" onClick={()=>onLayoutChange({visible:true})}>先在助手选择模型</button>}<Button size="sm" disabled={blocked||!selectedModelReady||!prompt.trim()} onClick={()=>{void send();}}><Send className="mr-1 h-3 w-3"/>发送</Button></div></div>,document.body):null;
 return <>{panel}{bubble}<ProjectActionsPreview projectId={projectId} actions={fileActions} readOnly={readOnly} disabled={blocked} beforeApply={beforeProjectAction} portalTarget={actionsHost} onDecided={decision=>{const action=decision.action;void persist([{id:crypto.randomUUID(),role:'action',content:`用户${decision.status==='applied'?'确认执行':'拒绝'}文件操作：${action.type}，${action.path}${action.newPath?` → ${action.newPath}`:''}。其他操作仍需分别确认。`,filePath:action.path,...(fileActionSelection.current?{selectionId:fileActionSelection.current}:{})}]);}} onApplied={operation=>{setNotice(operation.type==='create_directory'?'目录已创建。':operation.type==='move_file'?'文件已移动。':'文件已创建。');}}/></>;
}

import { useEffect, useRef, useState } from 'react';
import { assistantCloseKey, assistantReturnKey, canContinueWithoutParent, assistantTabChannel, assistantTabSession, assistantTabUrl, editorTabUrl, validTabMessage, validTabState, validEditorContext, restoreInEditor, handoffDisposition, type EditorTabContext, type AssistantTabKind, type AssistantTabState } from '../lib/assistant-tab';
interface Options { userId:string|undefined;projectId:string;fileId?:string;standalone:boolean;busy:boolean;state:AssistantTabState;receive:(state:AssistantTabState)=>Promise<void>;onAway:(away:boolean)=>void;onRefresh:()=>void;editor:Omit<EditorTabContext,'revision'>;transitionOut:()=>Promise<void>;onNavigate:(path:string)=>void;onDock:(mode:'right'|'bottom'|'float')=>void }
/** A user-click handoff, not a second conversation or a second model request. */
export function useAssistantTab(options:Options) {
 const [editorContext,setEditorContext]=useState<EditorTabContext|null>(null);
 const editorRevision=useRef(0),lastEditorRevision=useRef(-1),transitioning=useRef(false),activated=useRef(false);
 const current=useRef(options);current.current=options;
 const [away,setAway]=useState(false),[moving,setMoving]=useState(false),[ready,setReady]=useState(!options.standalone),[error,setError]=useState('');
 const channel=useRef<BroadcastChannel|null>(null),session=useRef<string|null>(null),handed=useRef(false),grant=useRef(false),cached=useRef<AssistantTabState|null>(null),timer=useRef<ReturnType<typeof setTimeout>|null>(null),receiving=useRef(false);
 const childWindow=useRef<Window|null>(null),returning=useRef(false);
 const closeKey=()=>{const o=current.current;return o.userId&&session.current?assistantCloseKey(o.userId,o.projectId,session.current):null;};
 const marker=(value?:'parent'|'child'|'closed')=>{const o=current.current;if(!o.userId||!session.current)return null;try{const key=assistantTabChannel(o.userId,o.projectId,session.current)+':owner';if(value)localStorage.setItem(key,value);return localStorage.getItem(key);}catch{return 'unknown';}};
 const navigateBack=()=>{const o=current.current;if(!o.userId)return;try{const key=assistantReturnKey(o.userId,o.projectId),value=JSON.stringify(o.state);sessionStorage.setItem(key,value);if(sessionStorage.getItem(key)!==value)throw new Error('storage');}catch{grant.current=true;setReady(true);setMoving(false);setError('无法暂存草稿，请保留此标签页后重试。');return;}window.location.replace(editorTabUrl(window.location.origin,o.projectId,o.state.filePath));};
 const clearTimer=()=>{if(timer.current!==null)clearTimeout(timer.current);timer.current=null;};
 const send=(kind:AssistantTabKind,state?:AssistantTabState,mode?:'right'|'bottom'|'float')=>{const o=current.current;if(!o.userId||!session.current)return;channel.current?.postMessage({version:1,session:session.current,userId:o.userId,projectId:o.projectId,sender:o.standalone?'child':'parent',kind,...(state?{state}:{}),...(mode?{mode}:{})});};
 const sendEditor=()=>{const o=current.current;if(o.standalone||!o.userId||!session.current)return;channel.current?.postMessage({version:1,session:session.current,userId:o.userId,projectId:o.projectId,sender:'parent',kind:'editor-context',editor:{...o.editor,revision:++editorRevision.current}});};
 const markAway=(value:boolean)=>{handed.current=value;setAway(value);current.current.onAway(value);};
 async function acceptState(state:AssistantTabState){if(receiving.current)return;receiving.current=true;try{await current.current.receive(state);cached.current=state;return true;}catch{setError('对话迁移未完成，请重试。');return false;}finally{receiving.current=false;}}
 async function restoreParent(state:AssistantTabState,mode?:'right'|'bottom'|'float') {
  if(returning.current||!handed.current)return;returning.current=true;setMoving(true);
  const ok=await acceptState(restoreInEditor(state,current.current.editor));
  if(ok){if(mode)current.current.onDock(mode);marker('parent');markAway(false);setMoving(false);current.current.onRefresh();send('return-ack');const key=closeKey();if(key)try{localStorage.removeItem(key);}catch{/* Optional recovery copy. */}childWindow.current=null;}else setMoving(false);
  returning.current=false;
 }
 function attach(id:string) {
  channel.current?.close();session.current=id;activated.current=false;const o=current.current;if(!o.userId||typeof BroadcastChannel==='undefined')return;
  const instance=new BroadcastChannel(assistantTabChannel(o.userId,o.projectId,id));channel.current=instance;
  instance.onmessage=event=>{if(channel.current!==instance||!validTabMessage(event.data,o.userId!,o.projectId,id,o.standalone?'parent':'child'))return;const message=event.data;
   if(message.kind==='busy'){clearTimer();setMoving(false);setError('请等当前生成或保存结束后再移动。');return;}
   if(!o.standalone){
    if(message.kind==='hello'){
     if(current.current.busy){send('busy');return;}
     if(!cached.current||!handed.current)cached.current=current.current.state;
     setMoving(true);send('state',cached.current);return;
    }
    if(message.kind==='active'){const disposition=handoffDisposition(activated.current,handed.current);if(disposition==='grant'){send('grant');sendEditor();return;}if(disposition==='ignore'||transitioning.current)return;activated.current=true;transitioning.current=true;clearTimer();void current.current.transitionOut().finally(()=>{transitioning.current=false;if(channel.current!==instance)return;marker('child');markAway(true);setMoving(false);send('grant');sendEditor();});return;}
    if(message.kind==='context-request'&&handed.current){sendEditor();return;}
    if(message.kind==='open-file'&&handed.current&&typeof message.filePath==='string'&&message.filePath.length<=1024){current.current.onNavigate(message.filePath);return;}
    if(message.kind==='sync'&&handed.current&&validTabState(message.state)){cached.current=message.state;return;}
    if((message.kind==='return'||message.kind==='closed')&&handed.current){
     const state=validTabState(message.state)?message.state:cached.current;
     if(!state)return;void restoreParent(state,['right','bottom','float'].includes(message.mode??'')?message.mode:undefined);return;
    }
   }else{
    if(message.kind==='editor-context'&&grant.current&&validEditorContext(message.editor)&&message.editor.revision>lastEditorRevision.current){lastEditorRevision.current=message.editor.revision;setEditorContext(message.editor);return;}
    if(message.kind==='state'&&validTabState(message.state)&&!grant.current){setMoving(true);void acceptState(message.state).then(ok=>{if(channel.current!==instance)return;if(ok){clearTimer();send('active');}else setMoving(false);});return;}
    if(message.kind==='grant'){clearTimer();grant.current=true;setReady(true);setMoving(false);return;}
    if(message.kind==='request-return'){if(current.current.busy){send('busy');return;}returnToEditor();return;}
    if(message.kind==='return-ack'){clearTimer();grant.current=false;setReady(false);window.close();window.setTimeout(()=>{if(!window.closed)navigateBack();},100);return;}
    if(message.kind==='parent-closed'){clearTimer();setEditorContext(null);grant.current=true;setReady(true);setMoving(false);}
   }
  };
 }
 function openTab(){const o=current.current;if(o.busy||away||moving||!o.userId)return;if(typeof BroadcastChannel==='undefined'){setError('此浏览器不支持独立助手标签页。');return;}if(!validTabState(o.state)){setError('对话内容超过标签页迁移容量，请先处理待审建议后再移动；草稿仍保留在此处。');return;}setError('');cached.current=null;grant.current=false;const id=crypto.randomUUID();attach(id);marker('parent');setMoving(true);
  // Retain only a closure handle. Clear opener on the inert blank page before
  // navigating to the app; the child cannot access its original editor.
  const child=window.open('about:blank','_blank');
  if(child){child.opener=null;childWindow.current=child;child.location.replace(assistantTabUrl(window.location.origin,o.projectId,id,o.fileId));}
  else {setMoving(false);setError('请允许弹出窗口后重试。');channel.current?.close();channel.current=null;return;}
  clearTimer();timer.current=setTimeout(()=>{if(!handed.current){setMoving(false);setError('新标签页尚未连接，请允许弹出窗口后重试。');channel.current?.close();channel.current=null;}},7000);
 }
 function returnToEditor(mode?:'right'|'bottom'|'float'){const o=current.current;if(o.busy||moving)return;if(o.standalone&&!validTabState(o.state)){setError('对话内容超过标签页迁移容量，请先处理待审建议后再移动；草稿仍保留在此处。');return;}setError('');if(!o.standalone){setMoving(true);send('request-return');clearTimer();timer.current=setTimeout(()=>{setMoving(false);setError('未连接到助手标签页。关闭它后可恢复。');},3000);return;}setMoving(true);void o.transitionOut().finally(()=>{send('return',current.current.state,mode);clearTimer();timer.current=setTimeout(()=>{grant.current=false;setReady(false);navigateBack();},1500);});}
 useEffect(()=>{
  const o=current.current;setReady(!o.standalone);setMoving(false);setError('');markAway(false);grant.current=false;cached.current=null;setEditorContext(null);lastEditorRevision.current=-1;editorRevision.current=0;
  if(o.standalone&&o.userId){const id=assistantTabSession(window.location.search);if(id){attach(id);send('hello');const hello=setInterval(()=>{if(!grant.current&&!receiving.current)send('hello');},500);timer.current=setTimeout(()=>{if(!grant.current&&!receiving.current){const ownership=marker();if(canContinueWithoutParent(ownership)){grant.current=true;setReady(true);setMoving(false);}else setError('原编辑器尚未连接，可放回编辑器重试。');}},4000);const unload=()=>{if(grant.current){const key=closeKey();if(key)try{localStorage.setItem(key,JSON.stringify(current.current.state));}catch{/* Scoped channel still carries the state when storage is unavailable. */}send('closed',current.current.state);}};window.addEventListener('pagehide',unload);return()=>{clearInterval(hello);window.removeEventListener('pagehide',unload);clearTimer();channel.current?.close();channel.current=null;};}setReady(true);}
  if(!o.standalone&&o.userId){try{const key=assistantReturnKey(o.userId,o.projectId),saved=sessionStorage.getItem(key);if(saved){const state:unknown=JSON.parse(saved);if(validTabState(state))void acceptState(state).then(ok=>{if(ok)sessionStorage.removeItem(key);});else sessionStorage.removeItem(key);}}catch{/* Ignore unrelated or unavailable session storage. */}}
  const unload=()=>{marker('closed');send('parent-closed');};window.addEventListener('pagehide',unload);return()=>{window.removeEventListener('pagehide',unload);unload();clearTimer();channel.current?.close();channel.current=null;};
 },[options.userId,options.projectId,options.standalone]);
 useEffect(()=>{if(!options.standalone||!ready||moving)return;const id=setTimeout(()=>send('sync',current.current.state),180);return()=>clearTimeout(id);},[options.state,options.standalone,ready,moving]);
 useEffect(()=>{if(options.standalone||!away)return;sendEditor();},[away,options.standalone,options.editor.fileId,options.editor.filePath,options.editor.selection?.from,options.editor.selection?.to,options.editor.selection?.text]);
 useEffect(()=>{if(!options.standalone||!ready)return;const refresh=()=>send('context-request');window.addEventListener('focus',refresh);document.addEventListener('visibilitychange',refresh);return()=>{window.removeEventListener('focus',refresh);document.removeEventListener('visibilitychange',refresh);};},[options.standalone,ready]);
 // Window.closed is a closure signal, unlike a heartbeat timeout on a throttled
 // background tab. A synchronous pagehide copy preserves the last keystroke.
 useEffect(()=>{if(options.standalone)return;const inspect=()=>{if(!handed.current||returning.current||!childWindow.current?.closed)return;let state=cached.current;const key=closeKey();if(key)try{const saved=localStorage.getItem(key);if(saved){const parsed:unknown=JSON.parse(saved);if(validTabState(parsed))state=parsed;}}catch{/* Use the most recent bridge snapshot. */}if(state)void restoreParent(state);};const interval=setInterval(inspect,300);window.addEventListener('focus',inspect);window.addEventListener('storage',inspect);return()=>{clearInterval(interval);window.removeEventListener('focus',inspect);window.removeEventListener('storage',inspect);};},[options.standalone]);
 const openEditorFile=(filePath:string)=>{const o=current.current;if(!o.standalone||!o.userId||!session.current||!editorContext)return;channel.current?.postMessage({version:1,session:session.current,userId:o.userId,projectId:o.projectId,sender:'child',kind:'open-file',filePath});};
 return {openEditorFile,editorContext,away,moving,ready,error,locked:away||moving||!ready,openTab,returnToEditor};
}

import { useEffect, useRef, useState } from 'react';
import { assistantReturnKey, canContinueWithoutParent, assistantTabChannel, assistantTabSession, assistantTabUrl, editorTabUrl, validTabMessage, validTabState, type AssistantTabKind, type AssistantTabState } from '../lib/assistant-tab';
interface Options { userId:string|undefined;projectId:string;fileId?:string;standalone:boolean;busy:boolean;state:AssistantTabState;receive:(state:AssistantTabState)=>Promise<void>;onAway:(away:boolean)=>void;onRefresh:()=>void }
/** A user-click handoff, not a second conversation or a second model request. */
export function useAssistantTab(options:Options) {
 const current=useRef(options);current.current=options;
 const [away,setAway]=useState(false),[moving,setMoving]=useState(false),[ready,setReady]=useState(!options.standalone),[error,setError]=useState('');
 const channel=useRef<BroadcastChannel|null>(null),session=useRef<string|null>(null),handed=useRef(false),grant=useRef(false),cached=useRef<AssistantTabState|null>(null),timer=useRef<ReturnType<typeof setTimeout>|null>(null),receiving=useRef(false);
 const marker=(value?:'parent'|'child'|'closed')=>{const o=current.current;if(!o.userId||!session.current)return null;try{const key=assistantTabChannel(o.userId,o.projectId,session.current)+':owner';if(value)localStorage.setItem(key,value);return localStorage.getItem(key);}catch{return 'unknown';}};
 const navigateBack=()=>{const o=current.current;if(!o.userId)return;try{const key=assistantReturnKey(o.userId,o.projectId),value=JSON.stringify(o.state);sessionStorage.setItem(key,value);if(sessionStorage.getItem(key)!==value)throw new Error('storage');}catch{grant.current=true;setReady(true);setMoving(false);setError('无法暂存草稿，请保留此标签页后重试。');return;}window.location.replace(editorTabUrl(window.location.origin,o.projectId,o.state.filePath));};
 const clearTimer=()=>{if(timer.current!==null)clearTimeout(timer.current);timer.current=null;};
 const send=(kind:AssistantTabKind,state?:AssistantTabState)=>{const o=current.current;if(!o.userId||!session.current)return;channel.current?.postMessage({version:1,session:session.current,userId:o.userId,projectId:o.projectId,sender:o.standalone?'child':'parent',kind,...(state?{state}:{})});};
 const markAway=(value:boolean)=>{handed.current=value;setAway(value);current.current.onAway(value);};
 async function acceptState(state:AssistantTabState){if(receiving.current)return;receiving.current=true;try{await current.current.receive(state);cached.current=state;return true;}catch{setError('对话迁移未完成，请重试。');return false;}finally{receiving.current=false;}}
 function attach(id:string) {
  channel.current?.close();session.current=id;const o=current.current;if(!o.userId||typeof BroadcastChannel==='undefined')return;
  const instance=new BroadcastChannel(assistantTabChannel(o.userId,o.projectId,id));channel.current=instance;
  instance.onmessage=event=>{if(channel.current!==instance||!validTabMessage(event.data,o.userId!,o.projectId,id,o.standalone?'parent':'child'))return;const message=event.data;
   if(message.kind==='busy'){clearTimer();setMoving(false);setError('请等当前生成或保存结束后再移动。');return;}
   if(!o.standalone){
    if(message.kind==='hello'){
     if(current.current.busy){send('busy');return;}
     if(!cached.current||!handed.current)cached.current=current.current.state;
     setMoving(true);send('state',cached.current);return;
    }
    if(message.kind==='active'){clearTimer();marker('child');markAway(true);setMoving(false);send('grant');return;}
    if(message.kind==='sync'&&handed.current&&validTabState(message.state)){cached.current=message.state;return;}
    if((message.kind==='return'||message.kind==='closed')&&handed.current){
     const state=validTabState(message.state)?message.state:cached.current;
     if(!state)return;setMoving(true);void acceptState(state).then(ok=>{if(channel.current!==instance)return;if(ok){markAway(false);setMoving(false);current.current.onRefresh();send('return-ack');}else setMoving(false);});return;
    }
   }else{
    if(message.kind==='state'&&validTabState(message.state)&&!grant.current){setMoving(true);void acceptState(message.state).then(ok=>{if(channel.current!==instance)return;if(ok){clearTimer();send('active');}else setMoving(false);});return;}
    if(message.kind==='grant'){clearTimer();grant.current=true;setReady(true);setMoving(false);return;}
    if(message.kind==='request-return'){if(current.current.busy){send('busy');return;}returnToEditor();return;}
    if(message.kind==='return-ack'){clearTimer();grant.current=false;setReady(false);window.close();window.setTimeout(()=>{if(!window.closed)navigateBack();},100);return;}
    if(message.kind==='parent-closed'){clearTimer();grant.current=true;setReady(true);setMoving(false);}
   }
  };
 }
 function openTab(){const o=current.current;if(o.busy||away||moving||!o.userId)return;if(typeof BroadcastChannel==='undefined'){setError('此浏览器不支持独立助手标签页。');return;}setError('');cached.current=null;grant.current=false;const id=crypto.randomUUID();attach(id);marker('parent');setMoving(true);
  // noopener deliberately gives no window.opener reference. Readiness is proven
  // by the scoped channel, never by trusting window.open's return value.
  window.open(assistantTabUrl(window.location.origin,o.projectId,id,o.fileId),'_blank','noopener');
  clearTimer();timer.current=setTimeout(()=>{if(!handed.current){setMoving(false);setError('新标签页尚未连接，请允许弹出窗口后重试。');channel.current?.close();channel.current=null;}},7000);
 }
 function returnToEditor(){const o=current.current;if(o.busy||moving)return;setError('');if(!o.standalone){setMoving(true);send('request-return');clearTimer();timer.current=setTimeout(()=>{setMoving(false);setError('未连接到助手标签页。关闭它后可恢复。');},3000);return;}setMoving(true);send('return',o.state);clearTimer();timer.current=setTimeout(()=>{grant.current=false;setReady(false);navigateBack();},1500);}
 useEffect(()=>{
  const o=current.current;setReady(!o.standalone);setMoving(false);setError('');markAway(false);grant.current=false;cached.current=null;
  if(o.standalone&&o.userId){const id=assistantTabSession(window.location.search);if(id){attach(id);send('hello');const hello=setInterval(()=>{if(!grant.current&&!receiving.current)send('hello');},500);timer.current=setTimeout(()=>{if(!grant.current&&!receiving.current){const ownership=marker();if(canContinueWithoutParent(ownership)){grant.current=true;setReady(true);setMoving(false);}else setError('原编辑器尚未连接，可放回编辑器重试。');}},4000);const unload=()=>{if(grant.current)send('closed',current.current.state);};window.addEventListener('pagehide',unload);return()=>{clearInterval(hello);window.removeEventListener('pagehide',unload);clearTimer();channel.current?.close();channel.current=null;};}setReady(true);}
  if(!o.standalone&&o.userId){try{const key=assistantReturnKey(o.userId,o.projectId),saved=sessionStorage.getItem(key);if(saved){const state:unknown=JSON.parse(saved);if(validTabState(state))void acceptState(state).then(ok=>{if(ok)sessionStorage.removeItem(key);});else sessionStorage.removeItem(key);}}catch{/* Ignore unrelated or unavailable session storage. */}}
  const unload=()=>{marker('closed');send('parent-closed');};window.addEventListener('pagehide',unload);return()=>{window.removeEventListener('pagehide',unload);unload();clearTimer();channel.current?.close();channel.current=null;};
 },[options.userId,options.projectId,options.standalone]);
 useEffect(()=>{if(!options.standalone||!ready||moving)return;const id=setTimeout(()=>send('sync',current.current.state),180);return()=>clearTimeout(id);},[options.state,options.standalone,ready,moving]);
 return {away,moving,ready,error,locked:away||moving||!ready,openTab,returnToEditor};
}

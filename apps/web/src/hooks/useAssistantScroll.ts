import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { AssistantScrollPolicy } from '../lib/assistant-conversation';

/** Own the actual overflow viewport, including async Markdown/proposal height changes. */
export function useAssistantScroll(scopeKey:string,visible:boolean,positionKey:string){
 const [viewport,setViewport]=useState<HTMLDivElement|null>(null),[content,setContent]=useState<HTMLDivElement|null>(null);
 const policy=useRef(new AssistantScrollPolicy()),frame=useRef<number|null>(null),node=useRef(viewport);node.current=viewport;
 const schedule=useCallback((force=false)=>{
  if(force)policy.current.reset();
  if(frame.current!==null)cancelAnimationFrame(frame.current);
  frame.current=requestAnimationFrame(()=>{frame.current=null;const current=node.current;if(!current||!policy.current.shouldFollow)return;current.scrollTop=current.scrollHeight;policy.current.scrolledToEnd(current);});
 },[]);
 const forceToEnd=useCallback(()=>schedule(true),[schedule]);
 useLayoutEffect(()=>{if(visible&&viewport)schedule(true);},[scopeKey,visible,positionKey,viewport,schedule]);
 useLayoutEffect(()=>{
  if(!viewport||!visible)return;
  const scroll=()=>policy.current.onScroll(viewport);
  const wheel=(event:WheelEvent)=>{if(!event.ctrlKey&&event.deltaY<0&&viewport.scrollHeight>viewport.clientHeight)policy.current.pause();};
  const key=(event:KeyboardEvent)=>{if(!(event.target as HTMLElement).closest('textarea,input,select,[contenteditable="true"]')&&['ArrowUp','PageUp','Home'].includes(event.key)&&viewport.scrollHeight>viewport.clientHeight)policy.current.pause();};
  viewport.addEventListener('scroll',scroll,{passive:true});viewport.addEventListener('wheel',wheel,{passive:true});viewport.addEventListener('keydown',key);
  const resize=()=>schedule();const observer=typeof ResizeObserver==='undefined'?null:new ResizeObserver(resize);
  observer?.observe(viewport);if(content)observer?.observe(content);
  // WebViews without ResizeObserver still follow committed DOM updates.
  const mutations=!observer&&content?new MutationObserver(resize):null;mutations?.observe(content!,{childList:true,subtree:true,characterData:true});
  window.addEventListener('resize',resize);
  return()=>{viewport.removeEventListener('scroll',scroll);viewport.removeEventListener('wheel',wheel);viewport.removeEventListener('keydown',key);observer?.disconnect();mutations?.disconnect();window.removeEventListener('resize',resize);if(frame.current!==null){cancelAnimationFrame(frame.current);frame.current=null;}};
 },[viewport,content,visible,schedule]);
 return {viewportRef:setViewport,contentRef:setContent,forceToEnd};
}

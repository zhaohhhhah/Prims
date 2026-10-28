import { useCallback, useEffect, useLayoutEffect, useRef, type RefObject } from 'react';

type Box={left:number;top:number;width:number;height:number};
const box=(node:HTMLElement):Box=>{const r=node.getBoundingClientRect();return {left:r.left,top:r.top,width:r.width,height:r.height};};
const compact=(r:Box):Box=>({left:Math.max(8,Math.min(window.innerWidth-288,r.left+r.width-280)),top:Math.max(8,Math.min(56,r.top)),width:280,height:56});
/** Animate the chrome's actual bounds; the editor, inputs and review state stay mounted. */
export function useAssistantMorph(ref:RefObject<HTMLDivElement>,key:string) {
 const previous=useRef<{key:string;box:Box}|null>(null),cancel=useRef<(()=>void)|null>(null);
 const run=useCallback((node:HTMLElement,from:Box,to:Box)=>new Promise<void>(resolve=>{
  cancel.current?.();
  if(window.matchMedia('(prefers-reduced-motion: reduce)').matches||!node.animate||!from.width||!to.width){resolve();return;}
  const clone=node.cloneNode(true) as HTMLElement;
  clone.setAttribute('aria-hidden','true');clone.inert=true;clone.removeAttribute('data-testid');
  clone.querySelectorAll('[id],[data-testid]').forEach(e=>{e.removeAttribute('id');e.removeAttribute('data-testid');});
  const originals=node.querySelectorAll('textarea,input');clone.querySelectorAll('textarea,input').forEach((e,index)=>{(e as HTMLInputElement).value=(originals[index] as HTMLInputElement|undefined)?.value??'';});
  const visibility=node.style.visibility;
  Object.assign(clone.style,{position:'fixed',margin:'0',zIndex:'100',pointerEvents:'none',overflow:'hidden',animation:'none',transform:'none',left:`${to.left}px`,top:`${to.top}px`,width:`${to.width}px`,height:`${to.height}px`});
  document.body.appendChild(clone);node.style.visibility='hidden';
  let done=false;const finish=()=>{if(done)return;done=true;clone.remove();node.style.visibility=visibility;if(cancel.current===finish)cancel.current=null;resolve();};cancel.current=finish;
  try {const animation=clone.animate([{...from,left:`${from.left}px`,top:`${from.top}px`,width:`${from.width}px`,height:`${from.height}px`,borderRadius:'14px',opacity:.86},{left:`${to.left}px`,top:`${to.top}px`,width:`${to.width}px`,height:`${to.height}px`,borderRadius:'6px',opacity:1}],{duration:230,easing:'cubic-bezier(.2,.75,.25,1)',fill:'none'});animation.onfinish=finish;animation.oncancel=finish;}catch{finish();}
 }),[]);
 useLayoutEffect(()=>{const node=ref.current;if(!node){previous.current=null;return;}const next=box(node),old=previous.current;previous.current={key,box:next};if(old?.key===key)return;void run(node,old?.box??compact(next),next);});
 useEffect(()=>{const cancelMotion=()=>cancel.current?.();const media=window.matchMedia('(prefers-reduced-motion: reduce)');window.addEventListener('resize',cancelMotion);media.addEventListener('change',cancelMotion);return()=>{window.removeEventListener('resize',cancelMotion);media.removeEventListener('change',cancelMotion);cancelMotion();};},[]);
 return useCallback(async()=>{const node=ref.current;if(node){node.dataset.primsExitHandled='true';const start=box(node);await run(node,start,compact(start));}},[run,ref]);
}

import { useEffect } from 'react';
/** Portal chrome shares one geometry transition. Never observe editor text or animate user drags. */
export function useEditorChromeMotion() {
 useEffect(()=>{
  const seen=new WeakSet<HTMLElement>(),active=new Set<Animation>();
  const media=window.matchMedia('(prefers-reduced-motion: reduce)');
  const selector='[role="menu"],[role="dialog"],[role="listbox"],[role="tooltip"]';
  let frame=0;const queued=new Set<HTMLElement>();
  const flush=()=>{frame=0;for(const node of queued){
   if(seen.has(node)||!node.isConnected||node.closest('.prims-assistant-panel,.prims-selection-popup')||node.matches('[data-state="closed"]'))continue;
   seen.add(node);if(media.matches||!node.animate)continue;
   const rect=node.getBoundingClientRect();if(!rect.width||!rect.height)continue;
   // Width and height animate on the original portal surface; content is clipped
   // briefly rather than remounted, with no layout changes to the editor.
   const base=getComputedStyle(node).transform==='none'?'':getComputedStyle(node).transform;
   const animation=node.animate([{width:`${rect.width*.94}px`,height:`${Math.min(rect.height,Math.max(28,rect.height*.72))}px`,transform:`${base} translateY(-5px)`,borderRadius:'12px',opacity:.88},{width:`${rect.width}px`,height:`${rect.height}px`,transform:base||'none',borderRadius:getComputedStyle(node).borderRadius,opacity:1}],{duration:190,easing:'cubic-bezier(.2,.75,.25,1)'});
   const overflow=node.style.overflow;node.style.overflow='hidden';active.add(animation);const finish=()=>{active.delete(animation);node.style.overflow=overflow;};animation.onfinish=finish;animation.oncancel=finish;
  }queued.clear();};
  const observer=new MutationObserver(records=>{for(const record of records)for(const added of record.addedNodes){if(!(added instanceof HTMLElement))continue;if(added.matches(selector))queued.add(added);added.querySelectorAll<HTMLElement>(selector).forEach(node=>queued.add(node));}if(queued.size&&!frame)frame=requestAnimationFrame(flush);});
  observer.observe(document.body,{childList:true,subtree:true});
  const cancel=()=>{for(const animation of active)animation.cancel();};
  window.addEventListener('resize',cancel);media.addEventListener('change',cancel);
  return()=>{observer.disconnect();cancelAnimationFrame(frame);window.removeEventListener('resize',cancel);media.removeEventListener('change',cancel);cancel();};
 },[]);
}

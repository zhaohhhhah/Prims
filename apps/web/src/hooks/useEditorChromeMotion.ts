import { useEffect } from 'react';
import { ChromeExitMotion } from '../lib/chrome-exit-motion';
/** Portal chrome shares geometry on entry and exit, including immediate unmounts. */
export function useEditorChromeMotion() {
 useEffect(()=>{
  const tracked=new Set<HTMLElement>(),active=new Map<HTMLElement,()=>void>(),cssClosing=new WeakSet<HTMLElement>();
  const media=window.matchMedia('(prefers-reduced-motion: reduce)');
  const exits=new ChromeExitMotion(document.body,()=>media.matches);
  const selector='[role="menu"],[role="dialog"],[role="listbox"],[role="tooltip"],.prims-assistant-panel,.prims-selection-popup';
  let frame=0;const queued=new Set<HTMLElement>();
  const resize=new ResizeObserver(entries=>{for(const entry of entries)if(entry.target instanceof HTMLElement)exits.capture(entry.target);});
  const flush=()=>{frame=0;for(const node of queued){
   if(tracked.has(node)||!node.isConnected||node.closest('[aria-hidden="true"],[data-prims-exit-layer]')||node.matches('[data-state="closed"]'))continue;
   tracked.add(node);exits.capture(node);resize.observe(node);
   const rect=node.getBoundingClientRect();if(!rect.width||!rect.height)continue;
   const base=getComputedStyle(node).transform;
   node.style.setProperty('--prims-chrome-width',`${rect.width}px`);node.style.setProperty('--prims-chrome-height',`${rect.height}px`);node.style.setProperty('--prims-chrome-transform',base==='none'?'translate(0,0)':base);node.setAttribute('data-prims-editor-chrome','');
   // The assistant and selection composer already have their own entry/morph.
   if(media.matches||!node.animate||node.matches('.prims-assistant-panel,.prims-selection-popup'))continue;
   const animation=node.animate([{width:`${rect.width*.94}px`,height:`${Math.min(rect.height,Math.max(28,rect.height*.72))}px`,transform:`${base==='none'?'':base} translateY(-5px)`,borderRadius:'12px',opacity:.88},{width:`${rect.width}px`,height:`${rect.height}px`,transform:base,borderRadius:getComputedStyle(node).borderRadius,opacity:1}],{duration:190,easing:'cubic-bezier(.2,.75,.25,1)'});
   const overflow=node.style.overflow;node.style.overflow='hidden';
   const finish=()=>{active.delete(node);node.style.overflow=overflow;};active.set(node,()=>{animation.cancel();finish();});animation.onfinish=finish;animation.oncancel=finish;
  }queued.clear();};
  const inspect=()=>{
   for(const node of tracked){if(node.isConnected)continue;resize.unobserve(node);active.get(node)?.();
    if(cssClosing.has(node)||node.matches('.prims-assistant-panel')&&document.querySelector('.prims-assistant-panel:not([aria-hidden="true"])'))exits.forget(node);else exits.exit(node);
    tracked.delete(node);
   }
  };
  const observer=new MutationObserver(records=>{
   for(const record of records){
    if(record.type==='attributes'&&record.target instanceof HTMLElement&&record.target.isConnected&&record.target.matches('[data-state="closed"]')&&tracked.has(record.target)){
     active.get(record.target)?.();cssClosing.add(record.target);
    }
    if(record.type==='attributes'&&record.target instanceof HTMLElement&&record.target.isConnected&&record.target.matches('[data-state="open"]')&&cssClosing.has(record.target)){cssClosing.delete(record.target);tracked.delete(record.target);queued.add(record.target);exits.cancel();}
    for(const added of record.addedNodes){if(!(added instanceof HTMLElement)||added.closest('[aria-hidden="true"],[data-prims-exit-layer]'))continue;if(added.matches(selector))queued.add(added);added.querySelectorAll<HTMLElement>(selector).forEach(node=>queued.add(node));}
   }
   inspect();if(queued.size&&!frame)frame=requestAnimationFrame(flush);
  });
  observer.observe(document.body,{childList:true,subtree:true,attributes:true,attributeFilter:['data-state']});
  const capture=()=>{for(const node of tracked)exits.capture(node);};
  const cancel=()=>{for(const stop of [...active.values()])stop();exits.cancel();};
  // Capture the last visible geometry before clicks/escape can unmount a surface.
  document.addEventListener('pointerdown',capture,true);document.addEventListener('keydown',capture,true);document.addEventListener('input',capture,true);
  window.addEventListener('resize',cancel);media.addEventListener('change',cancel);
  return()=>{observer.disconnect();resize.disconnect();cancelAnimationFrame(frame);document.removeEventListener('pointerdown',capture,true);document.removeEventListener('keydown',capture,true);document.removeEventListener('input',capture,true);window.removeEventListener('resize',cancel);media.removeEventListener('change',cancel);cancel();exits.clear();tracked.clear();};
 },[]);
}

import { useEffect, useState } from 'react';
/** Keep the empty dock's layout space until its directional collapse finishes. */
export function useDockPresence(open:boolean) {
 const [retained,setRetained]=useState(open);
 useEffect(()=>{
  if(open){setRetained(true);return;}
  if(window.matchMedia('(prefers-reduced-motion: reduce)').matches){setRetained(false);return;}
  const timer=setTimeout(()=>setRetained(false),190);return()=>clearTimeout(timer);
 },[open]);
 return {present:open||retained,closing:!open&&retained};
}

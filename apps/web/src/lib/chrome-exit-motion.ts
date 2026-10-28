interface SurfaceSnapshot { left:number;top:number;width:number;height:number;radius:string;background:string;color:string;font:string;shadow:string;border:string }
/** Exit pixels only: no focus, handlers or editable fields survive in the layer. */
export class ChromeExitMotion {
 private snapshots=new Map<HTMLElement,SurfaceSnapshot>();
 private active=new Map<HTMLElement,{animation:Animation;finish:()=>void}>();
 constructor(private host:HTMLElement,private reduced:()=>boolean) {}
 capture(node:HTMLElement):void {
  if(!node.isConnected||node.matches('[data-state="closed"]'))return;
  const r=node.getBoundingClientRect();if(!r.width||!r.height)return;const s=getComputedStyle(node);
  node.style.setProperty('--prims-chrome-width',`${r.width}px`);node.style.setProperty('--prims-chrome-height',`${r.height}px`);
  this.snapshots.set(node,{left:r.left,top:r.top,width:r.width,height:r.height,radius:s.borderRadius,background:s.backgroundColor,color:s.color,font:s.font,shadow:s.boxShadow,border:s.border});
 }
 forget(node:HTMLElement):void {this.snapshots.delete(node);}
 exit(node:HTMLElement):boolean {
  const r=this.snapshots.get(node);this.snapshots.delete(node);
  if(!r||node.isConnected||this.reduced()||node.dataset.primsExitHandled==='true'||typeof node.animate!=='function')return false;
  const clone=node.cloneNode(true) as HTMLElement;
  clone.removeAttribute('id');clone.removeAttribute('data-testid');clone.removeAttribute('data-state');
  clone.setAttribute('aria-hidden','true');clone.setAttribute('data-prims-exit-layer','');clone.inert=true;
  clone.querySelectorAll('[id],[data-testid],[autofocus]').forEach(e=>{e.removeAttribute('id');e.removeAttribute('data-testid');e.removeAttribute('autofocus');});
  const inputs=node.querySelectorAll('input,textarea');clone.querySelectorAll('input,textarea').forEach((e,index)=>{(e as HTMLInputElement).value=(inputs[index] as HTMLInputElement|undefined)?.value??'';});
  Object.assign(clone.style,{position:'fixed',left:`${r.left}px`,top:`${r.top}px`,width:`${r.width}px`,height:`${r.height}px`,margin:'0',maxWidth:'none',maxHeight:'none',minWidth:'0',minHeight:'0',zIndex:'100',pointerEvents:'none',overflow:'hidden',boxSizing:'border-box',transform:'none',animation:'none',transition:'none',visibility:'visible',borderRadius:r.radius,background:r.background,color:r.color,font:r.font,boxShadow:r.shadow,border:r.border});
  this.host.appendChild(clone);
  let finished=false;const finish=()=>{if(finished)return;finished=true;clone.remove();this.active.delete(node);};
  try {
   const animation=clone.animate([{width:`${r.width}px`,height:`${r.height}px`,transform:'translateY(0)',opacity:1},{width:`${r.width*.94}px`,height:`${Math.min(r.height,Math.max(24,r.height*.7))}px`,transform:'translateY(-7px)',borderRadius:'12px',opacity:0}],{duration:170,easing:'cubic-bezier(.35,0,.7,.3)',fill:'forwards'});
   this.active.set(node,{animation,finish});animation.onfinish=finish;animation.oncancel=finish;return true;
  }catch{finish();return false;}
 }
 cancel():void {for(const {animation,finish} of [...this.active.values()]){animation.cancel();finish();}}
 clear():void {this.cancel();this.snapshots.clear();}
}

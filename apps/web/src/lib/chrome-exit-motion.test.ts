import { afterEach, describe, expect, it, vi } from 'vitest';
import { ChromeExitMotion } from './chrome-exit-motion';
class Surface {
 isConnected=true;dataset:Record<string,string>={};style:Record<string,unknown>={setProperty:vi.fn()};inert=false;removed=false;closed=false;attrs=new Map<string,string>();
 input={value:'last keystroke'};frames:Keyframe[]=[];clone:Surface|null=null;
 animation={cancel:vi.fn(),onfinish:null as null|(()=>void),oncancel:null as null|(()=>void)};
 getBoundingClientRect(){return this.isConnected?{left:12,top:30,width:320,height:200}:{left:0,top:0,width:0,height:0};}
 matches(){return this.closed;}
 cloneNode(){this.clone=new Surface();this.clone.input.value='';return this.clone;}
 removeAttribute(key:string){this.attrs.delete(key);}
 setAttribute(key:string,value:string){this.attrs.set(key,value);}
 querySelectorAll(selector:string){return selector==='input,textarea'?[this.input]:[];}
 animate(frames:Keyframe[]){this.frames=frames;return this.animation;}
 remove(){this.removed=true;}
}
const cast=(n:Surface)=>n as unknown as HTMLElement;
function fixture(reduced=false){
 vi.stubGlobal('getComputedStyle',()=>({borderRadius:'8px',backgroundColor:'white',color:'black',font:'14px serif',boxShadow:'none',border:'1px solid gray'}));
 const host={appendChild:vi.fn()},surface=new Surface(),motion=new ChromeExitMotion(host as unknown as HTMLElement,()=>reduced);motion.capture(cast(surface));return {host,surface,motion};
}
afterEach(()=>vi.unstubAllGlobals());
describe('exit presence for immediately unmounted editor surfaces',()=>{
 it('uses the captured visible bounds after detached DOM reports zero geometry',()=>{const {surface,motion,host}=fixture();surface.isConnected=false;expect(motion.exit(cast(surface))).toBe(true);expect(host.appendChild).toHaveBeenCalledOnce();expect(surface.clone?.style.left).toBe('12px');expect(surface.clone?.style.width).toBe('320px');expect(surface.clone?.style.height).toBe('200px');expect(parseFloat(String(surface.clone?.frames[1]?.width))).toBeCloseTo(300.8);expect(surface.clone?.frames[1]).toMatchObject({height:'140px',opacity:0});});
 it('keeps the closing layer inert, inaccessible and preserves the final input value',()=>{const {surface,motion}=fixture();surface.isConnected=false;motion.exit(cast(surface));expect(surface.clone?.inert).toBe(true);expect(surface.clone?.attrs.get('aria-hidden')).toBe('true');expect(surface.clone?.style.pointerEvents).toBe('none');expect(surface.clone?.input.value).toBe('last keystroke');});
 it('removes the visual layer at animation end and will not replay the same removal',()=>{const {surface,motion}=fixture();surface.isConnected=false;motion.exit(cast(surface));surface.clone?.animation.onfinish?.();expect(surface.clone?.removed).toBe(true);expect(motion.exit(cast(surface))).toBe(false);});
 it('cancels layers on rapid reopening, resizing or route cleanup',()=>{const {surface,motion}=fixture();surface.isConnected=false;motion.exit(cast(surface));motion.clear();expect(surface.clone?.animation.cancel).toHaveBeenCalledOnce();expect(surface.clone?.removed).toBe(true);});
 it('respects reduced motion and skips a surface still mounted or already morphed',()=>{const reduced=fixture(true);reduced.surface.isConnected=false;expect(reduced.motion.exit(cast(reduced.surface))).toBe(false);expect(reduced.host.appendChild).not.toHaveBeenCalled();const live=fixture();expect(live.motion.exit(cast(live.surface))).toBe(false);live.motion.capture(cast(live.surface));live.surface.dataset.primsExitHandled='true';live.surface.isConnected=false;expect(live.motion.exit(cast(live.surface))).toBe(false);});
 it('cleans up even when the browser cannot start an exit animation',()=>{const {surface,motion}=fixture();surface.isConnected=false;const clone=new Surface();clone.animate=()=>{throw new Error('unsupported');};surface.cloneNode=()=>clone;expect(motion.exit(cast(surface))).toBe(false);expect(clone.removed).toBe(true);});
});

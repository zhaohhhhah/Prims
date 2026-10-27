import { describe,expect,it } from 'vitest';
import { applyResolvedTheme, listenToSystemTheme, readStoredTheme, resolveTheme } from './theme-state';
describe('theme preference and live system changes',()=>{
 it('retains supported choices and tolerates unavailable storage',()=>{expect(readStoredTheme('system',{getItem:()=> 'dark'})).toBe('dark');expect(readStoredTheme('system',{getItem:()=> 'invalid'})).toBe('system');expect(readStoredTheme('system',{getItem:()=>{throw new Error('blocked');}})).toBe('system');});
 it('applies explicit themes independently of the system and preserves legacy contrast',()=>{expect(resolveTheme('light','dark')).toBe('light');expect(resolveTheme('dark','light')).toBe('dark');expect(resolveTheme('high-contrast','light')).toBe('high-contrast');});
 it('keeps data-theme, dark utilities, native color scheme and consumers consistent as the system changes',()=>{
  let listener:(()=>void)|undefined;let matches=false;const seen:string[]=[];let attribute='',dark=false,scheme='';
  const root={setAttribute:(_key:string,value:string)=>{attribute=value;},classList:{toggle:(_key:string,value:boolean)=>{dark=value;}},style:{get colorScheme(){return scheme;},set colorScheme(value:string){scheme=value;}}} as unknown as HTMLElement;
  const media={get matches(){return matches;},addEventListener:(_name:string,fn:()=>void)=>{listener=fn;},removeEventListener:()=>{listener=undefined;}} as unknown as MediaQueryList;
  const dispose=listenToSystemTheme(media,system=>{const resolved=resolveTheme('system',system);seen.push(resolved);applyResolvedTheme(root,resolved);});
  expect([attribute,dark,scheme]).toEqual(['light',false,'light']);matches=true;listener!();expect([attribute,dark,scheme]).toEqual(['dark',true,'dark']);matches=false;listener!();expect(seen).toEqual(['light','dark','light']);expect(dark).toBe(false);dispose();expect(listener).toBeUndefined();
 });
});

import {describe,expect,it} from 'vitest';
import {assistantLayoutKey,clampAssistantLayout,defaultAssistantLayout,effectiveAssistantMode,parseAssistantLayout} from './assistant-layout';
const viewport={width:1280,height:800};
describe('persistent assistant workspace layout',()=>{
 it('defaults to a visible dock and recovers from invalid or untrusted storage',()=>{expect(defaultAssistantLayout().mode).toBe('right');expect(defaultAssistantLayout().visible).toBe(true);expect(parseAssistantLayout('{broken',viewport)).toEqual(clampAssistantLayout(null,viewport));expect(clampAssistantLayout({mode:'unexpected',rightWidth:Infinity,floating:{x:NaN}},viewport).mode).toBe('right');});
 it('clamps floating size and position so the header stays reachable after a viewport change',()=>{const layout=clampAssistantLayout({mode:'float',floating:{x:5000,y:-200,width:3000,height:4000}},{width:600,height:500});expect(layout.floating).toEqual({x:8,y:8,width:584,height:484});});
 it('restores chosen mode, size and visibility independently for a signed-in user',()=>{const stored={...defaultAssistantLayout(),mode:'bottom',visible:false,bottomHeight:250,floating:{x:120,y:100,width:480,height:500}};expect(parseAssistantLayout(JSON.stringify(stored),viewport)).toEqual(stored);expect(assistantLayoutKey('account/a')).not.toBe(assistantLayoutKey('account/b'));});
 it('uses the editor bottom on narrow workspaces without overwriting the right-dock preference',()=>{const layout=defaultAssistantLayout();expect(effectiveAssistantMode(layout,480)).toBe('bottom');expect(layout.mode).toBe('right');expect(effectiveAssistantMode(layout,1200)).toBe('right');expect(effectiveAssistantMode({...layout,mode:'float'},480)).toBe('float');});
});

it('honors right docking on ordinary narrow desktop panes instead of silently using bottom',()=>{const layout=defaultAssistantLayout();expect(effectiveAssistantMode(layout,650)).toBe('right');expect(effectiveAssistantMode(layout,480)).toBe('bottom');});

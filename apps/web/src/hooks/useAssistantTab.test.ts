import { afterEach, describe, expect, it, vi } from 'vitest';
import { createElement } from 'react';
import { renderToString } from 'react-dom/server';
import { useAssistantTab } from './useAssistantTab';
import type { AssistantTabState } from '../lib/assistant-tab';
const state:AssistantTabState={drafts:{panel:'draft',selection:''},threadId:'thread',modelChoice:'model',filePath:'main.tex',plan:null,fileActions:[],target:null};
function renderBridge(standalone:boolean,transfer:AssistantTabState){
 const transitionOut=vi.fn(async()=>{}),receive=vi.fn(async()=>{});let bridge:ReturnType<typeof useAssistantTab>|undefined;
 function Probe(){bridge=useAssistantTab({userId:'user',projectId:'project',standalone,busy:false,state:transfer,receive,onAway:vi.fn(),onRefresh:vi.fn(),editor:{fileId:'main',filePath:'main.tex',selection:null},transitionOut,onNavigate:vi.fn(),onDock:vi.fn()});return null;}
 renderToString(createElement(Probe));if(!bridge)throw new Error('bridge missing');return {bridge,transitionOut,receive};
}
afterEach(()=>vi.unstubAllGlobals());
describe('assistant transfer keeps the current window before invalid handoffs',()=>{
 it('does not open an empty tab for an oversized state',()=>{
  const open=vi.fn();vi.stubGlobal('window',{open,location:{origin:'https://example.test'}});vi.stubGlobal('BroadcastChannel',class{});
  const {bridge}=renderBridge(false,{...state,drafts:{panel:'x'.repeat(8*1024*1024),selection:''}});bridge.openTab();expect(open).not.toHaveBeenCalled();expect(bridge.locked).toBe(false);
 });
 it('keeps the detached assistant visible instead of transitioning away with an untransferable draft',()=>{
  const {bridge,transitionOut,receive}=renderBridge(true,{...state,drafts:{panel:'x'.repeat(8*1024*1024),selection:''}});bridge.returnToEditor('right');expect(transitionOut).not.toHaveBeenCalled();expect(receive).not.toHaveBeenCalled();
 });
});

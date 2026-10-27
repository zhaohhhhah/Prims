import {afterEach,beforeEach,describe,expect,it,vi} from 'vitest';
const state=vi.hoisted(()=>({sessionUser:'owner',fetch:vi.fn()}));
vi.mock('../stores/auth',()=>({useAuthStore:()=>undefined}));
vi.mock('./supabase',()=>({API_URL:'',supabase:{auth:{getSession:async()=>({data:{session:{user:{id:state.sessionUser},access_token:'TEST_TOKEN'}}})}}}));
import {summaryRequestAction,versionSummariesApi,type VersionSummary} from './version-summaries';
const now=Date.parse('2026-10-30T10:00:00Z');
const item:VersionSummary={versionId:'version',status:'not_requested',summary:null,error:null,updatedAt:null,attempts:0,requestCount:0,canGenerate:true};
beforeEach(()=>{state.sessionUser='owner';state.fetch.mockReset();vi.stubGlobal('fetch',state.fetch);});
afterEach(()=>vi.unstubAllGlobals());
describe('version summary eligibility',()=>{
 it('never offers work to a non-creator, readonly user or an already successful/inflight task',()=>{for(const status of ['queued','running','succeeded'] as const)expect(summaryRequestAction({...item,status},false,now)).toBeNull();expect(summaryRequestAction({...item,canGenerate:false},false,now)).toBeNull();expect(summaryRequestAction(item,true,now)).toBeNull();});
 it('requires explicit retries and respects cooldown and the attempt bound',()=>{expect(summaryRequestAction(item,false,now)).toMatchObject({retry:false,disabled:false});expect(summaryRequestAction({...item,status:'failed',updatedAt:'2026-10-30T09:59:45Z'},false,now)).toMatchObject({retry:true,disabled:true});expect(summaryRequestAction({...item,status:'skipped',updatedAt:'2026-10-30T09:59:00Z'},false,now)).toMatchObject({retry:true,disabled:false});expect(summaryRequestAction({...item,status:'failed',attempts:10},false,now)?.disabled).toBe(true);});
});
describe('version summary requests contain no project document',()=>{
 it('only submits an ID and explicit retry flag; it never guesses a queued state or posts automatically',async()=>{expect(state.fetch).not.toHaveBeenCalled();state.fetch.mockResolvedValue({ok:true,json:async()=>({versionId:'version',queued:true})});await versionSummariesApi.generate('project','version',false,'owner');await versionSummariesApi.generate('project','version',true,'owner');expect(state.fetch.mock.calls.map(call=>JSON.parse(call[1].body))).toEqual([{}, {retry:true}]);expect(state.fetch.mock.calls[0]![0]).toBe('/api/projects/project/versions/version/summary');expect(state.fetch).toHaveBeenCalledTimes(2);});
 it('stores only the user preference and refuses a switched account before any request',async()=>{state.fetch.mockResolvedValue({ok:true,json:async()=>({enabled:true,enabledSince:'2026-10-30T10:00:00Z'})});await versionSummariesApi.setPreferences(true,'owner');expect(JSON.parse(state.fetch.mock.calls[0]![1].body)).toEqual({enabled:true});state.fetch.mockClear();state.sessionUser='other';await expect(versionSummariesApi.generate('project','version',false,'owner')).rejects.toThrow('账号已切换');expect(state.fetch).not.toHaveBeenCalled();});
 it('surfaces one failed request without silently retrying a model call',async()=>{state.fetch.mockRejectedValue(new Error('network failed'));await expect(versionSummariesApi.generate('project','version',true,'owner')).rejects.toThrow('network failed');expect(state.fetch).toHaveBeenCalledTimes(1);});
});

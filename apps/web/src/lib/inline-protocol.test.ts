import {afterEach,describe,expect,it,vi} from 'vitest';
vi.mock('./supabase',()=>({API_URL:'',supabase:{auth:{getSession:vi.fn(async()=>({data:{session:{access_token:'test-session'}}}))}}}));
import {streamInline} from './inline-protocol';
const input={feature:'chat' as const,selection:'a small clue',options:{projectId:'project',filePath:'main.tex'}};
afterEach(()=>vi.unstubAllGlobals());
describe('inline stream safety and image warning',()=>{
 it('forwards a warning without interrupting the same successful response',async()=>{
  const warning={code:'image_unsupported',message:'image not supported'};const raw=JSON.stringify({reply:'text answer',replacement:null});
  const fetcher=vi.fn(async()=>new Response(`data: ${JSON.stringify({warning})}\n\ndata: ${JSON.stringify({text:raw})}\n\ndata: [DONE]\n\n`));vi.stubGlobal('fetch',fetcher);const activity=vi.fn();
  expect(await streamInline(input,new AbortController().signal,vi.fn(),activity)).toBe(raw);expect(activity).toHaveBeenCalledWith({warning});expect(fetcher).toHaveBeenCalledTimes(1);
 });
 it('never returns an editable proposal when a connection closes before DONE',async()=>{
  vi.stubGlobal('fetch',vi.fn(async()=>new Response(`data: ${JSON.stringify({text:'{"reply":"ok","replacement":"partial"}'})}\n\n`)));
  await expect(streamInline(input,new AbortController().signal,vi.fn())).rejects.toThrow('连接提前中断');
 });
 it('reports authentication errors as errors instead of showing an image warning or retrying',async()=>{
  const fetcher=vi.fn(async()=>new Response(JSON.stringify({message:'Authentication failed'}),{status:401}));vi.stubGlobal('fetch',fetcher);const activity=vi.fn();
  await expect(streamInline(input,new AbortController().signal,vi.fn(),activity)).rejects.toThrow('Authentication failed');expect(activity).not.toHaveBeenCalled();expect(fetcher).toHaveBeenCalledTimes(1);
 });
 it('delivers complete candidates before a later error without accepting partial final JSON',async()=>{const candidate={id:'candidate-a',filePath:'main.tex',before:'old',after:'new',status:'pending_review'};const fetcher=vi.fn(async()=>new Response(`data: ${JSON.stringify({proposal:candidate})}\n\ndata: ${JSON.stringify({error:'Output limit'})}\n\n`));vi.stubGlobal('fetch',fetcher);const retained:unknown[]=[];await expect(streamInline(input,new AbortController().signal,vi.fn(),event=>{if(event.proposal)retained.push(event.proposal);})).rejects.toThrow('Output limit');expect(retained).toEqual([candidate]);expect(fetcher).toHaveBeenCalledTimes(1);});
 it('keeps already committed candidates on EOF or cancellation but never forwards a truncated candidate frame',async()=>{const candidate={id:'candidate-a',filePath:'main.tex',before:'old',after:'new',status:'pending_review'};for(const tail of ['', 'data: {"proposal":{"id":"incomplete"']){vi.stubGlobal('fetch',vi.fn(async()=>new Response(`data: ${JSON.stringify({proposal:candidate})}\n\n${tail}`)));const activity=vi.fn();await expect(streamInline(input,new AbortController().signal,vi.fn(),activity)).rejects.toThrow();expect(activity.mock.calls.filter(([event])=>event.proposal).map(([event])=>event.proposal)).toEqual([candidate]);}const control=new AbortController();vi.stubGlobal('fetch',vi.fn(async()=>new Response(`data: ${JSON.stringify({proposal:candidate})}\n\n`)));const retained:unknown[]=[];await expect(streamInline(input,control.signal,vi.fn(),event=>{if(event.proposal){retained.push(event.proposal);control.abort();}})).rejects.toMatchObject({name:'AbortError'});expect(retained).toEqual([candidate]);});
 it('forwards partial completion status and the normal canonical final once',async()=>{const raw=JSON.stringify({reply:'Some complete segments',replacement:null,edits:[],partial:true});vi.stubGlobal('fetch',vi.fn(async()=>new Response(`data: ${JSON.stringify({partial:true,warning:{code:'partial_completion',message:'Complete segments remain'}})}\n\ndata: ${JSON.stringify({text:raw})}\n\ndata: [DONE]\n\n`)));const activity=vi.fn();expect(await streamInline(input,new AbortController().signal,vi.fn(),activity)).toBe(raw);expect(activity).toHaveBeenCalledWith(expect.objectContaining({partial:true}));});

});

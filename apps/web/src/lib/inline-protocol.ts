import type { ContextUsed } from './project-ai-context';
import type { AICompleteInput } from '@scribe/shared';
import { API_URL, supabase } from './supabase';

/** A completed SSE frame is required; stopped or disconnected output is never a proposal. */
export async function streamInline(input:AICompleteInput, signal:AbortSignal, onChunk:(count:number)=>void, onActivity?:(event:{status?:string;file?:string;received?:number;contextUsed?:ContextUsed;warning?:{code:string;message:string}})=>void):Promise<string> {
  const {data:{session}}=await supabase.auth.getSession();
  if(!session)throw new Error('登录已过期，请重新登录');
  if(signal.aborted)throw new DOMException('Aborted','AbortError');
  const response=await fetch(`${API_URL}/api/ai/complete`,{method:'POST',headers:{'Content-Type':'application/json',Authorization:`Bearer ${session.access_token}`},body:JSON.stringify(input),signal});
  if(!response.ok || !response.body){
    let message=`请求失败 (${response.status})`;
    try {const body=await response.json() as {message?:string};message=body.message ?? message;}catch{/* use status */}
    throw new Error(message);
  }
  const reader=response.body.getReader();const decoder=new TextDecoder();
  let buffer='',text='',done=false;
  const frame=(line:string)=>{
    if(!line.startsWith('data:'))return;
    const payload=line.slice(5).trim();
    if(payload==='[DONE]'){done=true;return;}
    const value=JSON.parse(payload) as {text?:string;error?:string;status?:string;file?:string;received?:number;contextUsed?:ContextUsed;warning?:{code:string;message:string}};
    if(value.error)throw new Error(value.error);
    if(value.status||value.contextUsed||value.warning)onActivity?.(value);
    if(typeof value.text==='string'){text+=value.text;if(text.length>150000)throw new Error('生成内容过长，请缩小选区后重试');onChunk(text.length);}
  };
  try {
    while(!done){const chunk=await reader.read();if(chunk.done)break;buffer+=decoder.decode(chunk.value,{stream:true});let index;
      while((index=buffer.indexOf('\n'))!==-1){const line=buffer.slice(0,index).trim();buffer=buffer.slice(index+1);frame(line);if(done)break;}}
    buffer+=decoder.decode();if(!done&&buffer.trim())frame(buffer.trim());
    if(signal.aborted)throw new DOMException('Aborted','AbortError');
    if(!done)throw new Error('连接提前中断，原文未改动，请重试');
    return text;
  } finally {await reader.cancel().catch(()=>{});reader.releaseLock();}
}

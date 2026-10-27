import { API_URL, supabase } from './supabase';
export interface ProjectAIMessage {id:string;role:'user'|'assistant'|'action';content:string;raw?:string;filePath?:string;selection?:string;selectionId?:string;createdAt?:string}
export interface ProjectAIContext {brief:string;autoSummary:string;summaryUpdatedAt:string|null;threadId:string;messages:ProjectAIMessage[];hasEarlier:boolean}
export interface ContextUsed {brief:boolean;historyMessages:number;historyTruncated:boolean;fileTools:boolean;currentSection?:string;selectionStartLine?:string;selectionEndLine?:string;documentWindowStartLine?:string;documentWindowEndLine?:string}
export class ContextRequestError extends Error {constructor(public status:number,message:string){super(message);}}
async function request<T>(projectId:string,suffix='',body?:unknown,method=body===undefined?'GET':'POST'):Promise<T>{
 const {data:{session}}=await supabase.auth.getSession();if(!session)throw new Error('登录已过期，请重新登录');
 const response=await fetch(`${API_URL}/api/projects/${encodeURIComponent(projectId)}/ai/context${suffix}`,{method,headers:{Authorization:`Bearer ${session.access_token}`,'Content-Type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)})});
 if(!response.ok){let message=`上下文请求失败（${response.status}）`;try{message=((await response.json()) as {message?:string}).message??message;}catch{/* status fallback */}throw new ContextRequestError(response.status,message);}
 return response.json() as Promise<T>;
}
export interface ProjectAIThread {id:string;title:string;updatedAt:string;messageCount:number}
export const projectAIContext={get:(projectId:string)=>request<ProjectAIContext>(projectId),saveBrief:(projectId:string,brief:string)=>request<ProjectAIContext>(projectId,'',{brief},'PUT'),append:(projectId:string,threadId:string,messages:ProjectAIMessage[])=>request<ProjectAIContext>(projectId,'/messages',{threadId,messages}),newThread:(projectId:string)=>request<ProjectAIContext>(projectId,'/new',{}),threads:(projectId:string)=>request<{threads:ProjectAIThread[];hasEarlier:boolean}>(projectId,'/threads'),select:(projectId:string,threadId:string)=>request<ProjectAIContext>(projectId,'/select',{threadId})};
export function mergeContextMessages(saved:ProjectAIMessage[],pending:ProjectAIMessage[]):ProjectAIMessage[]{const ids=new Set(saved.map(m=>m.id));return [...saved,...pending.filter(m=>!ids.has(m.id))];}
/** A new selected target never inherits an unapplied replacement from an old target. */
export function proposalForTarget<T extends {selectionId:string;filePath:string;threadId?:string}>(proposal:T|null,selectionId:string|undefined,filePath:string,threadId?:string):T|null{return proposal&&proposal.selectionId===selectionId&&proposal.filePath===filePath&&(proposal.threadId===undefined||proposal.threadId===threadId)?proposal:null;}

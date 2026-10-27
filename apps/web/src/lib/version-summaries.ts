import {useQuery} from '@tanstack/react-query';
import {useAuthStore} from '../stores/auth';
import {API_URL,supabase} from './supabase';
export interface VersionSummaryPreferences {enabled:boolean;enabledSince:string|null}
export type VersionSummaryStatus='not_requested'|'queued'|'running'|'succeeded'|'failed'|'skipped';
export interface VersionSummary {versionId:string;status:VersionSummaryStatus;summary:string|null;error:string|null;updatedAt:string|null;attempts:number;requestCount:number;canGenerate:boolean}
export const versionSummaryPreferencesKey=(userId:string|undefined)=>['version-summary-preferences',userId] as const;
export const versionSummariesKey=(userId:string|undefined,projectId:string)=>['version-summaries',userId,projectId] as const;
async function request<T>(path:string,body:unknown|undefined,method:string,expectedUser:string|undefined):Promise<T>{
 const {data:{session}}=await supabase.auth.getSession();if(!session||expectedUser&&session.user.id!==expectedUser)throw new Error('账号已切换，请重新打开页面');
 const response=await fetch(`${API_URL}/api${path}`,{method,cache:'no-store',headers:{Authorization:`Bearer ${session.access_token}`,'Content-Type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)})});
 let value:unknown;try{value=await response.json();}catch{throw new Error('摘要服务响应异常，请稍后重试');}
 if(!response.ok){const error=value as {message?:string;error?:{message?:string}};throw new Error(error.message??error.error?.message??'摘要操作未完成');}return value as T;
}
export const versionSummariesApi={preferences:(userId?:string)=>request<VersionSummaryPreferences>('/version-summary/preferences',undefined,'GET',userId),setPreferences:(enabled:boolean,userId?:string)=>request<VersionSummaryPreferences>('/version-summary/preferences',{enabled},'PUT',userId),list:(projectId:string,userId?:string)=>request<{summaries:VersionSummary[]}>(`/projects/${encodeURIComponent(projectId)}/version-summaries`,undefined,'GET',userId),generate:(projectId:string,versionId:string,retry:boolean,userId?:string)=>request<{versionId:string;queued:boolean}>(`/projects/${encodeURIComponent(projectId)}/versions/${encodeURIComponent(versionId)}/summary`,retry?{retry:true}:{},'POST',userId)};
export function useVersionSummaryPreferences(){const userId=useAuthStore(state=>state.user?.id);return useQuery({queryKey:versionSummaryPreferencesKey(userId),queryFn:()=>versionSummariesApi.preferences(userId),enabled:!!userId,staleTime:30000,refetchOnWindowFocus:false});}
export function useVersionSummaries(projectId:string){const userId=useAuthStore(state=>state.user?.id);return useQuery({queryKey:versionSummariesKey(userId,projectId),queryFn:()=>versionSummariesApi.list(projectId,userId),enabled:!!userId&&!!projectId,refetchOnWindowFocus:false,refetchInterval:query=>query.state.data?.summaries.some(item=>item.status==='queued'||item.status==='running')?5000:30000});}
/** Only explicit creator-authorized requests can enqueue work; successes never regenerate. */
export function summaryRequestAction(item:VersionSummary|undefined,readOnly:boolean,now:number){
 if(!item||!item.canGenerate||readOnly)return null;
 if(item.status==='not_requested')return {label:'生成摘要',retry:false,disabled:false,reason:''};
 if(item.status!=='failed'&&item.status!=='skipped')return null;
 const when=item.updatedAt?Date.parse(item.updatedAt):NaN,cooling=Number.isFinite(when)&&now<when+30000;
 return {label:'重试',retry:true,disabled:item.attempts>=10||cooling,reason:item.attempts>=10?'已达到重试上限':cooling?'请稍后再试':''};
}

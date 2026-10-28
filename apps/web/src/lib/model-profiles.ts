import type { AIProvider } from '@scribe/shared';
import { useQuery } from '@tanstack/react-query';
import { useAuthStore } from '../stores/auth';
import { API_URL, supabase } from './supabase';
export interface ModelProfile {id:string;name:string;provider:AIProvider;model:string;models?:string[];contextWindows?:Record<string,number>;baseUrl:string|null;apiKeySet:boolean;ready:boolean;updatedAt:string}
export interface ModelProfileList {profiles:ModelProfile[];defaultProfileId?:string|null;defaultModel?:string|null;migration?:{status:'migrated'|'none'|'capacity'|'invalid'|'decrypt_failed';message:string|null}}
export interface ModelProfileDraft {name:string;provider:AIProvider;model:string;models?:string[];contextWindows?:Record<string,number>;baseUrl?:string|null;apiKey?:string}
export function profileModelNames(profile:Pick<ModelProfile,'model'|'models'>):string[]{const source=profile.models?.length?profile.models:[profile.model];return [...new Set(source.map(name=>name.trim()).filter(Boolean))];}
/** One complete model ID per line: commas inside a model ID are deliberately preserved. */
export function parseModelNamesInput(input:string):string[]{
 const models=[...new Set(input.split(/\r?\n/).map(name=>name.trim()).filter(Boolean))];
 if(!models.length)throw new Error('请至少填写一个模型名称，每行一个');
 if(models.some(name=>name.length>256||/[\x00-\x1f\x7f]/.test(name)))throw new Error('每个模型名称最多 256 个字符，不能包含控制字符');
 return models;
}
export function isModelProfileReady(profile:ModelProfile):boolean{return profile.ready??(profile.apiKeySet||profile.provider==='ollama'||profile.provider==='lmstudio');}
export function defaultModelChoice(list:ModelProfileList|undefined){const profile=list?.profiles.find(item=>item.id===list.defaultProfileId);if(!profile||!isModelProfileReady(profile))return undefined;const model=list?.defaultModel??profile.model;return profileModelNames(profile).includes(model)?{profile,model,value:JSON.stringify([profile.id,model])}:undefined;}
export class ModelProfileError extends Error {constructor(public status:number,message:string){super(message);}}
export const modelProfilesQueryKey=(userId:string|undefined)=>['ai-model-profiles',userId] as const;
/** Only a saved profile UUID is sent; default deliberately leaves the existing API config untouched. */
export function normalizeModelProfileId(value:string|null|undefined):string|undefined {
 if(!value||value==='default')return undefined;
 if(!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value))throw new Error('请选择默认模型或已保存的模型配置');
 return value;
}
async function request<T>(suffix='',body?:unknown,method=body===undefined?'GET':'POST',expectedUser=useAuthStore.getState().user?.id):Promise<T>{
 const {data:{session}}=await supabase.auth.getSession();if(!session)throw new ModelProfileError(401,'登录已过期，请重新登录');if(expectedUser&&session.user.id!==expectedUser)throw new ModelProfileError(401,'账号已切换，请重新打开模型设置');
 const response=await fetch(`${API_URL}/api/ai/profiles${suffix}`,{method,cache:'no-store',headers:{Authorization:`Bearer ${session.access_token}`,'Content-Type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)})});
 if(response.status===204)return undefined as T;
 let value:unknown;try{value=await response.json();}catch{throw new ModelProfileError(response.status,'模型配置服务响应异常，请稍后重试');}
 if(!response.ok){const error=value as {message?:string;error?:{message?:string}};throw new ModelProfileError(response.status,error.message??error.error?.message??`模型配置操作失败（${response.status}）`);}
 return value as T;
}
function path(id:string):string {const valid=normalizeModelProfileId(id);if(!valid)throw new Error('默认配置请在上方表单修改');return '/'+valid;}
export const modelProfilesApi={updateContextWindow:(id:string,modelName:string,contextWindowTokens:number)=>request<ModelProfile>(path(id)+'/context-window',{modelName,contextWindowTokens},'PUT'),list:(userId?:string)=>request<ModelProfileList>('',undefined,'GET',userId),setDefault:(profileId:string,modelName:string)=>request<ModelProfileList>('/default',{profileId,modelName},'PUT'),create:(input:ModelProfileDraft)=>request<ModelProfile>('',input),update:(id:string,input:ModelProfileDraft)=>request<ModelProfile>(path(id),input,'PUT'),remove:(id:string)=>request<void>(path(id),undefined,'DELETE')};
export function useModelProfiles(){const userId=useAuthStore(state=>state.user?.id);return useQuery({queryKey:modelProfilesQueryKey(userId),queryFn:()=>modelProfilesApi.list(userId),enabled:!!userId,staleTime:10000,refetchOnWindowFocus:false});}

export const DEFAULT_CONTEXT_WINDOW=262144;
export function parseContextWindowsInput(models:string[],draft:Record<string,string>):Record<string,number>{
 const windows:Record<string,number>={};
 for(const model of models){const value=draft[model]?.trim();if(!value)continue;if(!/^\d+$/.test(value))throw new Error(`${model} 的上下文长度应为整数`);const count=Number(value);if(!Number.isSafeInteger(count)||count<16384||count>2097152)throw new Error(`${model} 的上下文长度需在 16384–2097152 tokens 之间`);windows[model]=count;}
 return windows;
}

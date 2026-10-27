import {API_URL,supabase} from './supabase';
import {useAuthStore} from '../stores/auth';
export interface ImportedSkillProposal {id:string;name:string;description:string;content:string;installed:false;status:'pending_review';permissionsGranted:false;untrusted:true}
export interface SkillImportPreview {source:string;content:string;proposal:ImportedSkillProposal|null;validationError:string|null;installed:false;requiresAI:boolean;sourceKind:'html'|'text';truncated:boolean}
export interface SkillRefineInput {content:string;instruction?:string;modelProfileId?:string;modelName?:string}
function checkOwner(userId:string){if(useAuthStore.getState().user?.id!==userId)throw new Error('账号已切换，请重新打开导入窗口');}
async function post<T>(userId:string,path:string,body:unknown,signal:AbortSignal):Promise<T>{
 checkOwner(userId);const {data:{session}}=await supabase.auth.getSession();checkOwner(userId);
 if(!session||session.user.id!==userId)throw new Error('登录已过期，请重新登录');
 const response=await fetch(`${API_URL}/api/agent/skill-import/${path}`,{method:'POST',signal,cache:'no-store',headers:{Authorization:`Bearer ${session.access_token}`,'Content-Type':'application/json'},body:JSON.stringify(body)});
 checkOwner(userId);const value:unknown=await response.json();checkOwner(userId);
 if(!response.ok){const error=value&&typeof value==='object'?value as {message?:string;error?:{message?:string}}:{};throw new Error(error.message??error.error?.message??'技能导入请求失败');}
 return value as T;
}
export const skillImportApi={preview:(user:string,url:string,signal:AbortSignal)=>post<SkillImportPreview>(user,'preview',{url},signal),refine:(user:string,input:SkillRefineInput,signal:AbortSignal)=>post<ImportedSkillProposal>(user,'refine',input,signal)};

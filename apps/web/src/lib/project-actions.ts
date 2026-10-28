import { API_URL,supabase } from './supabase';
import { useAuthStore } from '../stores/auth';
export type CompilerName='tectonic'|'pdflatex'|'xelatex'|'lualatex';
export interface CompileSettings {compiler:CompilerName;mainFile:string}
export interface FileActionInput {type:'create_file'|'create_directory'|'move_file';path:string;content?:string;newPath?:string}
export interface CompileSettingsAction extends CompileSettings {type:'set_compile_settings';expected:CompileSettings}
export type ProjectActionInput=FileActionInput|CompileSettingsAction;
export type ProjectActionOperation=({type:FileActionInput['type'];path:string;newPath?:string;fileId?:string})|({type:'set_compile_settings';before:CompileSettings}&CompileSettings);
export interface PreparedProjectAction {id:string;action:ProjectActionInput;status:'pending'|'applied'|'rejected';result:null|{operation:ProjectActionOperation;textVersionId:string|null;note:string}}
export interface PreparedProjectActions {planId:string;actions:PreparedProjectAction[]}
export function projectActionPath(action:ProjectActionInput):string{return action.type==='set_compile_settings'?action.mainFile:action.path;}
export function compilerLabel(value:CompilerName):string{return {tectonic:'Tectonic',pdflatex:'pdfLaTeX',xelatex:'XeLaTeX',lualatex:'LuaLaTeX'}[value];}
function compilerValid(value:unknown):value is CompilerName{return ['tectonic','pdflatex','xelatex','lualatex'].includes(value as string);}
export interface RestorePlan {versionId:string;historyKind:'text';edits:{fileId:string;path:string;before:string;after:string}[];missingPaths:string[];unsupportedPaths:string[];note:string}
function pathValid(value:unknown):value is string{return typeof value==='string'&&value.length>0&&new TextEncoder().encode(value).length<=512&&!value.startsWith('/')&&!/[\\:\x00-\x1f\x7f]/.test(value)&&!value.split('/').some(p=>!p||p==='.'||p==='..')&&value.split('/')[0]!=='.git';}
export function parseProjectActions(value:unknown):ProjectActionInput[]{
 const source=typeof value==='string'?JSON.parse(value) as unknown:value;
 const array=Array.isArray(source)?source:(source as {actions?:unknown}|null)?.actions;
 if(array===undefined)return [];if(!Array.isArray(array)||array.length>12)throw new Error('文件操作计划最多包含 12 项');let total=0;const targets=new Set<string>();
 return array.map(raw=>{if(!raw||typeof raw!=='object'||Array.isArray(raw))throw new Error('文件操作计划格式无效');const action=raw as Record<string,unknown>;
  if(action.type==='set_compile_settings'){
   const expected=action.expected as Record<string,unknown>|undefined;
   if(!compilerValid(action.compiler)||!pathValid(action.mainFile)||!/\.tex$/i.test(action.mainFile)||!expected||typeof expected!=='object'||Array.isArray(expected)||!compilerValid(expected.compiler)||!pathValid(expected.mainFile)||Object.keys(expected).some(k=>!['compiler','mainFile'].includes(k))||Object.keys(action).some(k=>!['type','compiler','mainFile','expected'].includes(k)))throw new Error('编译设置建议格式无效，请重新生成');
   if(targets.has('\0compile-settings'))throw new Error('每轮只能有一项编译设置建议');targets.add('\0compile-settings');
   return {type:'set_compile_settings',compiler:action.compiler,mainFile:action.mainFile,expected:{compiler:expected.compiler,mainFile:expected.mainFile}};
  }
  if(!pathValid(action.path))throw new Error('文件路径必须位于当前项目内，不能包含上级目录或绝对路径');const path=action.path;total+=path.length;
  let parsed:FileActionInput;
  if(action.type==='create_file'&&typeof action.content==='string'&&action.content.length<=40000&&Object.keys(action).every(k=>['type','path','content'].includes(k))){total+=action.content.length;parsed={type:'create_file',path,content:action.content};}
  else if(action.type==='create_directory'&&Object.keys(action).every(k=>['type','path'].includes(k)))parsed={type:'create_directory',path};
  else if(action.type==='move_file'&&pathValid(action.newPath)&&action.newPath!==path&&Object.keys(action).every(k=>['type','path','newPath'].includes(k))){total+=action.newPath.length;parsed={type:'move_file',path,newPath:action.newPath};}
  else throw new Error('仅支持新建文本文件、目录和移动文件，不能删除或覆盖');
  const target=parsed.newPath??parsed.path;if(targets.has(target))throw new Error('多个操作不能使用同一个目标路径');targets.add(target);if(total>80000)throw new Error('文件操作计划文本总量不能超过 80000 字符');return parsed;
 });
}
async function request<T>(projectId:string,suffix:string,body?:unknown,method=body===undefined?'GET':'POST'):Promise<T>{
 const expected=useAuthStore.getState().user?.id;const {data:{session}}=await supabase.auth.getSession();if(!session||expected&&session.user.id!==expected)throw new Error('登录已过期或账号已切换，请重新打开项目');
 const response=await fetch(`${API_URL}/api/projects/${encodeURIComponent(projectId)}${suffix}`,{method,cache:'no-store',headers:{Authorization:`Bearer ${session.access_token}`,'Content-Type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)})});
 const value:unknown=await response.json();if(!response.ok){const error=value as {message?:string;error?:{message?:string}};throw new Error(error.message??error.error?.message??'文件操作未完成，请刷新文件树检查');}return value as T;
}
export const projectActionsApi={prepare:(project:string,actions:readonly ProjectActionInput[])=>request<PreparedProjectActions>(project,'/ai/actions/prepare',{actions}),apply:(project:string,plan:string,item:string)=>request<PreparedProjectAction>(project,`/ai/actions/${encodeURIComponent(plan)}/${encodeURIComponent(item)}/apply`,{}),reject:(project:string,plan:string,item:string)=>request<PreparedProjectAction>(project,`/ai/actions/${encodeURIComponent(plan)}/${encodeURIComponent(item)}/reject`,{}),listDirectories:(project:string)=>request<{directories:{path:string}[]}>(project,'/directories'),restorePlan:(project:string,version:string)=>request<RestorePlan>(project,`/versions/${encodeURIComponent(version)}/restore-plan`)};

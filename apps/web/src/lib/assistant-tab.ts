import type { AssistantDrafts } from './assistant-drafts';
import type { ProjectActionInput } from './project-actions';
import type { ProjectTextPlan } from './project-edit-plan';
export interface AssistantTabState {
  drafts: AssistantDrafts;
  threadId: string;
  modelChoice: string;
  filePath: string;
  plan: ProjectTextPlan | null;
  fileActions: ProjectActionInput[];
  target: { from: number; to: number; text: string } | null;
}
export interface EditorTabContext { revision:number;fileId:string|null;filePath:string;selection:{from:number;to:number;text:string}|null }
export function validEditorContext(value:unknown):value is EditorTabContext {
 if(!value||typeof value!=='object')return false;const v=value as EditorTabContext;
 if(!Number.isSafeInteger(v.revision)||v.revision<0||typeof v.filePath!=='string'||v.filePath.length>1024||(v.fileId!==null&&(typeof v.fileId!=='string'||v.fileId.length>128)))return false;
 const r=v.selection;return r===null||!!r&&Number.isSafeInteger(r.from)&&Number.isSafeInteger(r.to)&&r.from>=0&&r.to>r.from&&typeof r.text==='string'&&r.text.length<=16000&&r.to-r.from===r.text.length;
}
export function restoreInEditor(state:AssistantTabState,editor:Omit<EditorTabContext,'revision'>):AssistantTabState {return {...state,filePath:editor.filePath,target:editor.selection};}
export type AssistantTabKind = 'hello'|'state'|'active'|'grant'|'sync'|'return'|'return-ack'|'request-return'|'closed'|'parent-closed'|'busy'|'editor-context'|'context-request'|'open-file';
export interface AssistantTabMessage { version:1;session:string;userId:string;projectId:string;sender:'parent'|'child';kind:AssistantTabKind;state?:AssistantTabState;editor?:EditorTabContext;filePath?:string;mode?:'right'|'bottom'|'float' }
const kinds = new Set<AssistantTabKind>(['hello','state','active','grant','sync','return','return-ack','request-return','closed','parent-closed','busy','editor-context','context-request','open-file']);
export function assistantTabSession(search:string):string|null {
  const query=new URLSearchParams(search), id=query.get('bridge');
  return query.get('assistant')==='tab'&&id&&/^[a-f\d-]{36}$/i.test(id)?id:null;
}
export function assistantTabChannel(userId:string,projectId:string,session:string):string{return `prims-assistant-tab:${encodeURIComponent(userId)}:${encodeURIComponent(projectId)}:${session}`;}
export function assistantTabUrl(origin:string,projectId:string,session:string,fileId?:string):string{
  const url=new URL(`/project/${encodeURIComponent(projectId)}`,origin);
  url.searchParams.set('assistant','tab');url.searchParams.set('bridge',session);if(fileId)url.searchParams.set('file',fileId);return url.href;
}
export function editorTabUrl(origin:string,projectId:string,filePath?:string):string {
 const url=new URL(`/project/${encodeURIComponent(projectId)}`,origin);if(filePath)url.searchParams.set('filePath',filePath);return url.href;
}
export function validTabMessage(value:unknown,userId:string,projectId:string,session:string,sender:'parent'|'child'):value is AssistantTabMessage {
 if(!value||typeof value!=='object')return false;const message=value as AssistantTabMessage;
 return message.version===1&&message.session===session&&message.userId===userId&&message.projectId===projectId&&message.sender===sender&&kinds.has(message.kind);
}
export function validTabState(value:unknown):value is AssistantTabState {
 if(!value||typeof value!=='object')return false;const state=value as AssistantTabState;
 if(typeof state.threadId!=='string'||typeof state.modelChoice!=='string'||typeof state.filePath!=='string'||!state.drafts||typeof state.drafts.panel!=='string'||typeof state.drafts.selection!=='string'||state.drafts.panel.length>16000||state.drafts.selection.length>4000||!Array.isArray(state.fileActions)||state.fileActions.length>12)return false;
 if(state.target!==null&&(!state.target||!Number.isInteger(state.target.from)||!Number.isInteger(state.target.to)||state.target.from<0||state.target.to<state.target.from||typeof state.target.text!=='string'||state.target.text.length>16000))return false;
 if(state.plan!==null&&(!state.plan||typeof state.plan.id!=='string'||state.plan.threadId!==state.threadId||!Array.isArray(state.plan.edits)||state.plan.edits.length>48||state.plan.edits.some(edit=>!edit||typeof edit.id!=='string'||typeof edit.filePath!=='string'||typeof edit.before!=='string'||typeof edit.after!=='string'||!['pending','applied','rejected','undone'].includes(edit.state))))return false;
 try{return JSON.stringify(state).length<=240000;}catch{return false;}
}

export const assistantReturnKey=(userId:string,projectId:string)=>`prims-assistant-return:${encodeURIComponent(userId)}:${encodeURIComponent(projectId)}`;

export const canContinueWithoutParent=(ownership:string|null)=>ownership==='closed'||ownership===null;

export const handoffDisposition=(activated:boolean,away:boolean):'start'|'grant'|'ignore'=>away?'grant':activated?'ignore':'start';

export const assistantCloseKey=(userId:string,projectId:string,session:string)=>assistantTabChannel(userId,projectId,session)+':closing-state';

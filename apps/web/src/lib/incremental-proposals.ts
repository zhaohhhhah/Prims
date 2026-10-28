import type {ProjectAIMessage} from './project-ai-context';
import {decideProjectEdit,type ProjectTextPlan} from './project-edit-plan';
export interface CommittedProposal {id:string;filePath:string;before:string;after:string;status:'pending_review'}
let lastOrder=0;
function nextOrder(){lastOrder=Math.max(lastOrder+1,Date.now()*1000);return lastOrder;}
const validId=(value:unknown):value is string=>typeof value==='string'&&value.length>0&&value.length<=128&&!/[\u0000-\u001f\u007f]/.test(value);
export function parseCommittedProposal(value:unknown,allowed:readonly string[]):CommittedProposal {
 if(!value||typeof value!=='object')throw new Error('候选修改格式不完整');const item=value as Record<string,unknown>;
 if(!validId(item.id)||typeof item.filePath!=='string'||!allowed.includes(item.filePath)||item.filePath.startsWith('/')||item.filePath.includes('\\')||item.filePath.includes(':')||item.filePath.split('/').some(part=>!part||part==='.'||part==='..')||item.filePath.split('/')[0]==='.git'||new TextEncoder().encode(item.filePath).length>512||/[\u0000-\u001f\u007f]/.test(item.filePath))throw new Error('候选只允许当前项目已授权的文本文件');
 if(typeof item.before!=='string'||!item.before.trim()||item.before.length>8388608||typeof item.after!=='string'||item.after.length>8388608||item.status!=='pending_review')throw new Error('候选修改为空、过长或未完整提交');
 return {id:item.id,filePath:item.filePath,before:item.before,after:item.after,status:'pending_review'};
}
export function mergeCommittedProposal(plan:ProjectTextPlan|null,requestId:string,threadId:string,proposal:CommittedProposal,selectionId?:string):{plan:ProjectTextPlan;added:boolean}{
 if(!validId(requestId))throw new Error('候选请求标识无效');
 const current=plan?.id===requestId?plan:{id:requestId,threadId,...(selectionId?{selectionId}:{}),edits:[]};
 const sameId=current.edits.find(edit=>edit.id===proposal.id);
 if(sameId){if(sameId.filePath!==proposal.filePath||sameId.before!==proposal.before||(sameId.sourceAfter??sameId.after)!==proposal.after)throw new Error('重复候选的内容不一致，已保留原建议');return {plan:current,added:false};}
 if(current.edits.some(edit=>edit.filePath===proposal.filePath&&edit.before===proposal.before&&(edit.sourceAfter??edit.after)===proposal.after))return {plan:current,added:false};
 const edits=[...current.edits,{id:proposal.id,filePath:proposal.filePath,before:proposal.before,after:proposal.after,sourceAfter:proposal.after,state:'pending' as const}];
 if(new TextEncoder().encode(JSON.stringify(edits.map(({filePath,before,sourceAfter})=>({filePath,before,after:sourceAfter})))).length>8388608)throw new Error('本轮候选总量过长，请分次修改');
 return {plan:{...current,edits},added:true};
}
export function proposalRunMessage(requestId:string):ProjectAIMessage{return {id:crypto.randomUUID(),role:'action',content:'本轮 AI 修改仍需用户逐段确认；待审候选不能视作已应用。',raw:JSON.stringify({kind:'prims.proposal_run',requestId,order:nextOrder()})};}
export function proposalMessage(requestId:string,proposal:CommittedProposal):ProjectAIMessage{return {id:crypto.randomUUID(),role:'action',content:`AI 提交了 ${proposal.filePath} 的完整待审候选，尚未应用。`,filePath:proposal.filePath,raw:JSON.stringify({kind:'prims.proposal',requestId,proposal,order:nextOrder()})};}
export function decisionRaw(requestId:string,proposalId:string,state:'applied'|'rejected'|'undone'){return JSON.stringify({kind:'prims.proposal_decision',requestId,proposalId,state,order:nextOrder()});}
/** Restore display state only; every pending hunk still needs actual-editor validation. */
export function restoreCommittedPlan(messages:readonly ProjectAIMessage[],allowed:readonly string[],threadId:string):ProjectTextPlan|null{
 let requestId:string|null=null;const events:Record<string,unknown>[]=[];
 for(const message of messages){if(message.role!=='action'||!message.raw)continue;try{const value=JSON.parse(message.raw) as Record<string,unknown>;if(value&&typeof value==='object'&&typeof value.kind==='string'&&validId(value.requestId)){events.push(value);}}catch{/* Unstructured historical actions are reference only. */}}
 events.sort((a,b)=>(typeof a.order==='number'?a.order:0)-(typeof b.order==='number'?b.order:0));for(const event of events){if(typeof event.order==='number'&&Number.isSafeInteger(event.order))lastOrder=Math.max(lastOrder,event.order);if(event.kind==='prims.proposal_run')requestId=event.requestId as string;}
 if(!requestId)return null;let plan:ProjectTextPlan|null=null;
 for(const event of events){if(event.requestId!==requestId)continue;try{if(event.kind==='prims.proposal'){plan=mergeCommittedProposal(plan,requestId,threadId,parseCommittedProposal(event.proposal,allowed)).plan;}else if(event.kind==='prims.proposal_decision'&&plan&&validId(event.proposalId)&&['applied','rejected','undone'].includes(String(event.state))){plan=decideProjectEdit(plan,event.proposalId,event.state as 'applied'|'rejected'|'undone');}}catch{/* Unsafe, unknown or inconsistent history never becomes an executable proposal. */}}
 return plan;
}

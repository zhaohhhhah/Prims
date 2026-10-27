export interface ProgressEvent {status?:unknown;file?:unknown;publicStatus?:unknown;proposal?:unknown;plan?:unknown;meta?:unknown}
export interface AssistantProgress {current:string;completed:string[]}
export const emptyAssistantProgress=():AssistantProgress=>({current:'',completed:[]});
function publicText(value:unknown):string|null {
 if(typeof value!=='string')return null;const text=value.replace(/[\u0000-\u001f\u007f]/g,' ').replace(/\s+/g,' ').trim();
 if(!text||text.length>160||/^[{\[]/.test(text)||/[\"'](?:arguments|before|after|apiKey)[\"']\s*:/.test(text)||/```|<\/?(?:think|analysis)>|(?:chain.?of.?thought|reasoning_content|思维链|内心推理|contextSummary|autoSummary)|Bearer\s|sk-[\w-]{8}/i.test(text))return null;
 if(/^(?:模型正在结合项目上下文生成|正在整理上下文并生成|正在生成(?:答复|回复|内容)?|generating(?: response)?|thinking|正在(?:分析|思考|处理))[… .\s]*$/i.test(text))return null;
 if(/(?:已|正在)(?:自动)?(?:应用|写入|保存).*(?:修改|候选|文件)/.test(text))return null;
 return text.replace('正在按Codex方法整理上下文','正在整理上下文');
}
function planSteps(value:unknown):Record<string,unknown>[] {const block=value&&typeof value==='object'?value as Record<string,unknown>:null;const steps=Array.isArray(value)?value:block?.plan??block?.steps;return Array.isArray(steps)?steps.filter((step):step is Record<string,unknown>=>!!step&&typeof step==='object'):[];}
export function updateAssistantProgress(previous:AssistantProgress,event:ProgressEvent):AssistantProgress {
 const meta=event.meta&&typeof event.meta==='object'?event.meta as Record<string,unknown>:null;
 const steps=planSteps(event.plan),active=steps.find(step=>step.status==='in_progress');
 let current=publicText(event.publicStatus)??publicText(event.status)??publicText(meta?.publicStatus)??publicText(active?.step??active?.title)??previous.current;
 const completed=[...previous.completed];const add=(value:string|null)=>{if(!value||/上下文|记忆|压缩|摘要|生成答复|生成回复/.test(value))return;const old=completed.indexOf(value);if(old>=0)completed.splice(old,1);completed.push(value);};
 for(const step of steps)if(step.status==='completed'){const label=publicText(step.step??step.title);if(label&&!/应用|写入|保存/.test(label))add(`已完成：${label}`);}
 const status=publicText(event.status);if(status&&/^(?:已|完成了|结束了)/.test(status))add(status);
 if(event.proposal&&typeof event.proposal==='object'&&(event.proposal as {status?:unknown}).status==='pending_review'){add('已收到完整待审修改');if(!publicText(event.status)&&!active)current='正在继续处理后续修改';}
 else if(!publicText(event.status)&&typeof event.file==='string'&&event.file.length<=160&&!event.file.startsWith('/')&&!event.file.includes('..')&&!/[\u0000-\u001f]/.test(event.file))current=`正在读取 ${event.file}`;
 return {current,completed:completed.slice(-3)};
}

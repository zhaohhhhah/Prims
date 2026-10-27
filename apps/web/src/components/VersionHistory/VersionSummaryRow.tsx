import {Button} from '@scribe/ui';
import {useQueryClient} from '@tanstack/react-query';
import {useEffect,useRef,useState} from 'react';
import {summaryRequestAction,versionSummariesApi,versionSummariesKey,type VersionSummary} from '../../lib/version-summaries';
import {useAuthStore} from '../../stores/auth';
export function VersionSummaryRow({projectId,versionId,item,readOnly}:{projectId:string;versionId:string;item:VersionSummary|undefined;readOnly:boolean}){
 const userId=useAuthStore(state=>state.user?.id),cache=useQueryClient(),key=`${userId}:${projectId}:${versionId}`;const owner=useRef({key});if(owner.current.key!==key)owner.current={key};const [busy,setBusy]=useState(false),[error,setError]=useState(''),[now,setNow]=useState(Date.now);
 const action=summaryRequestAction(item,readOnly,now);
 useEffect(()=>{setBusy(false);setError('');setNow(Date.now());},[key]);
 useEffect(()=>{if(item?.status==='queued'||item?.status==='running'||item?.status==='succeeded')setError('');},[item?.status,item?.updatedAt]);
 useEffect(()=>{if(!action?.disabled||action.reason!=='请稍后再试')return;const timestamp=item?.updatedAt?Date.parse(item.updatedAt):NaN;if(!Number.isFinite(timestamp))return;const timeout=window.setTimeout(()=>setNow(Date.now()),Math.max(50,Math.min(30000,timestamp+30050-Date.now())));return()=>window.clearTimeout(timeout);},[item?.updatedAt,action?.disabled,action?.reason,now]);
 async function generate(){if(busy||!action||action.disabled)return;const scope=owner.current;setBusy(true);setError('');try{await versionSummariesApi.generate(projectId,versionId,action.retry,userId);if(owner.current!==scope)return;await cache.invalidateQueries({queryKey:versionSummariesKey(userId,projectId)});}catch(failure){if(owner.current===scope)setError(failure instanceof Error?failure.message:'摘要请求未完成');}finally{if(owner.current===scope)setBusy(false);}}
 const status=item?.status??'not_requested';const label=status==='queued'?'等待生成摘要…':status==='running'?'正在生成摘要…':status==='failed'?'摘要生成失败':status==='skipped'?'摘要已跳过':status==='succeeded'?'摘要已生成':'';
 return <div className="space-y-1 px-2 pb-2 text-xs">{status==='succeeded'&&item?.summary?<p className="whitespace-pre-wrap break-words leading-relaxed text-foreground">{item.summary}</p>:label?<p className="text-muted-foreground" role={status==='queued'||status==='running'?'status':undefined}>{label}</p>:null}{(status==='failed'||status==='skipped')&&item?.error&&<p className="break-words text-muted-foreground">{item.error}</p>}{action&&<Button type="button" variant="ghost" size="sm" className="h-6 px-2 text-xs" disabled={busy||action.disabled} title={action.reason||undefined} onClick={()=>{void generate();}}>{busy?'正在提交…':action.label}</Button>}{action?.disabled&&action.reason&&<span className="ml-1 text-[10px] text-muted-foreground">{action.reason}</span>}{error&&<p role="alert" className="text-destructive">{error}</p>}</div>;
}

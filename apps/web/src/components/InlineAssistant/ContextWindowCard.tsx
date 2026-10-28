import {Button,Input,Label} from '@scribe/ui';
import {useState} from 'react';
import {useQueryClient} from '@tanstack/react-query';
import {DEFAULT_CONTEXT_WINDOW,modelProfilesApi,modelProfilesQueryKey,parseContextWindowsInput,useModelProfiles} from '../../lib/model-profiles';
import {useAuthStore} from '../../stores/auth';
export function ContextWindowCard({profileId,model}:{profileId:string;model:string}){
 const profiles=useModelProfiles(),userId=useAuthStore(s=>s.user?.id),cache=useQueryClient();
 const configured=profiles.data?.profiles.find(p=>p.id===profileId)?.contextWindows?.[model]??DEFAULT_CONTEXT_WINDOW;
 const [tokens,setTokens]=useState(String(Math.max(16384,Math.floor(configured/2)))),[busy,setBusy]=useState(false),[error,setError]=useState(''),[saved,setSaved]=useState(false);
 async function save(){setBusy(true);setError('');try{const windows=parseContextWindowsInput([model],{[model]:tokens});const count=windows[model];if(!count)throw new Error('请填写上下文长度');await modelProfilesApi.updateContextWindow(profileId,model,count);await cache.invalidateQueries({queryKey:modelProfilesQueryKey(userId)});setSaved(true);}catch(error){setError(error instanceof Error?error.message:'保存失败');}finally{setBusy(false);}}
 return <div role="alert" className="space-y-3 rounded-lg border bg-muted/30 p-3"><p className="text-sm font-medium">上下文超过模型接口上限</p><p className="break-all text-xs text-muted-foreground">{model} · 当前配置 {configured.toLocaleString()} tokens</p><div className="flex flex-wrap items-end gap-2"><div className="space-y-1"><Label htmlFor="context-window-recovery">上下文长度（tokens）</Label><Input id="context-window-recovery" type="number" min={16384} max={2097152} step={1} value={tokens} onChange={e=>{setTokens(e.target.value);setSaved(false);setError('');}} disabled={busy}/></div><Button size="sm" disabled={busy} onClick={()=>void save()}>{busy?'保存中…':'保存长度'}</Button></div>{saved&&<p role="status" className="text-xs">已保存。重新发送即可，原文和待审建议已保留。</p>}{error&&<p className="text-xs text-destructive">{error}</p>}</div>;
}

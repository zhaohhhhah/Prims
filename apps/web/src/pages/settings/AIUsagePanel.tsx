import {UsageBarChart,cacheUsageBarRows,inputTokenParts} from './UsageBarChart';
import {UsageHeatmap} from './UsageHeatmap';
import {Button} from '@scribe/ui';
import {useQuery} from '@tanstack/react-query';
import {useState} from 'react';
import {BarChart3,Grid2X2} from 'lucide-react';
import {supabase,API_URL} from '../../lib/supabase';
import {useAuthStore} from '../../stores/auth';
type Counts={inputTokens:number;outputTokens:number;totalTokens:number;cachedTokens:number;requests:number;missingUsageRequests:number;partialUsageRequests:number};
type Usage={days:number;timeZone:string;totals:Counts;buckets:(Counts&{label:string})[]};
const number=(n:number)=>n.toLocaleString();
export function AIUsagePanel(){
 const userId=useAuthStore(s=>s.user?.id),[days,setDays]=useState(7),[view,setView]=useState<'grid'|'bars'>('grid');
 const zone=Intl.DateTimeFormat().resolvedOptions().timeZone||'UTC';
 const query=useQuery({queryKey:['ai-usage',userId,days,zone,'heatmap-v3'],enabled:!!userId,refetchInterval:60000,queryFn:async()=>{
  const {data:{session}}=await supabase.auth.getSession();if(!session||session.user.id!==userId)throw new Error('请重新登录');
  const response=await fetch(`${API_URL}/api/ai/usage?days=${days}&timeZone=${encodeURIComponent(zone)}&resolution=heatmap`,{cache:'no-store',headers:{Authorization:`Bearer ${session.access_token}`}});if(!response.ok)throw new Error('暂时无法加载用量统计');return response.json() as Promise<Usage>;
 }});
 const value=query.data,counts=value?.totals,inputParts=counts?inputTokenParts(counts):null;
 return <section className="space-y-4 border-t pt-5" aria-labelledby="ai-usage-title"><div className="flex flex-wrap items-center justify-between gap-3"><h3 id="ai-usage-title" className="text-base font-medium">用量统计</h3><div className="flex gap-1" aria-label="统计时间范围">{[[1,'当天'],[7,'近7天'],[30,'近一个月']].map(([count,label])=><Button key={count} size="sm" variant={days===count?'default':'ghost'} aria-pressed={days===count} onClick={()=>setDays(Number(count))}>{label}</Button>)}</div></div>
 {query.isPending?<p role="status" className="text-sm text-muted-foreground">正在加载…</p>:query.isError?<div role="alert" className="text-sm text-destructive">{query.error.message}<Button size="sm" variant="ghost" onClick={()=>void query.refetch()}>重试</Button></div>:value&&counts&&<>
 <div className="grid grid-cols-3 gap-3">{[['总 Tokens',counts.totalTokens],['输入',counts.inputTokens],['输出',counts.outputTokens]].map(([label,count])=><div key={label} className="min-w-0 rounded-lg border bg-muted/20 p-3"><p className="text-xs text-muted-foreground">{label}</p><p className="mt-1 break-all text-xl font-medium tabular-nums">{number(Number(count))}</p>{label==='输入'&&inputParts&&<dl className="mt-2 space-y-1 text-xs text-muted-foreground"><div className="flex flex-wrap justify-between gap-x-2"><dt>缓存命中</dt><dd className="tabular-nums">{number(inputParts.cached)}</dd></div><div className="flex flex-wrap justify-between gap-x-2"><dt title="接口未上报缓存状态时按未命中归类">缓存未命中</dt><dd className="tabular-nums">{number(inputParts.uncached)}</dd></div></dl>}</div>)}</div>
 <div className="flex flex-wrap items-center justify-between gap-2"><p className="text-xs text-muted-foreground">{value.buckets[0]?.label.slice(0,10)}{days>1?` — ${value.buckets.at(-1)?.label.slice(0,10)}`:''} · {number(counts.requests)} 次请求</p><div className="flex gap-1" aria-label="图表样式"><Button size="sm" variant={view==='grid'?'secondary':'ghost'} aria-label="格子图" aria-pressed={view==='grid'} onClick={()=>setView('grid')}><Grid2X2 size={15}/></Button><Button size="sm" variant={view==='bars'?'secondary':'ghost'} aria-label="条形图" aria-pressed={view==='bars'} onClick={()=>setView('bars')}><BarChart3 size={15}/></Button></div></div>
 {view==='grid'?<UsageHeatmap rows={value.buckets} selectedDays={days}/>:<UsageBarChart rows={cacheUsageBarRows(value.buckets,days)} hourly={days===1}/>}
 {(counts.missingUsageRequests>0||counts.partialUsageRequests>0)&&<p className="text-xs text-muted-foreground">{counts.missingUsageRequests>0?`${number(counts.missingUsageRequests)} 次请求未返回用量，`:''}{counts.partialUsageRequests>0?`${number(counts.partialUsageRequests)} 次仅返回部分用量，`:''}总量仅汇总接口已报告的数据。</p>}
 {!counts.requests&&<p className="text-xs text-muted-foreground">此时间段暂无记录，使用模型后会自动更新。</p>}
 </>}
 </section>;
}

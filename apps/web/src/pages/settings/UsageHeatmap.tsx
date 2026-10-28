export type HeatmapDay={label:string;totalTokens:number;inputTokens:number;outputTokens:number};
const DAY=86400000;
export function calendarWeeks(rows:HeatmapDay[]):(HeatmapDay|null)[][]{
 if(!rows.length)return [];
 const first=Date.parse(rows[0]!.label.slice(0,10)+'T00:00:00Z'),last=Date.parse(rows.at(-1)!.label.slice(0,10)+'T00:00:00Z');
 const start=first-((new Date(first).getUTCDay()+6)%7)*DAY;
 const byDate=new Map(rows.map(row=>[row.label.slice(0,10),row]));
 return Array.from({length:Math.ceil((last-start+DAY)/(7*DAY))},(_,week)=>Array.from({length:7},(_,day)=>byDate.get(new Date(start+(week*7+day)*DAY).toISOString().slice(0,10))??null));
}
export function UsageHeatmap({rows,selectedDays}:{rows:HeatmapDay[];selectedDays:number}){
 const weeks=calendarWeeks(rows),max=Math.max(1,...rows.map(r=>r.totalTokens)),last=rows.at(-1)?.label.slice(0,10)??'',selectedStart=Date.parse(last+'T00:00:00Z')-(selectedDays-1)*DAY;
 const color=(level:number)=>level===0?'hsl(var(--muted-foreground) / .13)':`color-mix(in srgb, var(--prims-accent, hsl(var(--primary))) ${[0,25,45,65,90][level]}%, var(--prims-ground, hsl(var(--background))))`;
 const level=(tokens:number)=>tokens===0?0:Math.max(1,Math.ceil(Math.sqrt(tokens/max)*4));
 const width=weeks.length*13+36;
 return <div className="space-y-2"><div className="overflow-x-auto" aria-label="按日统计的 Token 用量日历"><svg viewBox={`0 0 ${width} 124`} className="block w-full" style={{minWidth:Math.min(width,550)}} role="img" aria-label="近一年 Token 用量热力图，列为星期、行按周一至周日排列，颜色越深用量越多"><title>近一年 Token 用量</title>
 {[0,2,4].map(day=><text key={day} x={0} y={29+day*13} fill="currentColor" className="text-muted-foreground" fontSize={10}>{['一','二','三','四','五','六','日'][day]}</text>)}
 {weeks.map((week,index)=>{const month=week.find(day=>day?.label.slice(8,10)==='01')??(index===0?week.find(Boolean):null);return <g key={index} transform={`translate(${index*13+30},0)`}>{month&&<text x={0} y={12} fill="currentColor" className="text-muted-foreground" fontSize={11}>{Number(month.label.slice(5,7))}月</text>}{week.map((day,row)=>day?<rect key={day.label} x={0} y={20+row*13} width={10} height={10} rx={2} fill={color(level(day.totalTokens))} opacity={Date.parse(day.label.slice(0,10)+'T00:00:00Z')<selectedStart ? .3 : 1}><title>{`${day.label.slice(0,10)}：${day.totalTokens.toLocaleString()} tokens；输入 ${day.inputTokens.toLocaleString()}，输出 ${day.outputTokens.toLocaleString()}`}</title></rect>:null)}</g>;})}
 </svg></div><div className="flex items-center justify-between gap-3 text-xs text-muted-foreground"><span>近一年 · 每格一天</span><span className="flex items-center gap-1.5"><span>少</span>{[0,1,2,3,4].map(l=><span key={l} className="inline-block h-2.5 w-2.5 rounded-sm" style={{background:color(l)}}/>)}<span>多</span></span></div></div>;
}

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
 const hourly=selectedDays===1,last=rows.at(-1)?.label.slice(0,10);
 const visible=hourly?rows.filter(row=>row.label.slice(0,10)===last):rows.slice(-selectedDays);
 const weeks=calendarWeeks(visible),max=Math.max(1,...visible.map(r=>r.totalTokens));
 const range=hourly?'当天':selectedDays===7?'近7天':'近一个月';
 const color=(level:number)=>level===0?'hsl(var(--muted-foreground) / .13)':`color-mix(in srgb, var(--prims-accent, hsl(var(--primary))) ${[0,25,45,65,90][level]}%, var(--prims-ground, hsl(var(--background))))`;
 const level=(tokens:number)=>tokens===0?0:Math.max(1,Math.ceil(Math.sqrt(tokens/max)*4));
 const title=(day:HeatmapDay)=>`${hourly?day.label:day.label.slice(0,10)}：${day.totalTokens.toLocaleString()} tokens；输入 ${day.inputTokens.toLocaleString()}，输出 ${day.outputTokens.toLocaleString()}`;
 const width=hourly?visible.length*18:weeks.length*18+40;
 return <div className="space-y-2"><div className="overflow-x-auto" aria-label="所选时间范围的 Token 用量热力图"><svg width={width} height={hourly?52:152} viewBox={`0 0 ${width} ${hourly?52:152}`} className="block max-w-full" role="img" aria-label={`${range} Token 用量热力图，每格一${hourly?'小时':'天'}`}><title>{range} Token 用量</title>
 {hourly?visible.map((day,index)=><g key={day.label} transform={`translate(${index*18},0)`}>{index%6===0&&<text x={0} y={12} fill="currentColor" className="text-muted-foreground" fontSize={11}>{day.label.slice(11,13)}时</text>}<rect y={20} width={14} height={14} rx={2} fill={color(level(day.totalTokens))}><title>{title(day)}</title></rect></g>):<>
 {[0,2,4].map(day=><text key={day} x={0} y={30+day*18} fill="currentColor" className="text-muted-foreground" fontSize={11}>{['一','二','三','四','五','六','日'][day]}</text>)}
 {weeks.map((week,index)=>{const month=week.find(day=>day?.label.slice(8,10)==='01')??(index===0&&!weeks.slice(1,3).some(next=>next.some(day=>day?.label.slice(8,10)==='01'))?week.find(Boolean):null);return <g key={index} transform={`translate(${index*18+34},0)`}>{month&&<text x={0} y={12} fill="currentColor" className="text-muted-foreground" fontSize={11}>{Number(month.label.slice(5,7))}月</text>}{week.map((day,row)=>day?<rect key={day.label} y={20+row*18} width={14} height={14} rx={2} fill={color(level(day.totalTokens))}><title>{title(day)}</title></rect>:null)}</g>;})}</>}
 </svg></div><div className="flex items-center justify-between gap-3 text-xs text-muted-foreground"><span>{range} · 每格一{hourly?'小时':'天'}</span><span className="flex items-center gap-1.5"><span>少</span>{[0,1,2,3,4].map(l=><span key={l} className="inline-block h-2.5 w-2.5 rounded-sm" style={{background:color(l)}}/>)}<span>多</span></span></div></div>;
}

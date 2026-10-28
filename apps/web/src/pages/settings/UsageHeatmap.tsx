import {useEffect,useRef,useState} from 'react';
import {usageTickIndices} from './UsageBarChart';
export type HeatmapDay={label:string;totalTokens:number;inputTokens:number;outputTokens:number};
const DAY=86400000;
export function calendarWeeks(rows:HeatmapDay[]):(HeatmapDay|null)[][]{
 if(!rows.length)return [];
 const first=Date.parse(rows[0]!.label.slice(0,10)+'T00:00:00Z'),last=Date.parse(rows.at(-1)!.label.slice(0,10)+'T00:00:00Z');
 const start=first-((new Date(first).getUTCDay()+6)%7)*DAY,byDate=new Map(rows.map(row=>[row.label.slice(0,10),row]));
 return Array.from({length:Math.ceil((last-start+DAY)/(7*DAY))},(_,week)=>Array.from({length:7},(_,day)=>byDate.get(new Date(start+(week*7+day)*DAY).toISOString().slice(0,10))??null));
}
export function coarsenHeatmapRows(rows:HeatmapDay[],_days:number,minutes:number):HeatmapDay[]{
 const groups=new Map<string,HeatmapDay>();
 for(const value of rows){
  const total=Number(value.label.slice(11,13))*60+Number(value.label.slice(14,16)),start=Math.floor(total/minutes)*minutes;
  const label=value.label.slice(0,10)+' '+String(Math.floor(start/60)).padStart(2,'0')+':'+String(start%60).padStart(2,'0');
  const item=groups.get(label)??{label,totalTokens:0,inputTokens:0,outputTokens:0};
  item.totalTokens+=value.totalTokens;item.inputTokens+=value.inputTokens;item.outputTokens+=value.outputTokens;groups.set(label,item);
 }
 return [...groups.values()];
}
export function heatmapCells(rows:HeatmapDay[],days:number,minutes=days===1?6:days===7?60:360){
 rows=coarsenHeatmapRows(rows,days,minutes);
 const dates=[...new Set(rows.map(row=>row.label.slice(0,10)))].slice(-days),allowed=new Set(dates),visible=rows.filter(row=>allowed.has(row.label.slice(0,10)));
 if(days===1)return visible.map(value=>({value,column:Number(value.label.slice(11,13)),row:Math.floor(Number(value.label.slice(14,16))/minutes),slot:0}));
 if(days===7)return visible.map(value=>({value,column:Math.floor(Number(value.label.slice(11,13))*60/minutes),row:dates.indexOf(value.label.slice(0,10)),slot:0}));
 const first=Date.parse(dates[0]+'T00:00:00Z'),start=first-((new Date(first).getUTCDay()+6)%7)*DAY;
 return visible.map(value=>{const day=Math.round((Date.parse(value.label.slice(0,10)+'T00:00:00Z')-start)/DAY);return {value,column:day%7,row:Math.floor(day/7),slot:Math.floor(Number(value.label.slice(11,13))*60/minutes)};});
}
export function usageBarRows(rows:HeatmapDay[],days:number):HeatmapDay[]{
 const grouped=new Map<string,HeatmapDay>();for(const value of rows){const label=days===1?value.label.slice(0,13)+':00':value.label.slice(0,10);const item=grouped.get(label)??{label,totalTokens:0,inputTokens:0,outputTokens:0};item.totalTokens+=value.totalTokens;item.inputTokens+=value.inputTokens;item.outputTokens+=value.outputTokens;grouped.set(label,item);}return [...grouped.values()];
}
function bucketTitle(value:HeatmapDay,minutes:number){
 const hour=Number(value.label.slice(11,13)),minute=Number(value.label.slice(14,16)),end=hour*60+minute+minutes-1;
 return `${value.label}–${String(Math.floor(end/60)).padStart(2,'0')}:${String(end%60).padStart(2,'0')}`;
}
export function UsageHeatmap({rows,selectedDays}:{rows:HeatmapDay[];selectedDays:number}){
 const [overrides,setOverrides]=useState<Record<number,number>>({}),[availableWidth,setAvailableWidth]=useState<number|null>(null);const surface=useRef<HTMLDivElement>(null);
 useEffect(()=>{const element=surface.current;if(!element)return;const update=()=>{if(element.clientWidth>0)setAvailableWidth(Math.floor(element.clientWidth));};update();const observer=new ResizeObserver(update);observer.observe(element);return()=>observer.disconnect();},[]);
 const minutes=overrides[selectedDays]??(selectedDays===1?6:selectedDays===7?60:360),cells=heatmapCells(rows,selectedDays,minutes),max=Math.max(1,...cells.map(cell=>cell.value.totalTokens));
 const dates=[...new Set(cells.map(cell=>cell.value.label.slice(0,10)))],range=selectedDays===1?'当天':selectedDays===7?'近7天':'近一个月';
 const minute=selectedDays===1,week=selectedDays===7,month=selectedDays===30,columns=minute?24:week?1440/minutes:7,rowCount=minute?60/minutes:week?dates.length:Math.max(1,...cells.map(cell=>cell.row+1));
 const left=minute?48:week?52:8,top=30,width=Math.max(availableWidth??600,month?330:420),stepX=(width-left-4)/columns;
 // Calendar days contain a compact time grid; cells stay small as the panel grows.
 const slots=1440/minutes,miniColumns=slots===24?6:4,miniRows=Math.ceil(slots/miniColumns),miniGap=3;
 const cellWidth=month?Math.min(16,Math.max(5,(stepX-10-(miniColumns-1)*miniGap)/miniColumns)):stepX-4;
 const cellHeight=month?cellWidth:Math.min(24,Math.max(13,cellWidth)),stepY=month?miniRows*(cellHeight+miniGap)+24:cellHeight+7,height=top+rowCount*stepY+6;
 const yTicks=new Set(usageTickIndices(rowCount));
 const color=(level:number)=>level===0?'hsl(var(--muted-foreground) / .13)':`color-mix(in srgb, var(--prims-accent, hsl(var(--primary))) ${[0,25,45,65,90][level]}%, var(--prims-ground, hsl(var(--background))))`;
 const level=(tokens:number)=>tokens===0?0:Math.max(1,Math.ceil(Math.sqrt(tokens/max)*4));
 const caption=minute?`每格${minutes}分钟`:`每格${minutes/60}小时`,axes=minute?'横轴小时，纵轴分钟':week?'横轴小时，纵轴日期':'横轴周一至周日，按周换行';
 return <div className="space-y-2"><div className="overflow-x-auto" ref={surface}><svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} className="block" role="img" aria-label={`${range} Token 用量热力图，${axes}，${caption}`}><title>{`${range} Token 用量`}</title>
 {minute&&<text x={0} y={15} fill="currentColor" className="text-muted-foreground" fontSize={11}>分钟</text>}
 {Array.from({length:columns},(_,column)=>{if(!month&&column%Math.max(1,Math.round(columns/6))!==0&&column!==columns-1)return null;const label=month?['周一','周二','周三','周四','周五','周六','周日'][column]:String(minute?column:column*minutes/60).padStart(2,'0');return <text key={column} x={left+column*stepX+(month?stepX/2:cellWidth/2)} y={16} textAnchor="middle" fill="currentColor" className="text-muted-foreground" fontSize={11} data-heatmap-x>{label}</text>;})}
 {(minute||week)&&Array.from({length:rowCount},(_,row)=>{if(!yTicks.has(row))return null;const first=row*minutes,last=first+minutes-1;return <text key={row} x={left-8} y={top+row*stepY+11} textAnchor="end" fill="currentColor" className="text-muted-foreground" fontSize={11} data-heatmap-y>{minute?`${String(first).padStart(2,'0')}–${String(last).padStart(2,'0')}`:dates[row]?.slice(5).replace('-','/')}</text>;})}
 {month&&cells.filter(cell=>cell.slot===0).map(({value,column,row})=><text key={value.label} x={left+column*stepX+stepX/2} y={top+row*stepY+10} textAnchor="middle" fill="currentColor" className="text-muted-foreground" fontSize={10}>{value.label.slice(5,10).replace('-','/')}</text>)}
 {cells.map(({value,column,row,slot})=><rect key={value.label} x={left+column*stepX+(month?(stepX-(miniColumns*(cellWidth+miniGap)-miniGap))/2+(slot%miniColumns)*(cellWidth+miniGap):0)} y={top+row*stepY+(month?16+Math.floor(slot/miniColumns)*(cellHeight+miniGap):0)} width={cellWidth} height={cellHeight} rx={2} fill={color(level(value.totalTokens))} data-heatmap-column={column} data-heatmap-row={row} data-heatmap-slot={slot} data-heatmap-bucket={value.label}><title>{`${bucketTitle(value,minutes)}：${value.totalTokens.toLocaleString()} tokens；输入 ${value.inputTokens.toLocaleString()}，输出 ${value.outputTokens.toLocaleString()}`}</title></rect>)}
 </svg></div><div className="flex flex-wrap items-center justify-between gap-3 text-xs text-muted-foreground"><span>{range} · {caption}</span><label className="flex items-center gap-2">粒度<select aria-label="热力图粒度" className="rounded border bg-background px-2 py-1" value={minutes} onChange={event=>setOverrides(previous=>({...previous,[selectedDays]:Number(event.target.value)}))}>{(minute?[6,12,30]:week?[60,120,240]:[360,180,60]).map(value=><option key={value} value={value}>{minute?`${value}分钟`:`${value/60}小时`}</option>)}</select></label><span className="flex items-center gap-1.5"><span>少</span>{[0,1,2,3,4].map(l=><span key={l} className="inline-block h-2.5 w-2.5 rounded-sm" style={{background:color(l)}}/>)}<span>多</span></span></div></div>;
}

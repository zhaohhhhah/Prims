import type {HeatmapDay} from './UsageHeatmap';
export function usageTickIndices(length:number,maxTicks=4):number[]{
 if(length<=maxTicks)return Array.from({length},(_,i)=>i);
 return [...new Set(Array.from({length:maxTicks},(_,i)=>Math.round(i*(length-1)/(maxTicks-1))))];
}
export function UsageBarChart({rows,hourly}:{rows:HeatmapDay[];hourly:boolean}){
 const rawMax=Math.max(2,...rows.map(row=>row.totalTokens)),power=10**Math.floor(Math.log10(rawMax)),maximum=Math.ceil(rawMax/power)*power;
 const left=52,top=16,plotWidth=570,plotHeight=176,bottom=top+plotHeight,step=plotWidth/Math.max(1,rows.length),barWidth=Math.min(26,step*.7);
 const label=(row:HeatmapDay)=>hourly?row.label.slice(11):row.label.slice(5).replace('-','/');
 const number=(value:number)=>new Intl.NumberFormat(undefined,{notation:'compact',maximumFractionDigits:1}).format(value);
 const input='var(--prims-accent, hsl(var(--primary)))',output=`color-mix(in srgb, ${input} 35%, var(--prims-ground, hsl(var(--background))))`;
 return <div className="rounded-lg border bg-muted/10 p-3"><svg viewBox="0 0 640 228" className="block w-full" role="img" aria-label="按时间排列的 Token 用量条形图，日期按间隔显示"><title>Token 用量条形图</title>
 {[0,maximum/2,maximum].map(value=>{const y=bottom-value/maximum*plotHeight;return <g key={value}><line x1={left} x2={left+plotWidth} y1={y} y2={y} stroke="hsl(var(--border))"/><text x={left-10} y={y+4} textAnchor="end" fill="currentColor" className="text-muted-foreground" fontSize={11}>{number(value)}</text></g>;})}
 {rows.map((row,index)=>{const x=left+(index+.5)*step-barWidth/2,inputHeight=row.inputTokens/maximum*plotHeight,outputHeight=row.outputTokens/maximum*plotHeight;return <g key={row.label}><title>{`${row.label}：${row.totalTokens.toLocaleString()} tokens；输入 ${row.inputTokens.toLocaleString()}，输出 ${row.outputTokens.toLocaleString()}`}</title><rect x={x} y={bottom-inputHeight} width={barWidth} height={inputHeight} fill={input}/><rect x={x} y={bottom-inputHeight-outputHeight} width={barWidth} height={outputHeight} fill={output} rx={3}/><rect x={left+index*step} y={top} width={step} height={plotHeight} fill="transparent"/></g>;})}
 {usageTickIndices(rows.length).map(index=><text key={index} x={left+(index+.5)*step} y={bottom+23} textAnchor={index===0?'start':index===rows.length-1?'end':'middle'} fill="currentColor" className="text-muted-foreground" fontSize={11} data-usage-date-tick>{label(rows[index]!)}</text>)}
 </svg><div className="flex gap-4 text-xs text-muted-foreground"><span className="flex items-center gap-1.5"><i className="h-2 w-2 rounded-full" style={{background:input}}/>输入</span><span className="flex items-center gap-1.5"><i className="h-2 w-2 rounded-full" style={{background:output}}/>输出</span></div></div>;
}

import type {HeatmapDay} from './UsageHeatmap';
export type UsageBarRow=HeatmapDay&{cachedTokens?:number};
const nonnegative=(value:number|undefined)=>Number.isFinite(value)?Math.max(0,value??0):0;
/** Cache reads are a subset of input for every normalized provider. */
export function inputTokenParts(row:Pick<UsageBarRow,'inputTokens'|'cachedTokens'>){
 const input=nonnegative(row.inputTokens),cached=Math.min(input,nonnegative(row.cachedTokens));
 return {input,cached,uncached:input-cached};
}
/** Preserve cache counters while changing the chart's visual time grouping. */
export function cacheUsageBarRows(rows:UsageBarRow[],days:number):UsageBarRow[]{
 const grouped=new Map<string,UsageBarRow>();
 for(const value of rows){
  const label=days===1?value.label.slice(0,13)+':00':value.label.slice(0,10),parts=inputTokenParts(value);
  const item=grouped.get(label)??{label,totalTokens:0,inputTokens:0,outputTokens:0,cachedTokens:0};
  item.totalTokens+=value.totalTokens;item.inputTokens+=parts.input;item.outputTokens+=value.outputTokens;item.cachedTokens=(item.cachedTokens??0)+parts.cached;grouped.set(label,item);
 }
 return [...grouped.values()];
}
export function usageTickIndices(length:number,maxTicks=4):number[]{
 if(length<=maxTicks)return Array.from({length},(_,i)=>i);
 return [...new Set(Array.from({length:maxTicks},(_,i)=>Math.round(i*(length-1)/(maxTicks-1))))];
}
export function UsageBarChart({rows,hourly}:{rows:UsageBarRow[];hourly:boolean}){
 const rawMax=Math.max(2,...rows.map(row=>Math.max(row.totalTokens,row.inputTokens+row.outputTokens))),power=10**Math.floor(Math.log10(rawMax)),maximum=Math.ceil(rawMax/power)*power;
 const left=52,top=16,plotWidth=570,plotHeight=176,bottom=top+plotHeight,step=plotWidth/Math.max(1,rows.length),barWidth=Math.min(26,step*.7);
 const label=(row:UsageBarRow)=>hourly?row.label.slice(11):row.label.slice(5).replace('-','/');
 const number=(value:number)=>new Intl.NumberFormat(undefined,{notation:'compact',maximumFractionDigits:1}).format(value);
 const cached='var(--prims-accent, hsl(var(--primary)))',uncached=`color-mix(in srgb, ${cached} 52%, var(--prims-ground, hsl(var(--background))))`,output='hsl(var(--muted-foreground) / .38)';
 return <div className="rounded-lg border bg-muted/10 p-3"><svg viewBox="0 0 640 228" className="block w-full" role="img" aria-label="按时间排列的 Token 用量条形图，输入区分缓存命中与未命中或未上报缓存，输出单独显示"><title>Token 用量条形图</title>
 {[0,maximum/2,maximum].map(value=>{const y=bottom-value/maximum*plotHeight;return <g key={value}><line x1={left} x2={left+plotWidth} y1={y} y2={y} stroke="hsl(var(--border))"/><text x={left-10} y={y+4} textAnchor="end" fill="currentColor" className="text-muted-foreground" fontSize={11}>{number(value)}</text></g>;})}
 {rows.map((row,index)=>{const x=left+(index+.5)*step-barWidth/2,parts=inputTokenParts(row),cachedHeight=parts.cached/maximum*plotHeight,uncachedHeight=parts.uncached/maximum*plotHeight,inputHeight=cachedHeight+uncachedHeight,outputHeight=row.outputTokens/maximum*plotHeight;return <g key={row.label}><title>{`${row.label}：${row.totalTokens.toLocaleString()} tokens；输入 ${parts.input.toLocaleString()}（缓存命中 ${parts.cached.toLocaleString()}，缓存未命中 ${parts.uncached.toLocaleString()}，接口未上报缓存时按未命中归类）；输出 ${row.outputTokens.toLocaleString()}`}</title><rect x={x} y={bottom-cachedHeight} width={barWidth} height={cachedHeight} fill={cached} data-usage-series="cached-input"/><rect x={x} y={bottom-inputHeight} width={barWidth} height={uncachedHeight} fill={uncached} data-usage-series="uncached-input"/><rect x={x} y={bottom-inputHeight-outputHeight} width={barWidth} height={outputHeight} fill={output} rx={3} data-usage-series="output"/><rect x={left+index*step} y={top} width={step} height={plotHeight} fill="transparent"/></g>;})}
 {usageTickIndices(rows.length).map(index=><text key={index} x={left+(index+.5)*step} y={bottom+23} textAnchor={index===0?'start':index===rows.length-1?'end':'middle'} fill="currentColor" className="text-muted-foreground" fontSize={11} data-usage-date-tick>{label(rows[index]!)}</text>)}
 </svg><div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-muted-foreground"><span className="flex flex-wrap items-center gap-x-3 gap-y-1"><span>输入</span><span className="flex items-center gap-1.5"><i className="h-2 w-2 rounded-full" style={{background:cached}}/>缓存命中</span><span className="flex items-center gap-1.5"><i className="h-2 w-2 rounded-full" style={{background:uncached}}/>缓存未命中</span></span><span className="flex items-center gap-1.5"><i className="h-2 w-2 rounded-full" style={{background:output}}/>输出</span></div></div>;
}

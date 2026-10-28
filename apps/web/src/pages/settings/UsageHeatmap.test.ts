import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {describe,it,expect} from 'vitest';
import {calendarWeeks,heatmapCells,usageBarRows,coarsenHeatmapRows,UsageHeatmap} from './UsageHeatmap';
import {usageTickIndices} from './UsageBarChart';
const bucket=(label:string,n=1)=>({label,totalTokens:n,inputTokens:n,outputTokens:0});
const today=Array.from({length:240},(_,i)=>bucket(`2026-10-28 ${String(Math.floor(i/10)).padStart(2,'0')}:${String(i%10*6).padStart(2,'0')}`));
const week=Array.from({length:168},(_,i)=>bucket(`2026-10-${22+Math.floor(i/24)} ${String(i%24).padStart(2,'0')}:00`));
const month=Array.from({length:720},(_,i)=>bucket(new Date(Date.UTC(2026,8,29, i)).toISOString().slice(0,13).replace('T',' ')+':00'));
describe('usage heatmap granularity',()=>{
 it('maps each six-minute bucket to an hour column and one of ten minute rows',()=>{const cells=heatmapCells(today,1);expect(cells).toHaveLength(240);expect(cells[0]).toMatchObject({column:0,row:0});expect(cells[9]).toMatchObject({column:0,row:9});expect(cells[10]).toMatchObject({column:1,row:0});expect(cells[239]).toMatchObject({column:23,row:9});});
 it('maps seven dates to rows and twenty-four hours to columns',()=>{const cells=heatmapCells(week,7);expect(cells).toHaveLength(168);expect(cells[23]).toMatchObject({column:23,row:0});expect(cells[24]).toMatchObject({column:0,row:1});expect(cells[167]).toMatchObject({column:23,row:6});});
 it('places monthly dates across Monday to Sunday, then wraps by week',()=>{const cells=heatmapCells(month,30);expect(cells).toHaveLength(120);expect(cells[0]).toMatchObject({column:1,row:0,slot:0});expect(cells[3]).toMatchObject({column:1,row:0,slot:3});expect(cells[24]).toMatchObject({column:0,row:1,slot:0});});
 it('preserves totals when rolling heatmap data back up for bars',()=>{for(const [rows,days,count] of [[today,1,24],[week,7,7],[month,30,30]] as const){const bars=usageBarRows([...rows],days);expect(bars).toHaveLength(count);expect(bars.reduce((sum,r)=>sum+r.totalTokens,0)).toBe(rows.length);}});
 it('renders the corresponding axes and cell counts for every range',()=>{for(const [rows,days,count,caption] of [[today,1,240,'每格6分钟'],[week,7,168,'每格1小时'],[month,30,120,'每格6小时']] as const){const html=renderToStaticMarkup(createElement(UsageHeatmap,{rows:[...rows],selectedDays:days}));expect(html.match(/<rect /g)).toHaveLength(count);expect(html).toContain(caption);}});
 it('aligns calendar weeks across leap days and changes of year',()=>{expect(calendarWeeks([bucket('2024-02-29')])[0]![3]!.label).toBe('2024-02-29');expect(calendarWeeks([bucket('2027-01-01')])[0]![4]!.label).toBe('2027-01-01');});
 it('keeps chart dates spaced and includes both edges',()=>{expect(usageTickIndices(30)).toEqual([0,10,19,29]);expect(usageTickIndices(24)).toEqual([0,8,15,23]);});
});

describe('readable heatmap density',()=>{it('coarsens only exact source buckets without changing totals',()=>{for(const [rows,days,minutes,count] of [[today,1,12,120],[today,1,30,48],[week,7,240,42],[week,7,360,28],[month,30,360,120],[month,30,180,240],[month,30,60,720]] as const){const coarse=coarsenHeatmapRows([...rows],days,minutes);expect(coarse).toHaveLength(count);expect(coarse.reduce((sum,row)=>sum+row.totalTokens,0)).toBe(rows.length);}});it('keeps at most four vertical labels with both extremes',()=>{for(const days of [1,7]){const html=renderToStaticMarkup(createElement(UsageHeatmap,{rows:days===1?today:week,selectedDays:days}));expect(html.match(/data-heatmap-y=/g)).toHaveLength(4);expect(html).toContain(days===1?'54–59':'10/28');}});});

it('keeps monthly tiles compact and preserves every hourly token',()=>{for(const minutes of [60,180,360]){const cells=heatmapCells(month,30,minutes);expect(cells.reduce((sum,cell)=>sum+cell.value.totalTokens,0)).toBe(720);}const html=renderToStaticMarkup(createElement(UsageHeatmap,{rows:month,selectedDays:30}));expect([...html.matchAll(/<rect[^>]*width="([^"]+)/g)].every(match=>Number(match[1])<=16)).toBe(true);});

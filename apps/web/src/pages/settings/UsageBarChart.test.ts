import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {describe,it,expect} from 'vitest';
import {cacheUsageBarRows,inputTokenParts,UsageBarChart} from './UsageBarChart';
const row=(label:string,input=100,cached:number|undefined=40,output=20)=>({label,inputTokens:input,cachedTokens:cached,outputTokens:output,totalTokens:input+output});
describe('cache input split',()=>{
 it('includes cache reads only once in the input total',()=>{expect(inputTokenParts(row('2026-10-28 10:00'))).toEqual({input:100,cached:40,uncached:60});expect(inputTokenParts(row('x',100,100))).toEqual({input:100,cached:100,uncached:0});});
 it('keeps absent cache accounting in the unclassified input remainder',()=>{expect(inputTokenParts({inputTokens:100})).toEqual({input:100,cached:0,uncached:100});});
 it('guards invalid cache counters without creating negative input segments',()=>{for(const cached of [-20,Infinity,NaN])expect(inputTokenParts(row('x',100,cached))).toEqual({input:100,cached:0,uncached:100});expect(inputTokenParts(row('x',100,200))).toEqual({input:100,cached:100,uncached:0});expect(inputTokenParts(row('x',-1,10))).toEqual({input:0,cached:0,uncached:0});});
 it('preserves input, cache, output and totals when combining time buckets',()=>{
  const source=[row('2026-10-28 10:00'),row('2026-10-28 10:06',200,60,30),row('2026-10-28 11:00',80,0,5),row('2026-10-29 00:00',20,10,2)];
  for(const days of [1,7,30]){const combined=cacheUsageBarRows(source,days);for(const field of ['inputTokens','cachedTokens','outputTokens','totalTokens'] as const)expect(combined.reduce((sum,item)=>sum+(item[field]??0),0)).toBe(source.reduce((sum,item)=>sum+(item[field]??0),0));expect(combined[0]!.label).toBe(days===1?'2026-10-28 10:00':'2026-10-28');}
 });
 it('stacks cache below uncached input below output without changing input height',()=>{
  const html=renderToStaticMarkup(createElement(UsageBarChart,{rows:[row('2026-10-28 10:00')],hourly:true}));
  const rects=[...html.matchAll(/<rect ([^>]+)data-usage-series="([^"]+)"/g)].map(match=>({series:match[2],y:Number(match[1]!.match(/y="([^"]+)"/)![1]),height:Number(match[1]!.match(/height="([^"]+)"/)![1])}));
  expect(rects.map(rect=>rect.series)).toEqual(['cached-input','uncached-input','output']);expect(rects[0]!.y+rects[0]!.height).toBeCloseTo(192);expect(rects[1]!.y+rects[1]!.height).toBeCloseTo(rects[0]!.y);expect(rects[2]!.y+rects[2]!.height).toBeCloseTo(rects[1]!.y);expect(rects[0]!.height/rects[1]!.height).toBeCloseTo(40/60);expect(html).toContain('缓存未命中 60，接口未上报缓存时按未命中归类');expect(html).toContain('输入 100');
 });
});

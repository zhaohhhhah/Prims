import {describe,it,expect,vi} from 'vitest';
import {PageBlobCache,READER_CACHE_BYTES,READER_CACHE_PAGES} from './page-blob-cache';
describe('logical-page Blob LRU',()=>{
 it('keeps both qualities within 50 logical pages, touches hits and releases both variants on eviction',()=>{
  const release=vi.fn(),cache=new PageBlobCache(release);
  for(let page=1;page<=50;page++){cache.set(page,`thumb-${page}`,10,'thumb');cache.set(page,`clear-${page}`,20);}
  expect(cache.size).toBe(READER_CACHE_PAGES);expect(cache.get(1)).toBe('clear-1');
  cache.set(51,'thumb-51',10,'thumb');cache.prune(new Set());
  expect(cache.size).toBe(50);expect(cache.peek(1)).toBe('clear-1');expect(cache.peek(2)).toBeUndefined();expect(release.mock.calls.flat()).toEqual(['thumb-2','clear-2']);
 });
 it('enforces compressed bytes even below the page-count limit',()=>{
  const cache=new PageBlobCache(vi.fn());
  for(let page=1;page<=8;page++){cache.set(page,`thumb-${page}`,256*1024,'thumb');cache.set(page,`clear-${page}`,2*1024*1024);cache.prune(new Set());}
  expect(cache.byteSize).toBeLessThanOrEqual(READER_CACHE_BYTES);expect(cache.size).toBe(5);expect(cache.peek(1)).toBeUndefined();expect(cache.peek(8)).toBe('clear-8');
 });
 it('protects visible and turning sources then enforces the budget when they unpin',()=>{
  const released:string[]=[];const cache=new PageBlobCache(url=>released.push(url),2,100);
  cache.set(1,'front',60,'thumb');cache.set(2,'back',60);cache.prune(new Set(['front','back']));
  expect(cache.byteSize).toBe(120);expect(released).toEqual([]);
  cache.prune(new Set(['back']));expect(cache.byteSize).toBe(60);expect(released).toEqual(['front']);
 });
});

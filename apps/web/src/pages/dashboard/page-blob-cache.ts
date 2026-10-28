/** LRU is counted per logical page; thumbnails and clear images share its byte budget. */
export type PageQuality='thumb'|'clear';
type BlobEntry={url:string;bytes:number};
type PageEntry=Partial<Record<PageQuality,BlobEntry>>;
export const READER_CACHE_PAGES=50;
export const READER_CACHE_BYTES=12*1024*1024;
export class PageBlobCache implements Iterable<[number,string]> {
 private entries=new Map<number,PageEntry>();
 private bytes=0;
 constructor(private release:(url:string)=>void,readonly maxPages=READER_CACHE_PAGES,readonly maxBytes=READER_CACHE_BYTES){}
 get size(){return this.entries.size;}
 get byteSize(){return this.bytes;}
 peek(page:number,quality?:PageQuality){const item=this.entries.get(page);return quality?item?.[quality]?.url:item?.clear?.url??item?.thumb?.url;}
 get(page:number,quality?:PageQuality){const value=this.peek(page,quality);if(value){const item=this.entries.get(page)!;this.entries.delete(page);this.entries.set(page,item);}return value;}
 has(page:number,quality?:PageQuality){return this.get(page,quality)!==undefined;}
 set(page:number,url:string,bytes:number,quality:PageQuality='clear'){
  const item=this.entries.get(page)??{};const old=item[quality];
  if(old){this.bytes-=old.bytes;if(old.url!==url)this.release(old.url);}
  item[quality]={url,bytes};this.entries.delete(page);this.entries.set(page,item);this.bytes+=bytes;
 }
 /** A visible/turning page may temporarily pin more bytes; retry immediately after transition. */
 prune(protectedUrls:ReadonlySet<string>){
  for(const [page,item] of this.entries){
   if(this.size<=this.maxPages&&this.bytes<=this.maxBytes)break;
   const variants=Object.values(item);if(variants.some(entry=>protectedUrls.has(entry.url)))continue;
   this.entries.delete(page);for(const entry of variants){this.bytes-=entry.bytes;this.release(entry.url);}
  }
 }
 clear(){this.entries.clear();this.bytes=0;}
 *[Symbol.iterator]():IterableIterator<[number,string]>{for(const [page,item] of this.entries){const url=item.clear?.url??item.thumb?.url;if(url)yield[page,url];}}
}

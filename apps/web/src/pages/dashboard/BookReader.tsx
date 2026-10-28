import * as pdfjs from 'pdfjs-dist';
import workerSrc from 'pdfjs-dist/build/pdf.worker.min.mjs?worker&url';
import {ChevronLeft,ChevronRight,FileText,Loader2} from 'lucide-react';
import {useCallback,useEffect,useLayoutEffect,useRef,useState} from 'react';
import {spreadStart,bookTurnPages} from '../../lib/book-spreads';
import {PageBlobCache,type PageQuality} from './page-blob-cache';
import {useDashboardCopy} from './dashboard-copy';
pdfjs.GlobalWorkerOptions.workerSrc=workerSrc;
export interface ReaderPage {url:string;width:number;height:number;thumbUrl?:string;thumbWidth?:number;thumbHeight?:number}
type Turn={id:number;forward:boolean;front:string|null;back:string|null;stationary:string|null;duration:number};
interface Props{documentKey?:string;url:string|null;pages?:ReaderPage[]|undefined;filter:string;onPageCount:(count:number|null)=>void;onRetryUrl?:()=>Promise<void>;onLoadError?:()=>void;}
type Resource={pdf:pdfjs.PDFDocumentProxy|null;pages:ReaderPage[]|null};
type QueuedPage={page:number;quality:PageQuality;upgrade:boolean;generation:number;resolve:(ready:boolean)=>void;reject:(error:unknown)=>void};
export function BookReader({documentKey,url,pages,filter,onPageCount,onRetryUrl,onLoadError}:Props){
 const copy=useDashboardCopy();const stage=useRef<HTMLDivElement>(null),leaf=useRef<HTMLDivElement>(null),stationary=useRef<HTMLDivElement>(null);
 const visibleImages=useRef(new Map<number,string>());
 const decodedThumbs=useRef(new Map<string,HTMLImageElement>()),protectedUrls=useRef(new Set<string>()),activeClear=useRef<AbortController|null>(null),clearTimer=useRef<ReturnType<typeof setTimeout>|null>(null),fadeTimers=useRef(new Set<ReturnType<typeof setTimeout>>()),fadeUrls=useRef(new Set<string>());
 const [fadeRevision,setFadeRevision]=useState(0);void fadeRevision;
 const allUrls=useRef(new Set<string>()),heldUrls=useRef(new Set<string>()),cache=useRef(new PageBlobCache(src=>{URL.revokeObjectURL(src);allUrls.current.delete(src);const image=decodedThumbs.current.get(src);if(image)image.src='';decodedThumbs.current.delete(src);})),inFlight=useRef(new Map<string,Promise<boolean>>()),queue=useRef<QueuedPage[]>([]),rendering=useRef(false),generation=useRef(0),tasks=useRef<pdfjs.RenderTask[]>([]),controllers=useRef(new Set<AbortController>());
 const resource=useRef<Resource>({pdf:null,pages:null}),epoch=useRef(0),position=useRef(0),requested=useRef(0),spread=useRef(2),loadedDocument=useRef<string|null>(null),loadedMode=useRef('');
 const animation=useRef<Animation|null>(null),stationaryAnimation=useRef<Animation|null>(null);
 const [revision,setRevision]=useState(0),[total,setTotal]=useState(0),[current,setCurrent]=useState(0),[pendingPage,setPendingPage]=useState<number|null>(null),[images,setImages]=useState(new Map<number,string>()),[turn,setTurn]=useState<Turn|null>(null),[error,setError]=useState(false),[loading,setLoading]=useState(false),[retry,setRetry]=useState(0),[ratio,setRatio]=useState(.7071),[narrow,setNarrow]=useState(()=>window.matchMedia('(max-width:660px)').matches),[reduced,setReduced]=useState(()=>window.matchMedia('(prefers-reduced-motion:reduce)').matches),[size,setSize]=useState({width:600,height:430});
 visibleImages.current=images;
 protectedUrls.current=new Set([images.get(current+1),...(narrow?[]:[images.get(current+2)]),turn?.front,turn?.back,turn?.stationary].filter((src):src is string=>!!src));
 for(const page of [current+1,...(narrow?[]:[current+2])]){const thumb=cache.current.peek(page,'thumb');if(thumb&&fadeUrls.current.has(images.get(page)??''))protectedUrls.current.add(thumb);}
 const trimCache=useCallback(()=>{const protectedSet=new Set(protectedUrls.current);for(let page=requested.current+1;page<=requested.current+spread.current;page++){for(const quality of ['thumb','clear'] as const){const src=cache.current.peek(page,quality);if(src)protectedSet.add(src);}}cache.current.prune(protectedSet);},[]);
 const step=narrow?1:2;spread.current=step;const last=spreadStart(total-1,step,total);const hasSource=!!pages?.length||url!==null;
 const stop=useCallback(()=>{epoch.current++;activeClear.current?.abort();activeClear.current=null;if(clearTimer.current!==null){clearTimeout(clearTimer.current);clearTimer.current=null;}animation.current?.cancel();stationaryAnimation.current?.cancel();animation.current=null;stationaryAnimation.current=null;setTurn(null);},[]);
 const clearPages=useCallback((keep?:Set<string>)=>{generation.current++;for(const task of tasks.current)task.cancel();tasks.current=[];for(const controller of controllers.current)controller.abort();controllers.current.clear();for(const item of queue.current)item.resolve(false);queue.current=[];inFlight.current.clear();for(const image of decodedThumbs.current.values())image.src='';decodedThumbs.current.clear();for(const timer of fadeTimers.current)clearTimeout(timer);fadeTimers.current.clear();fadeUrls.current.clear();for(const src of allUrls.current)if(!keep?.has(src)){URL.revokeObjectURL(src);allUrls.current.delete(src);}heldUrls.current=keep??new Set();cache.current.clear();},[]);
 useEffect(()=>{const n=window.matchMedia('(max-width:660px)'),r=window.matchMedia('(prefers-reduced-motion:reduce)');const change=()=>{stop();setNarrow(n.matches);setReduced(r.matches);};n.addEventListener('change',change);r.addEventListener('change',change);return()=>{n.removeEventListener('change',change);r.removeEventListener('change',change);};},[stop]);
 useEffect(()=>{if(!stage.current)return;const observer=new ResizeObserver(entries=>{const box=entries[0]?.contentRect;if(box)setSize({width:box.width,height:box.height});});observer.observe(stage.current);return()=>observer.disconnect();},[]);
 // Signed WebP URL renewal retains decoded blobs and page position for the same job.
 useEffect(()=>{
  const mode=pages?.length?'images':'pdf';const identity=documentKey??url;
  const same=identity!==null&&loadedDocument.current===identity;
  if(mode==='images'&&same&&loadedMode.current===mode){resource.current.pages=pages!;setError(false);setRevision(n=>n+1);return;}
  let disposed=false;stop();const retainedImages=same?new Map([...visibleImages.current].filter(([page])=>page>=position.current+1&&page<=position.current+spread.current)):new Map<number,string>();clearPages(new Set(retainedImages.values()));resource.current={pdf:null,pages:null};setImages(retainedImages);setTotal(0);setError(false);setPendingPage(null);
  const retained=same?position.current:0;position.current=retained;requested.current=retained;setCurrent(retained);loadedDocument.current=identity;loadedMode.current=mode;setLoading(hasSource);onPageCount(null);
  if(pages?.length){resource.current.pages=pages;setTotal(pages.length);onPageCount(pages.length);setRatio(pages[0]!.width/pages[0]!.height);setRevision(n=>n+1);return;}
  if(!url)return;
  const loader=pdfjs.getDocument({url,disableAutoFetch:true,disableStream:true,rangeChunkSize:65536,cMapUrl:`${import.meta.env.BASE_URL}pdfjs/cmaps/`,cMapPacked:true,standardFontDataUrl:`${import.meta.env.BASE_URL}pdfjs/standard_fonts/`,useSystemFonts:true});
  void loader.promise.then(async pdf=>{if(disposed){await pdf.destroy();return;}resource.current.pdf=pdf;setTotal(pdf.numPages);onPageCount(pdf.numPages);const first=await pdf.getPage(1);if(disposed)return;const view=first.getViewport({scale:1});setRatio(view.width/view.height);setRevision(n=>n+1);}).catch(()=>{if(!disposed){setError(true);setLoading(false);onLoadError?.();}});
  return()=>{disposed=true;for(const task of tasks.current)task.cancel();for(const controller of controllers.current)controller.abort();resource.current.pdf=null;void loader.destroy();};
 },[url,pages,documentKey,retry,stop,clearPages,onPageCount,onLoadError,hasSource]);
 useEffect(()=>{const displayed=new Set(images.values());for(const src of heldUrls.current)if(!displayed.has(src)){URL.revokeObjectURL(src);allUrls.current.delete(src);heldUrls.current.delete(src);}},[images]);
 useEffect(()=>()=>{stop();clearPages();},[stop,clearPages]);
 const wanted=useCallback((page:number,upgrade=false)=>{const s=spread.current;if(upgrade)return requested.current===position.current&&page>=position.current+1&&page<=position.current+s;return(page>=requested.current-s+1&&page<=requested.current+s*2)||(page>=position.current+1&&page<=position.current+s);},[]);
 const pump=useCallback(async()=>{
  if(rendering.current)return;rendering.current=true;
  try{while(queue.current.length){
   const s=spread.current;let index=queue.current.findIndex(item=>!item.upgrade&&item.page>=requested.current+1&&item.page<=requested.current+s);if(index<0)index=0;
   const item=queue.current.splice(index,1)[0]!;
   if(item.generation!==generation.current||!wanted(item.page,item.upgrade)){if(item.generation===generation.current)inFlight.current.delete(`${item.page}:${item.quality}`);item.resolve(false);continue;}
   let objectUrl:string|null=null;let blobBytes=0;let decodedImage:HTMLImageElement|null=null;
   try{
    const imagePage=resource.current.pages?.[item.page-1];
    if(imagePage){
     const controller=new AbortController();controllers.current.add(controller);if(item.upgrade)activeClear.current=controller;
     try{const response=await fetch(item.quality==='thumb'?imagePage.thumbUrl!:imagePage.url,{signal:controller.signal});if(!response.ok)throw new Error('Page image unavailable');const blob=await response.blob();if(item.generation!==generation.current||!wanted(item.page,item.upgrade)){item.resolve(false);continue;}blobBytes=blob.size;objectUrl=URL.createObjectURL(blob);allUrls.current.add(objectUrl);const image=new Image();image.src=objectUrl;controller.signal.addEventListener('abort',()=>{image.src='';},{once:true});await image.decode();decodedImage=image;}finally{controllers.current.delete(controller);if(activeClear.current===controller)activeClear.current=null;}
    }else{
     const pdf=resource.current.pdf;if(!pdf){item.resolve(false);continue;}
     const page=await pdf.getPage(item.page);if(item.generation!==generation.current||!wanted(item.page,item.upgrade)){item.resolve(false);continue;}
     const view=page.getViewport({scale:1});const scale=Math.min(2,window.devicePixelRatio||1)*Math.min(560/view.width,780/view.height);const viewport=page.getViewport({scale});const canvas=document.createElement('canvas');canvas.width=Math.ceil(viewport.width);canvas.height=Math.ceil(viewport.height);
     try{const context=canvas.getContext('2d');if(!context)throw new Error('Canvas unavailable');const task=page.render({canvasContext:context,viewport});tasks.current.push(task);try{await task.promise;}finally{tasks.current=tasks.current.filter(x=>x!==task);}if(item.generation===generation.current){const blob=await new Promise<Blob|null>(resolve=>canvas.toBlob(resolve,'image/webp',.88));if(!blob)throw new Error('Page raster unavailable');blobBytes=blob.size;objectUrl=URL.createObjectURL(blob);allUrls.current.add(objectUrl);}}finally{canvas.width=0;canvas.height=0;}
    }
    if(item.generation===generation.current&&wanted(item.page,item.upgrade)&&objectUrl){cache.current.set(item.page,objectUrl,blobBytes,item.quality);if(item.quality==='thumb'&&decodedImage){decodedThumbs.current.set(objectUrl,decodedImage);decodedImage=null;}objectUrl=null;trimCache();item.resolve(true);}else item.resolve(false);
   }catch(error){item.reject(error);}finally{if(decodedImage)decodedImage.src='';if(objectUrl){URL.revokeObjectURL(objectUrl);allUrls.current.delete(objectUrl);}if(item.generation===generation.current)inFlight.current.delete(`${item.page}:${item.quality}`);}
  }}finally{rendering.current=false;}
 },[wanted,trimCache]);
 const renderPages=useCallback(async(start:number,s:number,upgrade=false)=>{
  for(const page of Array.from({length:s},(_,i)=>start+i+1).filter(p=>p<=total)){
   const quality:PageQuality=upgrade?'clear':resource.current.pages?.[page-1]?.thumbUrl?'thumb':'clear';
   if(cache.current.has(page,upgrade?'clear':undefined))continue;
   const key=`${page}:${quality}`;let pending=inFlight.current.get(key);
   if(!pending){pending=new Promise<boolean>((resolve,reject)=>queue.current.push({page,quality,upgrade,generation:generation.current,resolve,reject}));inFlight.current.set(key,pending);void pump();}
   if(!await pending)return false;
  }return true;
 },[total,pump]);
 const go=useCallback(async(destination:number,scrub=false)=>{
  if(!total)return;const target=spreadStart(destination,step,total);requested.current=target;
  const hit=Array.from({length:step},(_,i)=>target+i+1).filter(p=>p<=total).every(p=>cache.current.has(p));
  if(target===position.current&&hit){setLoading(false);setPendingPage(null);return;}
  const previous=position.current;stop();const id=epoch.current;setPendingPage(target);setLoading(!hit);
  try{
   if(!hit&&!await renderPages(target,step))return;if(epoch.current!==id)return;
   const faces=bookTurnPages(previous,target,step);const front=cache.current.get(faces.front)??null;
   position.current=target;setCurrent(target);setPendingPage(null);setError(false);setLoading(false);setImages(new Map(cache.current));
   if(!reduced&&target!==previous&&front)setTurn({id,forward:faces.forward,front,back:cache.current.get(faces.back)??null,stationary:cache.current.get(faces.stationary)??null,duration:scrub?360:440});
   // Keep visited compressed pages under both LRU limits; only nearby pages are decoded.
   trimCache();
   setImages(new Map(cache.current));
   for(const neighbour of [target+step,target-step])if(neighbour>=0&&neighbour<total){void renderPages(neighbour,step).catch(()=>{});}
  }catch(e){if(epoch.current===id&&!(e instanceof Error&&e.name==='RenderingCancelledException')){setError(true);setLoading(false);onLoadError?.();}}
 },[total,step,reduced,stop,renderPages,onLoadError,trimCache]);
 useEffect(()=>{
  if(!resource.current.pages?.some(page=>page.thumbUrl)||pendingPage!==null||loading)return;
  const target=current;
  clearTimer.current=setTimeout(()=>{clearTimer.current=null;void(async()=>{
   const fresh=new Set<string>();for(let page=target+1;page<=Math.min(total,target+step);page++){if(!cache.current.peek(page,'clear')){const thumb=cache.current.peek(page,'thumb');if(thumb)fresh.add(String(page));}}
   try{if(!await renderPages(target,step,true)||requested.current!==target||position.current!==target)return;
    for(const pageText of fresh){const src=cache.current.peek(Number(pageText),'clear');if(src){fadeUrls.current.add(src);const timer=setTimeout(()=>{fadeUrls.current.delete(src);fadeTimers.current.delete(timer);setFadeRevision(n=>n+1);trimCache();},220);fadeTimers.current.add(timer);}}
    setImages(new Map(cache.current));trimCache();
   }catch{/* A failed optional clear upgrade keeps the already decoded thumbnail. */}
  })();},180);
  return()=>{if(clearTimer.current!==null){clearTimeout(clearTimer.current);clearTimer.current=null;}activeClear.current?.abort();};
 },[current,pendingPage,loading,revision,step,total,renderPages,trimCache]);
 useEffect(()=>{trimCache();},[turn,current,images,trimCache]);
 useEffect(()=>{if(total)void go(spreadStart(requested.current,step,total));},[revision,total,step,go]);
 useLayoutEffect(()=>{if(!turn||!leaf.current)return;const angle=turn.forward?-180:180;const effect=leaf.current.animate([{transform:'rotateY(0deg)'},{transform:`rotateY(${angle/2}deg)`,offset:.5},{transform:`rotateY(${angle}deg)`}],{duration:turn.duration,easing:'cubic-bezier(.3,.1,.2,1)',fill:'forwards'});animation.current=effect;if(stationary.current)stationaryAnimation.current=stationary.current.animate([{visibility:'visible'},{visibility:'visible',offset:.499},{visibility:'hidden',offset:.5},{visibility:'hidden'}],{duration:turn.duration,easing:'cubic-bezier(.3,.1,.2,1)',fill:'forwards'});void effect.finished.then(()=>{if(epoch.current===turn.id)setTurn(null);}).catch(()=>{});return()=>{effect.cancel();stationaryAnimation.current?.cancel();};},[turn]);
 const width=Math.max(40,Math.min(760,size.width-72,(size.height-28)*ratio*step));const height=width/(ratio*step);
 const picture=(src:string|null|undefined,hidden=false)=>{
  if(!src)return <div className="reader-blank"/>;
  const page=[...cache.current].find(([,url])=>url===src)?.[0];
  const thumb=page===undefined?undefined:cache.current.peek(page,'thumb');
  const low=thumb&&thumb!==src?thumb:undefined;
  const high=<img src={src} alt="" draggable={false} aria-hidden={hidden} style={{filter,...(low?{position:'absolute' as const,inset:0}:{} )}} onLoad={event=>{if(low&&fadeUrls.current.has(src)&&!reduced&&typeof event.currentTarget.animate==='function')event.currentTarget.animate([{opacity:0},{opacity:1}],{duration:200,easing:'ease-out'});}}/>;
  return low?<span style={{display:'block',position:'relative',width:'100%',height:'100%'}}><img src={low} alt="" draggable={false} aria-hidden={hidden} style={{filter}}/>{high}</span>:high;
 };
 const text=total?`${copy.page} ${current+1}${step===2&&current+2<=total?`–${current+2}`:''} ${copy.of} ${total}`:hasSource?copy.loading:copy.noPdf;
 return <div className="book-reader-content"><div className="real-book-stage" ref={stage}>
 {!hasSource?<div className="reader-empty"><FileText size={32}/><strong>{copy.noPdf}</strong><p>{copy.noPdfHint}</p></div>:error&&images.size===0?<div className="reader-empty"><p>{copy.pdfError}</p><button onClick={()=>{if(!onRetryUrl){setRetry(n=>n+1);return;}setLoading(true);void onRetryUrl().then(()=>setRetry(n=>n+1)).catch(()=>{setError(true);setLoading(false);});}} disabled={loading}>{copy.retry}</button></div>:<>
 <button className="book-edge previous" aria-label={copy.previous} disabled={!total||current===0} onClick={()=>void go(position.current-step)}><ChevronLeft/></button>
 <div className={`real-open-book ${narrow?'single':''}`} tabIndex={0} role="group" aria-label={`${copy.pdfLabel}, ${text}`} style={{width,height}} onKeyDown={event=>{if(event.ctrlKey||event.metaKey||event.altKey)return;let next:number|undefined;if(['ArrowRight','PageDown'].includes(event.key))next=position.current+step;if(['ArrowLeft','PageUp'].includes(event.key))next=position.current-step;if(event.key==='Home')next=0;if(event.key==='End')next=last;if(next!==undefined){event.preventDefault();void go(next);}}}>
 <div className="reader-page left" onClick={()=>void go(position.current-step)}>{picture(images.get(current+1))}</div>{!narrow&&<div className="reader-page right" onClick={()=>void go(position.current+step)}>{picture(images.get(current+2))}</div>}
 {turn&&!narrow&&<div ref={stationary} className={`reader-stationary ${turn.forward?'left':'right'}`} aria-hidden="true">{picture(turn.stationary,true)}</div>}
 {turn&&<div ref={leaf} className={`reader-turn-leaf ${turn.forward?'forward':'backward'}`} aria-hidden="true"><div className="reader-leaf-face front">{picture(turn.front,true)}</div><div className="reader-leaf-face back">{picture(turn.back,true)}</div></div>}
 </div><button className="book-edge next" aria-label={copy.next} disabled={!total||current>=last} onClick={()=>void go(position.current+step)}><ChevronRight/></button>{error&&images.size>0&&<button className="book-loading" onClick={()=>{setError(false);if(onRetryUrl)void onRetryUrl().then(()=>setRetry(n=>n+1)).catch(()=>setError(true));else setRetry(n=>n+1);}}>{copy.retry}</button>}{loading&&images.size===0&&<span className="book-loading" role="status"><Loader2 className="animate-spin" size={15}/>{copy.loading}</span>}</>}
 </div><div className="real-reading-progress"><div><span aria-live="polite">{text}</span><span>{total?`${last?Math.round(current/last*100):100}%`:''}</span></div><input type="range" min={0} max={Math.max(0,Math.ceil(total/step)-1)} value={Math.floor((pendingPage??current)/step)} disabled={!total} onChange={e=>void go(Number(e.target.value)*step,true)} aria-label={copy.progress} aria-valuetext={text}/></div></div>;
}

import {useCallback,useEffect,useState} from 'react';
export type AssistantMode='right'|'bottom'|'float';
export interface AssistantViewport {width:number;height:number}
export interface AssistantRect {x:number;y:number;width:number;height:number}
export interface AssistantLayout {mode:AssistantMode;visible:boolean;rightWidth:number;bottomHeight:number;floating:AssistantRect}
export type AssistantLayoutPatch=Partial<Omit<AssistantLayout,'floating'>>&{floating?:Partial<AssistantRect>};
const bounded=(n:number,min:number,max:number)=>Math.min(max,Math.max(min,n));
const number=(value:unknown,fallback:number)=>typeof value==='number'&&Number.isFinite(value)?value:fallback;
const record=(value:unknown):Record<string,unknown>=>value!==null&&typeof value==='object'?value as Record<string,unknown>:{};
export const assistantLayoutKey=(userId:string)=>`prims-assistant-workspace:${encodeURIComponent(userId)}`;
export function defaultAssistantLayout(viewport:AssistantViewport={width:1280,height:800}):AssistantLayout{return {mode:'right',visible:true,rightWidth:380,bottomHeight:360,floating:{x:Math.max(8,viewport.width-568),y:72,width:540,height:620}};}
export function clampAssistantLayout(value:unknown,viewport:AssistantViewport):AssistantLayout{
 const source=record(value),floating=record(source.floating),defaults=defaultAssistantLayout(viewport);
 const maxWidth=Math.max(180,viewport.width-16),maxHeight=Math.max(140,viewport.height-16);
 const width=bounded(number(floating.width,defaults.floating.width),Math.min(320,maxWidth),maxWidth),height=bounded(number(floating.height,defaults.floating.height),Math.min(320,maxHeight),maxHeight);
 return {mode:source.mode==='bottom'||source.mode==='float'||source.mode==='right'?source.mode:'right',visible:typeof source.visible==='boolean'?source.visible:true,rightWidth:bounded(number(source.rightWidth,defaults.rightWidth),240,Math.max(240,Math.min(640,viewport.width-240))),bottomHeight:bounded(number(source.bottomHeight,defaults.bottomHeight),240,Math.max(240,viewport.height*.7)),floating:{x:bounded(number(floating.x,defaults.floating.x),8,Math.max(8,viewport.width-width-8)),y:bounded(number(floating.y,defaults.floating.y),8,Math.max(8,viewport.height-height-8)),width,height}};
}
export function parseAssistantLayout(raw:string|null,viewport:AssistantViewport):AssistantLayout{try{return clampAssistantLayout(raw?JSON.parse(raw):null,viewport);}catch{return clampAssistantLayout(null,viewport);}}
export function effectiveAssistantMode(layout:AssistantLayout,workspaceWidth:number):AssistantMode{return layout.mode==='right'&&workspaceWidth<560?'bottom':layout.mode;}
function currentViewport():AssistantViewport{return typeof window==='undefined'?{width:1280,height:800}:{width:window.innerWidth,height:window.innerHeight};}
/** Layout only. Conversation content and credentials never enter this browser preference. */
export function useAssistantLayout(userId:string|undefined){
 const [viewport,setViewport]=useState(currentViewport);
 const [saved,setSaved]=useState<{userId:string;loaded:boolean;layout:AssistantLayout}>({userId:'',loaded:false,layout:defaultAssistantLayout(viewport)});
 useEffect(()=>{const resize=()=>setViewport(currentViewport());window.addEventListener('resize',resize);return()=>window.removeEventListener('resize',resize);},[]);
 useEffect(()=>{let raw:string|null=null;try{if(userId)raw=localStorage.getItem(assistantLayoutKey(userId));}catch{/* Use defaults when browser storage is unavailable. */}setSaved({userId:userId??'',loaded:true,layout:parseAssistantLayout(raw,currentViewport())});},[userId]);
 useEffect(()=>{if(!userId||saved.userId!==userId||!saved.loaded)return;try{localStorage.setItem(assistantLayoutKey(userId),JSON.stringify(saved.layout));}catch{/* Layout remains usable without persistence. */}},[saved,userId]);
 const layout=clampAssistantLayout(saved.loaded&&saved.userId===(userId??'')?saved.layout:null,viewport);
 const updateLayout=useCallback((patch:AssistantLayoutPatch)=>{setSaved(previous=>{const base=clampAssistantLayout(previous.loaded&&previous.userId===(userId??'')?previous.layout:null,viewport);const next=clampAssistantLayout({...base,...patch,floating:{...base.floating,...patch.floating}},viewport);if(previous.loaded&&previous.userId===(userId??'')&&JSON.stringify(previous.layout)===JSON.stringify(next))return previous;return {userId:userId??'',loaded:true,layout:next};});},[userId,viewport.width,viewport.height]);
 return {layout,updateLayout,viewport};
}

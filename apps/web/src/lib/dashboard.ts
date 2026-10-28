import type {ProjectId,CompileJobId} from '@scribe/shared';
import {API_URL,supabase} from './supabase';
export interface DashboardSummary {createdAt:string;updatedAt:string;collaborators:{id:string;name:string}[];versionCount:number;latestPdfJobId:CompileJobId|null;wordCount:number|null;wordCountPartial:boolean}
export async function dashboardSummary(id:ProjectId,signal?:AbortSignal):Promise<DashboardSummary>{const {data}=await supabase.auth.getSession();const response=await fetch(`${API_URL}/api/projects/${id}/dashboard-summary`,{headers:data.session?{Authorization:`Bearer ${data.session.access_token}`}:{},signal:signal??null});if(!response.ok)throw new Error('Project summary unavailable');return response.json() as Promise<DashboardSummary>;}

export interface BookshelfPdf {url:string;jobId:CompileJobId;optimized:boolean;signedAt?:number;previewStatus?:'pending'|'ready'|'failed'|'disabled';pages?:{page:number;url:string;width:number;height:number}[]}
export async function bookshelfPdf(id:ProjectId,original=false,signal?:AbortSignal):Promise<BookshelfPdf>{
 const {data}=await supabase.auth.getSession();
 const response=await fetch(`${API_URL}/api/projects/${id}/bookshelf-pdf?original=${original}`,{headers:data.session?{Authorization:`Bearer ${data.session.access_token}`}:{},signal:signal??null});
 if(!response.ok)throw new Error('Bookshelf PDF unavailable');
 return {...await response.json(),signedAt:Date.now()} as BookshelfPdf;
}

export async function previewInterest(id:ProjectId,kind:'editor'|'reader',signal?:AbortSignal):Promise<void>{
 const {data}=await supabase.auth.getSession();
 await fetch(`${API_URL}/api/projects/${id}/preview-interest`,{method:'POST',headers:{'Content-Type':'application/json',...(data.session?{Authorization:`Bearer ${data.session.access_token}`}:{})},body:JSON.stringify({kind}),signal:signal??null});
}

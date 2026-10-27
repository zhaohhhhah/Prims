import type {ProjectId,CompileJobId} from '@scribe/shared';
import {API_URL,supabase} from './supabase';
export interface DashboardSummary {createdAt:string;updatedAt:string;collaborators:{id:string;name:string}[];versionCount:number;latestPdfJobId:CompileJobId|null;wordCount:number|null;wordCountPartial:boolean}
export async function dashboardSummary(id:ProjectId,signal?:AbortSignal):Promise<DashboardSummary>{const {data}=await supabase.auth.getSession();const response=await fetch(`${API_URL}/api/projects/${id}/dashboard-summary`,{headers:data.session?{Authorization:`Bearer ${data.session.access_token}`}:{},signal:signal??null});if(!response.ok)throw new Error('Project summary unavailable');return response.json() as Promise<DashboardSummary>;}

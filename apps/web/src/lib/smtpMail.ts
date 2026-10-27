import { supabase } from './supabase';
export interface SMTPDraft {enabled:boolean;host:string;port:number;username:string;password?:string;clearPassword?:boolean;senderEmail:string;senderName:string;requireConfirmation:boolean}
export interface SMTPConfig extends Omit<SMTPDraft,'password'|'clearPassword'> {passwordSet:boolean;updatedAt:string|null;activeChannel:'smtp'|'outlook'|'off'}
async function request<T>(path='',body?:unknown,method=body===undefined?'GET':'PUT'):Promise<T>{
 const {data:{session}}=await supabase.auth.getSession();if(!session)throw new Error('请重新登录');
 const response=await fetch('/api/admin/mail/smtp'+path,{method,credentials:'same-origin',cache:'no-store',headers:{Authorization:`Bearer ${session.access_token}`,'Content-Type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)})});
 const data:unknown=await response.json();if(!response.ok){const error=data as {message?:string;error?:{message?:string}};throw new Error(error.error?.message??error.message??'SMTP 操作失败');}return data as T;
}
export const smtpMailApi={config:()=>request<SMTPConfig>(),save:(draft:SMTPDraft)=>request<SMTPConfig>('',draft),test:(draft:SMTPDraft&{recipient:string})=>request<{message:string}>('/test',draft,'POST')};

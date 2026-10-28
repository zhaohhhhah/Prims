import { describe, expect, it } from 'vitest';
import { assistantCloseKey, handoffDisposition, validEditorContext, restoreInEditor, assistantReturnKey, assistantTabChannel, assistantTabSession, assistantTabUrl, canContinueWithoutParent, editorTabUrl, validTabMessage, validTabState, type AssistantTabState } from './assistant-tab';
const id='e640f62c-4ff7-4c66-b729-ef1907a87805';
const state:AssistantTabState={drafts:{panel:'A fixed draft',selection:'A separate selection draft'},threadId:'thread',modelChoice:'["connection","model"]',filePath:'main.tex',plan:{id:'run',threadId:'thread',edits:[{id:'edit',filePath:'main.tex',before:'original',after:'manually adjusted',state:'pending'}]},fileActions:[],target:{from:3,to:11,text:'original'}};
describe('scoped assistant tab transfer',()=>{
 it('puts only project, session and current file identity in a same-origin URL',()=>{
  const url=new URL(assistantTabUrl('https://example.test','project',id,'file'));
  expect(url.origin).toBe('https://example.test');expect(url.pathname).toBe('/project/project');
  expect([...url.searchParams.keys()]).toEqual(['assistant','bridge','file']);
  expect(assistantTabSession(url.search)).toBe(id);
  expect(url.href).not.toContain('draft');expect(url.href).not.toContain('model');
 });
 it('rejects unrelated projects, accounts, sessions and reflected sender messages',()=>{
  const message={version:1,session:id,userId:'user',projectId:'project',sender:'child',kind:'active'};
  expect(validTabMessage(message,'user','project',id,'child')).toBe(true);
  for(const [user,project,session,sender] of [['other','project',id,'child'],['user','other',id,'child'],['user','project','other','child'],['user','project',id,'parent']] as const)expect(validTabMessage(message,user,project,session,sender)).toBe(false);
  expect(validTabMessage({...message,kind:'execute_shell'},'user','project',id,'child')).toBe(false);
 });
 it('keeps both unsent drafts and pending candidate source separate',()=>{
  const clone:unknown=JSON.parse(JSON.stringify(state));expect(validTabState(clone)).toBe(true);
  if(!validTabState(clone))throw new Error('invalid fixture');
  expect(clone.drafts.panel).toBe('A fixed draft');expect(clone.drafts.selection).toBe('A separate selection draft');
  expect(validTabState({...state,plan:{...state.plan,edits:Array.from({length:100},(_,i)=>({...state.plan!.edits[0]!,id:'edit-'+i}))}})).toBe(true);
  expect(clone.plan?.edits[0]?.after).toBe('manually adjusted');expect(clone.plan?.edits[0]?.state).toBe('pending');
 });
 it('refuses cross-thread plans and oversized or malformed transfer data',()=>{
  expect(validTabState({...state,plan:{...state.plan,threadId:'other'}})).toBe(false);
  expect(validTabState({...state,drafts:{panel:'x'.repeat(8*1024*1024),selection:''}})).toBe(false);
  expect(validTabState({...state,target:{from:-1,to:4,text:'bad'}})).toBe(false);
  expect(validTabState({...state,plan:{...state.plan,edits:[{filePath:'main.tex'}]}})).toBe(false);
 });
 it('transfers long independent drafts, large reviews and large live selections without losing content',()=>{
  const longDraft='详细修改要求'.repeat(4000);
  const plan={...state.plan!,edits:Array.from({length:100},(_,i)=>({...state.plan!.edits[0]!,id:'hunk-'+i,before:'原文'.repeat(1000),after:'手工调整'.repeat(1000)}))};
  const transferred={...state,drafts:{panel:longDraft,selection:longDraft},plan};
  const clone=structuredClone(transferred);expect(validTabState(clone)).toBe(true);expect(clone).toEqual(transferred);
  const selection={from:0,to:40000,text:'x'.repeat(40000)};expect(validEditorContext({revision:1,fileId:'main',filePath:'main.tex',selection})).toBe(true);
 });
 it('measures the transport boundary in UTF-8 bytes and rejects incoherent selections',()=>{
  expect(validTabState({...state,drafts:{panel:'文'.repeat(3*1024*1024),selection:''}})).toBe(false);
  expect(validTabState({...state,target:{from:0,to:3,text:'hello'}})).toBe(false);
  expect(validEditorContext({revision:1,fileId:'main',filePath:'main.tex',selection:{from:0,to:3*1024*1024,text:'文'.repeat(3*1024*1024)}})).toBe(false);
 });
 it('never self-grants while an original editor still owns or an unknown storage state exists',()=>{
  expect(canContinueWithoutParent('parent')).toBe(false);expect(canContinueWithoutParent('child')).toBe(false);expect(canContinueWithoutParent('unknown')).toBe(false);
  expect(canContinueWithoutParent('closed')).toBe(true);expect(canContinueWithoutParent(null)).toBe(true);
 });
 it('scopes fallback drafts to the account and project and returns to a normal editor URL',()=>{
  expect(assistantReturnKey('a','p')).not.toBe(assistantReturnKey('b','p'));
  expect(assistantTabChannel('a','p',id)).not.toBe(assistantTabChannel('a','q',id));
  const url=new URL(editorTabUrl('https://example.test','project','chapters/intro.tex'));
  expect(url.searchParams.get('filePath')).toBe('chapters/intro.tex');expect(url.searchParams.has('assistant')).toBe(false);expect(url.searchParams.has('bridge')).toBe(false);
  expect(assistantTabSession('?assistant=tab&bridge=anything')).toBe(null);
 });
});

 describe('live editor context',()=>{
 const editor={revision:4,fileId:'intro-id',filePath:'chapters/intro.tex',selection:{from:9,to:14,text:'hello'}};
 it('accepts a bounded selection and an explicitly cleared selection',()=>{expect(validEditorContext(editor)).toBe(true);expect(validEditorContext({...editor,selection:null})).toBe(true);});
 it('rejects malformed ranges, oversized content and out-of-order invalid revision values',()=>{for(const selection of [{from:-1,to:4,text:'hello'},{from:9,to:13,text:'hello'}])expect(validEditorContext({...editor,selection})).toBe(false);expect(validEditorContext({...editor,revision:NaN})).toBe(false);expect(validEditorContext({...editor,filePath:'x'.repeat(1025)})).toBe(false);});
 it('returns discussion and review to the editor current context without overwriting drafts or candidates',()=>{const restored=restoreInEditor(state,editor);expect(restored.filePath).toBe('chapters/intro.tex');expect(restored.target).toEqual(editor.selection);expect(restored.drafts).toEqual(state.drafts);expect(restored.plan).toEqual(state.plan);expect(restored.modelChoice).toBe(state.modelChoice);});
 it('keeps context traffic scoped to the same project, account and active bridge',()=>{const message={version:1,session:id,userId:'user',projectId:'project',sender:'parent',kind:'editor-context',editor};expect(validTabMessage(message,'user','project',id,'parent')).toBe(true);expect(validTabMessage(message,'user','other',id,'parent')).toBe(false);expect(validTabMessage(message,'user','project',id,'child')).toBe(false);});
});

it('makes asynchronous handoff idempotent and ignores stale active messages after return',()=>{expect(handoffDisposition(false,false)).toBe('start');expect(handoffDisposition(true,false)).toBe('ignore');expect(handoffDisposition(true,true)).toBe('grant');expect(handoffDisposition(true,false)).toBe('ignore');});

it('scopes crash-recovery drafts to the same user, project and detached session',()=>{expect(assistantCloseKey('a','p',id)).not.toBe(assistantCloseKey('b','p',id));expect(assistantCloseKey('a','p',id)).not.toBe(assistantCloseKey('a','q',id));expect(assistantCloseKey('a','p',id)).not.toBe(assistantCloseKey('a','p','other-session'));});

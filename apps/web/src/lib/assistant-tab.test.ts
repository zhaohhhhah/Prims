import { describe, expect, it } from 'vitest';
import { assistantReturnKey, assistantTabChannel, assistantTabSession, assistantTabUrl, canContinueWithoutParent, editorTabUrl, validTabMessage, validTabState, type AssistantTabState } from './assistant-tab';
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
  expect(clone.plan?.edits[0]?.after).toBe('manually adjusted');expect(clone.plan?.edits[0]?.state).toBe('pending');
 });
 it('refuses cross-thread plans and oversized or malformed transfer data',()=>{
  expect(validTabState({...state,plan:{...state.plan,threadId:'other'}})).toBe(false);
  expect(validTabState({...state,drafts:{panel:'x'.repeat(16001),selection:''}})).toBe(false);
  expect(validTabState({...state,target:{from:-1,to:4,text:'bad'}})).toBe(false);
  expect(validTabState({...state,plan:{...state.plan,edits:[{filePath:'main.tex'}]}})).toBe(false);
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

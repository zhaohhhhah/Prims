import { describe,it,expect } from 'vitest';
import { AssistantScrollPolicy,assistantBottomDistance,showAssistantTurnProgress } from './assistant-conversation';
const metrics=(scrollTop:number,scrollHeight=1200,clientHeight=400)=>({scrollTop,scrollHeight,clientHeight});
describe('assistant conversation follows the reader',()=>{
 it('starts at the latest turn and handles short content',()=>{const policy=new AssistantScrollPolicy();expect(policy.shouldFollow).toBe(true);expect(assistantBottomDistance(metrics(0,100,400))).toBe(0);});
 it('keeps following new content when the reader was at the end',()=>{const policy=new AssistantScrollPolicy();policy.scrolledToEnd(metrics(800));expect(policy.shouldFollow).toBe(true);expect(assistantBottomDistance(metrics(800,1600))).toBe(400);expect(policy.shouldFollow).toBe(true);});
 it('pauses even a small deliberate upward scroll near the end',()=>{const policy=new AssistantScrollPolicy();policy.scrolledToEnd(metrics(800));policy.onScroll(metrics(780));expect(policy.shouldFollow).toBe(false);});
 it('does not let new content or a stationary scroll event pull a reader down',()=>{const policy=new AssistantScrollPolicy();policy.scrolledToEnd(metrics(800));policy.onScroll(metrics(300));policy.onScroll(metrics(300,1800));expect(policy.shouldFollow).toBe(false);});
 it('resumes when scrolling toward the end and when explicitly reopening or sending',()=>{const policy=new AssistantScrollPolicy();policy.scrolledToEnd(metrics(800));policy.onScroll(metrics(300));policy.onScroll(metrics(760));expect(policy.shouldFollow).toBe(true);policy.pause();expect(policy.shouldFollow).toBe(false);policy.reset();expect(policy.shouldFollow).toBe(true);});
 it('keeps the prior follow choice when a taller dock clamps scrollTop at the bottom',()=>{const policy=new AssistantScrollPolicy();policy.scrolledToEnd(metrics(800));policy.onScroll(metrics(600,1200,600));expect(policy.shouldFollow).toBe(true);policy.onScroll(metrics(580,1200,600));expect(policy.shouldFollow).toBe(false);policy.onScroll(metrics(500,1200,700));expect(policy.shouldFollow).toBe(false);});
 it('wheel intent pauses before a token can resize the content',()=>{const policy=new AssistantScrollPolicy();policy.scrolledToEnd(metrics(800));policy.pause();policy.onScroll(metrics(800));expect(policy.shouldFollow).toBe(false);});
});
describe('public activity belongs to the current turn',()=>{
 it('shows ongoing work and interrupted turns',()=>{expect(showAssistantTurnProgress(true,false,'generating')).toBe(true);expect(showAssistantTurnProgress(false,true,'ready')).toBe(true);for(const phase of ['stopped','partial','error'])expect(showAssistantTurnProgress(false,false,phase)).toBe(true);});
 it('removes standalone completion states',()=>{for(const phase of ['ready','answered','preview','applied','rejected','undone'])expect(showAssistantTurnProgress(false,false,phase)).toBe(false);});
});

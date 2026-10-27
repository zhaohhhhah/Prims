import { describe,expect,it } from 'vitest';
import type { AIConfigPublic } from '@scribe/shared';
import { needsPersonalAISetup,normalizeEndpoint } from './ai-setup';

const configured={provider:'openai-compatible',model:'personal-model',baseUrl:'https://example.test/v1',apiKeyPreview:'sk-…test',updatedAt:null} as AIConfigPublic;
describe('per-account API setup',()=>{
  it('guides missing keys but leaves configured users alone',()=>{
    expect(needsPersonalAISetup(null)).toBe(true);
    expect(needsPersonalAISetup(configured)).toBe(false);
    expect(needsPersonalAISetup({...configured,apiKeyPreview:null})).toBe(true);
    expect(needsPersonalAISetup({...configured,model:''})).toBe(true);
    expect(needsPersonalAISetup({...configured,baseUrl:null})).toBe(true);
    expect(needsPersonalAISetup({...configured,provider:'openai',baseUrl:null})).toBe(false);
  });
  it('normalizes common complete endpoints without duplicating v1',()=>{
    expect(normalizeEndpoint(' https://example.test/v1/chat/completions ',true)).toBe('https://example.test/v1');
    expect(normalizeEndpoint('https://example.test/v1/responses',true)).toBe('https://example.test/v1');
    expect(normalizeEndpoint('http://localhost:1234/v1/',true)).toBe('http://localhost:1234/v1');
    expect(normalizeEndpoint('',true)).toBeNull();
  });
  it('rejects credentials, query secrets and unsupported protocols',()=>{
    for(const value of ['javascript:alert(1)','https://user:secret@example.test','https://example.test/v1?key=secret','https://example.test/#fragment','not a url'])expect(()=>normalizeEndpoint(value,true)).toThrow();
  });
});

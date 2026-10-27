import { AI_PROVIDER_NEEDS_BASE_URL, type AIConfigPublic } from '@scribe/shared';

export function needsPersonalAISetup(config:AIConfigPublic|null|undefined):boolean {
  return !config || !config.apiKeyPreview || !config.model.trim() || (AI_PROVIDER_NEEDS_BASE_URL[config.provider] && !config.baseUrl?.trim());
}

export function normalizeEndpoint(value:string,compatible:boolean):string|null {
  const text=value.trim();if(!text)return null;
  let url:URL;try{url=new URL(text);}catch{throw new Error('invalid-endpoint');}
  if(!['http:','https:'].includes(url.protocol)||url.username||url.password||url.search||url.hash)throw new Error('invalid-endpoint');
  if(compatible)url.pathname=url.pathname.replace(/\/(?:chat\/completions|responses)\/?$/,'');
  return url.toString().replace(/\/$/,'');
}

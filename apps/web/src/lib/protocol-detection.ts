import type {AIProvider} from '@scribe/shared';
import {normalizeEndpoint} from './ai-setup';
const OPENAI_HOSTS=new Set(['api.openai.com','openrouter.ai','api.deepseek.com','api.groq.com','api.together.xyz','api.together.ai','api.moonshot.cn','api.moonshot.ai','api.siliconflow.cn','api.siliconflow.com','api.mistral.ai']);
/** No network or model-name guesses: ambiguous base URLs require an explicit choice. */
export function detectEndpointProvider(value:string):AIProvider|null {
 let url:URL;try{url=new URL(value.trim());}catch{return null;}
 if(!['https:','http:'].includes(url.protocol)||url.username||url.password||url.search||url.hash)return null;
 const host=url.hostname.toLowerCase(),path=url.pathname.toLowerCase();
 if(host==='localhost'||host.endsWith('.localhost')||host==='[::1]'||host==='::1'||/^127\./.test(host)||host==='generativelanguage.googleapis.com')return null;
 const anthropic=host==='api.anthropic.com'||/(?:^|\/)anthropic(?:\/|$)/.test(path)||/\/v1\/messages\/?$/.test(path);
 // An explicit Anthropic route wins over an otherwise OpenAI-compatible host,
 // e.g. the same gateway can expose /v1 and /anthropic for different protocols.
 if(anthropic)return 'anthropic';
 if(OPENAI_HOSTS.has(host)||/(?:^|\/)openai(?:\/|$)/.test(path)||/\/chat\/completions\/?$/.test(path))return 'openai-compatible';
 return null;
}
export function resolveEndpointProvider(value:string,explicit:AIProvider|null):AIProvider|null{return explicit??detectEndpointProvider(value);}
/** Both adapters accept a base URL; pasted complete endpoint routes are stripped. */
export function normalizeProtocolEndpoint(value:string,provider:AIProvider):string|null {
 const normalized=normalizeEndpoint(value,provider==='openai'||provider==='openai-compatible');
 if(!normalized||provider!=='anthropic')return normalized;
 const url=new URL(normalized);url.pathname=url.pathname.replace(/\/(?:v1\/)?messages\/?$/i,'');
 return url.toString().replace(/\/$/,'');
}

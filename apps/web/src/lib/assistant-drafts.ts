export type AssistantDraftSource='panel'|'selection';
export interface AssistantDrafts {panel:string;selection:string}
export const emptyAssistantDrafts=():AssistantDrafts=>({panel:'',selection:''});
export function editAssistantDraft(drafts:AssistantDrafts,source:AssistantDraftSource,text:string):AssistantDrafts{return {...drafts,[source]:text};}
export function consumeAssistantDraft(drafts:AssistantDrafts,source:AssistantDraftSource):AssistantDrafts{return editAssistantDraft(drafts,source,'');}
export function restoreAssistantDraft(drafts:AssistantDrafts,source:AssistantDraftSource,text:string):AssistantDrafts{return drafts[source]?drafts:editAssistantDraft(drafts,source,text);}

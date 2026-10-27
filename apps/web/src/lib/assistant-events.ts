export interface AssistantDiagnostic {filePath:string;line:number;column?:number;message:string}
export interface AssistantOpenDetail {projectId?:string;dock?:boolean;prompt?:string;ocrText?:string;source?:'compile'|'restore'|'project-edits';targetFilePath?:string;diagnostic?:AssistantDiagnostic;restore?:{filePath:string;before:string;after:string;versionId:string}}
export const ASSISTANT_OPEN_EVENT='scribe:project-assistant-open';
export function openProjectAssistant(detail:AssistantOpenDetail={}){window.dispatchEvent(new CustomEvent<AssistantOpenDetail>(ASSISTANT_OPEN_EVENT,{detail}));}

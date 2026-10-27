import { parseProjectActions, type ProjectActionInput } from './project-actions';
export interface InlineFileEdit {filePath:string;before:string;after:string}
export interface InlineProposal {reply:string;replacement:string|null;edits:InlineFileEdit[];actions:ProjectActionInput[]}
/** Complete, bounded proposals must target explicitly authorized project text paths. */
export function parseInlineProposal(raw:string,expectedFilePath?:string,allowedFilePaths?:readonly string[]):InlineProposal {
  const clean=raw.trim().replace(/^```(?:json)?\s*/i,'').replace(/\s*```$/,'');
  let data:unknown;
  try {data=JSON.parse(clean);}catch {throw new Error('模型没有返回完整的修改方案，原文未改动。可以重试或换个模型。');}
  if(!data||typeof data!=='object'||!('reply' in data)||typeof data.reply!=='string'||!('replacement' in data)||(data.replacement!==null&&typeof data.replacement!=='string'))throw new Error('模型返回格式不正确，原文未改动');
  if(typeof data.replacement==='string'&&data.replacement.length>100000)throw new Error('生成结果过长，原文未改动');
  const edits:InlineFileEdit[]=[];
  if('edits' in data){
    if(!Array.isArray(data.edits)||data.edits.length>48)throw new Error('一次修改最多支持 48 处，原文未改动');
    let total=0;
    for(const item of data.edits){
      if(!item||typeof item!=='object'||typeof item.filePath!=='string'||typeof item.before!=='string'||typeof item.after!=='string')throw new Error('修改建议缺少文件路径、原文或新文，原文未改动');
      if(!(allowedFilePaths?allowedFilePaths.includes(item.filePath):!!expectedFilePath&&item.filePath===expectedFilePath)||item.filePath.startsWith('/')||item.filePath.includes('\\')||item.filePath.includes(':')||item.filePath.split('/').some((part:string)=>!part||part==='..'||part==='.')||item.filePath.split('/')[0]==='.git'||new TextEncoder().encode(item.filePath).length>512||/[\u0000-\u001f\u007f]/.test(item.filePath))throw new Error('只能预览当前项目内已授权文本文件的修改');
      if(!item.before.trim()||item.before.length>16000||item.after.length>24000)throw new Error('修改片段为空或过长，请缩小修改范围');
      total+=item.before.length+item.after.length;if(total>80000)throw new Error('本轮修改内容过长，请分次修改');
      edits.push({filePath:item.filePath,before:item.before,after:item.after});
    }
  }
  return {reply:data.reply,replacement:edits.length?null:data.replacement,edits,actions:parseProjectActions('actions' in data?data.actions:[])};
}

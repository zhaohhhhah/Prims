import { parseProjectActions, type ProjectActionInput } from './project-actions';
export interface InlineFileEdit {filePath:string;before:string;after:string}
export interface InlineProposal {reply:string;replacement:string|null;edits:InlineFileEdit[];actions:ProjectActionInput[]}
/** Only complete, bounded proposals for this exact open file are eligible for preview. */
export function parseInlineProposal(raw:string,expectedFilePath?:string):InlineProposal {
  const clean=raw.trim().replace(/^```(?:json)?\s*/i,'').replace(/\s*```$/,'');
  let data:unknown;
  try {data=JSON.parse(clean);}catch {throw new Error('模型没有返回完整的修改方案，原文未改动。可以重试或换个模型。');}
  if(!data||typeof data!=='object'||!('reply' in data)||typeof data.reply!=='string'||!('replacement' in data)||(data.replacement!==null&&typeof data.replacement!=='string'))throw new Error('模型返回格式不正确，原文未改动');
  if(typeof data.replacement==='string'&&data.replacement.length>100000)throw new Error('生成结果过长，原文未改动');
  const edits:InlineFileEdit[]=[];
  if('edits' in data){
    if(!Array.isArray(data.edits)||data.edits.length>12)throw new Error('一次修改最多支持 12 处，原文未改动');
    let total=0;
    for(const item of data.edits){
      if(!item||typeof item!=='object'||typeof item.filePath!=='string'||typeof item.before!=='string'||typeof item.after!=='string')throw new Error('修改建议缺少文件路径、原文或新文，原文未改动');
      if(!expectedFilePath||item.filePath!==expectedFilePath||item.filePath.startsWith('/')||item.filePath.includes('\\')||item.filePath.split('/').some((part:string)=>part==='..'||part==='.')||/[\u0000-\u001f]/.test(item.filePath))throw new Error('只能预览当前文件的修改，其他文件不会被写入');
      if(!item.before.trim()||item.before.length>16000||item.after.length>24000)throw new Error('修改片段为空或过长，请缩小修改范围');
      total+=item.before.length+item.after.length;if(total>80000)throw new Error('本轮修改内容过长，请分次修改');
      edits.push({filePath:item.filePath,before:item.before,after:item.after});
    }
  }
  return {reply:data.reply,replacement:edits.length?null:data.replacement,edits,actions:parseProjectActions('actions' in data?data.actions:[])};
}

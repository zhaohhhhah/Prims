import DiffMatchPatch from 'diff-match-patch';
export interface VersionReviewEdit {before:string;after:string;from:number;to:number}
/** Builds small exact, unique review anchors; never falls back to a blind whole-file write. */
export function buildVersionReview(before:string,after:string):VersionReviewEdit[]{
 if(before===after)return [];if(!before.trim())throw new Error('当前文件为空，请先手动输入或粘贴版本内容。');
 const engine=new DiffMatchPatch();engine.Diff_Timeout=.5;const diffs=engine.diff_main(before,after);engine.diff_cleanupSemantic(diffs);
 const changes:{from:number;to:number;insert:string}[]=[];let position=0,pending:{from:number;to:number;insert:string}|null=null;
 for(const [kind,text] of diffs){if(kind===0){if(pending){changes.push(pending);pending=null;}position+=text.length;}else{pending??={from:position,to:position,insert:''};if(kind<0){position+=text.length;pending.to=position;}else pending.insert+=text;}}
 if(pending)changes.push(pending);
 const ranges=changes.map(change=>{let radius=64,from=0,to=0;for(let attempt=0;attempt<9;attempt++){from=Math.max(0,change.from-radius);to=Math.min(before.length,change.to+radius);if(from>0&&/[\uDC00-\uDFFF]/.test(before[from]!))from--;if(to<before.length&&/[\uDC00-\uDFFF]/.test(before[to]!))to++;const anchor=before.slice(from,to);if(anchor.length<=16000&&before.indexOf(anchor)===from&&before.indexOf(anchor,from+1)===-1)return {from,to};radius*=2;}throw new Error('版本差异无法唯一定位，请对照版本内容手动恢复。');});
 const merged:{from:number;to:number}[]=[];for(const range of [...ranges].sort((a,b)=>a.from-b.from)){const last=merged.at(-1);if(last&&range.from<=last.to)last.to=Math.max(last.to,range.to);else merged.push({...range});}
 if(merged.length>12)throw new Error('版本差异超过 12 处，请分段对照恢复。');
 return merged.map(range=>{const original=before.slice(range.from,range.to);let result='',cursor=range.from;for(const change of changes)if(change.from>=range.from&&change.to<=range.to){result+=before.slice(cursor,change.from)+change.insert;cursor=change.to;}result+=before.slice(cursor,range.to);if(original.length>16000||result.length>24000)throw new Error('这段版本差异过长，请对照版本内容手动恢复。');return {before:original,after:result,...range};});
}

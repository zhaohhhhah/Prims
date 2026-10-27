/** Read-only context around the current target. Offsets use CodeMirror/JS UTF-16 units. */
export interface InlineWritingContext {
  document: string;
  documentTruncated: boolean;
  documentWindowStartLine: number;
  documentWindowEndLine: number;
  selectionStartLine: number;
  selectionEndLine: number;
  currentSection: string;
  documentOutline: string;
}
function lineNumber(text:string,offset:number):number {let line=1;for(let i=0;i<offset;i++)if(text.charCodeAt(i)===10)line++;return line;}
function withoutComment(line:string):string {
  for(let i=0;i<line.length;i++)if(line[i]==='%'){let slashes=0;for(let j=i-1;j>=0&&line[j]==='\\';j--)slashes++;if(slashes%2===0)return line.slice(0,i);}
  return line;
}
function boundary(text:string,offset:number,direction:-1|1):number {
  if(offset>0&&offset<text.length){const a=text.charCodeAt(offset-1),b=text.charCodeAt(offset);if(a>=0xd800&&a<=0xdbff&&b>=0xdc00&&b<=0xdfff)return offset+direction;}
  return offset;
}
export function getInlineWritingContext(text:string,from:number,to:number,limit=32000):InlineWritingContext {
  from=Math.max(0,Math.min(text.length,from));to=Math.max(from,Math.min(text.length,to));limit=Math.max(1,limit);
  let start=0,end=text.length;
  if(text.length>limit){
    const room=Math.max(0,limit-(to-from));start=Math.max(0,from-Math.floor(room/2));end=Math.min(text.length,start+limit);
    if(end<to){end=to;start=Math.max(0,end-limit);}if(end===text.length)start=Math.max(0,end-limit);
    start=boundary(text,start,1);end=boundary(text,end,-1);
  }
  const outline:string[]=[];const hierarchy:Array<{level:number;title:string}>=[];
  let offset=0,line=1;
  for(const sourceLine of text.split('\n')){
    const clean=withoutComment(sourceLine);const match=/^\s*\\(chapter|section|subsection|subsubsection)\*?\s*(?:\[[^\]]*\])?\s*\{(.+)\}/.exec(clean);
    if(match){
      const level=['chapter','section','subsection','subsubsection'].indexOf(match[1]!);const title=match[2]!.replace(/\}\s*.*$/,'').trim().slice(0,160);
      if(outline.length<60)outline.push(`${'  '.repeat(level)}L${line} ${title}`);
      if(offset<=from){while(hierarchy.length&&hierarchy[hierarchy.length-1]!.level>=level)hierarchy.pop();hierarchy.push({level,title});}
    }
    offset+=sourceLine.length+1;line++;
  }
  return {document:text.slice(start,end),documentTruncated:start>0||end<text.length,
    documentWindowStartLine:lineNumber(text,start),documentWindowEndLine:lineNumber(text,end),
    selectionStartLine:lineNumber(text,from),selectionEndLine:lineNumber(text,to),
    currentSection:hierarchy.length?hierarchy.map(item=>item.title).join(' › ').slice(0,500):'文档开头',documentOutline:outline.join('\n').slice(0,10000)};
}

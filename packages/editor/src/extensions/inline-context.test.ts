import { describe,expect,it } from 'vitest';
import { getInlineWritingContext } from './inline-context';
describe('selection-aware writing context',()=>{
 it('extracts section hierarchy, line range and ignores commented headings',()=>{
  const text='\\section{Introduction}\nContext\n% \\section{Not real}\n\\subsection{Study design}\nSelected sentence\n';const start=text.indexOf('Selected');const ctx=getInlineWritingContext(text,start,start+8);
  expect(ctx.currentSection).toBe('Introduction › Study design');expect(ctx.selectionStartLine).toBe(5);expect(ctx.documentOutline).not.toContain('Not real');expect(ctx.document).toBe(text);expect(ctx.documentTruncated).toBe(false);
 });
 it('sends the complete unsaved document while retaining the late selection location',()=>{
  const text='opening\n'+'A'.repeat(50000)+'\n\\section{Results}\nREAD_THIS_TARGET\n'+'Z'.repeat(20000);const start=text.indexOf('READ_THIS_TARGET');const ctx=getInlineWritingContext(text,start,start+16);
  expect(ctx.document).toContain('READ_THIS_TARGET');expect(ctx.document).toBe(text);expect(ctx.currentSection).toBe('Results');expect(ctx.documentTruncated).toBe(false);expect(ctx.documentWindowStartLine).toBe(1);
 });
 it('does not split a Unicode surrogate pair at a window boundary',()=>{
  const text='😀'.repeat(100)+'target'+'😀'.repeat(100);const from=text.indexOf('target');const ctx=getInlineWritingContext(text,from,from+6,31);
  expect(ctx.document).toContain('target');expect(ctx.document.length).toBeLessThanOrEqual(31);expect(ctx.document.charCodeAt(0)).not.toBeGreaterThanOrEqual(0xdc00);const last=ctx.document.charCodeAt(ctx.document.length-1);expect(last>=0xd800&&last<=0xdbff).toBe(false);
 });
});

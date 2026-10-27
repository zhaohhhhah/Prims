import { describe,expect,it } from 'vitest';
import { parseInlineProposal } from './inline-result';

describe('inline proposal protocol',()=>{
  it('preserves exact LaTeX and line breaks for legacy selected replacements',()=>{
    const replacement='\\section{中文}\nA & B';
    expect(parseInlineProposal(JSON.stringify({reply:'说明',replacement}))).toEqual({reply:'说明',replacement,edits:[]});
  });
  it('supports answers without edits and an explicitly proposed deletion',()=>{
    expect(parseInlineProposal('{"reply":"解释","replacement":null}').replacement).toBeNull();
    expect(parseInlineProposal('{"reply":"删除冗余文字","replacement":""}').replacement).toBe('');
  });
  it('accepts JSON fences but never prose, truncated JSON or malformed edits',()=>{
    expect(parseInlineProposal('```json\n{"reply":"ok","replacement":"text"}\n```').replacement).toBe('text');
    for(const raw of ['Here is a replacement','{"reply":"ok","replacement":"partial','{"reply":"ok"}','{"reply":"ok","replacement":false}','{"reply":"ok","replacement":null,"edits":null}'])expect(()=>parseInlineProposal(raw)).toThrow();
  });
  it('keeps multiple exact changes in the current file and prevents double application',()=>{
    const edits=[{filePath:'main.tex',before:'one\n',after:'ONE\n'},{filePath:'main.tex',before:'two',after:''}];
    expect(parseInlineProposal(JSON.stringify({reply:'两处修改',replacement:'legacy ignored',edits}),'main.tex')).toEqual({reply:'两处修改',replacement:null,edits});
  });
  it('rejects cross-file, unknown, absolute and traversal destinations',()=>{
    for(const filePath of ['other.tex','/main.tex','../main.tex','a/../main.tex','a\\main.tex']){
      expect(()=>parseInlineProposal(JSON.stringify({reply:'ok',replacement:null,edits:[{filePath,before:'old',after:'new'}]}),'main.tex')).toThrow();
    }
    expect(()=>parseInlineProposal(JSON.stringify({reply:'ok',replacement:null,edits:[{filePath:'main.tex',before:'old',after:'new'}]}))).toThrow();
  });
  it('bounds edit count, anchor size and aggregate payload',()=>{
    const edit={filePath:'main.tex',before:'old',after:'new'};
    for(const edits of [Array.from({length:13},()=>edit),[{...edit,before:' '}],[{...edit,before:'a'.repeat(16001)}],[{...edit,after:'b'.repeat(24001)}],Array.from({length:3},()=>({...edit,before:'a'.repeat(16000),after:'b'.repeat(24000)}))])expect(()=>parseInlineProposal(JSON.stringify({reply:'ok',replacement:null,edits}),'main.tex')).toThrow();
  });
});

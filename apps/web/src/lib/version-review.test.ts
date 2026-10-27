import {describe,expect,it} from 'vitest';
import {buildVersionReview} from './version-review';
function apply(before:string,edits:ReturnType<typeof buildVersionReview>){let result=before;for(const edit of [...edits].reverse())result=result.slice(0,edit.from)+edit.after+result.slice(edit.to);return result;}
describe('version restoration is a review plan',()=>{
 it('returns no proposal for unchanged text',()=>expect(buildVersionReview('same','same')).toEqual([]));
 it('reconstructs the chosen version through exact non-overlapping candidates',()=>{const before='Introduction '+'.'.repeat(200)+' original A '+'.'.repeat(200)+' original B End';const after=before.replace('original A','restored A').replace('original B','restored B');const edits=buildVersionReview(before,after);expect(edits.length).toBeGreaterThan(0);expect(apply(before,edits)).toBe(after);for(const edit of edits){expect(before.indexOf(edit.before,edit.from+1)).toBe(-1);expect(edit.before.length).toBeLessThanOrEqual(16000);}});
 it('supports explicit insertion and deletion while keeping a real source anchor',()=>{for(const [before,after] of [['alpha beta','new alpha beta'],['alpha beta','alpha'],['before\nafter','before\ninserted\nafter']])expect(apply(before!,buildVersionReview(before!,after!))).toBe(after);});
 it('does not generate a blind replacement for an empty or oversized change',()=>{expect(()=>buildVersionReview('','restored')).toThrow();expect(()=>buildVersionReview('a'.repeat(20000),'b'.repeat(30000))).toThrow();});
});

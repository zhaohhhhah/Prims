import {describe,expect,it} from 'vitest';
import {spreadStart,spreadPages,bookTurnPages} from './book-spreads';
describe('real PDF spread navigation',()=>{
 it('uses the physically corresponding front/back pages in both directions',()=>{expect(bookTurnPages(0,2,2)).toEqual({forward:true,front:2,back:3,stationary:1});expect(bookTurnPages(2,0,2)).toEqual({forward:false,front:3,back:2,stationary:4});expect(bookTurnPages(4,5,1)).toEqual({forward:true,front:5,back:6,stationary:5});});
 it('clamps to real odd/even PDF page counts',()=>{expect(spreadStart(100,2,5)).toBe(4);expect(spreadPages(4,2,5)).toEqual([5]);expect(spreadPages(2,2,4)).toEqual([3,4]);});
 it('range jumps target exactly one final spread, without a queued sequence',()=>{expect(spreadStart(36,2,80)).toBe(36);expect(spreadStart(-3,2,80)).toBe(0);expect(spreadStart(5,2,80)).toBe(4);});
 it('normalises responsive spread changes while keeping the current page visible',()=>{expect(spreadStart(5,2,10)).toBe(4);expect(spreadStart(5,1,10)).toBe(5);expect(spreadPages(0,1,1)).toEqual([1]);});
});

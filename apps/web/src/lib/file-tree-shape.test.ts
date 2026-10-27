import type {ProjectFile} from '@scribe/shared';
import {describe,expect,it} from 'vitest';
import {buildFileTree} from './file-tree-shape';
describe('explicit project directories',()=>{
 it('shows nested empty directories without creating placeholder files',()=>{expect(buildFileTree([],['figures/drafts'])).toEqual([{type:'folder',name:'figures',path:'figures',children:[{type:'folder',name:'drafts',path:'figures/drafts',children:[]}]}]);});
 it('merges explicit and file-derived folders without duplicate nodes',()=>{const files=[{id:'file',path:'figures/chart.tex',type:'tex'} as ProjectFile];const tree=buildFileTree(files,['figures','figures/drafts']);expect(tree).toHaveLength(1);expect(tree[0]).toMatchObject({type:'folder',path:'figures'});if(tree[0]?.type==='folder')expect(tree[0].children.map(item=>item.name)).toEqual(['drafts','chart.tex']);});
 it('ignores unsafe directory paths and keeps legacy placeholders hidden',()=>{const files=[{id:'file',path:'old/.gitkeep',type:'other'} as ProjectFile];const tree=buildFileTree(files,['../outside','/absolute','a\\b']);expect(tree).toEqual([{type:'folder',name:'old',path:'old',children:[]}]);});
});

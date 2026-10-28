import { describe, expect, it } from 'vitest';
import { projectFolderPaths } from './project-folders';
describe('projectFolderPaths', () => {
 it('offers project root, nested file ancestors and explicit empty folders without duplicates', () => {
  expect(projectFolderPaths([{path:'main.tex'},{path:'chapters/part/one.tex'},{path:'chapters/two.tex'}],['empty','chapters/part'])).toEqual(['','chapters','chapters/part','empty']);
 });
 it('keeps international folder names and numeric ordering', () => {
  expect(projectFolderPaths([{path:'资料/插图/a.png'}],['chapter10','chapter2'])).toEqual(expect.arrayContaining(['','资料','资料/插图','chapter2','chapter10']));
  const paths=projectFolderPaths([],['chapter10','chapter2']);expect(paths.indexOf('chapter2')).toBeLessThan(paths.indexOf('chapter10'));
 });
 it('does not turn traversal fragments into selectable destinations', () => {
  expect(projectFolderPaths([{path:'../elsewhere/a.tex'}],['normal/../../outside'])).toEqual(['']);
 });
});

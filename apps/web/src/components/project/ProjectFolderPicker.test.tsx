import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { ProjectFolderPicker } from './ProjectFolderPicker';
vi.mock('react-i18next',()=>({useTranslation:()=>({i18n:{language:'zh-CN'}})}));
describe('ProjectFolderPicker',()=>{
 it('renders clickable existing folders and project root, with the chosen folder checked',()=>{
  const html=renderToStaticMarkup(createElement(ProjectFolderPicker,{files:[{path:'chapters/intro.tex'}],directories:['empty'],value:'chapters',onChange:vi.fn()}));
  expect(html).toContain('项目根目录');expect(html).toContain('empty');expect(html).toContain('type="radio"');expect(html).toMatch(/checked=""[^>]*value="chapters"|value="chapters"[^>]*checked=""/);
  expect(html).not.toContain('chapters/archive');
 });
});

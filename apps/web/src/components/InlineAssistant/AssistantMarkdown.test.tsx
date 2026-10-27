import {renderToStaticMarkup} from 'react-dom/server';
import {describe,expect,it} from 'vitest';
import {AssistantMarkdown} from './AssistantMarkdown';
const render=(text:string)=>renderToStaticMarkup(<AssistantMarkdown text={text}/>);
describe('public assistant Markdown rendering',()=>{
 it('renders headings, lists, emphasis and GFM tables',()=>{const html=render('## 修改思路\n\n- **第一点**\n- 第二点\n\n| A | B |\n| - | - |\n| 1 | 2 |');expect(html).toContain('<h2');expect(html).toContain('<ul');expect(html).toContain('<strong>第一点</strong>');expect(html).toContain('<table');});
 it('preserves literal LaTeX in fenced and inline code',()=>{const tex='\\section{Results}\n\\textbf{A & B}\n$x_1 \\leq y$';const html=render('```latex\n'+tex+'\n```\n\nUse `\\cite{smith}`.');expect(html).toContain('language-latex');expect(html).toContain('\\section{Results}\n\\textbf{A &amp; B}\n$x_1 \\leq y$');expect(html).toContain('\\cite{smith}');expect(html).toContain('overflow-x-auto');});
 it('does not execute raw HTML or unsafe URL schemes',()=>{const html=render('<script>alert(1)</script>\n\n<img src=x onerror="alert(1)">\n\n[bad](javascript:alert%281%29)');expect(html).not.toContain('<script');expect(html).not.toContain('<img');expect(html).not.toContain('onerror');expect(html).not.toContain('href="javascript:');});
 it('opens external links safely and avoids automatic remote image requests',()=>{const html=render('[文档](https://example.org/docs)\n\n![图示](https://tracker.invalid/image.png)');expect(html).toContain('href="https://example.org/docs"');expect(html).toContain('target="_blank"');expect(html).toContain('rel="noopener noreferrer"');expect(html).not.toContain('<img');expect(html).not.toContain('tracker.invalid');expect(html).toContain('[图片：图示]');});
});

import { renderToStaticMarkup } from 'react-dom/server';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
const fixture=vi.hoisted(()=>({admin:false}));
vi.mock('../hooks/useDocumentTitle',()=>({useDocumentTitle:()=>{}}));
vi.mock('../stores/auth',()=>({useAuthStore:(select:any)=>select({user:{id:'test-only'}})}));
vi.mock('../lib/admin',()=>({adminApi:{me:()=>{},users:()=>{},invites:()=>{}}}));
vi.mock('react-i18next',()=>({useTranslation:()=>({t:(key:string)=>key})}));
vi.mock('@tanstack/react-query',()=>({
 useQuery:({queryKey}:any)=>({data:queryKey[0]==='admin-access'?{isAdmin:fixture.admin}:[],isPending:false,isError:false}),
 useQueryClient:()=>({invalidateQueries:()=>Promise.resolve()})
}));
vi.mock('./settings/ProfileSettingsTab',()=>({ProfileSettingsTab:()=> <p>profile-content</p>}));
vi.mock('./settings/EditorSettingsTab',()=>({EditorSettingsTab:()=> <p>editor-content</p>}));
vi.mock('./settings/AISettingsTab',()=>({AISettingsTab:()=> <p>ai-content</p>}));
import { SettingsPage } from './SettingsPage';
function render(){return renderToStaticMarkup(<MemoryRouter initialEntries={['/settings?tab=admin']}><SettingsPage/></MemoryRouter>);}
describe('settings administration access',()=>{
 beforeEach(()=>{fixture.admin=false;});
 it('hides the administration tab and content from ordinary accounts',()=>{const html=render();expect(html).not.toContain('settings.admin.title');expect(html).not.toContain('生成邀请码');expect(html).toContain('profile-content');});
 it('embeds invitation and account administration for the existing administrator',()=>{fixture.admin=true;const html=render();expect(html).toContain('settings.admin.title');expect(html).toContain('生成邀请码');expect(html).toContain('账号列表');expect(html).not.toContain('profile-content');});
});

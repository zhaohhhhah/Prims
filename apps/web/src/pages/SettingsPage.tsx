import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle } from '@scribe/ui';
import { useQuery } from '@tanstack/react-query';
import { Bot, Palette, ShieldCheck, Sparkles, Type, UserCircle2, X, type LucideIcon } from 'lucide-react';
import { useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { useDocumentTitle } from '../hooks/useDocumentTitle';
import { adminApi } from '../lib/admin';
import { useAuthStore } from '../stores/auth';
import { AdminPage } from './AdminPage';
import { AppearanceSettingsTab } from './settings/AppearanceSettingsTab';
import { AISettingsTab } from './settings/AISettingsTab';
import { AgentSettingsTab } from './settings/AgentSettingsTab';
import { agentLibraryCopy } from '../locales/agent-library-copy';
import { EditorSettingsTab } from './settings/EditorSettingsTab';
import { ProfileSettingsTab } from './settings/ProfileSettingsTab';

type TabId='profile'|'appearance'|'editor'|'ai'|'agent'|'admin';
interface TabDef {id:TabId;icon:LucideIcon;labelKey:string;descriptionKey:string}
const tabs:TabDef[]=[
  {id:'profile',icon:UserCircle2,labelKey:'settings.profile.title',descriptionKey:'settings.profile.description'},
  {id:'appearance',icon:Palette,labelKey:'settings.appearance.title',descriptionKey:'settings.appearance.description'},
  {id:'editor',icon:Type,labelKey:'settings.editor.title',descriptionKey:'settings.editor.description'},
  {id:'ai',icon:Sparkles,labelKey:'settings.ai.title',descriptionKey:'settings.ai.description'},
  {id:'agent',icon:Bot,labelKey:'settings.agent.title',descriptionKey:'settings.agent.description'},
];
const adminTab:TabDef={id:'admin',icon:ShieldCheck,labelKey:'settings.admin.title',descriptionKey:'settings.admin.description'};
export function visibleSettingsTabs(isAdmin:boolean):TabDef[] {return isAdmin?[...tabs,adminTab]:tabs;}

export function SettingsPage() {
  const {t,i18n}=useTranslation();const agentCopy=agentLibraryCopy(i18n?.resolvedLanguage);useDocumentTitle(t('settings.title'));
  const userId=useAuthStore(state=>state.user?.id);
  const access=useQuery({queryKey:['admin-access',userId],queryFn:adminApi.me,enabled:!!userId});
  const [params,setParams]=useSearchParams();
  const available=visibleSettingsTabs(access.data?.isAdmin===true);
  const requested=params.get('tab')??'profile';
  const tab=available.find(item=>item.id===requested)??tabs[0]!;
  const active=tab.id;
  const navigate=useNavigate();
  const close=useCallback(()=>{if(window.history.length>1)void navigate(-1);else void navigate('/dashboard');},[navigate]);
  useEffect(()=>{
    const handler=(event:KeyboardEvent)=>{if(event.key==='Escape'&&!event.defaultPrevented){event.preventDefault();close();}};
    window.addEventListener('keydown',handler);return()=>window.removeEventListener('keydown',handler);
  },[close]);
  return <div className={`container py-6 sm:py-8 ${active==='admin'?'max-w-6xl':'max-w-4xl'}`}>
    <div className="mb-4 flex items-center justify-between"><h1 className="text-xl font-semibold">{t('settings.title')}</h1><Button variant="ghost" size="icon" aria-label={t('common.close')} title={`${t('common.close')} · Esc`} onClick={close} className="h-8 w-8"><X className="h-4 w-4"/></Button></div>
    <div className="grid gap-4 md:grid-cols-[180px_minmax(0,1fr)] md:gap-6">
      <nav className="flex flex-wrap gap-1 self-start md:flex-col">{available.map(item=>{const Icon=item.icon;return <button key={item.id} type="button" aria-pressed={active===item.id} className={`flex items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors md:w-full ${active===item.id?'bg-accent font-medium text-accent-foreground':'text-muted-foreground hover:bg-accent/60'}`} onClick={()=>setParams({tab:item.id},{replace:true})}><Icon className="h-4 w-4"/>{item.id==='agent'?agentCopy.title:t(item.labelKey)}</button>;})}</nav>
      <Card className="min-w-0"><CardHeader><CardTitle>{active==='agent'?agentCopy.title:t(tab.labelKey)}</CardTitle><CardDescription>{active==='agent'?agentCopy.description:t(tab.descriptionKey)}</CardDescription></CardHeader><CardContent>
        {requested==='admin'&&access.isPending?<p>{t('common.loading')}…</p>:<>
          {active==='profile'&&<ProfileSettingsTab/>}{active==='appearance'&&<AppearanceSettingsTab/>}{active==='editor'&&<EditorSettingsTab/>}{active==='ai'&&<AISettingsTab/>}{active==='agent'&&<AgentSettingsTab/>}{active==='admin'&&access.data?.isAdmin===true&&<AdminPage embedded/>}
        </>}
      </CardContent></Card>
    </div>
    <footer className="mt-6 flex flex-wrap gap-x-4 gap-y-2 border-t pt-4 text-xs text-muted-foreground"><a href="https://github.com/zhaohhhhah/Prims" target="_blank" rel="noopener noreferrer" className="underline underline-offset-2 hover:text-foreground">{t('settings.sourceCode')}</a><a href="https://github.com/sunnyallana/Scribe" target="_blank" rel="noopener noreferrer" className="underline underline-offset-2 hover:text-foreground">{t('settings.basedOn')}</a></footer>
  </div>;
}

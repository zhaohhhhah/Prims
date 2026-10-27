import { Button, Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@scribe/ui';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';

import { SUPPORTED_LANGUAGES } from '../../i18n';
import { api } from '../../lib/api';
import { needsPersonalAISetup } from '../../lib/ai-setup';
import { AISettingsTab } from '../../pages/settings/AISettingsTab';
import { useAuthStore } from '../../stores/auth';

export function AISetupOnboarding() {
  const {t,i18n}=useTranslation();
  const userId=useAuthStore(state=>state.user?.id);
  const {pathname}=useLocation();
  const [dismissed,setDismissed]=useState(false);
  const config=useQuery({queryKey:['ai-config',userId],queryFn:api.ai.getConfig,enabled:!!userId,staleTime:30000});
  useEffect(()=>setDismissed(false),[userId]);
  const open=!!userId&&config.isSuccess&&needsPersonalAISetup(config.data)&&!dismissed&&!pathname.startsWith('/settings')&&!pathname.startsWith('/admin');
  return <Dialog open={open} onOpenChange={next=>{if(!next)setDismissed(true);}}>
    <DialogContent className="max-h-[90vh] max-w-xl overflow-y-auto">
      <DialogHeader><DialogTitle>{t('onboarding.title')}</DialogTitle><DialogDescription>{t('onboarding.description')}</DialogDescription></DialogHeader>
      <label className="flex items-center justify-between gap-3 text-xs text-muted-foreground">{t('settings.language')}<select aria-label={t('settings.language')} className="rounded-md border bg-background p-1.5 text-foreground" value={i18n.language} onChange={event=>{void i18n.changeLanguage(event.target.value);}}>{SUPPORTED_LANGUAGES.map(language=><option key={language.code} value={language.code}>{language.label}</option>)}</select></label>
      <AISettingsTab onboarding onSaved={()=>setDismissed(true)}/>
      <div className="space-y-2 border-t pt-3"><p className="text-xs text-muted-foreground">{t('onboarding.privacy')}</p><p className="text-xs text-muted-foreground">{t('onboarding.hint')}</p><Button type="button" variant="ghost" className="w-full" onClick={()=>setDismissed(true)}>{t('onboarding.later')}</Button></div>
    </DialogContent>
  </Dialog>;
}

import { useTheme } from '@scribe/ui';
import { Monitor, Moon, Sun } from 'lucide-react';
import { useTranslation } from 'react-i18next';
const options=[{id:'light',icon:Sun,label:'theme.light'},{id:'dark',icon:Moon,label:'theme.dark'},{id:'system',icon:Monitor,label:'theme.system'}] as const;
export function AppearanceSettingsTab(){
 const {theme,setTheme,resolvedTheme}=useTheme();const {t}=useTranslation();
 return <div className="space-y-4"><fieldset className="space-y-3"><legend className="mb-3 text-sm font-medium">{t('theme.label')}</legend><div className="grid gap-3 sm:grid-cols-3">{options.map(({id,icon:Icon,label})=><label key={id} className={`flex cursor-pointer items-center gap-3 rounded-lg border p-4 transition-colors focus-within:ring-2 focus-within:ring-ring ${theme===id?'border-primary bg-accent':'hover:bg-accent/50'}`}><input type="radio" name="appearance-theme" value={id} checked={theme===id} onChange={()=>setTheme(id)} aria-label={t(label)}/><Icon className="h-4 w-4" aria-hidden="true"/><span className="text-sm">{t(label)}</span></label>)}</div></fieldset><p className="text-sm text-muted-foreground">{t('settings.appearance.hint')}</p><p role="status" className="text-xs text-muted-foreground">{t('settings.appearance.current', {theme:t(resolvedTheme==='light'?'theme.light':'theme.dark')})}</p></div>;
}

import {useTheme} from '@scribe/ui';
import {FileText,Folder,Monitor,Moon,Sparkles,Sun} from 'lucide-react';
import {useTranslation} from 'react-i18next';
import {PrimsBrand} from '../../components/Appearance/PrimsBrand';
import {APPEARANCE_SERIES,BOOK_TITLE_STYLES,BRAND_STYLES,CODE_FONTS,ICON_STYLES,UI_FONTS,type AppearancePalette,type AppearanceSeries} from '../../lib/appearance-catalog';
import {type AppearancePreferences} from '../../lib/appearance-state';
import {APPEARANCE_THEMES,selectedAppearanceTheme} from '../../lib/appearance-themes';
import {useAppearanceStore} from '../../stores/appearance';
const modes=[{id:'light',Icon:Sun},{id:'dark',Icon:Moon},{id:'system',Icon:Monitor}] as const;
const selectClass='h-9 w-full rounded-md border bg-background px-2 text-sm text-foreground';
function ThemePreview({palette,chinese}:{palette:AppearancePalette;chinese:boolean}){
 const classic=palette.id==='classic-light'?['#d6d6d6','#c4c4c4','#e2e2e2']:palette.id==='classic-dark'?['#363636','#454545','#303030']:null;
 return <span className="flex h-14 min-w-0 items-end justify-center gap-1 rounded-md border border-black/10 px-1.5 py-2" style={{backgroundColor:palette.tokens.side}} aria-hidden="true">{[0,1,2].map(index=><span key={index} className="flex min-w-0 flex-1 items-center justify-center rounded-sm" style={{height:[30,35,32][index],backgroundColor:classic?.[index]??palette.tokens[`cover-${index}`]??palette.swatches[index+1],color:palette.tokens[`cover-ink-${index}`]??palette.tokens.ink,boxShadow:'inset 2px 0 0 #00000018, 1px 2px 3px #00000018',fontFamily:'var(--prims-book-title-font,serif)',fontSize:12}}>{chinese?'文':'Aa'}</span>)}</span>;
}
export function AppearanceSettingsTab(){
 const {t,i18n}=useTranslation('appearance');const {theme,setTheme}=useTheme();const preferences=useAppearanceStore();const chinese=i18n.language.startsWith('zh');
 const selectedTheme=selectedAppearanceTheme(preferences);
 const currentName=selectedTheme?(chinese?selectedTheme.name:selectedTheme.nameEn):t('custom');
 const choose=<K extends keyof AppearancePreferences>(key:K,value:AppearancePreferences[K])=>preferences.setPreference(key,value);
 return <div className="space-y-7">
  <section aria-labelledby="appearance-colors">
   <div className="flex flex-wrap items-center justify-between gap-3"><h3 id="appearance-colors" className="text-sm font-medium">{t('themes')}</h3><select aria-label={t('series')} value={preferences.paletteSeries} onChange={event=>choose('paletteSeries',event.target.value as AppearanceSeries)} className="h-8 min-w-0 max-w-full rounded-md border bg-background px-2 text-xs">{APPEARANCE_SERIES.map(series=><option key={series.id} value={series.id}>{chinese?series.name:series.nameEn}</option>)}</select></div>
   <p className="mt-2 text-xs leading-relaxed text-muted-foreground">{t('themeHint')}</p>
   <p className="mt-2 text-xs text-muted-foreground" role="status">{t('current',{name:currentName})}</p>
   <div className="mt-3 grid grid-cols-2 gap-2 sm:grid-cols-3">{APPEARANCE_THEMES.filter(theme=>theme.series===preferences.paletteSeries).map(collection=>{const selected=selectedTheme?.id===collection.id;const label=chinese?collection.name:collection.nameEn;return <button key={collection.id} type="button" aria-pressed={selected} aria-label={`${t('applyPreset')} · ${label}`} onClick={()=>preferences.applyTheme(collection.id)} className={`min-w-0 rounded-lg border p-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${selected?'border-primary bg-accent':'hover:bg-accent/40'}`}>
    <span className="mb-2 grid grid-cols-2 gap-1.5"><span className="min-w-0"><ThemePreview palette={collection.day} chinese={chinese}/><span className="mt-1 flex items-center justify-center gap-1 text-[10px] text-muted-foreground"><Sun className="h-2.5 w-2.5" aria-hidden="true"/>{t('light')}</span></span><span className="min-w-0"><ThemePreview palette={collection.night} chinese={chinese}/><span className="mt-1 flex items-center justify-center gap-1 text-[10px] text-muted-foreground"><Moon className="h-2.5 w-2.5" aria-hidden="true"/>{t('dark')}</span></span></span>
    <span className="block text-sm font-medium">{label}{selected&&<span className="ms-1">✓</span>}</span><span className="mt-1 block line-clamp-2 text-xs leading-relaxed text-muted-foreground">{chinese?collection.day.note:(collection.day.noteEn??'')}</span>
   </button>;})}</div>
  </section>
  <fieldset><legend className="mb-3 text-sm font-medium">{t('modes')}</legend><div className="grid gap-2 sm:grid-cols-3">{modes.map(({id,Icon})=><label key={id} className={`flex cursor-pointer items-center gap-2 rounded-lg border p-3 text-sm focus-within:ring-2 focus-within:ring-ring ${theme===id?'border-primary bg-accent':'hover:bg-accent/40'}`}><input type="radio" name="appearance-mode" checked={theme===id} onChange={()=>setTheme(id)}/><Icon className="h-4 w-4" aria-hidden="true"/>{t(id)}</label>)}</div></fieldset>
  <section aria-labelledby="appearance-fonts"><h3 id="appearance-fonts" className="text-sm font-medium">{t('fonts')}</h3><p className="mt-1 text-xs leading-relaxed text-muted-foreground">{t('fontsHint')}</p><div className="mt-3 grid gap-3 sm:grid-cols-2">
   <label className="space-y-1 text-xs"><span>{t('uiFont')}</span><select value={preferences.uiFont} onChange={event=>choose('uiFont',event.target.value as AppearancePreferences['uiFont'])} className={selectClass}>{UI_FONTS.map(value=><option key={value} value={value}>{t(`font.${value}`)}</option>)}</select></label>
   <label className="space-y-1 text-xs"><span>{t('codeFont')}</span><select value={preferences.codeFont} onChange={event=>choose('codeFont',event.target.value as AppearancePreferences['codeFont'])} className={selectClass}>{CODE_FONTS.map(value=><option key={value} value={value}>{t(`font.${value}`)}</option>)}</select></label>
   <label className="space-y-1 text-xs"><span>{t('brand')}</span><select value={preferences.brandStyle} onChange={event=>choose('brandStyle',event.target.value as AppearancePreferences['brandStyle'])} className={selectClass}>{BRAND_STYLES.map(value=><option key={value} value={value}>{t(`brandStyle.${value}`)}</option>)}</select></label>
   <label className="space-y-1 text-xs"><span>{t('bookTitle')}</span><select value={preferences.bookTitleStyle} onChange={event=>choose('bookTitleStyle',event.target.value as AppearancePreferences['bookTitleStyle'])} className={selectClass}>{BOOK_TITLE_STYLES.map(value=><option key={value} value={value}>{t(`bookStyle.${value}`)}</option>)}</select></label>
  </div><div className="mt-3 flex min-h-20 flex-wrap items-center gap-5 rounded-lg border bg-muted/20 p-4"><PrimsBrand/><span className="prims-book-title text-lg">{t('preview')}</span></div></section>
  <fieldset><legend className="mb-3 text-sm font-medium">{t('icons')}</legend><div className="grid grid-cols-2 gap-2 sm:grid-cols-4">{ICON_STYLES.map(value=><button key={value} type="button" aria-pressed={preferences.iconStyle===value} onClick={()=>choose('iconStyle',value)} className={`rounded-lg border p-3 text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${preferences.iconStyle===value?'border-primary bg-accent':'hover:bg-accent/40'}`}><span className="mb-2 flex justify-center gap-2" aria-hidden="true">{[FileText,Folder,Sparkles].map((Icon,index)=><Icon key={index} className="h-4 w-4" style={{strokeWidth:value==='fine'?1.35:value==='soft'?1.7:value==='sketch'?1.45:2,strokeLinecap:value==='sketch'?'square':'round'}}/>)}</span>{t(`iconStyle.${value}`)}</button>)}</div></fieldset>
  <p className="border-t pt-3 text-xs leading-relaxed text-muted-foreground">{t('storage')}</p>
 </div>;
}

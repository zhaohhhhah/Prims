import { type Theme, themes } from '../tokens.js';
export type SystemTheme='light'|'dark';
export type ResolvedTheme=Exclude<Theme,'system'>;
export const THEME_STORAGE_KEY='scribe-theme';
export const SYSTEM_THEME_QUERY='(prefers-color-scheme: dark)';
export function readStoredTheme(fallback:Theme,storage:Pick<Storage,'getItem'>|null):Theme {
 try{const stored=storage?.getItem(THEME_STORAGE_KEY);return stored!=null&&(themes as readonly string[]).includes(stored)?stored as Theme:fallback;}catch{return fallback;}
}
export function resolveTheme(theme:Theme,system:SystemTheme):ResolvedTheme{return theme==='system'?system:theme;}
export function applyResolvedTheme(root:Pick<HTMLElement,'setAttribute'|'classList'|'style'>,theme:ResolvedTheme):void {
 const dark=theme==='dark'||theme==='high-contrast';root.setAttribute('data-theme',theme);root.classList.toggle('dark',dark);root.style.colorScheme=dark?'dark':'light';
}
/** React consumes every change so editor hooks and CSS resolve to the same palette. */
export function listenToSystemTheme(media:MediaQueryList,onChange:(theme:SystemTheme)=>void):()=>void {
 const changed=()=>onChange(media.matches?'dark':'light');changed();
 if(typeof media.addEventListener==='function'){media.addEventListener('change',changed);return()=>media.removeEventListener('change',changed);}
 media.addListener(changed);return()=>media.removeListener(changed);
}

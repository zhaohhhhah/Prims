import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { type Theme } from '../tokens.js';
import { applyResolvedTheme, listenToSystemTheme, readStoredTheme, resolveTheme, SYSTEM_THEME_QUERY, THEME_STORAGE_KEY, type SystemTheme } from './theme-state.js';
import type { ReactNode } from 'react';
interface ThemeContextValue {readonly theme:Theme;readonly resolvedTheme:Exclude<Theme,'system'>;readonly setTheme:(theme:Theme)=>void}
const ThemeContext=createContext<ThemeContextValue|null>(null);
function getSystemTheme():SystemTheme{return typeof window!=='undefined'&&typeof window.matchMedia==='function'&&window.matchMedia(SYSTEM_THEME_QUERY).matches?'dark':'light';}
function initialTheme(fallback:Theme):Theme {try{return readStoredTheme(fallback,typeof window==='undefined'?null:window.localStorage);}catch{return fallback;}}
interface ThemeProviderProps {readonly children:ReactNode;readonly defaultTheme?:Theme}
export function ThemeProvider({children,defaultTheme='system'}:ThemeProviderProps){
 const [theme,setThemeState]=useState<Theme>(()=>initialTheme(defaultTheme));
 const [systemTheme,setSystemTheme]=useState<SystemTheme>(getSystemTheme);
 const resolvedTheme=resolveTheme(theme,systemTheme);
 useEffect(()=>{applyResolvedTheme(document.documentElement,resolvedTheme);},[resolvedTheme]);
 useEffect(()=>{if(theme!=='system'||typeof window.matchMedia!=='function')return;return listenToSystemTheme(window.matchMedia(SYSTEM_THEME_QUERY),setSystemTheme);},[theme]);
 const setTheme=useCallback((next:Theme)=>{try{window.localStorage.setItem(THEME_STORAGE_KEY,next);}catch{/* Private browsing may deny storage; the choice still applies to this session. */}setThemeState(next);},[]);
 const value=useMemo<ThemeContextValue>(()=>({theme,resolvedTheme,setTheme}),[theme,resolvedTheme,setTheme]);
 return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}
export function useTheme():ThemeContextValue{const ctx=useContext(ThemeContext);if(ctx===null)throw new Error('useTheme must be used within a <ThemeProvider>');return ctx;}

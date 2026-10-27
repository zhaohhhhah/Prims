import {create} from 'zustand';
import {persist} from 'zustand/middleware';
import {applyAppearancePreset,DEFAULT_APPEARANCE,normalizeAppearance,updateAppearance,type AppearancePreferences} from '../lib/appearance-state';
import {applyAppearanceTheme} from '../lib/appearance-themes';
interface AppearanceState extends AppearancePreferences {setPreference:<K extends keyof AppearancePreferences>(key:K,value:AppearancePreferences[K])=>void;applyPreset:(id:string)=>void;applyTheme:(id:string)=>void}
/** Local visual preferences follow the existing browser-local theme/settings convention. */
export const useAppearanceStore=create<AppearanceState>()(persist((set)=>({...DEFAULT_APPEARANCE,setPreference:(key,value)=>set(current=>updateAppearance(current,key,value)),applyPreset:id=>set(current=>applyAppearancePreset(current,id)),applyTheme:id=>set(current=>applyAppearanceTheme(current,id))}),{name:'prims-appearance',version:1,partialize:state=>normalizeAppearance(state),merge:(stored,current)=>({...current,...normalizeAppearance(stored)})}));

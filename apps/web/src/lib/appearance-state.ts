import {appearancePalette,APPEARANCE_SERIES,BOOK_TITLE_STYLES,BRAND_STYLES,CODE_FONTS,ICON_STYLES,UI_FONTS,type AppearanceSeries} from './appearance-catalog';
export interface AppearancePreferences {lightPalette:string;darkPalette:string;paletteSeries:AppearanceSeries;uiFont:(typeof UI_FONTS)[number];codeFont:(typeof CODE_FONTS)[number];brandStyle:(typeof BRAND_STYLES)[number];bookTitleStyle:(typeof BOOK_TITLE_STYLES)[number];iconStyle:(typeof ICON_STYLES)[number];presetId:string|null}
export const DEFAULT_APPEARANCE:AppearancePreferences={lightPalette:'classic-light',darkPalette:'classic-dark',paletteSeries:'original',uiFont:'inherit',codeFont:'inherit',brandStyle:'calligraphy',bookTitleStyle:'pen',iconStyle:'system',presetId:null};
export type AppearancePresetId='classic'|'mist'|'impression'|'celadon'|'indigo'|'nocturne';
export interface AppearancePreset {id:AppearancePresetId;mode:'system'|'light'|'dark';preferences:Omit<AppearancePreferences,'presetId'>}
const classic={...DEFAULT_APPEARANCE};
export const APPEARANCE_PRESETS:readonly AppearancePreset[]=[
 {id:'classic',mode:'system',preferences:{...classic}},
 {id:'mist',mode:'system',preferences:{...classic,lightPalette:'mist',darkPalette:'nocturne',brandStyle:'calligraphy',iconStyle:'soft'}},
 {id:'impression',mode:'system',preferences:{...classic,lightPalette:'waterlilies',darkPalette:'pond',paletteSeries:'impression',brandStyle:'calligraphy',bookTitleStyle:'handwritten',iconStyle:'soft'}},
 {id:'celadon',mode:'system',preferences:{...classic,lightPalette:'celadon',darkPalette:'moon-jade',paletteSeries:'china',bookTitleStyle:'literary',iconStyle:'fine'}},
 {id:'indigo',mode:'system',preferences:{...classic,lightPalette:'indigo-paper',darkPalette:'ink-indigo',paletteSeries:'japan',brandStyle:'simple',iconStyle:'fine'}},
 {id:'nocturne',mode:'dark',preferences:{...classic,lightPalette:'pearl',darkPalette:'nocturne',brandStyle:'calligraphy',bookTitleStyle:'literary',iconStyle:'soft'}},
];
function choice<T extends string>(value:unknown,allowed:readonly T[],fallback:T):T{return typeof value==='string'&&(allowed as readonly string[]).includes(value)?value as T:fallback;}
export function normalizeAppearance(value:unknown):AppearancePreferences{const raw=value&&typeof value==='object'?value as Partial<Record<keyof AppearancePreferences,unknown>>:{};return {
 lightPalette:typeof raw.lightPalette==='string'&&appearancePalette(raw.lightPalette)?.period==='light'?raw.lightPalette:DEFAULT_APPEARANCE.lightPalette,
 darkPalette:typeof raw.darkPalette==='string'&&appearancePalette(raw.darkPalette)?.period==='dark'?raw.darkPalette:DEFAULT_APPEARANCE.darkPalette,
 paletteSeries:choice(raw.paletteSeries,APPEARANCE_SERIES.map(series=>series.id),'original'),
 uiFont:choice(raw.uiFont,UI_FONTS,'inherit'),codeFont:choice(raw.codeFont,CODE_FONTS,'inherit'),brandStyle:choice(raw.brandStyle,BRAND_STYLES,DEFAULT_APPEARANCE.brandStyle),bookTitleStyle:choice(raw.bookTitleStyle,BOOK_TITLE_STYLES,DEFAULT_APPEARANCE.bookTitleStyle),iconStyle:choice(raw.iconStyle,ICON_STYLES,'system'),
 presetId:typeof raw.presetId==='string'&&APPEARANCE_PRESETS.some(preset=>preset.id===raw.presetId)?raw.presetId:null,
};}
export function updateAppearance<K extends keyof AppearancePreferences>(current:AppearancePreferences,key:K,value:AppearancePreferences[K]):AppearancePreferences{return normalizeAppearance({...current,[key]:value,presetId:key==='paletteSeries'?current.presetId:key==='presetId'?value:null});}
export function applyAppearancePreset(current:AppearancePreferences,id:string):AppearancePreferences{const preset=APPEARANCE_PRESETS.find(item=>item.id===id);return preset?normalizeAppearance({...current,...preset.preferences,uiFont:current.uiFont,codeFont:current.codeFont,presetId:id}):current;}
export function resolvedPaletteId(preferences:AppearancePreferences,resolvedTheme:'light'|'dark'|'high-contrast'):string|null{return resolvedTheme==='high-contrast'?null:resolvedTheme==='dark'?preferences.darkPalette:preferences.lightPalette;}

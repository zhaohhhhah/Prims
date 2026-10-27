import {useTheme} from '@scribe/ui';
import {useLayoutEffect} from 'react';
import {resolvedPaletteId} from '../../lib/appearance-state';
import {useAppearanceStore} from '../../stores/appearance';
/** Color mode remains owned by ThemeProvider; these attributes only select appearance details. */
export function AppearanceBridge(){const {resolvedTheme}=useTheme();const preferences=useAppearanceStore();useLayoutEffect(()=>{const root=document.documentElement,palette=resolvedPaletteId(preferences,resolvedTheme);if(palette)root.dataset.palette=palette;else delete root.dataset.palette;root.dataset.uiFont=preferences.uiFont;root.dataset.codeFont=preferences.codeFont;root.dataset.brandStyle=preferences.brandStyle;root.dataset.bookTitleStyle=preferences.bookTitleStyle;root.dataset.iconStyle=preferences.iconStyle;},[resolvedTheme,preferences.lightPalette,preferences.darkPalette,preferences.uiFont,preferences.codeFont,preferences.brandStyle,preferences.bookTitleStyle,preferences.iconStyle]);return null;}

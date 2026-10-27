import 'react-i18next';
import type en from '../locales/en.json';
import type {appearanceResources} from '../locales/appearance-resources';
declare module 'react-i18next' {interface CustomTypeOptions {defaultNS:'translation';resources:{translation:typeof en;appearance:typeof appearanceResources.en};}}

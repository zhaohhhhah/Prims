import { Folder, FolderOpen, Search } from 'lucide-react';
import { useId, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Input } from '@scribe/ui';
import { projectFolderPaths } from '../../lib/project-folders';
interface Props { files: readonly { path: string }[]; directories: readonly string[]; value: string; disabled?: boolean; onChange: (path: string) => void }
export function ProjectFolderPicker({ files, directories, value, disabled = false, onChange }: Props) {
 const { i18n } = useTranslation(), id = useId();
 const zh = i18n.language.startsWith('zh'), [query, setQuery] = useState('');
 const folders = useMemo(() => projectFolderPaths(files, directories), [files, directories]);
 const needle=query.trim().toLocaleLowerCase();
 const visible=folders.filter(path=>!path||!needle||path.toLocaleLowerCase().includes(needle));
 return <fieldset disabled={disabled} className="space-y-2">
  <legend className="mb-2 text-sm font-medium">{zh?'目标文件夹':'Destination folder'}</legend>
  <div className="relative"><Search aria-hidden="true" className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"/><Input aria-label={zh?'查找文件夹':'Find a folder'} value={query} onChange={event=>setQuery(event.target.value)} placeholder={zh?'查找文件夹…':'Find a folder…'} className="pl-9"/></div>
  <div className="max-h-56 overflow-y-auto rounded-lg border p-1" aria-label={zh?'项目文件夹':'Project folders'}>
   {visible.map(path=>{const selected=path===value;const Icon=selected?FolderOpen:Folder;return <label key={path} className={`flex min-h-10 cursor-pointer items-center gap-2 rounded-md py-2 pr-3 text-sm transition-colors hover:bg-accent ${selected?'bg-accent text-accent-foreground':'text-muted-foreground'} ${disabled?'pointer-events-none opacity-60':''}`} style={{paddingLeft:`${.75+((path?path.split('/').length:0)*.75)}rem`}}>
    <input type="radio" name={id} value={path} checked={selected} onChange={()=>onChange(path)} className="h-3.5 w-3.5 shrink-0 accent-current"/>
    <Icon aria-hidden="true" className="h-4 w-4 shrink-0"/>
    <span className="min-w-0 break-all" title={path||'/'}>{path?(needle?path:path.split('/').at(-1)):(zh?'项目根目录':'Project root')}</span>
   </label>;})}
   {visible.length===1&&needle&&<p className="px-3 py-2 text-sm text-muted-foreground">{zh?'没有匹配的子文件夹':'No matching subfolders'}</p>}
  </div>
  <p className="truncate text-xs text-muted-foreground" title={value||'/'}>{zh?'已选择：':'Selected: '}{value?`/ ${value}`:'/'}</p>
 </fieldset>;
}

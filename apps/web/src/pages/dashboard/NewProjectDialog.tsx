import { zodResolver } from '@hookform/resolvers/zod';
import { type CreateProjectInput, createProjectInputSchema, type Project } from '@scribe/shared';
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Input,
  Label,
} from '@scribe/ui';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Loader2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Controller, useForm } from 'react-hook-form';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { toast } from 'sonner';

import {
  TemplateGallery,
  type TemplateSelection,
} from '../../components/TemplateGallery/TemplateGallery';
import { api, type ZipManifest } from '../../lib/api';
import {packageProjectImport} from '../../lib/project-file-import';

interface NewProjectDialogProps {
  readonly open: boolean;
  readonly initialMode?: 'template' | 'import';
  readonly onOpenChange: (open: boolean) => void;
}

export function NewProjectDialog({ open, onOpenChange, initialMode = 'template' }: NewProjectDialogProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  // The selected template — either one of the six built-ins (single
  // enum value) or a community template (full file list). The form's
  // `template` field always carries a valid built-in enum so the
  // create-project API call stays well-typed; for community
  // selections we send `blank` then seed files after creation.
  const [selection, setSelection] = useState<TemplateSelection>({ kind: 'builtin', id: 'blank' });
  const [mode,setMode]=useState<'template'|'import'>(initialMode);
  useEffect(()=>{if(open)setMode(initialMode);},[open,initialMode]);
  const [zipFile,setZipFile]=useState<File|null>(null);
  const [manifest,setManifest]=useState<ZipManifest|null>(null);
  const [mainFile,setMainFile]=useState('');
  const [inspecting,setInspecting]=useState(false);
  const [zipError,setZipError]=useState('');
  const inspectionVersion=useRef(0);
  const lastSelection=useRef<{files:readonly File[];directory:boolean}|undefined>(undefined);
  const filePicker=useRef<HTMLInputElement>(null);
  const folderPicker=useRef<HTMLInputElement>(null);
  const [folderSkipped,setFolderSkipped]=useState(0);
  function clearImport(){lastSelection.current=undefined;inspectionVersion.current+=1;setZipFile(null);setManifest(null);setMainFile('');setZipError('');setInspecting(false);setFolderSkipped(0);}
  function changeMode(next:'template'|'import'){clearImport();setMode(next);}
  async function chooseImport(selected:readonly File[],directory=false){
    if(!selected.length)return;
    lastSelection.current={files:selected,directory};
    const version=++inspectionVersion.current;
    setZipFile(null);setManifest(null);setMainFile('');setZipError('');setFolderSkipped(0);setInspecting(true);
    try{
      const imported=await packageProjectImport(selected,directory);
      if(version!==inspectionVersion.current)return;
      const result=await api.projects.inspectZip(imported.archive);
      if(version!==inspectionVersion.current)return;
      setZipFile(imported.archive);setManifest(result);setMainFile(result.suggestedMainFile??'');setFolderSkipped(imported.skippedMetadata);
      if(!form.getValues('name').trim())form.setValue('name',imported.name);
    }catch(error){if(version===inspectionVersion.current)setZipError(error instanceof Error?error.message:'导入检查失败');}
    finally{if(version===inspectionVersion.current)setInspecting(false);}
  }


  const form = useForm<CreateProjectInput>({
    resolver: zodResolver(createProjectInputSchema),
    defaultValues: { name: '', description: '', template: 'blank', compiler: 'xelatex' },
  });

  const mutation = useMutation<Project, Error, CreateProjectInput>({
    mutationFn: async (input) => {
      if(mode!=='template'){
        if(!zipFile||!manifest||!mainFile)throw new Error('请先选择导入文件和主文件');
        return api.projects.importZip(input,zipFile,mainFile);
      }

      // Community templates: create the project as `blank`, then seed
      // each file via api.files.create. This avoids any server-side
      // changes — the existing template loader stays untouched and
      // community content lives entirely on the client.
      if (selection.kind === 'community') {
        const created = await api.projects.create({ ...input, template: 'blank' });
        // Best-effort file seeding. If one file fails (e.g. duplicate
        // path), we keep going so the user still gets a usable project.
        for (const f of selection.template.files) {
          try {
            await api.files.create(created.id, f.path, f.content);
          } catch (err) {
            console.warn('community template seed failed for', f.path, err);
          }
        }
        // Update mainFile to match the template's preferred entry
        // point if it isn't already main.tex (the server defaults).
        if (selection.template.mainFile !== 'main.tex') {
          try {
            await api.projects.update(created.id, { mainFile: selection.template.mainFile });
          } catch {
            /* leave the default — user can change in settings */
          }
        }
        return created;
      }
      return api.projects.create(input);
    },
    onSuccess: async (project) => {
      await queryClient.invalidateQueries({ queryKey: ['projects'] });
      onOpenChange(false);
      form.reset();
      setSelection({ kind: 'builtin', id: 'blank' });clearImport();setMode('template');
      void navigate(`/project/${project.id}`);
    },
    onError: (error) => {
      toast.error(error.message);
    },
  });

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {form.reset();clearImport();}
        onOpenChange(next);
      }}
    >
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('project.newTitle')}</DialogTitle>
          <DialogDescription>{t('project.newDescription')}</DialogDescription>
        </DialogHeader>
        <form
          onSubmit={form.handleSubmit((values) => {
            mutation.mutate(values);
          })}
          className="space-y-4"
          noValidate
        >
          <div className="space-y-2">
            <Label htmlFor="name">{t('project.nameLabel')}</Label>
            <Input
              id="name"
              placeholder={t('project.namePlaceholder')}
              {...form.register('name')}
            />
            {form.formState.errors.name !== undefined && (
              <p className="text-xs text-destructive">{form.formState.errors.name.message}</p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="description">{t('project.descriptionLabel')}</Label>
            <Input
              id="description"
              placeholder={t('project.descriptionPlaceholder')}
              {...form.register('description')}
            />
          </div>
          <div className="flex gap-2">
            <Button type="button" variant={mode==='template'?'default':'outline'} disabled={mutation.isPending||inspecting} onClick={()=>changeMode('template')}>使用模板</Button>
            <Button type="button" variant={mode==='import'?'default':'outline'} disabled={mutation.isPending||inspecting} onClick={()=>changeMode('import')}>导入项目</Button>
          </div>
          <div className="space-y-2">
            <Label htmlFor="new-project-compiler">编译器</Label>
            <Controller control={form.control} name="compiler" render={({field})=>(<select id="new-project-compiler" className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm" value={field.value} onChange={field.onChange} disabled={mutation.isPending||inspecting}><option value="xelatex">XeLaTeX</option><option value="pdflatex">pdfLaTeX</option><option value="lualatex">LuaLaTeX</option></select>)}/>
          </div>
          {mode==='import'?<div className="space-y-2">
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" disabled={mutation.isPending||inspecting} onClick={()=>filePicker.current?.click()}>选择文件或 ZIP</Button>
              <Button type="button" variant="outline" disabled={mutation.isPending||inspecting} onClick={()=>folderPicker.current?.click()}>选择文件夹</Button>
            </div>
            <input ref={filePicker} className="hidden" type="file" multiple disabled={mutation.isPending||inspecting} onChange={event=>{void chooseImport(Array.from(event.target.files??[]));event.target.value='';}}/>
            <input ref={folderPicker} className="hidden" type="file" multiple {...{webkitdirectory:'',directory:''}} disabled={mutation.isPending||inspecting} onChange={event=>{void chooseImport(Array.from(event.target.files??[]),true);event.target.value='';}}/>
            {inspecting&&<p role="status" className="text-sm">正在检查导入内容…</p>}
            {zipError&&<div><p role="alert" className="text-sm text-destructive">{zipError}</p>{lastSelection.current&&<Button type="button" variant="outline" disabled={inspecting||mutation.isPending} onClick={()=>{const selected=lastSelection.current;if(selected)void chooseImport(selected.files,selected.directory);}}>重试检查</Button>}</div>}
            {manifest&&<>
              <p className="text-sm">已检查 {manifest.files.length} 个文件、{manifest.directories.length} 个目录{(manifest.skippedMetadata+folderSkipped)?`，忽略${manifest.skippedMetadata+folderSkipped}个系统元数据或 .git 条目`:''}。</p>
              <Label htmlFor="zip-main-file">LaTeX主文件</Label>
              <select id="zip-main-file" className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm" value={mainFile} onChange={event=>setMainFile(event.target.value)} disabled={mutation.isPending}>
                <option value="">请选择主文件</option>{manifest.mainCandidates.map(path=><option key={path} value={path}>{path}</option>)}
              </select>
              {!manifest.mainCandidates.length&&<p className="text-sm text-destructive">没有.tex主文件；可将这些资源上传到已有项目。</p>}
            </>}
          </div>:<div className="space-y-2">
            <Label>{t('project.template')}</Label>
            <Controller
              control={form.control}
              name="template"
              render={({ field }) => (
                <TemplateGallery
                  selected={selection}
                  onSelect={(next) => {
                    setSelection(next);
                    // Keep the form's template field bound to a valid
                    // built-in enum value — the server expects one of
                    // the six. Community selections route through the
                    // `blank` + seed-files path in the mutation.
                    field.onChange(next.kind === 'builtin' ? next.id : 'blank');
                  }}
                />
              )}
            />
          </div>}
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => {
                clearImport();form.reset();onOpenChange(false);
              }}
            >
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={mutation.isPending||inspecting||(mode!=='template'&&(!manifest||!mainFile))}>
              {mutation.isPending && (
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
              )}
              {t('common.create')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

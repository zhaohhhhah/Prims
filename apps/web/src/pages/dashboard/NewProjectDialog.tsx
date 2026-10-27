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
import { useEffect, useState } from 'react';
import { Controller, useForm } from 'react-hook-form';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { toast } from 'sonner';

import {
  TemplateGallery,
  type TemplateSelection,
} from '../../components/TemplateGallery/TemplateGallery';
import { api, type ZipManifest } from '../../lib/api';
import {ZIP_MAX_BYTES} from '../../lib/archive-upload';

interface NewProjectDialogProps {
  readonly open: boolean;
  readonly initialMode?: 'template' | 'zip';
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
  const [mode,setMode]=useState<'template'|'zip'>(initialMode);
  useEffect(()=>{if(open)setMode(initialMode);},[open,initialMode]);
  const [zipFile,setZipFile]=useState<File|null>(null);
  const [manifest,setManifest]=useState<ZipManifest|null>(null);
  const [mainFile,setMainFile]=useState('');
  const [inspecting,setInspecting]=useState(false);
  const [zipError,setZipError]=useState('');
  async function chooseZip(file:File|undefined){
    setZipFile(file??null);setManifest(null);setMainFile('');setZipError('');if(!file)return;
    if(!file.name.toLowerCase().endsWith('.zip')||file.size>ZIP_MAX_BYTES){setZipError('请选择不超过20MiB的ZIP压缩包');return;}
    setInspecting(true);
    try{const result=await api.projects.inspectZip(file);setManifest(result);setMainFile(result.suggestedMainFile??'');if(!form.getValues('name').trim())form.setValue('name',file.name.replace(/\.zip$/i,''));}
    catch(error){setZipError(error instanceof Error?error.message:'ZIP检查失败');}
    finally{setInspecting(false);}
  }


  const form = useForm<CreateProjectInput>({
    resolver: zodResolver(createProjectInputSchema),
    defaultValues: { name: '', description: '', template: 'blank', compiler: 'tectonic' },
  });

  const mutation = useMutation<Project, Error, CreateProjectInput>({
    mutationFn: async (input) => {
      if(mode==='zip'){
        if(!zipFile||!manifest||!mainFile)throw new Error('请先选择ZIP和主文件');
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
      setSelection({ kind: 'builtin', id: 'blank' });setZipFile(null);setManifest(null);setMainFile('');setZipError('');setMode('template');
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
        if (!next) form.reset();
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
            <Button type="button" variant={mode==='template'?'default':'outline'} disabled={mutation.isPending||inspecting} onClick={()=>setMode('template')}>使用模板</Button>
            <Button type="button" variant={mode==='zip'?'default':'outline'} disabled={mutation.isPending||inspecting} onClick={()=>setMode('zip')}>从 ZIP 创建</Button>
          </div>
          {mode==='zip'?<div className="space-y-2">
            <Label htmlFor="new-project-zip">ZIP压缩包</Label>
            <Input id="new-project-zip" type="file" accept=".zip,application/zip" disabled={mutation.isPending||inspecting} onChange={event=>{void chooseZip(event.target.files?.[0]);}}/>
            <p className="text-xs text-muted-foreground">保留目录和文件原始内容；压缩包≤20MiB，最多256条目，单文件≤8MiB，展开后合计≤32MiB。</p>
            {inspecting&&<p className="text-sm">正在检查压缩包…</p>}
            {zipError&&<p role="alert" className="text-sm text-destructive">{zipError}</p>}
            {manifest&&<>
              <p className="text-sm">已检查 {manifest.files.length} 个文件、{manifest.directories.length} 个目录{manifest.skippedMetadata?`，忽略${manifest.skippedMetadata}个系统元数据条目`:''}。</p>
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
                onOpenChange(false);
              }}
            >
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={mutation.isPending||inspecting||(mode==='zip'&&(!manifest||!mainFile))}>
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

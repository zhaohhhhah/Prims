import { type Project, type CompilerEngine } from '@scribe/shared';
import { Button, Separator, SheetDescription, SheetHeader, SheetTitle, Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@scribe/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Loader2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { toast } from 'sonner';

import { api, type ApiError } from '../../lib/api';

import { useAuthStore } from '../../stores/auth';
import { MembersPanel } from './MembersPanel';

interface ProjectSettingsSheetProps {
  readonly project: Project;
  readonly onClosed: () => void;
}

export function ProjectSettingsSheet({ project, onClosed }: ProjectSettingsSheetProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const user=useAuthStore(state=>state.user);
  const canWrite=project.ownerId===user?.id;
  const [compiler,setCompiler]=useState<CompilerEngine>(project.compiler);
  const [mainFile,setMainFile]=useState(project.mainFile);
  useEffect(()=>{setCompiler(project.compiler);setMainFile(project.mainFile);},[project.id,project.compiler,project.mainFile]);
  const files=useQuery({queryKey:['files',project.id],queryFn:()=>api.files.list(project.id)});
  const engines=useQuery({queryKey:['compile-engines'],queryFn:()=>api.compiles.engines(),staleTime:30000});
  const save=useMutation<Project,ApiError>({mutationFn:()=>api.projects.update(project.id,{compiler,mainFile}),onSuccess:value=>{queryClient.setQueryData(['project',project.id],value);void queryClient.invalidateQueries({queryKey:['projects']});toast.success(t('project.compileSettingsSaved'));},onError:error=>toast.error(error.body.message)});
  const available=engines.data?.available??['tectonic'];
  const mainFiles=(files.data??[]).filter(file=>/\.(tex|ltx|latex)$/i.test(file.path));

  const deleteMutation = useMutation<unknown, ApiError>({
    mutationFn: () => api.projects.remove(project.id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['projects'] });
      onClosed();
      void navigate('/dashboard', { replace: true });
    },
    onError: (error) => {
      toast.error(error.body.message);
    },
  });

  const handleDelete = () => {
    if (window.confirm(t('project.deleteProjectConfirm'))) {
      deleteMutation.mutate();
    }
  };

  return (
    <div className="flex h-full flex-col">
      <SheetHeader className="pb-4">
        <SheetTitle>{t('project.projectSettings')}</SheetTitle>
        <SheetDescription>{project.name}</SheetDescription>
      </SheetHeader>

      <Separator />

      {/* `px-1` gives the input's focus ring (`ring-offset-2` + `ring-2`,
          ~4px outside the element) room to render — otherwise the
          scroll container clips it and the input visually hugs the
          left wall of the settings sheet. `-mx-1` cancels the
          padding for the section dividers so they still touch edge
          to edge. */}
      <div className="flex-1 space-y-6 overflow-auto px-1 py-4">
        <section className="space-y-3">
          <h3 className="text-sm font-semibold">{t('project.compilePipeline')}</h3>
          <div className="space-y-1">
            <span className="text-sm" id="project-compiler-label">{t('project.compiler')}</span>
            <Select value={compiler} disabled={!canWrite || save.isPending} onValueChange={value => setCompiler(value as CompilerEngine)}>
              <SelectTrigger aria-labelledby="project-compiler-label"><SelectValue /></SelectTrigger>
              <SelectContent>
                {([['tectonic', 'Tectonic'], ['xelatex', 'XeLaTeX'], ['lualatex', 'LuaLaTeX'], ['pdflatex', 'pdfLaTeX']] as const).map(([value, label]) => (
                  <SelectItem key={value} value={value} disabled={!available.includes(value) && project.compiler !== value}>
                    {label}{value === 'tectonic' ? ` (${t('project.defaultCompiler')})` : !available.includes(value) ? ` (${t('project.compilerUnavailable')})` : ''}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1">
            <span className="text-sm" id="project-main-label">{t('project.mainDocument')}</span>
            <Select value={mainFile} disabled={!canWrite || save.isPending} onValueChange={setMainFile}>
              <SelectTrigger aria-labelledby="project-main-label"><SelectValue /></SelectTrigger>
              <SelectContent>
                {!mainFiles.some(file => file.path === mainFile) && <SelectItem value={mainFile}>{mainFile}</SelectItem>}
                {mainFiles.map(file => <SelectItem key={file.id} value={file.path}>{file.path}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
          <Button size="sm" onClick={()=>save.mutate()} disabled={!canWrite||save.isPending||!mainFile||!available.includes(compiler)||(compiler===project.compiler&&mainFile===project.mainFile)}>{save.isPending?t('project.saving'):t('project.saveCompileSettings')}</Button>
        </section>
        <Separator />
        <section>
          <h3 className="mb-3 text-sm font-semibold">{t('project.members')}</h3>
          <MembersPanel projectId={project.id} />
        </section>

        <Separator />

        <section>
          <h3 className="mb-3 text-sm font-semibold text-destructive">{t('project.danger')}</h3>
          <Button
            variant="destructive"
            size="sm"
            onClick={handleDelete}
            disabled={deleteMutation.isPending}
          >
            {deleteMutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            ) : null}
            {deleteMutation.isPending
              ? t('project.deleteProjectInProgress')
              : t('project.deleteProject')}
          </Button>
        </section>
      </div>
    </div>
  );
}

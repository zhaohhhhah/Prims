import type { Project } from '@scribe/shared';
import {
  Button, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader,
  DialogTitle, DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuTrigger, Input, Label,
} from '@scribe/ui';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Loader2, MoreHorizontal, Pencil, Trash2 } from 'lucide-react';
import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api } from '../../lib/api';

interface Props {
  project: Project;
  userId: string;
}

export function ProjectActionsMenu({ project, userId }: Props) {
  const { i18n } = useTranslation();
  const zh = i18n.language.startsWith('zh');
  const copy = zh ? {
    more: '更多操作', rename: '重命名', remove: '删除项目', name: '项目名称',
    cancel: '取消', save: '保存', confirm: '确认删除',
    renameDescription: '修改书架上显示的项目名称。',
    deleteDescription: '项目中的文件和版本历史将永久删除，无法撤销。',
    failed: '操作未完成，请重试',
  } : {
    more: 'Project actions', rename: 'Rename', remove: 'Delete project', name: 'Project name',
    cancel: 'Cancel', save: 'Save', confirm: 'Delete permanently',
    renameDescription: 'Change the project name shown on your bookshelf.',
    deleteDescription: 'The project files and version history will be permanently deleted. This cannot be undone.',
    failed: 'Unable to complete this action. Please try again.',
  };
  const queryClient = useQueryClient();
  const inputId = useId();
  const [action, setAction] = useState<'rename' | 'delete' | null>(null);
  const [name, setName] = useState(project.name);
  const mutation = useMutation({
    mutationFn: async (operation: 'rename' | 'delete') => {
      if (operation === 'delete') {
        await api.projects.remove(project.id);
        return null;
      }
      return api.projects.update(project.id, { name: name.trim() });
    },
    onSuccess: (updated) => {
      queryClient.setQueryData<Project[]>(['projects', userId], previous =>
        previous?.flatMap(item => item.id === project.id ? updated ? [updated] : [] : [item]));
      if (updated) queryClient.setQueryData(['project', project.id], updated);
      else queryClient.removeQueries({ queryKey: ['project', project.id] });
      void queryClient.invalidateQueries({ queryKey: ['projects', userId] });
      void queryClient.invalidateQueries({ queryKey: ['dashboard-summary', userId, project.id] });
      setAction(null);
    },
  });
  function choose(next: 'rename' | 'delete') {
    mutation.reset();
    setName(project.name);
    setAction(next);
  }
  return <>
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" className="prims-project-actions" aria-label={`${project.name} · ${copy.more}`} title={copy.more}>
          <MoreHorizontal size={17} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" onCloseAutoFocus={event => { if (action) event.preventDefault(); }}>
        <DropdownMenuItem onSelect={() => choose('rename')}><Pencil size={15} />{copy.rename}</DropdownMenuItem>
        <DropdownMenuItem destructive onSelect={() => choose('delete')}><Trash2 size={15} />{copy.remove}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
    <Dialog open={action !== null} onOpenChange={open => { if (!open && !mutation.isPending) setAction(null); }}>
      <DialogContent onEscapeKeyDown={event => { if (mutation.isPending) event.preventDefault(); }} onPointerDownOutside={event => { if (mutation.isPending) event.preventDefault(); }}>
        <DialogHeader>
          <DialogTitle>{action === 'delete' ? copy.remove : copy.rename}</DialogTitle>
          <DialogDescription>{action === 'delete' ? copy.deleteDescription : copy.renameDescription}</DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={event => { event.preventDefault(); if (action && !mutation.isPending && (action === 'delete' || name.trim())) mutation.mutate(action); }}>
          {action === 'delete' ? <p className="break-words rounded-md border bg-muted/40 p-3 font-medium">{project.name}</p> : <div className="space-y-2">
            <Label htmlFor={inputId}>{copy.name}</Label>
            <Input id={inputId} value={name} onChange={event => setName(event.target.value)} maxLength={200} required disabled={mutation.isPending} autoFocus />
          </div>}
          {mutation.isError && <p role="alert" className="text-sm text-destructive">{mutation.error instanceof Error ? mutation.error.message : copy.failed}</p>}
          <DialogFooter>
            <Button type="button" variant="outline" disabled={mutation.isPending} onClick={() => setAction(null)}>{copy.cancel}</Button>
            <Button type="submit" variant={action === 'delete' ? 'destructive' : 'default'} disabled={mutation.isPending || (action === 'rename' && (!name.trim() || name.trim() === project.name))}>
              {mutation.isPending && <Loader2 size={15} className="animate-spin" />}{action === 'delete' ? copy.confirm : copy.save}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  </>;
}

import { type ProjectFile, type ProjectId } from '@scribe/shared';
import {
  Button,
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@scribe/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ChevronDown,
  ChevronRight,
  Download,
  FilePlus,
  FileText,
  FolderClosed,
  FolderOpen,
  FolderPlus,
  Loader2,
  ListChecks,
  Pencil,
  Trash2,
  Upload,
} from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';

import { api, type ApiError } from '../../lib/api';
import {uploadBatch} from '../../lib/archive-upload';
import { batchError } from '../../lib/file-batch';
import { claimFileTreeDrag, mutateFolder } from '../../lib/file-tree-operations';
import { withFileOperationGuard } from '../../lib/file-operation-guard';
import { packageFolderUpload } from '../../lib/folder-upload';
import { useAuthStore } from '../../stores/auth';
import { projectActionsApi } from '../../lib/project-actions';
import { buildFileTree, type FolderNode, type TreeNode } from '../../lib/file-tree-shape';
import { FileBatchActions } from './FileBatchActions';

interface FileTreeProps {
  readonly projectId: ProjectId;
  readonly files: readonly ProjectFile[];
  readonly selectedFileId?: ProjectFile['id'] | null;
  readonly onSelect?: (file: ProjectFile) => void;
}

const MIME_FILE_DRAG = 'application/x-scribe-file';

function filesUnder(files: readonly ProjectFile[], folderPath: string): ProjectFile[] {
  const prefix = `${folderPath}/`;
  return files.filter((f) => f.path === `${folderPath}/.gitkeep` || f.path.startsWith(prefix));
}

export function FileTree({ projectId, files, selectedFileId, onSelect }: FileTreeProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const uploadInputRef = useRef<HTMLInputElement>(null);
  const folderInputRef = useRef<HTMLInputElement>(null);
  const [packingFolder, setPackingFolder] = useState(false);
  useEffect(() => { folderInputRef.current?.setAttribute('webkitdirectory', ''); }, []);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set(['']));
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const [selecting, setSelecting] = useState(false);
  const [checkedIds, setCheckedIds] = useState<ReadonlySet<string>>(() => new Set());
  const [batchBusy, setBatchBusy] = useState(false);
  const selectedFiles = files.filter(file => checkedIds.has(file.id));
  useEffect(() => { setSelecting(false); setCheckedIds(new Set()); }, [projectId]);
  useEffect(() => { setCheckedIds(previous => { const valid = new Set<string>(files.map(file => file.id)); const next = new Set([...previous].filter(id => valid.has(id))); return next.size === previous.size ? previous : next; }); }, [files]);
  function checkFiles(items: readonly ProjectFile[], value: boolean) {
    setCheckedIds(previous => { const next = new Set(previous); for (const file of items) { if (value) next.add(file.id); else next.delete(file.id); } return next; });
  }
  async function refreshFiles() {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['files', projectId] }),
      queryClient.invalidateQueries({ queryKey: ['project', projectId] }),
      queryClient.invalidateQueries({ queryKey: ['versions', projectId] }),
      queryClient.invalidateQueries({ queryKey: ['project-directories'] }),
    ]);
  }

  const userId=useAuthStore(state=>state.user?.id);
  const members=useQuery({queryKey:['members',projectId],queryFn:()=>api.members.list(projectId),enabled:!!userId});
  const role=members.data?.find(member=>member.userId===userId)?.role;
  const canWrite=role==='owner'||role==='editor';
  const directories=useQuery({queryKey:['project-directories',userId,projectId],queryFn:()=>projectActionsApi.listDirectories(projectId),enabled:!!userId});
  const directoryPaths=directories.data?.directories.map(item=>item.path)??[];
  const protectedDirectory=(path:string)=>directoryPaths.some(item=>item===path||item.startsWith(`${path}/`));
  const tree=useMemo(()=>buildFileTree(files,directories.data?.directories.map(item=>item.path)??[]),[files,directories.data]);

  const createMutation = useMutation<ProjectFile, ApiError, { path: string; content?: string }>({
    mutationFn: ({ path, content }) => api.files.create(projectId, path, content),
    onSuccess: async (file) => {
      toast.success(t('project.fileCreated', { path: file.path }));
      await queryClient.invalidateQueries({ queryKey: ['files', projectId] });
    },
    onError: (err) => {
      toast.error(err.body.message);
    },
  });

  const uploadMutation=useMutation<{uploaded:number;failed:string|null},Error,File[]>({
    mutationFn:selected=>uploadBatch(selected,directoryPaths,{
      listFiles:()=>api.files.list(projectId),inspectZip:file=>api.projects.inspectZip(file),uploadZip:file=>api.projects.uploadZip(projectId,file),uploadFile:(path,file)=>api.files.upload(projectId,path,file),
    }),
    onSuccess:async(result)=>{await queryClient.invalidateQueries({queryKey:['files',projectId]});await queryClient.invalidateQueries({queryKey:['project-directories']});if(result.failed)toast.warning(`已上传${result.uploaded}个文件；${result.failed}。已有文件未覆盖。`);else toast.success(`已上传${result.uploaded}个文件`);},
    onError:error=>toast.error(error.message),
  });

  const renameMutation = useMutation<
    unknown,
    ApiError,
    { fileId: ProjectFile['id']; newPath: string }
  >({
    mutationFn: ({ fileId, newPath }) => api.files.rename(projectId, fileId, { newPath }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['files', projectId] });
    },
    onError: (err) => {
      toast.error(err.body.message);
    },
  });

  const deleteMutation = useMutation<unknown, ApiError, ProjectFile>({
    mutationFn: (file) => api.files.remove(projectId, file.id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['files', projectId] });
    },
    onError: (err) => {
      toast.error(err.body.message);
    },
  });

  async function downloadFile(file: ProjectFile) {
    try {
      const { url } = await api.files.downloadUrl(projectId, file.id);
      const a = document.createElement('a');
      a.href = url;
      a.download = file.path.split('/').pop() ?? file.path;
      a.click();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  function promptNewFile(prefix = '') {
    if (!canWrite || busy) return;
    const seed = prefix === '' ? 'new.tex' : `${prefix}/new.tex`;
    const name = window.prompt(t('project.newFilePrompt'), seed);
    if (name === null || name.trim() === '') return;
    createMutation.mutate({ path: name.trim() });
  }

  function promptNewFolder(prefix = '') {
    if (!canWrite || busy) return;
    const seed = prefix === '' ? 'chapters' : `${prefix}/sub`;
    const name = window.prompt(t('project.newFolderPrompt'), seed);
    if (name === null || name.trim() === '') return;
    const folder = name.trim().replace(/\/$/, '');
    createMutation.mutate({ path: `${folder}/.gitkeep`, content: '' });
    setExpanded((s) => new Set([...s, folder]));
  }

  function promptRenameFile(file: ProjectFile) {
    if (!canWrite || busy) return;
    const next = window.prompt(t('project.renameFilePrompt'), file.path);
    if (next === null || next.trim() === '' || next.trim() === file.path) return;
    renameMutation.mutate(
      { fileId: file.id, newPath: next.trim() },
      {
        onSuccess: () => {
          toast.success(t('project.fileRenamed', { path: next.trim() }));
        },
      },
    );
  }

  async function changeFolder(folder: FolderNode, kind: 'rename' | 'delete', destination?: string) {
    setBatchBusy(true);
    try {
      const results = await withFileOperationGuard(projectId, true, () => mutateFolder({
        path: folder.path, expected: filesUnder(files, folder.path), kind, destination,
        listFiles: () => api.files.list(projectId),
        listDirectories: async () => (await projectActionsApi.listDirectories(projectId)).directories.map(item => item.path),
        checkpoint: () => api.versions.snapshot(projectId, { label: `${kind === 'delete' ? '删除目录前' : '目录改名前'} · ${folder.path}` }),
        rename: (file, newPath) => api.files.rename(projectId, file.id, { newPath }),
        remove: file => api.files.remove(projectId, file.id),
        refresh: refreshFiles,
      }));
      const failed = results.filter(item => !item.ok);
      if (failed.length) toast.warning(`已完成 ${results.length - failed.length} 项，${failed.length} 项失败：${failed.map(item => `${item.path}：${item.error}`).join('；')}`);
      else {
        if (kind === 'rename' && destination) setExpanded(previous => new Set([...previous, destination]));
        toast.success(t(kind === 'rename' ? 'project.folderRenamed' : 'project.folderDeleted', { path: destination ?? folder.path }));
      }
    } catch (error) { toast.error(batchError(error)); }
    finally { setBatchBusy(false); }
  }

  async function promptRenameFolder(folder: FolderNode) {
    if (!canWrite || busy) return;
    if(protectedDirectory(folder.path)){toast.message('此目录暂不支持整体改名，可单独移动其中的文件。');return;}
    const next = window.prompt(t('project.renameFolderPrompt'), folder.path);
    if (next === null || next.trim() === '' || next.trim() === folder.path) return;
    await changeFolder(folder, 'rename', next.trim().replace(/\/$/, ''));
  }

  async function confirmDeleteFolder(folder: FolderNode) {
    if (!canWrite || busy) return;
    if(protectedDirectory(folder.path)){toast.message('此目录暂不支持删除，可单独管理其中的文件。');return;}
    if (!window.confirm(t('project.deleteFolderConfirm', { path: folder.path, count: filesUnder(files, folder.path).length }))) return;
    await changeFolder(folder, 'delete');
  }

  function handleDelete(file: ProjectFile) {
    if (!canWrite || busy) return;
    if (window.confirm(t('project.deleteFileConfirm', { path: file.path }))) {
      deleteMutation.mutate(file, {
        onSuccess: () => {
          toast.success(t('project.fileDeleted', { path: file.path }));
        },
      });
    }
  }

  function handleUploadChange(event:React.ChangeEvent<HTMLInputElement>){
    if (!canWrite || busy) return;
    const selected=Array.from(event.target.files??[]);event.target.value='';if(selected.length)uploadMutation.mutate(selected);
  }

  async function handleFolderUploadChange(event: React.ChangeEvent<HTMLInputElement>) {
    const selected = Array.from(event.target.files ?? []); event.target.value = '';
    if (!canWrite || busy || !selected.length) return;
    setPackingFolder(true);
    try {
      const { archive, name } = await packageFolderUpload(selected);
      setExpanded(previous => new Set([...previous, name]));
      uploadMutation.mutate([archive]);
    } catch (error) { toast.error(error instanceof Error ? error.message : String(error)); }
    finally { setPackingFolder(false); }
  }

  function toggleFolder(path: string) {
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  function handleFileDragStart(e: React.DragEvent<HTMLDivElement>, file: ProjectFile) {
    e.dataTransfer.setData(MIME_FILE_DRAG, file.id);
    e.dataTransfer.effectAllowed = 'move';
  }

  function handleFolderDragOver(e: React.DragEvent<HTMLElement>, folderPath: string) {
    if (!claimFileTreeDrag(e, MIME_FILE_DRAG) || !canWrite || busy || selecting) return;
    e.dataTransfer.dropEffect = 'move';
    setDropTarget(folderPath);
  }

  function handleFolderDrop(e: React.DragEvent<HTMLElement>, folderPath: string) {
    if (!claimFileTreeDrag(e, MIME_FILE_DRAG) || !canWrite || busy || selecting) return;
    setDropTarget(null);
    const fileId = e.dataTransfer.getData(MIME_FILE_DRAG);
    const file = files.find((f) => f.id === fileId);
    if (file === undefined) return;
    const leaf = file.path.split('/').pop() ?? file.path;
    const newPath = folderPath === '' ? leaf : `${folderPath}/${leaf}`;
    if (newPath === file.path) return;
    renameMutation.mutate(
      { fileId: file.id, newPath },
      {
        onSuccess: () => {
          toast.success(t('project.fileMoved', { path: newPath }));
        },
      },
    );
  }

  const renderNode = (node: TreeNode, depth: number): React.ReactNode => {
    const indent = { paddingLeft: `${(depth * 8 + 4).toString()}px` };
    if (node.type === 'folder') {
      const isOpen = expanded.has(node.path);
      const Icon = isOpen ? FolderOpen : FolderClosed;
      const Chevron = isOpen ? ChevronDown : ChevronRight;
      const isDropTarget = dropTarget === node.path;
      return (
        <li key={`folder:${node.path}`}>
          <ContextMenu>
            <ContextMenuTrigger asChild>
              <div
                className={`group flex items-center gap-1 rounded px-1 py-1 text-sm hover:bg-accent/60 ${
                  isDropTarget ? 'outline outline-1 outline-primary/60 bg-primary/5' : ''
                }`}
                style={indent}
                onDragOver={(e) => {
                  handleFolderDragOver(e, node.path);
                }}
                onDragLeave={(event) => {
                  if (event.dataTransfer.types.includes(MIME_FILE_DRAG)) event.stopPropagation();
                  if (dropTarget === node.path) setDropTarget(null);
                }}
                onDrop={(e) => {
                  handleFolderDrop(e, node.path);
                }}
              >
                {selecting && <input type="checkbox" className="h-3.5 w-3.5 shrink-0" disabled={busy} aria-label={`选择目录 ${node.path}`} checked={filesUnder(files,node.path).length > 0 && filesUnder(files,node.path).every(file=>checkedIds.has(file.id))} onChange={event=>checkFiles(filesUnder(files,node.path),event.target.checked)} />}
                <button
                  type="button"
                  className="flex flex-1 items-center gap-1 truncate text-left"
                  onClick={() => {
                    toggleFolder(node.path);
                  }}
                >
                  <Chevron
                    className="h-3 w-3 flex-shrink-0 text-muted-foreground"
                    aria-hidden="true"
                  />
                  <Icon
                    className="h-3.5 w-3.5 flex-shrink-0 text-muted-foreground"
                    aria-hidden="true"
                  />
                  <span className="flex-1 truncate">{node.name}</span>
                </button>
                <button
                  type="button"
                  className="opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100 disabled:invisible"
                  disabled={!canWrite || busy}
                  onClick={() => {
                    promptNewFile(node.path);
                  }}
                  aria-label={t('project.newFile')}
                  title={t('project.newFile')}
                >
                  <FilePlus className="h-3 w-3 text-muted-foreground hover:text-foreground" />
                </button>
              </div>
            </ContextMenuTrigger>
            <ContextMenuContent>
              <ContextMenuItem
                disabled={!canWrite || busy}
                onClick={() => {
                  promptNewFile(node.path);
                }}
              >
                <FilePlus className="h-3.5 w-3.5" aria-hidden="true" />
                {t('project.newFile')}
              </ContextMenuItem>
              <ContextMenuItem
                disabled={!canWrite || busy}
                onClick={() => {
                  promptNewFolder(node.path);
                }}
              >
                <FolderPlus className="h-3.5 w-3.5" aria-hidden="true" />
                {t('project.newFolder')}
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem
                disabled={!canWrite || busy || protectedDirectory(node.path)}
                title={protectedDirectory(node.path)?'真实目录暂不支持整体改名':undefined}
                onClick={() => {
                  void promptRenameFolder(node);
                }}
              >
                <Pencil className="h-3.5 w-3.5" aria-hidden="true" />
                {t('project.renameFile')}
              </ContextMenuItem>
              <ContextMenuItem
                destructive
                disabled={!canWrite || busy || protectedDirectory(node.path)}
                title={protectedDirectory(node.path)?'真实目录暂不支持删除':undefined}
                onClick={() => {
                  void confirmDeleteFolder(node);
                }}
              >
                <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                {t('common.delete')}
              </ContextMenuItem>
            </ContextMenuContent>
          </ContextMenu>
          {isOpen && node.children.length > 0 ? (
            <ul className="space-y-px">{node.children.map((c) => renderNode(c, depth + 1))}</ul>
          ) : null}
        </li>
      );
    }
    const isSelected = selectedFileId === node.file.id;
    return (
      <li key={node.file.id}>
        <ContextMenu>
          <ContextMenuTrigger asChild>
            <div
              className={`group flex items-center gap-1 rounded px-1 py-1 text-sm ${
                isSelected ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/60'
              }`}
              style={indent}
              draggable={canWrite && !busy && !selecting}
              onDragStart={(e) => {
                handleFileDragStart(e, node.file);
              }}
            >
              {selecting && <input type="checkbox" className="h-3.5 w-3.5 shrink-0" disabled={busy} aria-label={`选择文件 ${node.file.path}`} checked={checkedIds.has(node.file.id)} onChange={event=>checkFiles([node.file],event.target.checked)} />}
              <button
                type="button"
                disabled={batchBusy}
                className="flex flex-1 items-center gap-1 truncate text-left"
                onClick={() => { if (selecting) checkFiles([node.file], !checkedIds.has(node.file.id)); else onSelect?.(node.file); }}
                onDoubleClick={() => {
                  promptRenameFile(node.file);
                }}
                aria-current={isSelected ? 'true' : undefined}
              >
                <span className="w-3" aria-hidden="true" />
                <FileText
                  className="h-3.5 w-3.5 flex-shrink-0 text-muted-foreground"
                  aria-hidden="true"
                />
                <span className="flex-1 truncate">{node.name}</span>
              </button>
              <button
                type="button"
                className="opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100 disabled:invisible"
                disabled={!canWrite || busy}
                onClick={() => {
                  promptRenameFile(node.file);
                }}
                aria-label={t('project.renameFile')}
                title={t('project.renameFile')}
              >
                <Pencil className="h-3 w-3 text-muted-foreground hover:text-foreground" />
              </button>
              <button
                type="button"
                className="opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100 disabled:invisible"
                disabled={!canWrite || busy}
                onClick={() => {
                  handleDelete(node.file);
                }}
                aria-label={t('common.delete')}
              >
                <Trash2 className="h-3 w-3 text-muted-foreground hover:text-destructive" />
              </button>
            </div>
          </ContextMenuTrigger>
          <ContextMenuContent>
            <ContextMenuItem disabled={batchBusy} onClick={() => onSelect?.(node.file)}>
              <FileText className="h-3.5 w-3.5" aria-hidden="true" />
              {t('project.openFile')}
            </ContextMenuItem>
            <ContextMenuItem
              disabled={!canWrite || busy}
              onClick={() => {
                promptRenameFile(node.file);
              }}
            >
              <Pencil className="h-3.5 w-3.5" aria-hidden="true" />
              {t('project.renameFile')}
            </ContextMenuItem>
            <ContextMenuItem
              onClick={() => {
                void downloadFile(node.file);
              }}
            >
              <Download className="h-3.5 w-3.5" aria-hidden="true" />
              {t('project.downloadFile')}
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem
              destructive
              disabled={!canWrite || busy}
              onClick={() => {
                handleDelete(node.file);
              }}
            >
              <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
              {t('common.delete')}
            </ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
      </li>
    );
  };

  const busy = packingFolder || batchBusy || createMutation.isPending || uploadMutation.isPending || renameMutation.isPending || deleteMutation.isPending;

  return (
    <div
      className={`space-y-2 ${dropTarget === '' ? 'rounded outline outline-1 outline-primary/60' : ''}`}
      onDragOver={(e) => {
        handleFolderDragOver(e, '');
      }}
      onDragLeave={() => {
        if (dropTarget === '') setDropTarget(null);
      }}
      onDrop={(e) => {
        handleFolderDrop(e, '');
      }}
    >
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold uppercase text-muted-foreground">
          {t('project.files')}
        </span>
        <div className="flex items-center gap-0.5">
          <Button variant="ghost" size="icon" className={`h-6 w-6 ${selecting ? 'bg-accent' : ''}`} disabled={busy} aria-label="批量选择文件" title="批量选择文件" aria-pressed={selecting} onClick={()=>{setSelecting(value=>!value);setCheckedIds(new Set());}}><ListChecks className="h-3.5 w-3.5" /></Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            aria-label={t('project.newFile')}
            title={t('project.newFile')}
            onClick={() => {
              promptNewFile();
            }}
            disabled={busy || !canWrite}
          >
            <FilePlus className="h-3 w-3" aria-hidden="true" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            aria-label={t('project.newFolder')}
            title={t('project.newFolder')}
            onClick={() => {
              promptNewFolder();
            }}
            disabled={busy || !canWrite}
          >
            <FolderPlus className="h-3 w-3" aria-hidden="true" />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" className="h-6 gap-1 px-1.5 text-xs" aria-label="上传" title="上传" disabled={busy || !canWrite}>
                {uploadMutation.isPending || packingFolder ? <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" /> : <Upload className="h-3 w-3" aria-hidden="true" />}
                上传<ChevronDown className="h-3 w-3" aria-hidden="true" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem disabled={busy || !canWrite} onSelect={() => uploadInputRef.current?.click()}><FilePlus className="h-3.5 w-3.5" />文件或 ZIP 压缩包</DropdownMenuItem>
              <DropdownMenuItem disabled={busy || !canWrite} onSelect={() => folderInputRef.current?.click()}><FolderOpen className="h-3.5 w-3.5" />文件夹</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <input
            ref={uploadInputRef}
            type="file"
            id="file-tree-upload"
            name="file-tree-upload"
            multiple
            disabled={busy || !canWrite}
            accept=".tex,.bib,.cls,.sty,.bst,.tikz,.latex,.md,.txt,.csv,.json,.png,.jpg,.jpeg,.gif,.webp,.svg,.pdf,.eps,.zip"
            className="hidden"
            onChange={handleUploadChange}
            aria-hidden="true"
          />
          <input ref={folderInputRef} type="file" multiple id="file-tree-folder-upload" name="file-tree-folder-upload" className="hidden" disabled={busy || !canWrite} onChange={event => void handleFolderUploadChange(event)} aria-hidden="true" />
        </div>
      </div>
      {selecting && <FileBatchActions key={projectId} projectId={projectId} files={files} selected={selectedFiles} directories={directoryPaths} canWrite={canWrite} onSelection={setCheckedIds} onBusy={setBatchBusy} onRefresh={refreshFiles} onClose={()=>{setSelecting(false);setCheckedIds(new Set());}} />}
      {tree.length === 0 ? (
        <p className="text-xs text-muted-foreground">{t('project.noFiles')}</p>
      ) : (
        <ul className="space-y-px">{tree.map((n) => renderNode(n, 0))}</ul>
      )}
    </div>
  );
}

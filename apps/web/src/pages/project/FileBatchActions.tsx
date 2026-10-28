import { type ProjectFile, type ProjectId } from '@scribe/shared';
import { Button, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, Input } from '@scribe/ui';
import { zip } from 'fflate';
import { Download, FolderInput, Loader2, Trash2, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { api } from '../../lib/api';
import { BATCH_MAX_BYTES, uploadPath } from '../../lib/archive-upload';
import { batchError, planFileMoves, runFileBatch, type FileBatchResult } from '../../lib/file-batch';
import { withFileOperationGuard } from '../../lib/file-operation-guard';
import { projectActionsApi } from '../../lib/project-actions';
import { ProjectFolderPicker } from '../../components/project/ProjectFolderPicker';

interface Props {
  projectId: ProjectId;
  files: readonly ProjectFile[];
  selected: readonly ProjectFile[];
  directories: readonly string[];
  canWrite: boolean;
  moveRequested?: number;
  onSelection: (ids: ReadonlySet<string>) => void;
  onBusy: (busy: boolean) => void;
  onRefresh: () => Promise<void>;
  onClose: () => void;
}

function makeZip(entries: Record<string, Uint8Array>): Promise<Uint8Array> {
  return new Promise((resolve, reject) => zip(entries, { level: 6 }, (error, data) => error ? reject(error) : resolve(data)));
}

export function FileBatchActions({ projectId, files, selected, directories, canWrite, moveRequested = 0, onSelection, onBusy, onRefresh, onClose }: Props) {
  const { i18n } = useTranslation();
  const text = (zh: string, en: string) => i18n.language.startsWith('zh') ? zh : en;
  const [mode, setMode] = useState<'move' | 'delete' | null>(null);
  const [directory, setDirectory] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<FileBatchResult[]>([]);
  const [error, setError] = useState('');
  const [progress, setProgress] = useState(0);
  useEffect(() => { if (moveRequested > 0) { setMode('move'); setError(''); } }, [moveRequested]);
  const chosen = selected.length;
  const planned = (() => {
    if (mode !== 'move') return { moves: [], error: '' };
    try { return { moves: planFileMoves(selected, files, directories, directory), error: '' }; }
    catch (failure) { return { moves: [], error: batchError(failure) }; }
  })();

  async function execute(kind: 'move' | 'delete' | 'export') {
    if (busy || chosen === 0 || (kind !== 'export' && !canWrite)) return;
    const batch = [...selected];
    setBusy(true); onBusy(true); setError(''); setResults([]); setProgress(0);
    try {
      await withFileOperationGuard(projectId, kind !== 'export', async () => {
        // Read fresh paths before preflight; the API re-checks permissions and move guards.
        const current = await api.files.list(projectId);
        const live = batch.map(file => {
          const next = current.find(item => item.id === file.id && item.path === file.path);
          if (!next) throw new Error(text('选中文件已变化，请刷新后重新选择', 'Selected files changed; refresh and select again'));
          return next;
        });
        if (kind === 'export') {
          if (live.reduce((sum, file) => sum + file.sizeBytes, 0) > BATCH_MAX_BYTES || live.length > 256) throw new Error(text('每次导出最多 256 个文件、32MiB', 'Export supports up to 256 files and 32MiB'));
          const entries: Record<string, Uint8Array> = Object.create(null) as Record<string, Uint8Array>;
          let bytes = 0;
          const exported = await runFileBatch(live, async file => {
            const path = uploadPath(file.path);
            const { url } = await api.files.downloadUrl(projectId, file.id);
            const response = await fetch(url, { credentials: 'omit', cache: 'no-store' });
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            const data = new Uint8Array(await response.arrayBuffer());
            bytes += data.byteLength;
            if (bytes > BATCH_MAX_BYTES) throw new Error(text('导出超过 32MiB', 'Export exceeds 32MiB'));
            entries[path] = data;
            setProgress(value => value + 1);
          });
          // An incomplete archive is never downloaded as a successful export.
          if (exported.some(item => !item.ok)) { setResults(exported); throw new Error(text('部分文件读取失败，未生成压缩包；可重试导出', 'Some files could not be read; no archive was downloaded. Retry the export.')); }
          const compressed = await makeZip(entries);
          const url = URL.createObjectURL(new Blob([compressed as BlobPart], { type: 'application/zip' }));
          const link = document.createElement('a'); link.href = url; link.download = `project-files-${new Date().toISOString().slice(0, 10)}.zip`; link.click();
          window.setTimeout(() => URL.revokeObjectURL(url), 60000);
          setResults(exported); toast.success(text(`已导出 ${live.length} 个文件`, `Exported ${live.length} files`));
          return;
        }
        let operationResults: FileBatchResult[];
        if (kind === 'delete') {
          // Stop before the first delete if the history checkpoint cannot be written.
          await api.versions.snapshot(projectId, { label: text(`批量删除前 · ${live.length} 个文件`, `Before deleting ${live.length} files`) });
          operationResults = await runFileBatch(live, async file => { await api.files.remove(projectId, file.id); setProgress(value => value + 1); });
        } else {
          const freshDirectories = await projectActionsApi.listDirectories(projectId);
          const moves = planFileMoves(live, current, freshDirectories.directories.map(item => item.path), directory);
          operationResults = await runFileBatch(moves.map(move => ({ ...move.file, newPath: move.newPath })), async move => {
            const plan = await projectActionsApi.prepare(projectId, [{ type: 'move_file', path: move.path, newPath: move.newPath }]);
            const action = plan.actions[0];
            if (!action) throw new Error(text('移动计划未生成', 'Move plan is empty'));
            const applied = await projectActionsApi.apply(projectId, plan.planId, action.id);
            if (applied.status !== 'applied') throw new Error(text('移动未完成', 'Move was not applied'));
            setProgress(value => value + 1);
          });
          for (const result of operationResults) { const destination = moves.find(move => move.file.id === result.id)?.newPath; if (destination) result.destination = destination; }
          const movedIds = new Set(moves.map(move => move.file.id));
          operationResults.push(...live.filter(file => !movedIds.has(file.id)).map(file => ({ id: file.id, path: file.path, ok: true, destination: file.path })));
        }
        setResults(operationResults);
        onSelection(new Set(operationResults.filter(item => !item.ok).map(item => item.id)));
        const failed = operationResults.filter(item => !item.ok).length;
        if (failed) toast.warning(text(`${operationResults.length - failed} 项完成，${failed} 项失败`, `${operationResults.length - failed} completed; ${failed} failed`));
        else toast.success(text(`已完成 ${operationResults.length} 项`, `Completed ${operationResults.length} items`));
        await onRefresh();
        setMode(null); setConfirmation('');
      });
    } catch (failure) { setError(batchError(failure)); }
    finally {
      // Even a transport error may follow a successful server write; always refresh.
      try { await onRefresh(); } catch { setError(previous => previous || text('刷新失败，请刷新项目查看结果', 'Refresh failed; reload the project to check results')); }
      setBusy(false); onBusy(false);
    }
  }

  return <div className="rounded-lg border bg-accent/20 p-2 text-xs">
    <div className="flex items-center justify-between gap-2">
      <label className="flex items-center gap-2"><input type="checkbox" checked={files.length > 0 && chosen === files.length} disabled={busy} onChange={event => onSelection(new Set(event.target.checked ? files.map(file => file.id) : []))} />{text(`已选 ${chosen} 项`, `${chosen} selected`)}</label>
      <Button variant="ghost" size="icon" className="h-6 w-6" disabled={busy} onClick={onClose} aria-label={text('退出多选', 'Exit selection')}><X className="h-3.5 w-3.5" /></Button>
    </div>
    <div className="mt-2 flex flex-wrap gap-1">
      <Button variant="outline" size="sm" className="h-7 px-2 text-xs" disabled={busy || !chosen} onClick={() => void execute('export')}><Download className="mr-1 h-3.5 w-3.5" />{text('导出', 'Export')}</Button>
      {canWrite && <><Button variant="outline" size="sm" className="h-7 px-2 text-xs" disabled={busy || !chosen} onClick={() => { setMode('move'); setError(''); }}><FolderInput className="mr-1 h-3.5 w-3.5" />{text('移动', 'Move')}</Button><Button variant="outline" size="sm" className="h-7 px-2 text-xs text-destructive" disabled={busy || !chosen} onClick={() => { setMode('delete'); setConfirmation(''); setError(''); }}><Trash2 className="mr-1 h-3.5 w-3.5" />{text('删除', 'Delete')}</Button></>}
    </div>
    {busy && <p className="mt-2 flex items-center gap-1" role="status"><Loader2 className="h-3 w-3 animate-spin" />{progress} / {chosen}</p>}
    {error && !mode && <p role="alert" className="mt-2 break-words text-destructive">{error}</p>}
    {results.length > 0 && <details open={results.some(item => !item.ok)} className="mt-2"><summary className="cursor-pointer">{text('逐项结果', 'Item results')} · {results.filter(item => item.ok).length}/{results.length}</summary><ul className="mt-1 max-h-48 space-y-1 overflow-auto">{results.map(item => <li key={item.id} className={`break-all ${item.ok ? 'text-muted-foreground' : 'text-destructive'}`}>{item.ok ? '✓' : '×'} {item.path}{item.destination && item.destination !== item.path ? ` → ${item.destination}` : ''}{item.error ? ` — ${item.error}` : ''}</li>)}</ul></details>}
    <Dialog open={mode !== null} onOpenChange={open => { if (!busy && !open) { setMode(null); setError(''); } }}>
      <DialogContent className="max-w-lg" onEscapeKeyDown={event => { if (busy) event.preventDefault(); }} onPointerDownOutside={event => { if (busy) event.preventDefault(); }}>
        <DialogHeader><DialogTitle>{mode === 'delete' ? text(`删除 ${chosen} 个文件？`, `Delete ${chosen} files?`) : text(`移动 ${chosen} 个文件`, `Move ${chosen} files`)}</DialogTitle><DialogDescription>{mode === 'delete' ? text('先保存当前编辑内容并创建文本版本，再逐项删除。图片等二进制文件无法从文本版本恢复，请先导出备份。', 'The editor is saved and a text checkpoint is created before deletion. Binary files such as images cannot be recovered from text history; export a backup first.') : text('保留所选文件的相对目录。目标已有同名文件时停止，不覆盖。', 'Preserves relative folders. Existing destination files are never overwritten.')}</DialogDescription></DialogHeader>
        {mode === 'move' && <ProjectFolderPicker files={files} directories={directories} value={directory} disabled={busy} onChange={setDirectory} />}
        <ul className="max-h-52 space-y-1 overflow-auto rounded border p-3 text-sm">{selected.map(file => <li className="break-all" key={file.id}>{file.path}{mode === 'move' && planned.moves.find(move => move.file.id === file.id) ? ` → ${planned.moves.find(move => move.file.id === file.id)?.newPath}` : ''}</li>)}</ul>
        {mode === 'delete' && <label className="space-y-2 text-sm"><span>{text(`输入 ${chosen} 确认删除`, `Type ${chosen} to confirm deletion`)}</span><Input autoFocus value={confirmation} disabled={busy} inputMode="numeric" onChange={event => setConfirmation(event.target.value)} /></label>}
        {(error || planned.error) && <p role="alert" className="break-words text-sm text-destructive">{error || planned.error}</p>}
        <DialogFooter><Button variant="outline" disabled={busy} onClick={() => setMode(null)}>{text('取消', 'Cancel')}</Button><Button disabled={busy || !chosen || !canWrite || (mode === 'delete' ? confirmation !== String(chosen) : !!planned.error || !planned.moves.length)} onClick={() => mode && void execute(mode)}>{busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}{mode === 'delete' ? text('确认删除', 'Confirm deletion') : text('确认移动', 'Confirm move')}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  </div>;
}

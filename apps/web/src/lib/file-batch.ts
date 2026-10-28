import { preflightUploadPaths, uploadPath } from './archive-upload';

export interface BatchFile { id: string; path: string; sizeBytes?: number }
export interface FileBatchResult { id: string; path: string; destination?: string; ok: boolean; error?: string }
export interface FileMove { file: BatchFile; newPath: string }

/** Keep the selected subtree relative to its common parent, without flattening names. */
export function planFileMoves(selected: readonly BatchFile[], current: readonly BatchFile[], directories: readonly string[], rawDirectory: string): FileMove[] {
  const raw = rawDirectory.trim();
  if (raw.startsWith('/')) throw new Error('目标必须是项目内的相对目录 / Use a project-relative folder');
  const directory = raw.replace(/\/$/, '');
  if (directory) uploadPath(directory);
  const parents = selected.map(file => file.path.split('/').slice(0, -1));
  const common = [...(parents[0] ?? [])];
  for (const parent of parents) while (common.some((part, index) => parent[index] !== part)) common.pop();
  const moves = selected.map(file => {
    const relative = file.path.split('/').slice(common.length).join('/');
    return { file, newPath: uploadPath(directory ? `${directory}/${relative}` : relative) };
  }).filter(move => move.file.path !== move.newPath);
  preflightUploadPaths(moves.map(move => move.newPath), current.map(file => file.path), directories);
  return moves;
}

export function batchError(error: unknown): string {
  const body = (error as { body?: { message?: string } } | null)?.body;
  return body?.message ?? (error instanceof Error ? error.message : String(error));
}

/** Every selected item gets a result; successful items are never silently repeated. */
export async function runFileBatch<T extends BatchFile>(files: readonly T[], operation: (file: T) => Promise<void>): Promise<FileBatchResult[]> {
  const results: FileBatchResult[] = [];
  for (const file of files) {
    try { await operation(file); results.push({ id: file.id, path: file.path, ok: true }); }
    catch (error) { results.push({ id: file.id, path: file.path, ok: false, error: batchError(error) }); }
  }
  return results;
}

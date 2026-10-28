import { preflightUploadPaths, uploadPath } from './archive-upload';
import { type BatchFile, type FileBatchResult, runFileBatch } from './file-batch';

/** The innermost folder owns this drag; ancestor folders must not issue another move. */
export function claimFileTreeDrag(event: { dataTransfer: { types: readonly string[] }; preventDefault(): void; stopPropagation(): void }, mime: string): boolean {
  if (!event.dataTransfer.types.includes(mime)) return false;
  event.preventDefault();
  event.stopPropagation();
  return true;
}

interface FolderMutation<T extends BatchFile> {
  path: string;
  expected: readonly T[];
  kind: 'rename' | 'delete';
  destination?: string | undefined;
  listFiles(): Promise<T[]>;
  listDirectories(): Promise<string[]>;
  checkpoint(): Promise<unknown>;
  rename(file: T, path: string): Promise<unknown>;
  remove(file: T): Promise<unknown>;
  refresh(): Promise<void>;
}

/** Preflight the complete fresh subtree before writes; refresh even after a partial failure. */
export async function mutateFolder<T extends BatchFile>(operation: FolderMutation<T>): Promise<FileBatchResult[]> {
  try {
    const current = await operation.listFiles();
    const contained = current.filter(file => file.path.startsWith(`${operation.path}/`));
    if (contained.length !== operation.expected.length || contained.some(file => !operation.expected.some(expected => expected.id === file.id && expected.path === file.path))) {
      throw new Error('目录内容已变化，请刷新后重试 / Folder changed; refresh and retry');
    }
    let destination: string | undefined;
    if (operation.kind === 'rename') {
      destination = uploadPath(operation.destination ?? '');
      if (destination === operation.path || destination.startsWith(`${operation.path}/`)) throw new Error('不能将目录移动到自身内部 / Cannot move a folder into itself');
      const targets = contained.map(file => destination + file.path.slice(operation.path.length));
      preflightUploadPaths(targets, current.map(file => file.path), await operation.listDirectories());
    }
    await operation.checkpoint();
    return await runFileBatch(contained, file => operation.kind === 'delete'
      ? operation.remove(file).then(() => {})
      : operation.rename(file, destination! + file.path.slice(operation.path.length)).then(() => {}));
  } finally { await operation.refresh(); }
}

import { zip } from 'fflate';

import { BATCH_MAX_BYTES, FILE_MAX_BYTES, preflightUploadPaths, uploadPath, ZIP_MAX_BYTES } from './archive-upload';

export interface FolderFile {
  readonly name: string;
  readonly size: number;
  readonly webkitRelativePath: string;
  arrayBuffer(): Promise<ArrayBuffer>;
}

/** Directory pickers expose files, not empty directories or the local absolute path. */
export function planFolderImport(selected: readonly FolderFile[]) {
  if (!selected.length) throw new Error('请选择包含 LaTeX 文件的文件夹');
  let root = '';
  let totalBytes = 0;
  let skippedMetadata = 0;
  const files: { path: string; file: FolderFile }[] = [];
  for (const file of selected) {
    const raw = file.webkitRelativePath;
    const separator = raw.indexOf('/');
    if (separator < 1) throw new Error('浏览器未提供文件夹路径，请改用 ZIP 导入');
    const folder = uploadPath(raw.slice(0, separator));
    if (root && root !== folder) throw new Error('请一次选择一个项目文件夹');
    root = folder;
    const path = raw.slice(separator + 1);
    // Keep useful dotfiles such as .latexmkrc; omit OS metadata and VCS internals.
    if (path.split('/').some(part => part === '__MACOSX' || part === '.DS_Store' || part === 'Thumbs.db' || part === 'desktop.ini' || part === '.git' || part.startsWith('._'))) {
      skippedMetadata += 1;
      continue;
    }
    uploadPath(path);
    if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > FILE_MAX_BYTES) throw new Error(`单文件最多8MiB：${path}`);
    totalBytes += file.size;
    files.push({ path, file });
    if (totalBytes > BATCH_MAX_BYTES || files.length > 256) throw new Error('文件夹最多256个文件，合计最多32MiB');
  }
  if (!files.length) throw new Error('所选文件夹没有可导入的文件');
  preflightUploadPaths(files.map(file => file.path), []);
  return { name: root, files, totalBytes, skippedMetadata };
}

/** Reuse the same server-verified ZIP import and initial version path. */
export async function packageFolderImport(selected: readonly FolderFile[]) {
  const plan = planFolderImport(selected);
  const entries: Record<string, Uint8Array> = Object.create(null);
  // Read sequentially to keep memory bounded by the existing archive quota.
  for (const { path, file } of plan.files) {
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (bytes.byteLength !== file.size) throw new Error(`读取文件时内容发生变化，请重新选择：${path}`);
    entries[path] = bytes;
  }
  const bytes = await new Promise<Uint8Array>((resolve, reject) => {
    zip(entries, { level: 6 }, (error, data) => error ? reject(error) : resolve(data));
  });
  if (bytes.byteLength > ZIP_MAX_BYTES) throw new Error('文件夹打包后超过20MiB，请拆分项目后重试');
  const archive = new File([new Uint8Array(bytes).buffer], `${plan.name}.zip`, { type: 'application/zip' });
  return { archive, name: plan.name, skippedMetadata: plan.skippedMetadata };
}

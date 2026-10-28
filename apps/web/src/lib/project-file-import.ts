import { ZIP_MAX_BYTES } from './archive-upload';
import { type FolderFile, packageFolderImport } from './folder-import';

/** A ZIP must be selected alone; ordinary files share the directory import path. */
export function classifyProjectImport(selected: readonly Pick<File, 'name'>[]): 'zip' | 'files' {
  if (!selected.length) throw new Error('请选择项目文件');
  const archives = selected.filter(file => /\.zip$/i.test(file.name));
  if (archives.length) {
    if (selected.length !== 1) throw new Error('ZIP 请单独选择，不能与其他文件或 ZIP 混合导入');
    return 'zip';
  }
  if (selected.some(file => /\.(?:rar|7z|tar|tgz|gz|bz2|xz)$/i.test(file.name))) throw new Error('压缩包目前支持 ZIP，请先转换为 ZIP 或选择解压后的文件夹');
  return 'files';
}

export async function packageProjectImport(selected: readonly File[], directory = false) {
  if (directory) return packageFolderImport(selected);
  if (classifyProjectImport(selected) === 'zip') {
    const archive = selected[0]!;
    if (archive.size > ZIP_MAX_BYTES) throw new Error('ZIP压缩包最多20MiB');
    return { archive, name: archive.name.replace(/\.zip$/i, ''), skippedMetadata: 0 };
  }
  const main = selected.find(file => /\.(?:tex|ltx|latex)$/i.test(file.name)) ?? selected[0]!;
  const name = main.name.replace(/\.[^.]+$/, '') || '导入项目';
  // The synthetic root is removed before packaging; original File bytes are never rewritten.
  const files: FolderFile[] = selected.map(file => ({ name: file.name, size: file.size, webkitRelativePath: `project/${file.webkitRelativePath || file.name}`, arrayBuffer: () => file.arrayBuffer() }));
  const packaged = await packageFolderImport(files);
  return { ...packaged, name };
}

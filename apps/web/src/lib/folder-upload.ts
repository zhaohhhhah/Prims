import { packageFolderImport, planFolderImport, type FolderFile } from './folder-import';

/** Project uploads keep the selected root folder; new-project imports strip it. */
export function planFolderUpload(selected: readonly FolderFile[]) {
  const plan = planFolderImport(selected);
  return {
    name: plan.name,
    skippedMetadata: plan.skippedMetadata,
    // The shared packager strips one root. A synthetic outer root lets it retain
    // the user's selected directory without changing new-project import behavior.
    files: plan.files.map(({ path, file }) => ({
      name: file.name,
      size: file.size,
      webkitRelativePath: `__prims_upload__/${plan.name}/${path}`,
      arrayBuffer: () => file.arrayBuffer(),
    })),
  };
}

export async function packageFolderUpload(selected: readonly FolderFile[]) {
  const plan = planFolderUpload(selected);
  const { archive } = await packageFolderImport(plan.files);
  return { archive, name: plan.name, skippedMetadata: plan.skippedMetadata };
}

/** Include explicit empty folders and every ancestor inferred from project files. */
export function projectFolderPaths(files: readonly { path: string }[], directories: readonly string[]): string[] {
  const paths = new Set<string>(['']);
  const add = (path: string) => {
    const parts = path.split('/').filter(Boolean);
    if (parts.some(part => part === '.' || part === '..')) return;
    for (let index = 1; index <= parts.length; index++) paths.add(parts.slice(0, index).join('/'));
  };
  directories.forEach(add);
  files.forEach(file => add(file.path.slice(0, Math.max(0, file.path.lastIndexOf('/')))));
  return [...paths].sort((left, right) => left.localeCompare(right, undefined, { numeric: true }));
}

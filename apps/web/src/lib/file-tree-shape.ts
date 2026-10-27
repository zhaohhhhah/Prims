import type {ProjectFile} from '@scribe/shared';
export interface FolderNode {
  readonly type: 'folder';
  readonly name: string;
  readonly path: string;
  children: TreeNode[];
}
export interface FileNode {
  readonly type: 'file';
  readonly name: string;
  readonly file: ProjectFile;
}
export type TreeNode = FolderNode | FileNode;

export function buildFileTree(files: readonly ProjectFile[],directories:readonly string[]=[]): TreeNode[] {
  const root: FolderNode = { type: 'folder', name: '', path: '', children: [] };

  for(const path of directories){
    if(path.startsWith('/')||path.includes('\\'))continue;
    const parts=path.split('/').filter(Boolean);if(!parts.length||parts.some(part=>part==='.'||part==='..'))continue;
    let cursor=root;for(let index=0;index<parts.length;index++){const name=parts[index]!;let child=cursor.children.find((node):node is FolderNode=>node.type==='folder'&&node.name===name);if(!child){child={type:'folder',name,path:parts.slice(0,index+1).join('/'),children:[]};cursor.children.push(child);}cursor=child;}
  }

  for (const file of files) {
    const parts = file.path.split('/').filter((p) => p.length > 0);
    if (parts.length === 0) continue;
    let cursor = root;
    for (let i = 0; i < parts.length - 1; i += 1) {
      const segment = parts[i];
      if (segment === undefined) continue;
      let folder = cursor.children.find(
        (c): c is FolderNode => c.type === 'folder' && c.name === segment,
      );
      if (folder === undefined) {
        folder = {
          type: 'folder',
          name: segment,
          path: parts.slice(0, i + 1).join('/'),
          children: [],
        };
        cursor.children.push(folder);
      }
      cursor = folder;
    }
    const leaf = parts[parts.length - 1];
    if (leaf === undefined || leaf === '.gitkeep') continue;
    cursor.children.push({ type: 'file', name: leaf, file });
  }

  sort(root.children);
  return root.children;
}

function sort(nodes: TreeNode[]): void {
  nodes.sort((a, b) => {
    if (a.type !== b.type) return a.type === 'folder' ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  for (const n of nodes) {
    if (n.type === 'folder') sort(n.children);
  }
}

/** All files whose path is `folderPath/...`. */

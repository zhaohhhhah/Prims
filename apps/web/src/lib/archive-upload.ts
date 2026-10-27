/** Preflight UX only: the server independently verifies archives and authorization. */
export const ZIP_MAX_BYTES=20*1024*1024;
export const FILE_MAX_BYTES=8*1024*1024;
export const BATCH_MAX_BYTES=32*1024*1024;
export function uploadPath(raw:string):string{
 if(raw.startsWith('/')||/[\\:\x00-\x1f\x7f]/.test(raw))throw new Error('文件路径不安全，请改名后上传');
 const path=raw.replace(/^(?:\.\/)+/,'').replace(/\/$/,'');
 if(!path||new TextEncoder().encode(path).length>512||path.split('/').length>32||path.split('/').some(part=>!part||part==='.'||part==='..'||part==='.git'))throw new Error('文件路径不安全或过长，请改名后上传');
 return path;
}
export function preflightUploadPaths(incoming:readonly string[],existingFiles:readonly string[],existingDirectories:readonly string[]=[],incomingDirectories:readonly string[]=[]):string[]{
 const paths=incoming.map(uploadPath);const seen=new Set<string>();
 for(const path of paths){if(seen.has(path))throw new Error(`本次选择包含重名文件：${path}`);seen.add(path);}
 const allFiles=[...existingFiles,...paths];const newDirectories=incomingDirectories.map(uploadPath);const allDirectories=[...existingDirectories,...newDirectories];
 for(const path of paths){if(existingFiles.includes(path)||allDirectories.some(directory=>directory===path||directory.startsWith(`${path}/`))||allFiles.some(other=>other!==path&&(other.startsWith(`${path}/`)||path.startsWith(`${other}/`))))throw new Error(`路径已存在或与目录冲突：${path}。请先改名；本次未上传任何文件。`);}
 for(const directory of newDirectories){if(allFiles.some(file=>file===directory||directory.startsWith(`${file}/`)))throw new Error(`待导入目录与文件冲突：${directory}。本次未上传任何文件。`);}
 return paths;
}

export interface BatchTransport{
 listFiles():Promise<{path:string}[]>;
 inspectZip(file:File):Promise<{files:{path:string}[];directories:string[];totalBytes:number}>;
 uploadZip(file:File):Promise<{created:unknown[];complete:boolean;failed:{message:string}|null}>;
 uploadFile(path:string,file:File):Promise<unknown>;
}
/** Preflight every selected file/archive before the first write. Runtime failures keep prior new files. */
export async function uploadBatch(selected:readonly File[],directories:readonly string[],io:BatchTransport):Promise<{uploaded:number;failed:string|null}>{
 if(selected.length>256)throw new Error('每批最多选择256个文件');
 const plans:({file:File;zip:true}|{file:File;zip:false;path:string})[]=[];const paths:string[]=[];const pendingDirectories:string[]=[];let total=0;let entries=0;
 for(const file of selected){
  if(file.name.toLowerCase().endsWith('.zip')){if(file.size>ZIP_MAX_BYTES)throw new Error('ZIP压缩包最多20MiB');const manifest=await io.inspectZip(file);total+=manifest.totalBytes;entries+=manifest.files.length+manifest.directories.length;paths.push(...manifest.files.map(item=>item.path));pendingDirectories.push(...manifest.directories);plans.push({file,zip:true});}
  else{if(file.size>FILE_MAX_BYTES)throw new Error(`单文件最多8MiB：${file.name}`);const path=uploadPath(file.webkitRelativePath||file.name);total+=file.size;entries+=1;paths.push(path);plans.push({file,zip:false,path});}
  if(total>BATCH_MAX_BYTES||entries>256)throw new Error('每批展开后最多32MiB、256个条目；本次未上传任何文件');
 }
 const current=await io.listFiles();preflightUploadPaths(paths,current.map(file=>file.path),directories,pendingDirectories);let uploaded=0;
 for(const plan of plans){try{if(plan.zip){const result=await io.uploadZip(plan.file);uploaded+=result.created.length;if(!result.complete)return{uploaded,failed:result.failed?.message??'ZIP部分文件未导入'};}else{await io.uploadFile(plan.path,plan.file);uploaded+=1;}}catch(error){return{uploaded,failed:error instanceof Error?error.message:'上传失败'};}}
 return{uploaded,failed:null};
}

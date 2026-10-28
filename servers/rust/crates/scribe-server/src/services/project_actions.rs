//! User-confirmed project operations. Model output is only data; it can never execute this service.
use std::{collections::HashSet,sync::Arc};
use chrono::{DateTime,Utc};
use scribe_shared::{ApiError,ApiResult,ErrorCode,FileId,FileType,ProjectId,UserId,CompilerEngine,infer_file_type};
use bytes::Bytes;
use scribe_storage::{SupabaseStorage,Storage,PROJECT_FILES_BUCKET,project_file_key};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sqlx::PgPool;
use uuid::Uuid;
use super::{files::FileService,membership::{assert_can_write,assert_member},versions::VersionService};
#[derive(Clone,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ActionInput{#[serde(rename="type")]pub kind:String,#[serde(default,skip_serializing_if="String::is_empty")]pub path:String,pub content:Option<String>,pub new_path:Option<String>,#[serde(default,skip_serializing_if="Option::is_none")]pub compiler:Option<CompilerEngine>,#[serde(default,skip_serializing_if="Option::is_none")]pub main_file:Option<String>,#[serde(default,skip_serializing_if="Option::is_none")]pub expected:Option<SettingsGuard>}
#[derive(Clone,Serialize,Deserialize)]#[serde(rename_all="camelCase")]
struct SourceGuard{id:Uuid,path:String,updated_at:DateTime<Utc>}
#[derive(Clone,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase")]
pub struct SettingsGuard{compiler:String,main_file:String}
#[derive(Clone,Serialize,Deserialize)]#[serde(rename_all="camelCase")]
struct Item{id:Uuid,action:ActionInput,source:Option<SourceGuard>,#[serde(default)]settings:Option<SettingsGuard>,status:String,result:Option<Value>}
#[derive(Deserialize)]#[serde(deny_unknown_fields)]pub struct PrepareInput{pub actions:Vec<ActionInput>}
#[derive(Clone)]pub struct ProjectActions{pool:PgPool,storage:Arc<SupabaseStorage>}
impl ProjectActions{
 pub fn new(pool:PgPool,storage:Arc<SupabaseStorage>)->Self{Self{pool,storage}}
 pub async fn directories(&self,user:UserId,project:ProjectId)->ApiResult<Value>{assert_member(&self.pool,user,project).await?;let paths:Vec<String>=sqlx::query_scalar("select path from public.scribe_project_directories where project_id=$1 order by path limit 2000").bind(project.into_inner()).fetch_all(&self.pool).await.map_err(db)?;Ok(json!({"directories":paths.into_iter().map(|path|json!({"path":path})).collect::<Vec<_>>()}))}
 pub async fn prepare(&self,user:UserId,project:ProjectId,input:PrepareInput)->ApiResult<Value>{
  assert_can_write(&self.pool,user,project).await?;validate_actions(&input.actions)?;
  let files=FileService::new(self.pool.clone(),self.storage.clone()).list(user,project).await?;let dirs=self.directory_paths(project).await?;let mut items=vec![];
  for action in input.actions{
   if action.kind=="set_compile_settings"{
    validate_main(action.main_file.as_deref().unwrap(),&files)?;
    if !scribe_compile::isolated::available().await{return Err(ApiError::new(ErrorCode::ServiceUnavailable,"所选编译器暂不可用，设置未保存"));}
    let settings=self.owner_settings(user,project).await?;
    let expected=action.expected.as_ref().ok_or_else(||ApiError::validation("缺少原编译设置，请重新生成建议"))?;
    if expected.compiler!=settings.compiler||expected.main_file!=settings.main_file{return Err(ApiError::new(ErrorCode::Conflict,"编译设置已被修改，请重新生成建议"));}
    items.push(Item{id:Uuid::new_v4(),action,source:None,settings:Some(settings),status:"pending".into(),result:None});continue;
   }
   let destination=action.new_path.as_deref().unwrap_or(&action.path);collision(destination,action.kind=="create_directory",&files.iter().map(|f|f.path.as_str()).collect::<Vec<_>>(),&dirs)?;
   let source=if action.kind=="move_file"{let file=files.iter().find(|file|file.path==action.path).ok_or_else(||ApiError::not_found("待移动的文件不存在，请刷新文件树"))?;Some(SourceGuard{id:file.id.into_inner(),path:file.path.clone(),updated_at:file.updated_at})}else{None};
   items.push(Item{id:Uuid::new_v4(),action,source,settings:None,status:"pending".into(),result:None});
  }
  let id=Uuid::new_v4();sqlx::query("insert into public.scribe_project_action_plans(id,user_id,project_id,items,expires_at) values($1,$2,$3,$4,now()+interval '30 minutes')").bind(id).bind(user.into_inner()).bind(project.into_inner()).bind(serde_json::to_value(&items).map_err(|_|ApiError::internal("无法记录文件计划"))?).execute(&self.pool).await.map_err(db)?;
  Ok(json!({"planId":id,"actions":items.into_iter().map(|item|public_item(&item)).collect::<Vec<_>>()}))
 }
 async fn directory_paths(&self,project:ProjectId)->ApiResult<Vec<String>>{sqlx::query_scalar("select path from public.scribe_project_directories where project_id=$1").bind(project.into_inner()).fetch_all(&self.pool).await.map_err(db)}
 async fn owner_settings(&self,user:UserId,project:ProjectId)->ApiResult<SettingsGuard>{
  let value:Option<(String,String)>=sqlx::query_as("select compiler::text,main_file from public.projects where id=$1 and owner_id=$2").bind(project.into_inner()).bind(user.into_inner()).fetch_optional(&self.pool).await.map_err(db)?;
  let (compiler,main_file)=value.ok_or_else(||ApiError::forbidden("仅项目所有者可更改编译设置"))?;Ok(SettingsGuard{compiler,main_file})
 }
 pub async fn decide(&self,user:UserId,project:ProjectId,plan:Uuid,item_id:Uuid,reject:bool)->ApiResult<Value>{
  // Capabilities are checked before any row locks; model output never executes an engine.
  if !reject{
   let raw:Option<Value>=sqlx::query_scalar("select items from public.scribe_project_action_plans where id=$1 and user_id=$2 and project_id=$3 and expires_at>now()").bind(plan).bind(user.into_inner()).bind(project.into_inner()).fetch_optional(&self.pool).await.map_err(db)?;
   if let Some(raw)=raw{if let Ok(items)=serde_json::from_value::<Vec<Item>>(raw){if items.iter().any(|item|item.id==item_id&&item.status=="pending"&&item.action.kind=="set_compile_settings")&&!scribe_compile::isolated::available().await{return Err(ApiError::new(ErrorCode::ServiceUnavailable,"所选编译器暂不可用，设置未保存"));}}}
  }
  assert_can_write(&self.pool,user,project).await?;let mut tx=self.pool.begin().await.map_err(db)?;
  let raw:Value=sqlx::query_scalar("select items from public.scribe_project_action_plans where id=$1 and user_id=$2 and project_id=$3 and expires_at>now() for update").bind(plan).bind(user.into_inner()).bind(project.into_inner()).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(||ApiError::not_found("文件计划已过期或不可访问，请重新生成"))?;
  let mut items:Vec<Item>=serde_json::from_value(raw).map_err(|_|ApiError::internal("文件计划数据无效"))?;let index=items.iter().position(|i|i.id==item_id).ok_or_else(||ApiError::not_found("文件计划项不存在"))?;
  if items[index].status!="pending"{return Ok(public_item(&items[index]));}
  if reject{items[index].status="rejected".into();}else{
   let action=items[index].action.clone();validate_actions(std::slice::from_ref(&action))?;
   // Serialize AI confirmations. Unique project/path constraints also reject concurrent ordinary creates.
   sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,4078242))").bind(format!("prims-actions:{project}")).execute(&mut *tx).await.map_err(db)?;
   assert_can_write(&self.pool,user,project).await?;
   let files=FileService::new(self.pool.clone(),self.storage.clone()).list(user,project).await?;let dirs=self.directory_paths(project).await?;
   if action.kind!="set_compile_settings"{collision(action.new_path.as_deref().unwrap_or(&action.path),action.kind=="create_directory",&files.iter().map(|f|f.path.as_str()).collect::<Vec<_>>(),&dirs)?;}
   if let Some(source)=&items[index].source{if !files.iter().any(|f|f.id.into_inner()==source.id&&f.path==source.path&&f.updated_at==source.updated_at){return Err(ApiError::new(ErrorCode::Conflict,"原文件已变化，请重新检查文件计划后确认"));}}
   let checkpoint=if action.kind=="set_compile_settings"{None}else{Some(VersionService::new(self.pool.clone(),self.storage.clone()).checkpoint(user,project).await?)};
   let result=match action.kind.as_str(){
    "set_compile_settings"=>{
     validate_main(action.main_file.as_deref().unwrap(),&files)?;
     let current:Option<(String,String)>=sqlx::query_as("select compiler::text,main_file from public.projects where id=$1 and owner_id=$2 for update").bind(project.into_inner()).bind(user.into_inner()).fetch_optional(&mut *tx).await.map_err(db)?;
     let (compiler,main_file)=current.ok_or_else(||ApiError::forbidden("仅项目所有者可更改编译设置"))?;
     let before=items[index].settings.as_ref().ok_or_else(||ApiError::new(ErrorCode::Conflict,"缺少原编译设置，请重新检查建议"))?;
     if compiler!=before.compiler||main_file!=before.main_file{return Err(ApiError::new(ErrorCode::Conflict,"编译设置已被修改，请重新检查建议后接受"));}
     let main:Option<Uuid>=sqlx::query_scalar("select id from public.project_files where project_id=$1 and path=$2 for share").bind(project.into_inner()).bind(action.main_file.as_ref().unwrap()).fetch_optional(&mut *tx).await.map_err(db)?;
     if main.is_none(){return Err(ApiError::new(ErrorCode::Conflict,"主文件已移动或删除，设置未保存"));}
     sqlx::query("update public.projects set compiler=$1,main_file=$2 where id=$3 and owner_id=$4").bind(action.compiler.unwrap().as_str()).bind(action.main_file.as_ref().unwrap()).bind(project.into_inner()).bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;
     json!({"type":action.kind,"compiler":action.compiler,"mainFile":action.main_file,"before":{"compiler":compiler,"mainFile":main_file}})
    },
    "create_directory"=>{sqlx::query("insert into public.scribe_project_directories(project_id,path,created_by) values($1,$2,$3)").bind(project.into_inner()).bind(&action.path).bind(user.into_inner()).execute(&mut *tx).await.map_err(conflict_db)?;json!({"type":action.kind,"path":action.path})},
    "create_file"=>{
     // The immutable plan item UUID is also the file UUID. Upload retries target the same object,
     // and file metadata plus applied status commit in one DB transaction (no ambiguous second file).
     let id=FileId::new(items[index].id);let storage_key=project_file_key(project,id);let content=action.content.as_deref().unwrap_or("");
     let kind=match infer_file_type(&action.path){FileType::Tex=>"tex",FileType::Bib=>"bib",FileType::Image=>"image",FileType::Other=>"other"};
     let inserted=sqlx::query("insert into public.project_files(id,project_id,path,type,storage_key,size_bytes,created_by) values($1,$2,$3,$4,$5,$6,$7)").bind(id.into_inner()).bind(project.into_inner()).bind(&action.path).bind(kind).bind(&storage_key).bind(content.len() as i64).bind(user.into_inner()).execute(&mut *tx).await;
     if let Err(error)=inserted{return Err(conflict_db(error));}
     if let Err(error)=self.storage.upload(PROJECT_FILES_BUCKET,&storage_key,Bytes::copy_from_slice(content.as_bytes()),"text/plain; charset=utf-8").await{let _=self.storage.remove(PROJECT_FILES_BUCKET,&[storage_key]).await;return Err(error);}
     json!({"type":action.kind,"path":action.path,"fileId":id})},
    "move_file"=>{let source=items[index].source.as_ref().ok_or_else(||ApiError::internal("缺少原文件校验信息"))?;
     let row=sqlx::query("update public.project_files set path=$1,updated_at=now() where project_id=$2 and id=$3 and path=$4 and updated_at=$5 returning id").bind(action.new_path.as_ref().unwrap()).bind(project.into_inner()).bind(source.id).bind(&source.path).bind(source.updated_at).fetch_optional(&mut *tx).await.map_err(conflict_db)?;
     if row.is_none(){return Err(ApiError::new(ErrorCode::Conflict,"移动前原文件已变化，未执行"));}sqlx::query("update public.projects set main_file=$1 where id=$2 and main_file=$3").bind(action.new_path.as_ref().unwrap()).bind(project.into_inner()).bind(&source.path).execute(&mut *tx).await.map_err(db)?;json!({"type":action.kind,"path":action.path,"newPath":action.new_path,"fileId":source.id})},_=>return Err(ApiError::validation("不支持的文件操作"))};
   items[index].status="applied".into();items[index].result=Some(json!({"operation":result,"textVersionId":checkpoint.map(|checkpoint|checkpoint.id),"note":if action.kind=="set_compile_settings"{"仅保存编译设置，不自动编译；原设置已记录在此计划"}else{"此快照保护项目文本；目录和二进制文件路径变化记录在本操作计划中"}}));
  }
  sqlx::query("update public.scribe_project_action_plans set items=$2 where id=$1").bind(plan).bind(serde_json::to_value(&items).map_err(|_|ApiError::internal("无法记录执行结果"))?).execute(&mut *tx).await.map_err(db)?;tx.commit().await.map_err(db)?;Ok(public_item(&items[index]))
 }
}
fn public_item(item:&Item)->Value{json!({"id":item.id,"action":item.action,"status":item.status,"result":item.result})}
pub fn validate_path(path:&str)->ApiResult<()>{if path.is_empty()||path.len()>512||path.starts_with('/')||path.contains('\\')||path.chars().any(|c|c.is_control()||c==':')||path.split('/').any(|p|p.is_empty()||p=="."||p=="..")||path.split('/').next()==Some(".git"){return Err(ApiError::validation("路径必须是项目内的安全相对路径，不能含上级目录、空段或控制字符"));}Ok(())}
fn validate_main(path:&str,files:&[scribe_shared::ProjectFile])->ApiResult<()>{validate_path(path)?;if !path.to_ascii_lowercase().ends_with(".tex")||!files.iter().any(|file|file.path==path){return Err(ApiError::validation("主文件必须是当前项目中已存在的.tex文件"));}Ok(())}
pub fn validate_actions(actions:&[ActionInput])->ApiResult<()>{
 if actions.is_empty(){return Err(ApiError::validation("项目操作不能为空"));}
 let mut total=0;let mut targets=HashSet::new();
 for a in actions{
  if a.kind=="set_compile_settings"{
   if !a.path.is_empty()||a.content.is_some()||a.new_path.is_some()||a.compiler.is_none()||a.compiler==Some(CompilerEngine::Tectonic)||a.main_file.is_none()||a.expected.is_none(){return Err(ApiError::validation("编译设置仅接受compiler和mainFile"));}
   validate_path(a.main_file.as_deref().unwrap())?;let expected=a.expected.as_ref().unwrap();validate_path(&expected.main_file)?;if !matches!(expected.compiler.as_str(),"tectonic"|"xelatex"|"lualatex"|"pdflatex"){return Err(ApiError::validation("原编译设置无效"));}
   if !a.main_file.as_deref().unwrap().to_ascii_lowercase().ends_with(".tex"){return Err(ApiError::validation("主文件必须是.tex文件"));}
   if !targets.insert("\0compile-settings"){return Err(ApiError::validation("每轮只能有一项编译设置建议"));}continue;
  }
  if a.compiler.is_some()||a.main_file.is_some()||a.expected.is_some(){return Err(ApiError::validation("文件操作不能更改编译设置"));}
  validate_path(&a.path)?;total+=a.path.chars().count()+a.new_path.as_deref().unwrap_or("").chars().count();
  match a.kind.as_str(){"create_file"=>{if a.new_path.is_some()||a.content.is_none()||a.content.as_ref().is_some_and(|c|c.len()>8*1024*1024){return Err(ApiError::validation("新建文本文件需要不超过 40000 字符的内容"));}total+=a.content.as_ref().unwrap().chars().count();},"create_directory"=>{if a.content.is_some()||a.new_path.is_some(){return Err(ApiError::validation("创建目录不能带文件内容或目标路径"));}},"move_file"=>{let new=a.new_path.as_deref().ok_or_else(||ApiError::validation("移动文件需要目标路径"))?;validate_path(new)?;if new==a.path||a.content.is_some(){return Err(ApiError::validation("移动目标需不同，且不能修改文件内容"));}},_=>return Err(ApiError::validation("不支持的项目操作"))}
  if !targets.insert(a.new_path.as_deref().unwrap_or(&a.path)){return Err(ApiError::validation("多个操作不能写向同一路径"));}
 }
 if total>8*1024*1024{return Err(ApiError::validation("本轮新文件文本总量不能超过 80000 字符"));}Ok(())
}
fn collision(path:&str,_directory:bool,files:&[&str],dirs:&[String])->ApiResult<()>{if files.iter().any(|f|*f==path||f.starts_with(&(path.to_string()+"/")))||dirs.iter().any(|d|d==path||d.starts_with(&(path.to_string()+"/")))||path.split('/').scan(String::new(),|base,part|{if !base.is_empty(){base.push('/');}base.push_str(part);Some(base.clone())}).any(|parent|files.contains(&parent.as_str())){return Err(ApiError::new(ErrorCode::Conflict,"目标路径或其父路径已存在文件/目录，未覆盖任何内容"));}Ok(())}
fn db(_:sqlx::Error)->ApiError{ApiError::internal("文件计划服务暂时不可用")}
fn conflict_db(_:sqlx::Error)->ApiError{ApiError::new(ErrorCode::Conflict,"目标已被占用或文件已变化，操作未确认成功，请刷新文件树")}
#[cfg(test)]mod tests{use super::*;#[test]fn path_and_action_limits_are_fail_closed(){for path in ["../x","/absolute","a/../b","a//b","C:/x","a\\b",".git/config"]{assert!(validate_path(path).is_err());}assert!(validate_path("sections/中文.tex").is_ok());let a=ActionInput{kind:"create_file".into(),path:"intro.tex".into(),content:Some("safe".into()),new_path:None,compiler:None,main_file:None,expected:None};assert!(validate_actions(std::slice::from_ref(&a)).is_ok());assert!(validate_actions(&[a.clone(),a]).is_err());assert!(collision("a/b.tex",false,&["a"],&[]).is_err());assert!(collision("a",false,&["a/b.tex"],&[]).is_err());}}

#[cfg(test)]mod settings_tests{
 use super::*;
 #[test]fn compile_proposals_are_typed_and_guarded(){
  let value=json!({"type":"set_compile_settings","compiler":"xelatex","mainFile":"章节/主文件.tex","expected":{"compiler":"tectonic","mainFile":"main.tex"}});
  let action:ActionInput=serde_json::from_value(value.clone()).unwrap();assert!(validate_actions(&[action.clone()]).is_ok());assert!(validate_actions(&[action.clone(),action]).is_err());
  for patch in [json!({"compiler":"shell"}),json!({"mainFile":"../main.tex"}),json!({"content":"overwrite"}),json!({"expected":null}),json!({"path":"main.tex"})]{let mut candidate=value.clone();for (key,value) in patch.as_object().unwrap(){candidate[key]=value.clone();}assert!(serde_json::from_value::<ActionInput>(candidate).map_or(true,|action|validate_actions(&[action]).is_err()));}
 }
 #[test]fn legacy_file_plans_remain_readable(){let item:Item=serde_json::from_value(json!({"id":Uuid::new_v4(),"action":{"type":"create_directory","path":"chapters","content":null,"newPath":null},"source":null,"status":"pending","result":null})).unwrap();assert!(item.settings.is_none());assert!(validate_actions(&[item.action]).is_ok());}
}

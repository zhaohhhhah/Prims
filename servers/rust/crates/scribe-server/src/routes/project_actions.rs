use axum::{extract::{Path,State},middleware::from_fn,routing::{get,post},Json,Router};
use scribe_auth::{require_auth,Authenticated};
use scribe_shared::{ApiError,ApiResult,ProjectId,VersionId};
use serde_json::{json,Value};
use uuid::Uuid;
use crate::{state::AppState,services::{project_actions::{ProjectActions,PrepareInput},membership::assert_owner,files::FileService,versions::VersionService}};
pub fn router()->Router<AppState>{Router::new().route("/api/projects/:project_id/directories",get(directories)).route("/api/projects/:project_id/ai/actions/prepare",post(prepare)).route("/api/projects/:project_id/ai/actions/:plan_id/:item_id/apply",post(apply)).route("/api/projects/:project_id/ai/actions/:plan_id/:item_id/reject",post(reject)).route("/api/projects/:project_id/versions/:version_id/restore-plan",get(restore_plan)).layer(from_fn(require_auth))}
fn service(s:&AppState)->ApiResult<ProjectActions>{Ok(ProjectActions::new(s.db().ok_or_else(||ApiError::internal("数据库未配置"))?.pool().clone(),s.inner.storage.clone().ok_or_else(||ApiError::internal("存储未配置"))?))}
async fn directories(State(s):State<AppState>,Authenticated(u):Authenticated,Path(p):Path<Uuid>)->ApiResult<Json<Value>>{Ok(Json(service(&s)?.directories(u.id,ProjectId::new(p)).await?))}
async fn prepare(State(s):State<AppState>,Authenticated(u):Authenticated,Path(p):Path<Uuid>,Json(input):Json<PrepareInput>)->ApiResult<Json<Value>>{Ok(Json(service(&s)?.prepare(u.id,ProjectId::new(p),input).await?))}
async fn apply(State(s):State<AppState>,Authenticated(u):Authenticated,Path((p,plan,item)):Path<(Uuid,Uuid,Uuid)>)->ApiResult<Json<Value>>{let result=service(&s)?.decide(u.id,ProjectId::new(p),plan,item,false).await?;let cache=s.response_cache();let list=format!("files:list:{p}:");let content=format!("files:content:{p}:");tokio::join!(cache.invalidate_prefix(&list),cache.invalidate_prefix(&content));Ok(Json(result))}
async fn reject(State(s):State<AppState>,Authenticated(u):Authenticated,Path((p,plan,item)):Path<(Uuid,Uuid,Uuid)>)->ApiResult<Json<Value>>{Ok(Json(service(&s)?.decide(u.id,ProjectId::new(p),plan,item,true).await?))}
async fn restore_plan(State(s):State<AppState>,Authenticated(u):Authenticated,Path((p,v)):Path<(Uuid,Uuid)>)->ApiResult<Json<Value>>{
 let pool=s.db().ok_or_else(||ApiError::internal("数据库未配置"))?.pool().clone();let storage=s.inner.storage.clone().ok_or_else(||ApiError::internal("存储未配置"))?;let project=ProjectId::new(p);assert_owner(&pool,u.id,project).await?;
 let history=VersionService::new(pool.clone(),storage.clone()).get_payload(u.id,project,VersionId::new(v)).await?;let files=FileService::new(pool,storage);let current=files.list(u.id,project).await?;let mut edits=vec![];let mut missing=vec![];let mut unsupported=vec![];let mut total=0;
 for old in history.files{let Some(file)=current.iter().find(|file|file.path==old.path)else{missing.push(old.path);continue};if edits.len()>=12||old.content.chars().count()>40000{unsupported.push(old.path);continue;}let before=files.read_content(u.id,project,file.id).await?;let size=before.chars().count()+old.content.chars().count();if size>80000||total+size>80000{unsupported.push(old.path);continue;}if before==old.content{continue;}total+=size;edits.push(json!({"fileId":file.id,"path":old.path,"before":before,"after":old.content}));}
 Ok(Json(json!({"versionId":v,"historyKind":"text","edits":edits,"missingPaths":missing,"unsupportedPaths":unsupported,"note":"此预览仅恢复仍存在的文本文件，不恢复目录或二进制资产。必须在当前编辑器中逐项确认；尚未写入任何文件。"})))
}

//! Authenticated private writing context; project membership is required for every operation.
use axum::{extract::{Path,State},middleware::from_fn,routing::{get,post},Json,Router};
use scribe_auth::{require_auth,Authenticated};
use scribe_shared::{ApiError,ApiResult,ErrorCode,ProjectId};
use uuid::Uuid;
use crate::{state::AppState,services::ai_context::{AIContextService,ContextState,BriefInput,AppendInput,ThreadList,SelectThreadInput}};

pub fn router()->Router<AppState>{Router::new()
 .route("/api/projects/:project_id/ai/context",get(load).put(brief))
 .route("/api/projects/:project_id/ai/context/messages",post(append))
 .route("/api/projects/:project_id/ai/context/new",post(new_thread))
 .route("/api/projects/:project_id/ai/context/threads",get(threads))
 .route("/api/projects/:project_id/ai/context/select",post(select_thread))
 .layer(from_fn(require_auth))}
fn service(state:&AppState)->ApiResult<AIContextService>{let pool=state.db().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"database unavailable"))?.pool().clone();Ok(AIContextService::new(pool))}
async fn load(State(state):State<AppState>,Authenticated(user):Authenticated,Path(project):Path<Uuid>)->ApiResult<Json<ContextState>>{Ok(Json(service(&state)?.get(user.id,ProjectId::new(project)).await?))}
async fn brief(State(state):State<AppState>,Authenticated(user):Authenticated,Path(project):Path<Uuid>,Json(input):Json<BriefInput>)->ApiResult<Json<ContextState>>{Ok(Json(service(&state)?.update_brief(user.id,ProjectId::new(project),input).await?))}
async fn append(State(state):State<AppState>,Authenticated(user):Authenticated,Path(project):Path<Uuid>,Json(input):Json<AppendInput>)->ApiResult<Json<ContextState>>{Ok(Json(service(&state)?.append(user.id,ProjectId::new(project),input).await?))}
async fn new_thread(State(state):State<AppState>,Authenticated(user):Authenticated,Path(project):Path<Uuid>)->ApiResult<Json<ContextState>>{Ok(Json(service(&state)?.new_thread(user.id,ProjectId::new(project)).await?))}

async fn threads(State(state):State<AppState>,Authenticated(user):Authenticated,Path(project):Path<Uuid>)->ApiResult<Json<ThreadList>>{Ok(Json(service(&state)?.threads(user.id,ProjectId::new(project)).await?))}
async fn select_thread(State(state):State<AppState>,Authenticated(user):Authenticated,Path(project):Path<Uuid>,Json(input):Json<SelectThreadInput>)->ApiResult<Json<ContextState>>{Ok(Json(service(&state)?.select_thread(user.id,ProjectId::new(project),input).await?))}

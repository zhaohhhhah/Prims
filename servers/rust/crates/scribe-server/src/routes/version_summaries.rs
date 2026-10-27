use axum::{extract::{Path,State},middleware::from_fn,routing::{get,post},Json,Router};
use scribe_auth::{require_auth,Authenticated};
use scribe_shared::{ApiError,ApiResult,ProjectId,VersionId};
use serde_json::Value;
use uuid::Uuid;
use crate::{state::AppState,services::version_summaries::{VersionSummaries,Preference,PreferenceInput,GenerateInput}};
pub fn router()->Router<AppState>{Router::new().route("/api/version-summary/preferences",get(preference).put(set_preference)).route("/api/projects/:project_id/version-summaries",get(list)).route("/api/projects/:project_id/versions/:version_id/summary",post(generate)).layer(from_fn(require_auth))}
fn service(state:&AppState)->ApiResult<VersionSummaries>{Ok(VersionSummaries::new(state.db().ok_or_else(||ApiError::internal("数据库未配置"))?.pool().clone(),state.inner.storage.clone().ok_or_else(||ApiError::internal("存储未配置"))?,state.inner.ai_crypto.clone()))}
async fn preference(State(s):State<AppState>,Authenticated(u):Authenticated)->ApiResult<Json<Preference>>{Ok(Json(service(&s)?.preference(u.id).await?))}
async fn set_preference(State(s):State<AppState>,Authenticated(u):Authenticated,Json(input):Json<PreferenceInput>)->ApiResult<Json<Preference>>{Ok(Json(service(&s)?.set_preference(u.id,input).await?))}
async fn list(State(s):State<AppState>,Authenticated(u):Authenticated,Path(p):Path<Uuid>)->ApiResult<Json<Value>>{Ok(Json(service(&s)?.list(u.id,ProjectId::new(p)).await?))}
async fn generate(State(s):State<AppState>,Authenticated(u):Authenticated,Path((p,v)):Path<(Uuid,Uuid)>,body:Option<Json<GenerateInput>>)->ApiResult<Json<Value>>{Ok(Json(service(&s)?.generate(u.id,ProjectId::new(p),VersionId::new(v),body.map(|b|b.0).unwrap_or_default()).await?))}

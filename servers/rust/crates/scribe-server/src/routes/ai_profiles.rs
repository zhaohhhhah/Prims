//! Named model configuration APIs; sessions never grant access to another user's profile.
use axum::{extract::{Path,State},http::StatusCode,middleware::from_fn,routing::{get,put},Json,Router};
use scribe_auth::{require_auth,Authenticated};
use scribe_shared::{ApiError,ApiResult,ErrorCode};
use uuid::Uuid;
use crate::{state::AppState,services::ai_profiles::{AIProfiles,ProfileInput,ProfilePublic,ProfileList,SetDefaultInput}};
pub fn router()->Router<AppState>{Router::new().route("/api/ai/profiles",get(list).post(create)).route("/api/ai/profiles/default",put(set_default)).route("/api/ai/profiles/:id/context-window",put(context_window)).route("/api/ai/profiles/:id",put(update).delete(delete)).layer(from_fn(require_auth))}
fn service(state:&AppState)->ApiResult<AIProfiles>{let pool=state.db().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"database unavailable"))?.pool().clone();let crypto=state.inner.ai_crypto.clone().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"AI unavailable"))?;Ok(AIProfiles::new(pool,crypto))}
async fn list(State(state):State<AppState>,Authenticated(user):Authenticated)->ApiResult<Json<ProfileList>>{Ok(Json(service(&state)?.list(user.id).await?))}
async fn create(State(state):State<AppState>,Authenticated(user):Authenticated,Json(input):Json<ProfileInput>)->ApiResult<(StatusCode,Json<ProfilePublic>)>{Ok((StatusCode::CREATED,Json(service(&state)?.create(user.id,input).await?)))}
async fn update(State(state):State<AppState>,Authenticated(user):Authenticated,Path(id):Path<Uuid>,Json(input):Json<ProfileInput>)->ApiResult<Json<ProfilePublic>>{Ok(Json(service(&state)?.update(user.id,id,input).await?))}
async fn delete(State(state):State<AppState>,Authenticated(user):Authenticated,Path(id):Path<Uuid>)->ApiResult<StatusCode>{service(&state)?.delete(user.id,id).await?;Ok(StatusCode::NO_CONTENT)}

async fn set_default(State(state):State<AppState>,Authenticated(user):Authenticated,Json(input):Json<SetDefaultInput>)->ApiResult<Json<ProfileList>>{Ok(Json(service(&state)?.set_default(user.id,input).await?))}

#[derive(serde::Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]struct ContextWindowInput{model_name:String,context_window_tokens:u32}
async fn context_window(State(state):State<AppState>,Authenticated(user):Authenticated,Path(id):Path<Uuid>,Json(input):JsonExtractContext)->ApiResult<Json<ProfilePublic>>{Ok(Json(service(&state)?.update_context_window(user.id,id,&input.model_name,input.context_window_tokens).await?))}
type JsonExtractContext=Json<ContextWindowInput>;

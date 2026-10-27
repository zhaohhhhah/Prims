//! Authenticated settings draft endpoints. No automatic library writes.
use axum::{extract::{DefaultBodyLimit,State},middleware::from_fn,routing::post,Json,Router};
use scribe_auth::{require_auth,Authenticated};
use scribe_shared::ApiResult;
use serde_json::Value;
use crate::{state::AppState,services::skill_import::{self,PreviewInput,RefineInput}};
pub fn router()->Router<AppState>{Router::new().route("/api/agent/skill-import/preview",post(preview)).route("/api/agent/skill-import/refine",post(refine)).layer(DefaultBodyLimit::max(160000)).layer(from_fn(require_auth))}
async fn preview(Authenticated(_user):Authenticated,Json(input):Json<PreviewInput>)->ApiResult<Json<Value>>{Ok(Json(skill_import::preview(input).await?))}
async fn refine(State(state):State<AppState>,Authenticated(user):Authenticated,Json(input):Json<RefineInput>)->ApiResult<Json<Value>>{Ok(Json(skill_import::refine(&state,user.id,input).await?))}

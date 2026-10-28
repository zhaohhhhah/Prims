//! /api/ai routes — config CRUD, ping, complete (SSE).
//!
//! Mirrors `server/src/routes/ai.ts`. All routes require auth. The
//! `/complete` endpoint streams server-sent events; the others are
//! plain JSON.

use axum::extract::{Json as JsonExtract, State};
use axum::middleware::from_fn;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use scribe_auth::{require_auth, Authenticated};
use scribe_shared::{
    AICompleteInput, AIConfigPublic, AIPingResult, ApiError, ApiResult, ErrorCode,
    UpdateAIConfigInput,
};
use serde::Serialize;
use std::convert::Infallible;

use crate::services::ai::AIService;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ai/usage", get(usage))
        .route("/api/ai/config", get(get_config).put(update_config))
        .route("/api/ai/ping", post(ping))
        .route("/api/ai/complete", post(complete))
        .merge(super::ai_context::router())
        .merge(super::ai_profiles::router())
        .layer(from_fn(require_auth))
}

#[derive(Serialize)]
struct GetConfigBody {
    config: Option<AIConfigPublic>,
}

async fn get_config(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
) -> ApiResult<Json<GetConfigBody>> {
    let svc = service(&state)?;
    Ok(Json(GetConfigBody { config: svc.get_config(user.id).await? }))
}

async fn update_config(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    JsonExtract(input): JsonExtract<UpdateAIConfigInput>,
) -> ApiResult<Json<AIConfigPublic>> {
    let svc = service(&state)?;
    Ok(Json(svc.update_config(user.id, input).await?))
}

async fn ping(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
) -> ApiResult<Json<AIPingResult>> {
    let svc = service(&state)?;
    Ok(Json(svc.ping(user.id).await?))
}

/// SSE completion stream. Each AI token chunk is emitted as a single
/// SSE message; on EOS or error we emit a `done` event and close.
async fn complete(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    JsonExtract(input): JsonExtract<AICompleteInput>,
) -> axum::response::Response {
    if crate::services::inline_prompt::is_inline(&input) && input.options.as_ref().is_some_and(|o|o.contains_key("projectId")) {
        return match crate::services::project_agent::start(state,user.id,input).await {Ok(response)=>response,Err(error)=>error.into_response()};
    }
    let svc = match service(&state) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };
    let stream = match svc.complete_stream(user.id, input).await {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };

    // Map each adapter chunk → SSE Event. Errors become a final
    // `event: error` frame so the client can surface them in-line.
    let sse_stream = stream.map(|result| -> Result<Event, Infallible> {
        match result {
            Ok(chunk) => Ok(Event::default().data(serde_json::json!({"text":chunk.text}).to_string())),
            Err(err) => Ok(Event::default().event("error").data(serde_json::json!({"error":err.to_string()}).to_string())),
        }
    }).chain(futures::stream::once(async { Ok::<_, Infallible>(Event::default().data("[DONE]")) }));

    Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

fn service(state: &AppState) -> ApiResult<AIService> {
    let pool = state
        .db()
        .ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "database not configured"))?
        .pool()
        .clone();
    let crypto = state
        .inner
        .ai_crypto
        .clone()
        .ok_or_else(|| ApiError::new(
            ErrorCode::ServiceUnavailable,
            "AI features disabled: AI_KEY_ENCRYPTION_KEY not configured on server",
        ))?;
    Ok(AIService::new(pool, crypto))
}

#[derive(serde::Deserialize)]#[serde(rename_all="camelCase")]
struct UsageQuery{#[serde(default="usage_days")]days:u32,#[serde(default="usage_zone")]time_zone:String,#[serde(default="usage_resolution")]resolution:String}
fn usage_resolution()->String{"standard".into()}
fn usage_days()->u32{7}fn usage_zone()->String{"UTC".into()}
async fn usage(State(state):State<AppState>,Authenticated(user):Authenticated,axum::extract::Query(query):axum::extract::Query<UsageQuery>)->ApiResult<Json<serde_json::Value>>{
 let pool=state.db().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"database unavailable"))?.pool();
 Ok(Json(crate::services::ai_usage::summary(pool,user.id,query.days,&query.time_zone,&query.resolution).await?))
}

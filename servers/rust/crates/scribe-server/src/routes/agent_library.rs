//! Private user-owned agent library and immutable capability boundaries.
use crate::{
    services::agent_library::{AgentLibrary, AgentPreferences, MemoryInput, SkillInput},
    state::AppState,
};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    middleware::from_fn,
    routing::get,
    Json, Router,
};
use scribe_auth::{require_auth, Authenticated};
use scribe_shared::{ApiError, ApiResult, ErrorCode, ProjectId};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/agent/skills", get(list).post(create))
        .route(
            "/api/agent/skills/:id",
            get(read).put(update).delete(archive),
        )
        .route(
            "/api/agent/preferences",
            get(preferences).put(set_preferences),
        )
        .route(
            "/api/projects/:project_id/agent/memory",
            get(memory).put(set_memory),
        )
        .route("/api/agent/capabilities", get(capabilities))
        .route(
            "/api/projects/:project_id/agent/check",
            axum::routing::post(check),
        )
        .layer(DefaultBodyLimit::max(160000))
        .layer(from_fn(require_auth))
}
fn service(state: &AppState) -> ApiResult<AgentLibrary> {
    Ok(AgentLibrary::new(
        state
            .db()
            .ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "database unavailable"))?
            .pool()
            .clone(),
    ))
}
async fn list(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
) -> ApiResult<Json<Value>> {
    Ok(Json(service(&state)?.list(user.id, false).await?))
}
async fn read(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(service(&state)?.read(user.id, id, false).await?))
}
async fn create(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Json(input): Json<SkillInput>,
) -> ApiResult<Json<Value>> {
    Ok(Json(service(&state)?.save(user.id, None, input).await?))
}
async fn update(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(id): Path<Uuid>,
    Json(input): Json<SkillInput>,
) -> ApiResult<Json<Value>> {
    Ok(Json(service(&state)?.save(user.id, Some(id), input).await?))
}
async fn archive(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    service(&state)?.archive(user.id, id).await?;
    Ok(Json(json!({"ok":true})))
}
async fn preferences(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
) -> ApiResult<Json<AgentPreferences>> {
    Ok(Json(service(&state)?.preferences(user.id).await?))
}
async fn set_preferences(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Json(input): Json<AgentPreferences>,
) -> ApiResult<Json<AgentPreferences>> {
    Ok(Json(
        service(&state)?.set_preferences(user.id, input).await?,
    ))
}
async fn memory(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<super::super::services::agent_library::ProjectMemory>> {
    Ok(Json(
        service(&state)?.memory(user.id, ProjectId::new(id)).await?,
    ))
}
async fn set_memory(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(id): Path<Uuid>,
    Json(input): Json<MemoryInput>,
) -> ApiResult<Json<super::super::services::agent_library::ProjectMemory>> {
    Ok(Json(
        service(&state)?
            .save_memory(user.id, ProjectId::new(id), input)
            .await?,
    ))
}
async fn capabilities(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
) -> ApiResult<Json<Value>> {
    let prefs = service(&state)?.preferences(user.id).await?;
    let configured = crate::services::agent_workbench::configured();
    let worker = if configured {
        crate::services::agent_workbench::health().await.ok()
    } else {
        None
    };
    let available = worker
        .as_ref()
        .is_some_and(|value| value.get("ok") == Some(&Value::Bool(true)));
    let search = if let (Some(db), Some(crypto)) = (state.db(), state.inner.ai_crypto.clone()) {
        crate::services::ai::AIService::new(db.pool().clone(), crypto)
            .official_search_available(user.id)
            .await
            .unwrap_or(false)
    } else {
        false
    };
    Ok(Json(
        json!({"limits":{"modelRounds":32,"requestSeconds":600,"readCalls":12,"diagnosticCalls":3,"skills":32,"skillBytes":49152,"memoryCharacters":4000,"pendingProposals":48,"shellAllowed":false,"networkFromWorkbench":false,"automaticProjectWrites":false},"skills":{"available":true},"memory":{"available":true},"diagnostics":{"configured":configured,"available":available&&prefs.diagnostics_enabled,"ready":available&&prefs.diagnostics_enabled,"health":{"ready":available,"status":if available{"ready"}else if configured{"unavailable"}else{"not_configured"}},"tasks":["latex_check","font_check","list_fonts"],"reason":if !configured{"worker_not_configured"}else if !available{"worker_unavailable"}else if !prefs.diagnostics_enabled{"disabled"}else{"ready"}},"search":{"available":search&&prefs.search_enabled,"provider":"deepseek","scope":"official_deepseek_connection_only","reason":if !prefs.search_enabled{"disabled"}else if search{"ready"}else{"default_connection_is_not_official_deepseek_or_missing_key"}},"preferences":prefs}),
    ))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckInput {
    task: String,
    path: Option<String>,
}
async fn check(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(id): Path<Uuid>,
    Json(input): Json<CheckInput>,
) -> ApiResult<Json<Value>> {
    let mut call = json!({"tool":"run_project_check","task":input.task});
    if let Some(path) = input.path {
        call["path"] = json!(path);
    }
    crate::services::agent_library::validate_tool(&call)?;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        crate::services::agent_workbench::run(&state, user.id, ProjectId::new(id), &call),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            ErrorCode::ServiceUnavailable,
            "项目诊断超时；取消请求已发出，工作容器另有硬超时保护",
        )
    })??;
    Ok(Json(result))
}

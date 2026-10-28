//! /api/projects/:projectId/files — list/create/rename/remove + content I/O.
//!
//! Two paths for create:
//!   * `application/json` → `{ path, type?, content? }` body. Used by the
//!     in-app "New file" flow and the ZIP-unpack on the client.
//!   * `multipart/form-data` → field "path" + a binary file part. Used
//!     by user uploads from their machine.

use axum::{
    extract::{DefaultBodyLimit, FromRequest, Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    middleware::from_fn,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use scribe_auth::{require_auth, Authenticated};
use scribe_shared::{
     ApiError, ApiResult, CreateFileInput, ErrorCode, FileContentInput, FileId,
    FileType, ProjectFile, ProjectId, RenameFileInput, UserId,
};
use serde::Serialize; // used by ContentBody, WriteContentResponse, UrlBody
use serde_json::json;
use uuid::Uuid;

use crate::services::FileService;
use crate::state::AppState;

/// Cache key for a project's file list. **Project-first** key layout so
/// `invalidate_prefix("files:list:{project}:")` wipes every user's
/// cached view at once when any file in the project is created /
/// renamed / removed. Per-user suffix prevents a no-access user from
/// reading a member's cached value.
fn files_list_cache_key(project: ProjectId, user: UserId) -> String {
    format!("files:list:{project}:{user}")
}
fn files_list_invalidate_prefix(project: ProjectId) -> String {
    format!("files:list:{project}:")
}

/// Cache key for a single file's content. **File-first** key layout so
/// `invalidate_prefix("files:content:{project}:{file}:")` clears every
/// user's copy when the file's content changes.
fn file_content_cache_key(project: ProjectId, file: FileId, user: UserId) -> String {
    format!("files:content:{project}:{file}:{user}")
}
fn file_content_invalidate_prefix(project: ProjectId, file: FileId) -> String {
    format!("files:content:{project}:{file}:")
}
fn file_content_project_prefix(project: ProjectId) -> String {
    // Wipes every file under a project — used by create/rename/remove
    // where any cached list view is now stale.
    format!("files:content:{project}:")
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/projects/:project_id/files",
            get(list).post(create_dispatch),
        )
        .route(
            "/api/projects/:project_id/files/:file_id",
            axum::routing::patch(rename).delete(remove),
        )
        .route(
            "/api/projects/:project_id/files/:file_id/content",
            get(read_content).put(write_content).layer(DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .route(
            "/api/projects/:project_id/files/:file_id/download-url",
            get(download_url),
        )
        .layer(DefaultBodyLimit::max(crate::services::upload_limits::UPLOAD_BODY_BYTES))
        .layer(from_fn(require_auth))
}

async fn list(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let cache = state.response_cache().clone();
    let key = files_list_cache_key(ProjectId::new(project_id), user.id);
    let service = match files_service(&state) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };
    cache
        .cached_json::<Vec<ProjectFile>, _, _>(&key, &headers, None, move || async move {
            service
                .list(user.id, ProjectId::new(project_id))
                .await
                .map_err(IntoResponse::into_response)
        })
        .await
}

/// Dispatch on Content-Type. We accept JSON (for the in-app flow) and
/// multipart (for uploads from the user's machine) on the same path.
async fn create_dispatch(
    state: State<AppState>,
    auth: Authenticated,
    path: Path<Uuid>,
    req: axum::extract::Request,
) -> Response {
    let content_type = req
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.starts_with("application/json") {
        // Parse the body manually so we can keep using the same handler shape.
        let (parts, body) = req.into_parts();
        let bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
            Ok(b) => b,
            Err(err) => return bad_request(&format!("read body: {err}")),
        };
        let input: CreateFileInput = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(err) => return bad_request(&format!("invalid json: {err}")),
        };
        let _ = parts; // Not used; satisfies the destructure.
        match create_from_json(state, auth, path, input).await {
            Ok(json) => json.into_response(),
            Err(err) => api_error_response(err),
        }
    } else if content_type.starts_with("multipart/form-data") {
        let _permit=match crate::services::upload_limits::acquire(){Ok(p)=>p,Err(e)=>return api_error_response(e)};
        let multipart = match Multipart::from_request(req, &state.0.clone()).await {
            Ok(m) => m,
            Err(err) => return bad_request(&err.to_string()),
        };
        match create_from_multipart(state, auth, path, multipart).await {
            Ok(json) => json.into_response(),
            Err(err) => api_error_response(err),
        }
    } else {
        bad_request("expected application/json or multipart/form-data")
    }
}

async fn create_from_json(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(project_id): Path<Uuid>,
    input: CreateFileInput,
) -> ApiResult<Json<ProjectFile>> {
    let service = files_service(&state)?;
    let created = service.create(user.id, ProjectId::new(project_id), input).await?;
    invalidate_list_and_all_content(&state, ProjectId::new(project_id)).await;
    Ok(Json(created))
}

async fn create_from_multipart(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(project_id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Json<ProjectFile>> {
    let db=state.db().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"database unavailable"))?;
    crate::services::membership::assert_can_write(db.pool(),user.id,ProjectId::new(project_id)).await?;
    let mut requested_path: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::new(ErrorCode::BadRequest, format!("multipart: {err}")))?
    {
        match field.name() {
            Some("path") => {
                let mut path=Vec::new();
                while let Some(chunk)=field.chunk().await.map_err(|_|ApiError::validation("文件路径读取失败"))?{
                    if path.len()+chunk.len()>4096{return Err(ApiError::validation("文件路径过长"));}path.extend_from_slice(&chunk);
                }
                requested_path=Some(String::from_utf8(path).map_err(|_|ApiError::validation("文件路径必须是UTF8"))?);
            }
            Some("file") => {
                let filename = field.file_name().map(|s| s.to_string());
                if requested_path.is_none() {
                    requested_path = filename;
                }
                if bytes.is_some(){return Err(ApiError::validation("每次上传只接受一个文件"));}
                let max=crate::services::upload_limits::PROJECT_QUOTA_BYTES;
                let mut data=Vec::with_capacity(max);
                while let Some(chunk)=field.chunk().await.map_err(|_|ApiError::validation("文件上传中断"))?{
                    if data.len().saturating_add(chunk.len())>max{return Err(ApiError::validation("单文件最多500MiB，且不能超过项目剩余容量"));}
                    data.extend_from_slice(&chunk);
                }
                bytes=Some(data);
            }
            _ => {} // ignore unknown fields
        }
    }

    let path = requested_path
        .ok_or_else(|| ApiError::new(ErrorCode::ValidationFailed, "missing 'path' or 'file'"))?;
    let bytes = bytes
        .ok_or_else(|| ApiError::new(ErrorCode::ValidationFailed, "missing 'file' part"))?;

    let path=crate::services::archive_import::safe_path(&path)?;
    let input=CreateFileInput{path,file_type:None,content:None};
    let service=files_service(&state)?;
    let created=service.create_uploaded(user.id,ProjectId::new(project_id),input,bytes).await?;
    invalidate_list_and_all_content(&state, ProjectId::new(project_id)).await;
    Ok(Json(created))
}

async fn rename(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path((project_id, file_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<RenameFileInput>,
) -> ApiResult<Json<ProjectFile>> {
    let service = files_service(&state)?;
    let updated = service
        .rename(user.id, ProjectId::new(project_id), FileId::new(file_id), input)
        .await?;
    invalidate_list_and_file(&state, ProjectId::new(project_id), FileId::new(file_id)).await;
    Ok(Json(updated))
}

async fn remove(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path((project_id, file_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    let service = files_service(&state)?;
    service
        .remove(user.id, ProjectId::new(project_id), FileId::new(file_id))
        .await?;
    invalidate_list_and_file(&state, ProjectId::new(project_id), FileId::new(file_id)).await;
    Ok(Json(json!({ "ok": true })))
}

/// Invalidate the project's file list (any user) + every cached file
/// content under it. Called from create/rename/remove because those
/// change the list shape and may invalidate previously-cached paths.
async fn invalidate_list_and_all_content(state: &AppState, project: ProjectId) {
    let cache = state.response_cache();
    let list = files_list_invalidate_prefix(project);
    let content = file_content_project_prefix(project);
    tokio::join!(cache.invalidate_prefix(&list), cache.invalidate_prefix(&content));
}

/// Narrower: only invalidates one file's content + the project list.
/// Used by writes that target a single file body.
async fn invalidate_list_and_file(state: &AppState, project: ProjectId, file: FileId) {
    let cache = state.response_cache();
    let list = files_list_invalidate_prefix(project);
    let content = file_content_invalidate_prefix(project, file);
    tokio::join!(cache.invalidate_prefix(&list), cache.invalidate_prefix(&content));
}

#[derive(Serialize)]
struct ContentBody {
    content: String,
}

async fn read_content(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path((project_id, file_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Response {
    let cache = state.response_cache().clone();
    let key = file_content_cache_key(ProjectId::new(project_id), FileId::new(file_id), user.id);
    let service = match files_service(&state) {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };
    cache
        .cached_json::<ContentBody, _, _>(&key, &headers, None, move || async move {
            let content = service
                .read_content(user.id, ProjectId::new(project_id), FileId::new(file_id))
                .await
                .map_err(IntoResponse::into_response)?;
            Ok::<_, Response>(ContentBody { content })
        })
        .await
}

#[derive(Serialize)]
struct WriteContentResponse {
    path: String,
    size_bytes: i64,
    updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename="versionWarning",skip_serializing_if="Option::is_none")]
    version_warning: Option<String>,
}

async fn write_content(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path((project_id, file_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<FileContentInput>,
) -> ApiResult<Json<WriteContentResponse>> {
    let service = files_service(&state)?;
    let file = service
        .write_content(
            user.id,
            ProjectId::new(project_id),
            FileId::new(file_id),
            input.content,
        )
        .await?;
    // Content changed → every user's cached copy of this file is stale.
    // List shape didn't change (still the same file), but size_bytes did,
    // so the list response body would also be stale.
    invalidate_list_and_file(&state, ProjectId::new(project_id), FileId::new(file_id)).await;
    // Saving succeeded. Optional history failure must not look like a failed write or trigger blind retries.
    let version_warning=match (state.db(),state.inner.storage.clone()){
        (Some(db),Some(storage))=>crate::services::versions::VersionService::new(db.pool().clone(),storage).automatic_snapshot(user.id,ProjectId::new(project_id)).await.err().map(|_|"文件已保存，但自动文本版本暂时未能创建；请稍后手动创建快照".to_string()),
        _=>Some("文件已保存，但版本服务暂时不可用".to_string()),
    };
    Ok(Json(WriteContentResponse {
        path: file.path,
        size_bytes: file.size_bytes,
        updated_at: file.updated_at,
        version_warning,
    }))
}

#[derive(Serialize)]
struct UrlBody {
    url: String,
}

async fn download_url(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path((project_id, file_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<UrlBody>> {
    let service = files_service(&state)?;
    let url = service
        .signed_download_url(user.id, ProjectId::new(project_id), FileId::new(file_id))
        .await?;
    Ok(Json(UrlBody { url }))
}

fn files_service(state: &AppState) -> ApiResult<FileService> {
    let pool = state
        .db()
        .ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "database not configured"))?
        .pool()
        .clone();
    let storage = state
        .inner
        .storage
        .clone()
        .ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "storage not configured"))?;
    Ok(FileService::new(pool, storage))
}

// -- response helpers ---------------------------------------------------

fn bad_request(msg: &str) -> Response {
    let body = json!({ "code": "bad_request", "message": msg });
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

fn api_error_response(err: ApiError) -> Response {
    let status = StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = json!({ "code": err.code, "message": err.message });
    (status, Json(body)).into_response()
}

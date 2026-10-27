//! Authenticated ZIP inspection/import. Archive bytes never become filesystem paths.
use crate::{
    services::{
        archive_import::{self, ParsedArchive, ZipManifest, MAX_ZIP_BYTES},
        membership::assert_can_write,
        FileService, ProjectService,
    },
    state::AppState,
};
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    middleware::from_fn,
    routing::post,
    Json, Router,
};
use scribe_auth::{require_auth, Authenticated};
use scribe_shared::{
    ApiError, ApiResult, CompilerEngine, CreateFileInput, CreateProjectInput, ErrorCode, Project,
    ProjectFile, ProjectId, UpdateProjectInput, UserId,
};
use scribe_storage::{project_file_key, Storage, PROJECT_FILES_BUCKET};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;
static IMPORTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/projects/import-zip/inspect", post(inspect))
        .route("/api/projects/import-zip", post(create_project))
        .route("/api/projects/:project_id/import-zip", post(upload_project))
        .layer(DefaultBodyLimit::max(MAX_ZIP_BYTES + 16384))
        .layer(from_fn(require_auth))
}
struct Upload {
    archive: ParsedArchive,
    fields: HashMap<String, String>,
    _permit: OwnedSemaphorePermit,
}
async fn parse_upload(mut multipart: Multipart) -> ApiResult<Upload> {
    let permit = IMPORTS.clone().try_acquire_owned().map_err(|_| {
        ApiError::new(
            ErrorCode::ServiceUnavailable,
            "已有ZIP导入正在处理，请稍后重试",
        )
    })?;
    let mut bytes = None;
    let mut fields = HashMap::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::validation("上传内容不完整"))?
    {
        let name = field.name().unwrap_or("").to_string();
        let is_file = name == "file";
        if is_file && bytes.is_some() {
            return Err(ApiError::validation("每次ZIP请求只接受一个压缩包"));
        }
        if !is_file
            && !matches!(
                name.as_str(),
                "name" | "description" | "compiler" | "mainFile"
            )
        {
            return Err(ApiError::validation("ZIP请求含未知字段"));
        }
        let limit = if is_file { MAX_ZIP_BYTES } else { 4096 };
        let mut data = Vec::new();
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| ApiError::validation("上传数据中断"))?
        {
            if data.len() + chunk.len() > limit {
                return Err(ApiError::validation("ZIP最多20MiB，项目信息最多4096字节"));
            }
            data.extend_from_slice(&chunk);
        }
        if is_file {
            bytes = Some(data);
        } else {
            if fields.contains_key(&name) {
                return Err(ApiError::validation("ZIP请求含重复字段"));
            }
            fields.insert(
                name,
                String::from_utf8(data)
                    .map_err(|_| ApiError::validation("项目信息必须是UTF-8文本"))?,
            );
        }
    }
    let bytes = bytes.ok_or_else(|| ApiError::validation("请选择ZIP压缩包"))?;
    let (archive, permit) = tokio::task::spawn_blocking(move || {
        archive_import::parse_zip(bytes).map(|archive| (archive, permit))
    })
    .await
    .map_err(|_| ApiError::new(ErrorCode::Internal, "ZIP检查失败"))??;
    Ok(Upload {
        archive,
        fields,
        _permit: permit,
    })
}
async fn inspect(
    Authenticated(_): Authenticated,
    multipart: Multipart,
) -> ApiResult<Json<ZipManifest>> {
    let upload = parse_upload(multipart).await?;
    Ok(Json(upload.archive.manifest))
}
fn pool(state: &AppState) -> ApiResult<sqlx::PgPool> {
    Ok(state
        .db()
        .ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "database unavailable"))?
        .pool()
        .clone())
}
fn services(state: &AppState) -> ApiResult<(FileService, ProjectService)> {
    let pool = pool(state)?;
    let storage = state
        .inner
        .storage
        .clone()
        .ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "storage unavailable"))?;
    Ok((
        FileService::new(pool.clone(), storage.clone()),
        ProjectService::new(pool, storage),
    ))
}
async fn invalidate(state: &AppState, user: UserId, project: Option<ProjectId>) {
    state
        .response_cache()
        .invalidate(&format!("projects:list:{user}"))
        .await;
    if let Some(project) = project {
        state
            .response_cache()
            .invalidate_prefix(&format!("files:list:{project}:"))
            .await;
        state
            .response_cache()
            .invalidate_prefix(&format!("files:content:{project}:"))
            .await;
    }
}
async fn add_directories(
    state: &AppState,
    user: UserId,
    project: ProjectId,
    dirs: &[String],
) -> ApiResult<()> {
    for path in dirs {
        sqlx::query("insert into public.scribe_project_directories(project_id,path,created_by) values($1,$2,$3) on conflict(project_id,path) do nothing").bind(project.into_inner()).bind(path).bind(user.into_inner()).execute(&pool(state)?).await.map_err(|_|ApiError::new(ErrorCode::Internal,"创建ZIP目录失败"))?;
    }
    Ok(())
}
async fn create_project(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    multipart: Multipart,
) -> ApiResult<Json<Project>> {
    let upload = parse_upload(multipart).await?;
    let main = archive_import::choose_main(
        &upload.archive,
        upload.fields.get("mainFile").map(String::as_str),
    )?;
    let name = upload
        .fields
        .get("name")
        .map(|name| name.trim())
        .filter(|name| !name.is_empty() && name.chars().count() <= 120)
        .ok_or_else(|| ApiError::validation("项目名称需要1–120个字符"))?;
    let description = upload.fields.get("description").cloned();
    if description
        .as_ref()
        .is_some_and(|value| value.chars().count() > 2000)
    {
        return Err(ApiError::validation("项目描述最多2000字"));
    }
    let compiler: CompilerEngine = serde_json::from_value(json!(upload
        .fields
        .get("compiler")
        .map(String::as_str)
        .unwrap_or("tectonic")))
    .map_err(|_| ApiError::validation("编译器无效"))?;
    let input: CreateProjectInput = serde_json::from_value(
        json!({"name":name,"description":description,"template":"blank","compiler":compiler}),
    )
    .map_err(|_| ApiError::validation("项目信息无效"))?;
    // The validated import owns its short-lived task so an HTTP disconnect/30s outer timeout
    // cannot interrupt the failure-cleanup path halfway through creating a new project.
    tokio::spawn(async move { create_validated_project(state, user.id, upload, input, main).await })
        .await
        .map_err(|_| ApiError::new(ErrorCode::Internal, "ZIP导入任务异常，请检查项目列表"))?
        .map(Json)
}
async fn create_validated_project(
    state: AppState,
    user: UserId,
    upload: Upload,
    input: CreateProjectInput,
    main: String,
) -> ApiResult<Project> {
    let (files, projects) = services(&state)?;
    let project = projects.create_empty_for_import(user, &input).await?;
    let mut created = vec![];
    let result = async {
        for file in upload.archive.files {
            let row = files
                .create_uploaded(
                    user,
                    project.id,
                    CreateFileInput {
                        path: file.path,
                        file_type: None,
                        content: None,
                    },
                    file.bytes,
                )
                .await?;
            created.push(row);
        }
        add_directories(
            &state,
            user,
            project.id,
            &upload.archive.manifest.directories,
        )
        .await?;
        projects
            .update(
                user,
                project.id,
                UpdateProjectInput {
                    main_file: Some(main),
                    ..Default::default()
                },
            )
            .await
    }
    .await;
    if result.is_err() {
        // Only this operation's newly-created UUID and objects are eligible for rollback.
        let keys = created
            .iter()
            .map(|file: &ProjectFile| project_file_key(project.id, file.id))
            .collect::<Vec<_>>();
        let removed = projects.remove(user, project.id).await;
        if let Some(storage) = state.inner.storage.as_ref() {
            if !keys.is_empty() {
                if storage.remove(PROJECT_FILES_BUCKET, &keys).await.is_err() {
                    tracing::warn!(project=%project.id,"ZIP import failed; temporary storage cleanup needs retry");
                }
            }
        }
        if removed.is_err() {
            tracing::warn!(project=%project.id,"ZIP import failed; new project rollback needs retry");
        }
    }
    invalidate(&state, user, Some(project.id)).await;
    result
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ZipUploadResult {
    created: Vec<ProjectFile>,
    complete: bool,
    failed: Option<Value>,
    skipped_metadata: usize,
}
async fn upload_project(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(id): Path<Uuid>,
    multipart: Multipart,
) -> ApiResult<Json<ZipUploadResult>> {
    let project = ProjectId::new(id);
    let pool = pool(&state)?;
    assert_can_write(&pool, user.id, project).await?;
    let upload = parse_upload(multipart).await?;
    let (files, _) = services(&state)?;
    let existing = files.list(user.id, project).await?;
    if existing.len() + upload.archive.files.len() > 2000 {
        return Err(ApiError::validation("导入后文件数超过2000，请拆分项目"));
    }
    let dirs: Vec<String> = sqlx::query_scalar(
        "select path from public.scribe_project_directories where project_id=$1",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .map_err(|_| ApiError::new(ErrorCode::Internal, "读取项目目录失败"))?;
    preflight_conflicts(
        &upload.archive,
        &existing
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>(),
        &dirs,
    )?;
    let mut result = ZipUploadResult {
        created: vec![],
        complete: true,
        failed: None,
        skipped_metadata: upload.archive.manifest.skipped_metadata,
    };
    for file in upload.archive.files {
        let path = file.path.clone();
        match files
            .create_uploaded(
                user.id,
                project,
                CreateFileInput {
                    path: file.path,
                    file_type: None,
                    content: None,
                },
                file.bytes,
            )
            .await
        {
            Ok(file) => result.created.push(file),
            Err(error) => {
                result.complete = false;
                result.failed = Some(json!({"path":path,"message":error.message}));
                break;
            }
        }
    }
    if result.complete {
        if let Err(error) = add_directories(
            &state,
            user.id,
            project,
            &upload.archive.manifest.directories,
        )
        .await
        {
            result.complete = false;
            result.failed = Some(json!({"message":error.message}));
        }
    }
    // Existing projects are never rolled back or deleted. Successful new files stay visible on partial failure.
    invalidate(&state, user.id, Some(project)).await;
    Ok(Json(result))
}
fn preflight_conflicts(
    archive: &ParsedArchive,
    files: &[String],
    dirs: &[String],
) -> ApiResult<()> {
    for incoming in &archive.files {
        if files.iter().any(|old| {
            old == &incoming.path
                || old.starts_with(&(incoming.path.clone() + "/"))
                || incoming.path.starts_with(&(old.clone() + "/"))
        }) || dirs
            .iter()
            .any(|dir| dir == &incoming.path || dir.starts_with(&(incoming.path.clone() + "/")))
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!(
                    "路径已存在或与目录冲突：{}。请先改名；本次未导入任何文件。",
                    incoming.path
                ),
            ));
        }
    }
    for incoming in &archive.manifest.directories {
        if files
            .iter()
            .any(|old| old == incoming || incoming.starts_with(&(old.clone() + "/")))
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "ZIP目录与已有文件冲突，本次未导入任何文件",
            ));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_paths_never_get_silent_timestamp_renames() {
        let archive = ParsedArchive {
            files: vec![archive_import::ArchiveFile {
                path: "main.tex".into(),
                bytes: vec![],
            }],
            manifest: ZipManifest {
                files: vec![],
                directories: vec![],
                main_candidates: vec![],
                suggested_main_file: None,
                total_bytes: 0,
                skipped_metadata: 0,
            },
        };
        assert!(preflight_conflicts(&archive, &["main.tex".into()], &[]).is_err());
        assert!(preflight_conflicts(&archive, &[], &["main.tex".into()]).is_err());
        assert!(preflight_conflicts(&archive, &["other.tex".into()], &[]).is_ok());
    }
}

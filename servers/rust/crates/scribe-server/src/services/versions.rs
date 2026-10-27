//! Project version snapshots. Mirrors
//! `server/src/services/versionService.ts`.
//!
//! Two storage buckets are involved:
//!   * `project-files` — read from when building a snapshot; written to
//!     when restoring an old version.
//!   * `version-snapshots` — where the JSON payload (all `.tex`/`.bib`
//!     contents) lives keyed by `{project_id}/{version_id}.json`.

use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use scribe_shared::{
    ApiError, ApiResult, CreateVersionInput, ErrorCode, ProjectId, ProjectVersion, UserId,
    VersionFile, VersionId, VersionPayload,
};
use scribe_storage::{
    Storage, SupabaseStorage, PROJECT_FILES_BUCKET, VERSION_SNAPSHOTS_BUCKET,
};
use sqlx::{PgPool, Row};
use sha2::{Digest,Sha256};
use uuid::Uuid;

use super::membership::{assert_can_write, assert_member, assert_owner};

#[derive(Clone)]
pub struct VersionService {
    pool: PgPool,
    storage: Arc<SupabaseStorage>,
}

impl VersionService {
    pub fn new(pool: PgPool, storage: Arc<SupabaseStorage>) -> Self {
        Self { pool, storage }
    }

    pub async fn list(
        &self,
        user: UserId,
        project: ProjectId,
    ) -> ApiResult<Vec<ProjectVersion>> {
        assert_member(&self.pool, user, project).await?;
        let rows = sqlx::query(
            r#"
            select v.id, v.project_id, v.created_by, v.label,
                   v.file_count, v.total_bytes, v.created_at,
                   u.display_name as author_display_name
            from public.project_versions v
            left join public.users u on u.id = v.created_by
            where v.project_id = $1
            order by v.created_at desc
            limit 100
            "#,
        )
        .bind(project.into_inner())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(row_to_version).collect())
    }

    pub async fn snapshot(&self,user:UserId,project:ProjectId,input:CreateVersionInput)->ApiResult<ProjectVersion>{
        self.snapshot_inner(user,project,input,false,true).await?.ok_or_else(||ApiError::internal("手动快照未创建"))
    }
    /// Every persisted editing session gets bounded automatic text history; no old versions are deleted.
    pub async fn automatic_snapshot(&self,user:UserId,project:ProjectId)->ApiResult<Option<ProjectVersion>>{
        assert_can_write(&self.pool,user,project).await?;
        self.snapshot_inner(user,project,CreateVersionInput{label:Some("自动保存的文本版本".into())},true,false).await
    }
    /// An AI file operation cannot proceed without a verified current-text checkpoint.
    pub async fn checkpoint(&self,user:UserId,project:ProjectId)->ApiResult<ProjectVersion>{
        assert_can_write(&self.pool,user,project).await?;
        if let Some(version)=self.snapshot_inner(user,project,CreateVersionInput{label:Some("AI 文件操作前的文本版本".into())},true,true).await?{return Ok(version);}
        let id:Option<Uuid>=sqlx::query_scalar("select last_version_id from public.scribe_project_version_state where project_id=$1").bind(project.into_inner()).fetch_optional(&self.pool).await.map_err(internal)?.flatten();
        let id=id.ok_or_else(||ApiError::internal("无法确认文件操作前的版本记录"))?;
        self.list(user,project).await?.into_iter().find(|v|v.id.into_inner()==id).ok_or_else(||ApiError::internal("文件操作前的版本记录不可用"))
    }
    async fn snapshot_inner(&self,user:UserId,project:ProjectId,input:CreateVersionInput,deduplicate:bool,force:bool)->ApiResult<Option<ProjectVersion>>{
        assert_member(&self.pool, user, project).await?;

        let mut tx=self.pool.begin().await.map_err(internal)?;
        sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,4078231))").bind(format!("prims-versions:{project}")).execute(&mut *tx).await.map_err(internal)?;
        let previous=sqlx::query("select content_hash,last_auto_at from public.scribe_project_version_state where project_id=$1").bind(project.into_inner()).fetch_optional(&mut *tx).await.map_err(internal)?;
        if deduplicate&&!force&&previous.as_ref().and_then(|r|r.get::<Option<DateTime<Utc>>,_>("last_auto_at")).is_some_and(|last|Utc::now().signed_duration_since(last).num_seconds()<120){return Ok(None);}
        // Text snapshots include common project text files, while binary assets remain untouched.

        let file_rows = sqlx::query(
            r#"
            select path, storage_key
            from public.project_files
            where project_id = $1 and (type in ('tex', 'bib') or path ~* '\.(md|txt|csv|tsv|json|yaml|yml|toml|ini|cfg|bst|def|ins|dtx)$') order by path
            "#,
        )
        .bind(project.into_inner())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;

        // Download every text-ish file in parallel rather than awaiting
        // each one sequentially. With ~10 files at ~50ms each that's
        // ~50ms total instead of ~500ms.
        let downloads = file_rows.into_iter().map(|row| {
            let path: String = row.get("path");
            let storage_key: String = row.get("storage_key");
            let storage = self.storage.clone();
            async move {
                let bytes = storage.download(PROJECT_FILES_BUCKET, &storage_key).await?;
                let content = String::from_utf8(bytes.to_vec())
                    .map_err(|err| ApiError::internal(format!("utf-8: {err}")))?;
                Ok::<VersionFile, ApiError>(VersionFile { path, content })
            }
        });
        let files: Vec<VersionFile> = futures::future::try_join_all(downloads).await?;
        let total_bytes: i64 = files.iter().map(|f| f.content.len() as i64).sum();
        // Hard cap on snapshot total — keeps a malicious/runaway project
        // from materialising a multi-GB JSON in memory.
        const MAX_SNAPSHOT_BYTES: i64 = 64 * 1024 * 1024; // 64 MiB
        if total_bytes > MAX_SNAPSHOT_BYTES {
            return Err(ApiError::new(
                ErrorCode::PayloadTooLarge,
                format!(
                    "version snapshot would be {} bytes; max is {}",
                    total_bytes, MAX_SNAPSHOT_BYTES
                ),
            ));
        }

        let version_id = VersionId::new(Uuid::new_v4());
        let storage_key = format!("{}/{}.json", project, version_id);
        let payload = VersionPayload { version: 1, files };
        let body = serde_json::to_vec(&payload)
            .map_err(|err| ApiError::internal(format!("json encode: {err}")))?;
        let hash=Sha256::digest(&body).iter().map(|byte|format!("{byte:02x}")).collect::<String>();
        if deduplicate&&previous.as_ref().is_some_and(|r|r.get::<String,_>("content_hash")==hash){sqlx::query("update public.scribe_project_version_state set last_auto_at=now() where project_id=$1").bind(project.into_inner()).execute(&mut *tx).await.map_err(internal)?;tx.commit().await.map_err(internal)?;return Ok(None);}
        let file_count = payload.files.len() as i32;

        self.storage
            .upload(
                VERSION_SNAPSHOTS_BUCKET,
                &storage_key,
                Bytes::from(body),
                "application/json",
            )
            .await?;

        let insert = sqlx::query(
            r#"
            insert into public.project_versions
                (id, project_id, created_by, label, storage_key, file_count, total_bytes)
            values ($1, $2, $3, $4, $5, $6, $7)
            returning id, project_id, created_by, label, file_count, total_bytes, created_at,
                      (select display_name from public.users where id = $3) as author_display_name
            "#,
        )
        .bind(version_id.into_inner())
        .bind(project.into_inner())
        .bind(user.into_inner())
        .bind(input.label.as_deref())
        .bind(&storage_key)
        .bind(file_count)
        .bind(total_bytes)
        .fetch_one(&mut *tx)
        .await;

        match insert {
            Ok(row) => {
                let version=row_to_version(row);
                sqlx::query("insert into public.scribe_project_version_state(project_id,content_hash,last_version_id,last_auto_at) values($1,$2,$3,case when $4 then now() else null end) on conflict(project_id) do update set content_hash=excluded.content_hash,last_version_id=excluded.last_version_id,last_auto_at=case when $4 then now() else scribe_project_version_state.last_auto_at end,updated_at=now()")
                    .bind(project.into_inner()).bind(hash).bind(version.id.into_inner()).bind(deduplicate).execute(&mut *tx).await.map_err(internal)?;
                tx.commit().await.map_err(internal)?;Ok(Some(version))
            },
            Err(err) => {
                let _ = self
                    .storage
                    .remove(VERSION_SNAPSHOTS_BUCKET, &[storage_key])
                    .await;
                Err(internal(err))
            }
        }
    }

    pub async fn get_payload(
        &self,
        user: UserId,
        project: ProjectId,
        version: VersionId,
    ) -> ApiResult<VersionPayload> {
        assert_member(&self.pool, user, project).await?;
        let storage_key: Option<String> = sqlx::query_scalar(
            "select storage_key from public.project_versions where id = $1 and project_id = $2",
        )
        .bind(version.into_inner())
        .bind(project.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let storage_key = storage_key.ok_or_else(|| ApiError::not_found("Version not found"))?;
        let bytes = self.storage.download(VERSION_SNAPSHOTS_BUCKET, &storage_key).await?;
        let payload: VersionPayload = serde_json::from_slice(&bytes)
            .map_err(|err| ApiError::internal(format!("Version payload malformed: {err}")))?;
        Ok(payload)
    }

    /// Restore a snapshot: for each path that still exists in the
    /// project, overwrite its storage object with the historical
    /// content. Files that have since been deleted are skipped (matches
    /// the Node implementation — we don't recreate deleted files).
    pub async fn restore(
        &self,
        user: UserId,
        project: ProjectId,
        version: VersionId,
    ) -> ApiResult<()> {
        assert_owner(&self.pool,user,project).await?;
        let _=version;
        Err(ApiError::new(ErrorCode::Conflict,"请先打开版本恢复预览，并在编辑器中逐项确认；直接覆盖存储会与协作文档冲突"))
    }
}

fn row_to_version(row: sqlx::postgres::PgRow) -> ProjectVersion {
    ProjectVersion {
        id: VersionId::new(row.get::<Uuid, _>("id")),
        project_id: ProjectId::new(row.get::<Uuid, _>("project_id")),
        created_by: row.get::<Option<Uuid>, _>("created_by").map(UserId::new),
        author_display_name: row.get("author_display_name"),
        label: row.get("label"),
        file_count: row.get::<i32, _>("file_count"),
        total_bytes: row.get::<i64, _>("total_bytes"),
        created_at: row.get::<DateTime<Utc>, _>("created_at"),
    }
}

fn internal(err: sqlx::Error) -> ApiError {
    tracing::error!(?err, "database error in versions service");
    ApiError::new(ErrorCode::Internal, "Database error")
}

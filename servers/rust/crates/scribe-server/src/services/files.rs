//! File CRUD + storage. Mirrors `server/src/services/fileService.ts`.
//!
//! All paths take a `UserId` so membership can be enforced inline — we
//! either join through `project_members` or filter on `projects.owner_id`,
//! never trusting a raw `project_id` argument.

use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use scribe_shared::{
    infer_file_type, ApiError, ApiResult, CreateFileInput, ErrorCode, FileId, FileType,
    ProjectFile, ProjectId, RenameFileInput, UserId,
};
use scribe_storage::{project_file_key, Storage, SupabaseStorage, PROJECT_FILES_BUCKET};
use sqlx::{PgPool, Row};
use tracing::debug;
use uuid::Uuid;

use super::membership::{assert_can_write, assert_member};

#[derive(Clone)]
pub struct FileService {
    pool: PgPool,
    storage: Arc<SupabaseStorage>,
}

impl FileService {
    pub fn new(pool: PgPool, storage: Arc<SupabaseStorage>) -> Self {
        Self { pool, storage }
    }

    #[tracing::instrument(skip(self), fields(%user, %project))]
    pub async fn list(&self, user: UserId, project: ProjectId) -> ApiResult<Vec<ProjectFile>> {
        debug!("file list start");
        assert_member(&self.pool, user, project).await?;
        let rows = sqlx::query(
            r#"
            select id, project_id, path, type, storage_key, size_bytes,
                   created_by, created_at, updated_at
            from public.project_files
            where project_id = $1
            order by path
            "#,
        )
        .bind(project.into_inner())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(row_to_file).collect())
    }

    #[tracing::instrument(skip(self, input), fields(%user, %project, path = %input.path))]
    pub async fn create(
        &self,
        user: UserId,
        project: ProjectId,
        input: CreateFileInput,
    ) -> ApiResult<ProjectFile> {
        let bytes=Bytes::from(input.content.clone().unwrap_or_default().into_bytes());
        self.create_bytes(user,project,input,bytes,"text/plain; charset=utf-8").await
    }

    /// Preserve uploaded binary bytes exactly; text creation keeps its original API.
    pub async fn create_uploaded(&self,user:UserId,project:ProjectId,input:CreateFileInput,bytes:Vec<u8>)->ApiResult<ProjectFile>{
        let ext=input.path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let mime=match ext.as_str(){"png"=>"image/png","jpg"|"jpeg"=>"image/jpeg","webp"=>"image/webp","gif"=>"image/gif","pdf"=>"application/pdf",_=>"application/octet-stream"};
        self.create_bytes(user,project,input,Bytes::from(bytes),mime).await
    }

    async fn create_bytes(&self,user:UserId,project:ProjectId,input:CreateFileInput,bytes:Bytes,content_type:&str)->ApiResult<ProjectFile>{
        assert_can_write(&self.pool,user,project).await?;
        let id=FileId::new(Uuid::new_v4());let key=project_file_key(project,id);
        let file_type=input.file_type.unwrap_or_else(||infer_file_type(&input.path));

        let mut tx=self.pool.begin().await.map_err(internal)?;
        // Serialize quota reservations and metadata before touching Storage.
        // The DB trigger updates the authoritative project counter atomically.
        let exists:Option<Uuid>=sqlx::query_scalar("select id from public.projects where id=$1 for update").bind(project.into_inner()).fetch_optional(&mut *tx).await.map_err(internal)?;
        if exists.is_none(){return Err(ApiError::not_found("Project not found"));}

        let insert = sqlx::query(
            r#"
            insert into public.project_files (id, project_id, path, type, storage_key, size_bytes, created_by)
            values ($1, $2, $3, $4, $5, $6, $7)
            returning id, project_id, path, type, storage_key, size_bytes,
                      created_by, created_at, updated_at
            "#,
        )
        .bind(id.into_inner())
        .bind(project.into_inner())
        .bind(&input.path)
        .bind(file_type_as_str(file_type))
        .bind(&key)
        .bind(bytes.len() as i64)
        .bind(user.into_inner())
        .fetch_one(&mut *tx).await.map_err(map_insert_err)?;
        if let Err(error)=self.storage.upload(PROJECT_FILES_BUCKET,&key,bytes,content_type).await {
            let _=self.storage.remove(PROJECT_FILES_BUCKET,&[key]).await;
            return Err(error);
        }
        if let Err(error)=tx.commit().await {
            let _=self.storage.remove(PROJECT_FILES_BUCKET,&[key]).await;
            return Err(map_insert_err(error));
        }
        Ok(row_to_file(insert))
    }

    #[tracing::instrument(skip(self, input), fields(%user, %project, %file, new_path = %input.new_path))]
    pub async fn rename(
        &self,
        user: UserId,
        project: ProjectId,
        file: FileId,
        input: RenameFileInput,
    ) -> ApiResult<ProjectFile> {
        debug!("file rename start");
        assert_can_write(&self.pool, user, project).await?;
        let row = sqlx::query(
            r#"
            update public.project_files
            set path = $1
            where project_id = $2 and id = $3
            returning id, project_id, path, type, storage_key, size_bytes,
                      created_by, created_at, updated_at
            "#,
        )
        .bind(&input.new_path)
        .bind(project.into_inner())
        .bind(file.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_insert_err)?;
        row.map(row_to_file).ok_or_else(|| ApiError::not_found("File not found"))
    }

    #[tracing::instrument(skip(self), fields(%user, %project, %file))]
    pub async fn remove(
        &self,
        user: UserId,
        project: ProjectId,
        file: FileId,
    ) -> ApiResult<()> {
        debug!("file remove start");
        assert_can_write(&self.pool, user, project).await?;
        let key: Option<String> = sqlx::query_scalar(
            r#"
            delete from public.project_files
            where project_id = $1 and id = $2
            returning storage_key
            "#,
        )
        .bind(project.into_inner())
        .bind(file.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let key = key.ok_or_else(|| ApiError::not_found("File not found"))?;
        let _ = self.storage.remove(PROJECT_FILES_BUCKET, &[key]).await;
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(%user, %project, %file))]
    pub async fn read_content(
        &self,
        user: UserId,
        project: ProjectId,
        file: FileId,
    ) -> ApiResult<String> {
        debug!("file read start");
        assert_member(&self.pool, user, project).await?;
        let key = self.fetch_storage_key(project, file).await?;
        let bytes = self.storage.download(PROJECT_FILES_BUCKET, &key).await?;
        debug!(bytes = bytes.len(), "file read storage download complete");
        // Files in this bucket are almost always text (.tex/.bib/etc).
        // Binary files (images, PDFs) can occasionally land here via
        // the multipart upload path; rather than 500ing on invalid
        // UTF-8, decode lossily — matches the Node server's
        // `await blob.text()` behavior.
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tracing::instrument(skip(self, content), fields(%user, %project, %file, content_bytes = content.len()))]
    pub async fn write_content(
        &self,
        user: UserId,
        project: ProjectId,
        file: FileId,
        content: String,
    ) -> ApiResult<ProjectFile> {
        debug!("file write start");
        assert_can_write(&self.pool, user, project).await?;
        let mut tx=self.pool.begin().await.map_err(internal)?;
        sqlx::query("select id from public.projects where id=$1 for update").bind(project.into_inner()).fetch_optional(&mut *tx).await.map_err(internal)?.ok_or_else(||ApiError::not_found("Project not found"))?;
        let old:String=sqlx::query_scalar("select storage_key from public.project_files where project_id=$1 and id=$2").bind(project.into_inner()).bind(file.into_inner()).fetch_optional(&mut *tx).await.map_err(internal)?.ok_or_else(||ApiError::not_found("File not found"))?;
        let bytes=Bytes::from(content.into_bytes());
        // Stage a fresh object: a failed quota/DB operation can never overwrite
        // the previously committed file bytes.
        let key=format!("{}/{}",project,Uuid::new_v4());
        let row=sqlx::query("update public.project_files set size_bytes=$3,storage_key=$4,updated_at=now() where project_id=$1 and id=$2 returning id,project_id,path,type,storage_key,size_bytes,created_by,created_at,updated_at")
            .bind(project.into_inner()).bind(file.into_inner()).bind(bytes.len() as i64).bind(&key).fetch_one(&mut *tx).await.map_err(map_insert_err)?;
        if let Err(error)=self.storage.upload(PROJECT_FILES_BUCKET,&key,bytes,"text/plain; charset=utf-8").await {
            let _=self.storage.remove(PROJECT_FILES_BUCKET,&[key]).await;return Err(error);
        }
        if let Err(error)=tx.commit().await {
            let _=self.storage.remove(PROJECT_FILES_BUCKET,&[key]).await;return Err(map_insert_err(error));
        }
        let _=self.storage.remove(PROJECT_FILES_BUCKET,&[old]).await;
        Ok(row_to_file(row))
    }

    pub async fn signed_download_url(
        &self,
        user: UserId,
        project: ProjectId,
        file: FileId,
    ) -> ApiResult<String> {
        assert_member(&self.pool, user, project).await?;
        let key = self.fetch_storage_key(project, file).await?;
        self.storage.signed_url(PROJECT_FILES_BUCKET, &key, 60).await
    }

    pub async fn read_reference_bytes(&self,user:UserId,project:ProjectId,file:FileId,max:usize)->ApiResult<Bytes>{
        assert_member(&self.pool,user,project).await?;
        let size:Option<i64>=sqlx::query_scalar("select size_bytes from public.project_files where project_id=$1 and id=$2").bind(project.into_inner()).bind(file.into_inner()).fetch_optional(&self.pool).await.map_err(internal)?;
        let size=size.ok_or_else(||ApiError::not_found("File not found"))?;
        if size<0||size as usize>max{return Err(ApiError::validation("文件超过读取限制"));}
        let key=self.fetch_storage_key(project,file).await?;
        let bytes=self.storage.download(PROJECT_FILES_BUCKET,&key).await?;
        if bytes.len()>max{return Err(ApiError::validation("文件超过读取限制"));}Ok(bytes)
    }

    async fn fetch_storage_key(
        &self,
        project: ProjectId,
        file: FileId,
    ) -> ApiResult<String> {
        let key: Option<String> = sqlx::query_scalar(
            "select storage_key from public.project_files where project_id = $1 and id = $2",
        )
        .bind(project.into_inner())
        .bind(file.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        key.ok_or_else(|| ApiError::not_found("File not found"))
    }
}

fn file_type_as_str(t: FileType) -> &'static str {
    match t {
        FileType::Tex => "tex",
        FileType::Bib => "bib",
        FileType::Image => "image",
        FileType::Other => "other",
    }
}

fn parse_file_type(s: &str) -> FileType {
    match s {
        "tex" => FileType::Tex,
        "bib" => FileType::Bib,
        "image" => FileType::Image,
        _ => FileType::Other,
    }
}

fn row_to_file(row: sqlx::postgres::PgRow) -> ProjectFile {
    ProjectFile {
        id: FileId::new(row.get::<Uuid, _>("id")),
        project_id: ProjectId::new(row.get::<Uuid, _>("project_id")),
        path: row.get("path"),
        file_type: parse_file_type(row.get::<&str, _>("type")),
        size_bytes: row.get::<i64, _>("size_bytes"),
        created_by: row.get::<Option<Uuid>, _>("created_by").map(UserId::new),
        created_at: row.get::<DateTime<Utc>, _>("created_at"),
        updated_at: row.get::<DateTime<Utc>, _>("updated_at"),
    }
}

fn internal(err: sqlx::Error) -> ApiError {
    // sqlx Display can leak schema details and (rarely) connection
    // info into the user-facing string. Log the full error
    // server-side via tracing and hand the client a generic message.
    tracing::error!(?err, "database error in files service");
    ApiError::new(ErrorCode::Internal, "Database error")
}

/// Map Postgres unique-violation (23505) to a 409 so the client can
/// distinguish "duplicate path" from a generic failure.
fn map_insert_err(err: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(ref db_err) = err {
        if db_err.code().as_deref() == Some("P5001") { return ApiError::new(ErrorCode::PayloadTooLarge,"项目源文件和附件合计最多500MiB，请先删除不需要的文件"); }
        if db_err.code().as_deref() == Some("23505") {
            return ApiError::new(ErrorCode::Conflict, "A file already exists at that path");
        }
    }
    internal(err)
}
#[cfg(test)]mod quota_tests;

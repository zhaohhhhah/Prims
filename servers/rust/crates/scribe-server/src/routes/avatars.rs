//! Small private profile photos. Never expose the project bucket or accept storage paths.
use axum::{body::Bytes, extract::{DefaultBodyLimit, Path, State}, http::{header, StatusCode}, middleware::from_fn, response::IntoResponse, routing::{get, post}, Json, Router};
use scribe_auth::{require_auth, Authenticated};
use scribe_shared::{ApiError, ApiResult, ErrorCode};
use scribe_storage::{Storage, PROJECT_FILES_BUCKET};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;
use crate::state::AppState;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const PREFIX: &str = "prims-avatar:";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/profile/avatar", post(upload).delete(reset))
        .route("/api/users/:user_id/avatar", get(read))
        .layer(DefaultBodyLimit::max(MAX_BYTES))
        .layer(from_fn(require_auth))
}

fn owned_key(marker: &str, owner: Uuid) -> Option<String> {
    let filename = marker.strip_prefix(PREFIX)?.strip_suffix(".png")?;
    let id = Uuid::parse_str(filename).ok()?;
    // Canonicalize instead of using any user-supplied path.
    Some(format!("avatars/{owner}/{id}.png"))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1)); }
    }
    !crc
}

fn validate_png(bytes: &[u8]) -> ApiResult<()> {
    if bytes.len() > MAX_BYTES || bytes.len() < 45 || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
        || bytes[8..12] != 13u32.to_be_bytes() || &bytes[12..16] != b"IHDR" {
        return Err(ApiError::validation("Avatar must be a PNG image up to 2 MiB"));
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 || width > 512 || height > 512 || bytes[24] != 8
        || !matches!(bytes[25], 2 | 6) || bytes[26..29] != [0, 0, 0] {
        return Err(ApiError::validation("Avatar must be an 8-bit PNG up to 512 × 512"));
    }
    let mut pos = 8usize;
    let mut compressed = Vec::new();
    let mut end = false;
    while pos + 12 <= bytes.len() {
        let n = u32::from_be_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
        let next = pos.checked_add(n).and_then(|v| v.checked_add(12)).filter(|v| *v <= bytes.len())
            .ok_or_else(|| ApiError::validation("Invalid PNG chunk"))?;
        let kind = &bytes[pos+4..pos+8];
        let checksum = u32::from_be_bytes(bytes[pos+8+n..next].try_into().unwrap());
        if crc32(&bytes[pos+4..pos+8+n]) != checksum { return Err(ApiError::validation("Invalid PNG checksum")); }
        if kind == b"IDAT" { compressed.extend_from_slice(&bytes[pos+8..pos+8+n]); }
        else if kind == b"IEND" { end = n == 0 && next == bytes.len(); break; }
        else if kind != b"IHDR" && kind[0].is_ascii_uppercase() {
            return Err(ApiError::validation("Unsupported PNG chunk"));
        }
        pos = next;
    }
    if !end || compressed.is_empty() { return Err(ApiError::validation("Incomplete PNG image")); }
    // Verify bounded decompressed scanlines too; header dimensions alone are insufficient.
    use std::io::Read;
    let channels = if bytes[25] == 6 { 4 } else { 3 };
    let row = 1 + width as usize * channels;
    let expected = row * height as usize;
    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(compressed.as_slice()).take(expected as u64 + 1)
        .read_to_end(&mut decoded).map_err(|_| ApiError::validation("Invalid PNG image data"))?;
    if decoded.len() != expected || decoded.chunks_exact(row).any(|r| r[0] > 4) {
        return Err(ApiError::validation("Invalid PNG image dimensions"));
    }
    Ok(())
}

async fn store_marker(state: &AppState, owner: Uuid, marker: &str) -> ApiResult<Option<String>> {
    let pool = state.db().ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "Database unavailable"))?.pool();
    let mut tx = pool.begin().await.map_err(|_| ApiError::internal("Profile unavailable"))?;
    let old: Option<String> = sqlx::query_scalar("select avatar_url from public.users where id=$1 for update")
        .bind(owner).fetch_one(&mut *tx).await.map_err(|_| ApiError::internal("Profile unavailable"))?;
    sqlx::query("update public.users set avatar_url=$2 where id=$1").bind(owner).bind(marker)
        .execute(&mut *tx).await.map_err(|_| ApiError::internal("Cannot save avatar"))?;
    sqlx::query("update auth.users set raw_user_meta_data=coalesce(raw_user_meta_data,'{}'::jsonb)||jsonb_build_object('avatar_url',$2::text) where id=$1")
        .bind(owner).bind(marker).execute(&mut *tx).await.map_err(|_| ApiError::internal("Cannot save avatar"))?;
    tx.commit().await.map_err(|_| ApiError::internal("Cannot save avatar"))?;
    Ok(old.and_then(|value| owned_key(&value, owner)))
}

async fn upload(State(state): State<AppState>, Authenticated(user): Authenticated, bytes: Bytes) -> ApiResult<Json<serde_json::Value>> {
    validate_png(&bytes)?;
    let owner = user.id.into_inner();
    let marker = format!("{PREFIX}{}.png", Uuid::new_v4());
    let key = owned_key(&marker, owner).unwrap();
    let storage = state.storage().ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "Storage unavailable"))?;
    storage.upload(PROJECT_FILES_BUCKET, &key, bytes, "image/png").await?;
    match store_marker(&state, owner, &marker).await {
        Ok(old) => { if let Some(old) = old { let _ = storage.remove(PROJECT_FILES_BUCKET, &[old]).await; } }
        Err(error) => { let _ = storage.remove(PROJECT_FILES_BUCKET, &[key]).await; return Err(error); }
    }
    Ok(Json(json!({"avatarUrl": marker})))
}

async fn reset(State(state): State<AppState>, Authenticated(user): Authenticated) -> ApiResult<StatusCode> {
    let old = store_marker(&state, user.id.into_inner(), "").await?;
    if let (Some(storage), Some(key)) = (state.storage(), old) { let _ = storage.remove(PROJECT_FILES_BUCKET, &[key]).await; }
    Ok(StatusCode::NO_CONTENT)
}

async fn read(State(state): State<AppState>, Authenticated(user): Authenticated, Path(target): Path<Uuid>) -> ApiResult<impl IntoResponse> {
    let pool = state.db().ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "Database unavailable"))?.pool();
    let row = sqlx::query(r#"select u.avatar_url from public.users u where u.id=$1 and ($1=$2 or exists (
      select 1 from public.projects p where
      (p.owner_id=$1 or exists(select 1 from public.project_members m where m.project_id=p.id and m.user_id=$1 and m.invite_accepted_at is not null)) and
      (p.owner_id=$2 or exists(select 1 from public.project_members m where m.project_id=p.id and m.user_id=$2 and m.invite_accepted_at is not null))
    ))"#).bind(target).bind(user.id.into_inner()).fetch_optional(pool).await
        .map_err(|_| ApiError::internal("Avatar unavailable"))?.ok_or_else(|| ApiError::new(ErrorCode::NotFound, "Avatar unavailable"))?;
    let marker: Option<String> = row.get("avatar_url");
    let key = marker.and_then(|v| owned_key(&v, target)).ok_or_else(|| ApiError::new(ErrorCode::NotFound, "Avatar unavailable"))?;
    let storage = state.storage().ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable, "Storage unavailable"))?;
    let bytes = storage.download(PROJECT_FILES_BUCKET, &key).await?;
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "private, no-store"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn namespace_is_owned_and_canonical() {
        let owner = Uuid::new_v4(); let file = Uuid::new_v4();
        assert_eq!(owned_key(&format!("prims-avatar:{file}.png"), owner), Some(format!("avatars/{owner}/{file}.png")));
        for value in ["../file.png", "prims-avatar:../../file.png", "https://example.com/a.png", "prims-avatar:{}.svg"] { assert!(owned_key(value,owner).is_none()); }
    }
    fn png(raw: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut compressor = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(raw).unwrap();
        let compressed = compressor.finish().unwrap();
        let mut image = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, data) in [(b"IHDR".as_slice(), vec![0,0,0,1,0,0,0,1,8,6,0,0,0]), (b"IDAT".as_slice(), compressed), (b"IEND".as_slice(), vec![])] {
            image.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let start = image.len(); image.extend_from_slice(kind); image.extend_from_slice(&data);
            let crc = crc32(&image[start..]); image.extend_from_slice(&crc.to_be_bytes());
        }
        image
    }
    #[test] fn accepts_complete_png_but_rejects_expansion_and_corruption() {
        assert!(validate_png(&png(&[0,255,0,0,255])).is_ok());
        assert!(validate_png(&png(&vec![0; 1024*1024])).is_err());
        let mut corrupt = png(&[0,255,0,0,255]); corrupt[40] ^= 1;
        assert!(validate_png(&corrupt).is_err());
    }
    #[test] fn rejects_wrong_types_dimensions_and_incomplete_images() {
        assert!(validate_png(b"<svg></svg>").is_err());
        let mut image = vec![0; 45]; image[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n"); image[8..12].copy_from_slice(&13u32.to_be_bytes()); image[12..16].copy_from_slice(b"IHDR");
        image[16..20].copy_from_slice(&100000u32.to_be_bytes()); image[20..24].copy_from_slice(&1u32.to_be_bytes());
        assert!(validate_png(&image).is_err());
        image[16..20].copy_from_slice(&1u32.to_be_bytes()); image[24]=8; image[25]=6;
        assert!(validate_png(&image).is_err());
    }
}

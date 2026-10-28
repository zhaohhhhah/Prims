//! Site registration administration. Authorization is database-backed, never user metadata.
use axum::{extract::{Path, Query, State}, middleware::from_fn, routing::{get, post}, Json, Router};
use chrono::{DateTime, Utc};
use scribe_auth::{require_auth, Authenticated};
use scribe_shared::{ApiError, ApiResult, ErrorCode};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, postgres::PgRow};
use uuid::Uuid;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/me", get(me))
        .route("/api/admin/users", get(users))
        .route("/api/admin/users/:id/deletion-preview", get(deletion_preview))
        .route("/api/admin/users/:id/delete", post(delete_account))
        .route("/api/admin/user-deletions/:id", get(deletion_status))
        .route("/api/admin/registration-invites", get(list_invites).post(create_invite))
        .route("/api/admin/registration-invites/:id/revoke", post(revoke_invite))
        .layer(from_fn(require_auth))
}
fn pool(state: &AppState) -> ApiResult<PgPool> {
    state.db().map(|d| d.pool().clone()).ok_or_else(|| ApiError::new(ErrorCode::ServiceUnavailable,"Database unavailable"))
}
fn db_error(_: sqlx::Error) -> ApiError { ApiError::internal("Registration administration database error") }
async fn is_admin(db: &PgPool, user: Uuid) -> ApiResult<bool> {
    sqlx::query_scalar("select exists(select 1 from public.scribe_site_admins where user_id=$1)")
        .bind(user).fetch_one(db).await.map_err(db_error)
}
async fn require_admin(db: &PgPool, user: Uuid) -> ApiResult<()> {
    if is_admin(db,user).await? { Ok(()) } else { Err(ApiError::new(ErrorCode::Forbidden,"仅管理员可访问")) }
}
#[derive(Serialize)] #[serde(rename_all="camelCase")]
struct Access { is_admin: bool }
async fn me(State(state): State<AppState>, Authenticated(user): Authenticated) -> ApiResult<Json<Access>> {
    let db=pool(&state)?;
    Ok(Json(Access { is_admin:is_admin(&db,user.id.into_inner()).await? }))
}
#[derive(Serialize)] #[serde(rename_all="camelCase")]
struct Account {
    id: Uuid, email: Option<String>, display_name: Option<String>,
    created_at: DateTime<Utc>, last_sign_in_at: Option<DateTime<Utc>>, is_admin: bool,
}
#[derive(Deserialize,Default)] struct UserSearch { q: Option<String> }
async fn users(State(state): State<AppState>, Authenticated(user): Authenticated, Query(search):Query<UserSearch>) -> ApiResult<Json<Vec<Account>>> {
    let db=pool(&state)?; require_admin(&db,user.id.into_inner()).await?;
    let rows=sqlx::query("select u.id,u.email,u.raw_user_meta_data->>'display_name' as display_name,u.created_at,u.last_sign_in_at,exists(select 1 from public.scribe_site_admins a where a.user_id=u.id) as is_admin from auth.users u where u.deleted_at is null and ($1='' or strpos(lower(coalesce(u.email,'')),lower($1))>0 or strpos(lower(coalesce(u.raw_user_meta_data->>'display_name','')),lower($1))>0) order by u.created_at desc limit 1000")
        .bind(search.q.unwrap_or_default().trim().chars().take(200).collect::<String>()).fetch_all(&db).await.map_err(db_error)?;
    Ok(Json(rows.into_iter().map(|r| Account { id:r.get("id"),email:r.get("email"),display_name:r.get("display_name"),created_at:r.get("created_at"),last_sign_in_at:r.get("last_sign_in_at"),is_admin:r.get("is_admin") }).collect()))
}
#[derive(Serialize)] #[serde(rename_all="camelCase")]
struct Invite {
    id:Uuid, hint:String, label:String, max_uses:i32, used_count:i32,
    expires_at:DateTime<Utc>, revoked_at:Option<DateTime<Utc>>, created_at:DateTime<Utc>,
}
fn invite(row:PgRow) -> Invite {
    Invite { id:row.get("id"),hint:row.get("hint"),label:row.get("label"),max_uses:row.get("max_uses"),used_count:row.get("used_count"),expires_at:row.get("expires_at"),revoked_at:row.get("revoked_at"),created_at:row.get("created_at") }
}
async fn list_invites(State(state): State<AppState>, Authenticated(user): Authenticated) -> ApiResult<Json<Vec<Invite>>> {
    let db=pool(&state)?;require_admin(&db,user.id.into_inner()).await?;
    let rows=sqlx::query("select id,hint,label,max_uses,used_count,expires_at,revoked_at,created_at from public.scribe_registration_invites order by created_at desc limit 1000")
        .fetch_all(&db).await.map_err(db_error)?;
    Ok(Json(rows.into_iter().map(invite).collect()))
}
#[derive(Deserialize)] #[serde(rename_all="camelCase")]
struct NewInvite { #[serde(default)] label:String, #[serde(default="one")] max_uses:i32, #[serde(default="seven")] expires_days:i32 }
fn one()->i32 {1} fn seven()->i32 {7}
#[derive(Serialize)] #[serde(rename_all="camelCase")]
struct Created { code:String, invite:Invite }
async fn create_invite(State(state): State<AppState>, Authenticated(user): Authenticated, Json(input):Json<NewInvite>) -> ApiResult<Json<Created>> {
    let db=pool(&state)?;require_admin(&db,user.id.into_inner()).await?;
    if !(1..=10000).contains(&input.max_uses) || !(1..=365).contains(&input.expires_days) || input.label.chars().count()>80 {
        return Err(ApiError::validation("使用次数需为 1–10000，有效期需为 1–365 天，备注最多 80 字"));
    }
    let code=format!("SCRIBE-{}",Uuid::new_v4().simple()).to_uppercase();
    let hint=format!("{}…{}",&code[..11],&code[code.len()-4..]);
    let row=sqlx::query("insert into public.scribe_registration_invites(code_hash,hint,label,max_uses,expires_at,created_by) values(encode(sha256(convert_to($1,'UTF8')),'hex'),$2,$3,$4,now()+($5::int * interval '1 day'),$6) returning id,hint,label,max_uses,used_count,expires_at,revoked_at,created_at")
        .bind(&code).bind(hint).bind(input.label.trim()).bind(input.max_uses).bind(input.expires_days).bind(user.id.into_inner())
        .fetch_one(&db).await.map_err(db_error)?;
    Ok(Json(Created { code,invite:invite(row) }))
}
async fn revoke_invite(State(state):State<AppState>, Authenticated(user):Authenticated, Path(id):Path<Uuid>) -> ApiResult<Json<Invite>> {
    let db=pool(&state)?;require_admin(&db,user.id.into_inner()).await?;
    let row=sqlx::query("update public.scribe_registration_invites set revoked_at=coalesce(revoked_at,now()) where id=$1 returning id,hint,label,max_uses,used_count,expires_at,revoked_at,created_at")
        .bind(id).fetch_optional(&db).await.map_err(db_error)?.ok_or_else(||ApiError::not_found("邀请码不存在"))?;
    Ok(Json(invite(row)))
}

async fn deletion_preview(State(state):State<AppState>,Authenticated(user):Authenticated,Path(id):Path<Uuid>)->ApiResult<Json<crate::services::user_management::Impact>> {
 let db=pool(&state)?;Ok(Json(crate::services::user_management::impact(&db,user.id.into_inner(),id).await?))
}
async fn delete_account(State(state):State<AppState>,Authenticated(user):Authenticated,Path(id):Path<Uuid>,Json(input):Json<crate::services::user_management::DeleteInput>)->ApiResult<Json<crate::services::user_management::Deleted>> {
 if state.storage().is_none(){return Err(ApiError::new(ErrorCode::ServiceUnavailable,"附件清理服务未配置，不能删除账号"));}
 let db=pool(&state)?;Ok(Json(crate::services::user_management::delete(&db,user.id.into_inner(),id,input).await?))
}
async fn deletion_status(State(state):State<AppState>,Authenticated(user):Authenticated,Path(id):Path<Uuid>)->ApiResult<Json<crate::services::user_management::CleanupStatus>> {
 let db=pool(&state)?;Ok(Json(crate::services::user_management::status(&db,user.id.into_inner(),id).await?))
}

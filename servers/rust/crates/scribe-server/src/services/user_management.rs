//! Administrator hard deletion with transactional ownership capture and durable,
//! exact-namespace Storage cleanup. No user-supplied bucket or object key.
use std::{sync::Arc,time::Duration};
use chrono::{DateTime,Utc};
use scribe_shared::{ApiError,ApiResult,ErrorCode};
use scribe_storage::{Storage,SupabaseStorage};
use serde::{Deserialize,Serialize};
use sqlx::{PgPool,Row};
use uuid::Uuid;

#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub struct OwnedProject { pub id:Uuid,pub name:String }
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub struct Impact {
 pub user_id:Uuid,pub email:String,pub owned_projects:Vec<OwnedProject>,pub attachment_count:i64,
 pub joined_project_count:i64,pub active_compile_count:i64,pub can_delete:bool,pub blocked_reason:Option<String>,
}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct DeleteInput { pub confirmation_email:String,pub permanently_delete_owned_projects:bool }
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub struct Deleted { pub deletion_id:Uuid,pub deleted:bool,pub owned_project_count:usize,pub cleanup_status:&'static str }
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub struct CleanupStatus { pub deletion_id:Uuid,pub status:&'static str,pub pending_scopes:i64,pub last_checked_at:Option<DateTime<Utc>> }

pub async fn require_admin(pool:&PgPool,user:Uuid)->ApiResult<()> {
 let ok:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_site_admins a join auth.users u on u.id=a.user_id where a.user_id=$1 and u.deleted_at is null)").bind(user).fetch_one(pool).await.map_err(db)?;
 if ok {Ok(())}else{Err(ApiError::forbidden("仅管理员可访问"))}
}
pub async fn impact(pool:&PgPool,actor:Uuid,target:Uuid)->ApiResult<Impact>{
 require_admin(pool,actor).await?;
 let email:String=sqlx::query_scalar("select email from auth.users where id=$1 and deleted_at is null").bind(target).fetch_optional(pool).await.map_err(db)?.ok_or_else(||ApiError::not_found("账号不存在"))?;
 let projects=sqlx::query("select id,name from public.projects where owner_id=$1 order by created_at,id").bind(target).fetch_all(pool).await.map_err(db)?;
 let owned_projects=projects.into_iter().map(|r|OwnedProject{id:r.get("id"),name:r.get("name")}).collect::<Vec<_>>();
 let ids=owned_projects.iter().map(|p|p.id).collect::<Vec<_>>();
 let attachment_count:i64=sqlx::query_scalar("select count(*) from storage.objects where (bucket_id in ('project-files','compile-artifacts','version-snapshots') and split_part(name,'/',1)=any(select x::text from unnest($1::uuid[])x)) or (bucket_id='project-files' and split_part(name,'/',1)='avatars' and split_part(name,'/',2)=$2::uuid::text)").bind(&ids).bind(target).fetch_one(pool).await.map_err(db)?;
 let joined_project_count:i64=sqlx::query_scalar("select count(*) from public.project_members m join public.projects p on p.id=m.project_id where m.user_id=$1 and p.owner_id<>$1").bind(target).fetch_one(pool).await.map_err(db)?;
 let active_compile_count:i64=sqlx::query_scalar("select count(*) from public.compile_jobs where status in ('queued','running') and (project_id=any($1) or triggered_by=$2)").bind(&ids).bind(target).fetch_one(pool).await.map_err(db)?;
 let admin_count:i64=sqlx::query_scalar("select count(*) from public.scribe_site_admins").fetch_one(pool).await.map_err(db)?;
 let target_admin:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_site_admins where user_id=$1)").bind(target).fetch_one(pool).await.map_err(db)?;
 let blocked_reason=blocked(actor,target,target_admin,admin_count,active_compile_count).map(str::to_owned);
 Ok(Impact{user_id:target,email,owned_projects,attachment_count,joined_project_count,active_compile_count,can_delete:blocked_reason.is_none(),blocked_reason})
}
fn blocked(actor:Uuid,target:Uuid,target_admin:bool,admins:i64,active:i64)->Option<&'static str>{
 if actor==target {Some("不能删除当前登录的管理员账号")}
 else if target_admin&&admins<=1 {Some("不能删除最后一位管理员")}
 else if active>0 {Some("该账号或其项目正在编译，请等待全部编译结束后再删除")}
 else{None}
}
fn confirm(expected:&str,input:&DeleteInput)->ApiResult<()> {
 if !input.permanently_delete_owned_projects||input.confirmation_email.trim()!=expected {Err(ApiError::validation("请准确输入目标邮箱，并确认永久删除账号及其名下项目"))}else{Ok(())}
}
pub async fn delete(pool:&PgPool,actor:Uuid,target:Uuid,input:DeleteInput)->ApiResult<Deleted>{
 require_admin(pool,actor).await?;
 let mut tx=pool.begin().await.map_err(db)?;
 // Serialize all administrator membership changes, including concurrent deletes.
 sqlx::query("lock table public.scribe_site_admins in share row exclusive mode").execute(&mut *tx).await.map_err(db)?;
 let admin:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_site_admins where user_id=$1)").bind(actor).fetch_one(&mut *tx).await.map_err(db)?;
 if !admin{return Err(ApiError::forbidden("管理员权限已变更"));}
 let email:String=sqlx::query_scalar("select email from auth.users where id=$1 and deleted_at is null for update").bind(target).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(||ApiError::not_found("账号不存在"))?;
 confirm(&email,&input)?;
 // Public-user lock blocks concurrent new owned projects through the owner FK.
 sqlx::query("select id from public.users where id=$1 for update").bind(target).fetch_optional(&mut *tx).await.map_err(db)?;
 let ids:Vec<Uuid>=sqlx::query_scalar("select id from public.projects where owner_id=$1 order by id for update").bind(target).fetch_all(&mut *tx).await.map_err(db)?;
 let active:i64=sqlx::query_scalar("select count(*) from public.compile_jobs where status in ('queued','running') and (project_id=any($1) or triggered_by=$2)").bind(&ids).bind(target).fetch_one(&mut *tx).await.map_err(db)?;
 let admins:i64=sqlx::query_scalar("select count(*) from public.scribe_site_admins").fetch_one(&mut *tx).await.map_err(db)?;
 let target_admin:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_site_admins where user_id=$1)").bind(target).fetch_one(&mut *tx).await.map_err(db)?;
 if let Some(reason)=blocked(actor,target,target_admin,admins,active){return Err(ApiError::new(ErrorCode::Conflict,reason));}
 let deletion=Uuid::new_v4();
 sqlx::query("insert into public.prims_user_deletions(id,target_user_id,requested_by,project_count) values($1,$2,$3,$4)").bind(deletion).bind(target).bind(actor).bind(ids.len() as i32).execute(&mut *tx).await.map_err(db)?;
 // Persist only namespaces proven to belong to the target, never arbitrary keys.
 sqlx::query("insert into public.prims_user_deletion_scopes(deletion_id,bucket,kind,scope_id) select $1,bucket,'project',pid from unnest($2::uuid[])pid cross join unnest(array['project-files','compile-artifacts','version-snapshots'])bucket union all select $1,'project-files','avatar',$3").bind(deletion).bind(&ids).bind(target).execute(&mut *tx).await.map_err(db)?;
 // Freeze both known object records and authoritative app references; namespace
 // checks reject corrupt cross-project references instead of deleting others' data.
 sqlx::query(r#"insert into public.prims_user_deletion_objects(scope_id,object_key)
 select s.id,k.key from public.prims_user_deletion_scopes s join lateral (
  select o.name as key from storage.objects o where o.bucket_id=s.bucket and
   ((s.kind='project' and split_part(o.name,'/',1)=s.scope_id::text) or
    (s.kind='avatar' and split_part(o.name,'/',1)='avatars' and split_part(o.name,'/',2)=s.scope_id::text))
  union select f.storage_key from public.project_files f where s.kind='project' and s.bucket='project-files' and f.project_id=s.scope_id and split_part(f.storage_key,'/',1)=s.scope_id::text
  union select v.storage_key from public.project_versions v where s.kind='project' and s.bucket='version-snapshots' and v.project_id=s.scope_id and split_part(v.storage_key,'/',1)=s.scope_id::text
  union select k from public.compile_jobs c cross join lateral unnest(array[c.pdf_key,c.log_key,c.synctex_key])k where s.kind='project' and s.bucket='compile-artifacts' and c.project_id=s.scope_id and split_part(k,'/',1)=s.scope_id::text
 )k on true where s.deletion_id=$1 on conflict do nothing"#).bind(deletion).execute(&mut *tx).await.map_err(db)?;
 sqlx::query("delete from public.yjs_updates where split_part(doc_id,'/',1)=any(select x::text from unnest($1::uuid[])x)").bind(&ids).execute(&mut *tx).await.map_err(db)?;
 // Equivalent hard-delete transaction for this audited GoTrue schema. Cascades
 // remove sessions, identities, MFA, one-time tokens and public personal data.
 // Older refresh tokens without a session FK use a textual user_id.
 sqlx::query("delete from auth.refresh_tokens where user_id=$1::text").bind(target.to_string()).execute(&mut *tx).await.map_err(db)?;
 sqlx::query("delete from auth.flow_state where user_id=$1").bind(target).execute(&mut *tx).await.map_err(db)?;
 sqlx::query("delete from auth.users where id=$1").bind(target).execute(&mut *tx).await.map_err(db)?;
 tx.commit().await.map_err(db)?;
 Ok(Deleted{deletion_id:deletion,deleted:true,owned_project_count:ids.len(),cleanup_status:"pending"})
}
pub async fn status(pool:&PgPool,actor:Uuid,id:Uuid)->ApiResult<CleanupStatus>{
 require_admin(pool,actor).await?;
 let exists:bool=sqlx::query_scalar("select exists(select 1 from public.prims_user_deletions where id=$1)").bind(id).fetch_one(pool).await.map_err(db)?;
 if !exists{return Err(ApiError::not_found("删除记录不存在"));}
 let row=sqlx::query("select count(*) filter(where status='pending') as pending,max(last_checked_at) as last_checked from public.prims_user_deletion_scopes where deletion_id=$1").bind(id).fetch_one(pool).await.map_err(db)?;
 let pending:i64=row.get("pending");Ok(CleanupStatus{deletion_id:id,status:if pending==0{"cleaned"}else{"pending"},pending_scopes:pending,last_checked_at:row.get("last_checked")})
}
fn key_allowed(bucket:&str,kind:&str,scope:Uuid,key:&str)->bool {
 if !matches!(bucket,"project-files"|"compile-artifacts"|"version-snapshots")||key.contains(['\\','\0'])||key.split('/').any(|p|p=="."||p==".."||p.is_empty()){return false;}
 match kind {"project"=>key.starts_with(&format!("{scope}/")),"avatar"=>bucket=="project-files"&&key.starts_with(&format!("avatars/{scope}/")),_=>false}
}
pub async fn cleanup_one(pool:&PgPool,storage:&dyn Storage,only_deletion:Option<Uuid>)->ApiResult<bool>{
 let lease=Uuid::new_v4();
 let row=sqlx::query("update public.prims_user_deletion_scopes set lease_id=$1,lease_until=now()+interval '90 seconds',attempts=attempts+1 where id=(select id from public.prims_user_deletion_scopes where next_attempt_at<=now() and (lease_until is null or lease_until<now()) and ($2::uuid is null or deletion_id=$2) order by next_attempt_at,id for update skip locked limit 1) returning id,bucket,kind,scope_id").bind(lease).bind(only_deletion).fetch_optional(pool).await.map_err(db)?;
 let Some(row)=row else{return Ok(false)};
 let id:Uuid=row.get("id");let bucket:String=row.get("bucket");let kind:String=row.get("kind");let scope:Uuid=row.get("scope_id");
 let keys:Vec<String>=sqlx::query_scalar("select object_key from public.prims_user_deletion_objects where scope_id=$1 union select name from storage.objects where bucket_id=$2 and (($3='project' and split_part(name,'/',1)=$4::uuid::text) or ($3='avatar' and split_part(name,'/',1)='avatars' and split_part(name,'/',2)=$4::uuid::text)) limit 100").bind(id).bind(&bucket).bind(&kind).bind(scope).fetch_all(pool).await.map_err(db)?;
 let result=if keys.iter().all(|k|key_allowed(&bucket,&kind,scope,k)){storage.remove(&bucket,&keys).await}else{Err(ApiError::internal("Unsafe deletion namespace rejected"))};
 match result {
  Ok(())=>{
   sqlx::query("delete from public.prims_user_deletion_objects where scope_id=$1 and object_key=any($2)").bind(id).bind(&keys).execute(pool).await.map_err(db)?;
   // Keep completed scopes as durable tombstones and periodically rescan for
   // requests already in flight; never scan unrelated users or namespaces.
   sqlx::query("update public.prims_user_deletion_scopes set status=case when $3 then 'cleaned' else 'pending' end,next_attempt_at=now()+case when $3 then interval '1 minute' else interval '1 second' end,last_checked_at=now(),last_error=null,lease_id=null,lease_until=null where id=$1 and lease_id=$2").bind(id).bind(lease).bind(keys.is_empty()).execute(pool).await.map_err(db)?;
  },
  Err(_)=>{sqlx::query("update public.prims_user_deletion_scopes set status='pending',next_attempt_at=now()+interval '30 seconds',last_error='storage_cleanup_failed',lease_id=null,lease_until=null where id=$1 and lease_id=$2").bind(id).bind(lease).execute(pool).await.map_err(db)?;}
 }
 Ok(true)
}
pub fn start_cleanup_worker(state:crate::state::AppState){
 let Some(pool)=state.db().map(|db|db.pool().clone())else{return};let Some(storage):Option<Arc<SupabaseStorage>>=state.inner.storage.clone()else{return};
 tokio::spawn(async move{let mut tick=tokio::time::interval(Duration::from_secs(2));tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);loop{tick.tick().await;for _ in 0..8{match cleanup_one(&pool,storage.as_ref(),None).await{Ok(true)=>{},Ok(false)=>break,Err(_)=>{tracing::warn!("User deletion cleanup deferred; persistent queue retained");break;}}}}});
}
fn db(_:sqlx::Error)->ApiError {ApiError::internal("用户管理操作失败，未完成的附件清理将保留并重试")}
#[cfg(test)]mod tests{use super::*;
 #[test]fn destructive_confirmation_is_exact(){let valid=DeleteInput{confirmation_email:"target@example.test".into(),permanently_delete_owned_projects:true};assert!(confirm("target@example.test",&valid).is_ok());assert!(confirm("other@example.test",&valid).is_err());assert!(confirm("target@example.test",&DeleteInput{permanently_delete_owned_projects:false,..valid}).is_err());}
 #[test]fn protects_self_last_admin_and_active_compile(){let a=Uuid::new_v4();let b=Uuid::new_v4();assert!(blocked(a,a,false,2,0).is_some());assert!(blocked(a,b,true,1,0).is_some());assert!(blocked(a,b,false,2,1).is_some());assert!(blocked(a,b,true,2,0).is_none());}
 #[test]fn cleanup_keys_cannot_escape_proven_namespace(){let id=Uuid::new_v4();assert!(key_allowed("project-files","project",id,&format!("{id}/file")));assert!(key_allowed("project-files","avatar",id,&format!("avatars/{id}/x.png")));for k in [format!("{}/file",Uuid::new_v4()),format!("{id}/../x"),format!("{id}//x"),format!("avatars/{id}/../x")]{assert!(!key_allowed("project-files","project",id,&k));}assert!(!key_allowed("other","project",id,&format!("{id}/file")));}
}
#[cfg(test)]mod integration_tests;

use super::*;
use std::sync::atomic::{AtomicBool,Ordering};
use bytes::Bytes;
use scribe_auth::TokenVerifier;
use serde_json::json;

struct FailingOnceStorage { pool:PgPool, fail:AtomicBool }
#[axum::async_trait]
impl Storage for FailingOnceStorage {
 async fn upload(&self,_:&str,_:&str,_:Bytes,_:&str)->ApiResult<()>{unreachable!()}
 async fn download(&self,_:&str,_:&str)->ApiResult<Bytes>{unreachable!()}
 async fn signed_url(&self,_:&str,_:&str,_:u32)->ApiResult<String>{unreachable!()}
 async fn remove(&self,bucket:&str,keys:&[String])->ApiResult<()>{
  if self.fail.swap(false,Ordering::SeqCst){return Err(ApiError::internal("synthetic storage unavailable"));}
  let mut tx=self.pool.begin().await.map_err(db)?;
  sqlx::query("set local storage.allow_delete_query='true'").execute(&mut *tx).await.map_err(db)?;
  sqlx::query("delete from storage.objects where bucket_id=$1 and name=any($2)").bind(bucket).bind(keys).execute(&mut *tx).await.map_err(db)?;tx.commit().await.map_err(db)?;Ok(())
 }
}
async fn account(pool:&PgPool,admin:bool)->(Uuid,String){
 let id=Uuid::new_v4();let email=format!("fixture-{id}@example.test");let mut tx=pool.begin().await.unwrap();
 // Disposable schema-only DB only. Avoid registration fixtures and mail entirely.
 sqlx::query("set local session_replication_role=replica").execute(&mut *tx).await.unwrap();
 sqlx::query("insert into auth.users(id,email,email_confirmed_at,raw_user_meta_data,created_at) values($1,$2,now(),'{}',now())").bind(id).bind(&email).execute(&mut *tx).await.unwrap();
 sqlx::query("insert into public.users(id,email) values($1,$2)").bind(id).bind(&email).execute(&mut *tx).await.unwrap();
 if admin{sqlx::query("insert into public.scribe_site_admins(user_id) values($1)").bind(id).execute(&mut *tx).await.unwrap();}
 tx.commit().await.unwrap();(id,email)
}
async fn project(pool:&PgPool,owner:Uuid)->Uuid{
 sqlx::query_scalar("insert into public.projects(name,owner_id,compiler) values('Synthetic deletion fixture',$1,'xelatex') returning id").bind(owner).fetch_one(pool).await.unwrap()
}
fn input(email:String)->DeleteInput{DeleteInput{confirmation_email:email,permanently_delete_owned_projects:true}}
async fn count(pool:&PgPool,table:&str,column:&str,id:Uuid)->i64{
 sqlx::query_scalar(&format!("select count(*) from {table} where {column}=$1")).bind(id).fetch_one(pool).await.unwrap()
}
#[tokio::test]
#[ignore="Requires disposable schema-only DB, never production"]
async fn actual_schema_hard_delete_revokes_cached_tokens_preserves_shared_projects_and_retries_cleanup(){
 let url=std::env::var("PRIMS_USER_MANAGEMENT_TEST_DB").expect("explicit isolated test DB required");
 assert!(url.ends_with("/prims_user_management_test"));
 let pool=sqlx::postgres::PgPoolOptions::new().max_connections(8).connect(&url).await.unwrap();
 assert_eq!(sqlx::query_scalar::<_,String>("select current_database()").fetch_one(&pool).await.unwrap(),"prims_user_management_test");
 let (admin,_)=account(&pool,true).await;let(target,email)=account(&pool,false).await;let(other,_)=account(&pool,false).await;
 let owned=project(&pool,target).await;let shared=project(&pool,other).await;
 sqlx::query("insert into public.project_members(project_id,user_id,role,invite_accepted_at) values($1,$2,'editor',now())").bind(shared).bind(target).execute(&pool).await.unwrap();
 let fid=Uuid::new_v4();
 for pid in [owned,shared]{sqlx::query("insert into public.project_files(project_id,path,type,storage_key,size_bytes,created_by) values($1,'main.tex','tex',$2,10,$3)").bind(pid).bind(format!("{pid}/{fid}")).bind(target).execute(&pool).await.unwrap();}
 let sid=Uuid::new_v4();sqlx::query("insert into auth.sessions(id,user_id) values($1,$2)").bind(sid).bind(target).execute(&pool).await.unwrap();
 sqlx::query("insert into auth.refresh_tokens(id,user_id,session_id,token) values(90000001,$1,$2,'synthetic-refresh')").bind(target.to_string()).bind(sid).execute(&pool).await.unwrap();
 sqlx::query("insert into auth.identities(provider_id,user_id,identity_data,provider) values($1,$2,$3,'email')").bind(target.to_string()).bind(target).bind(json!({"sub":target,"email":email})).execute(&pool).await.unwrap();
 sqlx::query("insert into public.scribe_ai_model_profiles(user_id,name,provider,model) values($1,'Synthetic profile','openai','fake')").bind(target).execute(&pool).await.unwrap();
 sqlx::query("insert into public.scribe_ai_contexts(user_id,project_id) values($1,$2)").bind(target).bind(shared).execute(&pool).await.unwrap();
 let thread=Uuid::new_v4();sqlx::query("insert into public.scribe_ai_context_threads(user_id,project_id,id,title) values($1,$2,$3,'Synthetic thread')").bind(target).bind(shared).bind(thread).execute(&pool).await.unwrap();
 sqlx::query("insert into public.scribe_ai_context_messages(user_id,project_id,thread_id,id,role,content) values($1,$2,$3,$4,'user','Synthetic only')").bind(target).bind(shared).bind(thread).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
 for bucket in ["project-files","compile-artifacts","version-snapshots"]{sqlx::query("insert into storage.buckets(id,name) values($1,$1) on conflict do nothing").bind(bucket).execute(&pool).await.unwrap();}
 let owned_key=format!("{owned}/{fid}");let shared_key=format!("{shared}/{fid}");let avatar=format!("avatars/{target}/{}.png",Uuid::new_v4());
 for (bucket,key) in [("project-files",owned_key.clone()),("project-files",shared_key.clone()),("project-files",avatar.clone()),("compile-artifacts",format!("{owned}/{}/compile.log",Uuid::new_v4())),("version-snapshots",format!("{owned}/{}.json",Uuid::new_v4()))]{sqlx::query("insert into storage.objects(bucket_id,name) values($1,$2)").bind(bucket).bind(key).execute(&pool).await.unwrap();}
 let verifier=TokenVerifier::new(Some("synthetic-test-signing-secret"),None).with_accounts(pool.clone());
 let token=jsonwebtoken::encode(&jsonwebtoken::Header::default(),&json!({"sub":target,"email":email,"role":"authenticated","exp":Utc::now().timestamp()+3600}),&jsonwebtoken::EncodingKey::from_secret(b"synthetic-test-signing-secret")).unwrap();
 assert!(verifier.verify(&token).await.is_ok());assert!(verifier.verify(&token).await.is_ok());
 assert!(require_admin(&pool,other).await.is_err());
 assert!(delete(&pool,admin,target,input("wrong@example.test".into())).await.is_err());assert_eq!(count(&pool,"auth.users","id",target).await,1);
 assert!(delete(&pool,admin,admin,input("not relevant".into())).await.is_err());
 let active:Uuid=sqlx::query_scalar("insert into public.compile_jobs(project_id,triggered_by,status,engine,main_file) values($1,$2,'queued','xelatex','main.tex') returning id").bind(owned).bind(target).fetch_one(&pool).await.unwrap();
 assert!(delete(&pool,admin,target,input(email.clone())).await.is_err());assert_eq!(count(&pool,"auth.users","id",target).await,1);
 sqlx::query("update public.compile_jobs set status='error' where id=$1").bind(active).execute(&pool).await.unwrap();
 let preview=impact(&pool,admin,target).await.unwrap();assert_eq!(preview.owned_projects.len(),1);assert_eq!(preview.joined_project_count,1);assert_eq!(preview.attachment_count,4);
 let deleted=delete(&pool,admin,target,input(email)).await.unwrap();assert!(deleted.deleted);
 for (table,col) in [("auth.users","id"),("public.users","id"),("auth.sessions","user_id"),("auth.identities","user_id"),("public.scribe_ai_model_profiles","user_id"),("public.scribe_ai_context_threads","user_id"),("public.scribe_ai_context_messages","user_id")]{assert_eq!(count(&pool,table,col,target).await,0,"{table}");}
 assert!(verifier.verify(&token).await.is_err(),"cached cryptographic token must be rejected after account deletion");
 assert_eq!(count(&pool,"public.projects","id",owned).await,0);assert_eq!(count(&pool,"public.projects","id",shared).await,1);assert_eq!(count(&pool,"public.project_members","user_id",target).await,0);
 assert_eq!(sqlx::query_scalar::<_,i64>("select count(*) from public.project_files where project_id=$1 and created_by is null").bind(shared).fetch_one(&pool).await.unwrap(),1);
 let storage=FailingOnceStorage{pool:pool.clone(),fail:AtomicBool::new(true)};
 assert!(cleanup_one(&pool,&storage,Some(deleted.deletion_id)).await.unwrap());
 assert!(status(&pool,admin,deleted.deletion_id).await.unwrap().pending_scopes>0);
 for _ in 0..12{
  sqlx::query("update public.prims_user_deletion_scopes set next_attempt_at=now() where deletion_id=$1 and status='pending'").bind(deleted.deletion_id).execute(&pool).await.unwrap();
  cleanup_one(&pool,&storage,Some(deleted.deletion_id)).await.unwrap();
 }
 assert_eq!(sqlx::query_scalar::<_,i64>("select count(*) from storage.objects where name=$1").bind(&shared_key).fetch_one(&pool).await.unwrap(),1);
 assert_eq!(sqlx::query_scalar::<_,i64>("select count(*) from storage.objects where name=$1 or name=$2").bind(&owned_key).bind(&avatar).fetch_one(&pool).await.unwrap(),0);
 assert_eq!(status(&pool,admin,deleted.deletion_id).await.unwrap().status,"cleaned");
 // Persisted cleaned namespaces catch a late upload from an already-running request.
 sqlx::query("insert into storage.objects(bucket_id,name) values('project-files',$1)").bind(&owned_key).execute(&pool).await.unwrap();
 sqlx::query("update public.prims_user_deletion_scopes set next_attempt_at=now() where deletion_id=$1").bind(deleted.deletion_id).execute(&pool).await.unwrap();
 for _ in 0..8{sqlx::query("update public.prims_user_deletion_scopes set next_attempt_at=now() where deletion_id=$1 and status='pending'").bind(deleted.deletion_id).execute(&pool).await.unwrap();cleanup_one(&pool,&storage,Some(deleted.deletion_id)).await.unwrap();}
 assert_eq!(sqlx::query_scalar::<_,i64>("select count(*) from storage.objects where name=$1").bind(&owned_key).fetch_one(&pool).await.unwrap(),0);
 // Concurrent administrators attempting to delete each other leave one admin.
 let (second,second_email)=account(&pool,true).await;let first_email:String=sqlx::query_scalar("select email from auth.users where id=$1").bind(admin).fetch_one(&pool).await.unwrap();
 let (a,b)=tokio::join!(delete(&pool,admin,second,input(second_email)),delete(&pool,second,admin,input(first_email)));
 assert_eq!(usize::from(a.is_ok())+usize::from(b.is_ok()),1);
 assert_eq!(sqlx::query_scalar::<_,i64>("select count(*) from public.scribe_site_admins").fetch_one(&pool).await.unwrap(),1);
 println!("actual schema deletion, auth-cache revocation, shared preservation, failed/late Storage retry, admin concurrency PASS");
}

use super::*;
use axum::{Router,body::Bytes as BodyBytes,extract::{Path,State},http::Method,routing::any,Json};
use serde_json::{Value,json};
use std::collections::HashMap;
use tokio::sync::Mutex;
type Objects=Arc<Mutex<HashMap<String,Vec<u8>>>>;
async fn storage(State(objects):State<Objects>,Path(path):Path<String>,method:Method,body:BodyBytes)->Json<Value>{
 let mut objects=objects.lock().await;
 if method==Method::POST{objects.insert(path,body.to_vec());}
 else if method==Method::DELETE {let v:Value=serde_json::from_slice(&body).unwrap();for key in v["prefixes"].as_array().unwrap(){objects.remove(&format!("{}/{}",path,key.as_str().unwrap()));}}
 Json(json!({}))
}
#[tokio::test]
#[ignore="Requires disposable schema-only DB and synthetic Storage HTTP endpoint"]
async fn quota_atomic_concurrent_uploads_reject_before_storage_and_preserve_original_bytes(){
 let url=std::env::var("PRIMS_USER_MANAGEMENT_TEST_DB").expect("explicit isolated DB");assert!(url.ends_with("/prims_user_management_test"));
 let pool=sqlx::postgres::PgPoolOptions::new().max_connections(8).connect(&url).await.unwrap();
 let uid=Uuid::new_v4();let mut tx=pool.begin().await.unwrap();sqlx::query("set local session_replication_role=replica").execute(&mut *tx).await.unwrap();
 sqlx::query("insert into auth.users(id,email,email_confirmed_at,raw_user_meta_data,created_at) values($1,$2,now(),'{}',now())").bind(uid).bind(format!("quota-{uid}@example.test")).execute(&mut *tx).await.unwrap();sqlx::query("insert into public.users(id,email) values($1,$2)").bind(uid).bind(format!("quota-{uid}@example.test")).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
 let pid:Uuid=sqlx::query_scalar("insert into public.projects(name,owner_id) values('Synthetic quota test',$1) returning id").bind(uid).fetch_one(&pool).await.unwrap();let project=ProjectId::new(pid);let user=UserId::new(uid);
 let objects:Objects=Arc::new(Mutex::new(HashMap::new()));let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let addr=listener.local_addr().unwrap();let app=Router::new().route("/storage/v1/object/*path",any(storage)).with_state(objects.clone());let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
 let files=FileService::new(pool.clone(),Arc::new(SupabaseStorage::new(&format!("http://{addr}"),"synthetic-test-key").unwrap()));
 let make=|path:&str,content:&str|CreateFileInput{path:path.into(),file_type:None,content:Some(content.into())};
 let original=files.create(user,project,make("original.tex","a")).await.unwrap();let original_key=project_file_key(project,original.id);
 let quota=crate::services::upload_limits::PROJECT_QUOTA_BYTES as i64;
 // Reserve almost all capacity using metadata sizes only, without allocating or
 // sending 500MiB. The production trigger uses the real fixed 524288000 budget.
 let filler:Uuid=sqlx::query_scalar("insert into public.project_files(project_id,path,type,storage_key,size_bytes,created_by) values($1,'reserved.bin','other',$2,$3,$4) returning id").bind(pid).bind(format!("{pid}/synthetic-reservation")).bind(quota-5).bind(uid).fetch_one(&pool).await.unwrap();
 let (a,b)=tokio::join!(files.create(user,project,make("a.bin","123")),files.create(user,project,make("b.bin","123")));
 assert_eq!(usize::from(a.is_ok())+usize::from(b.is_ok()),1,"parallel uploads cannot exceed shared remaining capacity");
 let used:i64=sqlx::query_scalar("select source_bytes from public.projects where id=$1").bind(pid).fetch_one(&pool).await.unwrap();assert_eq!(used,quota-1);
 let before=objects.lock().await.clone();assert_eq!(before.get(&format!("project-files/{original_key}")),Some(&b"a".to_vec()));
 assert!(files.write_content(user,project,original.id,"changed".into()).await.is_err());assert_eq!(*objects.lock().await,before,"quota rejection must happen before any Storage write or overwrite");
 assert!(files.create(user,project,make("too-large.tex","123456")).await.is_err());assert_eq!(*objects.lock().await,before);
 let oversized=sqlx::query("insert into public.project_files(project_id,path,type,storage_key,size_bytes) values($1,'oversized','other',$2,$3)").bind(pid).bind(format!("{pid}/oversized")).bind(quota+1).execute(&pool).await;assert!(oversized.is_err());
 // Counter cannot be forged via a direct project UPDATE.
 assert!(sqlx::query("update public.projects set source_bytes=0 where id=$1").bind(pid).execute(&pool).await.is_err());
 sqlx::query("delete from public.project_files where id=$1").bind(filler).execute(&pool).await.unwrap();
 files.write_content(user,project,original.id,"updated".into()).await.unwrap();let new_key:String=sqlx::query_scalar("select storage_key from public.project_files where id=$1").bind(original.id.into_inner()).fetch_one(&pool).await.unwrap();
 assert_ne!(new_key,original_key);let stored=objects.lock().await;assert_eq!(stored.get(&format!("project-files/{new_key}")),Some(&b"updated".to_vec()));assert!(!stored.contains_key(&format!("project-files/{original_key}")));drop(stored);
 // Direct authenticated writes cannot forge quota metadata or bypass Storage
 // source upload paths. Existing SELECT and avatar behavior are not widened.
 let mut tx=pool.begin().await.unwrap();sqlx::query("set local role authenticated").execute(&mut *tx).await.unwrap();sqlx::query("select set_config('request.jwt.claim.sub',$1,true)").bind(uid.to_string()).execute(&mut *tx).await.unwrap();
 assert!(sqlx::query("insert into storage.objects(bucket_id,name) values('project-files',$1)").bind(format!("{pid}/direct-bypass")).execute(&mut *tx).await.is_err());tx.rollback().await.unwrap();
 let mut tx=pool.begin().await.unwrap();sqlx::query("set local role authenticated").execute(&mut *tx).await.unwrap();sqlx::query("select set_config('request.jwt.claim.sub',$1,true)").bind(uid.to_string()).execute(&mut *tx).await.unwrap();
 assert!(sqlx::query("insert into public.project_files(project_id,path,type,storage_key,size_bytes) values($1,'forged','other',$2,0)").bind(pid).bind(format!("{pid}/forged")).execute(&mut *tx).await.is_err());tx.rollback().await.unwrap();
 server.abort();println!("atomic quota, concurrent uploads, quota-before-Storage, old bytes preserved, staged saves, direct-write bypass guards PASS");
}

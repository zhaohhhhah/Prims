//! Disposable, offline bookshelf derivatives. Original compile artifacts are immutable.
use std::{sync::Arc, time::Duration};
use bytes::Bytes;
use scribe_shared::{ApiError, ApiResult};
use scribe_storage::{Storage, SupabaseStorage, COMPILE_ARTIFACTS_BUCKET};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const MAX_PDF: u64 = 64 * 1024 * 1024;
const GLOBAL_LOCK: i64 = 0x5052494d53504446;
// The same server-owned liveness rule gates discovery and publication.
const IDLE_SQL: &str = r#"
 select j.id, j.project_id, j.pdf_key from public.projects p
 join lateral (select id,project_id,pdf_key,completed_at from public.compile_jobs
   where project_id=p.id and status='success' and pdf_key is not null
   order by enqueued_at desc,id desc limit 1) j on true
 where not exists(select 1 from public.compile_jobs where status in ('queued','running'))
 and greatest(p.updated_at,j.completed_at,coalesce((select max(updated_at)
   from public.project_files where project_id=p.id),p.updated_at))
     <= now()-make_interval(secs => $1::double precision)
"#;

fn idle_seconds(raw: Option<String>) -> u64 {
    raw.and_then(|s| s.parse::<u64>().ok()).filter(|s| (10..=86400).contains(s)).unwrap_or(90)
}
fn configuration() -> Option<(String, String)> {
    let raw=std::env::var("PRIMS_COMPILE_BROKER_URL").ok()?;
    let token=std::env::var("PRIMS_COMPILE_BROKER_TOKEN").ok()?;
    let url=url::Url::parse(&raw).ok()?;
    (url.scheme()=="http" && matches!(url.host_str(),Some("127.0.0.1")|Some("localhost")|Some("[::1]")|Some("::1"))
        && url.port()==Some(19342) && url.username().is_empty() && url.password().is_none()
        && url.query().is_none() && url.fragment().is_none() && matches!(url.path(),""|"/")
        && token.len()>=32).then(||(raw.trim_end_matches('/').to_string(),token))
}
fn db(_: sqlx::Error) -> ApiError { ApiError::internal("PDF preview state unavailable") }

/// Selection is scoped by the already-authorized caller and exact latest successful job.
/// Missing migration, disabled worker, failed optimization and missing objects all fall back.
pub async fn preview_key(pool: &PgPool, project: Uuid, job: Uuid) -> Option<String> {
    sqlx::query_scalar::<_,String>("select artifact_key from public.prims_pdf_previews where project_id=$1 and source_job_id=$2 and status='ready' and artifact_key is not null")
        .bind(project).bind(job).fetch_optional(pool).await.ok().flatten()
}

pub fn start_worker(state: crate::state::AppState) {
    if std::env::var("PRIMS_PDF_PREVIEW_WORKER").ok().as_deref()!=Some("enabled") {return;}
    let (Some(db),Some(storage),Some((base,token)))=(state.db(),state.inner.storage.clone(),configuration()) else {return;};
    let pool=db.pool().clone();
    let idle=idle_seconds(std::env::var("PRIMS_PDF_PREVIEW_IDLE_SECONDS").ok());
    tokio::spawn(async move {
        let mut tick=tokio::time::interval(Duration::from_secs(15));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error)=process_one(&pool,&storage,&base,&token,idle).await {
                tracing::warn!(?error,"Background PDF preview deferred; original PDF remains available");
            }
        }
    });
}

async fn process_one(pool:&PgPool, storage:&Arc<SupabaseStorage>, base:&str, token:&str, idle:u64) -> ApiResult<()> {
    // Transaction advisory lock spans the bounded operation: concurrent app replicas cannot
    // optimize two projects. A crash/timeout rolls back and automatically releases the lock.
    let mut tx=pool.begin().await.map_err(db)?;
    let locked:bool=sqlx::query_scalar("select pg_try_advisory_xact_lock($1)").bind(GLOBAL_LOCK)
        .fetch_one(&mut *tx).await.map_err(db)?;
    if !locked { return Ok(()); }
    let query=format!("select candidate.* from ({IDLE_SQL}) candidate left join public.prims_pdf_previews v on v.project_id=candidate.project_id where v.project_id is null or v.source_job_id<>candidate.id or (v.status='queued' and v.retry_after<=now()) order by candidate.id limit 1");
    let Some(row)=sqlx::query(&query).bind(idle as f64).fetch_optional(&mut *tx).await.map_err(db)? else {return Ok(());};
    let job:Uuid=row.get("id"); let project:Uuid=row.get("project_id"); let original:String=row.get("pdf_key");
    let previous:Option<String>=sqlx::query_scalar("select artifact_key from public.prims_pdf_previews where project_id=$1")
        .bind(project).fetch_optional(&mut *tx).await.map_err(db)?.flatten();
    let key=format!("{project}/{job}/bookshelf-v1.pdf");
    let result=tokio::time::timeout(Duration::from_secs(115),optimize(storage,base,token,&original)).await;
    let (status,output)=match result {
        Ok(Ok(Some(bytes)))=>("ready",Some(bytes)),
        Ok(Ok(None))=>("queued",None), // Busy broker is not an optimization failure.
        _=>("failed",None),
    };
    // Edits, a newer successful compile, deletion, or a compile starting during work invalidate
    // the derivative before it can become visible. Frontend selection independently checks job.
    let valid_query=format!("select exists(select 1 from ({IDLE_SQL}) candidate where candidate.project_id=$2 and candidate.id=$3)");
    let valid:bool=sqlx::query_scalar(&valid_query).bind(idle as f64).bind(project).bind(job)
        .fetch_one(&mut *tx).await.map_err(db)?;
    let mut uploaded=false;
    let artifact=if valid {
        if let Some(bytes)=output {
            if storage.upload(COMPILE_ARTIFACTS_BUCKET,&key,bytes,"application/pdf").await.is_ok() {
                uploaded=true; Some(key.clone())
            } else {None}
        } else {None}
    } else {None};
    let final_status=if !valid {"queued"} else if status=="ready" && artifact.is_none() {"failed"} else {status};
    // Keep the long transaction read-only until publication. No project/preview row or FK
    // lock is held during optimization, so even project deletion does not wait for Ghostscript.
    let published=sqlx::query("insert into public.prims_pdf_previews(project_id,source_job_id,status,artifact_key,attempts,retry_after) values($1,$2,$3,$4,1,now()+interval '45 seconds') on conflict(project_id) do update set source_job_id=$2,status=$3,artifact_key=$4,attempts=case when prims_pdf_previews.source_job_id=$2 then prims_pdf_previews.attempts+1 else 1 end,retry_after=now()+interval '45 seconds',updated_at=now()")
        .bind(project).bind(job).bind(final_status).bind(&artifact).execute(&mut *tx).await;
    if published.is_err() {
        if uploaded {let _=storage.remove(COMPILE_ARTIFACTS_BUCKET,&[key.clone()]).await;}
        return Err(ApiError::internal("PDF preview publication deferred"));
    }
    // The short publication FK check serializes with project deletion; a failed check removes
    // newly uploaded bytes. Namespace cleanup also knows this conventional project/job key.
    if tx.commit().await.is_err() {
        if uploaded {let _=storage.remove(COMPILE_ARTIFACTS_BUCKET,&[key]).await;}
        return Err(ApiError::internal("PDF preview publication deferred"));
    }
    if let Some(old)=previous.filter(|old| old!=&key && old!=&original) {
        let _=storage.remove(COMPILE_ARTIFACTS_BUCKET,&[old]).await;
    }
    Ok(())
}

async fn optimize(storage:&SupabaseStorage,base:&str,token:&str,key:&str) -> ApiResult<Option<Bytes>> {
    // Stream-bounded download; a corrupt/oversized PDF cannot grow app memory unboundedly.
    let dir=tempfile::tempdir().map_err(|_|ApiError::internal("PDF preview scratch unavailable"))?;
    let path=dir.path().join("input.pdf");
    storage.download_to_path(COMPILE_ARTIFACTS_BUCKET,key,&path,MAX_PDF).await?;
    let body=tokio::fs::read(path).await.map_err(|_|ApiError::internal("PDF preview input unavailable"))?;
    if !body.starts_with(b"%PDF-") {return Err(ApiError::validation("Invalid PDF preview input"));}
    let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(105)).build()
        .map_err(|_|ApiError::internal("PDF preview connection unavailable"))?;
    let mut reply=client.post(format!("{base}/optimize")).bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE,"application/pdf").body(body).send().await
        .map_err(|_|ApiError::internal("PDF preview sandbox unavailable"))?;
    if matches!(reply.status().as_u16(),429|503) {return Ok(None);}
    if !reply.status().is_success() {return Err(ApiError::internal("PDF preview sandbox rejected input"));}
    let mut output=Vec::new();
    while let Some(chunk)=reply.chunk().await.map_err(|_|ApiError::internal("PDF preview interrupted"))? {
        if output.len()+chunk.len()>MAX_PDF as usize {return Err(ApiError::validation("PDF preview output too large"));}
        output.extend_from_slice(&chunk);
    }
    if !output.starts_with(b"%PDF-") {return Err(ApiError::internal("Invalid PDF preview result"));}
    Ok(Some(Bytes::from(output)))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn bounded_idle_configuration() {
        assert_eq!(idle_seconds(None),90);
        assert_eq!(idle_seconds(Some("120".into())),120);
        for raw in ["0","-1","1","86401","bad"] {assert_eq!(idle_seconds(Some(raw.into())),90);}
    }
}

/// Explicitly invoked only against an ephemeral, loopback-only QA Postgres container.
#[cfg(test)] mod integration_tests {
    use super::*;
    use std::sync::{atomic::{AtomicUsize,Ordering},Mutex};
    use axum::{extract::State,http::{Request,StatusCode},body::{Body,to_bytes},response::IntoResponse,Router};
    #[derive(Clone)] struct Fixture {
        status:Arc<AtomicUsize>, calls:Arc<AtomicUsize>, uploads:Arc<Mutex<Vec<String>>>,
        removed:Arc<AtomicUsize>,
    }
    async fn fake(State(f):State<Fixture>,request:Request<Body>)->axum::response::Response {
        let path=request.uri().path().to_string();
        if path=="/optimize" {
            f.calls.fetch_add(1,Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(80)).await;
            return (StatusCode::from_u16(f.status.load(Ordering::SeqCst) as u16).unwrap(),b"%PDF-optimized-fixture".to_vec()).into_response();
        }
        if request.method()==axum::http::Method::GET {
            return (StatusCode::OK,b"%PDF-original-fixture".to_vec()).into_response();
        }
        if request.method()==axum::http::Method::DELETE {
            f.removed.fetch_add(1,Ordering::SeqCst);
            return (StatusCode::OK,"[]").into_response();
        }
        let bytes=to_bytes(request.into_body(),1024).await.unwrap();
        assert_eq!(&bytes[..],b"%PDF-optimized-fixture");
        f.uploads.lock().unwrap().push(path);
        (StatusCode::OK,"{}").into_response()
    }
    async fn new_job(pool:&PgPool,project:Uuid,minutes:i32,status:&str)->Uuid {
        let id=Uuid::new_v4();
        sqlx::query("insert into public.compile_jobs(id,project_id,status,pdf_key,enqueued_at,completed_at) values($1,$2,$3,$4,now()-make_interval(mins=>$5),now()-make_interval(mins=>$5))")
            .bind(id).bind(project).bind(status).bind(format!("{project}/{id}/main.pdf")).bind(minutes).execute(pool).await.unwrap();
        id
    }
    #[tokio::test]
    #[ignore = "requires isolated prims-pdf-preview-qa-db, never the application database"]
    async fn persistent_idle_latest_job_fallback_and_global_concurrency() {
        let pool=sqlx::postgres::PgPoolOptions::new().max_connections(4)
            .connect("postgres://postgres@127.0.0.1:19490/prims_pdf_qa").await.unwrap();
        sqlx::raw_sql("create table public.projects(id uuid primary key,updated_at timestamptz not null);create table public.project_files(project_id uuid,updated_at timestamptz);create table public.compile_jobs(id uuid primary key,project_id uuid references public.projects(id) on delete cascade,status text,pdf_key text,enqueued_at timestamptz,completed_at timestamptz);create table public.prims_pdf_previews(project_id uuid primary key references public.projects(id) on delete cascade,source_job_id uuid not null references public.compile_jobs(id) on delete cascade,status text,artifact_key text,attempts integer not null default 0,retry_after timestamptz not null default now(),updated_at timestamptz not null default now())")
            .execute(&pool).await.unwrap();
        let f=Fixture{status:Arc::new(AtomicUsize::new(200)),calls:Arc::new(AtomicUsize::new(0)),uploads:Arc::new(Mutex::new(vec![])),removed:Arc::new(AtomicUsize::new(0))};
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let app=Router::new().fallback(fake).with_state(f.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let storage=Arc::new(SupabaseStorage::new(&base,"isolated-test-not-a-key").unwrap());
        let project=Uuid::new_v4();
        sqlx::query("insert into public.projects values($1,now())").bind(project).execute(&pool).await.unwrap();
        let job=new_job(&pool,project,5,"success").await;
        process_one(&pool,&storage,&base,"fixture",90).await.unwrap();
        assert_eq!(f.calls.load(Ordering::SeqCst),0,"recent project must stay idle");
        sqlx::query("update public.projects set updated_at=now()-interval '10 minutes'").execute(&pool).await.unwrap();
        let active=new_job(&pool,project,4,"running").await;
        process_one(&pool,&storage,&base,"fixture",90).await.unwrap();
        assert_eq!(f.calls.load(Ordering::SeqCst),0,"foreground compile must win");
        sqlx::query("delete from public.compile_jobs where id=$1").bind(active).execute(&pool).await.unwrap();
        // Concurrent replicas cannot both hold the transaction advisory lock.
        let (a,b)=tokio::join!(process_one(&pool,&storage,&base,"fixture",90),process_one(&pool,&storage,&base,"fixture",90));
        a.unwrap();b.unwrap();
        assert_eq!(f.calls.load(Ordering::SeqCst),1);
        assert_eq!(preview_key(&pool,project,job).await,Some(format!("{project}/{job}/bookshelf-v1.pdf")));
        assert_eq!(preview_key(&pool,Uuid::new_v4(),job).await,None,"cross-project derivative must never select");
        process_one(&pool,&storage,&base,"fixture",90).await.unwrap();
        assert_eq!(f.calls.load(Ordering::SeqCst),1,"ready derivative must survive repeated poll without work");
        let next=new_job(&pool,project,4,"success").await;
        assert_eq!(preview_key(&pool,project,next).await,None,"new job immediately invalidates old derivative");
        process_one(&pool,&storage,&base,"fixture",90).await.unwrap();
        assert!(preview_key(&pool,project,next).await.is_some());
        assert_eq!(f.removed.load(Ordering::SeqCst),1,"superseded private derivative must be removed");
        let failed=new_job(&pool,project,3,"success").await;
        f.status.store(422,Ordering::SeqCst);
        process_one(&pool,&storage,&base,"fixture",90).await.unwrap();
        assert_eq!(preview_key(&pool,project,failed).await,None);
        let calls=f.calls.load(Ordering::SeqCst);
        process_one(&pool,&storage,&base,"fixture",90).await.unwrap();
        assert_eq!(f.calls.load(Ordering::SeqCst),calls,"failed source must not loop forever");
        let busy=new_job(&pool,project,2,"success").await;
        f.status.store(429,Ordering::SeqCst);
        process_one(&pool,&storage,&base,"fixture",90).await.unwrap();
        let status:String=sqlx::query_scalar("select status from public.prims_pdf_previews where project_id=$1").bind(project).fetch_one(&pool).await.unwrap();
        assert_eq!(status,"queued");
        assert_eq!(preview_key(&pool,project,busy).await,None);
        sqlx::query("update public.prims_pdf_previews set retry_after=now()-interval '1 second'").execute(&pool).await.unwrap();
        f.status.store(200,Ordering::SeqCst);
        process_one(&pool,&storage,&base,"fixture",90).await.unwrap();
        assert!(preview_key(&pool,project,busy).await.is_some());
        let original:String=sqlx::query_scalar("select pdf_key from public.compile_jobs where id=$1").bind(busy).fetch_one(&pool).await.unwrap();
        assert_eq!(original,format!("{project}/{busy}/main.pdf"));
        assert!(f.uploads.lock().unwrap().iter().all(|path|path.ends_with("/bookshelf-v1.pdf")),"no original overwritten");
        sqlx::query("delete from public.projects where id=$1").bind(project).execute(&pool).await.unwrap();
        assert_eq!(preview_key(&pool,project,busy).await,None,"project deletion cascades state");
        // Deletion while the sandbox is busy must not wait on background project/FK locks.
        let transient=Uuid::new_v4();
        sqlx::query("insert into public.projects values($1,now()-interval '10 minutes')").bind(transient).execute(&pool).await.unwrap();
        new_job(&pool,transient,5,"success").await;
        let before=f.calls.load(Ordering::SeqCst);
        let (task_pool,task_storage,task_base)=(pool.clone(),storage.clone(),base.clone());
        let task=tokio::spawn(async move{process_one(&task_pool,&task_storage,&task_base,"fixture",90).await});
        for _ in 0..100 {
            if f.calls.load(Ordering::SeqCst)>before {break;}
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(f.calls.load(Ordering::SeqCst)>before);
        tokio::time::timeout(Duration::from_millis(50),sqlx::query("delete from public.projects where id=$1").bind(transient).execute(&pool)).await
            .expect("background optimization must not hold project deletion locks").unwrap();
        assert!(task.await.unwrap().is_err());
        server.abort();pool.close().await;
    }
}

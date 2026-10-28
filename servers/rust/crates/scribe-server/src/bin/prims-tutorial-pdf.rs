//! Render released tutorial projects through the normal authorized compilation service.
#[path = "../services/membership.rs"]
pub(crate) mod membership;
mod services { pub(crate) use crate::membership; }
#[path = "../services/compiles.rs"]
mod compiles;
#[path = "../services/welcome.rs"]
mod welcome;

use anyhow::{bail,Context};
use scribe_compile::CompileQueue;
use scribe_shared::{CreateCompileJobInput,ProjectId,UserId};
use scribe_storage::{Storage,SupabaseStorage,COMPILE_ARTIFACTS_BUCKET};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions,Row};
use std::{sync::Arc,time::{Duration,Instant}};
use uuid::Uuid;
#[tokio::main]
async fn main()->anyhow::Result<()> {
 let args=std::env::args().collect::<Vec<_>>();
 let apply=args.iter().any(|arg|arg=="--apply");
 let pool=PgPoolOptions::new().max_connections(3).connect(&std::env::var("DATABASE_URL")?).await?;
 if let Some(index)=args.iter().position(|arg|arg=="--auto-user") {
  let user=UserId::new(Uuid::parse_str(args.get(index+1).context("missing user id")?)?);
  welcome::enqueue_initial_pdf(&pool,user,&std::env::var("REDIS_URL")?).await?;
  return Ok(());
 }
 let projects=sqlx::query("select r.user_id,r.project_id,p.compiler,p.main_file from public.prims_tutorial_releases r join public.projects p on p.id=r.project_id and p.owner_id=r.user_id where r.release_id='prims-guide-2026-10-28-v1' and p.archived_at is null order by r.user_id")
  .fetch_all(&pool).await?;
 println!("{}",json!({"mode":if apply{"apply"}else{"dry-run"},"tutorialCount":projects.len()}));
 if !apply {return Ok(());}
 let queue=Arc::new(CompileQueue::connect(&std::env::var("REDIS_URL")?).await?);
 let service=compiles::CompileService::new(pool.clone(),queue);
 let storage=SupabaseStorage::new(&std::env::var("SUPABASE_URL")?,&std::env::var("SUPABASE_SERVICE_ROLE_KEY")?)?;
 let mut failed=0;
 for row in projects {
  let user=UserId::new(row.get::<Uuid,_>("user_id"));let project=ProjectId::new(row.get::<Uuid,_>("project_id"));
  // Capture only metadata; the compile pipeline reads the project's current saved files.
  let before:Vec<(Uuid,String,i64,Option<chrono::DateTime<chrono::Utc>>)>=sqlx::query_as("select id,storage_key,size_bytes,updated_at from public.project_files where project_id=$1 order by id").bind(project.into_inner()).fetch_all(&pool).await?;
  let job=service.enqueue(user,project,CreateCompileJobInput{main_file:None}).await.context("enqueue tutorial compilation")?;
  println!("{}",json!({"projectId":project,"jobId":job.id,"status":"queued","engine":row.get::<String,_>("compiler")}));
  let started=Instant::now();
  loop {
   let job=service.get(user,job.id).await?;
   if job.status.is_terminal() {
    let after:Vec<(Uuid,String,i64,Option<chrono::DateTime<chrono::Utc>>)>=sqlx::query_as("select id,storage_key,size_bytes,updated_at from public.project_files where project_id=$1 order by id").bind(project.into_inner()).fetch_all(&pool).await?;
    let mut pdf_bytes=0;let mut tex_passes=0;let mut bibliography_runs=0;
    if let Some(key)=&job.pdf_key {let pdf=storage.download(COMPILE_ARTIFACTS_BUCKET,key).await?;assert!(pdf.starts_with(b"%PDF"));pdf_bytes=pdf.len();}
    if let Some(key)=&job.log_key {let log=storage.download(COMPILE_ARTIFACTS_BUCKET,key).await?;let log=String::from_utf8_lossy(&log);tex_passes=log.matches("This is XeTeX").count()+log.matches("This is LuaHBTeX").count()+log.matches("This is pdfTeX").count();bibliography_runs=log.matches("This is BibTeX").count()+log.matches("INFO - This is Biber").count();}
    let success=pdf_bytes>0&&job.exit_code==Some(0);
    if !success {failed+=1;}
    println!("{}",json!({"projectId":project,"jobId":job.id,"status":job.status,"pdfBytes":pdf_bytes,"texPasses":tex_passes,"bibliographyRuns":bibliography_runs,"warningEntries":job.entries.as_ref().map(|e|e.len()).unwrap_or(0),"sourceMetadataUnchanged":before==after,"durationMs":job.duration_ms}));
    break;
   }
   if started.elapsed()>Duration::from_secs(240){bail!("tutorial compilation timed out; pending job retained");}
   tokio::time::sleep(Duration::from_secs(1)).await;
  }
 }
 if failed>0 {bail!("{failed} tutorial jobs did not produce a successful PDF");}
 Ok(())
}

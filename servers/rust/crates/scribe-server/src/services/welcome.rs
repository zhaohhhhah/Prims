//! Each tutorial release is a new private project. Old projects and welcome markers stay untouched.
use bytes::Bytes;
use scribe_shared::{ApiError,ApiResult,FileId,ProjectId,UserId};
use scribe_storage::{project_file_key,Storage,PROJECT_FILES_BUCKET};
use sqlx::PgPool;
use uuid::Uuid;

struct WelcomeFile {path:&'static str,kind:&'static str,mime:&'static str,bytes:&'static [u8]}
pub const TUTORIAL_RELEASE: &str = "prims-guide-2026-10-28-v1";
pub const TUTORIAL_NAME: &str = "纸上微光 · Prims 写作手册";
const FILES:&[WelcomeFile]=&[
    WelcomeFile{path:"main.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("guide_files/main.tex")},
    WelcomeFile{path:"README.md",kind:"other",mime:"text/markdown; charset=utf-8",bytes:include_bytes!("guide_files/README.md")},
    WelcomeFile{path:"chapters/01-first-page.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("guide_files/chapters/01-first-page.tex")},
    WelcomeFile{path:"chapters/02-writing-with-ai.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("guide_files/chapters/02-writing-with-ai.tex")},
    WelcomeFile{path:"chapters/03-latex-workshop.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("guide_files/chapters/03-latex-workshop.tex")},
    WelcomeFile{path:"chapters/04-history-and-workspace.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("guide_files/chapters/04-history-and-workspace.tex")},
    WelcomeFile{path:"refs.bib",kind:"bib",mime:"text/plain; charset=utf-8",bytes:include_bytes!("guide_files/refs.bib")},
];

pub async fn ensure(pool:&PgPool,storage:&dyn Storage,user:UserId)->ApiResult<()> {
    let seeded:bool=sqlx::query_scalar("select exists(select 1 from public.prims_tutorial_releases where user_id=$1 and release_id=$2)").bind(user.into_inner()).bind(TUTORIAL_RELEASE).fetch_one(pool).await.map_err(db)?;
    if seeded{schedule_initial_pdf(pool,user);return Ok(());}
    let mut tx=pool.begin().await.map_err(db)?;
    sqlx::query("set local lock_timeout='10s'").execute(&mut *tx).await.map_err(db)?;
    // Serialize first visits from different tabs/processes. Marker + project/files commit together.
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 76129041))").bind(user.to_string()).execute(&mut *tx).await.map_err(db)?;
    let seeded:bool=sqlx::query_scalar("select exists(select 1 from public.prims_tutorial_releases where user_id=$1 and release_id=$2)").bind(user.into_inner()).bind(TUTORIAL_RELEASE).fetch_one(&mut *tx).await.map_err(db)?;
    if seeded{schedule_initial_pdf(pool,user);return Ok(());}
    let project=ProjectId::new(Uuid::new_v4());let mut uploaded=Vec::<String>::new();
    let result:ApiResult<()>=async {
        sqlx::query("insert into public.projects(id,name,description,owner_id,template,compiler,main_file,is_public) values($1,$3,'一份可以动手修改的全新写作手册：文件、AI、LaTeX 与版本历史。',$2,'blank','xelatex','main.tex',false)")
            .bind(project.into_inner()).bind(user.into_inner()).bind(TUTORIAL_NAME).execute(&mut *tx).await.map_err(db)?;
        for file in FILES {
            let id=FileId::new(Uuid::new_v4());let key=project_file_key(project,id);
            // Record the key first: a transport failure may occur after the object was stored.
            uploaded.push(key.clone());
            storage.upload(PROJECT_FILES_BUCKET,&key,Bytes::from_static(file.bytes),file.mime).await?;
            sqlx::query("insert into public.project_files(id,project_id,path,type,storage_key,size_bytes,created_by) values($1,$2,$3,$4,$5,$6,$7)")
                .bind(id.into_inner()).bind(project.into_inner()).bind(file.path).bind(file.kind).bind(key).bind(file.bytes.len() as i64).bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;
        }
        sqlx::query("insert into public.prims_tutorial_releases(user_id,release_id,project_id) values($1,$2,$3)").bind(user.into_inner()).bind(TUTORIAL_RELEASE).bind(project.into_inner()).execute(&mut *tx).await.map_err(db)?;
        Ok(())
    }.await;
    if let Err(error) = result {
        // Finish the transaction before compensating external storage writes.
        if let Err(rollback_error) = tx.rollback().await {
            tracing::warn!(?rollback_error, %project, "tutorial rollback unconfirmed; retaining objects");
        } else if let Err(cleanup_error) = storage.remove(PROJECT_FILES_BUCKET, &uploaded).await {
            tracing::warn!(?cleanup_error, %project, "tutorial object cleanup failed");
        }
        return Err(error);
    }
    match tx.commit().await {
        Ok(()) => { schedule_initial_pdf(pool,user); Ok(()) },
        Err(error) => {
            // A lost COMMIT response does not prove a rollback. Deleting objects here
            // could corrupt a successfully created project. Retain them for audit;
            // the durable release marker makes the next visit safe to retry.
            tracing::warn!(%project, "tutorial commit unconfirmed; retaining uploaded objects");
            Err(db(error))
        }
    }
}
/// A first visit returns immediately; actual PDFs use the same authenticated
/// producer and worker as the Compile button. Revisit retries queue transport
/// failures, but never repeatedly compiles a document with a real TeX error.
fn schedule_initial_pdf(pool:&PgPool,user:UserId) {
    let Ok(redis_url)=std::env::var("REDIS_URL") else { return; };
    if redis_url.is_empty() { return; }
    let pool=pool.clone();
    tokio::spawn(async move {
        if let Err(error)=enqueue_initial_pdf(&pool,user,&redis_url).await {
            tracing::warn!(?error,%user,"tutorial initial PDF deferred");
        }
    });
}

pub(crate) async fn enqueue_initial_pdf(pool:&PgPool,user:UserId,redis_url:&str)->ApiResult<()> {
    use scribe_shared::CreateCompileJobInput;
    use std::sync::Arc;
    // Keep one connection available for CompileService::enqueue even in the
    // small operator CLI pool while the advisory-lock transaction is held.
    static INITIAL_PDF_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
    let _permit=INITIAL_PDF_SLOT.acquire().await.map_err(|_|ApiError::internal("初次 PDF 队列已关闭"))?;
    let project:Option<Uuid>=sqlx::query_scalar("select p.id from public.prims_tutorial_releases r join public.projects p on p.id=r.project_id and p.owner_id=r.user_id where r.user_id=$1 and r.release_id=$2 and p.archived_at is null")
        .bind(user.into_inner()).bind(TUTORIAL_RELEASE).fetch_optional(pool).await.map_err(db)?;
    let Some(project)=project else {return Ok(());};
    let mut tx=pool.begin().await.map_err(db)?;
    let locked:bool=sqlx::query_scalar("select pg_try_advisory_xact_lock(hashtextextended($1,76129042))")
        .bind(project.to_string()).fetch_one(&mut *tx).await.map_err(db)?;
    if !locked {return Ok(());}
    let should_enqueue:bool=sqlx::query_scalar(r#"
        select not exists(select 1 from public.compile_jobs where project_id=$1 and status='success' and pdf_key is not null)
          and not exists(select 1 from public.compile_jobs where project_id=$1 and status in ('queued','running'))
          and not exists(select 1 from public.compile_jobs where project_id=$1 and
            (error_message is null or error_message not like 'enqueue failed:%' or enqueued_at > now()-interval '1 minute'))
    "#).bind(project).fetch_one(&mut *tx).await.map_err(db)?;
    if !should_enqueue {return Ok(());}
    let queue=scribe_compile::CompileQueue::connect(redis_url).await.map_err(|error|{
        tracing::warn!(?error,%project,"tutorial compile queue unavailable");
        ApiError::internal("写作手册 PDF 将稍后准备")
    })?;
    // Keep the advisory lock through enqueue so concurrent tabs create one job.
    super::compiles::CompileService::new(pool.clone(),Arc::new(queue))
        .enqueue(user,ProjectId::new(project),CreateCompileJobInput{main_file:None}).await?;
    tx.commit().await.map_err(db)?;
    Ok(())
}

fn db(error:sqlx::Error)->ApiError{tracing::error!(?error,"welcome seed database error");ApiError::internal("无法初始化写作手册，请稍后重试")}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn tutorial_files_are_safe_complete_utf8() {
  let paths=FILES.iter().map(|file|file.path).collect::<std::collections::HashSet<_>>();
  assert_eq!(paths.len(),FILES.len()); assert_eq!(FILES.len(),7);
  for file in FILES {
   assert!(!file.bytes.is_empty()); assert!(!file.path.starts_with('/')); assert!(!file.path.contains(".."));
   assert!(std::str::from_utf8(file.bytes).is_ok());
  }
 }
 #[test]
 fn guide_is_a_new_release_with_complete_chapters() {
  assert_eq!(TUTORIAL_RELEASE,"prims-guide-2026-10-28-v1");
  let main=std::str::from_utf8(FILES[0].bytes).unwrap(); assert!(main.contains("纸上微光")); assert!(main.contains("fontset=fandol"));
  for path in ["chapters/01-first-page","chapters/02-writing-with-ai","chapters/03-latex-workshop","chapters/04-history-and-workspace"] {
   assert!(main.contains(&format!("\\input{{{path}}}"))); assert!(FILES.iter().any(|file|file.path==format!("{path}.tex")));
  }
  assert!(main.contains("\\bibliography{refs}")); assert!(FILES.iter().any(|file|file.path=="refs.bib"));
 }
 #[test]
 fn tutorial_manifest_has_no_personal_deployment_data() {
  for file in FILES { let text=std::str::from_utf8(file.bytes).unwrap();
   assert!(!text.contains("/vol3/")); assert!(!text.contains("/vol1/")); assert!(!text.contains("Bearer ")); assert!(!text.contains("Welcome to Scribe"));
  }
 }
}

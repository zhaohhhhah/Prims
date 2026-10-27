//! Once-per-account, private welcome project. No existing user files are rewritten.
use bytes::Bytes;
use scribe_shared::{ApiError,ApiResult,FileId,ProjectId,UserId};
use scribe_storage::{project_file_key,Storage,PROJECT_FILES_BUCKET};
use sqlx::PgPool;
use uuid::Uuid;

struct WelcomeFile {path:&'static str,kind:&'static str,mime:&'static str,bytes:&'static [u8]}
const FILES:&[WelcomeFile]=&[
    WelcomeFile{path:"main.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("welcome_files/main.tex")},
    WelcomeFile{path:"README.md",kind:"other",mime:"text/markdown; charset=utf-8",bytes:include_bytes!("welcome_files/README.md")},
    WelcomeFile{path:"chapters/01-getting-started.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("welcome_files/chapters/01-getting-started.tex")},
    WelcomeFile{path:"chapters/02-ai-writing.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("welcome_files/chapters/02-ai-writing.tex")},
    WelcomeFile{path:"chapters/03-latex-examples.tex",kind:"tex",mime:"text/plain; charset=utf-8",bytes:include_bytes!("welcome_files/chapters/03-latex-examples.tex")},
    WelcomeFile{path:"references.bib",kind:"bib",mime:"text/plain; charset=utf-8",bytes:include_bytes!("welcome_files/references.bib")},
    WelcomeFile{path:"figures/workflow.png",kind:"image",mime:"image/png",bytes:include_bytes!("welcome_files/figures/workflow.png")},
];

pub async fn ensure(pool:&PgPool,storage:&dyn Storage,user:UserId)->ApiResult<()> {
    let seeded:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_welcome_projects where user_id=$1)").bind(user.into_inner()).fetch_one(pool).await.map_err(db)?;
    if seeded{return Ok(());}
    let mut tx=pool.begin().await.map_err(db)?;
    sqlx::query("set local lock_timeout='10s'").execute(&mut *tx).await.map_err(db)?;
    // Serialize first visits from different tabs/processes. Marker + project/files commit together.
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 76129041))").bind(user.to_string()).execute(&mut *tx).await.map_err(db)?;
    let seeded:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_welcome_projects where user_id=$1)").bind(user.into_inner()).fetch_one(&mut *tx).await.map_err(db)?;
    if seeded{return Ok(());}
    let project=ProjectId::new(Uuid::new_v4());let mut uploaded=Vec::<String>::new();
    let result:ApiResult<()>=async {
        sqlx::query("insert into public.projects(id,name,description,owner_id,template,compiler,main_file,is_public) values($1,'Welcome to Prims!','从 LaTeX 编译到选区 AI 协作：可编辑的中文上手教程。',$2,'blank','tectonic','main.tex',false)")
            .bind(project.into_inner()).bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;
        for file in FILES {
            let id=FileId::new(Uuid::new_v4());let key=project_file_key(project,id);
            storage.upload(PROJECT_FILES_BUCKET,&key,Bytes::from_static(file.bytes),file.mime).await?;
            uploaded.push(key.clone());
            sqlx::query("insert into public.project_files(id,project_id,path,type,storage_key,size_bytes,created_by) values($1,$2,$3,$4,$5,$6,$7)")
                .bind(id.into_inner()).bind(project.into_inner()).bind(file.path).bind(file.kind).bind(key).bind(file.bytes.len() as i64).bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;
        }
        sqlx::query("insert into public.scribe_welcome_projects(user_id,project_id,version) values($1,$2,1)").bind(user.into_inner()).bind(project.into_inner()).execute(&mut *tx).await.map_err(db)?;
        tx.commit().await.map_err(db)?;Ok(())
    }.await;
    if result.is_err()&&!uploaded.is_empty(){if let Err(error)=storage.remove(PROJECT_FILES_BUCKET,&uploaded).await{tracing::warn!(?error,"welcome object cleanup failed");}}
    result
}
fn db(error:sqlx::Error)->ApiError{tracing::error!(?error,"welcome seed database error");ApiError::internal("无法初始化欢迎项目，请稍后重试")}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn template_has_unique_paths_and_valid_image(){
  let paths=FILES.iter().map(|f|f.path).collect::<std::collections::HashSet<_>>();assert_eq!(paths.len(),FILES.len());assert_eq!(FILES.len(),7);
  for f in FILES{assert!(!f.bytes.is_empty());assert!(!f.path.starts_with('/'));assert!(!f.path.contains(".."));if f.kind=="image"{assert!(f.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));}else{assert!(std::str::from_utf8(f.bytes).is_ok());}}
 }
 #[test]fn future_welcome_template_uses_prims_visible_brand(){let main=std::str::from_utf8(FILES[0].bytes).unwrap();assert!(main.contains("Welcome to Prims!"));assert!(!main.contains("Welcome to Scribe!"));let readme=FILES.iter().find(|f|f.path=="README.md").unwrap();assert!(std::str::from_utf8(readme.bytes).unwrap().contains("Welcome to Prims!"));let first=FILES.iter().find(|f|f.path=="chapters/01-getting-started.tex").unwrap();assert!(std::str::from_utf8(first.bytes).unwrap().contains("欢迎来到 Prims"));}
 #[test]fn compiled_guide_inputs_exist_and_citation_is_explicitly_a_placeholder(){
  let main=std::str::from_utf8(FILES[0].bytes).unwrap();assert!(main.contains("fontset=fandol"));
  for path in ["chapters/01-getting-started","chapters/02-ai-writing","chapters/03-latex-examples"]{assert!(main.contains(&format!("\\input{{{path}}}")));assert!(FILES.iter().any(|f|f.path==format!("{path}.tex")));}
  let bib=FILES.iter().find(|f|f.kind=="bib").unwrap();assert!(std::str::from_utf8(bib.bytes).unwrap().contains("Teaching placeholder only"));
 }
}

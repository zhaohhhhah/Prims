//! Immutable-source, resumable private bookshelf WebP batches. Editor/download PDFs are unchanged.
use std::{sync::Arc,time::Duration,io::{Cursor,Read}};
use bytes::Bytes;
use scribe_shared::{ApiError,ApiResult};
use scribe_storage::{Storage,SupabaseStorage,COMPILE_ARTIFACTS_BUCKET};
use serde::{Serialize,Deserialize};
use sqlx::{PgPool,Row};
use uuid::Uuid;
const PROFILE:&str="webp-v1";
const MAX_PDF:u64=64*1024*1024;
const MAX_ZIP:usize=33*1024*1024;
const GLOBAL_LOCK:i64=0x5052494d53504446;
const CANDIDATES:&str=r#"select j.id,j.project_id,j.pdf_key,
 greatest(p.updated_at,j.completed_at,coalesce((select max(updated_at) from public.project_files where project_id=p.id),p.updated_at)) activity,
 v.source_job_id old_job,v.webp_status,v.webp_profile,v.pages,v.page_count,v.batch_size,
 case when v.reader_until>now() then 2 when v.editor_until>now() then 1 else 0 end priority,
 case when v.reader_until>now() then v.reader_since when v.editor_until>now() then v.editor_since else coalesce(v.queued_at,j.completed_at) end fifo
 from public.projects p join lateral(select id,project_id,pdf_key,completed_at from public.compile_jobs where project_id=p.id and status='success' and pdf_key is not null order by enqueued_at desc,id desc limit 1)j on true
 left join public.prims_pdf_previews v on v.project_id=p.id
 where not exists(select 1 from public.compile_jobs where status in ('queued','running'))
 and(v.project_id is null or v.source_job_id<>j.id or v.webp_profile<>'webp-v1' or(v.webp_status='queued' and v.retry_after<=now()))"#;
fn db(_:sqlx::Error)->ApiError{ApiError::internal("Preview state unavailable")}
fn idle_seconds(raw:Option<String>)->u64{raw.and_then(|s|s.parse().ok()).filter(|n|(10..=86400).contains(n)).unwrap_or(90)}
fn configuration()->Option<(String,String)>{let raw=std::env::var("PRIMS_COMPILE_BROKER_URL").ok()?;let token=std::env::var("PRIMS_COMPILE_BROKER_TOKEN").ok()?;let url=url::Url::parse(&raw).ok()?;(url.scheme()=="http"&&matches!(url.host_str(),Some("127.0.0.1")|Some("localhost")|Some("[::1]")|Some("::1"))&&url.port()==Some(19342)&&url.username().is_empty()&&url.password().is_none()&&url.query().is_none()&&url.fragment().is_none()&&matches!(url.path(),""|"/")&&token.len()>=32).then(||(raw.trim_end_matches('/').into(),token))}
pub fn enabled()->bool{std::env::var("PRIMS_PDF_PREVIEW_WORKER").ok().as_deref()==Some("enabled")&&configuration().is_some()}
#[derive(Clone,Serialize,Deserialize,Debug)]#[serde(deny_unknown_fields)]struct Page{page:u32,key:String,width:u32,height:u32}
#[derive(Serialize)]#[serde(rename_all="camelCase")]pub struct PageUrl{page:u32,pub url:String,width:u32,height:u32}
pub struct Preview{pub status:&'static str,pub pages:Option<Vec<PageUrl>>}
fn prefix(project:Uuid,job:Uuid)->String{format!("{project}/{job}/bookshelf-webp-v1/")}
fn valid_pages(pages:&[Page],project:Uuid,job:Uuid)->bool{pages.len()<=2048&&pages.iter().enumerate().all(|(i,p)|p.page==i as u32+1&&p.width>0&&p.width<=1200&&p.height>0&&p.height<=2400&&p.key==format!("{}page-{:06}.webp",prefix(project,job),p.page))}
/// Called only after membership and latest-successful-job checks in the route.
pub async fn lookup(pool:&PgPool,storage:&SupabaseStorage,project:Uuid,job:Uuid)->Preview{
 let row=sqlx::query("select webp_status,page_count,pages,webp_manifest_key from public.prims_pdf_previews where project_id=$1 and source_job_id=$2 and webp_profile='webp-v1'").bind(project).bind(job).fetch_optional(pool).await.ok().flatten();
 let fallback=||Preview{status:if enabled(){"pending"}else{"disabled"},pages:None};let Some(row)=row else{return fallback();};
 let status:String=row.get("webp_status");if status!="ready"{return Preview{status:if status=="failed"{"failed"}else if enabled(){"pending"}else{"disabled"},pages:None};}
 let pages:Vec<Page>=match serde_json::from_value(row.get("pages")){Ok(v)=>v,Err(_)=>return fallback()};let count:Option<i32>=row.get("page_count");let manifest:Option<String>=row.get("webp_manifest_key");
 if count!=Some(pages.len() as i32)||pages.is_empty()||!valid_pages(&pages,project,job)||manifest!=Some(format!("{}manifest.json",prefix(project,job))){return fallback();}
 let keys=pages.iter().map(|p|p.key.clone()).collect::<Vec<_>>();let urls=match storage.signed_urls(COMPILE_ARTIFACTS_BUCKET,&keys,300).await{Ok(v)=>v,Err(_)=>return fallback()};
 Preview{status:"ready",pages:Some(pages.into_iter().zip(urls).map(|(p,url)|PageUrl{page:p.page,url,width:p.width,height:p.height}).collect())}
}
/// Independent expirations prevent an editor heartbeat extending/downgrading a reader.
pub async fn record_interest(pool:&PgPool,project:Uuid,job:Uuid,kind:&str)->ApiResult<()>{
 let (until,since)=match kind{"reader"=>("reader_until","reader_since"),"editor"=>("editor_until","editor_since"),_=>return Err(ApiError::validation("Invalid preview interest"))};
 let sql=format!("insert into public.prims_pdf_previews(project_id,source_job_id,status,webp_status,{until},{since}) values($1,$2,'failed','queued',now()+interval '90 seconds',now()) on conflict(project_id) do update set {since}=case when prims_pdf_previews.{until}>now() then prims_pdf_previews.{since} else now() end,{until}=now()+interval '90 seconds'");
 sqlx::query(&sql).bind(project).bind(job).execute(pool).await.map_err(db)?;Ok(())
}
pub fn start_worker(state:crate::state::AppState){
 if !enabled(){return;}let(Some(database),Some(storage),Some((base,token)))=(state.db(),state.inner.storage.clone(),configuration())else{return;};let pool=database.pool().clone();let idle=idle_seconds(std::env::var("PRIMS_PDF_PREVIEW_IDLE_SECONDS").ok());
 tokio::spawn(async move{loop{let result=process_one(&pool,&storage,&base,&token,idle).await;let delay=match result{Ok(true)=>Duration::from_millis(25),Ok(false)=>Duration::from_secs(15),Err(error)=>{tracing::warn!(?error,"WebP batch deferred; original PDF remains available");Duration::from_secs(15)}};tokio::time::sleep(delay).await;}});
}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]struct Batch{version:u32,total_pages:u32,start_page:u32,pages:Vec<BatchPage>}
#[derive(Deserialize)]#[serde(deny_unknown_fields)]struct BatchPage{page:u32,width:u32,height:u32,entry:String,size:u64}
struct Rendered{total:u32,pages:Vec<(BatchPage,Bytes)>}
fn webp_dimensions(bytes:&[u8])->Option<(u32,u32)>{
 if bytes.len()<30{return None;}let dims=match &bytes[12..16]{
  b"VP8 " if &bytes[23..26]==b"\x9d\x01\x2a"=>(u16::from_le_bytes(bytes[26..28].try_into().ok()?)as u32&0x3fff,u16::from_le_bytes(bytes[28..30].try_into().ok()?)as u32&0x3fff),
  b"VP8X" if bytes[20]&2==0=>(u32::from_le_bytes([bytes[24],bytes[25],bytes[26],0])+1,u32::from_le_bytes([bytes[27],bytes[28],bytes[29],0])+1),
  b"VP8L" if bytes[20]==0x2f=>{let bits=u32::from_le_bytes(bytes[21..25].try_into().ok()?);((bits&0x3fff)+1,((bits>>14)&0x3fff)+1)},_=>return None};Some(dims)
}
fn decode_batch(data:Vec<u8>,start:u32,count:u32)->ApiResult<Rendered>{
 let fail=||ApiError::internal("Invalid WebP page batch");if data.len()>MAX_ZIP{return Err(fail());}let mut zip=zip::ZipArchive::new(Cursor::new(data)).map_err(|_|fail())?;if zip.len()<2||zip.len()>9{return Err(fail());}
 let manifest={let mut f=zip.by_name("manifest.json").map_err(|_|fail())?;if f.size()>65536||f.compression()!=zip::CompressionMethod::Stored{return Err(fail());}let mut text=String::new();f.read_to_string(&mut text).map_err(|_|fail())?;serde_json::from_str::<Batch>(&text).map_err(|_|fail())?};
 if manifest.version!=1||!(1..=2048).contains(&manifest.total_pages)||manifest.start_page!=start||start>manifest.total_pages||manifest.pages.len()!=count.min(manifest.total_pages.saturating_sub(start)+1) as usize||manifest.pages.is_empty()||zip.len()!=manifest.pages.len()+1{return Err(fail());}
 let mut pages=vec![];for(i,p)in manifest.pages.into_iter().enumerate(){if p.page!=start+i as u32||p.entry!=format!("page-{:06}.webp",p.page)||p.width==0||p.width>1200||p.height==0||p.height>2400||p.size==0||p.size>4*1024*1024{return Err(fail());}
  let mut f=zip.by_name(&p.entry).map_err(|_|fail())?;if f.size()!=p.size||f.compression()!=zip::CompressionMethod::Stored{return Err(fail());}let mut bytes=vec![];f.read_to_end(&mut bytes).map_err(|_|fail())?;if bytes.len()<12||&bytes[..4]!=b"RIFF"||&bytes[8..12]!=b"WEBP"||u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize+8!=bytes.len()||webp_dimensions(&bytes)!=Some((p.width,p.height)){return Err(fail());}pages.push((p,Bytes::from(bytes)));
 }Ok(Rendered{total:manifest.total_pages,pages})
}
enum ResultBatch{Ready(Rendered),Busy,Smaller,Failed}
async fn render(storage:&SupabaseStorage,base:&str,token:&str,key:&str,start:u32,count:u32)->ApiResult<ResultBatch>{
 let dir=tempfile::tempdir().map_err(|_|ApiError::internal("Preview scratch unavailable"))?;let path=dir.path().join("input.pdf");storage.download_to_path(COMPILE_ARTIFACTS_BUCKET,key,&path,MAX_PDF).await?;let body=tokio::fs::read(path).await.map_err(|_|ApiError::internal("Preview input unavailable"))?;if !body.starts_with(b"%PDF-"){return Ok(ResultBatch::Failed);}
 let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(125)).build().map_err(|_|ApiError::internal("Preview connection unavailable"))?;
 let mut reply=client.post(format!("{base}/render-pages?start={start}&count={count}")).bearer_auth(token).header(reqwest::header::CONTENT_TYPE,"application/pdf").body(body).send().await.map_err(|_|ApiError::internal("WebP worker unavailable"))?;
 match reply.status().as_u16(){429|503=>return Ok(ResultBatch::Busy),408=>return Ok(ResultBatch::Smaller),200=>{},_=>return Ok(ResultBatch::Failed)}let mut data=vec![];while let Some(chunk)=reply.chunk().await.map_err(|_|ApiError::internal("WebP batch interrupted"))?{if data.len()+chunk.len()>MAX_ZIP{return Err(ApiError::internal("WebP batch too large"));}data.extend_from_slice(&chunk);}Ok(ResultBatch::Ready(decode_batch(data,start,count)?))
}
async fn queue_cleanup(pool:&PgPool,project:Uuid,job:Uuid){let _=sqlx::query("insert into public.prims_webp_cleanup(project_id,source_job_id)values($1,$2)on conflict do nothing").bind(project).bind(job).execute(pool).await;}
async fn cleanup_one(pool:&PgPool,storage:&SupabaseStorage){
 let row=sqlx::query("select c.project_id,c.source_job_id from public.prims_webp_cleanup c where c.retry_after<=now() and not exists(select 1 from public.prims_pdf_previews v where v.project_id=c.project_id and v.source_job_id=c.source_job_id and v.webp_status<>'failed') order by c.retry_after limit 1").fetch_optional(pool).await.ok().flatten();let Some(row)=row else{return;};let p:Uuid=row.get("project_id");let j:Uuid=row.get("source_job_id");let pre=prefix(p,j);
 let keys=sqlx::query_scalar::<_,String>("select name from storage.objects where bucket_id=$1 and left(name,length($2))=$2 limit 100").bind(COMPILE_ARTIFACTS_BUCKET).bind(pre).fetch_all(pool).await.unwrap_or_default();
 if keys.is_empty(){let _=sqlx::query("delete from public.prims_webp_cleanup where project_id=$1 and source_job_id=$2").bind(p).bind(j).execute(pool).await;}else if storage.remove(COMPILE_ARTIFACTS_BUCKET,&keys).await.is_err(){let _=sqlx::query("update public.prims_webp_cleanup set retry_after=now()+interval '30 seconds' where project_id=$1 and source_job_id=$2").bind(p).bind(j).execute(pool).await;}
}
async fn process_one(pool:&PgPool,storage:&Arc<SupabaseStorage>,base:&str,token:&str,idle:u64)->ApiResult<bool>{
 let mut tx=pool.begin().await.map_err(db)?;let locked:bool=sqlx::query_scalar("select pg_try_advisory_xact_lock($1)").bind(GLOBAL_LOCK).fetch_one(&mut *tx).await.map_err(db)?;if !locked{return Ok(false);}
 cleanup_one(pool,storage).await;
 let sql=format!("select * from({CANDIDATES})c where priority>0 or activity<=now()-make_interval(secs=>$1::double precision) order by priority desc,fifo asc nulls last,id limit 1");let Some(row)=sqlx::query(&sql).bind(idle as f64).fetch_optional(&mut *tx).await.map_err(db)?else{return Ok(false);};
 let project:Uuid=row.get("project_id");let job:Uuid=row.get("id");let original:String=row.get("pdf_key");let old:Option<Uuid>=row.get("old_job");let profile:Option<String>=row.get("webp_profile");let continuing=old==Some(job)&&profile.as_deref()==Some(PROFILE);
 let mut pages:Vec<Page>=if continuing{serde_json::from_value(row.try_get::<serde_json::Value,_>("pages").unwrap_or(serde_json::json!([]))).unwrap_or_default()}else{vec![]};if !valid_pages(&pages,project,job){pages.clear();}
 let mut total:Option<i32>=if continuing{row.try_get("page_count").ok().flatten()}else{None};let mut batch:i32=if continuing{row.try_get("batch_size").unwrap_or(8)}else{8};batch=batch.clamp(1,8);let start=pages.len()as u32+1;
 let result=render(storage,base,token,&original,start,batch as u32).await;let(mut status,mut retry)=("queued",0i32);let mut uploaded=vec![];let mut manifest_key=None;
 match result{
  Ok(ResultBatch::Ready(rendered))=>{if total.is_some_and(|v|v!=rendered.total as i32){status="failed";}else{total=Some(rendered.total as i32);for(meta,bytes)in rendered.pages{let key=format!("{}{}",prefix(project,job),meta.entry);if storage.upload(COMPILE_ARTIFACTS_BUCKET,&key,bytes,"image/webp").await.is_err(){status="failed";break;}uploaded.push(key.clone());pages.push(Page{page:meta.page,key,width:meta.width,height:meta.height});}if status!="failed"&&total==Some(pages.len()as i32){let key=format!("{}manifest.json",prefix(project,job));let manifest=serde_json::json!({"version":1,"jobId":job,"pageCount":pages.len(),"pages":pages});if storage.upload(COMPILE_ARTIFACTS_BUCKET,&key,Bytes::from(serde_json::to_vec(&manifest).unwrap()),"application/json").await.is_ok(){uploaded.push(key.clone());manifest_key=Some(key);status="ready";}else{status="failed";}}}},
  Ok(ResultBatch::Busy)=>retry=15,
  Ok(ResultBatch::Smaller)if batch>1=>batch=(batch/2).max(1),
  Ok(ResultBatch::Smaller)|Ok(ResultBatch::Failed)=>status="failed",
  Err(_)=>retry=30,
 }
 // Never publish partial pages, stale sources or bytes produced after deletion.
 let valid:bool=sqlx::query_scalar("select exists(select 1 from public.projects p where p.id=$1 and $2=(select id from public.compile_jobs where project_id=p.id and status='success' and pdf_key is not null order by enqueued_at desc,id desc limit 1)) and not exists(select 1 from public.compile_jobs where status in ('queued','running'))").bind(project).bind(job).fetch_one(&mut *tx).await.map_err(db)?;
 if !valid{if !uploaded.is_empty(){let _=storage.remove(COMPILE_ARTIFACTS_BUCKET,&uploaded).await;}queue_cleanup(pool,project,job).await;return Ok(false);}
 if status=="failed"{pages.clear();total=None;manifest_key=None;}
 let sql=r#"insert into public.prims_pdf_previews(project_id,source_job_id,status,webp_status,webp_profile,pages,page_count,batch_size,webp_manifest_key,retry_after) values($1,$2,'failed',$3,'webp-v1',$4,$5,$6,$7,now()+make_interval(secs=>$8::double precision)) on conflict(project_id)do update set status=case when prims_pdf_previews.source_job_id=$2 then prims_pdf_previews.status else 'failed' end,artifact_key=case when prims_pdf_previews.source_job_id=$2 then prims_pdf_previews.artifact_key else null end,source_job_id=$2,webp_status=$3,webp_profile='webp-v1',pages=$4,page_count=$5,batch_size=$6,webp_manifest_key=$7,retry_after=now()+make_interval(secs=>$8::double precision),queued_at=case when prims_pdf_previews.source_job_id=$2 then prims_pdf_previews.queued_at else now()end,updated_at=now()"#;
 let write=sqlx::query(sql).bind(project).bind(job).bind(status).bind(serde_json::to_value(&pages).unwrap()).bind(total).bind(batch).bind(&manifest_key).bind(retry as f64).execute(&mut *tx).await;
 if write.is_err()||tx.commit().await.is_err(){let _=storage.remove(COMPILE_ARTIFACTS_BUCKET,&uploaded).await;queue_cleanup(pool,project,job).await;return Err(ApiError::internal("WebP publication deferred"));}
 if let Some(old)=old.filter(|id|*id!=job){queue_cleanup(pool,project,old).await;}if status=="failed"{queue_cleanup(pool,project,job).await;}
 Ok(retry==0)
}
#[cfg(test)]mod tests{use super::*;#[test]fn idle_bounds(){assert_eq!(idle_seconds(None),90);assert_eq!(idle_seconds(Some("120".into())),120);for raw in["0","1","bad","86401"]{assert_eq!(idle_seconds(Some(raw.into())),90);}}
 #[test]fn stored_pages_require_contiguous_scoped_keys(){let p=Uuid::new_v4();let j=Uuid::new_v4();let mut pages=vec![Page{page:1,key:format!("{}page-000001.webp",prefix(p,j)),width:1200,height:1600}];assert!(valid_pages(&pages,p,j));pages[0].page=2;assert!(!valid_pages(&pages,p,j));pages[0].page=1;pages[0].key="other/path".into();assert!(!valid_pages(&pages,p,j));}
 #[test]fn archives_fail_closed(){assert!(decode_batch(b"bad".to_vec(),1,8).is_err());}
}

//! Project-scoped read-only compile evidence. No caller-supplied storage/host paths.
use crate::state::AppState;
use scribe_ai::adapter::NativeToolDefinition;
use scribe_shared::{ApiError,ApiResult,CompileJob,CompileLogLevel,ErrorCode,ProjectId,UserId};
use scribe_storage::{compile_artifact_key,Storage,COMPILE_ARTIFACTS_BUCKET};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use uuid::Uuid;

const MAX_LOG_BYTES:usize=32*1024*1024;
const DEFAULT_PAGE:usize=16_000;
const MAX_PAGE:usize=32_000;
pub const ARGUMENTS:&[&str]=&["jobId","offset","limit"];
pub fn normalize_call(value:&Value)->ApiResult<Value>{
 let obj=value.as_object().ok_or_else(||ApiError::validation("编译日志工具参数必须是完整对象"))?;
 if obj.get("tool").and_then(Value::as_str)!=Some("read_compile_log")||obj.keys().any(|key|key!="tool"&&!ARGUMENTS.contains(&key.as_str())){return Err(ApiError::validation("编译日志只接受jobId、offset、limit；不能指定其他项目、用户、URL或宿主路径"));}
 if let Some(id)=obj.get("jobId"){if !id.as_str().is_some_and(|id|Uuid::parse_str(id).is_ok()){return Err(ApiError::validation("jobId必须是当前项目编译任务的UUID"));}}
 for (key,min,max) in [("offset",0,MAX_LOG_BYTES),("limit",1,MAX_PAGE)]{if let Some(value)=obj.get(key){if !value.as_u64().is_some_and(|v|v>=min as u64&&v<=max as u64){return Err(ApiError::validation("编译日志分页位置或页大小无效"));}}}
 Ok(value.clone())
}
pub fn native_tool_definition()->NativeToolDefinition{NativeToolDefinition{
 name:"read_compile_log".into(),
 description:"Read actual saved compilation evidence in this project: selected job UUID or latest job, real engine/main file/status, diagnostic counts and a resumable raw-log page. offset is a UTF-8 byte cursor returned by nextCall; follow every page needed, including the tail. A pending job/log is explicitly pending and can be read again. Reveals PDF artifact presence; rendered PDF contents require the editor preview. No host paths, URLs, user/project override or document writes.".into(),
 parameters:json!({"type":"object","properties":{"jobId":{"type":"string","format":"uuid"},"offset":{"type":"integer","minimum":0,"maximum":MAX_LOG_BYTES},"limit":{"type":"integer","minimum":1,"maximum":MAX_PAGE}},"additionalProperties":false}),
}}
async fn authorize(state:&AppState,user:UserId,project:ProjectId)->ApiResult<sqlx::PgPool>{
 let pool=state.db().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"编译日志数据库暂不可用"))?.pool().clone();
 super::membership::assert_member(&pool,user,project).await?;
 if !super::agent_library::AgentLibrary::new(pool.clone()).preferences(user).await?.diagnostics_enabled{return Err(ApiError::forbidden("本账号已关闭编译诊断与日志工具"));}
 Ok(pool)
}
pub async fn run(state:&AppState,user:UserId,project:ProjectId,call:&Value)->ApiResult<Value>{
 let call=normalize_call(call)?;let pool=authorize(state,user,project).await?;
 let id=call.get("jobId").and_then(Value::as_str).map(Uuid::parse_str).transpose().map_err(|_|ApiError::validation("jobId无效"))?;
 let job=super::compiles::project_job(&pool,user,project,id).await?;
 let project_compiler=sqlx::query_scalar::<_,String>("select compiler::text from public.projects where id=$1").bind(project.into_inner()).fetch_one(&pool).await.map_err(|_|ApiError::not_found("项目不存在"))?;
 let mut result=metadata(&job,&project_compiler);
 if let Some(key)=job.log_key.as_deref(){
  let expected=compile_artifact_key(project,&job.id.to_string(),"compile.log");
  if key!=expected{return Err(ApiError::forbidden("编译日志不属于此项目任务的受控存储位置"));}
  let storage=state.inner.storage.as_ref().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"编译日志存储暂不可用"))?;
  match storage.download(COMPILE_ARTIFACTS_BUCKET,&expected).await{
   Ok(bytes)=>{
    if bytes.len()>MAX_LOG_BYTES{return Err(ApiError::validation("编译日志超过32MiB，未向模型静默截断；请通过日志下载查看原件"));}
    let lossy=String::from_utf8_lossy(&bytes);let offset=call.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;let limit=call.get("limit").and_then(Value::as_u64).unwrap_or(DEFAULT_PAGE as u64) as usize;
    let page=log_page(&lossy,job.id.into_inner(),offset,limit)?;
    for (key,value) in page.as_object().unwrap(){result[key]=value.clone();}
    result["logSha256"]=json!(format!("{:x}",Sha256::digest(&bytes)));
    result["decodingLossy"]=json!(matches!(lossy,std::borrow::Cow::Owned(_)));
   },
   Err(error) if error.code==ErrorCode::NotFound=>{result["logPending"]=json!(true);result["retryAfterMs"]=json!(1500);result["note"]=json!("日志对象正在后台上传；使用相同jobId稍后重新读取，不要把暂未读到日志当成编译失败。");},
   Err(_)=>return Err(ApiError::new(ErrorCode::ServiceUnavailable,"编译日志暂时无法读取；本次未向模型提供伪造或截断日志")),
  }
 }
 authorize(state,user,project).await?;
 Ok(result)
}
fn metadata(job:&CompileJob,project_compiler:&str)->Value{
 let entries=job.entries.as_deref().unwrap_or(&[]);let warnings=entries.iter().filter(|e|e.level==CompileLogLevel::Warning).count();let errors=entries.iter().filter(|e|e.level==CompileLogLevel::Error).count();
 json!({"tool":"read_compile_log","ok":true,"jobId":job.id,"projectCompiler":project_compiler,"engine":job.engine,"mainFile":job.main_file,"status":job.status,"exitCode":job.exit_code,"jobComplete":job.status.is_terminal(),"compileSucceeded":job.status==scribe_shared::CompileJobStatus::Success,"warningCount":warnings,"errorCount":errors,"diagnostics":entries.iter().take(100).collect::<Vec<_>>(),"diagnosticsTruncated":entries.len()>100,"diagnosticsTotal":entries.len(),"errorMessage":job.error_message,"durationMs":job.duration_ms,"enqueuedAt":job.enqueued_at,"completedAt":job.completed_at,"artifacts":{"pdfAvailable":job.pdf_key.is_some(),"logRecorded":job.log_key.is_some(),"synctexAvailable":job.synctex_key.is_some()},"logAvailable":false,"logUnavailable":job.status.is_terminal()&&job.log_key.is_none(),"logPending":!job.status.is_terminal(),"content":"","rangeComplete":false,"nextCall":null,"readOnly":true,"untrusted":true,"note":"Compile status and engine come from the actual job. Warnings can coexist with a successful PDF. Raw final log is authoritative for unresolved issues; older stored diagnostic entries may include earlier-pass warnings. Unsaved editor text and pending proposals were not compiled."})
}
fn log_page(text:&str,job:Uuid,offset:usize,limit:usize)->ApiResult<Value>{
 if offset>text.len()||!text.is_char_boundary(offset)||limit==0||limit>MAX_PAGE{return Err(ApiError::validation("日志续读位置必须使用服务器返回的UTF-8字节边界"));}
 let mut end=offset;for character in text[offset..].chars().take(limit){end+=character.len_utf8();}
 let more=end<text.len();Ok(json!({"logAvailable":true,"logPending":false,"content":&text[offset..end],"offset":offset,"endOffset":end,"totalBytes":text.len(),"rangeComplete":!more,"hasMore":more,"nextOffset":if more{Some(end)}else{None},"nextCall":if more{Some(json!({"tool":"read_compile_log","jobId":job,"offset":end,"limit":limit}))}else{None}}))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn only_scoped_job_and_bounded_paging_are_accepted(){for call in [json!({"tool":"read_compile_log"}),json!({"tool":"read_compile_log","jobId":Uuid::new_v4(),"offset":0,"limit":4096})]{assert!(normalize_call(&call).is_ok());}for call in [json!({"tool":"read_compile_log","projectId":Uuid::new_v4()}),json!({"tool":"read_compile_log","path":"/etc/passwd"}),json!({"tool":"read_compile_log","url":"https://evil.invalid"}),json!({"tool":"read_compile_log","jobId":"../other"}),json!({"tool":"read_compile_log","offset":-1}),json!({"tool":"read_compile_log","limit":0})]{assert!(normalize_call(&call).is_err());}}
 #[test]fn unicode_pages_reconstruct_every_byte_and_tail(){let text="中😀".repeat(40_000)+"\nFINAL_WARNING_TAIL";let mut offset=0;let mut joined=String::new();let id=Uuid::new_v4();loop{let p=log_page(&text,id,offset,4096).unwrap();joined.push_str(p["content"].as_str().unwrap());if p["nextOffset"].is_null(){assert_eq!(p["rangeComplete"],true);break;}offset=p["nextOffset"].as_u64().unwrap() as usize;assert!(text.is_char_boundary(offset));assert_eq!(p["nextCall"]["jobId"],json!(id));}assert_eq!(joined,text);assert!(log_page("中",id,1,100).is_err());}
}

#[cfg(test)]mod isolated_integration_tests{
 use super::*;
 use std::sync::{Arc,atomic::{AtomicBool,AtomicUsize,Ordering}};
 use tokio::io::{AsyncReadExt,AsyncWriteExt};
 #[tokio::test]
 #[ignore="requires the disposable network-none prims_compile_debug_test PostgreSQL fixture"]
 async fn real_postgres_and_storage_enforce_scope_preferences_and_dynamic_full_logs(){
  let url=std::env::var("PRIMS_COMPILE_LOG_TEST_DB").expect("explicit disposable DB URL required");
  assert!(url.contains("@127.0.0.1:5432/prims_compile_debug_test")&&!url.contains("15432"));
  let db=crate::db::Db::connect(&url).await.unwrap();let pool=db.pool().clone();
  sqlx::raw_sql("create table public.projects(id uuid primary key,owner_id uuid not null,compiler text not null,main_file text not null);create table public.project_members(project_id uuid,user_id uuid,role text,invite_accepted_at timestamptz);create table public.scribe_agent_preferences(user_id uuid primary key,memory_enabled bool default true,skills_enabled bool default true,diagnostics_enabled bool default true,search_enabled bool default true);create table public.compile_jobs(id uuid primary key,project_id uuid,triggered_by uuid,status text,engine text,main_file text,exit_code int,pdf_key text,log_key text,synctex_key text,error_message text,entries jsonb,duration_ms int,enqueued_at timestamptz default now(),started_at timestamptz,completed_at timestamptz);").execute(&pool).await.unwrap();
  let owner=UserId::new(Uuid::new_v4());let viewer=UserId::new(Uuid::new_v4());let outsider=UserId::new(Uuid::new_v4());let project=ProjectId::new(Uuid::new_v4());let other=ProjectId::new(Uuid::new_v4());let first=Uuid::new_v4();let foreign=Uuid::new_v4();let pending=Uuid::new_v4();
  for p in [project,other]{sqlx::query("insert into public.projects values($1,$2,'lualatex','current.tex')").bind(p.into_inner()).bind(owner.into_inner()).execute(&pool).await.unwrap();}
  sqlx::query("insert into public.project_members values($1,$2,'viewer',now())").bind(project.into_inner()).bind(viewer.into_inner()).execute(&pool).await.unwrap();
  sqlx::query("insert into public.scribe_agent_preferences(user_id) values($1),($2)").bind(owner.into_inner()).bind(viewer.into_inner()).execute(&pool).await.unwrap();
  for (id,p) in [(first,project),(foreign,other)]{
   sqlx::query("insert into public.compile_jobs(id,project_id,triggered_by,status,engine,main_file,exit_code,pdf_key,log_key,entries,started_at,completed_at) values($1,$2,$3,'success','xelatex','main.tex',0,$4,$5,$6,now(),now())")
    .bind(id).bind(p.into_inner()).bind(owner.into_inner()).bind(compile_artifact_key(p,&id.to_string(),"output.pdf")).bind(compile_artifact_key(p,&id.to_string(),"compile.log")).bind(json!([{"level":"warning","message":"Overfull box remains"}])).execute(&pool).await.unwrap();
  }
  sqlx::query("insert into public.compile_jobs(id,project_id,triggered_by,status,engine,main_file,enqueued_at) values($1,$2,$3,'queued','lualatex','current.tex',now()+interval '1 second')").bind(pending).bind(project.into_inner()).bind(owner.into_inner()).execute(&pool).await.unwrap();
  let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
  let requests=Arc::new(AtomicUsize::new(0));let ready=Arc::new(AtomicBool::new(true));let revoke=Arc::new(AtomicBool::new(false));
  let original="中😀".repeat(8000)+"\nFINAL_LOG_TAIL";let payload=Arc::new(original.clone());
  let (seen,available,revoke_pref,data,storage_pool)=(requests.clone(),ready.clone(),revoke.clone(),payload.clone(),pool.clone());
  let server=tokio::spawn(async move{loop{let Ok((mut socket,_))=listener.accept().await else{break};let mut header=vec![0u8;8192];let _=socket.read(&mut header).await;seen.fetch_add(1,Ordering::SeqCst);if revoke_pref.swap(false,Ordering::SeqCst){sqlx::query("update public.scribe_agent_preferences set diagnostics_enabled=false where user_id=$1").bind(owner.into_inner()).execute(&storage_pool).await.unwrap();}let (status,body)=if available.load(Ordering::SeqCst){("200 OK",data.as_bytes())}else{("404 Not Found",b"{\"error\":\"not found\"}".as_slice())};let response=format!("HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());socket.write_all(response.as_bytes()).await.unwrap();socket.write_all(body).await.unwrap();}});
  let storage=Arc::new(scribe_storage::SupabaseStorage::new(&format!("http://{address}"),"disposable-test-only-storage-key").unwrap());
  let state=AppState::new(Some(db.clone()),Some(storage),None,None,crate::response_cache::ResponseCache::with_flags(None,false,false));
  let call=json!({"tool":"read_compile_log","jobId":first,"limit":2000});
  assert!(matches!(run(&state,outsider,project,&call).await,Err(ApiError{code:ErrorCode::NotFound,..})));
  assert!(matches!(run(&state,owner,project,&json!({"tool":"read_compile_log","jobId":foreign})).await,Err(ApiError{code:ErrorCode::NotFound,..})));
  assert_eq!(requests.load(Ordering::SeqCst),0);
  sqlx::query("update public.compile_jobs set log_key=$2 where id=$1").bind(first).bind(compile_artifact_key(other,&foreign.to_string(),"compile.log")).execute(&pool).await.unwrap();
  assert!(matches!(run(&state,owner,project,&call).await,Err(ApiError{code:ErrorCode::Forbidden,..})));assert_eq!(requests.load(Ordering::SeqCst),0);
  sqlx::query("update public.compile_jobs set log_key=$2 where id=$1").bind(first).bind(compile_artifact_key(project,&first.to_string(),"compile.log")).execute(&pool).await.unwrap();
  sqlx::query("update public.scribe_agent_preferences set diagnostics_enabled=false where user_id=$1").bind(owner.into_inner()).execute(&pool).await.unwrap();
  assert!(matches!(run(&state,owner,project,&call).await,Err(ApiError{code:ErrorCode::Forbidden,..})));assert_eq!(requests.load(Ordering::SeqCst),0);
  sqlx::query("update public.scribe_agent_preferences set diagnostics_enabled=true where user_id=$1").bind(owner.into_inner()).execute(&pool).await.unwrap();
  let mut call=call;let mut joined=String::new();loop{let page=run(&state,viewer,project,&call).await.unwrap();assert_eq!(page["engine"],"xelatex");assert_eq!(page["projectCompiler"],"lualatex");assert_eq!(page["mainFile"],"main.tex");assert_eq!(page["compileSucceeded"],true);assert_eq!(page["warningCount"],1);joined.push_str(page["content"].as_str().unwrap());if page["nextCall"].is_null(){break;}call=page["nextCall"].clone();}assert_eq!(joined,original);
  let latest=json!({"tool":"read_compile_log"});let waiting=run(&state,owner,project,&latest).await.unwrap();assert_eq!(waiting["jobId"],json!(pending));assert_eq!(waiting["status"],"queued");assert_eq!(waiting["logPending"],true);
  sqlx::query("update public.compile_jobs set status='success',exit_code=0,log_key=$2,started_at=now(),completed_at=now() where id=$1").bind(pending).bind(compile_artifact_key(project,&pending.to_string(),"compile.log")).execute(&pool).await.unwrap();
  ready.store(false,Ordering::SeqCst);let uploading=run(&state,owner,project,&latest).await.unwrap();assert_eq!(uploading["status"],"success");assert_eq!(uploading["logPending"],true);assert_eq!(uploading["logAvailable"],false);
  ready.store(true,Ordering::SeqCst);let completed=run(&state,owner,project,&latest).await.unwrap();assert_eq!(completed["status"],"success");assert_eq!(completed["logAvailable"],true);assert_eq!(completed["logPending"],false);assert_eq!(completed["engine"],"lualatex");
  revoke.store(true,Ordering::SeqCst);assert!(matches!(run(&state,owner,project,&latest).await,Err(ApiError{code:ErrorCode::Forbidden,..})));
  server.abort();pool.close().await;
 }
}

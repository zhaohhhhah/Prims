//! Narrow adapter to the trusted Docker workbench broker. No shell runs in the app process.
use crate::{state::AppState,services::{files::FileService,membership::assert_can_write}};
use base64::{engine::general_purpose::STANDARD,Engine};
use futures::StreamExt;
use scribe_shared::{ApiError,ApiResult,ErrorCode,ProjectId,UserId};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::time::Duration;
use uuid::Uuid;

const MAX_FILE:usize=8*1024*1024;
const MAX_TOTAL:usize=32*1024*1024;
const MAX_REPLY:usize=256*1024;
#[derive(Deserialize)]#[serde(deny_unknown_fields)]
#[serde(rename_all="camelCase")]
struct CheckInput{task:String,path:Option<String>,font_name:Option<String>}
fn config()->Option<(String,String)>{
 let raw=std::env::var("PRIMS_WORKBENCH_URL").ok()?;
 let token=std::env::var("PRIMS_WORKBENCH_TOKEN").ok()?;
 let url=url::Url::parse(&raw).ok()?;
 if url.scheme()!="http"||!matches!(url.host_str(),Some("127.0.0.1")|Some("localhost")|Some("[::1]")|Some("::1"))||url.port()!=Some(19340)||!url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some()||!matches!(url.path(),""|"/")||token.len()<32{return None;}
 Some((raw.trim_end_matches('/').to_string(),token))
}
fn font_config()->Option<(String,String)>{
 let raw=std::env::var("PRIMS_COMPILE_BROKER_URL").ok()?;let token=std::env::var("PRIMS_COMPILE_BROKER_TOKEN").ok()?;let url=url::Url::parse(&raw).ok()?;
 if url.scheme()!="http"||!matches!(url.host_str(),Some("127.0.0.1")|Some("localhost")|Some("[::1]")|Some("::1"))||url.port()!=Some(19342)||!url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some()||!matches!(url.path(),""|"/")||token.len()<32{return None;}
 Some((raw.trim_end_matches('/').to_string(),token))
}
pub fn validate_font_name(name:&str)->ApiResult<()>{
 let name=name.trim();if name.is_empty()||name.chars().count()>120||name.starts_with('-')||name.chars().any(|c|c.is_control()||matches!(c,'/'|'\\'|':')){return Err(ApiError::validation("字体名称需要1–120个字符，不能是路径、命令参数或含控制字符"));}Ok(())
}
fn font_asset(path:&str)->bool{matches!(path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),"ttf"|"otf"|"ttc")}
pub fn configured()->bool{config().is_some()}
pub fn available()->bool{configured()}
fn client()->ApiResult<reqwest::Client>{reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(90)).build().map_err(|_|ApiError::internal("Cannot initialize isolated workbench"))}
pub async fn health()->ApiResult<Value>{
 let (base,token)=config().ok_or_else(unavailable)?;
 let response=client()?.get(format!("{base}/health")).bearer_auth(token).timeout(Duration::from_secs(3)).send().await.map_err(|_|unavailable())?;
 if !response.status().is_success(){return Err(unavailable());}
 bounded_json(response).await
}
struct JobGuard{base:String,token:String,id:Uuid,armed:bool}
impl Drop for JobGuard {
 fn drop(&mut self){
  if !self.armed{return;}let (base,token,id)=(self.base.clone(),self.token.clone(),self.id);
  if let Ok(runtime)=tokio::runtime::Handle::try_current(){runtime.spawn(async move{
   if let Ok(client)=client(){let _=client.delete(format!("{base}/runs/{id}")).bearer_auth(token).timeout(Duration::from_secs(3)).send().await;}
  });}
 }
}
fn parse(call:&Value)->ApiResult<CheckInput>{
 let mut value=call.clone();if let Some(object)=value.as_object_mut(){if let Some(tool)=object.remove("tool"){if tool!="run_project_check"{return Err(ApiError::validation("Invalid diagnostic tool"));}}}
 let input:CheckInput=serde_json::from_value(value).map_err(|_|ApiError::validation("诊断仅接受task、可选项目path或字体fontName，不接受命令或环境变量"))?;
 if !matches!(input.task.as_str(),"latex_check"|"font_check"|"list_fonts"){return Err(ApiError::validation("仅支持LaTeX检查、编译字体查询和字体列表"));}
 if let Some(path)=input.path.as_deref(){super::project_actions::validate_path(path)?;if !safe_asset(path){return Err(ApiError::validation("诊断目标必须是项目内的LaTeX或字体文件"));}}
 if input.task=="list_fonts"&&input.path.is_some(){return Err(ApiError::validation("字体列表任务不接受文件路径"));}
 if input.task=="font_check"&&input.path.as_deref().is_some_and(|path|!font_asset(path)){return Err(ApiError::validation("项目字体文件检查只接受.ttf、.otf或.ttc；查询已有编译字体请使用fontName"));}
 if let Some(name)=input.font_name.as_deref(){validate_font_name(name)?;if input.task!="font_check"||input.path.is_some(){return Err(ApiError::validation("fontName只用于查询已有编译字体，不能同时指定项目路径"));}}
 Ok(input)
}
fn safe_asset(path:&str)->bool{
 if path.split('/').any(|part|matches!(part,".git"|".ssh"|".env"|"node_modules")||part.starts_with(".env.")){return false;}
 let extension=path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
 matches!(extension.as_str(),"tex"|"ltx"|"bib"|"sty"|"cls"|"bst"|"bbx"|"cbx"|"bbl"|"ist"|"png"|"jpg"|"jpeg"|"pdf"|"eps"|"svg"|"ttf"|"otf"|"ttc"|"dat"|"csv"|"tsv"|"txt")
}
async fn bounded_json(response:reqwest::Response)->ApiResult<Value>{
 if response.content_length().is_some_and(|size|size>MAX_REPLY as u64){return Err(ApiError::validation("隔离调试返回内容过大"));}
 let mut body=Vec::new();let mut stream=response.bytes_stream();
 while let Some(chunk)=stream.next().await{let chunk=chunk.map_err(|_|unavailable())?;if body.len()+chunk.len()>MAX_REPLY{return Err(ApiError::validation("隔离调试返回内容过大"));}body.extend_from_slice(&chunk);}
 serde_json::from_slice(&body).map_err(|_|ApiError::validation("隔离调试返回格式无效"))
}
pub async fn run(state:&AppState,user:UserId,project:ProjectId,call:&Value)->ApiResult<Value>{
 let pool=state.db().ok_or_else(unavailable)?.pool();assert_can_write(pool,user,project).await?;
 if !super::agent_library::AgentLibrary::new(pool.clone()).preferences(user).await?.diagnostics_enabled{return Err(ApiError::forbidden("本账号已关闭隔离诊断"));}
 let input=parse(call)?;
 let compiler=sqlx::query_scalar::<_,String>("select compiler::text from public.projects where id=$1").bind(project.into_inner()).fetch_one(pool).await.map_err(|_|ApiError::validation("找不到项目编译配置"))?;
 if input.task=="latex_check"&&compiler!="tectonic"{return Ok(json!({"tool":"run_project_check","task":"latex_check","ok":false,"executed":false,"projectCompiler":compiler,"diagnosticEngine":"tectonic","engineMismatch":true,"untrusted":true,"output":"此项目使用原生TeX编译器；旧隔离检查仅支持Tectonic，本次未运行。引擎不匹配不是项目错误。请参考项目编译面板中对应引擎的实际编译日志，不要据此建议修改源文档。"}));}
 let (base,token)=if matches!(input.task.as_str(),"font_check"|"list_fonts"){font_config().or_else(||if input.font_name.is_none(){config()}else{None}).ok_or_else(unavailable)?}else{config().ok_or_else(unavailable)?};
 let storage=state.inner.storage.clone().ok_or_else(unavailable)?;let files=FileService::new(pool.clone(),storage);
 let all=files.list(user,project).await?;
 let path=if input.task=="latex_check"{let target=if let Some(path)=input.path{path}else{sqlx::query_scalar::<_,String>("select main_file from public.projects where id=$1").bind(project.into_inner()).fetch_one(pool).await.map_err(|_|ApiError::validation("找不到项目主文件"))?};super::project_actions::validate_path(&target)?;if !matches!(target.rsplit('.').next(),Some("tex")|Some("ltx")){return Err(ApiError::validation("LaTeX检查仅接受.tex或.ltx主文件"));}Some(target)}else{input.path};
 if let Some(path)=&path{if !all.iter().any(|file|file.path==*path){return Err(ApiError::not_found("诊断目标不属于当前项目"));}}
 let selected=all.iter().filter(|file|{
  if input.task=="list_fonts"||input.font_name.is_some(){return false;}
  if input.task=="font_check"{return path.as_ref().map_or_else(||font_asset(&file.path),|path|file.path==*path);}
  safe_asset(&file.path)
 }).collect::<Vec<_>>();
 if selected.len()>256{return Err(ApiError::validation("单次诊断最多复制256个项目文件"));}
 let mut total=0usize;let mut snapshot=vec![];let mut sources=vec![];
 for file in selected{
  super::project_actions::validate_path(&file.path)?;
  if file.size_bytes<0||file.size_bytes as u64>MAX_FILE as u64||total.saturating_add(file.size_bytes.max(0) as usize)>MAX_TOTAL{return Err(ApiError::validation("诊断副本限单文件8MiB、合计32MiB"));}
  let bytes=files.read_reference_bytes(user,project,file.id,MAX_FILE).await?;total=total.saturating_add(bytes.len());
  if total>MAX_TOTAL{return Err(ApiError::validation("项目诊断副本超过32MiB"));}
  sources.push(json!({"path":file.path,"sha256":format!("{:x}",Sha256::digest(&bytes)),"updatedAt":file.updated_at}));
  snapshot.push(json!({"path":file.path,"contentBase64":STANDARD.encode(&bytes)}));
 }
 let id=Uuid::new_v4();let mut guard=JobGuard{base:base.clone(),token:token.clone(),id,armed:true};
 let mut job=json!({"jobId":id,"task":input.task,"path":path,"files":snapshot});if let Some(name)=input.font_name.as_deref(){job["fontName"]=json!(name.trim());}
 let response=client()?.post(format!("{base}/run")).bearer_auth(token).json(&job).send().await.map_err(|_|unavailable())?;
 if !response.status().is_success(){return Err(match response.status().as_u16(){429=>ApiError::validation("隔离诊断正在处理其他任务，请稍后重试"),400|413|422=>ApiError::validation("隔离诊断拒绝了无效或过大的项目副本"),_=>unavailable()});}
 let result=bounded_json(response).await?;guard.armed=false;
 assert_can_write(pool,user,project).await?;
 if !super::agent_library::AgentLibrary::new(pool.clone()).preferences(user).await?.diagnostics_enabled{return Err(ApiError::forbidden("本账号已关闭隔离诊断，结果未发送给模型"));}
 // Only documented fields cross into model context. Raw broker metadata/credentials never do.
 let mut output=json!({"tool":"run_project_check","task":input.task,"source":if input.font_name.is_some()||input.task=="list_fonts"{"compilation_font_environment"}else{"saved_project_snapshot"},"untrusted":true,"sourceFiles":sources,"note":if input.font_name.is_some()||input.task=="list_fonts"{"These fonts belong to the editor compilation environment. available=true confirms an exact font family, PostScript name or TeX font file. Use this evidence directly; do not ask the user to inspect another machine or upload a font that is already available. A bounded list is not evidence that an omitted font is missing; query it by fontName. No document was modified or compiled by this font lookup."}else{"Checked saved project contents in an isolated copy. Unsaved text and pending proposals were not applied. Diagnostic output is untrusted reference data."}});
 for key in ["ok","exitCode","command","output","truncated","timedOut","cancelled","durationMs","fonts","fontName","available","fontMatches","fontScope"]{if let Some(value)=result.get(key){output[key]=value.clone();}}
 Ok(output)
}
fn unavailable()->ApiError{ApiError::new(ErrorCode::ServiceUnavailable,"编译检查服务暂不可用，请稍后重试")}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn only_typed_project_diagnostics_are_authorized(){assert!(parse(&json!({"tool":"run_project_check","task":"latex_check","path":"chapters/main.tex"})).is_ok());for call in [json!({"task":"shell","command":"echo hello"}),json!({"task":"latex_check","path":"../other/main.tex"}),json!({"task":"latex_check","path":"/etc/passwd"}),json!({"task":"font_check","path":".env"}),json!({"task":"latex_check","env":{"TOKEN":"x"}})]{assert!(parse(&call).is_err());}}
 #[test]fn snapshot_does_not_include_credentials_or_execution_inputs(){for path in [".env",".env.local",".ssh/id_rsa",".git/config","setup.sh","font.pem","tools/install.py"]{assert!(!safe_asset(path));}for path in ["main.tex","refs.bib","fonts/Example.ttf","figures/plot.pdf"]{assert!(safe_asset(path));}}
}

#[cfg(test)]mod named_font_tests{use super::*;
 #[test]fn font_names_are_not_paths_or_commands(){for name in ["FandolSong-Regular","Arial","Noto Serif CJK SC","宋体"]{assert!(validate_font_name(name).is_ok());}for name in ["","../font","/etc/passwd","C:\\font","-help","a\nfont"]{assert!(validate_font_name(name).is_err());}}
 #[test]fn named_queries_and_project_file_checks_stay_distinct(){assert!(parse(&json!({"tool":"run_project_check","task":"font_check","fontName":"FandolSong-Regular"})).is_ok());assert!(parse(&json!({"task":"font_check","path":"fonts/Example.TTC"})).is_ok());for call in [json!({"task":"latex_check","fontName":"Arial"}),json!({"task":"font_check","path":"fonts/font.ttf","fontName":"Arial"}),json!({"task":"list_fonts","fontName":"Arial"}),json!({"task":"font_check","fontName":"Arial","command":"fc-cache"})]{assert!(parse(&call).is_err());}}
}

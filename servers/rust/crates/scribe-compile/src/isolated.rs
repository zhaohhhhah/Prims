//! Fixed compile protocol to the existing trusted local sandbox broker.
use base64::{engine::general_purpose::STANDARD,Engine};
use futures::StreamExt;
use scribe_shared::CompilerEngine;
use serde::{Deserialize,Serialize};
use std::{path::Path,time::{Duration,Instant}};
use crate::CompileOutcome;
const MAX_TOTAL:usize=32*1024*1024;
const MAX_FILE:usize=8*1024*1024;
const MAX_REPLY:usize=64*1024*1024;
#[derive(Serialize)]struct File{path:String,#[serde(rename="contentBase64")]content:String}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]struct Reply{ok:bool,exit_code:i32,output:String,#[serde(default)]truncated:bool,#[serde(default)]timed_out:bool,#[serde(default)]cancelled:bool,#[serde(default)]duration_ms:u64,#[serde(default)]artifacts:std::collections::BTreeMap<String,String>,#[serde(default,rename="command")]_command:Vec<String>}
struct RunGuard{base:String,token:String,id:uuid::Uuid,armed:bool}
impl Drop for RunGuard{fn drop(&mut self){if !self.armed{return;}let (base,token,id)=(self.base.clone(),self.token.clone(),self.id);if let Ok(runtime)=tokio::runtime::Handle::try_current(){runtime.spawn(async move{if let Ok(client)=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(3)).build(){let _=client.delete(format!("{base}/runs/{id}")).bearer_auth(token).send().await;}});}}}
fn configuration()->anyhow::Result<(String,String)>{
 let raw=std::env::var("PRIMS_COMPILE_BROKER_URL")?;let token=std::env::var("PRIMS_COMPILE_BROKER_TOKEN")?;let url=url::Url::parse(&raw)?;
 anyhow::ensure!(url.scheme()=="http"&&matches!(url.host_str(),Some("127.0.0.1")|Some("localhost")|Some("[::1]")|Some("::1"))&&url.port()==Some(19342)&&url.username().is_empty()&&url.password().is_none()&&url.query().is_none()&&url.fragment().is_none()&&matches!(url.path(),""|"/")&&token.len()>=32,"isolated compiler broker configuration invalid");
 Ok((raw.trim_end_matches('/').to_string(),token))
}
pub async fn available()->bool {
 let Ok((base,token))=configuration()else{return false;};
 let Ok(client)=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(3)).build()else{return false;};
 let Ok(response)=client.get(format!("{base}/health")).bearer_auth(token).send().await else{return false;};
 if !response.status().is_success(){return false;}
 let Ok(value)=response.json::<serde_json::Value>().await else{return false;};
 value.get("ok").and_then(|v|v.as_bool())==Some(true)&&value.get("capabilities").and_then(|v|v.as_array()).is_some_and(|list|list.iter().any(|v|v=="latex_compile"))
}
fn safe_path(path:&str)->bool{!path.is_empty()&&path.len()<=512&&!path.starts_with('/')&&!path.contains('\\')&&!path.contains(':')&&!path.chars().any(char::is_control)&&path.split('/').all(|p|!p.is_empty()&&!matches!(p,"."|".."|".output"|".workbench")&&!p.starts_with('-'))}
pub async fn run_isolated(engine:CompilerEngine,workdir:&Path,main_file:&str,paths:&[String])->CompileOutcome{
 let started=Instant::now();match run(engine,workdir,main_file,paths).await{Ok(reply)=>CompileOutcome{success:reply.ok,exit_code:reply.exit_code,stdout:reply.output,stderr:if reply.timed_out{"隔离编译超过时间限制".into()}else if reply.cancelled{"隔离编译已取消".into()}else if reply.truncated{"编译日志已截断".into()}else{String::new()},duration:started.elapsed()},Err(error)=>CompileOutcome{success:false,exit_code:-1,stdout:String::new(),stderr:format!("隔离 {} 编译失败：{error}",engine.as_str()),duration:started.elapsed()}}
}
async fn run(engine:CompilerEngine,workdir:&Path,main:&str,paths:&[String])->anyhow::Result<Reply>{
 anyhow::ensure!(engine!=CompilerEngine::Tectonic&&safe_path(main)&&paths.len()<=256&&paths.iter().any(|p|p==main),"invalid project compile snapshot");let (base,token)=configuration().map_err(|_|anyhow::anyhow!("独立编译服务未配置；不会在主服务或NAS上运行TeX"))?;
 let mut total=0usize;let mut files=vec![];for path in paths{anyhow::ensure!(safe_path(path),"invalid project path");let target=workdir.join(path);let meta=tokio::fs::symlink_metadata(&target).await?;anyhow::ensure!(meta.is_file()&&!meta.file_type().is_symlink()&&meta.len()<=MAX_FILE as u64&&total.saturating_add(meta.len() as usize)<=MAX_TOTAL,"编译副本超过大小限制或含无效文件");let bytes=tokio::fs::read(target).await?;total+=bytes.len();anyhow::ensure!(bytes.len()<=MAX_FILE&&total<=MAX_TOTAL,"编译副本限单文件8MiB、合计32MiB");files.push(File{path:path.clone(),content:STANDARD.encode(bytes)});}
 let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(185)).build()?;
 let id=uuid::Uuid::new_v4();let mut guard=RunGuard{base:base.clone(),token:token.clone(),id,armed:true};
 let response=client.post(format!("{base}/run")).bearer_auth(token).json(&serde_json::json!({"jobId":id,"task":"latex_compile","engine":engine.as_str(),"path":main,"files":files})).send().await?;
 anyhow::ensure!(response.status().is_success(),"独立编译服务未就绪或繁忙 ({})",response.status());let mut body=vec![];let mut stream=response.bytes_stream();while let Some(chunk)=stream.next().await{let chunk=chunk?;anyhow::ensure!(body.len()+chunk.len()<=MAX_REPLY,"isolated compiler response too large");body.extend_from_slice(&chunk);}
 let reply:Reply=serde_json::from_slice(&body)?;guard.armed=false;let _=reply.duration_ms;let stem=Path::new(main).file_stem().and_then(|s|s.to_str()).ok_or_else(||anyhow::anyhow!("invalid main file"))?;
 for suffix in ["pdf","log","synctex.gz","bbl"]{let path=workdir.join(format!("{stem}.{suffix}"));let _=tokio::fs::remove_file(&path).await;if let Some(encoded)=reply.artifacts.get(suffix){anyhow::ensure!(encoded.len()<=44*1024*1024,"compile artifact encoding too large");let bytes=STANDARD.decode(encoded)?;anyhow::ensure!(bytes.len()<=32*1024*1024,"compile artifact too large");if suffix=="pdf"{anyhow::ensure!(bytes.starts_with(b"%PDF"),"invalid PDF artifact");}tokio::fs::write(path,bytes).await?;}}
 anyhow::ensure!(reply.artifacts.keys().all(|k|matches!(k.as_str(),"pdf"|"log"|"synctex.gz"|"bbl")),"invalid artifact kind");
 if reply.ok{anyhow::ensure!(reply.artifacts.contains_key("pdf"),"successful compile did not produce a PDF");}Ok(reply)
}
#[cfg(test)]mod tests{use super::*;#[test]fn snapshots_accept_only_project_relative_paths(){for p in ["main.tex","chapters/介绍.tex","figures/plot.pdf"]{assert!(safe_path(p));}for p in ["/etc/passwd","../secret","a/../../etc","a\\b","-command.tex","a//b",".output/log","c:evil"]{assert!(!safe_path(p));}}}

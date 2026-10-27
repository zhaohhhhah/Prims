//! Bounded project-aware assistant with private history and optional read-only file tools.
use axum::response::{sse::{Event,KeepAlive,Sse},IntoResponse,Response};
use futures::StreamExt;
use scribe_shared::{AICompleteInput,ApiError,ApiResult,ErrorCode,ProjectId,UserId};
use serde_json::{json,Value};
use std::{convert::Infallible,time::Duration};
use tokio::sync::mpsc;
use uuid::Uuid;
use crate::state::AppState;
use super::{ai::{AIService,AIStreamStartError},ai_context::{AIContextService,model_history,take_generated_summary,MAX_CONTENT,MAX_RAW},files::FileService,project_context::ProjectContext};

pub async fn start(state:AppState,user:UserId,mut input:AICompleteInput)->ApiResult<Response>{
 let raw=input.options.as_ref().and_then(|o|o.get("projectId")).ok_or_else(||ApiError::validation("缺少项目编号"))?;
 let project=ProjectId::new(Uuid::parse_str(raw).map_err(|_|ApiError::validation("项目编号无效"))?);
 let pool=state.db().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"database unavailable"))?.pool().clone();
 let memory=AIContextService::new(pool.clone());let saved=memory.get(user,project).await?;
 if let Some(thread)=input.options.as_ref().and_then(|o|o.get("threadId")){if Uuid::parse_str(thread).ok()!=Some(saved.thread_id){return Err(ApiError::new(ErrorCode::Conflict,"已开始新的项目对话，请刷新后重新发送"));}}
 let current=input.options.as_ref().and_then(|o|o.get("filePath")).cloned().unwrap_or_default();
 let selection_id=input.options.as_ref().and_then(|o|o.get("selectionId")).cloned().unwrap_or_default();
 let allow_files=!input.options.as_ref().and_then(|o|o.get("allowProjectFiles")).is_some_and(|v|v=="false");
 let (history,history_truncated)=model_history(&saved,&current,&selection_id);let history_count=history.len();input.history=Some(history);
 let options=input.options.get_or_insert_with(Default::default);options.insert("writingBrief".into(),saved.brief.clone());options.insert("projectTitle".into(),saved.project_title.clone());options.insert("projectDescription".into(),saved.project_description.clone().unwrap_or_default());options.insert("autoSummary".into(),saved.auto_summary.clone());
 // Client-provided synthetic tool results never bypass the server's read authorization.
 options.remove("projectTools");options.remove("projectContext");
 if !allow_files{options.remove("documentOutline");}
 let context_used=json!({"brief":!saved.brief.trim().is_empty(),"autoSummary":!saved.auto_summary.trim().is_empty(),"historyMessages":history_count,"historyTruncated":history_truncated,"fileTools":allow_files,"currentSection":options.get("currentSection"),"selectionStartLine":options.get("selectionStartLine"),"selectionEndLine":options.get("selectionEndLine"),"documentWindowStartLine":if allow_files{options.get("documentWindowStartLine")}else{None},"documentWindowEndLine":if allow_files{options.get("documentWindowEndLine")}else{None}});
 let storage=state.inner.storage.clone().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"storage unavailable"))?;
 let files=FileService::new(pool.clone(),storage);
 let mut context=if allow_files{Some(ProjectContext::new(&files,user,project).await?)}else{None};
 let crypto=state.inner.ai_crypto.clone().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"AI unavailable"))?;
 let ai=AIService::new(pool,crypto);let editing_allowed=super::inline_prompt::editing_allowed(&input);
 let live=input.options.as_ref().and_then(|o|o.get("currentDocument")).cloned();
 let (tx,rx)=mpsc::channel::<Value>(16);
 tokio::spawn(async move{
  let result=tokio::time::timeout(Duration::from_secs(180),async{
   emit(&tx,json!({"status":"已准备写作背景和连续对话上下文","contextUsed":context_used})).await?;
   if let Some(context)=context.as_mut(){
    emit(&tx,json!({"status":"正在查看当前项目目录…"})).await?;
    if context.files.iter().any(|f|f.path==current){
     if let Some(text)=live{context.add_live_document(&current,&text,&input);}else{context.read(&files,user,&current,&mut input).await?;}
     emit(&tx,json!({"status":format!("已读取当前文件：{current}"),"file":current})).await?;
    }
    input.options.as_mut().unwrap().insert("projectTools".into(),"enabled".into());
   }
   input.options.as_mut().unwrap().remove("currentDocument");
   let mut image_fallback_used=false;
   for step in 0..6 {
    if let Some(context)=context.as_ref(){context.sync(&mut input);}
    emit(&tx,json!({"status":if step==0{"模型正在分析写作目标和当前内容…"}else{"模型正在结合项目上下文生成…"}})).await?;
    let raw=loop{
     let round=async{
      let mut stream=ai.complete_stream_for_agent(user,input.clone()).await?;let mut text=String::new();let mut tick=0;
      while let Some(chunk)=stream.next().await{
       let chunk=chunk.map_err(AIStreamStartError::Provider)?;text.push_str(&chunk.text);
       if text.len()>150000{return Err(AIStreamStartError::Request(ApiError::validation("模型输出过长，请缩小问题范围")));}
       if text.len()/500>tick{tick=text.len()/500;emit(&tx,json!({"status":"模型正在生成…","received":text.chars().count()})).await?;}
      }Ok::<_,AIStreamStartError>(text)
     };
     let result=tokio::select!{_=tx.closed()=>return Ok(()),result=tokio::time::timeout(Duration::from_secs(90),round)=>result.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"模型响应超时，请重试"))?};
     match result{
      Ok(text)=>break text,
      Err(error) if may_retry_without_images(&input,image_fallback_used,&error)=>{
       image_fallback_used=true;strip_image_inputs(&mut input);
       if let Some(context)=context.as_mut(){context.disable_images(&mut input);}
       emit(&tx,json!({"warning":{"code":"image_unsupported","message":"当前接口或模型不支持读取图片，已继续使用文字上下文。建议切换支持图片的视觉模型。"},"status":"图片输入暂不可用，正在使用文字上下文继续…"})).await?;
      },
      Err(error)=>return Err(error.into_api_error()),
     }
    };
    let mut output=parse_model_output(&raw)?;
    if let Some(tool)=output.get("tool").and_then(Value::as_str){
     if step==5{return Err(ApiError::validation("本轮文件工具调用已达上限，请缩小问题或分次提问"));}
     let Some(context)=context.as_mut()else{return Err(ApiError::validation("本轮已关闭项目文件读取；请开启后重试"));};
     let result=match tool{
      "search_project"=>{emit(&tx,json!({"status":"正在查找项目内容…"})).await?;let query=output.get("query").and_then(Value::as_str).unwrap_or("");context.search(&files,user,query).await},
      "list_files"=>{emit(&tx,json!({"status":"正在列出项目文件…"})).await?;Ok(context.listing())},
      "read_file"=>{
       let path=output.get("path").and_then(Value::as_str).unwrap_or("");emit(&tx,json!({"status":format!("正在读取：{path}")})).await?;
       let start=output.get("startLine").and_then(Value::as_u64).map(|n|n as usize);let end=output.get("endLine").and_then(Value::as_u64).map(|n|n as usize);
       let result=context.read_window(&files,user,path,start,end,&mut input).await;
       if result.is_ok(){emit(&tx,json!({"status":format!("已读取：{path}"),"file":path})).await?;}result
      },_=>Err(ApiError::validation("只支持 list_files、search_project、read_file 三个只读工具"))
     };
     let value=match result{Ok(value) if tool=="search_project"=>value,Ok(_)=>json!({"ok":true,"note":"结果已加入 projectContext.directory 或 readFiles"}),Err(err)=>json!({"tool":tool,"error":err.message})};context.results.push(json!({"toolCall":output,"toolResult":value}));
    }else{
     if !output.get("reply").is_some_and(Value::is_string)||!output.get("replacement").is_some_and(|v|v.is_null()||v.is_string()){return Err(ApiError::validation("模型未返回有效答复或选区方案"));}
     // A model cannot manufacture an edit target when the assistant was opened without a selection.
     validate_edit_output(&mut output,&current,editing_allowed)?;
     let recovered=output.as_object_mut().and_then(|value|value.remove("_formatRecovery")).and_then(|value|value.as_bool()).unwrap_or(false);
     if recovered{emit(&tx,json!({"warning":{"code":"answer_only_recovered","message":"修改建议未完整生成，本轮只保留可读答复；没有任何修改已应用。"}})).await?;}
     let summary=take_generated_summary(&mut output);
     if output.to_string().chars().count()>MAX_RAW{return Err(ApiError::validation("修改方案过长，请减少修改范围后重试；正文未改动"));}
     let mut memory_updated=false;
     if let Some(summary)=summary{
      if tx.is_closed(){return Ok(());}
      // Disconnect drops the transaction future. Persistence failures are optional metadata failures,
      // never a reason to discard a valid answer or modification preview.
      let result=tokio::select!{_=tx.closed()=>return Ok(()),result=memory.save_generated_summary(user,project,&saved,&summary)=>result};
      match result{Ok(updated)=>memory_updated=updated,Err(_)=>tracing::warn!("Could not save generated private writing context; normal reply preserved")}
     }
     let has_preview=output.get("replacement").is_some_and(Value::is_string)||output.get("edits").and_then(Value::as_array).is_some_and(|edits|!edits.is_empty());
     emit(&tx,json!({"status":if has_preview{"答复与修改预览已生成"}else{"答复已生成"},"text":output.to_string(),"contextMemoryUpdated":memory_updated})).await?;return Ok(());
    }
   }Ok(())
  }).await;
  let error=match result{Ok(Err(err))=>Some(err.message),Err(_)=>Some("本轮对话超时，请重试".into()),_=>None};
  if let Some(error)=error{let _=tx.send(json!({"error":error})).await;}let _=tx.send(json!({"done":true})).await;
 });
 let stream=futures::stream::unfold(rx,|mut rx|async move{let value=rx.recv().await?;let data=if value.get("done")==Some(&Value::Bool(true)){"[DONE]".into()}else{value.to_string()};Some((Ok::<_,Infallible>(Event::default().data(data)),rx))});
 Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}
async fn emit(tx:&mpsc::Sender<Value>,value:Value)->ApiResult<()>{tx.send(value).await.map_err(|_|ApiError::validation("请求已取消"))}

const SUPPORT_FILE_ACTIONS:bool=false;
/// Only exact valid protocol JSON can carry edits. Recovery retains a complete reply only.
fn parse_model_output(raw:&str)->ApiResult<Value>{
 let text=raw.trim();if text.is_empty()||contains_hidden_reasoning(text){return Err(output_format_error());}
 let mut clean=text.to_string();let mut structured=false;let mut mixed=false;
 if let Some(at)=text.to_ascii_lowercase().find("```json"){
  structured=true;mixed=at>0;clean=text[at..].split_once('\n').map(|(_,body)|body).unwrap_or("").trim().strip_suffix("```").unwrap_or_else(||text[at..].split_once('\n').map(|(_,body)|body).unwrap_or("").trim()).trim().to_string();
 }else if text.starts_with("```")&&text.split_once('\n').is_some_and(|(_,body)|body.trim_start().starts_with('{')){
  structured=true;clean=text.split_once('\n').map(|(_,body)|body).unwrap_or("").trim().strip_suffix("```").unwrap_or("").trim().to_string();
 }else if !text.starts_with('{'){
  if let Some(at)=text.find("{\"reply\""){structured=true;mixed=true;clean=text[at..].trim().to_string();}
 }
 structured|=clean.starts_with('{')||clean.starts_with('[');
 match serde_json::from_str::<Value>(&clean){
  Ok(value)=>{
   let original=value.get("reply").and_then(Value::as_str);
   // JSON's \b/\f escapes may have swallowed an unescaped \begin/\frac in the reply.
   if original.is_some_and(|reply|reply.chars().any(|c|c.is_control()&&!matches!(c,'\n'|'\r'|'\t'))){return recover_reply(&clean);}
   normalize_model_value(value,!mixed)
  },
  Err(error)=>{
   if structured{tracing::warn!(category=?error.classify(),line=error.line(),column=error.column(),raw_chars=raw.chars().count(),"AI response format metadata; no content recorded");return recover_reply(&clean);}
   if text.to_ascii_lowercase().starts_with("tool:")||text.to_ascii_lowercase().starts_with("tool :"){return Err(output_format_error());}
   answer_only(text,false)
  }
 }
}
fn output_format_error()->ApiError{ApiError::validation("本轮答复格式不完整，正文未改动。可以继续提问，或重试本轮。")}
fn contains_hidden_reasoning(text:&str)->bool{let lower=text.to_ascii_lowercase();["<think","</think","<analysis","</analysis","<reasoning","</reasoning","<|analysis|>","<|channel|>analysis"].iter().any(|marker|lower.contains(marker))}
fn answer_only(reply:&str,recovered:bool)->ApiResult<Value>{
 let reply=reply.trim();if reply.is_empty()||contains_hidden_reasoning(reply)||(reply.starts_with('{')||reply.starts_with('['))||reply.chars().any(|c|c.is_control()&&!matches!(c,'\n'|'\r'|'\t')){return Err(output_format_error());}
 let message=if recovered{format!("本轮仅保留说明文字；修改建议未完整生成，没有可应用的方案，正文未改动。\n\n{reply}")}else{reply.to_string()};
 let mut output=json!({"reply":message,"replacement":null,"edits":[]});if SUPPORT_FILE_ACTIONS{output["actions"]=json!([]);}if recovered{output["_formatRecovery"]=json!(true);}Ok(output)
}
fn recover_reply(text:&str)->ApiResult<Value>{
 // A malformed tool object never becomes a natural answer, and never executes.
 if !text.starts_with('{')||text.contains("\"tool\""){return Err(output_format_error());}
 let reply=complete_reply_from_object(text).ok_or_else(output_format_error)?;answer_only(&reply,true)
}
fn normalize_model_value(value:Value,trusted:bool)->ApiResult<Value>{
 if let Some(reply)=value.as_str(){return answer_only(reply,false);}
 let object=value.as_object().ok_or_else(output_format_error)?;
 if let Some(tool)=object.get("tool").filter(|tool|!tool.is_null()){
  if !trusted{return Err(output_format_error());}
  let name=tool.as_str().filter(|name|matches!(*name,"list_files"|"search_project"|"read_file")).ok_or_else(output_format_error)?;
  let mut output=json!({"tool":name});for key in ["path","query","startLine","endLine"]{if let Some(value)=object.get(key){output[key]=value.clone();}}return Ok(output);
 }
 let reply=object.get("reply").and_then(Value::as_str).ok_or_else(output_format_error)?;
 if !trusted{return answer_only(reply,true);}let _=answer_only(reply,false)?;
 let replacement=object.get("replacement").cloned().unwrap_or(Value::Null);
 let edits=object.get("edits").filter(|value|!value.is_null()).cloned().unwrap_or_else(||json!([]));
 if (!replacement.is_null()&&!replacement.is_string())||!edits.is_array(){return answer_only(reply,true);}
 let mut output=json!({"reply":reply,"replacement":replacement,"edits":edits});
 if SUPPORT_FILE_ACTIONS{let actions=object.get("actions").filter(|value|!value.is_null()).cloned().unwrap_or_else(||json!([]));if !actions.is_array(){return answer_only(reply,true);}output["actions"]=actions;}
 if let Some(summary)=object.get("contextSummary").filter(|value|value.is_string()){output["contextSummary"]=summary.clone();}Ok(output)
}
/// Extract only a complete, root-level quoted reply when later edit JSON was truncated.
/// No other field, summary or tool call is recovered from this path.
fn complete_reply_from_object(text:&str)->Option<String>{
 fn quote_end(bytes:&[u8],start:usize)->Option<usize>{let mut escaped=false;for i in start+1..bytes.len(){let c=bytes[i];if escaped{escaped=false;}else if c==b'\\'{escaped=true;}else if c==b'"'{return Some(i);}}None}
 let bytes=text.as_bytes();let mut depth=0usize;let mut i=0usize;
 while i<bytes.len(){match bytes[i]{
  b'{'|b'['=>{depth+=1;i+=1;},b'}'|b']'=>{depth=depth.saturating_sub(1);i+=1;},
  b'"'=>{let end=quote_end(bytes,i)?;let mut after=end+1;while bytes.get(after).is_some_and(u8::is_ascii_whitespace){after+=1;}
   if depth==1&&bytes.get(after)==Some(&b':')&&serde_json::from_str::<String>(&text[i..=end]).ok().as_deref()==Some("reply"){
    after+=1;while bytes.get(after).is_some_and(u8::is_ascii_whitespace){after+=1;}if bytes.get(after)!=Some(&b'"'){return None;}
    let last=quote_end(bytes,after)?;let token=&text[after..=last];let fixed=repair_json_string_encoding(token)?;
    let value=serde_json::from_str::<String>(&fixed).ok()?;return if contains_hidden_reasoning(&value){None}else{Some(value)};
   }i=end+1;
  },_=>i+=1,
 }}None
}
/// Repair common invalid JSON *string encoding* only. Its parsed edits/summary are discarded.
fn repair_json_string_encoding(text:&str)->Option<String>{
 let chars=text.chars().collect::<Vec<_>>();let mut out=String::with_capacity(text.len());let mut i=0;let mut string=false;
 while i<chars.len(){let c=chars[i];
  if !string{out.push(c);if c=='"'{string=true;}i+=1;continue;}
  if c=='"'{out.push(c);string=false;i+=1;continue;}
  if c=='\\'{
   let next=*chars.get(i+1)?;
   let word=chars.iter().skip(i+1).take_while(|c|c.is_ascii_alphabetic()).collect::<String>();
   let tex=matches!(word.as_str(),"begin"|"beta"|"bf"|"bfseries"|"frac"|"footnote"|"text"|"textbf"|"textit"|"textrm"|"theta"|"times"|"tau"|"tilde"|"title"|"tableofcontents"|"nabla"|"newcommand"|"newline"|"notin"|"neq"|"ref"|"right"|"rho"|"renewcommand"|"usepackage"|"underline"|"uparrow");
   let valid_unicode=next=='u'&&chars.get(i+2..i+6).is_some_and(|digits|digits.iter().all(|c|c.is_ascii_hexdigit()));
   if next=='u'&&!valid_unicode&&!tex{return None;}
   if tex||!matches!(next,'"'|'\\'|'/'|'b'|'f'|'n'|'r'|'t'|'u'){out.push('\\');out.push('\\');i+=1;continue;}
   out.push(c);out.push(next);i+=2;continue;
  }
  match c{'\n'=>out.push_str("\\n"),'\r'=>out.push_str("\\r"),'\t'=>out.push_str("\\t"),c if c.is_control()=>return None,_=>out.push(c)}i+=1;
 }
 if string{return None;}Some(out)
}

fn may_retry_without_images(input:&AICompleteInput,already_used:bool,error:&AIStreamStartError)->bool{!already_used&&(input.image.is_some()||!input.project_images.is_empty())&&error.is_image_unsupported()}
fn strip_image_inputs(input:&mut AICompleteInput){input.image=None;input.project_images.clear();input.options.get_or_insert_with(Default::default).insert("imagesUnavailable".into(),"true".into());}
const MAX_EDITS:usize=12;
const MAX_EDIT_BEFORE:usize=16000;
const MAX_EDIT_AFTER:usize=24000;
const MAX_EDIT_TOTAL:usize=80000;
/// The selection identifies the question; explicit exact-text edits may target other relevant parts
/// of this same file. The client must resolve unique, non-overlapping anchors against its full live
/// snapshot before showing/applying a preview. This server never writes document content.
fn validate_edit_output(output:&mut Value,current_file:&str,editing_allowed:bool)->ApiResult<()>{
 if output.get("reply").and_then(Value::as_str).is_some_and(|v|v.chars().count()>MAX_CONTENT){return Err(ApiError::validation("答复过长，请缩小问题范围"));}
 if !editing_allowed{output["replacement"]=Value::Null;output["edits"]=json!([]);return Ok(());}
 if output.get("replacement").and_then(Value::as_str).is_some_and(|v|v.chars().count()>MAX_EDIT_AFTER){return Err(ApiError::validation("修改方案过长，请分次修改"));}
 let Some(value)=output.get("edits")else{return Ok(())};
 let edits=value.as_array().ok_or_else(||ApiError::validation("模型修改建议必须为 edits 数组，请重试；正文未改动"))?;
 if edits.len()>MAX_EDITS{return Err(ApiError::validation("每轮最多预览 12 处修改，请缩小问题范围"));}
 let has_edits=!edits.is_empty();let mut total=0usize;
 if has_edits{super::project_context::validate_path(current_file)?;}
 for edit in edits{
  let object=edit.as_object().ok_or_else(||ApiError::validation("模型修改建议格式不正确；正文未改动"))?;
  if object.len()!=3||object.keys().any(|k|!matches!(k.as_str(),"filePath"|"before"|"after")){return Err(ApiError::validation("修改建议只能包含 filePath、before、after"));}
  let path=object.get("filePath").and_then(Value::as_str).ok_or_else(||ApiError::validation("修改建议缺少当前文件路径"))?;
  let before=object.get("before").and_then(Value::as_str).ok_or_else(||ApiError::validation("修改建议缺少精确原文"))?;
  let after=object.get("after").and_then(Value::as_str).ok_or_else(||ApiError::validation("修改建议缺少替换文本"))?;
  if path.chars().any(char::is_control){return Err(ApiError::validation("修改建议的文件路径格式不正确"));}
  if path!=current_file{return Err(ApiError::validation("本轮只允许修改当前文件；其他项目文件保持只读"));}
  super::project_context::validate_path(path)?;
  let before_count=before.chars().count();let after_count=after.chars().count();
  if before.trim().is_empty()||before_count>MAX_EDIT_BEFORE||after_count>MAX_EDIT_AFTER{return Err(ApiError::validation("每处修改需要非空精确原文（最多 16000 字符），替换文本最多 24000 字符"));}
  total+=before_count+after_count;if total>MAX_EDIT_TOTAL{return Err(ApiError::validation("本轮修改建议总量过大，请分次修改"));}
 }
 // There is one proposal representation per response, preventing duplicate execution paths.
 if has_edits{output["replacement"]=Value::Null;}Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 fn edit(path:&str,before:&str,after:&str)->Value{json!({"filePath":path,"before":before,"after":after})}
 #[test]fn truncated_edit_can_only_recover_complete_reply_with_no_preview_notice(){let mut value=parse_model_output(r#"{"reply":"我已拟好章节展开思路。","replacement":null,"edits":[{"filePath":"main.tex","before":"unfinished"#).unwrap();assert!(value["reply"].as_str().unwrap().contains("没有可应用的方案"));assert!(value["replacement"].is_null());assert_eq!(value["edits"],json!([]));assert!(take_generated_summary(&mut value).is_none());assert!(parse_model_output(r#"{"reply":"incomplete"#).is_err());}
 #[test]fn optional_null_fields_allow_clarification_and_followup(){for raw in [r#"{"reply":"请先确认研究目标，我再提出修改。","tool":null,"replacement":null,"edits":null,"contextSummary":null}"#,r#"{"reply":"下一轮可以继续讨论。","replacement":null}"#]{let value=parse_model_output(raw).unwrap();assert!(value["replacement"].is_null());assert_eq!(value["edits"],json!([]));}}
 #[test]fn latex_encoding_recovery_never_recovers_modifications_or_summary(){for raw in [r#"{"reply":"请确认 \input{methods} 与 \begin{document}。","replacement":"UNTRUSTED","edits":null,"contextSummary":"must not save repaired metadata"}"#,r#"{"reply":"使用 \begin{document} 即可。","replacement":"UNTRUSTED"}"#]{let mut value=parse_model_output(raw).unwrap();assert!(value["reply"].as_str().unwrap().contains('\\'));assert!(value["replacement"].is_null());assert_eq!(value["edits"],json!([]));assert!(take_generated_summary(&mut value).is_none());}}
 #[test]fn mixed_prose_fences_and_closed_reasoning_recover_visible_answer_only(){for raw in ["我先说明需要你确认的内容。\n```json\n{\"reply\":\"请明确要保留的结论。\",\"replacement\":\"NO EDIT\"}\n```"]{let value=parse_model_output(raw).unwrap();assert!(!value["reply"].as_str().unwrap().contains("PRIVATE_REASONING"));assert!(value["replacement"].is_null());assert_eq!(value["edits"],json!([]));}for raw in [r#"{"reply":"incomplete"#,r#"{"reply":"bad unicode \uZZZZ"}"#,"<think>unfinished hidden reasoning",r#"{"tool":"unknown_write","reply":"no"}"#]{assert!(parse_model_output(raw).is_err());}}
 #[test]fn answer_only_json_and_normal_plaintext_never_create_edits(){for raw in [r#"{"reply":"我还不确定你希望保留哪个结论，可以先确认研究目标吗？"}"#,"目前证据不足，建议先确认研究目标；没有修改也没关系。","```latex\nE=mc^2\n```"]{let mut output=parse_model_output(raw).unwrap();assert!(!output["reply"].as_str().unwrap().is_empty());assert!(output["replacement"].is_null());assert_eq!(output["edits"],json!([]));assert!(take_generated_summary(&mut output).is_none());validate_edit_output(&mut output,"main.tex",true).unwrap();}}
 #[test]fn malformed_json_empty_hidden_analysis_and_unknown_tools_are_not_plain_answers(){for raw in [" ","{\"reply\":","[broken",r#"{"tool":"delete_file","reply":"pretend answer"}"#,r#"{"reply":"","replacement":null}"#,"```json\n{bad}\n```","null"]{assert!(parse_model_output(raw).is_err(),"{raw}");}}
 #[test]fn valid_structured_edits_tools_and_summary_remain_explicit(){let mut output=parse_model_output(r#"{"reply":"Here is one candidate","edits":[{"filePath":"main.tex","before":"original","after":"candidate"}],"contextSummary":"Goal confirmed; candidate pending","analysis":"private internal field"}"#).unwrap();assert!(output.get("analysis").is_none());assert_eq!(take_generated_summary(&mut output).as_deref(),Some("Goal confirmed; candidate pending"));validate_edit_output(&mut output,"main.tex",true).unwrap();assert_eq!(output["edits"][0]["after"],"candidate");let tool=parse_model_output(r#"{"tool":"search_project","query":"关键词","analysis":"internal"}"#).unwrap();assert_eq!(tool,json!({"tool":"search_project","query":"关键词"}));assert!(tool.get("reply").is_none());let fenced=parse_model_output("```json\n{\"reply\":\"No change needed\"}\n```").unwrap();assert_eq!(fenced["edits"],json!([]));}
 #[test]fn image_retry_is_once_and_only_for_explicit_capability_failure(){let mut input:AICompleteInput=serde_json::from_value(json!({"feature":"chat","selection":"exact source","options":{"mode":"inline-edit","message":"improve","filePath":"main.tex"},"history":[{"role":"user","content":"keep history"}],"image":{"mediaType":"image/png","data":"QUJD"}})).unwrap();input.project_images.push(scribe_shared::ImageInput{media_type:"image/png".into(),data:"REVG".into()});let error=AIStreamStartError::Provider(scribe_ai::AdapterError::HttpStatus{status:400,message:"model does not support images".into()});assert!(may_retry_without_images(&input,false,&error));assert!(!may_retry_without_images(&input,true,&error));let history=input.history.as_ref().unwrap()[0].content.clone();strip_image_inputs(&mut input);assert!(!may_retry_without_images(&input,false,&error));assert!(input.image.is_none()&&input.project_images.is_empty());assert_eq!(input.selection,"exact source");assert_eq!(input.options.as_ref().unwrap()["message"],"improve");assert_eq!(input.history.as_ref().unwrap()[0].content,history);assert_eq!(input.options.as_ref().unwrap()["imagesUnavailable"],"true");let mut result=json!({"reply":"text answer","replacement":null,"edits":[edit("main.tex","exact source","improved")]});validate_edit_output(&mut result,"main.tex",true).unwrap();assert_eq!(result["edits"][0]["after"],"improved");}
 #[test]fn answer_only_output_cannot_manufacture_a_replacement_or_edits(){let mut value=json!({"reply":"answer","replacement":"UNAUTHORIZED FULL FILE","edits":[edit("main.tex","any original","changed")]});validate_edit_output(&mut value,"main.tex",false).unwrap();assert!(value["replacement"].is_null());assert_eq!(value["edits"],json!([]));assert_eq!(value["reply"],"answer");}
 #[test]fn selected_legacy_edit_preserves_preview_without_applying_it(){let mut value=json!({"reply":"preview","replacement":"selected edit"});validate_edit_output(&mut value,"main.tex",true).unwrap();assert_eq!(value["replacement"],"selected edit");}
 #[test]fn multi_edit_can_reference_current_file_beyond_selection_and_disables_legacy_path(){let mut value=json!({"reply":"preview","replacement":"legacy duplicate","edits":[edit("main.tex","an exact paragraph outside the small selection","improved paragraph"),edit("main.tex","second exact sentence","")]});validate_edit_output(&mut value,"main.tex",true).unwrap();assert!(value["replacement"].is_null());assert_eq!(value["edits"].as_array().unwrap().len(),2);}
 #[test]fn rejects_cross_file_unsafe_empty_and_malformed_edits(){for edits in [json!([edit("other.tex","original","replacement")]),json!([edit("main.tex"," ","replacement")]),json!([{"filePath":"main.tex","before":"original"}]),json!("not an array")]{let mut value=json!({"reply":"preview","replacement":null,"edits":edits});assert!(validate_edit_output(&mut value,"main.tex",true).is_err());}let mut value=json!({"reply":"preview","replacement":null,"edits":[edit("../private","original","replacement")]});assert!(validate_edit_output(&mut value,"../private",true).is_err());}
 #[test]fn enforces_edit_count_and_character_budgets(){let mut value=json!({"reply":"preview","replacement":null,"edits":(0..13).map(|n|edit("main.tex",&format!("original {n}"),"replacement")).collect::<Vec<_>>()});assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!([edit("main.tex",&"界".repeat(16001),"replacement")]);assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!([edit("main.tex","original",&"界".repeat(24001))]);assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!((0..3).map(|_|edit("main.tex",&"b".repeat(15000),&"a".repeat(15000))).collect::<Vec<_>>());assert!(validate_edit_output(&mut value,"main.tex",true).is_err());}
}

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

/// Tolerate ordinary answers without inventing an edit. Broken structured output and tools remain
/// strict, and unknown/internal fields never become part of the user-visible response.
fn parse_model_output(raw:&str)->ApiResult<Value>{
 let fail=||ApiError::validation("模型答复格式不完整，请重试；正文未改动");
 let trimmed=raw.trim();if trimmed.is_empty(){return Err(fail());}
 let lower=trimmed.to_lowercase();
 if ["<think", "</think", "<analysis", "</analysis", "<reasoning", "</reasoning", "<|analysis|>", "<|channel|>analysis"].iter().any(|marker|lower.contains(marker)){return Err(fail());}
 let mut clean=trimmed.to_string();let mut structured_fence=false;
 if trimmed.starts_with("```"){
  let opening=trimmed.lines().next().unwrap_or("").trim().to_lowercase();
  let body=trimmed.lines().skip(1).collect::<Vec<_>>().join("\n");
  structured_fence=opening=="```json"||(opening=="```"&&(body.trim_start().starts_with('{')||body.trim_start().starts_with('[')));
  if structured_fence{
   if !trimmed.ends_with("```")||body.trim()=="```"{return Err(fail());}
   clean=body.trim_end().strip_suffix("```").ok_or_else(fail)?.trim().to_string();
  }
 }
 let value=match serde_json::from_str::<Value>(&clean){
  Ok(value)=>value,
  Err(_)=>{
   if structured_fence||clean.trim_start().starts_with('{')||clean.trim_start().starts_with('[')||lower.starts_with("tool:")||lower.starts_with("tool :"){return Err(fail());}
   return Ok(json!({"reply":trimmed,"replacement":null,"edits":[]}));
  }
 };
 let object=value.as_object().ok_or_else(fail)?;
 if let Some(tool)=object.get("tool"){
  let name=tool.as_str().filter(|name|matches!(*name,"list_files"|"search_project"|"read_file")).ok_or_else(fail)?;
  let mut tool_output=json!({"tool":name});
  for key in ["path","query","startLine","endLine"]{if let Some(value)=object.get(key){tool_output[key]=value.clone();}}
  return Ok(tool_output);
 }
 let reply=object.get("reply").and_then(Value::as_str).filter(|reply|!reply.trim().is_empty()).ok_or_else(fail)?;
 let replacement=object.get("replacement").cloned().unwrap_or(Value::Null);
 if !replacement.is_null()&&!replacement.is_string(){return Err(fail());}
 let edits=object.get("edits").cloned().unwrap_or_else(||json!([]));if !edits.is_array(){return Err(fail());}
 let mut output=json!({"reply":reply,"replacement":replacement,"edits":edits});
 if let Some(summary)=object.get("contextSummary"){output["contextSummary"]=summary.clone();}
 Ok(output)
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
 #[test]fn answer_only_json_and_normal_plaintext_never_create_edits(){for raw in [r#"{"reply":"我还不确定你希望保留哪个结论，可以先确认研究目标吗？"}"#,"目前证据不足，建议先确认研究目标；没有修改也没关系。","```latex\nE=mc^2\n```"]{let mut output=parse_model_output(raw).unwrap();assert!(!output["reply"].as_str().unwrap().is_empty());assert!(output["replacement"].is_null());assert_eq!(output["edits"],json!([]));assert!(take_generated_summary(&mut output).is_none());validate_edit_output(&mut output,"main.tex",true).unwrap();}}
 #[test]fn malformed_json_empty_hidden_analysis_and_unknown_tools_are_not_plain_answers(){for raw in [" ","{\"reply\":","[broken",r#"{"tool":"delete_file","reply":"pretend answer"}"#,r#"{"tool":null,"reply":"pretend answer"}"#,r#"{"reply":"","replacement":null}"#,"```json\n{bad}\n```","<think>private chain</think>answer",r#"{"reply":"<analysis>private chain</analysis>answer"}"#,"null"]{assert!(parse_model_output(raw).is_err(),"{raw}");}}
 #[test]fn valid_structured_edits_tools_and_summary_remain_explicit(){let mut output=parse_model_output(r#"{"reply":"Here is one candidate","edits":[{"filePath":"main.tex","before":"original","after":"candidate"}],"contextSummary":"Goal confirmed; candidate pending","analysis":"private internal field"}"#).unwrap();assert!(output.get("analysis").is_none());assert_eq!(take_generated_summary(&mut output).as_deref(),Some("Goal confirmed; candidate pending"));validate_edit_output(&mut output,"main.tex",true).unwrap();assert_eq!(output["edits"][0]["after"],"candidate");let tool=parse_model_output(r#"{"tool":"search_project","query":"关键词","analysis":"internal"}"#).unwrap();assert_eq!(tool,json!({"tool":"search_project","query":"关键词"}));assert!(tool.get("reply").is_none());let fenced=parse_model_output("```json\n{\"reply\":\"No change needed\"}\n```").unwrap();assert_eq!(fenced["edits"],json!([]));}
 #[test]fn image_retry_is_once_and_only_for_explicit_capability_failure(){let mut input:AICompleteInput=serde_json::from_value(json!({"feature":"chat","selection":"exact source","options":{"mode":"inline-edit","message":"improve","filePath":"main.tex"},"history":[{"role":"user","content":"keep history"}],"image":{"mediaType":"image/png","data":"QUJD"}})).unwrap();input.project_images.push(scribe_shared::ImageInput{media_type:"image/png".into(),data:"REVG".into()});let error=AIStreamStartError::Provider(scribe_ai::AdapterError::HttpStatus{status:400,message:"model does not support images".into()});assert!(may_retry_without_images(&input,false,&error));assert!(!may_retry_without_images(&input,true,&error));let history=input.history.as_ref().unwrap()[0].content.clone();strip_image_inputs(&mut input);assert!(!may_retry_without_images(&input,false,&error));assert!(input.image.is_none()&&input.project_images.is_empty());assert_eq!(input.selection,"exact source");assert_eq!(input.options.as_ref().unwrap()["message"],"improve");assert_eq!(input.history.as_ref().unwrap()[0].content,history);assert_eq!(input.options.as_ref().unwrap()["imagesUnavailable"],"true");let mut result=json!({"reply":"text answer","replacement":null,"edits":[edit("main.tex","exact source","improved")]});validate_edit_output(&mut result,"main.tex",true).unwrap();assert_eq!(result["edits"][0]["after"],"improved");}
 #[test]fn answer_only_output_cannot_manufacture_a_replacement_or_edits(){let mut value=json!({"reply":"answer","replacement":"UNAUTHORIZED FULL FILE","edits":[edit("main.tex","any original","changed")]});validate_edit_output(&mut value,"main.tex",false).unwrap();assert!(value["replacement"].is_null());assert_eq!(value["edits"],json!([]));assert_eq!(value["reply"],"answer");}
 #[test]fn selected_legacy_edit_preserves_preview_without_applying_it(){let mut value=json!({"reply":"preview","replacement":"selected edit"});validate_edit_output(&mut value,"main.tex",true).unwrap();assert_eq!(value["replacement"],"selected edit");}
 #[test]fn multi_edit_can_reference_current_file_beyond_selection_and_disables_legacy_path(){let mut value=json!({"reply":"preview","replacement":"legacy duplicate","edits":[edit("main.tex","an exact paragraph outside the small selection","improved paragraph"),edit("main.tex","second exact sentence","")]});validate_edit_output(&mut value,"main.tex",true).unwrap();assert!(value["replacement"].is_null());assert_eq!(value["edits"].as_array().unwrap().len(),2);}
 #[test]fn rejects_cross_file_unsafe_empty_and_malformed_edits(){for edits in [json!([edit("other.tex","original","replacement")]),json!([edit("main.tex"," ","replacement")]),json!([{"filePath":"main.tex","before":"original"}]),json!("not an array")]{let mut value=json!({"reply":"preview","replacement":null,"edits":edits});assert!(validate_edit_output(&mut value,"main.tex",true).is_err());}let mut value=json!({"reply":"preview","replacement":null,"edits":[edit("../private","original","replacement")]});assert!(validate_edit_output(&mut value,"../private",true).is_err());}
 #[test]fn enforces_edit_count_and_character_budgets(){let mut value=json!({"reply":"preview","replacement":null,"edits":(0..13).map(|n|edit("main.tex",&format!("original {n}"),"replacement")).collect::<Vec<_>>()});assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!([edit("main.tex",&"界".repeat(16001),"replacement")]);assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!([edit("main.tex","original",&"界".repeat(24001))]);assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!((0..3).map(|_|edit("main.tex",&"b".repeat(15000),&"a".repeat(15000))).collect::<Vec<_>>());assert!(validate_edit_output(&mut value,"main.tex",true).is_err());}
}

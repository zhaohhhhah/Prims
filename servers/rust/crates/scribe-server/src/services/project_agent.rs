//! Bounded project-aware assistant with private history and optional read-only file tools.
use axum::response::{sse::{Event,KeepAlive,Sse},IntoResponse,Response};
use futures::StreamExt;
use scribe_shared::{AICompleteInput,ApiError,ApiResult,ErrorCode,ProjectId,UserId};
use serde_json::{json,Value};
use std::{convert::Infallible,time::Duration};
use tokio::sync::mpsc;
use uuid::Uuid;
use scribe_ai::adapter::{NativeToolCall,NativeToolCallDelta,NativeToolTurn};
use crate::state::AppState;
use super::{ai::{AIService,AIStreamStartError},ai_context::{AIContextService,model_history,take_generated_summary,MAX_CONTENT,MAX_RAW},files::FileService,project_context::ProjectContext};

pub async fn start(state:AppState,user:UserId,mut input:AICompleteInput)->ApiResult<Response>{
 let raw=input.options.as_ref().and_then(|o|o.get("projectId")).ok_or_else(||ApiError::validation("缺少项目编号"))?;
 let project=ProjectId::new(Uuid::parse_str(raw).map_err(|_|ApiError::validation("项目编号无效"))?);
 let pool=state.db().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"database unavailable"))?.pool().clone();
 let memory=AIContextService::new(pool.clone());let saved=memory.get(user,project).await?;
 let library=super::agent_library::AgentLibrary::new(pool.clone());
 let initial_preferences=library.preferences(user).await?;let preference_revision=library.preference_revision(user).await?;
 let project_memory=library.memory(user,project).await?;
 let writer=super::membership::assert_can_write(&pool,user,project).await.is_ok();
 if let Some(thread)=input.options.as_ref().and_then(|o|o.get("threadId")){if Uuid::parse_str(thread).ok()!=Some(saved.thread_id){return Err(ApiError::new(ErrorCode::Conflict,"已开始新的项目对话，请刷新后重新发送"));}}
 let current=input.options.as_ref().and_then(|o|o.get("filePath")).cloned().unwrap_or_default();
 let selection_id=input.options.as_ref().and_then(|o|o.get("selectionId")).cloned().unwrap_or_default();
 let allow_files=!input.options.as_ref().and_then(|o|o.get("allowProjectFiles")).is_some_and(|v|v=="false");
 let (history,history_truncated)=model_history(&saved,&current,&selection_id);let history_count=history.len();input.history=Some(history);
 let (project_compiler,project_main): (String,String)=sqlx::query_as("select compiler::text,main_file from public.projects where id=$1").bind(project.into_inner()).fetch_one(&pool).await.map_err(|_|ApiError::validation("找不到项目编译配置"))?;
 let available_engines=if scribe_compile::isolated::available().await{vec!["tectonic","pdflatex","xelatex","lualatex"]}else{vec!["tectonic"]};
 let options=input.options.get_or_insert_with(Default::default);options.insert("projectMainFile".into(),project_main);options.insert("projectAvailableEngines".into(),serde_json::to_string(&available_engines).unwrap());options.insert("projectCompiler".into(),project_compiler);options.insert("writingBrief".into(),saved.brief.clone());options.insert("projectTitle".into(),saved.project_title.clone());options.insert("projectDescription".into(),saved.project_description.clone().unwrap_or_default());options.insert("autoSummary".into(),saved.auto_summary.clone());
 // Client-provided synthetic tool results never bypass the server's read authorization.
 for key in ["agentLibraryReference","agentFileToolsAllowed","agentPreferences","agentCapabilities","projectMemory","skillContent","skills","diagnosticsEnabled","skillsEnabled","memoryEnabled","searchEnabled"]{options.remove(key);}
 options.insert("agentFileToolsAllowed".into(),allow_files.to_string());
 options.insert("agentLibraryReference".into(),library.reference(user,project,&initial_preferences).await?.to_string());
 options.remove("projectTools");options.remove("projectContext");options.remove("projectContinuation");options.remove("projectEditsAllowed");options.remove("projectNativeTools");options.remove("projectNativeTurns");options.remove("projectCompaction");options.remove("projectPendingProposals");options.remove("contextCompaction");options.remove("projectSkillInstalls");options.remove("projectCompacted");options.remove("projectPlan");options.remove("projectSearchSources");options.remove("webReferences");
 if !allow_files{options.remove("documentOutline");}
 let mut context_used=json!({"brief":!saved.brief.trim().is_empty(),"autoSummary":!saved.auto_summary.trim().is_empty(),"historyMessages":history_count,"historyTruncated":history_truncated,"fileTools":allow_files,"currentSection":options.get("currentSection"),"selectionStartLine":options.get("selectionStartLine"),"selectionEndLine":options.get("selectionEndLine"),"documentWindowStartLine":if allow_files{options.get("documentWindowStartLine")}else{None},"documentWindowEndLine":if allow_files{options.get("documentWindowEndLine")}else{None}});
 let storage=state.inner.storage.clone().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"storage unavailable"))?;
 let files=FileService::new(pool.clone(),storage);
 let mut context=if allow_files{Some(ProjectContext::new(&files,user,project).await?)}else{None};
 let crypto=state.inner.ai_crypto.clone().ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"AI unavailable"))?;
 let ai=AIService::new(pool,crypto);let editing_allowed=super::inline_prompt::editing_allowed(&input);
 let project_edits_allowed=writer&&context.is_some()&&!input.options.as_ref().and_then(|options|options.get("readOnly")).is_some_and(|value|value=="true");
 input.options.as_mut().unwrap().insert("projectEditsAllowed".into(),project_edits_allowed.to_string());
 context_used["projectEditsAllowed"]=json!(project_edits_allowed);context_used["selectionAllowed"]=json!(editing_allowed);
 let live=input.options.as_ref().and_then(|o|o.get("currentDocument")).cloned();
 let (tx,rx)=mpsc::channel::<Value>(16);
 tokio::spawn(async move{
  let mut pending=ProposalQueue::default();
  let mut skill_installs=super::skill_install::SkillInstallQueue::default();
  let mut compaction_attempted=false;
  let mut search_session=super::web_search::SearchSession::default();
  let mut plan=super::agent_plan::AgentPlan::default();let mut loop_guard=super::agent_loop_guard::LoopGuard::default();let mut plan_updates=0usize;
  let result=tokio::time::timeout(Duration::from_secs(600),async{
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
   input.options.as_mut().unwrap().insert("projectTools".into(),"enabled".into());
   let mut native_mode=true;let mut native_fallback_used=false;let mut native_turns=Vec::<NativeToolTurn>::new();
   input.options.as_mut().unwrap().insert("projectNativeTools".into(),native_mode.to_string());
   let mut image_fallback_used=false;let mut read_tools_used=0usize;let mut completed=std::collections::HashMap::<String,Value>::new();let mut continuation=Vec::<Value>::new();let mut diagnostics_used=0usize;
   for step in 0..32 {
    let preferences=library.preferences(user).await?;
    input.options.as_mut().unwrap().insert("agentLibraryReference".into(),library.reference(user,project,&preferences).await?.to_string());
    if let Some(context)=context.as_ref(){context.sync(&mut input);}
    input.options.as_mut().unwrap().insert("projectPendingProposals".into(),pending.metadata().to_string());
    input.options.as_mut().unwrap().insert("projectPlan".into(),plan.metadata().to_string());
    input.options.as_mut().unwrap().insert("projectSkillInstalls".into(),skill_installs.metadata().to_string());
    input.options.as_mut().unwrap().insert("projectSearchSources".into(),search_session.metadata().to_string());
    let request=ai.prepare_agent_request(user,input.clone()).await.map_err(AIStreamStartError::into_api_error)?;
    let actual_budget=actual_request_budget(&request);
    if !compaction_attempted&&actual_budget.should_compact{
     compaction_attempted=true;emit(&tx,json!({"status":"正在整理长对话上下文…"})).await?;
     let result=tokio::select!{_=tx.closed()=>return Ok(()),result=run_codex_compaction(&ai,user,&mut input,&mut native_turns,&mut continuation,&pending,context.as_ref())=>result};
     match result{Ok(metadata)=>{emit(&tx,json!({"status":"上下文已整理，继续处理剩余段落","contextCompaction":metadata})).await?;},Err(_)=>{emit(&tx,json!({"warning":{"code":"context_compaction_failed","message":"上下文整理未完成，原始上下文和已有候选均已保留；本轮不会重试摘要。"}})).await?;}}
    }
    if serde_json::to_string(&native_turns).map_or(true,|value|value.len()>800000){return Err(ApiError::validation("本轮工具上下文已达安全上限，请处理已有候选后继续提问"));}
    if let Some(status)=plan.current_status(){emit(&tx,json!({"status":status})).await?;}else if !pending.items.is_empty(){emit(&tx,json!({"status":format!("已提交{}处待审修改，正在处理后续修改",pending.items.len())})).await?;}else if step==0{emit(&tx,json!({"status":"正在确定需要读取的文件和修改范围"})).await?;}else if context.as_ref().is_some_and(|context|context.results.iter().any(|result|result.get("kind").and_then(Value::as_str)==Some("text"))){emit(&tx,json!({"status":"正在核对已读取内容与修改要求"})).await?;}
    let (raw,native_calls)=loop{
     let round=async{
      let mut stream=ai.complete_stream_for_agent(user,input.clone()).await?;let mut text=String::new();let mut tick=0;let mut tools=NativeCalls::default();
      while let Some(chunk)=stream.next().await{
       if matches!(&chunk,Err(scribe_ai::AdapterError::OutputLimit)){tracing::warn!(finish_reason="length",raw_chars=text.chars().count(),"AI output reached model token limit; no content recorded");}
       let chunk=chunk.map_err(AIStreamStartError::Provider)?;text.push_str(&chunk.text);
       for delta in chunk.tool_call_deltas{tools.push(delta).map_err(AIStreamStartError::Request)?;}
       if text.len()>150000{return Err(AIStreamStartError::Request(ApiError::validation("模型输出过长，请缩小问题范围")));}
       if text.len()/500>tick{tick=text.len()/500;emit(&tx,json!({"received":text.chars().count()})).await?;}
      }Ok::<_,AIStreamStartError>((text,tools.finish().map_err(AIStreamStartError::Request)?))
     };
     let result=tokio::select!{_=tx.closed()=>return Ok(()),result=tokio::time::timeout(Duration::from_secs(90),round)=>result.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"模型响应超时，请重试"))?};
     match result{
      Ok(value)=>break value,
      Err(error) if native_mode&&!native_fallback_used&&error.is_tools_unsupported()=>{
       native_fallback_used=true;native_mode=false;input.options.as_mut().unwrap().insert("projectNativeTools".into(),"false".into());
       emit(&tx,json!({"status":"当前接口不支持原生工具，正在使用兼容读取模式…","warning":{"code":"native_tools_unsupported","message":"当前接口明确不支持原生工具，已切换兼容读取模式。"}})).await?;
      },
      Err(error) if may_retry_without_images(&input,image_fallback_used,&error)=>{
       image_fallback_used=true;strip_image_inputs(&mut input);
       if let Some(context)=context.as_mut(){context.disable_images(&mut input);}
       emit(&tx,json!({"warning":{"code":"image_unsupported","message":"当前接口或模型不支持读取图片，已继续使用文字上下文。建议切换支持图片的视觉模型。"},"status":"图片输入暂不可用，正在使用文字上下文继续…"})).await?;
      },
      Err(error)=>return Err(error.into_api_error()),
     }
    };
    private_format_trace(step,&raw);
    let has_native_calls=!native_calls.is_empty();
    let mut output=if has_native_calls{native_calls_output(&native_calls)?}else{parse_model_output(&raw)?};
    let public_status=output.as_object_mut().and_then(|object|object.remove("_publicStatus")).and_then(|value|value.as_str().map(String::from));
    if let Some(tool)=output.get("tool").and_then(Value::as_str){
     if step==31{return Err(ApiError::validation("本轮文件工具调用已达上限，请缩小问题或分次提问"));}
     let calls=if tool=="_read_batch"{output.get("calls").and_then(Value::as_array).cloned().ok_or_else(||ApiError::validation("读取工具批次格式无效"))?}else{vec![output.clone()]};
     if let Some(status)=public_status.as_ref(){emit(&tx,json!({"status":status})).await?;}
     let new_calls=count_new_read_calls(&calls,&completed);
     if new_calls>0{reserve_read_tools(&mut read_tools_used,new_calls)?;}
     let mut new_results=0usize;let before_proposals=pending.items.len();
     let mut round_results=vec![];
     for call in &calls{
      let key=call.to_string();
      if !matches!(call.get("tool").and_then(Value::as_str),Some("web_search"|"list_skills"|"read_skill"|"install_skill"|"read_project_memory"|"run_project_check")){if let Some(previous)=completed.get(&key){round_results.push(json!({"toolCall":call,"toolResult":previous,"reused":true}));continue;}}
      let tool=call.get("tool").and_then(Value::as_str).ok_or_else(||ApiError::validation("读取工具缺少名称"))?;
      let preferences=library.preferences(user).await?;
      if let Err(error)=super::agent_library::gate_tool(tool,&preferences,writer,allow_files){round_results.push(json!({"toolCall":call,"toolResult":{"error":error.message}}));continue;}
      let result=match tool{
       "install_skill"=>{
        emit(&tx,json!({"status":"正在准备待确认的技能安装建议…"})).await?;
        let text_bytes=pending.items.iter().map(|item|item.to_string().len()).sum();
        let prepared=tokio::select!{_=tx.closed()=>return Ok(()),result=skill_installs.prepare(call,text_bytes)=>result};
        match prepared{Ok((proposal,added))=>{pending.reserved_bytes=skill_installs.bytes();if added{emit(&tx,json!({"skillProposal":proposal,"status":"已准备技能建议，等待确认安装"})).await?;}Ok(json!({"ok":true,"proposalId":proposal["id"],"name":proposal["name"],"status":"pending_review","installed":false,"permissionsGranted":false,"note":"Only an installation preview was prepared. No skill was saved or enabled. The user must click Install; do not claim installation."}))},Err(error)=>Err(error)}
       },
       "list_skills"|"read_skill"=>{
        if !preferences.skills_enabled{Err(ApiError::validation("个人技能已关闭"))}else if tool=="list_skills"{emit(&tx,json!({"status":"正在查看已启用的个人技能…"})).await?;library.list(user,true).await}else{emit(&tx,json!({"status":"正在读取相关个人技能…"})).await?;let id=call.get("id").and_then(Value::as_str).and_then(|id|Uuid::parse_str(id).ok()).ok_or_else(output_format_error)?;library.read(user,id,true).await}
       },
       "read_project_memory"=>{
        if !preferences.memory_enabled{Err(ApiError::validation("项目记忆已关闭"))}else{emit(&tx,json!({"status":"正在查看项目笔记…"})).await?;library.memory(user,project).await.map(|memory|json!({"content":memory.content,"revision":memory.revision,"untrusted":true,"permissionsGranted":false}))}
       },
       "run_project_check"=>{
        if !preferences.diagnostics_enabled||!allow_files{Err(ApiError::validation("项目诊断或文件读取已关闭"))}else if !writer{Err(ApiError::forbidden("只读成员不能运行项目诊断"))}else if diagnostics_used>=3{Err(ApiError::validation("本轮最多运行3次项目诊断"))}else{
         diagnostics_used+=1;emit(&tx,json!({"status":match call.get("task").and_then(Value::as_str){Some("latex_check")=>"正在检查已保存项目的LaTeX…",Some("font_check")=>"正在检查项目字体…",_=>"正在查看可用字体…"}})).await?;
         tokio::select!{_=tx.closed()=>return Ok(()),result=tokio::time::timeout(Duration::from_secs(90),super::agent_workbench::run(&state,user,project,call))=>result.unwrap_or_else(|_|Err(ApiError::new(ErrorCode::ServiceUnavailable,"项目诊断超时；已发出取消请求，工作容器另有硬超时保护")))}
        }
       },
       "web_search"=>{
        if !preferences.search_enabled{round_results.push(json!({"toolCall":call,"toolResult":{"error":"公开搜索已关闭"}}));continue;}
        let config=ai.prepare_agent_request(user,input.clone()).await.map_err(AIStreamStartError::into_api_error)?;
        emit(&tx,json!({"status":"正在查找公开参考资料…"})).await?;
        let max_calls=ai.search_max_calls(user).await?;search_session.set_max_calls(max_calls);
        let selected=ai.resolve_search(user,&config).await;
        let found=match selected{Ok(selected)=>tokio::select!{_=tx.closed()=>return Ok(()),result=search_session.search_selected(selected.as_ref(),call)=>result},Err(_)=>Err("[search_config] 独立搜索设置暂不可用，请检查配置")};
        match found{Ok(value)=>{input.options.as_mut().unwrap().insert("projectSearchSources".into(),search_session.metadata().to_string());let count=value.get("results").and_then(Value::as_array).map_or(0,Vec::len);emit(&tx,json!({"status":if value.get("cached")==Some(&Value::Bool(true)){format!("复用已有{}条公开参考来源",count)}else{format!("已找到{}条公开参考来源",count)}})).await?;Ok(value)},Err(message)=>Err(ApiError::validation(message))}
       },
       "update_plan"=>{
        if plan_updates>=8{Err(ApiError::validation("本轮计划更新已达8次，请继续执行已确认步骤或结束本轮"))}else{match plan.update(call){Ok(ack)=>{plan_updates+=1;let metadata=plan.metadata();input.options.as_mut().unwrap().insert("projectPlan".into(),metadata.to_string());if ack.get("changed")==Some(&Value::Bool(true)){emit(&tx,json!({"plan":metadata,"status":plan.current_status().unwrap_or_else(||"已更新本轮处理计划".into())})).await?;}Ok(json!({"ok":true,"changed":ack.get("changed"),"plan":metadata,"note":"Plan progress describes assistant work only; completed does not mean a pending proposal was applied."}))},Err(message)=>Err(ApiError::validation(message))}}
       },
       "compact_context"=>{
        if compaction_attempted{Err(ApiError::validation("本轮已尝试上下文整理，不会重复调用摘要模型；请继续处理候选或结束本轮"))}else{
         compaction_attempted=true;emit(&tx,json!({"status":"正在按Codex方法整理上下文…"})).await?;
         tokio::select!{_=tx.closed()=>return Ok(()),result=run_codex_compaction(&ai,user,&mut input,&mut native_turns,&mut continuation,&pending,context.as_ref())=>result}
        }
       },
       "propose_edit"=>{
        let context=context.as_ref().ok_or_else(||ApiError::validation("本轮未读取项目文件，不能提交精确修改候选"))?;
        match pending.submit(call,context,project_edits_allowed){
         Ok((proposal,is_new))=>{if is_new{emit(&tx,json!({"proposal":proposal,"status":"已提交一段待审修改"})).await?;}Ok(json!({"ok":true,"proposalId":proposal["id"],"filePath":proposal["filePath"],"status":"pending_review","applied":false,"note":"Only a complete pending preview was submitted. The harness has not applied it. Continue other requested small hunks, do not resubmit this proposal."}))},
         Err(error)=>Err(error),
        }
       },
       name if super::agent_history::is_history_tool(name)=>{
        if !allow_files{Err(ApiError::validation("本轮已关闭项目文件读取"))}else{
         emit(&tx,json!({"status":if name=="compare_versions"{"正在比较项目版本…"}else if name=="read_version_file"{"正在读取历史正文…"}else{"正在查看项目版本…"}})).await?;
         tokio::select!{_=tx.closed()=>return Ok(()),result=tokio::time::timeout(Duration::from_secs(30),super::agent_history::run(&state,user,project,call))=>result.unwrap_or_else(|_|Err(ApiError::new(ErrorCode::ServiceUnavailable,"版本读取超时，请缩小范围后重试")))}
        }
       },
       "search_project"=>{emit(&tx,json!({"status":action_status(public_status.as_deref(),"正在查找项目内容…")})).await?;let query=call.get("query").and_then(Value::as_str).unwrap_or("");match context.as_mut(){Some(context)=>context.search(&files,user,query).await,None=>Err(ApiError::validation("本轮已关闭项目文件读取"))}},
       "list_files"=>{emit(&tx,json!({"status":action_status(public_status.as_deref(),"正在列出项目文件…")})).await?;context.as_ref().map(|context|context.listing()).ok_or_else(||ApiError::validation("本轮已关闭项目文件读取"))},
       "read_file"=>{
        let path=call.get("path").and_then(Value::as_str).unwrap_or("");emit(&tx,json!({"status":action_status(public_status.as_deref(),&format!("正在读取：{path}"))})).await?;
        let start=call.get("startLine").and_then(Value::as_u64).map(|n|n as usize);let end=call.get("endLine").and_then(Value::as_u64).map(|n|n as usize);
        let result=match context.as_mut(){Some(context)=>context.read_window(&files,user,path,start,end,&mut input).await,None=>Err(ApiError::validation("本轮已关闭项目文件读取"))};
        if result.is_ok(){emit(&tx,json!({"status":action_status(public_status.as_deref(),&format!("已读取：{path}")),"file":path})).await?;}result
       },_=>return Err(ApiError::validation("只支持项目读取工具和待审 propose_edit；不能直接写正文")),
      };
      let value=match result{Ok(value) if tool=="search_project"=>value,Ok(value)=>value,Err(err)=>json!({"tool":tool,"error":err.message})};
      let same_previous=completed.get(&key).is_some_and(|previous|previous==&value);
      if !same_previous&&value.get("error").is_none()&&value.get("ok")!=Some(&Value::Bool(false))&&((matches!(tool,"list_files"|"search_project"|"read_file"|"list_versions"|"read_version_file"|"compare_versions"|"compact_context"|"list_skills"|"read_skill"|"install_skill"|"read_project_memory"|"run_project_check"))||(tool=="update_plan"&&value.get("changed")==Some(&Value::Bool(true)))||(tool=="web_search"&&value.get("cached")!=Some(&Value::Bool(true)))){new_results+=1;}
      completed.insert(key,value.clone());round_results.push(json!({"toolCall":call,"toolResult":value}));
     }
     if loop_guard.observe(&calls,new_results,pending.items.len().saturating_sub(before_proposals)){return Err(ApiError::validation(super::agent_loop_guard::STOP_REPLY));}
     continuation.push(json!({"role":"assistant","content":if calls.len()==1{calls[0].to_string()}else{json!(calls).to_string()}}));
     continuation.push(json!({"role":"user","content":json!({"serverToolResults":round_results.clone(),"completedCalls":read_tools_used,"remainingBudget":MAX_READ_TOOLS.saturating_sub(read_tools_used),"nextAction":"The tool results above are real. propose_edit results are pending_review, not applied facts. Continue submitting one small requested hunk at a time with propose_edit until the requested scope is covered, then give a short final reply without repeating submitted edits. Do not resubmit proposal IDs or identical before/after. For a rejected proposal, fix the actual source or TeX syntax and retry only when justified. Read only when essential. User accept/reject/undo actions, when supplied in history, are authoritative."}).to_string()}));
     input.options.as_mut().unwrap().insert("projectContinuation".into(),json!(continuation).to_string());
     if has_native_calls{
      let content=if raw.trim().is_empty()||contains_hidden_reasoning(&raw){None}else{Some(raw.clone())};
      native_turns.push(NativeToolTurn::Assistant{content,tool_calls:native_calls.clone()});
      for (call,result) in native_calls.iter().zip(round_results.iter()){
       let value=&result["toolResult"];native_turns.push(NativeToolTurn::Tool{tool_call_id:call.id.clone(),content:value.to_string(),is_error:value.get("error").is_some()||value.get("ok")==Some(&Value::Bool(false))});
      }
      input.options.as_mut().unwrap().insert("projectNativeTurns".into(),serde_json::to_string(&native_turns).map_err(|_|output_format_error())?);
     }else if native_mode{
      // A provider that prints legacy calls instead of using native tools can continue safely
      // through the existing authorized compatibility loop, with the actual results retained.
      native_mode=false;input.options.as_mut().unwrap().insert("projectNativeTools".into(),"false".into());
     }
    }else{
     if !output.get("reply").is_some_and(Value::is_string)||!output.get("replacement").is_some_and(|v|v.is_null()||v.is_string()){return Err(ApiError::validation("模型未返回有效答复或选区方案"));}
     // Exact text edits can target other already-read project files; legacy replacement still needs a selection.
     validate_file_action_proposals(&output)?;
     stamp_compile_settings(&mut output,input.options.as_ref());
     let returned_edits=output.get("edits").and_then(Value::as_array).map_or(0,Vec::len);let returned_replacement=output.get("replacement").is_some_and(Value::is_string);
     tracing::info!(returned_edits,returned_replacement,project_edits_allowed,selection_allowed=editing_allowed,"AI proposal output metadata; no content recorded");
     validate_response_edits(&mut output,&current,editing_allowed,project_edits_allowed,context.as_ref())?;
     let blocked_tex=sanitize_tex_proposals(&mut output,&current);
     if blocked_tex>0{emit(&tx,json!({"warning":{"code":"tex_markdown_blocked","message":"部分LaTeX修改建议包含Markdown格式，已移除这些候选并保留正常答复；正文未改动。"}})).await?;}
     for edit in output.get("edits").and_then(Value::as_array).cloned().unwrap_or_default(){
      if let Some(context)=context.as_ref(){match pending.submit(&edit,context,project_edits_allowed){Ok((proposal,is_new))=>{if is_new{emit(&tx,json!({"proposal":proposal,"status":"已提交一段待审修改"})).await?;}},Err(_)=>{emit(&tx,json!({"warning":{"code":"proposal_limit","message":"部分候选超过本轮有界额度，已保留此前完整候选，可继续分批处理。"}})).await?;}}}
     }
     pending.merge(&mut output);skill_installs.merge(&mut output);
     if let Some(status)=public_status.as_ref(){emit(&tx,json!({"status":status})).await?;}
     let recovered=output.as_object_mut().and_then(|value|value.remove("_formatRecovery")).and_then(|value|value.as_bool()).unwrap_or(false);
     if recovered{emit(&tx,json!({"warning":{"code":"answer_only_recovered","message":"修改建议未完整生成，本轮只保留可读答复；没有任何修改已应用。"}})).await?;}
     let generated_project_memory=output.as_object_mut().and_then(|object|object.remove("projectMemory")).and_then(|value|value.as_str().map(String::from));
     let summary=take_generated_summary(&mut output);
     if output.to_string().chars().count()>MAX_RAW{return Err(ApiError::validation("修改方案过长，请减少修改范围后重试；正文未改动"));}
     let mut project_memory_updated=false;
     if let Some(content)=generated_project_memory{if !tx.is_closed(){let result=tokio::select!{_=tx.closed()=>return Ok(()),result=library.save_generated_memory(user,project,&project_memory,preference_revision,&content)=>result};match result{Ok(updated)=>project_memory_updated=updated,Err(_)=>tracing::warn!("Could not save optional private project notes; answer preserved")}}}
     let mut memory_updated=false;
     if let Some(summary)=summary{
      if tx.is_closed(){return Ok(());}
      // Disconnect drops the transaction future. Persistence failures are optional metadata failures,
      // never a reason to discard a valid answer or modification preview.
      let result=tokio::select!{_=tx.closed()=>return Ok(()),result=memory.save_generated_summary(user,project,&saved,&summary)=>result};
      match result{Ok(updated)=>memory_updated=updated,Err(_)=>tracing::warn!("Could not save generated private writing context; normal reply preserved")}
     }
     let has_preview=output.get("replacement").is_some_and(Value::is_string)||output.get("edits").and_then(Value::as_array).is_some_and(|edits|!edits.is_empty())||output.get("actions").and_then(Value::as_array).is_some_and(|actions|!actions.is_empty());
     emit(&tx,json!({"status":if has_preview{"答复与修改预览已生成"}else{"答复已生成"},"text":output.to_string(),"contextMemoryUpdated":memory_updated,"projectMemoryUpdated":project_memory_updated})).await?;return Ok(());
    }
   }Err(ApiError::validation("本轮已达32轮工具上限，已完成候选仍保留，可继续分批处理"))
  }).await;
  let error=match result{Ok(Err(err))=>Some(err.message),Err(_)=>Some("本轮对话超时，请重试".into()),_=>None};
  if let Some(error)=error{
   if (!pending.items.is_empty()||!skill_installs.items.is_empty())&&!tx.is_closed(){
    let message=format!("后续生成中断，已保留{}处文字建议与{}项待确认技能建议。",pending.items.len(),skill_installs.items.len());
    let mut output=json!({"reply":message,"replacement":null,"edits":[],"actions":[],"partial":true});pending.merge(&mut output);skill_installs.merge(&mut output);
    let _=tx.send(json!({"warning":{"code":"partial_completion","message":message},"partial":true})).await;
    let _=tx.send(json!({"text":output.to_string(),"partial":true,"status":"已保留完整待审候选"})).await;
   }else{let _=tx.send(json!({"error":error})).await;}
  }let _=tx.send(json!({"done":true})).await;
 });
 let stream=futures::stream::unfold(rx,|mut rx|async move{let value=rx.recv().await?;let data=if value.get("done")==Some(&Value::Bool(true)){"[DONE]".into()}else{value.to_string()};Some((Ok::<_,Infallible>(Event::default().data(data)),rx))});
 Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}
fn action_status(custom:Option<&str>,action:&str)->String{match custom{Some(custom)=>format!("{custom} · {action}"),None=>action.to_string()}}
async fn emit(tx:&mpsc::Sender<Value>,value:Value)->ApiResult<()>{tx.send(value).await.map_err(|_|ApiError::validation("请求已取消"))}

const SUPPORT_FILE_ACTIONS:bool=true;
/// Only exact valid protocol JSON can carry edits. Recovery retains a complete reply only.
fn parse_model_output(raw:&str)->ApiResult<Value>{
 let text=raw.trim();if text.is_empty()||contains_hidden_reasoning(text){return Err(output_format_error());}
 if let Some(batch)=parse_read_tool_sequence(text)?{return Ok(batch);}
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
fn actual_request_budget(request:&scribe_ai::CompleteRequest)->super::context_compaction::ContextBudget{
 let text=json!({"messages":request.messages,"tools":request.native_tools,"nativeTurns":request.native_turns}).to_string();super::context_compaction::context_budget(&[],&[],&text)
}
async fn run_codex_compaction(ai:&AIService,user:UserId,input:&mut AICompleteInput,turns:&mut Vec<NativeToolTurn>,continuation:&mut Vec<Value>,pending:&ProposalQueue,context:Option<&ProjectContext>)->ApiResult<Value>{
 let request=ai.prepare_agent_request(user,input.clone()).await.map_err(AIStreamStartError::into_api_error)?;let before=actual_request_budget(&request);
 let latest=input.options.as_ref().and_then(|options|options.get("message")).cloned().unwrap_or_default();let history=input.history.clone().unwrap_or_default();
 let options=input.options.as_ref().unwrap();let clues=context.map(|context|context.results.iter().map(|result|json!({"path":result.get("path"),"kind":result.get("kind"),"startLine":result.get("startLine"),"endLine":result.get("endLine")})).collect::<Vec<_>>()).unwrap_or_default();
 let pins=json!({"currentFile":options.get("filePath"),"selectionId":options.get("selectionId"),"selectionStartLine":options.get("selectionStartLine"),"selectionEndLine":options.get("selectionEndLine"),"pendingProposals":pending.metadata(),"readFileClues":clues,"plan":options.get("projectPlan").and_then(|raw|serde_json::from_str::<Value>(raw).ok()),"searchSources":options.get("projectSearchSources").and_then(|raw|serde_json::from_str::<Value>(raw).ok())});
 let result=super::context_compaction::compact_with_codex(request,&history,turns,&latest,&pins).await.map_err(|error|AIStreamStartError::Provider(error).into_api_error())?;
 let mut next=input.clone();next.history=Some(result.history);let next_turns=result.native_turns;let options=next.options.as_mut().unwrap();options.insert("projectCompaction".into(),result.metadata.to_string());options.insert("projectCompacted".into(),"true".into());options.insert("projectContinuation".into(),"[]".into());options.insert("projectNativeTurns".into(),serde_json::to_string(&next_turns).map_err(|_|output_format_error())?);
 if let Some(context)=context{context.sync(&mut next);}
 let after_request=ai.prepare_agent_request(user,next.clone()).await.map_err(AIStreamStartError::into_api_error)?;let after=actual_request_budget(&after_request);
 *input=next;*turns=next_turns;continuation.clear();
 Ok(json!({"ok":true,"method":"codex_local_compaction","before":before,"after":after,"summaryChars":result.summary.chars().count(),"pendingProposals":pending.items.len(),"note":"User-level recap only. Original current request, read evidence and pending IDs/states remain authoritative; no candidate was applied."}))
}
#[derive(Default)]struct ProposalQueue{items:Vec<Value>,reserved_bytes:usize,ranges:Vec<(String,(usize,usize),(usize,usize))>}
impl ProposalQueue{
 fn metadata(&self)->Value{json!(self.items.iter().map(|item|json!({"id":item["id"],"filePath":item["filePath"],"status":"pending_review","beforeChars":item["before"].as_str().map(|value|value.chars().count()),"afterChars":item["after"].as_str().map(|value|value.chars().count())})).collect::<Vec<_>>())}
 fn submit(&mut self,call:&Value,context:&ProjectContext,allowed:bool)->ApiResult<(Value,bool)>{
  if !allowed{return Err(ApiError::validation("当前项目为只读，不能提交文字修改候选"));}
  let path=call.get("filePath").and_then(Value::as_str).ok_or_else(output_format_error)?;let before=call.get("before").and_then(Value::as_str).ok_or_else(output_format_error)?;let after=call.get("after").and_then(Value::as_str).ok_or_else(output_format_error)?;
  if call.get("tool").and_then(Value::as_str)==Some("propose_edit")&&(before.chars().count()>4000||after.chars().count()>4000||before.chars().count()+after.chars().count()>6000){return Err(ApiError::validation("每次仅提交一小段，建议总计1000字以内；请拆小后重新提交"));}
  let mut value=json!({"reply":"候选待审","replacement":null,"edits":[{"filePath":path,"before":before,"after":after}]});validate_edit_output(&mut value,"",true)?;context.validate_edit_source(path,before)?;
  let after=super::tex_proposal::sanitize_tex_after(path,after).map_err(ApiError::validation)?;
  if let Some(existing)=self.items.iter().find(|item|item["filePath"]==path&&item["before"]==before&&item["after"]==after){return Ok((existing.clone(),false));}
  if self.items.iter().any(|item|item["filePath"]==path&&item["before"]==before){return Err(ApiError::validation("相同原文已有待审候选，请勿重复给出不同替换；继续处理其他段落"));}
  let (start,end)=context.edit_anchor(path,before)?;
  if self.ranges.iter().any(|(old_path,old_start,old_end)|old_path==path&&start<*old_end&&*old_start<end){return Err(ApiError::validation("这处精确修改与已有待审候选重叠，请缩小到未覆盖区域或先处理已有候选"));}
  if self.items.len()>=MAX_EDITS{return Err(ApiError::validation("本轮已提交48处完整候选，请先处理后继续分批修改"));}
  let proposal=json!({"id":Uuid::new_v4().to_string(),"filePath":path,"before":before,"after":after,"status":"pending_review"});
  let bytes=self.items.iter().map(|item|item.to_string().len()).sum::<usize>()+proposal.to_string().len();if bytes>60000usize.saturating_sub(self.reserved_bytes){return Err(ApiError::validation("本轮待审候选已达60KB，请先处理已有候选后继续分批修改"));}
  self.ranges.push((path.to_string(),start,end));self.items.push(proposal.clone());Ok((proposal,true))
 }
 fn merge(&self,output:&mut Value){if self.items.is_empty(){return;}output["edits"]=json!(self.items.iter().map(|item|json!({"filePath":item["filePath"],"before":item["before"],"after":item["after"]})).collect::<Vec<_>>());output["proposalIds"]=json!(self.items.iter().map(|item|item["id"].clone()).collect::<Vec<_>>());output["replacement"]=Value::Null;}
}
const MAX_READ_TOOLS:usize=12;
const MAX_TOOL_CALLS:usize=8;
/// Accumulate provider-native tool fragments without exposing arguments or private reasoning.
#[derive(Default)]struct NativeCalls{calls:std::collections::BTreeMap<usize,NativeToolCall>}
impl NativeCalls{
 fn push(&mut self,delta:NativeToolCallDelta)->ApiResult<()>{
  if delta.index>1024||(!self.calls.contains_key(&delta.index)&&self.calls.len()>=MAX_TOOL_CALLS){return Err(ApiError::validation("单轮最多允许8个有界工具调用"));}
  let call=self.calls.entry(delta.index).or_default();
  if let Some(id)=delta.id{if call.id!=id{call.id.push_str(&id);}}
  if let Some(name)=delta.name{if call.name!=name{call.name.push_str(&name);}}
  if let Some(arguments)=delta.arguments{call.arguments.push_str(&arguments);}
  if call.id.len()>256||call.name.len()>128||call.arguments.len()>(if call.name=="install_skill"{110000}else{32768}){return Err(ApiError::validation("工具调用参数过长，请缩小读取范围"));}
  Ok(())
 }
 fn finish(self)->ApiResult<Vec<NativeToolCall>>{
  let calls=self.calls.into_values().collect::<Vec<_>>();let mut ids=std::collections::HashSet::new();
  for call in &calls{if call.id.is_empty()||call.id.chars().any(char::is_control)||!ids.insert(call.id.clone())||call.name.is_empty(){return Err(ApiError::validation("原生工具调用缺少完整标识或工具名称，本轮未执行"));}}
  Ok(calls)
 }
}
fn native_calls_output(calls:&[NativeToolCall])->ApiResult<Value>{
 if calls.is_empty()||calls.len()>MAX_TOOL_CALLS{return Err(output_format_error());}let mut normalized=vec![];
 for call in calls{
  let allowed:&[&str]=match call.name.as_str(){"list_files"|"compact_context"|"list_skills"|"read_project_memory"=>&[],"search_project"=>&["query"],"read_file"=>&["path","startLine","endLine"],"propose_edit"=>&["filePath","before","after"],"update_plan"=>&["explanation","plan"],"web_search"=>&["query","count"],"read_skill"=>&["id"],"install_skill"=>&["content","url"],"run_project_check"=>&["task","path"],name if super::agent_history::is_history_tool(name)=>super::agent_history::allowed_arguments(name).unwrap(),_=>return Err(ApiError::validation("模型请求了未授权工具，只允许列目录、查找和读取项目文件"))};
  let args:Value=serde_json::from_str(if call.arguments.trim().is_empty()&&matches!(call.name.as_str(),"list_files"|"list_versions"|"compact_context"|"list_skills"|"read_project_memory"){"{}"}else{&call.arguments}).map_err(|_|ApiError::validation("原生工具参数未完整生成，本轮未执行"))?;
  let args=args.as_object().filter(|args|args.keys().all(|key|allowed.contains(&key.as_str()))).ok_or_else(||ApiError::validation("原生工具参数必须是白名单字段组成的完整对象"))?;
  let mut value=Value::Object(args.clone());value["tool"]=json!(call.name);normalized.push(value);
 }
 parse_read_tool_sequence(&json!(normalized).to_string())?.ok_or_else(output_format_error)
}

fn count_new_read_calls(calls:&[Value],completed:&std::collections::HashMap<String,Value>)->usize{calls.iter().filter(|call|call.get("tool").and_then(Value::as_str).is_some_and(|name|matches!(name,"read_file"|"list_files"|"search_project"|"list_versions"|"read_version_file"|"compare_versions"|"list_skills"|"read_skill"|"read_project_memory"))).map(Value::to_string).filter(|key|!completed.contains_key(key)).collect::<std::collections::HashSet<_>>().len()}
fn reserve_read_tools(used:&mut usize,count:usize)->ApiResult<()>{if count==0||used.saturating_add(count)>MAX_READ_TOOLS{return Err(ApiError::validation("本轮最多读取或查找 12 次，请缩小范围后继续提问"));}*used+=count;Ok(())}
/// Accept complete read-only JSON sequences/arrays; validate the whole batch before any I/O.
fn parse_read_tool_sequence(raw:&str)->ApiResult<Option<Value>>{
 let clean=raw.lines().filter(|line|!matches!(line.trim().to_ascii_lowercase().as_str(),"```"|"```json")).collect::<Vec<_>>().join("\n");
 let values=match serde_json::Deserializer::from_str(clean.trim()).into_iter::<Value>().collect::<Result<Vec<_>,_>>(){Ok(values)=>values,Err(_)=>return Ok(None)};
 let batch=if values.len()==1{if let Some(array)=values[0].as_array(){array.clone()}else{return Ok(None)}}else if values.len()>1{values}else{return Ok(None)};
 let mut calls=vec![];let mut public_status=None;
 for mut value in batch{
  // Some legacy models mix a human-facing note with read commands. Notes never execute,
  // and a string containing tool JSON is not promoted into an authorized call.
  if let Some(note)=value.as_str(){if contains_hidden_reasoning(note){return Err(output_format_error());}continue;}
  if value.get("type").and_then(Value::as_str)==Some("text")&&value.get("text").is_some_and(Value::is_string){continue;}
  if value.get("type").and_then(Value::as_str)==Some("tool_use"){
   let name=value.get("name").and_then(Value::as_str).ok_or_else(output_format_error)?.to_string();
   let args=value.get("input").and_then(Value::as_object).ok_or_else(output_format_error)?.clone();
   if args.contains_key("tool"){return Err(output_format_error());}value=Value::Object(args);value["tool"]=json!(name);
  }
  let status=super::project_progress::take_public_status(&mut value);if public_status.is_none(){public_status=status;}
  let item=value.as_object().ok_or_else(||ApiError::validation("读取工具批次必须由完整 JSON 对象组成"))?;
  if let Some(name)=item.get("tool").filter(|value|!value.is_null()){
   let name=name.as_str().filter(|name|matches!(*name,"list_files"|"search_project"|"read_file"|"propose_edit"|"compact_context"|"update_plan"|"web_search"|"list_skills"|"read_skill"|"install_skill"|"read_project_memory"|"run_project_check"|"list_versions"|"read_version_file"|"compare_versions")).ok_or_else(||ApiError::validation("工具批次只允许 list_files、search_project、read_file"))?;
   let mut call=json!({"tool":name});
   match name{
    name if super::agent_history::is_history_tool(name)=>{call=super::agent_history::normalize_call(&value)?;},
    "list_skills"|"read_skill"|"install_skill"|"read_project_memory"|"run_project_check"=>{super::agent_library::validate_tool(&value)?;for key in ["id","task","path","content","url"]{if let Some(value)=item.get(key){call[key]=value.clone();}}},
    "web_search"=>{for key in ["query","count"]{if let Some(value)=item.get(key){call[key]=value.clone();}}},
    "update_plan"=>{for key in ["explanation","plan"]{if let Some(value)=item.get(key){call[key]=value.clone();}}},
    "propose_edit"=>{for key in ["filePath","before","after"]{call[key]=item.get(key).filter(|value|value.is_string()).cloned().ok_or_else(||ApiError::validation("propose_edit需要完整filePath、before、after字符串"))?;}},
    "read_file"=>{let path=item.get("path").and_then(Value::as_str).ok_or_else(||ApiError::validation("read_file 缺少项目相对路径"))?;super::project_context::validate_path(path)?;call["path"]=json!(path);for key in ["startLine","endLine"]{if let Some(value)=item.get(key){if !value.is_u64(){return Err(ApiError::validation("read_file 行号必须为正整数"));}call[key]=value.clone();}}},
    "search_project"=>{let query=item.get("query").and_then(Value::as_str).ok_or_else(||ApiError::validation("search_project 缺少关键词"))?;if query.trim().is_empty()||query.chars().count()>200{return Err(ApiError::validation("查找关键词需要 1–200 个字符"));}call["query"]=json!(query);},_=>{},
   }
   calls.push(call);
  }else if !item.get("reply").is_some_and(Value::is_string){return Err(ApiError::validation("工具批次包含无效对象"));}
  // Any premature final answer is discarded. The model must first see authorized tool results.
 }
 if calls.is_empty(){return Err(output_format_error());}if calls.len()>MAX_TOOL_CALLS{return Err(ApiError::validation("单轮工具批次最多8个调用"));}
 let mut output=json!({"tool":"_read_batch","calls":calls});if let Some(status)=public_status{output["_publicStatus"]=json!(status);}Ok(Some(output))
}
/// Candidate operator opt-in only; response bodies never enter ordinary logs.
fn private_format_trace(round:usize,raw:&str){
 if std::env::var("PRIMS_AI_FORMAT_TRACE").ok().as_deref()!=Some("1"){return;}
 #[cfg(unix)]{
  use std::{fs::OpenOptions,io::Write,os::unix::fs::{OpenOptionsExt,PermissionsExt}};
  let path=std::path::Path::new("/tmp/prims-ai-format-trace.jsonl");
  if let Ok(meta)=std::fs::symlink_metadata(path){if !meta.is_file()||meta.file_type().is_symlink()||meta.permissions().mode()&0o077!=0{return;}}
  let line=match serde_json::to_string(&json!({"round":round,"raw_chars":raw.chars().count(),"response":raw})){Ok(line)=>line+"\n",Err(_)=>return};
  let used=std::fs::metadata(path).map(|meta|meta.len()).unwrap_or(0);if used.saturating_add(line.len() as u64)>150000{return;}
  if let Ok(mut file)=OpenOptions::new().create(true).append(true).mode(0o600).open(path){let _=file.write_all(line.as_bytes());}
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
fn normalize_model_value(mut value:Value,trusted:bool)->ApiResult<Value>{
 let status=super::project_progress::take_public_status(&mut value);
 let mut output=normalize_model_value_without_status(value,trusted)?;
 if trusted{if let Some(status)=status{output["_publicStatus"]=json!(status);}}
 Ok(output)
}
fn normalize_model_value_without_status(value:Value,trusted:bool)->ApiResult<Value>{
 if let Some(reply)=value.as_str(){return answer_only(reply,false);}
 let object=value.as_object().ok_or_else(output_format_error)?;
 if let Some(tool)=object.get("tool").filter(|tool|!tool.is_null()){
  if !trusted{return Err(output_format_error());}
  let name=tool.as_str().filter(|name|matches!(*name,"list_files"|"search_project"|"read_file"|"propose_edit"|"compact_context"|"update_plan"|"web_search"|"list_skills"|"read_skill"|"install_skill"|"read_project_memory"|"run_project_check"|"list_versions"|"read_version_file"|"compare_versions")).ok_or_else(output_format_error)?;
  if super::agent_history::is_history_tool(name){return super::agent_history::normalize_call(&value);}
  if matches!(name,"list_skills"|"read_skill"|"install_skill"|"read_project_memory"|"run_project_check"){super::agent_library::validate_tool(&value)?;}
  let mut output=json!({"tool":name});for key in ["id","task","path","content","url","query","startLine","endLine","filePath","before","after","explanation","plan","count"]{if let Some(value)=object.get(key){output[key]=value.clone();}}
  if name=="read_file"{let path=output.get("path").and_then(Value::as_str).ok_or_else(output_format_error)?;super::project_context::validate_path(path)?;for key in ["startLine","endLine"]{if output.get(key).is_some_and(|value|!value.is_u64()){return Err(output_format_error());}}}
  if name=="search_project"&&!output.get("query").and_then(Value::as_str).is_some_and(|query|!query.trim().is_empty()&&query.chars().count()<=200){return Err(output_format_error());}
  return Ok(output);
 }
 let reply=object.get("reply").and_then(Value::as_str).ok_or_else(output_format_error)?;
 if !trusted{return answer_only(reply,true);}let _=answer_only(reply,false)?;
 let replacement=object.get("replacement").cloned().unwrap_or(Value::Null);
 let edits=object.get("edits").filter(|value|!value.is_null()).cloned().unwrap_or_else(||json!([]));
 if (!replacement.is_null()&&!replacement.is_string())||!edits.is_array(){return answer_only(reply,true);}
 let mut output=json!({"reply":reply,"replacement":replacement,"edits":edits});
 if SUPPORT_FILE_ACTIONS{let actions=object.get("actions").filter(|value|!value.is_null()).cloned().unwrap_or_else(||json!([]));if !actions.is_array(){return answer_only(reply,true);}output["actions"]=actions;}
 if let Some(content)=object.get("projectMemory").filter(|value|value.is_string()){output["projectMemory"]=content.clone();}
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
/// File actions are proposal data only. No selection is needed, and no tool executes them.
/// The separate authenticated prepare/apply service owns permissions, existence, source snapshots
/// and version-history checks; this parser only bounds and validates the proposal contract.
fn validate_file_action_proposals(output:&Value)->ApiResult<()>{
 let Some(value)=output.get("actions")else{return Ok(())};let actions=value.as_array().ok_or_else(||ApiError::validation("文件整理建议必须是 actions 数组"))?;
 if actions.len()>12{return Err(ApiError::validation("每轮最多预览 12 个文件操作，请分次处理"));}
 let mut total=0usize;
 for action in actions{
  let item=action.as_object().ok_or_else(||ApiError::validation("文件操作建议格式不正确"))?;
  let kind=item.get("type").and_then(Value::as_str).ok_or_else(||ApiError::validation("文件操作建议缺少 type"))?;
  if kind=="set_compile_settings"{
   if item.len()!=3||item.keys().any(|key|!matches!(key.as_str(),"type"|"compiler"|"mainFile"))||!item.get("compiler").and_then(Value::as_str).is_some_and(|value|matches!(value,"tectonic"|"xelatex"|"lualatex"|"pdflatex")){return Err(ApiError::validation("编译设置建议仅接受compiler和mainFile"));}
   let main=item.get("mainFile").and_then(Value::as_str).ok_or_else(||ApiError::validation("编译设置建议缺少mainFile"))?;validate_action_path(main)?;if !main.to_ascii_lowercase().ends_with(".tex"){return Err(ApiError::validation("主文件必须是.tex文件"));}continue;
  }
  let path=item.get("path").and_then(Value::as_str).ok_or_else(||ApiError::validation("文件操作建议缺少相对路径"))?;
  validate_action_path(path)?;total+=path.chars().count();
  match kind{
   "create_file"=>{
    if item.len()!=3||item.keys().any(|key|!matches!(key.as_str(),"type"|"path"|"content")){return Err(ApiError::validation("新建文件建议仅支持 type、path、content"));}
    let content=item.get("content").and_then(Value::as_str).ok_or_else(||ApiError::validation("新建文件建议缺少完整 content"))?;
    if content.chars().count()>40000{return Err(ApiError::validation("每个新文件内容最多 40000 字符"));}total+=content.chars().count();
   },
   "create_directory"=>{if item.len()!=2||item.keys().any(|key|!matches!(key.as_str(),"type"|"path")){return Err(ApiError::validation("新建目录建议仅支持 type、path"));}},
   "move_file"=>{
    if item.len()!=3||item.keys().any(|key|!matches!(key.as_str(),"type"|"path"|"newPath")){return Err(ApiError::validation("移动/重命名建议仅支持 type、path、newPath"));}
    let destination=item.get("newPath").and_then(Value::as_str).ok_or_else(||ApiError::validation("移动/重命名建议缺少 newPath"))?;validate_action_path(destination)?;total+=destination.chars().count();
   },_=>return Err(ApiError::validation("只支持新建文件、创建目录、移动或重命名三个建议类型")),
  }
  if total>80000{return Err(ApiError::validation("本轮文件建议文本总量过大，请分次处理"));}
 }
 Ok(())
}
fn stamp_compile_settings(output:&mut Value,options:Option<&std::collections::HashMap<String,String>>){
 let Some(options)=options else{return};let (Some(compiler),Some(main))=(options.get("projectCompiler"),options.get("projectMainFile"))else{return};
 if let Some(actions)=output.get_mut("actions").and_then(Value::as_array_mut){for action in actions{if action.get("type").and_then(Value::as_str)==Some("set_compile_settings"){action["expected"]=json!({"compiler":compiler,"mainFile":main});}}}
}
fn validate_action_path(path:&str)->ApiResult<()>{
 super::project_context::validate_path(path)?;
 if path.chars().any(char::is_control)||(path.as_bytes().get(1)==Some(&b':')&&path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)){return Err(ApiError::validation("文件建议只允许安全的项目相对路径"));}Ok(())
}
const MAX_EDITS:usize=48;
const MAX_EDIT_BEFORE:usize=16000;
const MAX_EDIT_AFTER:usize=24000;
const MAX_EDIT_TOTAL:usize=80000;
/// The selection identifies the question; explicit exact-text edits may target other relevant parts
/// of any already-read text file in this authorized project. The client must resolve unique, non-overlapping anchors against its full live
/// snapshot before showing/applying a preview. This server never writes document content.
fn validate_edit_output(output:&mut Value,_current_file:&str,editing_allowed:bool)->ApiResult<()>{
 if output.get("reply").and_then(Value::as_str).is_some_and(|v|v.chars().count()>MAX_CONTENT){return Err(ApiError::validation("答复过长，请缩小问题范围"));}
 if !editing_allowed{output["replacement"]=Value::Null;output["edits"]=json!([]);return Ok(());}
 if output.get("replacement").and_then(Value::as_str).is_some_and(|v|v.chars().count()>MAX_EDIT_AFTER){return Err(ApiError::validation("修改方案过长，请分次修改"));}
 let Some(value)=output.get("edits")else{return Ok(())};
 let edits=value.as_array().ok_or_else(||ApiError::validation("模型修改建议必须为 edits 数组，请重试；正文未改动"))?;
 if edits.len()>MAX_EDITS{return Err(ApiError::validation("每轮最多预览48处修改，请缩小问题范围"));}
 let has_edits=!edits.is_empty();let mut total=0usize;
 for edit in edits{
  let object=edit.as_object().ok_or_else(||ApiError::validation("模型修改建议格式不正确；正文未改动"))?;
  if object.len()!=3||object.keys().any(|k|!matches!(k.as_str(),"filePath"|"before"|"after")){return Err(ApiError::validation("修改建议只能包含 filePath、before、after"));}
  let path=object.get("filePath").and_then(Value::as_str).ok_or_else(||ApiError::validation("修改建议缺少当前文件路径"))?;
  let before=object.get("before").and_then(Value::as_str).ok_or_else(||ApiError::validation("修改建议缺少精确原文"))?;
  let after=object.get("after").and_then(Value::as_str).ok_or_else(||ApiError::validation("修改建议缺少替换文本"))?;
  if path.chars().any(char::is_control){return Err(ApiError::validation("修改建议的文件路径格式不正确"));}
  validate_action_path(path)?;
  let before_count=before.chars().count();let after_count=after.chars().count();
  if before.trim().is_empty()||before_count>MAX_EDIT_BEFORE||after_count>MAX_EDIT_AFTER{return Err(ApiError::validation("每处修改需要非空精确原文（最多 16000 字符），替换文本最多 24000 字符"));}
  total+=before_count+after_count;if total>MAX_EDIT_TOTAL{return Err(ApiError::validation("本轮修改建议总量过大，请分次修改"));}
 }
 // There is one proposal representation per response, preventing duplicate execution paths.
 if has_edits{output["replacement"]=Value::Null;}Ok(())
}
/// Source text is never sent through a Markdown renderer or guessed into TeX syntax.
/// Strip only a deterministic outer TeX fence; reject just the affected proposal otherwise.
fn sanitize_tex_proposals(output:&mut Value,current_file:&str)->usize{
 let mut blocked=0usize;
 if let Some(after)=output.get("replacement").and_then(Value::as_str){match super::tex_proposal::sanitize_tex_after(current_file,after){Ok(after)=>output["replacement"]=json!(after),Err(_)=>{output["replacement"]=Value::Null;blocked+=1;}}}
 if let Some(edits)=output.get_mut("edits").and_then(Value::as_array_mut){edits.retain_mut(|edit|{let Some(path)=edit.get("filePath").and_then(Value::as_str)else{return true};let Some(after)=edit.get("after").and_then(Value::as_str)else{return true};match super::tex_proposal::sanitize_tex_after(path,after){Ok(after)=>{edit["after"]=json!(after);true},Err(_)=>{blocked+=1;false}}});}
 if let Some(actions)=output.get_mut("actions").and_then(Value::as_array_mut){actions.retain_mut(|action|{if action.get("type").and_then(Value::as_str)!=Some("create_file"){return true;}let Some(path)=action.get("path").and_then(Value::as_str)else{return true};let Some(content)=action.get("content").and_then(Value::as_str)else{return true};match super::tex_proposal::sanitize_tex_after(path,content){Ok(content)=>{action["content"]=json!(content);true},Err(_)=>{blocked+=1;false}}});}
 if blocked>0{let notice=format!("有 {blocked} 处LaTeX修改建议含Markdown格式，已移除这些候选。说明答复和其余安全建议已保留，正文尚未改动；可继续要求模型用LaTeX语法重新生成。");let reply=output.get("reply").and_then(Value::as_str).unwrap_or("");let remaining=MAX_CONTENT.saturating_sub(notice.chars().count()+2);output["reply"]=json!(format!("{}\n\n{}",reply.chars().take(remaining).collect::<String>(),notice));}
 blocked
}
fn validate_response_edits(output:&mut Value,current:&str,selection_allowed:bool,project_allowed:bool,context:Option<&ProjectContext>)->ApiResult<()>{
 if !selection_allowed{output["replacement"]=Value::Null;}
 validate_edit_output(output,current,project_allowed||selection_allowed)?;
 validate_project_edit_targets(output,context)
}
/// Model proposals do not write storage. Every target must come from this request's
/// server-authorized listing and an actually loaded exact source window.
fn validate_project_edit_targets(output:&Value,context:Option<&ProjectContext>)->ApiResult<()>{
 let Some(edits)=output.get("edits").and_then(Value::as_array).filter(|edits|!edits.is_empty())else{return Ok(())};
 let context=context.ok_or_else(||ApiError::validation("修改项目文件前需先读取当前项目的真实文本"))?;
 for edit in edits{let path=edit.get("filePath").and_then(Value::as_str).ok_or_else(output_format_error)?;let before=edit.get("before").and_then(Value::as_str).ok_or_else(output_format_error)?;context.validate_edit_source(path,before)?;}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 fn edit(path:&str,before:&str,after:&str)->Value{json!({"filePath":path,"before":before,"after":after})}
 fn known_context()->ProjectContext{
  let project=ProjectId::new(Uuid::new_v4());let file=serde_json::from_value(json!({"id":Uuid::new_v4(),"projectId":project,"path":"chapters/other.tex","type":"tex","sizeBytes":30,"createdBy":null,"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"})).unwrap();
  ProjectContext{project,files:vec![file],loaded:std::collections::HashSet::new(),text_chars:30,image_bytes:0,images_disabled:false,results:vec![json!({"kind":"text","path":"chapters/other.tex","content":"heading\nexact other source\nend"})]}
 }
 #[test]fn multi_file_edits_allow_read_owned_text_without_a_selection_but_never_unknown_source(){
  let context=known_context();let mut output=json!({"reply":"two-file proposal","replacement":"MUST_NOT_REPLACE","edits":[edit("chapters/other.tex","exact other source","proposed revision")]});
  validate_response_edits(&mut output,"main.tex",false,true,Some(&context)).unwrap();assert_eq!(output["edits"].as_array().unwrap().len(),1);assert!(output["replacement"].is_null());
  for (path,before) in [("outside.tex","exact other source"),("chapters/other.tex","invented original"),("../secret","exact other source")]{let mut invalid=json!({"reply":"candidate","edits":[edit(path,before,"after")]});assert!(validate_response_edits(&mut invalid,"main.tex",false,true,Some(&context)).is_err());}
  let mut viewer=json!({"reply":"explain","replacement":"no","edits":[edit("chapters/other.tex","exact other source","change")]});validate_response_edits(&mut viewer,"main.tex",false,false,Some(&context)).unwrap();assert_eq!(viewer["edits"],json!([]));assert!(viewer["replacement"].is_null());
  let mut unavailable=json!({"reply":"candidate","edits":[edit("chapters/other.tex","exact other source","change")]});assert!(validate_response_edits(&mut unavailable,"",false,true,None).is_err());
 }
 #[test]fn native_tool_fragments_merge_and_never_execute_unknown_or_incomplete_calls(){
  let mut calls=NativeCalls::default();calls.push(NativeToolCallDelta{index:1,id:Some("native-1".into()),name:Some("read_file".into()),arguments:Some("{\"path\":".into())}).unwrap();calls.push(NativeToolCallDelta{index:1,id:None,name:None,arguments:Some("\"chapters/a.tex\"}".into())}).unwrap();let merged=calls.finish().unwrap();let output=native_calls_output(&merged).unwrap();assert_eq!(output["calls"][0]["path"],"chapters/a.tex");
  for (name,args) in [("write_file","{}"),("read_file","{\"path\":"),("read_file","{\"path\":\"../outside\"}"),("read_file","{\"path\":\"a.tex\",\"shell\":\"no\"}")]{assert!(native_calls_output(&[NativeToolCall{id:"id".into(),name:name.into(),arguments:args.into()}]).is_err());}
 }
 #[test]fn soft_read_batches_may_contain_notes_but_never_promote_them_into_tools(){
  for raw in [r#"["我先读取相关章节",{"tool":"read_file","path":"a.tex"}]"#,r#"[{"type":"text","text":"先核对内容"},{"type":"tool_use","id":"legacy-id","name":"read_file","input":{"path":"a.tex"}}]"#]{let value=parse_model_output(raw).unwrap();assert_eq!(value["calls"].as_array().unwrap().len(),1);assert_eq!(value["calls"][0]["path"],"a.tex");}
  assert!(parse_model_output(r#"["note",{"tool":"write_file","path":"a.tex"}]"#).is_err());
 }
 #[test]fn tex_guard_preserves_reply_before_and_safe_hunks_while_blocking_only_markdown_candidates(){
  let mut output=json!({"reply":"**说明可以使用Markdown**","replacement":null,"edits":[edit("main.tex","exact first source","```latex\n\\section{Safe}\n```"),edit("other.tex","exact second source","# Markdown heading"),edit("notes.md","original markdown","# Allowed markdown")],"actions":[{"type":"create_file","path":"new.tex","content":"**not TeX**"}]});
  assert_eq!(sanitize_tex_proposals(&mut output,"main.tex"),2);assert!(output["reply"].as_str().unwrap().starts_with("**说明可以使用Markdown**"));assert_eq!(output["edits"].as_array().unwrap().len(),2);assert_eq!(output["edits"][0]["before"],"exact first source");assert_eq!(output["edits"][0]["after"],"\\section{Safe}");assert_eq!(output["edits"][1]["after"],"# Allowed markdown");assert_eq!(output["actions"],json!([]));
 }
 #[test]fn pending_hunks_require_unique_exact_positions_and_reject_partial_overlap(){
  let mut context=known_context();context.results[0]["content"]=json!("abcdef other abc");let mut queue=ProposalQueue::default();let proposal=|before:&str|json!({"tool":"propose_edit","filePath":"chapters/other.tex","before":before,"after":"new"});assert!(queue.submit(&proposal("abcde"),&context,true).is_ok());assert!(queue.submit(&proposal("cdef"),&context,true).is_err());assert!(queue.submit(&proposal(" other "),&context,true).is_ok());assert!(queue.submit(&proposal("abc"),&context,true).is_err());assert_eq!(queue.items.len(),2);
 }
 #[test]fn incremental_proposals_are_validated_pending_idempotent_and_separate_from_read_budget(){
  let context=known_context();let mut queue=ProposalQueue::default();let call=json!({"tool":"propose_edit","filePath":"chapters/other.tex","before":"exact other source","after":"revised source"});let (first,fresh)=queue.submit(&call,&context,true).unwrap();assert!(fresh);let (same,fresh)=queue.submit(&call,&context,true).unwrap();assert!(!fresh);assert_eq!(first["id"],same["id"]);assert_eq!(first["status"],"pending_review");
  assert!(queue.submit(&json!({"tool":"propose_edit","filePath":"chapters/other.tex","before":"unknown source","after":"change"}),&context,true).is_err());assert!(queue.submit(&json!({"tool":"propose_edit","filePath":"chapters/other.tex","before":"exact other source","after":"# Markdown"}),&context,true).is_err());assert!(queue.submit(&call,&context,false).is_err());assert_eq!(queue.items.len(),1);
  let mut output=json!({"reply":"partial","edits":[],"replacement":null,"partial":true});queue.merge(&mut output);assert_eq!(output["edits"].as_array().unwrap().len(),1);assert_eq!(output["proposalIds"][0],first["id"]);assert_eq!(count_new_read_calls(&[call],&std::collections::HashMap::new()),0);
 }
 #[test]fn file_actions_can_be_previewed_without_a_selection_but_never_become_tools(){let mut value=parse_model_output(r#"{"reply":"可以这样整理，等待你逐项确认","actions":[{"type":"create_directory","path":"chapters"},{"type":"create_file","path":"chapters/new.tex","content":"proposed content"},{"type":"move_file","path":"old.tex","newPath":"chapters/old.tex"}]}"#).unwrap();validate_file_action_proposals(&value).unwrap();validate_edit_output(&mut value,"",false).unwrap();assert_eq!(value["actions"].as_array().unwrap().len(),3);assert!(value["replacement"].is_null());assert_eq!(value["edits"],json!([]));assert!(parse_model_output(r#"{"tool":"create_file","path":"new.tex","content":"no execution"}"#).is_err());}
 #[test]fn file_action_paths_shape_and_limits_are_strict(){for action in [json!({"type":"create_file","path":"../outside","content":"x"}),json!({"type":"create_directory","path":"C:/system"}),json!({"type":"move_file","path":"main.tex","newPath":"/tmp/new.tex"}),json!({"type":"delete_file","path":"main.tex"}),json!({"type":"create_file","path":"new.tex","content":"x","overwrite":true}),json!({"type":"create_file","path":"new.tex","content":"界".repeat(40001)})]{assert!(validate_file_action_proposals(&json!({"actions":[action]})).is_err());}assert!(validate_file_action_proposals(&json!({"actions":(0..13).map(|n|json!({"type":"create_directory","path":format!("dir{n}")})).collect::<Vec<_>>()})).is_err());let oversized=json!({"actions":[{"type":"create_file","path":"one.tex","content":"a".repeat(40000)},{"type":"create_file","path":"two.tex","content":"b".repeat(40000)}]});assert!(validate_file_action_proposals(&oversized).is_err());let value=parse_model_output(r#"{"reply":"只解释，不需要文件变更"}"#).unwrap();assert_eq!(value["actions"],json!([]));}
 #[test]fn public_progress_survives_the_actual_tool_event(){assert_eq!(action_status(Some("正在核对引用"),"正在读取：refs.bib"),"正在核对引用 · 正在读取：refs.bib");assert_eq!(action_status(None,"正在查找项目内容…"),"正在查找项目内容…");}
 #[test]fn progress_metadata_is_validated_separate_from_tools_and_final_history(){
  let mut value=parse_model_output(r#"{"reply":"safe answer","publicStatus":"正在核对相关文件"}"#).unwrap();assert_eq!(value.as_object_mut().unwrap().remove("_publicStatus"),Some(json!("正在核对相关文件")));assert!(value.get("publicStatus").is_none());assert!(value.get("status").is_none());
  let value=parse_model_output(r#"[{"tool":"read_file","path":"a.tex","publicStatus":"正在查看参考内容"},{"tool":"read_file","path":"b.tex"}]"#).unwrap();assert_eq!(value["_publicStatus"],"正在查看参考内容");assert!(value["calls"][0].get("publicStatus").is_none());
  assert!(parse_model_output(r#"{"tool":"write_file","publicStatus":"trying"}"#).is_err());
  assert!(parse_model_output(r#"{"reply":"safe","publicStatus":"<think>hidden</think>"}"#).is_err()); // The outer hidden-reasoning guard stays stricter than optional metadata filtering.
 }
 #[test]fn completed_reads_reuse_results_without_spending_new_budget(){let a=json!({"tool":"read_file","path":"a.tex"});let b=json!({"tool":"read_file","path":"b.tex"});let mut completed=std::collections::HashMap::new();assert_eq!(count_new_read_calls(&[a.clone(),a.clone(),b.clone()],&completed),2);completed.insert(a.to_string(),json!({"content":"authorized"}));assert_eq!(count_new_read_calls(&[a.clone(),b.clone()],&completed),1);completed.insert(b.to_string(),json!({"content":"authorized"}));assert_eq!(count_new_read_calls(&[a,b],&completed),0);}
 #[test]fn complete_read_tool_sequences_execute_as_one_authorized_batch_before_answer(){let first=json!({"tool":"read_file","path":"chapters/a.tex"});let second=json!({"tool":"read_file","path":"chapters/b.tex","startLine":2,"endLine":8});for raw in [format!("{}\n{}",first,second),json!([first.clone(),second.clone()]).to_string(),format!("```json\n{}\n```\n```json\n{}\n```",first,second),format!("{}\n{}\n{}",first,second,json!({"reply":"premature","replacement":"NEVER_APPLY"}))]{let value=parse_model_output(&raw).unwrap();assert_eq!(value["tool"],"_read_batch");assert_eq!(value["calls"].as_array().unwrap().len(),2);assert!(value.get("reply").is_none());}let single=parse_model_output(&first.to_string()).unwrap();assert_eq!(single["tool"],"read_file");}
 #[test]fn personal_tools_work_in_native_and_compatibility_protocol_without_authority_fields(){
  for value in [json!({"tool":"list_skills"}),json!({"tool":"read_skill","id":Uuid::new_v4()}),json!({"tool":"read_project_memory"}),json!({"tool":"run_project_check","task":"latex_check","path":"main.tex"})]{
   assert!(parse_model_output(&value.to_string()).is_ok());let tool=value["tool"].as_str().unwrap();let mut args=value.clone();args.as_object_mut().unwrap().remove("tool");let native=NativeToolCall{id:Uuid::new_v4().to_string(),name:tool.into(),arguments:args.to_string()};assert!(native_calls_output(&[native]).is_ok());
  }
  for value in [json!({"tool":"read_skill","id":Uuid::new_v4(),"userId":Uuid::new_v4()}),json!({"tool":"run_project_check","task":"sh"}),json!({"tool":"read_project_memory","projectId":Uuid::new_v4()})]{assert!(parse_model_output(&value.to_string()).is_err());}
  let parsed=parse_model_output(&json!({"reply":"Useful factual response","projectMemory":"Audience: graduate students."}).to_string()).unwrap();assert_eq!(parsed["projectMemory"],"Audience: graduate students.");
 }
 #[test]fn read_tool_batches_reject_unknown_write_truncation_and_enforce_total_budget(){let first=json!({"tool":"read_file","path":"main.tex"});for raw in [format!("{}\n{}",first,json!({"tool":"delete_file","path":"main.tex"})),format!("{}\n{{\"tool\":",first),json!([{"tool":"read_file","path":"../outside"}]).to_string(),json!([{"tool":"read_file"}]).to_string(),serde_json::to_string(&(0..9).map(|_|first.clone()).collect::<Vec<_>>()).unwrap()]{assert!(parse_model_output(&raw).is_err());}let mut used=0;reserve_read_tools(&mut used,4).unwrap();reserve_read_tools(&mut used,8).unwrap();assert!(reserve_read_tools(&mut used,1).is_err());assert_eq!(used,12);}
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
 #[test]fn rejects_unsafe_empty_and_malformed_edits(){for edits in [json!([edit("main.tex"," ","replacement")]),json!([{"filePath":"main.tex","before":"original"}]),json!("not an array")]{let mut value=json!({"reply":"preview","replacement":null,"edits":edits});assert!(validate_edit_output(&mut value,"main.tex",true).is_err());}let mut value=json!({"reply":"preview","replacement":null,"edits":[edit("../private","original","replacement")]});assert!(validate_edit_output(&mut value,"../private",true).is_err());}
 #[test]fn enforces_edit_count_and_character_budgets(){let mut value=json!({"reply":"preview","replacement":null,"edits":(0..49).map(|n|edit("main.tex",&format!("original {n}"),"replacement")).collect::<Vec<_>>()});assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!([edit("main.tex",&"界".repeat(16001),"replacement")]);assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!([edit("main.tex","original",&"界".repeat(24001))]);assert!(validate_edit_output(&mut value,"main.tex",true).is_err());value["edits"]=json!((0..3).map(|_|edit("main.tex",&"b".repeat(15000),&"a".repeat(15000))).collect::<Vec<_>>());assert!(validate_edit_output(&mut value,"main.tex",true).is_err());}
 #[test]fn skill_install_uses_both_tool_protocols_but_cannot_grant_permissions(){let content="---\nname: proposed-skill\ndescription: Workflow reference.\n---\nPreserve citations.";let value=json!({"tool":"install_skill","content":content});assert_eq!(parse_model_output(&value.to_string()).unwrap()["tool"],"install_skill");let native=NativeToolCall{id:"skill-call".into(),name:"install_skill".into(),arguments:json!({"content":content}).to_string()};assert_eq!(native_calls_output(&[native]).unwrap()["calls"][0]["tool"],"install_skill");for bad in [json!({"tool":"install_skill","content":content,"enabled":true}),json!({"tool":"install_skill","content":content,"userId":"other"}),json!({"tool":"install_skill","command":"install.sh"})]{assert!(parse_model_output(&bad.to_string()).is_err());}}

}

#[cfg(test)]mod compile_settings_tests{
 use super::*;
 #[test]fn settings_stay_proposals_and_guards_come_from_the_server(){
  let mut answer=json!({"reply":"可改用XeLaTeX，等待接受","actions":[{"type":"set_compile_settings","compiler":"xelatex","mainFile":"main.tex"}]});validate_file_action_proposals(&answer).unwrap();
  let options=std::collections::HashMap::from([("projectCompiler".to_string(),"tectonic".to_string()),("projectMainFile".to_string(),"old.tex".to_string())]);stamp_compile_settings(&mut answer,Some(&options));assert_eq!(answer["actions"][0]["expected"],json!({"compiler":"tectonic","mainFile":"old.tex"}));
  assert!(parse_model_output(r#"{"tool":"set_compile_settings","compiler":"xelatex","mainFile":"main.tex"}"#).is_err());
  assert!(validate_file_action_proposals(&json!({"actions":[{"type":"set_compile_settings","compiler":"shell","mainFile":"main.tex"}]})).is_err());
 }
}

#[cfg(test)]mod history_tool_tests{
 use super::*;
 #[test]fn history_calls_share_native_and_compatibility_validation(){
  let id=Uuid::new_v4();
  for call in [json!({"tool":"list_versions"}),json!({"tool":"read_version_file","versionId":id,"path":"main.tex","startLine":1,"endLine":20}),json!({"tool":"compare_versions","beforeVersionId":id,"afterVersionId":id})]{
   assert_eq!(parse_model_output(&call.to_string()).unwrap(),call);
   assert!(parse_read_tool_sequence(&json!([call]).to_string()).unwrap().is_some());
  }
  let native=NativeToolCall{id:"history-1".into(),name:"list_versions".into(),arguments:String::new()};assert!(native_calls_output(&[native]).is_ok());
  for call in [json!({"tool":"list_versions","projectId":id}),json!({"tool":"read_version_file","versionId":id,"path":"../private"}),json!({"tool":"compare_versions","beforeVersionId":id,"afterVersionId":id,"apply":true})]{
   assert!(parse_model_output(&call.to_string()).is_err());assert!(parse_read_tool_sequence(&json!([call]).to_string()).is_err());
  }
 }
 #[test]fn history_reads_count_against_existing_budget_and_deduplicate(){
  let calls=vec![json!({"tool":"list_versions"}),json!({"tool":"list_versions"}),json!({"tool":"read_version_file","versionId":Uuid::new_v4(),"path":"main.tex"})];let mut completed=std::collections::HashMap::new();assert_eq!(count_new_read_calls(&calls,&completed),2);completed.insert(calls[0].to_string(),json!({"versions":[]}));assert_eq!(count_new_read_calls(&calls,&completed),1);
 }
}

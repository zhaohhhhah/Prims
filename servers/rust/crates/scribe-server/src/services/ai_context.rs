//! Private per-user/project writing background and persistent conversation.
use chrono::{DateTime,Utc};
use scribe_shared::{ApiError,ApiResult,ChatMessage,ChatRole,ErrorCode,ProjectId,UserId};
use serde::{Deserialize,Serialize};
use serde_json::json;
use sqlx::{PgPool,Row};
use uuid::Uuid;
use super::membership::assert_member;

pub const MAX_BRIEF:usize=8000;
pub const MAX_SUMMARY:usize=2000;
pub const MAX_CONTENT:usize=24000;
pub const MAX_RAW:usize=80000;
pub const MAX_MODEL_HISTORY:usize=40;
pub const MAX_MODEL_HISTORY_CHARS:usize=50000;

#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ContextMessage {
 pub id:Uuid,pub role:String,pub content:String,
 #[serde(skip_serializing_if="Option::is_none")]pub raw:Option<String>,
 #[serde(skip_serializing_if="Option::is_none")]pub file_path:Option<String>,
 #[serde(skip_serializing_if="Option::is_none")]pub selection:Option<String>,
 #[serde(skip_serializing_if="Option::is_none")]pub selection_id:Option<String>,
 #[serde(skip_serializing_if="Option::is_none")]pub created_at:Option<DateTime<Utc>>,
}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ContextState {
 pub brief:String,pub thread_id:Uuid,pub messages:Vec<ContextMessage>,pub has_earlier:bool,
 pub project_title:String,pub project_description:Option<String>,pub updated_at:DateTime<Utc>,
 pub auto_summary:String,pub summary_updated_at:Option<DateTime<Utc>>,
 #[serde(skip)]pub summary_revision:i64,
}
#[derive(Debug,Serialize)]#[serde(rename_all="camelCase")]pub struct ContextThread{pub id:Uuid,pub title:String,pub updated_at:DateTime<Utc>,pub created_at:DateTime<Utc>,pub last_message_at:Option<DateTime<Utc>>,pub message_count:i64}
#[derive(Debug,Serialize)]#[serde(rename_all="camelCase")]pub struct ThreadList{pub threads:Vec<ContextThread>,pub has_earlier:bool}
#[derive(Deserialize)]#[serde(rename_all="camelCase")]pub struct SelectThreadInput{pub thread_id:Uuid}
#[derive(Deserialize)]pub struct BriefInput{pub brief:String}
#[derive(Deserialize)]#[serde(rename_all="camelCase")]pub struct AppendInput{pub thread_id:Uuid,pub messages:Vec<ContextMessage>}
#[derive(Clone)]pub struct AIContextService{pool:PgPool}
impl AIContextService {
 pub fn new(pool:PgPool)->Self{Self{pool}}
 async fn ensure(&self,user:UserId,project:ProjectId)->ApiResult<()> {
  assert_member(&self.pool,user,project).await?;
  sqlx::query("insert into public.scribe_ai_contexts(user_id,project_id) values($1,$2) on conflict(user_id,project_id) do nothing").bind(user.into_inner()).bind(project.into_inner()).execute(&self.pool).await.map_err(db)?;
  sqlx::query("insert into public.scribe_ai_context_threads(user_id,project_id,id) select user_id,project_id,thread_id from public.scribe_ai_contexts where user_id=$1 and project_id=$2 on conflict(user_id,project_id,id) do nothing").bind(user.into_inner()).bind(project.into_inner()).execute(&self.pool).await.map_err(db)?;Ok(())
 }
 pub async fn get(&self,user:UserId,project:ProjectId)->ApiResult<ContextState>{
  self.ensure(user,project).await?;
  let row=sqlx::query("select c.brief,c.thread_id,c.updated_at,p.name,p.description,t.auto_summary,t.summary_updated_at,t.summary_revision from public.scribe_ai_contexts c join public.projects p on p.id=c.project_id join public.scribe_ai_context_threads t on t.user_id=c.user_id and t.project_id=c.project_id and t.id=c.thread_id where c.user_id=$1 and c.project_id=$2").bind(user.into_inner()).bind(project.into_inner()).fetch_one(&self.pool).await.map_err(db)?;
  let thread:Uuid=row.get("thread_id");
  let rows=sqlx::query("select id,role,content,raw,file_path,selection,selection_id,created_at from public.scribe_ai_context_messages where user_id=$1 and project_id=$2 and thread_id=$3 order by seq desc limit 41").bind(user.into_inner()).bind(project.into_inner()).bind(thread).fetch_all(&self.pool).await.map_err(db)?;
  let earlier=rows.len()>40;
  let mut messages=rows.into_iter().take(40).map(|r|ContextMessage{id:r.get("id"),role:r.get("role"),content:r.get("content"),raw:r.get("raw"),file_path:r.get("file_path"),selection:r.get("selection"),selection_id:r.get("selection_id"),created_at:Some(r.get("created_at"))}).collect::<Vec<_>>();messages.reverse();
  Ok(ContextState{brief:row.get("brief"),thread_id:thread,messages,has_earlier:earlier,project_title:row.get("name"),project_description:row.get("description"),updated_at:row.get("updated_at"),auto_summary:row.get("auto_summary"),summary_updated_at:row.get("summary_updated_at"),summary_revision:row.get("summary_revision")})
 }
 pub async fn update_brief(&self,user:UserId,project:ProjectId,input:BriefInput)->ApiResult<ContextState>{
  if input.brief.chars().count()>MAX_BRIEF{return Err(ApiError::validation("写作背景最多 8000 字符"));}
  self.ensure(user,project).await?;
  sqlx::query("update public.scribe_ai_contexts set brief=$3,updated_at=now() where user_id=$1 and project_id=$2").bind(user.into_inner()).bind(project.into_inner()).bind(input.brief.trim()).execute(&self.pool).await.map_err(db)?;self.get(user,project).await
 }
 pub async fn new_thread(&self,user:UserId,project:ProjectId)->ApiResult<ContextState>{
  self.ensure(user,project).await?;let mut tx=self.pool.begin().await.map_err(db)?;
  sqlx::query("select thread_id from public.scribe_ai_contexts where user_id=$1 and project_id=$2 for update").bind(user.into_inner()).bind(project.into_inner()).execute(&mut *tx).await.map_err(db)?;
  let thread=Uuid::new_v4();
  sqlx::query("insert into public.scribe_ai_context_threads(user_id,project_id,id) values($1,$2,$3)").bind(user.into_inner()).bind(project.into_inner()).bind(thread).execute(&mut *tx).await.map_err(db)?;
  sqlx::query("update public.scribe_ai_contexts set thread_id=$3,updated_at=now() where user_id=$1 and project_id=$2").bind(user.into_inner()).bind(project.into_inner()).bind(thread).execute(&mut *tx).await.map_err(db)?;
  tx.commit().await.map_err(db)?;self.get(user,project).await
 }
 pub async fn threads(&self,user:UserId,project:ProjectId)->ApiResult<ThreadList>{
  self.ensure(user,project).await?;
  let rows=sqlx::query("select t.id,t.title,t.created_at,max(m.created_at) filter(where m.role in ('user','assistant')) as last_message_at,coalesce(max(m.created_at) filter(where m.role in ('user','assistant')),t.created_at) as updated_at,count(m.seq) as message_count from public.scribe_ai_context_threads t left join public.scribe_ai_context_messages m on m.user_id=t.user_id and m.project_id=t.project_id and m.thread_id=t.id where t.user_id=$1 and t.project_id=$2 group by t.user_id,t.project_id,t.id,t.title,t.created_at order by coalesce(max(m.created_at) filter(where m.role in ('user','assistant')),t.created_at) desc,t.id limit 51").bind(user.into_inner()).bind(project.into_inner()).fetch_all(&self.pool).await.map_err(db)?;
  let has_earlier=rows.len()>50;let threads=rows.into_iter().take(50).map(|r|ContextThread{id:r.get("id"),title:r.get("title"),updated_at:r.get("updated_at"),created_at:r.get("created_at"),last_message_at:r.get("last_message_at"),message_count:r.get("message_count")}).collect();Ok(ThreadList{threads,has_earlier})
 }
 pub async fn select_thread(&self,user:UserId,project:ProjectId,input:SelectThreadInput)->ApiResult<ContextState>{
  self.ensure(user,project).await?;let mut tx=self.pool.begin().await.map_err(db)?;
  sqlx::query("select thread_id from public.scribe_ai_contexts where user_id=$1 and project_id=$2 for update").bind(user.into_inner()).bind(project.into_inner()).execute(&mut *tx).await.map_err(db)?;
  let exists:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_ai_context_threads where user_id=$1 and project_id=$2 and id=$3)").bind(user.into_inner()).bind(project.into_inner()).bind(input.thread_id).fetch_one(&mut *tx).await.map_err(db)?;
  if !exists{return Err(ApiError::not_found("找不到此项目的对话"));}
  sqlx::query("update public.scribe_ai_contexts set thread_id=$3,updated_at=now() where user_id=$1 and project_id=$2").bind(user.into_inner()).bind(project.into_inner()).bind(input.thread_id).execute(&mut *tx).await.map_err(db)?;
  tx.commit().await.map_err(db)?;self.get(user,project).await
 }
 /// Update the summary produced by a completed ordinary response without a second model request.
 /// Context timestamps detect switch-away/back and brief changes; thread revision detects concurrent
 /// summaries and every appended turn. Both are required so delayed results never replace newer state.
 pub async fn save_generated_summary(&self,user:UserId,project:ProjectId,source:&ContextState,summary:&str)->ApiResult<bool>{
  let Some(summary)=normalize_summary(summary)else{return Ok(false)};
  assert_member(&self.pool,user,project).await?;
  let mut tx=self.pool.begin().await.map_err(db)?;
  let row=sqlx::query("select thread_id,updated_at from public.scribe_ai_contexts where user_id=$1 and project_id=$2 for update").bind(user.into_inner()).bind(project.into_inner()).fetch_optional(&mut *tx).await.map_err(db)?;
  let Some(row)=row else{return Ok(false)};
  if row.get::<Uuid,_>("thread_id")!=source.thread_id||row.get::<DateTime<Utc>,_>("updated_at")!=source.updated_at{return Ok(false);}
  let count=sqlx::query("update public.scribe_ai_context_threads set auto_summary=$4,summary_updated_at=now(),summary_revision=summary_revision+1,updated_at=now() where user_id=$1 and project_id=$2 and id=$3 and summary_revision=$5").bind(user.into_inner()).bind(project.into_inner()).bind(source.thread_id).bind(summary).bind(source.summary_revision).execute(&mut *tx).await.map_err(db)?.rows_affected();
  tx.commit().await.map_err(db)?;Ok(count==1)
 }
 pub async fn append(&self,user:UserId,project:ProjectId,input:AppendInput)->ApiResult<ContextState>{
  validate_messages(&input.messages)?;self.ensure(user,project).await?;
  let mut tx=self.pool.begin().await.map_err(db)?;
  let thread:Uuid=sqlx::query_scalar("select thread_id from public.scribe_ai_contexts where user_id=$1 and project_id=$2 for update").bind(user.into_inner()).bind(project.into_inner()).fetch_one(&mut *tx).await.map_err(db)?;
  if thread!=input.thread_id{return Err(ApiError::new(ErrorCode::Conflict,"此项目已开始新对话，请刷新上下文后重试；当前修改方案未覆盖正文"));}
  for m in input.messages {
   // The idempotency key is scoped to the user; another account's UUID cannot collide/leak.
   let old=sqlx::query("select project_id,thread_id from public.scribe_ai_context_messages where user_id=$1 and id=$2").bind(user.into_inner()).bind(m.id).fetch_optional(&mut *tx).await.map_err(db)?;
   if let Some(old)=old {
    if old.get::<Uuid,_>("project_id")!=project.into_inner()||old.get::<Uuid,_>("thread_id")!=thread{return Err(ApiError::new(ErrorCode::Conflict,"消息标识已用于另一段对话，请刷新后重试"));}
    continue;
   }
   sqlx::query("insert into public.scribe_ai_context_messages(user_id,project_id,thread_id,id,role,content,raw,file_path,selection,selection_id) values($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)").bind(user.into_inner()).bind(project.into_inner()).bind(thread).bind(m.id).bind(m.role).bind(m.content).bind(m.raw).bind(m.file_path).bind(m.selection).bind(m.selection_id).execute(&mut *tx).await.map_err(db)?;
  }
  sqlx::query("update public.scribe_ai_contexts set updated_at=now() where user_id=$1 and project_id=$2").bind(user.into_inner()).bind(project.into_inner()).execute(&mut *tx).await.map_err(db)?;
  sqlx::query("update public.scribe_ai_context_threads t set updated_at=now(),summary_revision=summary_revision+1,title=coalesce((select nullif(left(btrim(regexp_replace(m.content,'[[:space:]]+',' ','g')),80),'') from public.scribe_ai_context_messages m where m.user_id=t.user_id and m.project_id=t.project_id and m.thread_id=t.id and m.role='user' order by m.seq limit 1),t.title) where t.user_id=$1 and t.project_id=$2 and t.id=$3").bind(user.into_inner()).bind(project.into_inner()).bind(thread).execute(&mut *tx).await.map_err(db)?;
  tx.commit().await.map_err(db)?;self.get(user,project).await
 }
}
pub fn normalize_summary(summary:&str)->Option<String>{let s=summary.trim();if s.is_empty(){None}else{Some(s.chars().take(MAX_SUMMARY).collect())}}
/// Internal model metadata is never included in the public reply/replacement JSON.
pub fn take_generated_summary(output:&mut serde_json::Value)->Option<String>{output.as_object_mut()?.remove("contextSummary").and_then(|v|v.as_str().and_then(normalize_summary))}
fn validate_messages(messages:&[ContextMessage])->ApiResult<()> {
 if messages.is_empty()||messages.len()>4{return Err(ApiError::validation("每次保存 1–4 条消息"));}
 let mut ids=std::collections::HashSet::new();
 for m in messages {
  if !ids.insert(m.id){return Err(ApiError::validation("消息标识不能重复"));}
  if !matches!(m.role.as_str(),"user"|"assistant"|"action"){return Err(ApiError::validation("不支持的消息类型"));}
  for(value,limit)in [(Some(m.content.as_str()),MAX_CONTENT),(m.raw.as_deref(),MAX_RAW),(m.selection.as_deref(),16000)]{if value.is_some_and(|v|v.chars().count()>limit){return Err(ApiError::validation("对话内容过长，当前页面仍保留方案；请缩短选区后保存"));}}
  for(value,limit)in [(m.file_path.as_deref(),512),(m.selection_id.as_deref(),128)]{if value.is_some_and(|v|v.chars().count()>limit||v.chars().any(char::is_control)){return Err(ApiError::validation("对话位置格式不正确"));}}
 }Ok(())
}

/// Model history is bounded independently from stored history; other-target drafts become reference-only.
pub fn model_history(state:&ContextState,file:&str,selection_id:&str)->(Vec<ChatMessage>,bool){
 let mut kept=Vec::new();let mut remaining=MAX_MODEL_HISTORY_CHARS;let mut truncated=state.has_earlier;let mut decided=std::collections::HashSet::new();
 for m in state.messages.iter().rev().take(MAX_MODEL_HISTORY) {
  let key=(m.file_path.clone().unwrap_or_default(),m.selection_id.clone().unwrap_or_default());
  if m.role=="action"{decided.insert(key.clone());}
  let same_target=!selection_id.is_empty()&&m.selection_id.as_deref()==Some(selection_id)&&m.file_path.as_deref()==Some(file)&&!decided.contains(&key);
  let mut content=match m.role.as_str(){
   "user"=>json!({"pastInstruction":m.content,"filePath":m.file_path,"pastSelection":m.selection.as_deref().unwrap_or("").chars().take(6000).collect::<String>(),"selectionId":m.selection_id,"referenceOnly":!same_target}).to_string(),
   "assistant" if same_target=>m.raw.clone().unwrap_or_else(||m.content.clone()),
   "assistant"=>json!({"pastReply":m.content,"filePath":m.file_path,"selectionId":m.selection_id,"referenceOnly":true,"note":"This is another target or a draft already accepted/rejected/undone. Its replacement is not the current document or active draft."}).to_string(),
   "action"=>json!({"pastUserAction":m.content,"filePath":m.file_path,"selectionId":m.selection_id,"referenceOnly":true,"note":"Historical user decision only; an accept/reject/undo may affect just one edit, not an entire batch. CurrentSelection and live document are the authoritative current text."}).to_string(),_=>continue,
  };
  let n=content.chars().count();
  if n>remaining||n>24000 {
   truncated=true;if remaining<500{continue;}
   let excerpt=m.content.chars().take(remaining.saturating_sub(400).min(4000)).collect::<String>();
   content=json!({"pastRole":m.role,"pastReplyOrInstruction":excerpt,"filePath":m.file_path,"selectionId":m.selection_id,"referenceOnly":true,"truncated":true,"note":"Large historical draft omitted; do not treat it as current document text."}).to_string();
  }
  let n=content.chars().count();if n>remaining{truncated=true;continue;}
  remaining-=n;kept.push(ChatMessage{role:if m.role=="assistant"{ChatRole::Assistant}else{ChatRole::User},content});
 }
 kept.reverse();(kept,truncated)
}
fn db(error:sqlx::Error)->ApiError{tracing::error!(?error,"private AI context database error");ApiError::internal("暂时无法读取或保存写作上下文")}

#[cfg(test)]mod tests{
 use super::*;
 fn msg(role:&str)->ContextMessage{ContextMessage{id:Uuid::new_v4(),role:role.into(),content:"prior idea".into(),raw:Some("{\"reply\":\"ok\",\"replacement\":\"OLD DRAFT\"}".into()),file_path:Some("a.tex".into()),selection:Some("selected".into()),selection_id:Some("old".into()),created_at:None}}
 fn state(messages:Vec<ContextMessage>)->ContextState{ContextState{brief:"purpose".into(),thread_id:Uuid::new_v4(),messages,has_earlier:false,project_title:"demo".into(),project_description:None,updated_at:Utc::now(),auto_summary:String::new(),summary_updated_at:None,summary_revision:0}}
 #[test]fn internal_summary_is_private_and_bounded(){let mut output=json!({"reply":"visible","replacement":null,"contextSummary":" Goal: confirmed constraints; proposal pending "});assert_eq!(take_generated_summary(&mut output).as_deref(),Some("Goal: confirmed constraints; proposal pending"));assert!(output.get("contextSummary").is_none());assert_eq!(output["reply"],"visible");assert_eq!(normalize_summary(&"界".repeat(3000)).unwrap().chars().count(),MAX_SUMMARY);}
 #[test]fn absent_empty_or_invalid_summary_never_overwrites(){for mut output in [json!({"reply":"ok","replacement":null}),json!({"contextSummary":"  "}),json!({"contextSummary":{"bad":true}})]{assert!(take_generated_summary(&mut output).is_none());assert!(output.get("contextSummary").is_none());}}
 #[test]fn rejects_system_role_and_oversized_messages(){assert!(validate_messages(&[msg("system")]).is_err());let mut m=msg("user");m.content="a".repeat(MAX_CONTENT+1);assert!(validate_messages(&[m]).is_err());}
 #[test]fn cross_selection_history_does_not_reuse_old_replacement(){let s=state(vec![msg("assistant")]);let(h,_)=model_history(&s,"b.tex","new");assert!(!h[0].content.contains("OLD DRAFT"));assert!(h[0].content.contains("referenceOnly"));let(h,_)=model_history(&s,"a.tex","old");assert!(h[0].content.contains("OLD DRAFT"));}
 #[test]fn historical_multi_edits_are_not_reused_across_targets_or_after_single_edit_decision(){let mut proposal=msg("assistant");proposal.raw=Some(json!({"reply":"two pending edits","replacement":null,"edits":[{"filePath":"a.tex","before":"old source","after":"OLD EDIT OUTSIDE SELECTION"}]}).to_string());let s=state(vec![proposal.clone()]);let(h,_)=model_history(&s,"a.tex","old");assert!(h[0].content.contains("OLD EDIT OUTSIDE SELECTION"));let(h,_)=model_history(&s,"a.tex","different");assert!(!h[0].content.contains("OLD EDIT OUTSIDE SELECTION"));let mut action=msg("action");action.content="Accepted edit 1 only; other edits pending".into();let s=state(vec![proposal,action]);let(h,_)=model_history(&s,"a.tex","old");assert!(!h[0].content.contains("OLD EDIT OUTSIDE SELECTION"));assert!(h[1].content.contains("not an entire batch"));}
 #[test]fn decided_proposal_is_not_reused_as_current_draft(){let mut action=msg("action");action.content="已撤销 AI 修改".into();let s=state(vec![msg("assistant"),action]);let(h,_)=model_history(&s,"a.tex","old");assert!(!h[0].content.contains("OLD DRAFT"));assert!(h[1].content.contains("CurrentSelection"));}
 #[test]fn oversized_latest_raw_falls_back_to_reply_instead_of_dropping_prior_history(){let mut latest=msg("assistant");latest.raw=Some("x".repeat(60000));let s=state(vec![msg("user"),latest]);let(h,truncated)=model_history(&s,"a.tex","old");assert!(truncated);assert_eq!(h.len(),2);assert!(h[1].content.contains("truncated"));}
 #[test]fn history_budget_is_bounded_and_current_brief_is_separate(){let s=state((0..40).map(|_|{let mut m=msg("assistant");m.content="界".repeat(12000);m}).collect());let(h,truncated)=model_history(&s,"b.tex","new");assert!(truncated);assert!(h.iter().map(|m|m.content.chars().count()).sum::<usize>()<=MAX_MODEL_HISTORY_CHARS);assert!(h.len()<=40);}
}

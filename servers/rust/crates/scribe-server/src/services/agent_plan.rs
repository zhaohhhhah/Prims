//! Codex `update_plan` argument shape, with Prims public-output bounds.
//! Source: openai/codex@6fe77fc12346591b8037a8704640e40196b19044,
//! core/src/tools/handlers/plan_spec.rs and protocol/src/plan_tool.rs (Apache-2.0).
//! Original copyright/license/notice are included under vendor/codex_compaction/.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]#[serde(rename_all="snake_case")]
pub enum PlanStepStatus{Pending,InProgress,Completed}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]#[serde(deny_unknown_fields)]
pub struct PlanStep{pub step:String,pub status:PlanStepStatus}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]#[serde(deny_unknown_fields)]
pub struct UpdatePlanArgs{#[serde(default,skip_serializing_if="Option::is_none")]pub explanation:Option<String>,pub plan:Vec<PlanStep>}
#[derive(Default)]pub struct AgentPlan{current:Option<UpdatePlanArgs>}
fn public_text(value:&str,limit:usize)->Result<String,&'static str>{
 let text=value.trim();let lower=text.to_ascii_lowercase();if text.is_empty()||text.chars().count()>limit||text.chars().any(|c|c.is_control()||matches!(c,'<'|'>'|'\u{2028}'|'\u{2029}'|'\u{feff}'|'\u{200b}'..='\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2060}'..='\u{206f}'))||["```","[analysis]","[reasoning]","chain-of-thought","scratchpad:","bearer ","api_key=","api_key:","api-key=","password=","client_secret="].iter().any(|v|lower.contains(v)){return Err("计划只允许公开的简短动作说明，不包含推理过程、密钥或控制字符");}
 if text.split(|c:char|!c.is_ascii_alphanumeric()&&c!='_'&&c!='-').any(|part|part.len()>=48||(part.to_ascii_lowercase().starts_with("sk-")&&part.len()>=8)){return Err("计划包含疑似凭据或长标识，请改成简短公开动作");}Ok(text.into())
}
impl AgentPlan{
 pub fn update(&mut self,value:&Value)->Result<Value,&'static str>{
  let mut value=value.clone();if let Some(object)=value.as_object_mut(){if let Some(tool)=object.remove("tool"){if tool!="update_plan"{return Err("计划工具名称无效");}}}
  let mut args:UpdatePlanArgs=serde_json::from_value(value).map_err(|_|"计划格式需要 explanation 可选及 plan 列表，状态仅 pending/in_progress/completed")?;
  if args.plan.is_empty(){return Err("计划不能为空");}if args.plan.iter().filter(|s|s.status==PlanStepStatus::InProgress).count()>1{return Err("计划最多只能有一个正在进行的步骤");}
  for step in &mut args.plan{step.step=public_text(&step.step,120)?;}args.explanation=args.explanation.as_deref().map(|s|public_text(s,240)).transpose()?;
  let changed=self.current.as_ref()!=Some(&args);self.current=Some(args);Ok(json!({"ok":true,"changed":changed,"message":"Plan updated","note":"Plan completion describes the agent step only. Submitted proposals remain pending_review until the user accepts them; no document was changed by update_plan."}))
 }
 pub fn metadata(&self)->Value{self.current.as_ref().map(|args|json!({"explanation":args.explanation,"plan":args.plan,"completionMeans":"agent step complete; submitted proposals are still pending_review, not applied"})).unwrap_or(Value::Null)}
 pub fn current_status(&self)->Option<String>{let plan=&self.current.as_ref()?.plan;if let Some(step)=plan.iter().find(|s|s.status==PlanStepStatus::InProgress){return Some(step.step.clone());}if plan.iter().all(|s|s.status==PlanStepStatus::Completed){Some("计划步骤已完成，修改仍需逐项确认".into())}else{Some("计划已更新，等待继续处理下一步".into())}}
}
pub fn tool_schema()->Value{json!({"type":"object","properties":{"explanation":{"type":"string","maxLength":240},"plan":{"type":"array","minItems":1,"items":{"type":"object","properties":{"step":{"type":"string","maxLength":120},"status":{"type":"string","enum":["pending","in_progress","completed"]}},"required":["step","status"],"additionalProperties":false}}},"required":["plan"],"additionalProperties":false})}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn codex_shape_updates_public_plan_and_current_action(){let mut plan=AgentPlan::default();assert!(plan.metadata().is_null());let result=plan.update(&json!({"tool":"update_plan","explanation":"先检查，再提交建议","plan":[{"step":"读取目标段落","status":"in_progress"},{"step":"提交小段待审建议","status":"pending"}]})).unwrap();assert_eq!(result["changed"],true);assert_eq!(plan.current_status().as_deref(),Some("读取目标段落"));assert_eq!(plan.metadata()["plan"][1]["status"],"pending");}
 #[test]fn validation_is_atomic_and_rejects_parallel_active_or_hidden_sensitive_fields(){let mut p=AgentPlan::default();let good=json!({"plan":[{"step":"读取正文","status":"in_progress"}]});p.update(&good).unwrap();let before=p.metadata();for bad in [json!({"plan":[{"step":"a","status":"in_progress"},{"step":"b","status":"in_progress"}]}),json!({"plan":[]}),json!({"plan":[{"step":"<think>private</think>","status":"pending"}]}),json!({"plan":[{"step":"API_KEY=secret","status":"pending"}]}),json!({"plan":[{"step":"a","status":"applied"}]}),json!({"plan":[{"step":"a","status":"pending","reasoning":"hidden"}]})]{assert!(p.update(&bad).is_err());assert_eq!(p.metadata(),before);}}
 #[test]fn completion_never_claims_document_was_applied_and_identical_updates_are_not_progress(){let mut p=AgentPlan::default();let value=json!({"plan":[{"step":"提交一段待审建议","status":"completed"}]});p.update(&value).unwrap();assert_eq!(p.update(&value).unwrap()["changed"],false);assert!(p.current_status().unwrap().contains("仍需逐项确认"));assert!(p.metadata()["completionMeans"].as_str().unwrap().contains("not applied"));}
 #[test]fn unicode_bounds_and_schema_are_explicit(){let mut p=AgentPlan::default();assert!(p.update(&json!({"plan":[{"step":"界".repeat(121),"status":"pending"}]})).is_err());assert!(p.update(&json!({"plan":(0..9).map(|_|json!({"step":"step","status":"pending"})).collect::<Vec<_>>()})).is_ok());assert_eq!(tool_schema()["plan"],Value::Null);assert_eq!(tool_schema()["properties"]["plan"]["maxItems"],Value::Null);}
 #[test]fn ordinary_task_filenames_are_not_mistaken_for_keys(){let mut p=AgentPlan::default();assert!(p.update(&json!({"plan":[{"step":"检查 task-list.tex","status":"in_progress"}]})).is_ok());assert!(p.update(&json!({"plan":[{"step":"读取 sk-fake-secret-key","status":"pending"}]})).is_err());}

}

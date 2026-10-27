//! Optional public action descriptions. Never use these strings to select or execute a tool.
use serde_json::Value;
pub const MAX_PUBLIC_STATUS_CHARS:usize=120;

/// Validate a single user-facing action description, not hidden reasoning or arbitrary output.
fn clean(value:&Value)->Option<String>{
 let raw=value.as_str()?;
 if raw.chars().count()>MAX_PUBLIC_STATUS_CHARS||raw.chars().any(|ch|ch.is_control()||matches!(ch,'\u{2028}'|'\u{2029}'|'\u{feff}'|'\u{200b}'..='\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2060}'..='\u{206f}')){return None;}
 let text=raw.trim();if text.is_empty(){return None;}
 // Progress is plain text. Reject XML/role tags and fenced reasoning instead of rendering them.
 let lowered=text.to_ascii_lowercase();
 if text.contains('<')||text.contains('>')||text.contains("```")||["[analysis]","[/analysis]","[reasoning]","[/reasoning]","[think]","[/think]","chain-of-thought","scratchpad:"].iter().any(|tag|lowered.contains(tag)){return None;}
 if serde_json::from_str::<Value>(text).is_ok_and(|v|v.is_object()||v.is_array()){return None;}
 Some(text.to_string())
}
/// Extract only validated metadata; publicStatus takes precedence over the older status name.
pub fn public_status(value:&Value)->Option<String>{value.get("publicStatus").and_then(clean).or_else(||value.get("status").and_then(clean))}
/// Strip both optional fields before strict command validation and before saving reply/raw history.
/// The caller keeps this return value alongside a validated tool/final object, then emits SSE `status`.
pub fn take_public_status(value:&mut Value)->Option<String>{
 let object=value.as_object_mut()?;let public=object.remove("publicStatus");let legacy=object.remove("status");
 public.as_ref().and_then(clean).or_else(||legacy.as_ref().and_then(clean))
}
#[cfg(test)]mod tests{
 use super::*;use serde_json::json;
 #[test]fn accepts_short_plain_actions_and_prefers_public_status(){assert_eq!(public_status(&json!({"publicStatus":"  正在核对引用与上下文  ","status":"fallback"})).as_deref(),Some("正在核对引用与上下文"));assert_eq!(public_status(&json!({"status":"Checking the selected paragraph"})).as_deref(),Some("Checking the selected paragraph"));assert_eq!(public_status(&json!({"publicStatus":"界".repeat(120)})).unwrap().chars().count(),120);}
 #[test]fn rejects_controls_hidden_reasoning_objects_and_long_output(){
  for status in [json!(""),json!("   "),json!("正在读取\nmain.tex"),json!("\r开始"),json!("a\tb"),json!("\u{202e}spoof"),json!("line\u{2028}two"),json!("<ThInK>private reasoning</ThInK>"),json!("[analysis]hidden"),json!("```analysis\nprivate```"),json!({"tool":"write"}),json!(["a"]),json!("{\"tool\":\"write\"}"),json!("[\"hidden\"]"),json!("x".repeat(121))]{assert!(public_status(&json!({"publicStatus":status})).is_none());}
 }
 #[test]fn stripping_metadata_preserves_the_actual_validated_tool_payload(){let mut value=json!({"tool":"read_file","path":"main.tex","publicStatus":"正在核对当前文件","status":"older"});assert!(take_public_status(&mut value).is_some());assert_eq!(value,json!({"tool":"read_file","path":"main.tex"}));}
 #[test]fn a_status_is_never_a_tool_and_never_remains_in_final_history(){let mut value=json!({"reply":"answer","replacement":null,"publicStatus":"准备删除所有内容"});assert_eq!(take_public_status(&mut value).as_deref(),Some("准备删除所有内容"));assert!(value.get("tool").is_none());assert_eq!(value,json!({"reply":"answer","replacement":null}));let mut invalid=json!({"reply":"safe","status":{"reasoning":"hidden"}});assert!(take_public_status(&mut invalid).is_none());assert!(invalid.get("status").is_none());}
}

//! Bounded no-progress guard for consecutive identical normalized tool batches.
//! This is a Prims harness guard; it does not execute tools, discard proposals or alter files.
use serde_json::Value;
pub const STOP_REPLY:&str="这一步连续重复且没有新结果，本轮先停在这里。已提交的待审建议会保留，你可以逐项确认后继续，或缩小修改范围。";
fn canonical(value:&Value)->String{match value{Value::Object(map)=>{let mut keys=map.keys().collect::<Vec<_>>();keys.sort();format!("{{{}}}",keys.into_iter().map(|key|format!("{}:{}",serde_json::to_string(key).unwrap_or_default(),canonical(&map[key]))).collect::<Vec<_>>().join(","))},Value::Array(values)=>format!("[{}]",values.iter().map(canonical).collect::<Vec<_>>().join(",")),_=>value.to_string()}}
#[derive(Default)]pub struct LoopGuard{previous:Option<String>,stalls:u8}
impl LoopGuard{
 /// `new_results` counts genuinely new data, not replayed cached results; errors alone are not progress.
 /// `new_proposals` counts newly assigned proposal IDs, not idempotent replays.
 /// Third consecutive same batch without progress stops. Independent windows/hunks reset the guard.
 pub fn observe(&mut self,batch:&[Value],new_results:usize,new_proposals:usize)->bool{
  if batch.is_empty(){self.previous=None;self.stalls=0;return false;}
  let mut normalized=batch.iter().map(canonical).collect::<Vec<_>>();normalized.sort();let key=normalized.join("\n");
  if new_results>0||new_proposals>0{self.stalls=0;}else if self.previous.as_deref()==Some(key.as_str()){self.stalls=self.stalls.saturating_add(1);}else{self.stalls=0;}
  self.previous=Some(key);self.stalls>=2
 }
}
#[cfg(test)]mod tests{
 use super::*;use serde_json::json;
 #[test]fn repeated_cached_batch_stops_on_third_no_progress_observation(){let mut g=LoopGuard::default();let batch=[json!({"tool":"read_file","path":"a.tex"})];assert!(!g.observe(&batch,0,0));assert!(!g.observe(&batch,0,0));assert!(g.observe(&batch,0,0));assert!(g.observe(&batch,0,0));}
 #[test]fn new_results_proposals_and_independent_read_windows_are_not_blocked(){let mut g=LoopGuard::default();for line in 0..20{assert!(!g.observe(&[json!({"tool":"read_file","path":"a.tex","startLine":line})],0,0));}let same=[json!({"tool":"propose_edit","filePath":"a.tex","before":"a","after":"b"})];for _ in 0..20{assert!(!g.observe(&same,0,1));}for _ in 0..20{assert!(!g.observe(&same,1,0));}}
 #[test]fn cached_failure_is_not_progress_and_reordered_same_batch_does_not_evade_guard(){let mut g=LoopGuard::default();let a=json!({"tool":"read_file","path":"a.tex"});let b=json!({"tool":"read_file","path":"b.tex"});assert!(!g.observe(&[a.clone(),b.clone()],0,0));assert!(!g.observe(&[b.clone(),a.clone()],0,0));assert!(g.observe(&[a,b],0,0));assert!(STOP_REPLY.contains("待审建议会保留"));}
 #[test]fn empty_batch_resets_without_affecting_final_answer(){let mut g=LoopGuard::default();let batch=[json!({"tool":"list_files"})];g.observe(&batch,0,0);g.observe(&batch,0,0);assert!(!g.observe(&[],0,0));assert!(!g.observe(&batch,0,0));}
}

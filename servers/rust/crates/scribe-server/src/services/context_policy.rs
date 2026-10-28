//! Server-owned per-model context budget. Token counts are estimates, not provider usage.
use scribe_shared::AICompleteInput;
pub const DEFAULT_WINDOW:usize=262_144;
pub const MIN_WINDOW:usize=16_384;
pub const MAX_WINDOW:usize=2_097_152;
pub const MAX_HISTORY_MESSAGES:usize=2_000;
#[derive(Clone,Copy,Debug)]
pub struct ContextPolicy{pub window:usize}
impl Default for ContextPolicy{fn default()->Self{Self{window:DEFAULT_WINDOW}}}
impl ContextPolicy{
 pub fn new(window:usize)->Self{Self{window:window.clamp(MIN_WINDOW,MAX_WINDOW)}}
 pub fn from_input(input:&AICompleteInput)->Self{Self::new(input.options.as_ref().and_then(|o|o.get("projectContextWindowTokens")).and_then(|v|v.parse().ok()).unwrap_or(DEFAULT_WINDOW))}
 pub fn compact_at(self)->usize{self.window*4/5}
 pub fn history_messages(self)->usize{(self.window/128).clamp(40,MAX_HISTORY_MESSAGES)}
 pub fn history_chars(self)->usize{(self.window*2).min(2_000_000)}
 pub fn source_limits(self)->(usize,usize){(self.window/8,self.window/2)}
 pub fn output_reserve(self)->usize{4096.min(self.window/4)}
}
#[cfg(test)]mod tests{use super::*;#[test]fn windows_scale_history_and_compaction_without_early_character_trigger(){let small=ContextPolicy::new(16384);let large=ContextPolicy::new(262144);assert!(large.compact_at()>small.compact_at());assert!(large.history_chars()>small.history_chars());assert!(large.history_messages()>40);assert!(large.compact_at()+large.output_reserve()<large.window);assert_eq!(ContextPolicy::new(usize::MAX).window,MAX_WINDOW);}}

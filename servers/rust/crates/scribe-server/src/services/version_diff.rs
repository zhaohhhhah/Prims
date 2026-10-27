//! Bounded, changed-lines-only input for optional version summaries.
//! Never truncates into an incomplete claim or falls back to whole-file context.
use std::collections::BTreeMap;
use serde::Serialize;
use scribe_shared::VersionFile;
pub const MAX_CHANGED_FILES: usize = 8;
pub const MAX_DIFF_CHARS: usize = 12_000;
const MAX_CELLS: usize = 2_000_000;
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug,Serialize,PartialEq)]#[serde(rename_all="camelCase")]
pub struct DiffHunk {pub old_start:usize,pub new_start:usize,pub removed:Vec<String>,pub added:Vec<String>}
#[derive(Debug,Serialize,PartialEq)]pub struct FileDiff {pub path:String,pub hunks:Vec<DiffHunk>}
#[derive(Debug,Serialize,PartialEq)]pub struct RenameDiff {pub from:String,pub to:String}
#[derive(Debug,Serialize,PartialEq)]pub struct ChangedLines {pub files:Vec<FileDiff>,pub renames:Vec<RenameDiff>}
fn lines(text:&str)->Vec<&str>{text.split_inclusive('\n').collect()}
fn file_hunks(before:&str,after:&str)->Result<Vec<DiffHunk>,&'static str>{
 if before.len()>MAX_FILE_BYTES||after.len()>MAX_FILE_BYTES{return Err("单个文本文件过大，未向模型发送内容");}
 if before.bytes().filter(|b|*b==b'\n').count()>100_000||after.bytes().filter(|b|*b==b'\n').count()>100_000{return Err("单文件行数超过比较上限，未向模型发送内容");}
 let a=lines(before);let b=lines(after);let mut prefix=0;while prefix<a.len().min(b.len())&&a[prefix]==b[prefix]{prefix+=1;}
 let mut suffix=0;while suffix<(a.len()-prefix).min(b.len()-prefix)&&a[a.len()-suffix-1]==b[b.len()-suffix-1]{suffix+=1;}
 let aa=&a[prefix..a.len()-suffix];let bb=&b[prefix..b.len()-suffix];
 let width=bb.len()+1;let cells=(aa.len()+1).checked_mul(width).ok_or("文本比较规模过大，未向模型发送内容")?;
 if cells>MAX_CELLS{return Err("文本改动分散且比较规模过大，未向模型发送内容");}
 let mut dp=vec![0u32;cells];for i in (0..aa.len()).rev(){for j in (0..bb.len()).rev(){dp[i*width+j]=if aa[i]==bb[j]{1+dp[(i+1)*width+j+1]}else{dp[(i+1)*width+j].max(dp[i*width+j+1])};}}
 let(mut i,mut j)=(0,0);let mut hunks=vec![];
 while i<aa.len()||j<bb.len(){if i<aa.len()&&j<bb.len()&&aa[i]==bb[j]{i+=1;j+=1;continue;}
  let mut h=DiffHunk{old_start:prefix+i+1,new_start:prefix+j+1,removed:vec![],added:vec![]};
  while i<aa.len()||j<bb.len(){if i<aa.len()&&j<bb.len()&&aa[i]==bb[j]{break;}
   if i<aa.len()&&(j==bb.len()||dp[(i+1)*width+j]>=dp[i*width+j+1]){h.removed.push(aa[i].to_string());i+=1;}else{h.added.push(bb[j].to_string());j+=1;}
  }hunks.push(h);
 }Ok(hunks)
}
fn index(files:&[VersionFile])->Result<BTreeMap<&str,&str>,&'static str>{let mut map=BTreeMap::new();for file in files{if file.path.is_empty()||file.path.len()>512||file.path.chars().any(char::is_control)||map.insert(file.path.as_str(),file.content.as_str()).is_some(){return Err("版本文件路径无效或重复，未向模型发送内容");}}Ok(map)}
pub fn changed_lines(before:&[VersionFile],after:&[VersionFile])->Result<ChangedLines,&'static str>{
 if before.len()>500||after.len()>500{return Err("版本文件数量超过比较上限，未向模型发送内容");}let old=index(before)?;let new=index(after)?;let mut paths=old.keys().chain(new.keys()).copied().collect::<Vec<_>>();paths.sort_unstable();paths.dedup();let mut files=vec![];let mut renames=vec![];let mut renamed=std::collections::HashSet::new();
 for (&from,&content) in &old{if new.contains_key(from){continue;}for (&to,&new_content) in &new{if !old.contains_key(to)&&!renamed.contains(to)&&content==new_content{renamed.insert(from);renamed.insert(to);renames.push(RenameDiff{from:from.into(),to:to.into()});break;}}}
 for path in paths{if renamed.contains(path){continue;}let a=old.get(path).copied().unwrap_or("");let b=new.get(path).copied().unwrap_or("");if a==b{continue;}if files.len()+renames.len()>=MAX_CHANGED_FILES{return Err("变更超过 8 个文件，未向模型发送内容");}files.push(FileDiff{path:path.into(),hunks:file_hunks(a,b)?});}
 if renames.len()>MAX_CHANGED_FILES{return Err("变更超过 8 个文件，未向模型发送内容");}let value=ChangedLines{files,renames};let encoded=serde_json::to_string(&value).map_err(|_|"无法编码文本改动")?;if encoded.chars().count()>MAX_DIFF_CHARS{return Err("文本改动超过 12000 字符，未向模型发送内容");}Ok(value)
}
#[cfg(test)]mod tests{
 use super::*;fn f(path:&str,text:&str)->VersionFile{VersionFile{path:path.into(),content:text.into()}}
 #[test]fn sends_only_changed_lines_and_merges_adjacent_replacements(){let a=vec![f("a.tex","private unchanged\na\nb\nkeep secret\nc\n")];let b=vec![f("a.tex","private unchanged\nx\ny\nkeep secret\nz\n")];let d=changed_lines(&a,&b).unwrap();assert_eq!(d.files[0].hunks.len(),2);let text=serde_json::to_string(&d).unwrap();assert!(!text.contains("private unchanged")&&!text.contains("keep secret"));assert_eq!(d.files[0].hunks[0].removed,vec!["a\n","b\n"]);}
 #[test]fn deduplicates_unchanged_paths_and_handles_added_deleted_files(){let d=changed_lines(&[f("gone.txt","removed")],&[f("new.txt","added")]).unwrap();assert_eq!(d.files.len(),2);assert_eq!(d.files[0].hunks[0].removed,vec!["removed"]);assert_eq!(d.files[1].hunks[0].added,vec!["added"]);assert!(changed_lines(&[f("a","x")],&[f("a","x")]).unwrap().files.is_empty());}
 #[test]fn unicode_newline_and_repeated_line_changes_are_exact(){let d=changed_lines(&[f("a","同\n同\n末")],&[f("a","同\n变\n同\n末\n")]).unwrap();let h=&d.files[0].hunks;assert_eq!(h[0].added,vec!["变\n"]);assert_eq!(h[1].removed,vec!["末"]);assert_eq!(h[1].added,vec!["末\n"]);}
 #[test]fn all_limits_fail_closed_without_whole_file_fallback(){let old=vec![f("a","unchanged\n")];let new=vec![f("a",&format!("unchanged\n{}","增".repeat(MAX_DIFF_CHARS)))];assert!(changed_lines(&old,&new).is_err());let new=(0..9).map(|n|f(&format!("{n}.tex"),"x")).collect::<Vec<_>>();assert!(changed_lines(&[],&new).is_err());let a=(0..1500).map(|n|format!("a{n}\n")).collect::<String>();let b=(0..1500).map(|n|format!("b{n}\n")).collect::<String>();assert!(changed_lines(&[f("a",&a)],&[f("a",&b)]).is_err());assert!(changed_lines(&[f("a","1"),f("a","2")],&[]).is_err());}
 #[test]fn exact_renames_send_metadata_without_repeated_contents(){let d=changed_lines(&[f("old.tex","private original contents")],&[f("new.tex","private original contents")]).unwrap();assert!(d.files.is_empty());assert_eq!(d.renames.len(),1);assert!(!serde_json::to_string(&d).unwrap().contains("private original"));}
 #[test]fn large_unchanged_prefix_is_not_serialized(){let prefix="secret untouched\n".repeat(5000);let d=changed_lines(&[f("a",&format!("{prefix}old\n"))],&[f("a",&format!("{prefix}new\n"))]).unwrap();let json=serde_json::to_string(&d).unwrap();assert!(!json.contains("secret untouched"));assert!(json.len()<200);}
}

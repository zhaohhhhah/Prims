//! Guard code proposals independently from Markdown-rendered assistant replies.
//! No semantic Markdown-to-TeX conversion and no changes to `before` anchors.
const MAX_AFTER_CHARS:usize=40_000;
const CONTAMINATED:&str="已保留答复；此处候选混入 Markdown 格式，未作为 LaTeX 修改应用，请要求模型只返回 LaTeX 源码。";
fn is_tex(path:&str)->bool{matches!(path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),"tex"|"sty"|"cls"|"ltx"|"bib")}
fn outer_tex_fence(text:&str)->Option<&str>{
 let outer=text.trim();let (first,rest)=outer.split_once('\n')?;if !matches!(first.trim().to_ascii_lowercase().as_str(),"```tex"|"```latex"){return None;}
 let (body,last)=rest.rsplit_once('\n')?;if last.trim()!="```"{return None;}Some(body.strip_suffix('\r').unwrap_or(body))
}
fn mask(text:&str)->String{text.chars().map(|c|if c=='\n'{'\n'}else{' '}).collect()}
fn literal_environment(name:&str)->bool{matches!(name,"verbatim"|"verbatim*"|"Verbatim"|"Verbatim*"|"BVerbatim"|"LVerbatim"|"SaveVerbatim"|"lstlisting"|"minted"|"comment"|"math"|"displaymath"|"equation"|"equation*"|"align"|"align*"|"alignat"|"alignat*"|"gather"|"gather*"|"multline"|"multline*")}
fn unescaped_end(text:&str,start:usize,delimiter:&str)->Option<usize>{let mut offset=start;while let Some(found)=text[offset..].find(delimiter){let position=offset+found;let escapes=text[..position].bytes().rev().take_while(|b|*b==b'\\').count();if escapes%2==0{return Some(position+delimiter.len());}offset=position+delimiter.len();}None}
/// Keep only ordinary source text visible to Markdown checks; never rewrite its payload.
fn visible_source(text:&str)->String{
 let mut out=String::with_capacity(text.len());let mut i=0;
 while i<text.len(){let tail=&text[i..];
  if tail.starts_with('%'){let length=tail.find('\n').unwrap_or(tail.len());out.push_str(&mask(&tail[..length]));i+=length;continue;}
  if tail.starts_with("\\begin{"){if let Some(close)=tail.find('}'){let name=&tail[7..close];if literal_environment(name){let end_tag=format!("\\end{{{name}}}");let end=tail[close+1..].find(&end_tag).map(|p|close+1+p+end_tag.len()).unwrap_or(tail.len());out.push_str(&mask(&tail[..end]));i+=end;continue;}}}
  if tail.starts_with("\\verb"){let start=if tail.starts_with("\\verb*"){6}else{5};if let Some(delimiter)=tail[start..].chars().next().filter(|c|!c.is_alphanumeric()&&!c.is_whitespace()){
   let rest=start+delimiter.len_utf8();let end=tail[rest..].find(delimiter).map(|p|rest+p+delimiter.len_utf8()).unwrap_or_else(||tail.find('\n').unwrap_or(tail.len()));out.push_str(&mask(&tail[..end]));i+=end;continue;
  }}
  let math=if tail.starts_with("\\["){Some((2,"\\]"))}else if tail.starts_with("\\("){Some((2,"\\)"))}else if tail.starts_with("$$"){Some((2,"$$"))}else if tail.starts_with('$'){Some((1,"$"))}else{None};
  if let Some((length,end_tag))=math{let end=unescaped_end(tail,length,end_tag).unwrap_or(tail.len());out.push_str(&mask(&tail[..end]));i+=end;continue;}
  if tail.starts_with('\\'){out.push('\\');i+=1;if let Some(c)=text[i..].chars().next(){out.push(c);i+=c.len_utf8();}continue;}
  let c=tail.chars().next().unwrap();out.push(c);i+=c.len_utf8();
 }out
}
fn has_bold(text:&str)->bool{for marker in ["**","__"]{let mut offset=0;while let Some(start)=text[offset..].find(marker){let begin=offset+start+marker.len();if let Some(end)=text[begin..].find(marker){let inner=&text[begin..begin+end];if !inner.is_empty()&&!inner.starts_with(char::is_whitespace)&&!inner.ends_with(char::is_whitespace){return true;}}offset=begin;if offset>=text.len(){break;}}}false}
fn bullet(line:&str)->bool{let line=line.trim_start();let rest=line.strip_prefix("- ").or_else(||line.strip_prefix("+ ")).or_else(||line.strip_prefix("* "));if let Some(rest)=rest{return rest.chars().next().is_some_and(|c|c.is_alphabetic());}let digits=line.bytes().take_while(u8::is_ascii_digit).count();digits>0&&digits<=3&&(line[digits..].starts_with(". ")||line[digits..].starts_with(") "))}
pub fn sanitize_tex_after(path:&str,after:&str)->Result<String,&'static str>{
 if after.len()>160_000||after.chars().count()>MAX_AFTER_CHARS{return Err("候选修改超过安全长度，请分段修改。");}if !is_tex(path){return Ok(after.to_string());}
 let source=outer_tex_fence(after).unwrap_or(after);let visible=visible_source(source);let mut last_bullet=false;
 for line in visible.lines(){let line=line.trim_start();let hashes=line.bytes().take_while(|b|*b==b'#').count();if (1..=6).contains(&hashes)&&line[hashes..].starts_with(char::is_whitespace){return Err(CONTAMINATED);}if line.starts_with("```")||line.starts_with("~~~"){return Err(CONTAMINATED);}let current=bullet(line);if current&&last_bullet{return Err(CONTAMINATED);}last_bullet=current&&!line.trim().is_empty();}
 if has_bold(&visible){return Err(CONTAMINATED);}Ok(source.to_string())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn only_complete_explicit_outer_tex_fence_is_removed(){let code="\\section{Title}\nText.";assert_eq!(sanitize_tex_after("main.tex",&format!("```latex\n{code}\n```")),Ok(code.into()));assert_eq!(sanitize_tex_after("x.LTX",&format!(" \n```TeX\r\n{code}\r\n```\n ")),Ok(code.into()));for text in ["```latex\nunfinished","explanation\n```latex\nx\n```","```markdown\n# bad\n```"]{assert!(sanitize_tex_after("main.tex",text).is_err());}}
 #[test]fn markdown_headings_bold_and_lists_are_rejected_only_for_tex_targets(){for path in ["main.tex","style.sty","a.cls","x.ltx","refs.bib"]{for text in ["# Heading","## Heading","This is **bold**.","This is __bold__.","- First\n- Second","1. First\n2. Second"]{assert!(sanitize_tex_after(path,text).is_err(),"{path}: {text}");}}assert_eq!(sanitize_tex_after("README.md","# Heading\n**bold**"),Ok("# Heading\n**bold**".into()));}
 #[test]fn normal_tex_commands_comments_and_math_negative_signs_are_preserved(){for text in ["\\section{Title}\n\\textbf{Strong}","\\newcommand{\\a}[1]{#1}","% # heading **example**\nText",r"$a ** b - x$",r"\[ - x + **y** \]",r"\( x__y \)","\\begin{align}\n- x &= y \\\\\n- z &= w\n\\end{align}","- x"]{assert_eq!(sanitize_tex_after("main.tex",text),Ok(text.into()));}}
 #[test]fn literal_environments_and_inline_verbs_allow_markdown_examples(){for environment in ["verbatim","verbatim*","Verbatim","lstlisting","minted","comment"]{let text=format!("\\begin{{{environment}}}{{text}}\n# heading\n**bold**\n```json\n- first\n- second\n\\end{{{environment}}}");assert_eq!(sanitize_tex_after("main.tex",&text),Ok(text));}for text in [r"\verb|# **bold** ```|",r"\verb*+__bold__+",r"Escaped \% then \textbf{normal}"]{assert_eq!(sanitize_tex_after("main.tex",text),Ok(text.into()));}}
 #[test]fn only_wrapper_changes_candidate_internal_source_remains_exact(){let body="  \\section{A}\n\n\\textbf{B}  ";assert_eq!(sanitize_tex_after("a.tex",&format!("```latex\n{body}\n```")),Ok(body.into()));let unchanged="\\section{A}\n";assert_eq!(sanitize_tex_after("a.tex",unchanged),Ok(unchanged.into()));assert!(sanitize_tex_after("a.tex",&"x".repeat(MAX_AFTER_CHARS+1)).is_err());}
}

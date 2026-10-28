//! Fixed-entry isolated TeX runner. Never execute this in the credential-bearing app.
use scribe_compile::{run_latexmk, LatexEngine, LatexmkConfig};
use std::{path::Path, time::Duration};
#[tokio::main]
async fn main() {
    let args:Vec<String>=std::env::args().collect();
    let engine=args.get(1).and_then(|s|LatexEngine::parse_name(s));
    let main=args.get(2).filter(|s|!s.starts_with('/')&&!s.contains('\\')&&s.split('/').all(|p|!p.is_empty()&&!matches!(p,"."|"..")&&!p.starts_with('-')));
    if args.len()!=3||engine.is_none()||main.is_none(){std::process::exit(2);}
    let config=LatexmkConfig{engine:engine.unwrap(),timeout:Duration::from_secs(60),..Default::default()};
    let result=run_latexmk(&config,Path::new("/workspace"),main.unwrap()).await;
    let log=format!("{}\n{}",result.stdout,result.stderr);
    println!("{}",serde_json::json!({"ok":result.success,"exitCode":result.exit_code,"output":log.chars().take(16000).collect::<String>(),"durationMs":result.duration.as_millis()}));
}

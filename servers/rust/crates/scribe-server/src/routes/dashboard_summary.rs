//! Selected-project bookshelf metadata. Read-only, member-scoped and bounded.
use axum::{
    extract::{Path, State}, middleware::from_fn, routing::get, Json, Router,
};
use scribe_auth::{require_auth, Authenticated};
use scribe_shared::{ApiError, ApiResult, ErrorCode, ProjectId};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;
use crate::{state::AppState, services::{FileService, membership::assert_member}};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    collaborators: Vec<Person>,
    version_count: i64,
    latest_pdf_job_id: Option<Uuid>,
    word_count: Option<usize>,
    word_count_partial: bool,
}
#[derive(Serialize)]
struct Person { id: Uuid, name: String }

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/projects/:project_id/dashboard-summary", get(summary))
        .layer(from_fn(require_auth))
}

async fn summary(
    State(state): State<AppState>,
    Authenticated(user): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Summary>> {
    let pool = state.db().ok_or_else(|| {
        ApiError::new(ErrorCode::ServiceUnavailable, "database not configured")
    })?.pool();
    let project = ProjectId::new(id);
    assert_member(pool, user.id, project).await?;
    let row = sqlx::query(r#"
        select p.created_at,
          greatest(p.updated_at, coalesce((select max(f.updated_at)
            from public.project_files f where f.project_id=p.id),p.updated_at)) updated_at,
          (select count(*) from public.project_versions v where v.project_id=p.id) versions,
          (select j.id from public.compile_jobs j where j.project_id=p.id
            and j.pdf_key is not null order by j.enqueued_at desc limit 1) latest_pdf_job_id
        from public.projects p where p.id=$1
    "#).bind(id).fetch_one(pool).await
        .map_err(|_| ApiError::internal("Project summary unavailable"))?;
    let people = sqlx::query(r#"
        select u.id, coalesce(nullif(u.display_name,''),'Member') name
        from public.users u where u.id in (
          select owner_id from public.projects where id=$1 union
          select user_id from public.project_members
          where project_id=$1 and invite_accepted_at is not null
        ) order by u.id limit 100
    "#).bind(id).fetch_all(pool).await
        .map_err(|_| ApiError::internal("Project members unavailable"))?;

    let mut count = 0usize;
    let mut partial = false;
    let mut readable = true;
    let mut total = 0usize;
    let mut read = 0usize;
    let mut eligible = 0usize;
    if let Some(storage) = state.inner.storage.clone() {
        let svc = FileService::new(pool.clone(), storage);
        let files = svc.list(user.id, project).await?;
        for file in files {
            let ext = file.path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if !matches!(ext.as_str(), "tex" | "ltx" | "latex") { continue; }
            eligible += 1;
            if read >= 32 || file.size_bytes < 0 || file.size_bytes > 262144
                || total.saturating_add(file.size_bytes.max(0) as usize) > 1048576 {
                partial = true;
                continue;
            }
            read += 1;
            total += file.size_bytes as usize;
            match svc.read_reference_bytes(user.id, project, file.id, 262144).await {
                Ok(bytes) => match std::str::from_utf8(&bytes) {
                    Ok(text) => count += estimate_words(text),
                    Err(_) => { readable = false; partial = true; }
                },
                Err(_) => { readable = false; partial = true; }
            }
        }
    } else {
        readable = false;
    }
    Ok(Json(Summary {
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        collaborators: people.into_iter().map(|r| Person {
            id: r.get("id"), name: r.get("name"),
        }).collect(),
        version_count: row.get("versions"),
        latest_pdf_job_id: row.get("latest_pdf_job_id"),
        word_count: (readable && (eligible == 0 || read > 0)).then_some(count),
        word_count_partial: partial,
    }))
}

/// A lightweight natural-text estimate: Han characters + alphabetic words.
/// Excludes comments, commands, common math/code environments and reference args.
fn estimate_words(text: &str) -> usize {
    const SKIP_ENV: &[&str] = &[
        "verbatim", "lstlisting", "minted", "equation", "equation*", "align", "align*",
        "gather", "gather*", "displaymath", "math",
    ];
    let mut cleaned = String::new();
    let mut math = false;
    let mut skip_env = false;
    for line in text.lines() {
        if SKIP_ENV.iter().any(|env| line.contains(&format!("\\begin{{{env}}}"))) {
            skip_env = true;
        }
        if skip_env {
            if SKIP_ENV.iter().any(|env| line.contains(&format!("\\end{{{env}}}"))) {
                skip_env = false;
            }
            continue;
        }
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '%' => break,
                '$' => { math = !math; cleaned.push(' '); }
                '\\' => {
                    if matches!(chars.peek(), Some('[' | '(')) {
                        chars.next(); math = true; continue;
                    }
                    if matches!(chars.peek(), Some(']' | ')')) {
                        chars.next(); math = false; continue;
                    }
                    if chars.peek() == Some(&'%') {
                        chars.next();
                        if !math { cleaned.push('%'); }
                        continue;
                    }
                    let mut command = String::new();
                    while chars.peek().is_some_and(|c| c.is_alphabetic()) {
                        command.push(chars.next().unwrap());
                    }
                    if matches!(command.as_str(), "documentclass" | "usepackage" | "include"
                        | "input" | "includegraphics" | "label" | "ref" | "cite"
                        | "bibliography" | "begin" | "end") {
                        while chars.peek() == Some(&'[') {
                            for x in chars.by_ref() { if x == ']' { break; } }
                        }
                        if chars.peek() == Some(&'{') {
                            chars.next();
                            let mut depth = 1;
                            for x in chars.by_ref() {
                                if x == '{' { depth += 1; }
                                if x == '}' {
                                    depth -= 1;
                                    if depth == 0 { break; }
                                }
                            }
                        }
                    }
                    cleaned.push(' ');
                }
                '{' | '}' => cleaned.push(' '),
                _ => if !math { cleaned.push(c); },
            }
        }
        cleaned.push(' ');
    }
    let mut count = 0;
    let mut word = false;
    for c in cleaned.chars() {
        if ('\u{3400}'..='\u{9fff}').contains(&c) { count += 1; word = false; }
        else if c.is_alphabetic() { if !word { count += 1; } word = true; }
        else if c != '\'' && c != '-' { word = false; }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural_text_not_tex_size() {
        assert_eq!(estimate_words("\\documentclass{article}\n\\begin{document}\n\\section{Hello world}\n你好 test words % hidden comment\n$x+y$\\label{not-words}\n\\end{document}"), 6);
    }
    #[test]
    fn ignores_code_and_math() {
        assert_eq!(estimate_words("Visible text\n\\begin{verbatim}\nsecret code here\n\\end{verbatim}\n$a+b$"), 2);
    }
    #[test]
    fn bracket_and_environment_math_is_not_prose() {
        assert_eq!(estimate_words("Visible text\n\\[ x + y \\] \\( alpha beta \\)\n\\begin{equation}\nx = many variables\n\\end{equation}\n\\begin{align*}\na &= b + c\n\\end{align*}"), 2);
    }
}

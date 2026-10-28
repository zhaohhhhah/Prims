//! Bundled workflow documents, stored as ordinary user-scoped skills.
//! Their stable IDs prevent reseeding after edits/disable/archive; no skill grants tools.
use super::agent_library::validate_skill;
use scribe_shared::{ApiError, ApiResult, UserId};
use sqlx::{PgPool, Postgres, Transaction};

pub const PERSONAL_LIMIT: i64 = 32;
pub const LIST_LIMIT: i64 = PERSONAL_LIMIT + DOCUMENTS.len() as i64;
const ID_NAMESPACE: &str = ":prims-builtin-v1:";
pub const DOCUMENTS: &[&str] = &[
    include_str!("builtin_skills/references/SKILL.md"),
    include_str!("builtin_skills/organize/SKILL.md"),
    include_str!("builtin_skills/consistency/SKILL.md"),
    include_str!("builtin_skills/compile-repair/SKILL.md"),
    include_str!("builtin_skills/submission/SKILL.md"),
    include_str!("builtin_skills/history/SKILL.md"),
];
fn documents() -> ApiResult<Vec<(String, String, &'static str)>> {
    DOCUMENTS.iter().map(|content| {
        let (name, description) = validate_skill(content)?;
        Ok((name, description, *content))
    }).collect()
}

/// Same per-user lock as AgentLibrary::save, with no schema migration or overwrite.
/// Archived IDs/names count as an explicit user decision and are never recreated.
pub async fn ensure(pool: &PgPool, user: UserId) -> ApiResult<()> {
    let docs = documents()?;
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,319872))")
        .bind(user.to_string()).execute(&mut *tx).await.map_err(db)?;
    for (name, description, content) in docs {
        sqlx::query("insert into public.scribe_agent_skills(id,user_id,name,description,content,enabled) select md5($1::uuid::text||$5||$2)::uuid,$1,$2,$3,$4,true where not exists(select 1 from public.scribe_agent_skills where id=md5($1::uuid::text||$5||$2)::uuid or (user_id=$1 and name=$2)) on conflict do nothing")
            .bind(user.into_inner()).bind(name).bind(description).bind(content).bind(ID_NAMESPACE)
            .execute(&mut *tx).await.map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(())
}

/// The six bundled records do not consume the existing 32-personal-skill quota.
/// Identity, not editable names or content, distinguishes a bundled record.
pub async fn personal_count(tx: &mut Transaction<'_, Postgres>, user: UserId) -> ApiResult<i64> {
    let names = documents()?.into_iter().map(|(name,_,_)|name).collect::<Vec<_>>();
    sqlx::query_scalar("select count(*) from public.scribe_agent_skills where user_id=$1 and archived_at is null and id not in(select md5($1::uuid::text||$3||name)::uuid from unnest($2::text[]) as names(name))")
        .bind(user.into_inner()).bind(names).bind(ID_NAMESPACE)
        .fetch_one(&mut **tx).await.map_err(db)
}
fn db(_: sqlx::Error) -> ApiError { ApiError::internal("Bundled workflow library unavailable") }

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    #[test]
    fn bundled_workflows_are_valid_small_and_progressively_loaded() {
        let docs=documents().unwrap();
        assert_eq!(docs.len(),6);
        assert_eq!(docs.iter().map(|(name,_,_)|name).collect::<HashSet<_>>().len(),6);
        assert_eq!(LIST_LIMIT,38);
        assert!(docs.iter().all(|(_,description,content)|description.chars().count()<100 && content.len()<9000));
        assert!(docs.iter().map(|(_,description,_)|description.chars().count()).sum::<usize>()<350);
    }
    #[test]
    fn workflow_documents_keep_mutations_pending_and_never_grant_execution() {
        for (_,_,content) in documents().unwrap() {
            assert!(content.contains("接受"));
            assert!(content.contains("不能"));
            assert!(!content.contains("allowed-tools:"));
            assert!(!content.contains("sudo "));
        }
    }
    #[test]
    fn bibliography_requires_verifiable_sources_and_history_requires_current_source() {
        assert!(DOCUMENTS[0].contains("DOI") && DOCUMENTS[0].contains("不得捏造"));
        assert!(DOCUMENTS[5].contains("read_file") && DOCUMENTS[5].contains("propose_edit"));
        assert!(DOCUMENTS[5].contains("list_versions") && DOCUMENTS[5].contains("compare_versions") && DOCUMENTS[5].contains("read_version_file"));
        assert!(DOCUMENTS[3].contains("engineMismatch") && DOCUMENTS[3].contains("set_compile_settings"));
    }
}

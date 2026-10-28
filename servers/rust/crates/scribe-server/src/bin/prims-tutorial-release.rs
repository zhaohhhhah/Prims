//! Operator-only release runner. Dry-run is the default; applies use the same service as login.
#[path = "../services/welcome.rs"]
mod welcome;
#[path = "../services/membership.rs"]
pub(crate) mod membership;
mod services { pub(crate) use crate::membership; }
#[path = "../services/compiles.rs"]
mod compiles;

use anyhow::{Context, bail};
use scribe_shared::UserId;
use scribe_storage::SupabaseStorage;
use serde_json::json;
use sqlx::{Row, postgres::PgPoolOptions};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut apply = false;
    let mut expected = None;
    let mut selected_user = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--apply" => apply = true,
            "--dry-run" => {},
            "--expect-pending" => expected = Some(args.next().context("missing pending count")?.parse::<usize>()?),
            "--user" => selected_user = Some(Uuid::parse_str(&args.next().context("missing user id")?)?),
            _ => bail!("usage: prims-tutorial-release [--dry-run | --apply --expect-pending N] [--user UUID]"),
        }
    }
    if apply && expected.is_none() { bail!("--apply requires --expect-pending from the reviewed dry-run"); }
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPoolOptions::new().max_connections(3).connect(&database_url).await.context("connecting database")?;
    let has_table: bool = sqlx::query_scalar("select to_regclass('public.prims_tutorial_releases') is not null").fetch_one(&pool).await?;
    if apply && !has_table { bail!("apply requires the tutorial release migration first"); }
    let users: Vec<Uuid> = sqlx::query_scalar("select id from auth.users where deleted_at is null and ($1::uuid is null or id=$1) order by id")
        .bind(selected_user).fetch_all(&pool).await?;
    if selected_user.is_some() && users.is_empty() { bail!("requested active user was not found"); }
    let mut pending = Vec::new();
    let mut already_released = 0;
    let mut deleted_projects = 0;
    for user in &users {
        let marker = if has_table {
            sqlx::query("select project_id from public.prims_tutorial_releases where user_id=$1 and release_id=$2")
                .bind(*user).bind(welcome::TUTORIAL_RELEASE).fetch_optional(&pool).await?
        } else { None };
        if let Some(marker) = marker {
            already_released += 1;
            if marker.get::<Option<Uuid>, _>("project_id").is_none() { deleted_projects += 1; }
        } else { pending.push(*user); }
    }
    println!("{}", json!({"mode":if apply {"apply"} else {"dry-run"}, "releaseId":welcome::TUTORIAL_RELEASE,
        "name":welcome::TUTORIAL_NAME,"migrationPresent":has_table,"activeUsers":users.len(),"pending":pending.len(),
        "alreadyReleased":already_released,"deletedTutorialsKeptDeleted":deleted_projects,"pendingUserIds":pending}));
    if !apply { return Ok(()); }
    if expected != Some(pending.len()) { bail!("pending count changed; review a fresh dry-run"); }
    // Storage credentials are needed only after the complete read-only plan passes.
    let storage = SupabaseStorage::new(&std::env::var("SUPABASE_URL").context("SUPABASE_URL is required")?,
        &std::env::var("SUPABASE_SERVICE_ROLE_KEY").context("SUPABASE_SERVICE_ROLE_KEY is required")?)?;
    let mut failed = 0;
    for user in pending {
        match welcome::ensure(&pool, &storage, UserId::new(user)).await {
            Ok(()) => {
                if let Ok(redis_url)=std::env::var("REDIS_URL") {
                    if let Err(_)=welcome::enqueue_initial_pdf(&pool,UserId::new(user),&redis_url).await {
                        eprintln!("{}",json!({"userId":user,"status":"released-pdf-deferred"}));
                    }
                }
                println!("{}", json!({"userId":user,"status":"released-or-already-released"}));
            },
            Err(_) => { failed += 1; eprintln!("{}", json!({"userId":user,"status":"failed-retry-safe"})); }
        }
    }
    if failed > 0 { bail!("{failed} users failed; rerun dry-run before retrying"); }
    println!("{}", json!({"status":"complete","releaseId":welcome::TUTORIAL_RELEASE}));
    Ok(())
}

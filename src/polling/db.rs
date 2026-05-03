//! Database operations for the polling engine.

use futures::{StreamExt, stream};
use sqlx::SqlitePool;
use tracing::warn;

use crate::{
    error::CommitHashError,
    model::Branch,
    polling::{branch::BranchInfo, git::GitFetcher},
};

/// Gathers stored branches that need to be updated.
pub(super) async fn gather_updated_branches(
    pool: &SqlitePool,
    fetcher: &dyn GitFetcher,
) -> Result<Vec<BranchInfo>, sqlx::Error> {
    // TODO: Make buffer size configurable.
    const BUFFER_SIZE: usize = 3;

    let branch_results = stream::iter(collect_branches(pool).await?)
        .map(|b| BranchInfo::new(b, fetcher))
        .buffer_unordered(BUFFER_SIZE)
        .collect::<Vec<Result<BranchInfo, CommitHashError>>>()
        .await;

    let errs = branch_results.iter().filter_map(|res| res.as_ref().err());
    for e in errs {
        warn!("{e}");
    }

    let updated_branches = branch_results
        .into_iter()
        .filter_map(|res| res.ok())
        .filter(BranchInfo::has_updated)
        .collect();
    Ok(updated_branches)
}

/// Collects all branch rows.
async fn collect_branches(pool: &SqlitePool) -> Result<Vec<Branch>, sqlx::Error> {
    let branches = sqlx::query_as::<_, Branch>("SELECT * FROM branches")
        .fetch_all(pool)
        .await?;

    Ok(branches)
}

/// Writes the updated branch hash to the row.
pub(super) async fn write_db<'e, E>(
    branch_info: &BranchInfo,
    executor: E,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query!(
        "UPDATE branches SET last_commit_hash = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        branch_info.latest_hash,
        branch_info.branch.id
    )
    .execute(executor)
    .await?;

    Ok(())
}

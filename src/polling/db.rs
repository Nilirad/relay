//! Database operations for the polling engine.

use futures::{StreamExt, TryFutureExt, stream};
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::{error::AppError, model::Branch, polling::branch::BranchInfo};

/// Gathers stored branches that need to be updated.
pub(super) async fn gather_updated_branches(
    pool: &SqlitePool,
) -> Result<Vec<BranchInfo>, AppError> {
    // TODO: Make buffer size configurable.
    const BUFFER_SIZE: usize = 3;

    let branch_results = stream::iter(collect_branches(pool).await?)
        .map(BranchInfo::new)
        .buffer_unordered(BUFFER_SIZE)
        .collect::<Vec<Result<BranchInfo, AppError>>>()
        .await;

    let errs = branch_results.iter().filter_map(|res| res.as_ref().err());
    for e in errs {
        warn!("Error fetching branch update: {e}");
    }

    let updated_branches = branch_results
        .into_iter()
        .filter_map(|res| res.ok())
        .filter(BranchInfo::has_updated)
        .collect();
    Ok(updated_branches)
}

/// Updates branch rows with the latest hash.
pub(super) async fn update_branches_table(pool: &SqlitePool, branch_infos: Vec<BranchInfo>) {
    let query_results = stream::iter(branch_infos)
        .then(|b_info| write_db(b_info, pool))
        .collect::<Vec<Result<BranchInfo, AppError>>>()
        .await;

    let errs = query_results.iter().filter_map(|res| res.as_ref().err());
    for e in errs {
        warn!("{e}");
    }

    let query_outcomes = query_results.into_iter().filter_map(|res| res.ok());
    for outcome in query_outcomes {
        info!(
            "New commit detected for branch {}. Hash: {}",
            outcome.branch.name, outcome.latest_hash
        );
    }
}

/// Collects all branch rows.
async fn collect_branches(pool: &SqlitePool) -> Result<Vec<Branch>, AppError> {
    let branches = sqlx::query_as::<_, Branch>("SELECT * FROM branches")
        .fetch_all(pool)
        .await?;

    Ok(branches)
}

/// Writes the updated branch hash to the row.
async fn write_db(branch_info: BranchInfo, pool: &SqlitePool) -> Result<BranchInfo, AppError> {
    sqlx::query!(
        "UPDATE branches SET last_commit_hash = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        branch_info.latest_hash,
        branch_info.branch.id
    )
    .execute(pool)
    .map_err(AppError::from)
    .await?;

    Ok(branch_info)
}

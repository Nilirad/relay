use futures::{StreamExt, TryFutureExt, stream};
use sqlx::SqlitePool;
use tracing::{error, info};

use crate::{error::AppError, polling::BranchInfo};

pub(super) async fn update_branches_table(pool: &SqlitePool, branch_infos: Vec<BranchInfo>) {
    let query_results = stream::iter(branch_infos)
        .then(|b_info| write_db(b_info, pool))
        .collect::<Vec<Result<BranchInfo, AppError>>>()
        .await;

    let errs = query_results.iter().filter_map(|res| res.as_ref().err());
    for e in errs {
        error!("{e}");
    }

    let query_outcomes = query_results.into_iter().filter_map(|res| res.ok());
    for outcome in query_outcomes {
        info!(
            "New commit detected for branch {}. Hash: {}",
            outcome.branch.name, outcome.latest_hash
        );
    }
}

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

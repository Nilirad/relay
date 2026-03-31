use futures::stream::{self, StreamExt};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{error::AppError, model::Branch};

use self::{branch::*, hash::*, update::*};

mod branch;
mod hash;
mod update;

pub fn start_polling_engine(pool: SqlitePool, token: CancellationToken) {
    tokio::spawn(async move {
        info!("Polling engine started");
        polling_loop(pool, token).await;
    });
}

async fn polling_loop(pool: SqlitePool, token: CancellationToken) {
    loop {
        tokio::select! {
            _ = poll_branches(&pool, &token) => {}
            _ = token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down polling engine");
}

async fn poll_branches(pool: &SqlitePool, token: &CancellationToken) {
    let branches_result = gather_updated_branches(pool.clone()).await;
    process_branches_result(pool, branches_result, token.clone()).await;
}

async fn gather_updated_branches(pool: SqlitePool) -> Result<Vec<BranchInfo>, AppError> {
    // TODO: Make buffer size configurable.
    const BUFFER_SIZE: usize = 3;

    let branch_results = stream::iter(collect_branches(pool).await?)
        .map(extract_branch_info)
        .buffer_unordered(BUFFER_SIZE)
        .collect::<Vec<Result<BranchInfo, AppError>>>()
        .await;

    let errs = branch_results.iter().filter_map(|res| res.as_ref().err());
    for e in errs {
        error!("Error fetching branch update: {e}");
    }

    let updated_branches = branch_results
        .into_iter()
        .filter_map(|res| res.ok())
        .filter(branch_has_updated)
        .collect();
    Ok(updated_branches)
}

async fn collect_branches(pool: SqlitePool) -> Result<Vec<Branch>, AppError> {
    let branches = sqlx::query_as::<_, Branch>("SELECT * FROM branches")
        .fetch_all(&pool)
        .await?;

    Ok(branches)
}

async fn extract_branch_info(branch: Branch) -> Result<BranchInfo, AppError> {
    let latest_hash = get_latest_hash(branch.repo_url.clone(), branch.name.clone()).await?;
    Ok(BranchInfo {
        branch,
        latest_hash,
    })
}

fn branch_has_updated(branch_info: &BranchInfo) -> bool {
    branch_info.branch.last_commit_hash.as_deref() != Some(&branch_info.latest_hash)
}

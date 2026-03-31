use std::time::Duration;

use futures::stream::{self, StreamExt};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{error::AppError, model::Branch, polling::update::update_branches_table};

use self::{branch::*, hash::*};

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
            res = poll_branches(&pool) => {followup_poll(res, &token).await}
            _ = token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down polling engine");
}

async fn poll_branches(pool: &SqlitePool) -> Result<(), AppError> {
    let updated_branches = gather_updated_branches(pool).await?;
    update_branches_table(pool, updated_branches).await;

    Ok(())
}

async fn gather_updated_branches(pool: &SqlitePool) -> Result<Vec<BranchInfo>, AppError> {
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

async fn collect_branches(pool: &SqlitePool) -> Result<Vec<Branch>, AppError> {
    let branches = sqlx::query_as::<_, Branch>("SELECT * FROM branches")
        .fetch_all(pool)
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

async fn followup_poll(res: Result<(), AppError>, token: &CancellationToken) {
    // TODO: Make polling cooldown configurable and specific for each branch.
    const SLEEP_SECS: u64 = 5 * 60;
    match res {
        Ok(_) => tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(SLEEP_SECS)) => {}
            _ = token.cancelled() => {}
        },
        Err(e) => handle_polling_error(e, token).await,
    }
}

async fn handle_polling_error(error: AppError, token: &CancellationToken) {
    // TODO: Make retry cooldown configurable.
    const DB_ERROR_COOLDOWN_SECS: u64 = 5 * 60;

    if let AppError::Sqlx(e) = error {
        error!("{e}");
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(DB_ERROR_COOLDOWN_SECS)) => {}
            _ = token.cancelled() => {}
        }
    } else {
        error!("Unexpected error raised: {error}");
    }
}

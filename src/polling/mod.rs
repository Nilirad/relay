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

    let branches = stream::iter(collect_branches(pool).await?)
        .map(|b| async move {
            let latest_hash = get_latest_hash(b.repo_url.clone(), b.name.clone()).await;
            // TODO: Handle error.
            (b, latest_hash)
        })
        .buffer_unordered(BUFFER_SIZE)
        .filter_map(check_branch_for_updates)
        .collect::<Vec<BranchInfo>>()
        .await;

    Ok(branches)
}

async fn collect_branches(pool: SqlitePool) -> Result<Vec<Branch>, AppError> {
    let branches = sqlx::query_as::<_, Branch>("SELECT * FROM branches")
        .fetch_all(&pool)
        .await?;

    Ok(branches)
}

async fn check_branch_for_updates(
    (branch, latest_hash_res): (Branch, Result<String, AppError>),
) -> Option<BranchInfo> {
    match latest_hash_res {
        Ok(latest_hash) if branch.last_commit_hash.as_deref() != Some(&latest_hash) => {
            Some(BranchInfo {
                branch,
                latest_hash,
            })
        }
        Ok(_) => {
            info!("No new commit for branch {}", branch.name);
            None
        }
        Err(e) => {
            error!("Error while checking branch {}: {}", branch.name, e);
            None
        }
    }
}

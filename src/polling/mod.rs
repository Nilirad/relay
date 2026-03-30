use futures::stream::{self, StreamExt};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{error::AppError, model::Branch};

use self::{hash::*, update::*};

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

async fn gather_updated_branches(pool: SqlitePool) -> Result<Vec<(Branch, String)>, AppError> {
    // TODO: Make buffer size configurable.
    const BUFFER_SIZE: usize = 3;

    let branches = stream::iter(collect_branches(pool).await?)
        .map(|b| async move {
            let hash_result = get_latest_hash(b.repo_url.clone(), b.name.clone()).await;
            (b, hash_result)
        })
        .buffer_unordered(BUFFER_SIZE)
        .filter_map(check_branch_for_updates)
        .collect::<Vec<(Branch, String)>>()
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
    (branch, hash_result): (Branch, Result<String, AppError>),
) -> Option<(Branch, String)> {
    match hash_result {
        Ok(curr_hash) if branch.last_commit_hash.as_deref() != Some(&curr_hash) => {
            Some((branch, curr_hash))
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

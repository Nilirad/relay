use std::time::Duration;

use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{error::AppError, polling::BranchInfo};

pub(super) async fn process_branches_result(
    pool: &SqlitePool,
    branches_result: Result<Vec<BranchInfo>, AppError>,
    token: CancellationToken,
) {
    match branches_result {
        Ok(branches) => {
            update_branches_table(pool, &token, branches).await;
        }
        Err(e) => {
            handle_db_error(token, e).await;
        }
    }
}

async fn update_branches_table(
    pool: &SqlitePool,
    token: &CancellationToken,
    branches: Vec<BranchInfo>,
) {
    // TODO: Make polling cooldown configurable and specific for each branch.
    const SLEEP_SECS: u64 = 5 * 60;

    write_updates_to_db(pool.clone(), branches).await;
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(SLEEP_SECS)) => {}
        _ = token.cancelled() => {}
    }
}

async fn write_updates_to_db(pool: SqlitePool, updates_to_process: Vec<BranchInfo>) {
    for BranchInfo {
        branch,
        latest_hash,
    } in updates_to_process
    {
        info!(
            "New commit detected for branch {}. Hash: {}",
            branch.name, latest_hash
        );

        let query_result = sqlx::query!(
            "UPDATE branches SET last_commit_hash = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            latest_hash,
            branch.id
        )
        .execute(&pool)
        .await;

        if let Err(e) = query_result {
            error!("SQLx error: {e}");
        }
    }
}

async fn handle_db_error(token: CancellationToken, e: AppError) {
    // TODO: Make retry cooldown configurable.
    const DB_ERROR_COOLDOWN_SECS: u64 = 5 * 60;

    error!("SQLx error: Could not read branches: {e}");
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(DB_ERROR_COOLDOWN_SECS)) => {}
        _ = token.cancelled() => {}
    }
}

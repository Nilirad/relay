use std::time::Duration;

use futures::stream::{self, StreamExt};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{error::AppError, model::Branch};

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

async fn process_branches_result(
    pool: &SqlitePool,
    branches_result: Result<Vec<(Branch, String)>, AppError>,
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
    branches: Vec<(Branch, String)>,
) {
    // TODO: Make polling cooldown configurable and specific for each branch.
    const SLEEP_MINS: u64 = 5;

    write_updates_to_db(pool.clone(), branches).await;
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_mins(SLEEP_MINS)) => {}
        _ = token.cancelled() => {}
    }
}

async fn handle_db_error(token: CancellationToken, e: AppError) {
    // TODO: Make retry cooldown configurable.
    const DB_ERROR_COOLDOWN_MINS: u64 = 5;

    error!("SQLx error: Could not read branches: {e}");
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_mins(DB_ERROR_COOLDOWN_MINS)) => {}
        _ = token.cancelled() => {}
    }
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

async fn write_updates_to_db(pool: SqlitePool, updates_to_process: Vec<(Branch, String)>) {
    for (branch, hash) in updates_to_process {
        info!(
            "New commit detected for branch {}. Hash: {}",
            branch.name, hash
        );

        let query_result = sqlx::query!(
            "UPDATE branches SET last_commit_hash = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            hash,
            branch.id
        )
        .execute(&pool)
        .await;

        if let Err(e) = query_result {
            error!("SQLx error: {e}");
        }
    }
}

async fn collect_branches(pool: SqlitePool) -> Result<Vec<Branch>, AppError> {
    let branches = sqlx::query_as::<_, Branch>("SELECT * FROM branches")
        .fetch_all(&pool)
        .await?;

    Ok(branches)
}

/// Extracts the latest commit of a branch in a git repository.
async fn get_latest_hash(repo_url: String, branch: String) -> Result<String, AppError> {
    tokio::process::Command::new("git")
        .args(["ls-remote", &repo_url, &branch])
        .output()
        .await
        .map_err(AppError::from)
        .and_then(handle_git_output_result)
        .and_then(extract_hash)
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

fn handle_git_output_result(output: std::process::Output) -> Result<Vec<u8>, AppError> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        let msg = format!(
            "Git command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let io_error = std::io::Error::other(msg);
        Err(io_error.into())
    }
}

fn extract_hash(stdout: Vec<u8>) -> Result<String, AppError> {
    String::from_utf8_lossy(&stdout)
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .ok_or(AppError::Process(
            "`git` could not get commit hash".to_string(),
        ))
}

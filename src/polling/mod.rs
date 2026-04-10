//! Asynchronous task to periodically check for updated remote branches.

use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{
    error::AppError,
    events::BranchUpdateEvent,
    polling::{
        db::{gather_updated_branches, update_branches_table},
        error::handle_polling_error,
    },
};

mod branch;
mod db;
mod error;
mod git;

/// Spawns an asynchronous task to periodically poll git branches for updates.
pub fn start_polling_engine(
    pool: SqlitePool,
    token: CancellationToken,
    tx: Sender<BranchUpdateEvent>,
) {
    tokio::spawn(async move {
        info!("Polling engine started");
        polling_loop(pool, token, tx).await;
    });
}

/// Controls whether to shut down the polling engine or run a polling cycle.
async fn polling_loop(pool: SqlitePool, token: CancellationToken, tx: Sender<BranchUpdateEvent>) {
    loop {
        tokio::select! {
            res = poll_branches(&pool, &tx) => {followup_poll(res, &token).await}
            _ = token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down polling engine");
}

/// Orchestrates polling operations.
async fn poll_branches(pool: &SqlitePool, tx: &Sender<BranchUpdateEvent>) -> Result<(), AppError> {
    let updated_branches = gather_updated_branches(pool).await?;
    update_branches_table(pool, updated_branches, tx).await?;

    Ok(())
}

/// Handles polling results and puts the task to sleep.
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

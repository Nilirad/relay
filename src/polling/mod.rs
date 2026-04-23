//! Asynchronous task to periodically check for updated remote branches.

use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::mpsc::{Sender, error::SendError};
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{
    context::SharedContext,
    events::BranchUpdateEvent,
    polling::{
        branch::BranchInfo,
        db::{gather_updated_branches, update_branches_table},
        error::{PollingError, handle_polling_error},
    },
};

mod branch;
mod db;
mod error;
mod git;

/// Spawns an asynchronous task to periodically poll git branches for updates.
pub fn start_polling_engine(ctx: SharedContext, tx: Sender<BranchUpdateEvent>) {
    tokio::spawn(async move {
        info!("Polling engine started");
        polling_loop(ctx, tx).await;
    });
}

/// Controls whether to shut down the polling engine or run a polling cycle.
async fn polling_loop(ctx: SharedContext, tx: Sender<BranchUpdateEvent>) {
    loop {
        tokio::select! {
            res = poll_branches(&ctx.db_pool, &tx) => {followup_poll(res, &ctx.token).await}
            _ = ctx.token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down polling engine");
}

/// Orchestrates polling operations.
async fn poll_branches(
    pool: &SqlitePool,
    tx: &Sender<BranchUpdateEvent>,
) -> Result<(), PollingError> {
    let updated_branches = gather_updated_branches(pool).await?;
    update_branches_table(pool, &updated_branches).await?;
    send_branch_update_events(&updated_branches, tx).await?;

    Ok(())
}

/// Handles polling results and puts the task to sleep.
async fn followup_poll(res: Result<(), PollingError>, token: &CancellationToken) {
    // TODO: Make polling cooldown configurable.
    const SLEEP_SECS: u64 = 5 * 60;

    match res {
        Ok(_) => tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(SLEEP_SECS)) => {}
            _ = token.cancelled() => {}
        },
        Err(e) => handle_polling_error(e, token).await,
    }
}

/// Sends [`BranchUpdateEvent`]s on a Tokio MPSC channel.
async fn send_branch_update_events(
    updated_branches: &[BranchInfo],
    tx: &Sender<BranchUpdateEvent>,
) -> Result<(), SendError<BranchUpdateEvent>> {
    for branch_info in updated_branches {
        info!(
            "New commit detected for branch {}. Hash: {}",
            branch_info.branch.name, branch_info.latest_hash
        );

        let event = BranchUpdateEvent {
            branch_id: branch_info.branch.id,
            new_hash: branch_info.latest_hash.clone(),
        };
        tx.send(event).await?;
    }

    Ok(())
}

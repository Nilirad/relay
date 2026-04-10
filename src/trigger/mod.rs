use sqlx::SqlitePool;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{error::AppError, events::BranchUpdateEvent, model::Subscriber};

mod jwt;

pub fn start_trigger_engine(
    pool: SqlitePool,
    token: CancellationToken,
    mut rx: Receiver<BranchUpdateEvent>,
) {
    tokio::spawn(async move {
        info!("Trigger engine started");
        trigger_loop(pool, token, rx).await;
    });
}

async fn trigger_loop(
    pool: SqlitePool,
    token: CancellationToken,
    mut rx: Receiver<BranchUpdateEvent>,
) {
    loop {
        tokio::select! {
            Some(event) = rx.recv() => handle_branch_update(&pool, event).await,
            _ = token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down trigger engine");
}

async fn handle_branch_update(pool: &SqlitePool, event: BranchUpdateEvent) {
    info!(
        "Received update event for branch {}: {}",
        event.branch_id, event.new_hash
    );

    let subscribers = get_subscribers(pool, event).await;
    // TODO: Generate JWT using `generate_gh_jwt`.
    // TODO: Request the IAT.
    // TODO: Send the `repository_dispatch` event.
}

async fn get_subscribers(
    pool: &SqlitePool,
    event: BranchUpdateEvent,
) -> Result<Vec<Subscriber>, AppError> {
    let subscribers =
        sqlx::query_as::<_, Subscriber>("SELECT * FROM subscribers WHERE branch_id = ?")
            .bind(event.branch_id)
            .fetch_all(pool)
            .await?;

    Ok(subscribers)
}

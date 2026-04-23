//! Asynchronous task to trigger remote repository workflows.

use reqwest::Client;
use sqlx::SqlitePool;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{
    events::BranchUpdateEvent,
    model::Subscriber,
    trigger::{
        auth::{generate_gh_jwt, request_iat},
        error::{RequestError, WorkflowTriggerError},
    },
};

pub use auth::*;

mod auth;
mod error;

/// Spawns an asynchronous task to trigger repository workflows.
///
/// The spawned task will listen to [`BranchUpdateEvent`]s,
/// triggering a workflow for each event it receives.
pub fn start_trigger_engine(
    pool: SqlitePool,
    http_client: Client,
    token: CancellationToken,
    rx: Receiver<BranchUpdateEvent>,
    creds: AuthCredentials,
) {
    tokio::spawn(async move {
        info!("Trigger engine started");
        trigger_loop(pool, http_client, token, rx, creds).await;
    });
}

/// Controls whether to shut down the trigger engine or process a [`BranchUpdateEvent`].
async fn trigger_loop(
    pool: SqlitePool,
    http_client: Client,
    token: CancellationToken,
    mut rx: Receiver<BranchUpdateEvent>,
    creds: AuthCredentials,
) {
    loop {
        tokio::select! {
            Some(event) = rx.recv() => handle_branch_update(&pool, &http_client, event, &creds).await,
            _ = token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down trigger engine");
}

/// Handles a [`BranchUpdateEvent`], handling any possible error.
async fn handle_branch_update(
    pool: &SqlitePool,
    http_client: &Client,
    event: BranchUpdateEvent,
    creds: &AuthCredentials,
) {
    let result = dispatch_events(pool, http_client, event, creds).await;

    if let Err(e) = result {
        error!("{e}");
    }
}

/// Sends a `repository_dispatch` event for each relevant [`Subscriber`].
async fn dispatch_events(
    pool: &SqlitePool,
    http_client: &Client,
    event: BranchUpdateEvent,
    creds: &AuthCredentials,
) -> Result<(), WorkflowTriggerError> {
    info!(
        "Received update event for branch {}: {}",
        event.branch_id, event.new_hash
    );

    let subscribers = get_subscribers(pool, &event).await?;

    let jwt = generate_gh_jwt(creds)?;

    for sub in subscribers {
        let result = notify_subscriber(http_client, &jwt, &event, sub).await;
        if let Err(e) = result {
            error!("{e:?}");
        }
    }

    Ok(())
}

/// Gets all the [`Subscriber`]s subscribed to the [`BranchUpdateEvent`].
async fn get_subscribers(
    pool: &SqlitePool,
    event: &BranchUpdateEvent,
) -> Result<Vec<Subscriber>, WorkflowTriggerError> {
    let subscribers =
        sqlx::query_as::<_, Subscriber>("SELECT * FROM subscribers WHERE branch_id = ?")
            .bind(event.branch_id)
            .fetch_all(pool)
            .await?;

    Ok(subscribers)
}

/// Manages IAT authentication,
/// and sends a `repository_dispatch` event to the specified [`Subscriber`].
async fn notify_subscriber(
    http_client: &Client,
    jwt: &str,
    event: &BranchUpdateEvent,
    sub: Subscriber,
) -> Result<(), WorkflowTriggerError> {
    let iat = request_iat(http_client, jwt, &sub).await?;
    send_repository_dispatch(http_client, &iat, event, &sub).await?;
    Ok(())
}

/// Sends a `repository_dispatch` event to the specified [`Subscriber`].
async fn send_repository_dispatch(
    http_client: &Client,
    iat: &str,
    event: &BranchUpdateEvent,
    sub: &Subscriber,
) -> Result<(), WorkflowTriggerError> {
    let api_url = format!(
        "https://api.github.com/repos/{}/dispatches",
        sub.target_repo
    );

    let payload = serde_json::json!({
        "event_type": sub.event_type,
        "client_payload": {
            "branch_id": event.branch_id.to_string(),
            "new_commit_hash": event.new_hash
       }
    });

    info!("Sending payload to {}: {}", sub.target_repo, payload);

    let response = http_client
        .post(&api_url)
        .bearer_auth(iat)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2026-03-10")
        .json(&payload)
        .send()
        .await?;

    if response.status().is_success() {
        info!(
            "`repository_dispatch` sent to {}: Event: {}",
            sub.target_repo, sub.event_type
        );
        Ok(())
    } else {
        Err(WorkflowTriggerError::Api(RequestError::Response {
            status: response.status(),
            text: response.text().await?,
        }))
    }
}

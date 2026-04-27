//! Asynchronous task to trigger remote repository workflows.

use reqwest::Client;
use sqlx::SqlitePool;
use tokio::sync::mpsc::Receiver;
use tracing::{error, info};

use crate::{
    context::SharedContext,
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
    ctx: SharedContext,
    http_client: Client,
    rx: Receiver<BranchUpdateEvent>,
    creds: AuthCredentials,
) {
    tokio::spawn(async move {
        info!("Trigger engine started");
        trigger_loop(ctx, http_client, rx, creds).await;
    });
}

/// Controls whether to shut down the trigger engine or process a [`BranchUpdateEvent`].
async fn trigger_loop(
    ctx: SharedContext,
    http_client: Client,
    mut rx: Receiver<BranchUpdateEvent>,
    creds: AuthCredentials,
) {
    loop {
        tokio::select! {
            Some(event) = rx.recv() => handle_branch_update(&ctx, &http_client, event, &creds).await,
            _ = ctx.token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down trigger engine");
}

/// Handles a [`BranchUpdateEvent`], handling any possible error.
async fn handle_branch_update(
    ctx: &SharedContext,
    http_client: &Client,
    event: BranchUpdateEvent,
    creds: &AuthCredentials,
) {
    let result = dispatch_events(
        &ctx.db_pool,
        &ctx.github_api_base_url,
        http_client,
        event,
        creds,
    )
    .await;

    if let Err(e) = result {
        error!("{e}");
    }
}

/// Sends a `repository_dispatch` event for each relevant [`Subscriber`].
async fn dispatch_events(
    pool: &SqlitePool,
    api_base_url: &str,
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
        let result = notify_subscriber(http_client, api_base_url, &jwt, &event, sub).await;
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
    api_base_url: &str,
    jwt: &str,
    event: &BranchUpdateEvent,
    sub: Subscriber,
) -> Result<(), WorkflowTriggerError> {
    let iat = request_iat(http_client, api_base_url, jwt, &sub).await?;
    send_repository_dispatch(http_client, api_base_url, &iat, event, &sub).await?;
    Ok(())
}

/// Sends a `repository_dispatch` event to the specified [`Subscriber`].
async fn send_repository_dispatch(
    http_client: &Client,
    api_base_url: &str,
    iat: &str,
    event: &BranchUpdateEvent,
    sub: &Subscriber,
) -> Result<(), WorkflowTriggerError> {
    let api_url = format!("{}/repos/{}/dispatches", api_base_url, sub.target_repo);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::BranchUpdateEvent;
    use crate::trigger::auth::AuthCredentials;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_dispatch_events() {
        let pool = crate::test_utils::create_test_db().await;
        let mock_server = MockServer::start().await;
        let http_client = reqwest::Client::new();

        // Setup mock subscriber in DB
        sqlx::query!(
            "INSERT INTO branches (repo_url, name) VALUES (?, ?)",
            "repo",
            "main"
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query!("INSERT INTO subscribers (branch_id, target_repo, event_type, gh_app_installation_id) VALUES (?, ?, ?, ?)",
                     1, "org/target", "dispatch", 1)
            .execute(&pool)
            .await
            .unwrap();

        // Mock IAT token request
        Mock::given(method("POST"))
            .and(path("/app/installations/1/access_tokens"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"token": "mock-iat-token"})),
            )
            .mount(&mock_server)
            .await;

        // Mock repository dispatch
        Mock::given(method("POST"))
            .and(path("/repos/org/target/dispatches"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        let event = BranchUpdateEvent {
            branch_id: 1,
            new_hash: "new-hash".to_string(),
        };

        let creds = AuthCredentials {
            client_id: "mock-client-id".to_string(),
            pem_path: "nilirad-relay-dev.pem".to_string(),
        };

        let res = dispatch_events(&pool, &mock_server.uri(), &http_client, event, &creds).await;
        assert!(res.is_ok());
    }
}

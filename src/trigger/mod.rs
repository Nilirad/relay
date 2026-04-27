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

/// Runs an asynchronous task
/// that triggers a workflow in a remote repository.
pub struct TriggerEngine {
    /// Shared data for all async engines.
    pub ctx: SharedContext,

    /// HTTP client to make requests to the GitHub API.
    pub http_client: Client,

    /// Receives [`BranchUpdateEvent`]s.
    pub rx: Receiver<BranchUpdateEvent>,

    /// Contains authentication credentials.
    pub creds: AuthCredentials,
}

impl TriggerEngine {
    /// Spawns an asynchronous task to trigger repository workflows.
    ///
    /// The spawned task will listen to [`BranchUpdateEvent`]s,
    /// triggering a workflow for each event it receives.
    pub fn start(self) {
        tokio::spawn(async move {
            info!("Trigger engine started");
            trigger_loop(self).await;
        });
    }
}

/// Controls whether to shut down the trigger engine or process a [`BranchUpdateEvent`].
async fn trigger_loop(mut engine: TriggerEngine) {
    loop {
        tokio::select! {
            Some(event) = engine.rx.recv() => handle_branch_update(&engine, event).await,
            _ = engine.ctx.token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down trigger engine");
}

/// Handles a [`BranchUpdateEvent`], handling any possible error.
async fn handle_branch_update(engine: &TriggerEngine, event: BranchUpdateEvent) {
    let result = dispatch_events(engine, event).await;

    if let Err(e) = result {
        error!("{e}");
    }
}

/// Sends a `repository_dispatch` event for each relevant [`Subscriber`].
async fn dispatch_events(
    engine: &TriggerEngine,
    event: BranchUpdateEvent,
) -> Result<(), WorkflowTriggerError> {
    info!(
        "Received update event for branch {}: {}",
        event.branch_id, event.new_hash
    );

    let subscribers = get_subscribers(&engine.ctx.db_pool, &event).await?;

    let jwt = generate_gh_jwt(&engine.creds)?;

    for sub in subscribers {
        let iat = request_iat(
            &engine.http_client,
            &engine.ctx.github_api_base_url,
            &jwt,
            &sub,
        )
        .await?;
        let result = notify_subscriber(engine, iat, &event, sub).await;
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
    engine: &TriggerEngine,
    iat: String,
    event: &BranchUpdateEvent,
    sub: Subscriber,
) -> Result<(), WorkflowTriggerError> {
    send_repository_dispatch(engine, &iat, event, &sub).await?;
    Ok(())
}

/// Sends a `repository_dispatch` event to the specified [`Subscriber`].
async fn send_repository_dispatch(
    engine: &TriggerEngine,
    iat: &str,
    event: &BranchUpdateEvent,
    sub: &Subscriber,
) -> Result<(), WorkflowTriggerError> {
    let api_url = format!(
        "{}/repos/{}/dispatches",
        engine.ctx.github_api_base_url, sub.target_repo
    );

    let payload = serde_json::json!({
        "event_type": sub.event_type,
        "client_payload": {
            "branch_id": event.branch_id.to_string(),
            "new_commit_hash": event.new_hash
       }
    });

    info!("Sending payload to {}: {}", sub.target_repo, payload);

    let response = engine
        .http_client
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
    use std::sync::Arc;

    use super::*;
    use crate::events::BranchUpdateEvent;
    use crate::test_utils::MockGitFetcher;
    use crate::trigger::auth::AuthCredentials;
    use tokio_util::sync::CancellationToken;
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

        let trigger_engine = TriggerEngine {
            ctx: SharedContext {
                db_pool: pool,
                token: CancellationToken::new(),
                github_api_base_url: mock_server.uri(),
                git_fetcher: Arc::new(MockGitFetcher {
                    hash: "".to_string(),
                }),
            },
            http_client,
            rx: tokio::sync::mpsc::channel::<BranchUpdateEvent>(1).1,
            creds,
        };

        let res = dispatch_events(&trigger_engine, event).await;
        assert!(res.is_ok());
    }
}

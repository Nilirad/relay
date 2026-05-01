//! Asynchronous task to trigger remote repository workflows.

use reqwest::Client;
use sqlx::SqlitePool;
use tracing::{error, info, warn};

use crate::{
    context::SharedContext,
    events::BranchUpdateEvent,
    model::Subscriber,
    trigger::error::{RequestError, WorkflowTriggerError},
};

pub use auth::*;

mod auth;
pub mod error;

/// Runs an asynchronous task
/// that triggers a workflow in a remote repository.
pub struct TriggerEngine {
    /// Shared data for all async engines.
    pub ctx: SharedContext,

    /// HTTP client to make requests to the GitHub API.
    pub http_client: Client,

    /// Authenticates requests to the GitHub API.
    pub authenticator: Box<dyn Authenticator + Send + Sync>,
}

impl TriggerEngine {
    /// Spawns an asynchronous task to trigger repository workflows.
    ///
    /// The spawned task will periodically read the `trigger_queue` table,
    /// triggering a workflow for each event it processes.
    pub fn start(self) {
        tokio::spawn(async move {
            info!("Trigger engine started");
            trigger_loop(self).await;
        });
    }
}

/// Controls whether to shut down the trigger engine or process a queued event.
async fn trigger_loop(engine: TriggerEngine) {
    const QUEUE_POLLING_INTERVAL_SECS: u64 = 5;
    loop {
        tokio::select! {
            _ = engine.ctx.token.cancelled() => break,
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(QUEUE_POLLING_INTERVAL_SECS)) => {
                if let Err(e) = process_queue(&engine).await {
                    warn!("Error processing queue: {e}");
                }
            }
        }
    }
    info!("Gracefully shutting down trigger engine");
}

/// Processes a single queued event.
async fn process_queue(engine: &TriggerEngine) -> Result<(), WorkflowTriggerError> {
    let oldest_pending_trigger = sqlx::query!(
        "SELECT id, event_payload, retry_count FROM trigger_queue
         WHERE status IN ('PENDING') AND next_retry_at <= CURRENT_TIMESTAMP
         ORDER BY next_retry_at ASC LIMIT 1"
    )
    .fetch_optional(&engine.ctx.db_pool)
    .await?;

    let Some(oldest_pending_trigger) = oldest_pending_trigger else {
        return Ok(());
    };

    // Mark PROCESSING
    sqlx::query!(
        "UPDATE trigger_queue SET status = 'PROCESSING' WHERE id = ?",
        oldest_pending_trigger.id
    )
    .execute(&engine.ctx.db_pool)
    .await?;

    // Parse payload
    let event: BranchUpdateEvent = serde_json::from_str(&oldest_pending_trigger.event_payload)
        .map_err(|e| {
            error!(
                "Failed to deserialize payload for {}: {}",
                oldest_pending_trigger.id, e
            );
            WorkflowTriggerError::Api(RequestError::Response {
                status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                text: e.to_string(),
            })
        })?;

    // Do work
    let dispatch_result = dispatch_events(engine, &event).await;

    // Handle result
    match dispatch_result {
        Ok(_) => {
            sqlx::query!(
                "DELETE FROM trigger_queue WHERE id = ?",
                oldest_pending_trigger.id
            )
            .execute(&engine.ctx.db_pool)
            .await?;
        }
        Err(e) => {
            let next_retry_count = oldest_pending_trigger.retry_count + 1;
            if next_retry_count >= 10 {
                tracing::warn!(
                    "Task {} failed after 10 attempts: {e}",
                    oldest_pending_trigger.id
                );
                sqlx::query!(
                    "UPDATE trigger_queue SET status = 'FAILED', retry_count = ? WHERE id = ?",
                    next_retry_count,
                    oldest_pending_trigger.id
                )
                .execute(&engine.ctx.db_pool)
                .await?;
            } else {
                // Backoff: 10s, 20s, 40s, 80s, 120s, 240s, 480s, ...
                let backoff_secs = match next_retry_count {
                    1 => 10,
                    2 => 20,
                    3 => 40,
                    4 => 80,
                    5 => 120,
                    6 => 240,
                    7 => 480,
                    8 => 960,
                    9 => 1920,
                    _ => 3600,
                };
                sqlx::query!("UPDATE trigger_queue SET status = 'PENDING', retry_count = ?, next_retry_at = datetime('now', ? || ' seconds') WHERE id = ?",
                    next_retry_count, backoff_secs, oldest_pending_trigger.id).execute(&engine.ctx.db_pool).await?;
            }
        }
    }

    Ok(())
}

/// Sends a `repository_dispatch` event for each relevant [`Subscriber`].
async fn dispatch_events(
    engine: &TriggerEngine,
    event: &BranchUpdateEvent,
) -> Result<(), WorkflowTriggerError> {
    info!(
        "Received update event for branch {}: {}",
        event.branch_id, event.new_hash
    );

    let subscribers = get_subscribers(&engine.ctx.db_pool, event).await?;

    for sub in subscribers {
        let iat = engine
            .authenticator
            .request_installation_token(&sub)
            .await?;
        let result = notify_subscriber(engine, iat, event, sub).await;
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
    use crate::test_utils::{MockAuthenticator, MockGitFetcher};

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
            authenticator: Box::new(MockAuthenticator {
                iat: "Test IAT".to_string(),
            }),
        };

        let res = dispatch_events(&trigger_engine, &event).await;
        assert!(res.is_ok());
    }
}

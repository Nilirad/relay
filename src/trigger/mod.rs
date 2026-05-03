//! Asynchronous task to trigger remote repository workflows.

use reqwest::Client;
use sqlx::SqlitePool;
use tracing::{error, info, warn};

use crate::{
    context::SharedContext,
    events::BranchUpdateEvent,
    model::{Subscriber, TriggerQueueItem},
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
    let Some(trigger) = get_oldest_queued_trigger(&engine.ctx.db_pool).await? else {
        return Ok(());
    };

    let event: BranchUpdateEvent = match serde_json::from_str(&trigger.event_payload) {
        Ok(event) => event,
        Err(e) => {
            error!("Failed to deserialize payload for {}: {}", trigger.id, e);
            mark_task_as_failed(&engine.ctx.db_pool, trigger.id).await?;
            return Err(WorkflowTriggerError::Api(RequestError::Response {
                status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                text: e.to_string(),
            }));
        }
    };

    let dispatch_result = dispatch_events(engine, &event).await;
    match dispatch_result {
        Ok(_) => {
            delete_trigger_from_queue(engine, &trigger).await?;
        }
        Err(e) => {
            schedule_retry(engine, trigger, e).await?;
        }
    }

    Ok(())
}

/// Returns the oldest `PENDING` trigger in the `trigger_queue` table.
async fn get_oldest_queued_trigger(
    pool: &SqlitePool,
) -> Result<Option<TriggerQueueItem>, sqlx::Error> {
    let trigger = sqlx::query_as::<_, TriggerQueueItem>(
        "SELECT id, event_payload, status, retry_count, next_retry_at, created_at, status_updated_at FROM trigger_queue
         WHERE status IN ('PENDING') AND next_retry_at <= CURRENT_TIMESTAMP
         ORDER BY next_retry_at ASC LIMIT 1"
    )
    .fetch_optional(pool)
    .await?;

    let Some(trigger) = trigger else {
        return Ok(None);
    };

    sqlx::query!(
        "UPDATE trigger_queue SET status = 'PROCESSING', status_updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        trigger.id
    )
    .execute(pool)
    .await?;

    Ok(Some(trigger))
}

/// Deletes a successful trigger from the `trigger_queue`.
async fn delete_trigger_from_queue(
    engine: &TriggerEngine,
    trigger: &TriggerQueueItem,
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM trigger_queue WHERE id = ?", trigger.id)
        .execute(&engine.ctx.db_pool)
        .await?;
    Ok(())
}

/// Marks a trigger as `FAILED` in the `trigger_queue`.
async fn mark_task_as_failed(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE trigger_queue SET status = 'FAILED' WHERE id = ?",
        id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Schedules the next retry for a trigger in the `trigger_queue`.
async fn schedule_retry(
    engine: &TriggerEngine,
    trigger: TriggerQueueItem,
    e: WorkflowTriggerError,
) -> Result<(), WorkflowTriggerError> {
    let next_retry_count = trigger.retry_count + 1;

    if next_retry_count >= 10 {
        tracing::warn!("Task {} failed after 10 attempts: {e}", trigger.id);
        sqlx::query!(
            "UPDATE trigger_queue SET status = 'FAILED', retry_count = ? WHERE id = ?",
            next_retry_count,
            trigger.id
        )
        .execute(&engine.ctx.db_pool)
        .await?;
    } else {
        let backoff_secs = 10 * (1 << (next_retry_count - 1));
        sqlx::query!("UPDATE trigger_queue SET status = 'PENDING', retry_count = ?, next_retry_at = datetime('now', ? || ' seconds') WHERE id = ?",
            next_retry_count, backoff_secs, trigger.id).execute(&engine.ctx.db_pool).await?;
    }

    Ok(())
}

/// Recovers tasks that have been stuck in `PROCESSING` for too long.
pub async fn recover_stuck_tasks(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE trigger_queue
         SET status = 'PENDING', status_updated_at = CURRENT_TIMESTAMP
         WHERE status = 'PROCESSING'
           AND status_updated_at < DATETIME('now', '-5 minutes')"
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Sends a `repository_dispatch` event for each relevant [`Subscriber`].
pub async fn dispatch_events(
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
        notify_subscriber(engine, iat, event, sub).await?;
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
    use super::*;
    use crate::test_utils::{MockAuthenticator, MockGitFetcher};
    use std::sync::Arc;

    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_recover_stuck_tasks() {
        let pool = crate::test_utils::create_test_db().await;

        // Insert tasks
        // 1. Processing (stuck)
        sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, status_updated_at) VALUES (?, ?, ?, DATETIME('now', '-10 minutes'))",
            "{}", "PROCESSING", 0).execute(&pool).await.unwrap();
        // 2. Processing (recent)
        sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, status_updated_at) VALUES (?, ?, ?, DATETIME('now', '-1 minute'))",
            "{}", "PROCESSING", 0).execute(&pool).await.unwrap();
        // 3. Pending
        sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, status_updated_at) VALUES (?, ?, ?, DATETIME('now'))",
            "{}", "PENDING", 0).execute(&pool).await.unwrap();

        recover_stuck_tasks(&pool).await.unwrap();

        // Check status
        let tasks = sqlx::query!("SELECT status FROM trigger_queue ORDER BY rowid")
            .fetch_all(&pool)
            .await
            .unwrap();

        assert_eq!(tasks[0].status, "PENDING"); // was stuck
        assert_eq!(tasks[1].status, "PROCESSING"); // was recent
        assert_eq!(tasks[2].status, "PENDING"); // was pending
    }

    #[tokio::test]
    async fn test_get_oldest_queued_trigger() {
        let pool = crate::test_utils::create_test_db().await;

        // Insert some dummy items
        sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, next_retry_at) VALUES (?, ?, ?, datetime('now', '-1 minute'))",
            "{}", "PENDING", 0).execute(&pool).await.unwrap();
        sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, next_retry_at) VALUES (?, ?, ?, datetime('now', '-5 minutes'))",
            "{}", "PENDING", 0).execute(&pool).await.unwrap();
        sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, next_retry_at) VALUES (?, ?, ?, datetime('now', '+1 minute'))",
            "{}", "PENDING", 0).execute(&pool).await.unwrap();

        let trigger = get_oldest_queued_trigger(&pool).await.unwrap().unwrap();

        // Assert: The one with -5 minutes should be returned
        assert_eq!(trigger.retry_count, 0);

        // Verify it was updated to PROCESSING
        let db_trigger = sqlx::query!("SELECT status FROM trigger_queue WHERE id = ?", trigger.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(db_trigger.status, "PROCESSING");
    }

    #[tokio::test]
    async fn test_schedule_retry() {
        let pool = crate::test_utils::create_test_db().await;
        let id = sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, next_retry_at) VALUES (?, ?, ?, datetime('now'))",
            "{}", "PROCESSING", 0).execute(&pool).await.unwrap().last_insert_rowid();

        let trigger = TriggerQueueItem {
            id,
            event_payload: "{}".to_string(),
            retry_count: 0,
        };

        let engine = TriggerEngine {
            ctx: SharedContext {
                db_pool: pool.clone(),
                token: CancellationToken::new(),
                github_api_base_url: "http://mock".to_string(),
                git_fetcher: Arc::new(MockGitFetcher {
                    hash: "".to_string(),
                }),
            },
            http_client: reqwest::Client::new(),
            authenticator: Box::new(MockAuthenticator {
                iat: "token".to_string(),
            }),
        };

        schedule_retry(
            &engine,
            trigger,
            WorkflowTriggerError::Api(RequestError::Response {
                status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                text: "error".to_string(),
            }),
        )
        .await
        .unwrap();

        let updated = sqlx::query!(
            "SELECT status, retry_count FROM trigger_queue WHERE id = ?",
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(updated.status, "PENDING");
        assert_eq!(updated.retry_count, 1);
    }

    #[tokio::test]
    async fn test_process_queue_failure_and_retry() {
        let pool = crate::test_utils::create_test_db().await;
        let mock_server = MockServer::start().await;

        // Setup subscriber
        sqlx::query!(
            "INSERT INTO branches (repo_url, name) VALUES (?, ?)",
            "repo",
            "main"
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query!("INSERT INTO subscribers (branch_id, target_repo, event_type, gh_app_installation_id) VALUES (?, ?, ?, ?)",
                     1, "org/target", "dispatch", 1).execute(&pool).await.unwrap();

        // Mock token success, but dispatch failure
        Mock::given(method("POST"))
            .and(path("/app/installations/1/access_tokens"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "token"})),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/org/target/dispatches"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let payload = serde_json::json!({"branch_id": 1, "new_hash": "hash"}).to_string();
        sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, next_retry_at) VALUES (?, ?, ?, '2000-01-01 00:00:00')",
            payload, "PENDING", 0).execute(&pool).await.unwrap();

        let engine = TriggerEngine {
            ctx: SharedContext {
                db_pool: pool.clone(),
                token: CancellationToken::new(),
                github_api_base_url: mock_server.uri(),
                git_fetcher: Arc::new(MockGitFetcher {
                    hash: "".to_string(),
                }),
            },
            http_client: reqwest::Client::new(),
            authenticator: Box::new(MockAuthenticator {
                iat: "token".to_string(),
            }),
        };

        process_queue(&engine).await.unwrap();

        // Should still exist and retry_count increased
        let trigger = sqlx::query!("SELECT retry_count, status FROM trigger_queue")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(trigger.retry_count, 1);
        assert_eq!(trigger.status, "PENDING");
    }

    #[tokio::test]
    async fn test_process_queue_deserialization_failure() {
        let pool = crate::test_utils::create_test_db().await;
        let mock_server = MockServer::start().await;

        // Insert invalid JSON payload
        sqlx::query!("INSERT INTO trigger_queue (event_payload, status, retry_count, next_retry_at) VALUES (?, ?, ?, '2000-01-01 00:00:00')",
            "invalid json", "PENDING", 0).execute(&pool).await.unwrap();

        let engine = TriggerEngine {
            ctx: SharedContext {
                db_pool: pool.clone(),
                token: CancellationToken::new(),
                github_api_base_url: mock_server.uri(),
                git_fetcher: Arc::new(MockGitFetcher {
                    hash: "".to_string(),
                }),
            },
            http_client: reqwest::Client::new(),
            authenticator: Box::new(MockAuthenticator {
                iat: "token".to_string(),
            }),
        };

        // Should return error
        let result = process_queue(&engine).await;
        assert!(result.is_err());

        // Should be marked as FAILED
        let trigger = sqlx::query!("SELECT status FROM trigger_queue")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(trigger.status, "FAILED");
    }
}

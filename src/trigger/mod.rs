use std::env;

use reqwest::Client;
use sqlx::SqlitePool;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{
    error::AppError, events::BranchUpdateEvent, model::Subscriber, trigger::jwt::generate_gh_jwt,
};

mod jwt;

pub fn start_trigger_engine(
    pool: SqlitePool,
    token: CancellationToken,
    rx: Receiver<BranchUpdateEvent>,
) {
    let http_client = build_http_client();
    tokio::spawn(async move {
        info!("Trigger engine started");
        trigger_loop(pool, http_client, token, rx).await;
    });
}

async fn trigger_loop(
    pool: SqlitePool,
    http_client: Client,
    token: CancellationToken,
    mut rx: Receiver<BranchUpdateEvent>,
) {
    loop {
        tokio::select! {
            Some(event) = rx.recv() => handle_branch_update(&pool, &http_client, event).await,
            _ = token.cancelled() => break,
        }
    }
    info!("Gracefully shutting down trigger engine");
}

async fn handle_branch_update(pool: &SqlitePool, http_client: &Client, event: BranchUpdateEvent) {
    let result = dispatch_events(pool, http_client, event).await;

    if let Err(e) = result {
        error!("{e}");
    }
}

async fn dispatch_events(
    pool: &SqlitePool,
    http_client: &Client,
    event: BranchUpdateEvent,
) -> Result<(), AppError> {
    info!(
        "Received update event for branch {}: {}",
        event.branch_id, event.new_hash
    );

    let subscribers = get_subscribers(pool, &event).await?;

    let client_id =
        env::var("GH_CLIENT_ID").expect("Environment variable `GH_CLIENT_ID` must be set");
    let pem_path =
        env::var("GH_APP_KEY_PATH").expect("Environment variable `GH_APP_KEY_PATH` must be set");
    let jwt = generate_gh_jwt(&client_id, &pem_path)?;

    for sub in subscribers {
        let result = notify_subscriber(http_client, &jwt, &event, sub).await;
        if let Err(e) = result {
            error!("{e:?}");
        }
    }

    Ok(())
}

async fn get_subscribers(
    pool: &SqlitePool,
    event: &BranchUpdateEvent,
) -> Result<Vec<Subscriber>, AppError> {
    let subscribers =
        sqlx::query_as::<_, Subscriber>("SELECT * FROM subscribers WHERE branch_id = ?")
            .bind(event.branch_id)
            .fetch_all(pool)
            .await?;

    Ok(subscribers)
}

async fn notify_subscriber(
    http_client: &Client,
    jwt: &str,
    event: &BranchUpdateEvent,
    sub: Subscriber,
) -> Result<(), AppError> {
    let iat = request_iat(http_client, jwt, &sub).await?;
    send_repository_dispatch(http_client, &iat, event, &sub).await?;
    Ok(())
}

async fn request_iat(
    http_client: &Client,
    jwt: &str,
    sub: &Subscriber,
) -> Result<String, AppError> {
    #[derive(serde::Deserialize)]
    struct IatResponse {
        token: String,
    }

    let api_url = format!(
        "https://api.github.com/app/installations/{}/access_tokens",
        sub.gh_app_installation_id
    );
    let response = http_client
        .post(&api_url)
        .bearer_auth(jwt)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2026-03-10")
        .send()
        .await?;

    if response.status().is_success() {
        let response_json = response.json::<IatResponse>().await?;
        info!("IAT received for subscriber {}", sub.target_repo);
        Ok(response_json.token)
    } else {
        Err(AppError::Response(format!(
            "Unexpected status {}: {}",
            response.status(),
            response.text().await?
        )))
    }
}

async fn send_repository_dispatch(
    http_client: &Client,
    iat: &str,
    event: &BranchUpdateEvent,
    sub: &Subscriber,
) -> Result<(), AppError> {
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
        Err(AppError::Response(format!(
            "Unexpected status {}: {}",
            response.status(),
            response.text().await?
        )))
    }
}

fn build_http_client() -> Client {
    const USER_AGENT: &str = "nilirad-relay-server";

    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("Failed to build HTTP client")
}

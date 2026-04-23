#![doc = include_str!("../README.md")]
#![warn(missing_docs, clippy::missing_docs_in_private_items)]
#![warn(
    clippy::panic,
    clippy::expect_used,
    clippy::todo,
    clippy::unimplemented,
    clippy::indexing_slicing
)]

use axum::{
    Router,
    routing::{get, post},
};
use reqwest::Client;
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::{
    error::{ClientCreationError, FatalError},
    events::BranchUpdateEvent,
    handler::{create_branch, create_subscriber},
    state::AppState,
    trigger::get_auth_credentials,
};

mod error;
mod events;
mod handler;
mod model;
mod polling;
mod state;
mod trigger;

#[tokio::main]
async fn main() {
    run_app().await.unwrap_or_else(|e| error!("{e}"));
}

/// Runs the server, delegating errors to the caller.
async fn run_app() -> Result<(), FatalError> {
    const BRANCH_UPDATE_EVENT_BUFFER_SIZE: usize = 64;

    tracing_subscriber::fmt::init();

    let pool = sqlx::SqlitePool::connect("sqlite://relay.db?mode=rwc").await?;
    let state = AppState {
        db_pool: pool.clone(),
    };

    let http_client = build_http_client()?;
    let creds = get_auth_credentials()?;

    let token = CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::channel::<BranchUpdateEvent>(BRANCH_UPDATE_EVENT_BUFFER_SIZE);
    polling::start_polling_engine(pool.clone(), token.clone(), tx);
    trigger::start_trigger_engine(pool, http_client, token.clone(), rx, creds);

    let app = Router::new()
        .route("/health", get(|| async { "Relay Server is alive" }))
        .route("/branches", post(create_branch))
        .route("/subscribers", post(create_subscriber))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .map_err(FatalError::TcpBinding)?;
    println!("Server listening on http://0.0.0.0:3000");
    axum::serve(listener, app)
        .await
        .map_err(FatalError::Serve)?;

    token.cancel();

    Ok(())
}

/// Creates a new HTTP client.
pub fn build_http_client() -> Result<Client, ClientCreationError> {
    const USER_AGENT: &str = "nilirad-relay-server";

    let client = Client::builder().user_agent(USER_AGENT).build()?;

    Ok(client)
}

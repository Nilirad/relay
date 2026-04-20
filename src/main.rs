#![doc = include_str!("../README.md")]
#![warn(missing_docs, clippy::missing_docs_in_private_items)]

use axum::{
    Router,
    routing::{get, post},
};
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::{
    error::AppError,
    events::BranchUpdateEvent,
    handler::{create_branch, create_subscriber},
    state::AppState,
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
async fn run_app() -> Result<(), AppError> {
    const BRANCH_UPDATE_EVENT_BUFFER_SIZE: usize = 64;

    tracing_subscriber::fmt::init();

    let pool = sqlx::SqlitePool::connect("sqlite://relay.db?mode=rwc").await?;
    let state = AppState {
        db_pool: pool.clone(),
    };

    let token = CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::channel::<BranchUpdateEvent>(BRANCH_UPDATE_EVENT_BUFFER_SIZE);
    polling::start_polling_engine(pool.clone(), token.clone(), tx);
    trigger::start_trigger_engine(pool, token.clone(), rx);

    let app = Router::new()
        .route("/health", get(|| async { "Relay Server is alive" }))
        .route("/branches", post(create_branch))
        .route("/subscribers", post(create_subscriber))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await?;

    token.cancel();

    Ok(())
}

use axum::{routing::{get, post}, Router};
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::{error::AppError, state::AppState, handler::create_branch};

mod error;
mod model;
mod polling;
mod state;
mod handler;

#[tokio::main]
async fn main() {
    run_app().await.unwrap_or_else(handle_app_error);
}

async fn run_app() -> Result<(), AppError> {
    tracing_subscriber::fmt::init();

    let pool = sqlx::SqlitePool::connect("sqlite://relay.db?mode=rwc").await?;
    let state = AppState {
        db_pool: pool.clone(),
    };

    let token = CancellationToken::new();
    polling::start_polling_engine(pool, token.clone());

    let app = Router::new()
        .route("/health", get(|| async { "Relay Server is alive" }))
        .route("/branches", post(create_branch))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await?;

    token.cancel();

    Ok(())
}

fn handle_app_error(app_error: AppError) {
    match app_error {
        AppError::Io(e) => {
            error!("{e}")
        }
        AppError::Sqlx(e) => {
            error!("{e}")
        }
        AppError::Process(e) => {
            error!("{e}")
        }
    }
}

use axum::{Router, routing::get};
use tracing::error;

use crate::{error::AppError, state::AppState};

mod error;
mod state;

#[tokio::main]
async fn main() {
    run_app().await.unwrap_or_else(handle_app_error);
}

async fn run_app() -> Result<(), AppError> {
    tracing_subscriber::fmt::init();

    let pool = sqlx::SqlitePool::connect("sqlite://relay.db?mode=rwc").await?;
    let state = AppState { db_pool: pool };

    let app = Router::new()
        .route("/health", get(|| async { "Relay Server is alive" }))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await?;

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
    }
}

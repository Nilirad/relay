//! Error type definitions and implementations.

use std::time::SystemTimeError;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::events::BranchUpdateEvent;

/// Consolidates every application error into a single type.
#[derive(Debug, Error)]
pub enum AppError {
    /// Error encountered during an I/O operation.
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    /// Error occurred while executing database query or connection.
    #[error("SQLx Error: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// Failure retrieving or computing system time.
    #[error("System Time Error: {0}")]
    SystemTime(#[from] SystemTimeError),

    /// Error during JWT token generation or validation.
    #[error("JWT Error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    /// Failed to send update event to the trigger engine channel.
    #[error("Channel send error: {0}")]
    ChannelSend(#[from] SendError<BranchUpdateEvent>),

    /// Error encountered during HTTP request to GitHub API.
    #[error("Client error: {0}")]
    Client(#[from] reqwest::Error),

    /// A spawned process returned an error or an unsuccessful result.
    #[error("Process failed: {0}")]
    Process(String),

    /// A failing HTTP response.
    #[error("Response error: {0}")]
    Response(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
    }
}

/// An error that requires the server to be shut down.
#[derive(Debug, Error)]
pub enum FatalError {
    /// Database is down or URL is incorrect.
    #[error("Database connection: {0}")]
    DbConnection(#[from] sqlx::Error),

    /// Could not reserve an IP address with a TCP port
    /// to connect to the server.
    #[error("TCP binding: {0}")]
    TcpBinding(#[source] std::io::Error),

    /// I/O error during server's execution loop.
    #[error("Serve: {0}")]
    Serve(#[source] std::io::Error),

    // Docs deferred to inner type.
    #[allow(clippy::missing_docs_in_private_items)]
    #[error("HTTP Client creation: {0}")]
    ClientCreation(#[from] ClientCreationError),
}

/// HTTP Client creation failed.
///
/// The server cannot trigger workflows.
#[derive(Debug, Error)]
#[error("HTTP Client creation: {0}")]
pub struct ClientCreationError(#[from] reqwest::Error);

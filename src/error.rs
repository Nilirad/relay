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
#[error(transparent)]
pub struct ClientCreationError(#[from] reqwest::Error);

/// Error in retrieving a commit or its info using `git ls-remote`.
#[derive(Debug, Error)]
pub enum CommitHashError {
    /// I/O error while spawning the process.
    #[error("I/O error in `git ls-remote`: {0}")]
    Io(#[from] std::io::Error),

    /// Unexpected exit status.
    #[error("Unexpected `git ls-remote` exit status: {0}")]
    UnexpectedStatus(String),

    /// Unexpected output format.
    #[error(
        "Unexpected `git ls-remote` output format. Repo: {repo_url}; Branch: {branch}; Stdout: {stdout}"
    )]
    UnexpectedOutput {
        /// The process output text.
        stdout: String,
        /// The relevant git repository URL.
        repo_url: String,
        /// The relevant git branch.
        branch: String,
    },
}

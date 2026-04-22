//! Error type definitions and implementations.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

/// Consolidates every application error into a single type.
#[derive(Debug, Error)]
pub enum AppError {
    /// Error occurred while executing database query or connection.
    #[error("SQLx Error: {0}")]
    Sqlx(#[from] sqlx::Error),
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

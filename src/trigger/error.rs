//! Error type definitions.

use thiserror::Error;

/// An error that interrupted a workflow trigger loop iteration.
#[derive(Debug, Error)]
pub enum WorkflowTriggerError {
    /// Error during JWT or IAT authentication.
    #[error("Authentication error: {0}")]
    Auth(#[from] AuthError),

    /// Database error.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// API service error.
    #[error(transparent)]
    Api(#[from] RequestError),
}

impl From<reqwest::Error> for WorkflowTriggerError {
    fn from(e: reqwest::Error) -> Self {
        Self::Api(RequestError::Request(e))
    }
}

/// Authentication errors.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Failed to read PEM file.
    #[error("Could not read PEM file: {0}")]
    PemFile(#[from] std::io::Error),

    /// An error with time APIs.
    #[error("System time: {0}")]
    Time(#[from] std::time::SystemTimeError),

    /// Error while generating a JWT.
    #[error("JWT generation failed: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    /// Authentication server error.
    #[error(transparent)]
    Server(#[from] RequestError),
}

impl From<reqwest::Error> for AuthError {
    fn from(e: reqwest::Error) -> Self {
        Self::Server(RequestError::Request(e))
    }
}

/// An error while performing an HTTP request.
#[derive(Debug, Error)]
pub enum RequestError {
    /// Missing response from API service.
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    /// Bad response from API service.
    #[error("Unexpected HTTP response ({status}): {text}")]
    Response {
        /// The HTTP status code of the response.
        status: reqwest::StatusCode,
        /// The full text of the HTTP response.
        text: String,
    },
}

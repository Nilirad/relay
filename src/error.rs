use std::time::SystemTimeError;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::events::BranchUpdateEvent;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLx Error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("Process failed: {0}")]
    Process(String),
    #[error("System Time Error: {0}")]
    SystemTime(#[from] SystemTimeError),
    #[error("JWT Error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("Channel send error: {0}")]
    ChannelSend(#[from] SendError<BranchUpdateEvent>),
    #[error("Client error: {0}")]
    Client(#[from] reqwest::Error),
    #[error("Response error: {0}")]
    Response(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
    }
}

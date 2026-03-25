use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
}

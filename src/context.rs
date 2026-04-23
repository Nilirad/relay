//! Shared context for background engines.

use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

/// Shared dependencies across background engines.
#[derive(Clone, Debug)]
pub struct SharedContext {
    /// SQLx connection pool for the SQLite database.
    pub db_pool: SqlitePool,
    /// Token to signal task cancellation.
    pub token: CancellationToken,
}

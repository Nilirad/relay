//! Shared context for background engines.

use crate::polling::git::GitFetcher;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Shared dependencies across background engines.
#[derive(Clone)]
pub struct SharedContext {
    /// SQLx connection pool for the SQLite database.
    pub db_pool: SqlitePool,
    /// Token to signal task cancellation.
    pub token: CancellationToken,
    /// Base URL for the GitHub API.
    pub github_api_base_url: String,
    /// Git fetcher for polling.
    pub git_fetcher: Arc<dyn GitFetcher>,
}

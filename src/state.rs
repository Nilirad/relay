//! State of the application.

/// Holds data accessible from each [handler].
///
/// <!-- LINKS -->
/// [handler]: crate::handler
#[derive(Debug, Clone)]
pub struct AppState {
    /// SQLx connection pool for the SQLite database.
    pub db_pool: sqlx::SqlitePool,
}

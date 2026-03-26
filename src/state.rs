#[derive(Debug, Clone)]
pub struct AppState {
    pub db_pool: sqlx::SqlitePool,
}

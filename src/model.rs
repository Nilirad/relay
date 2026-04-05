use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Branch {
    pub id: i64,
    pub repo_url: String,
    pub name: String,
    pub last_commit_hash: Option<String>,
    pub polling_interval_secs: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Subscriber {
    pub id: i64,
    pub branch_id: i64,
    pub target_repo: String,
    pub event_type: String,
    pub gh_app_installation_id: i64,
    pub created_at: String,
    pub updated_at: String,
}

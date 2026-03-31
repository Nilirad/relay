use sqlx::SqlitePool;
use tracing::{error, info};

use crate::polling::BranchInfo;

pub(super) async fn update_branches_table(pool: &SqlitePool, updates_to_process: Vec<BranchInfo>) {
    for BranchInfo {
        branch,
        latest_hash,
    } in updates_to_process
    {
        info!(
            "New commit detected for branch {}. Hash: {}",
            branch.name, latest_hash
        );

        let query_result = sqlx::query!(
            "UPDATE branches SET last_commit_hash = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            latest_hash,
            branch.id
        )
        .execute(pool)
        .await;

        if let Err(e) = query_result {
            error!("SQLx error: {e}");
        }
    }
}

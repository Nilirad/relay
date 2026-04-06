use crate::error::AppError;
use crate::model::{Branch, CreateBranch};
use crate::state::AppState;
use axum::{Json, extract::State};
use tracing::info;

pub async fn create_branch(
    State(state): State<AppState>,
    Json(payload): Json<CreateBranch>,
) -> Result<Json<Branch>, AppError> {
    let branch = sqlx::query_as::<_, Branch>(
        "INSERT INTO branches (repo_url, name, polling_interval_secs) VALUES (?, ?, ?) RETURNING *",
    )
    .bind(&payload.repo_url)
    .bind(&payload.name)
    .bind(payload.polling_interval_secs)
    .fetch_one(&state.db_pool)
    .await?;

    info!("Tracked new git branch: {:?}", branch);

    Ok(Json(branch))
}

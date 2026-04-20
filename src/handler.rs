//! Axum route handlers.

use crate::error::AppError;
use crate::model::{Branch, CreateBranch, CreateSubscriber, Subscriber};
use crate::state::AppState;
use axum::{Json, extract::State};
use tracing::info;

/// Stores a new [`Branch`] in the database.
pub async fn create_branch(
    State(state): State<AppState>,
    Json(payload): Json<CreateBranch>,
) -> Result<Json<Branch>, AppError> {
    let branch = sqlx::query_as::<_, Branch>(
        "INSERT INTO branches (repo_url, name) VALUES (?, ?) RETURNING *",
    )
    .bind(&payload.repo_url)
    .bind(&payload.name)
    .fetch_one(&state.db_pool)
    .await?;

    info!("Tracked new git branch: {:?}", branch);

    Ok(Json(branch))
}

/// Stores a new [`Subscriber`] in the database.
pub async fn create_subscriber(
    State(state): State<AppState>,
    Json(payload): Json<CreateSubscriber>,
) -> Result<Json<Subscriber>, AppError> {
    let mut transaction = state.db_pool.begin().await?;
    let branch_id = get_or_insert_branch_id(&mut transaction, &payload).await?;
    let subscriber = sqlx::query_as::<_, Subscriber>(
        "INSERT INTO subscribers (branch_id, target_repo, event_type, gh_app_installation_id) VALUES (?, ?, ?, ?) RETURNING *"
    )
    .bind(branch_id)
    .bind(&payload.target_repo)
    .bind(&payload.event_type)
    .bind(payload.gh_app_installation_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;

    info!("Registered new subscriber for branch ID {branch_id}: {subscriber:?}");

    Ok(Json(subscriber))
}

/// Gets the branch ID specified in the [`CreateSubscriber`] payload.
///
/// If the branch doesn't exist, it is created and its ID is returned.
async fn get_or_insert_branch_id(
    transaction: &mut sqlx::SqliteConnection,
    payload: &CreateSubscriber,
) -> Result<i64, AppError> {
    let branch_id_opt =
        sqlx::query_scalar::<_, i64>("SELECT id FROM branches WHERE repo_url = ? AND name = ?")
            .bind(&payload.source_repo_url)
            .bind(&payload.source_branch_name)
            .fetch_optional(&mut *transaction)
            .await?;

    if let Some(id) = branch_id_opt {
        return Ok(id);
    }

    sqlx::query_scalar::<_, i64>(
        "INSERT INTO branches (repo_url, name) VALUES (?, ?) RETURNING id"
    )
    .bind(&payload.source_repo_url)
    .bind(&payload.source_branch_name)
    .fetch_one(&mut *transaction)
    .await
    .map_err(Into::into)
}

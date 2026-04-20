//! Data structures representing items stored in database.
//!
//! The `Create_` `struct`s represent the payload
//! to create the corresponding row.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Represents a row in the `branches` table.
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Branch {
    /// Unique database primary key.
    pub id: i64,

    /// Full HTTPS URL of the monitored git repository.
    pub repo_url: String,

    /// Name of the git branch to poll.
    pub name: String,

    /// SHA of the latest commit polled.
    ///
    /// `None` if the branch has not been processed.
    pub last_commit_hash: Option<String>,

    /// Has no effect.
    pub polling_interval_secs: i32,

    /// Timestamp when the record was created, in standard SQL `DATETIME` format (`YYYY-MM-DD HH:MM:SS`).
    pub created_at: String,

    /// Timestamp when the record was updated, in standard SQL `DATETIME` format (`YYYY-MM-DD HH:MM:SS`).
    pub updated_at: String,
}

/// Holds payload data for the creation of a [`Branch`].
#[derive(Debug, Deserialize)]
pub struct CreateBranch {
    /// Determines the value of [`Branch::repo_url`].
    pub repo_url: String,

    /// Determines the value of [`Branch::name`].
    pub name: String,

    /// Determines the value of [`Branch::polling_interval_secs`].
    pub polling_interval_secs: i32,
}

/// Represents a row in the `subscribers` table.
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Subscriber {
    /// Unique database primary key.
    pub id: i64,

    /// Foreign key to [`Branch::id`].
    pub branch_id: i64,

    /// The repository whose workflow needs to be triggered.
    pub target_repo: String,

    /// Identifies the specific [`repository_dispatch`] event.
    ///
    /// The values must contain at most 100 characters.
    ///
    /// <!-- LINKS -->
    /// [`repository_dispatch`]: https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#repository_dispatch
    pub event_type: String,

    /// Allows authenticating as a [GitHub App installation][gh_app_auth].
    ///
    /// <!-- LINKS -->
    /// [gh_app_auth]: https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-as-a-github-app-installation
    pub gh_app_installation_id: i64,

    /// Timestamp when the record was created, in standard SQL `DATETIME` format (`YYYY-MM-DD HH:MM:SS`).
    pub created_at: String,

    /// Timestamp when the record was updated, in standard SQL `DATETIME` format (`YYYY-MM-DD HH:MM:SS`).
    pub updated_at: String,
}

/// Holds payload data for the creation of a [`Subscriber`].
#[derive(Debug, Deserialize)]
pub struct CreateSubscriber {
    /// Determines the value of [`Branch::repo_url`].
    pub source_repo_url: String,

    /// Determines the value of [`Branch::name`].
    pub source_branch_name: String,

    /// Determines the value of [`Branch::polling_interval_secs`].
    pub polling_interval_secs: i64,

    /// Determines the value of [`Subscriber::target_repo`].
    pub target_repo: String,

    /// Determines the value of [`Subscriber::event_type`].
    pub event_type: String,

    /// Determines the value of [`Subscriber::gh_app_installation_id`].
    pub gh_app_installation_id: i64,
}

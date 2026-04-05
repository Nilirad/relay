//! Utilities for checking whether a branch has updated.

use crate::{error::AppError, model::Branch, polling::git::get_latest_hash};

/// Enables comparison between a git branch row, and the newly fetched branch.
pub(super) struct BranchInfo {
    /// The branch currently stored in the database.
    pub branch: Branch,
    /// The actual branch hash.
    pub latest_hash: String,
}

/// Creates a [`BranchInfo`] given a branch row.
pub(super) async fn extract_branch_info(branch: Branch) -> Result<BranchInfo, AppError> {
    let latest_hash = get_latest_hash(branch.repo_url.clone(), branch.name.clone()).await?;
    Ok(BranchInfo {
        branch,
        latest_hash,
    })
}

/// Checks [`BranchInfo`] data to detect whether the branch has updated.
pub(super) fn branch_has_updated(branch_info: &BranchInfo) -> bool {
    branch_info.branch.last_commit_hash.as_deref() != Some(&branch_info.latest_hash)
}

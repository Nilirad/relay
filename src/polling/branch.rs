//! Utilities for checking whether a branch has updated.

use crate::{error::CommitHashError, model::Branch, polling::git::GitFetcher};

/// Enables comparison between a git branch row, and the newly fetched branch.
pub(super) struct BranchInfo {
    /// The branch currently stored in the database.
    pub branch: Branch,

    /// The actual branch hash.
    pub latest_hash: String,
}

impl BranchInfo {
    /// Creates a [`BranchInfo`] given a branch row.
    pub async fn new<F: GitFetcher + ?Sized>(
        branch: Branch,
        fetcher: &F,
    ) -> Result<BranchInfo, CommitHashError> {
        let latest_hash = fetcher
            .get_latest_hash(&branch.repo_url, &branch.name)
            .await?;
        Ok(BranchInfo {
            branch,
            latest_hash,
        })
    }

    /// Checks whether the branch has updated.
    pub fn has_updated(&self) -> bool {
        self.branch.last_commit_hash.as_deref() != Some(&self.latest_hash)
    }
}

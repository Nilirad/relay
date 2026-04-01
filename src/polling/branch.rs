use crate::{error::AppError, model::Branch, polling::git::get_latest_hash};

pub(super) struct BranchInfo {
    pub branch: Branch,
    pub latest_hash: String,
}

pub(super) async fn extract_branch_info(branch: Branch) -> Result<BranchInfo, AppError> {
    let latest_hash = get_latest_hash(branch.repo_url.clone(), branch.name.clone()).await?;
    Ok(BranchInfo {
        branch,
        latest_hash,
    })
}

pub(super) fn branch_has_updated(branch_info: &BranchInfo) -> bool {
    branch_info.branch.last_commit_hash.as_deref() != Some(&branch_info.latest_hash)
}

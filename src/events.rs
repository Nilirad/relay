//! Items allowing communication between modules.

/// Signals that a git branch has updated.
#[derive(Debug, Clone)]
pub struct BranchUpdateEvent {
    /// The database ID column of the updated branch.
    pub branch_id: i64,

    /// The hash of the latest commit on the branch.
    pub new_hash: String,
}

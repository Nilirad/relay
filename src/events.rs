#[derive(Debug, Clone)]
pub struct BranchUpdateEvent {
    pub branch_id: i64,
    pub new_hash: String,
}

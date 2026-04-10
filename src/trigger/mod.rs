use sqlx::SqlitePool;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;

use crate::events::BranchUpdateEvent;

mod jwt;

pub fn start_trigger_engine(
    _pool: SqlitePool,
    _token: CancellationToken,
    mut _rx: Receiver<BranchUpdateEvent>,
) {
    todo!()
}

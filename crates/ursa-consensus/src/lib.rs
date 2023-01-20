mod server;
pub use server::AbciApi;

mod engine;
pub use engine::{Engine, consensus_start};

mod config;
pub use config::ConsensusConfig;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BroadcastTxQuery {
    tx: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AbciQueryQuery {
    path: String,
    data: String,
    height: Option<usize>,
    prove: Option<bool>,
}

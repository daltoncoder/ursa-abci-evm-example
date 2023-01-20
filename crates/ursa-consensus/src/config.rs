use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsensusConfig {
    ///The address to receive ABCI connections to defaults too
    pub domain: String,
    ///File path where the json file containing committee location is located
    pub committee_path: String,
    ///The path on where to create the consensus database. defaults too "~/.ursa/data/index_provider_db"
    pub database_path: String,
}

impl ConsensusConfig {
    fn default_domain() -> String {
        "0.0.0.0:3002".into()
    }
    fn default_database_path() -> String {
        "~/.ursa/data/index_provider_db".into()
    }
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            domain: Self::default_domain(),
            database_path: Self::default_database_path(),
            committee_path: "".into(),
        }
    }
}

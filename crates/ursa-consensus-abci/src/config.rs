use narwhal_config::Parameters;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsensusConfig {
    ///The address to receive ABCI connections to defaults too
    #[serde(default = "ConsensusConfig::default_rpc_domain")]
    pub rpc_domain: String,
    ///File path where the json file containing committee location is located
    #[serde(default)]
    pub committee_path: String,
    ///The path on where to create the consensus database. defaults too "~/.ursa/data/index_provider_db"
    #[serde(default = "ConsensusConfig::default_database_path")]
    pub database_path: String,
    ///The narwhal parameters,
    #[serde(default = "ConsensusConfig::default_params")]
    pub narwhal_params: Parameters,
}

impl ConsensusConfig {
    fn default_rpc_domain() -> String {
        "0.0.0.0:3005".into()
    }
    fn default_database_path() -> String {
        "~/.ursa/data/narwhal".into()
    }
    fn default_params() -> Parameters {
        Parameters {
            max_header_delay: 1000,
            max_batch_delay: 1000,
            ..Parameters::default()
        }
    }
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            rpc_domain: Self::default_rpc_domain(),
            database_path: Self::default_database_path(),
            committee_path: "./committee.json".into(),
            narwhal_params: Self::default_params(),
        }
    }
}

use anyhow::{Context, Result};
use resolve_path::PathResolveExt;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::PathBuf};

pub const DEFAULT_GENESIS_PATH: &str = "~/.ursa/genesis.toml";

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct Genesis {
    #[serde(default)]
    pub hello: GenesisContract,
    #[serde(default)]
    pub token: GenesisContract,
    #[serde(default)]
    pub staking: GenesisContract,
    #[serde(default)]
    pub registry: GenesisContract,
}
#[derive(Default, Serialize, Deserialize, Debug)]
pub struct GenesisContract {
    pub address: String,
    pub bytecode: String,
}
//TODO: Generate genesis if it doesnt exist
impl Genesis {
    /// Load the genesis file
    pub fn load() -> Result<Genesis> {
        let mut file = File::open(PathBuf::from(DEFAULT_GENESIS_PATH).resolve())?;
        let mut raw = String::new();
        file.read_to_string(&mut raw)?;
        toml::from_str(&raw).context("Failed to parse genesis file")
    }
}

use std::path::PathBuf;

use alloy::primitives::Address;
use config::{Config as ConfigBuilder, ConfigError, Environment};
use config_secret::EnvironmentSecretFile;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub chain: ChainConfig,
    pub app: AppConfig,
}

/// Chain/RPC configuration
#[derive(Debug, Deserialize)]
pub struct ChainConfig {
    /// RPC endpoint URL
    pub rpc_endpoint: String,

    /// Contract address to monitor
    pub contract_address: Address,

    /// Chain ID (default: 421614 for Arbitrum Sepolia)
    pub chain_id: u32,

    /// Number of blocks to fetch per batch (default: 50)
    pub batch_size: u64,

    /// Delay between polls in milliseconds (default: 500)
    pub poll_delay_ms: u64,

    /// Delay between retries in milliseconds (default: 250)
    pub retry_delay_ms: u64,
}

/// Application configuration
#[derive(Debug, Deserialize)]
pub struct AppConfig {
    /// Initial block to start from (0 = require state file)
    pub initial_block: u64,

    /// State file path (default: nox_ingestor_state_421614.json)
    pub state_file: String,

    /// Flush interval in seconds (default: 5)
    pub flush_interval_secs: u64,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config = ConfigBuilder::builder()
            .set_default(
                "chain.rpc_endpoint",
                "https://arbitrum-sepolia-rpc.publicnode.com",
            )?
            .set_default(
                "chain.contract_address",
                "0x0000000000000000000000000000000000000000",
            )?
            .set_default("chain.chain_id", 421614)?
            .set_default("chain.poll_delay_ms", 500)?
            .set_default("chain.retry_delay_ms", 250)?
            .set_default("chain.initial_block", 0)?
            .set_default("chain.batch_size", 50)?
            .set_default("app.flush_interval_secs", 5)?
            .set_default("app.initial_block", 0)?
            .set_default("app.state_file", "nox_ingestor_state_421614.json")?
            .add_source(
                Environment::with_prefix("NOX_INGESTOR")
                    .prefix_separator("_")
                    .separator("__"),
            )
            .add_source(EnvironmentSecretFile::with_prefix("NOX_INGESTOR").separator("_"))
            .build()?;

        config.try_deserialize()
    }

    /// Get the state file path, using default if not specified
    pub fn state_file_path(&self) -> PathBuf {
        if self.app.state_file.is_empty() {
            PathBuf::from(format!("./nox_ingestor_state_{}.json", self.chain.chain_id))
        } else {
            PathBuf::from(&self.app.state_file)
        }
    }
}

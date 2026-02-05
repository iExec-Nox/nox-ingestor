use std::path::PathBuf;
use std::time::Duration;

use alloy::primitives::Address;
use config::{Config as ConfigBuilder, ConfigError, Environment};
use config_secret::EnvironmentSecretFile;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub app: AppConfig,
    pub chain: ChainConfig,
    pub nats: NatsConfig,
}

/// Chain/RPC configuration
#[derive(Debug, Deserialize)]
pub struct ChainConfig {
    /// Chain ID (default: 421614 for Arbitrum Sepolia)
    pub chain_id: u32,

    /// RPC endpoint URL
    pub rpc_endpoint: String,

    /// Contract address to monitor
    pub contract_address: Address,

    /// Number of blocks to fetch per batch (default: 50)
    pub batch_size: u64,

    /// Delay between polls (default: "500ms")
    #[serde(with = "humantime_serde")]
    pub poll_delay: Duration,

    /// Delay between retries (default: "250ms")
    #[serde(with = "humantime_serde")]
    pub retry_delay: Duration,
}

/// Application configuration
#[derive(Debug, Deserialize)]
pub struct AppConfig {
    /// Initial block to start from (0 = require state file)
    pub initial_block: u64,

    /// State file path (default: nox_ingestor_state_421614.json)
    pub state_file: String,

    /// Flush interval (default: "5s")
    #[serde(with = "humantime_serde")]
    pub flush_interval: Duration,
}

/// NATS JetStream configuration
#[derive(Debug, Clone, Deserialize)]
pub struct NatsConfig {
    /// NATS server URL
    pub url: String,

    /// JetStream stream name
    pub stream_name: String,

    /// Subject prefix for events
    pub subject: String,

    /// Stream retention (default: "1d")
    #[serde(with = "humantime_serde")]
    pub retention: Duration,

    /// Duplicate detection window (default: "10m")
    #[serde(with = "humantime_serde")]
    pub duplicate_window: Duration,
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
            .set_default("chain.poll_delay", "500ms")?
            .set_default("chain.retry_delay", "250ms")?
            .set_default("chain.initial_block", 0)?
            .set_default("chain.batch_size", 50)?
            .set_default("app.flush_interval", "5s")?
            .set_default("app.initial_block", 0)?
            .set_default("app.state_file", "nox_ingestor_state_421614.json")?
            .set_default("nats.url", "nats://localhost:4222")?
            .set_default("nats.stream_name", "nox_ingestor")?
            .set_default("nats.subject", "nox_ingestor")?
            .set_default("nats.retention", "1d")?
            .set_default("nats.duplicate_window", "10m")?
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

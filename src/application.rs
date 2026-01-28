use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::chain::{ChainClient, NoxEventParser};
use crate::config::Config;

pub struct Application {
    config: Config,
}

impl Application {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self { config })
    }

    pub async fn run(self) -> Result<()> {
        debug!("Starting ingestor");
        debug!("Config: {:?}", self.config);

        let contract_address = self
            .config
            .chain
            .contract_address
            .parse()
            .context("invalid chain.contract_address")?;
        let parser = NoxEventParser::new(contract_address);
        let client = ChainClient::new(
            &self.config.chain.rpc_endpoint,
            contract_address,
            parser.event_signatures(),
        )?;

        let latest_block = client.get_latest_block().await?;
        info!(latest_block, "Latest block");

        let from_block = latest_block.saturating_sub(self.config.chain.batch_size);
        let logs = client.get_logs(from_block, latest_block).await?;
        info!(
            count = logs.len(),
            from_block,
            to_block = latest_block,
            "Fetched logs"
        );

        if let Some(log) = logs.first() {
            parser.parse(log);
        }

        Ok(())
    }
}

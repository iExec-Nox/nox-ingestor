use anyhow::Result;
use tracing::{debug, info};

use crate::chain::{ChainClient, NoxEventParser};
use crate::config::Config;
use crate::error::NoxError;
use crate::state::StateStore;

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

        let parser = NoxEventParser::new(self.config.chain.contract_address);
        let client = ChainClient::new(
            &self.config.chain.rpc_endpoint,
            self.config.chain.contract_address,
            parser.event_signatures(),
        )?;

        let state_store = self.load_state_store().await?;
        let mut next_block = self.determine_start_block(&state_store)?;

        let latest_block = client.get_latest_block().await?;
        info!(latest_block, "Latest block");

        while next_block <= latest_block {
            let to_block = (next_block + self.config.chain.batch_size - 1).min(latest_block);
            let logs = client.get_logs(next_block, to_block).await?;
            info!(
                count = logs.len(),
                from_block = next_block,
                to_block,
                "Fetched logs"
            );
            for log in logs {
                parser.parse(&log);
            }
            state_store.update(to_block); // Last processed block
            next_block = to_block + 1;
        }
        state_store.persist().await?;
        Ok(())
    }

    /// Load state store
    async fn load_state_store(&self) -> Result<StateStore, NoxError> {
        let state_file_path = self.config.state_file_path();
        let initial_block = self.config.app.initial_block;

        info!(
            path = %state_file_path.display(),
            initial_block,
            "Loading state"
        );

        let state_store =
            StateStore::load(state_file_path, self.config.chain.chain_id, initial_block).await?;

        Ok(state_store)
    }

    /// Determine the starting block based on state
    fn determine_start_block(&self, state_store: &StateStore) -> Result<u64, NoxError> {
        if state_store.get() == 0 && !state_store.was_loaded_from_file() {
            // No state file and no INITIAL_BLOCK: fail fast
            return Err(NoxError::NoInitialBlock);
        }

        if state_store.was_loaded_from_file() {
            // Resume from last processed + 1
            let last = state_store.get();
            info!(
                resuming_from = last + 1,
                last_processed = last,
                "Resuming from persisted state"
            );
            Ok(last + 1)
        } else {
            // Fresh start with INITIAL_BLOCK
            let initial = state_store.get();
            info!(starting_from = initial, "Starting from initial block");
            Ok(initial)
        }
    }
}

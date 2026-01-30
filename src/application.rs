use anyhow::Result;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::chain::{BlockReader, NoxEventParser};
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

        let block_reader = BlockReader::new(
            &self.config.chain.rpc_endpoint,
            parser.contract_address(),
            parser.event_signatures(),
            self.config.chain.batch_size,
            self.config.chain.poll_delay_ms,
            self.config.chain.retry_delay_ms,
        )?;

        let state_store = self.load_state_store().await?;
        let mut next_block = self.determine_start_block(&state_store)?;

        loop {
            let latest_block = block_reader.get_latest_block().await?;
            if next_block > latest_block {
                sleep(block_reader.poll_delay()).await;
            } else {
                let batch = block_reader
                    .read_batch_with_retry(next_block, latest_block)
                    .await;

                for log in batch.logs {
                    let nox_event = parser.parse(&log);
                    if nox_event.is_none() {
                        continue;
                    }
                    let nox_event = nox_event.unwrap();
                    let event_type = nox_event.event_type();
                    let caller = nox_event.caller();
                    info!(
                        log_index = log.log_index,
                        event_type,
                        caller = %caller,
                        "Event"
                    );
                }
                if batch.end_block >= batch.start_block {
                    state_store.update(batch.end_block);
                    next_block = batch.end_block + 1;
                }

                // Only delay if we're caught up (within 1 batch of head)
                // Skip delay when catching up to maximize throughput
                let gap = latest_block.saturating_sub(batch.end_block);
                if gap <= 1 {
                    sleep(block_reader.poll_delay()).await;
                } else {
                    debug!(gap, "Catching up, skipping poll delay");
                }

                state_store.persist().await?;
            }
        }
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

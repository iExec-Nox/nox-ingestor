use std::time::Duration;

use anyhow::Result;
use tokio::time::{interval, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, info_span, warn};

/// Timeout for final state persistence on shutdown
const SHUTDOWN_PERSIST_TIMEOUT: Duration = Duration::from_secs(5);

use crate::chain::{BlockReader, NoxEventParser};
use crate::config::Config;
use crate::error::NoxError;
use crate::events::{Operator, TransactionEvent};
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

        let shutdown_token = self.setup_signal_handler();

        let parser = NoxEventParser::new(self.config.chain.contract_address);

        let block_reader = BlockReader::new(
            &self.config.chain.rpc_endpoint,
            parser,
            self.config.chain.batch_size,
            self.config.chain.poll_delay_ms,
            self.config.chain.retry_delay_ms,
            self.config.chain.chain_id,
        )?;

        let state_store = self.load_state_store().await?;
        let mut next_block = self.determine_start_block(&state_store)?;

        let mut flush_interval = interval(Duration::from_secs(self.config.app.flush_interval_secs));
        flush_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip the immediate first tick to avoid flushing before any work is done
        flush_interval.tick().await;

        let poll_delay = block_reader.poll_delay();
        let mut needs_delay = false;

        loop {
            // Handle delay outside of select! to allow cancellation during sleep
            if needs_delay {
                tokio::select! {
                    biased;
                    _ = shutdown_token.cancelled() => {
                        info!("Shutdown signal received");
                        break;
                    }
                    _ = sleep(poll_delay) => {
                        needs_delay = false;
                    }
                }
                if shutdown_token.is_cancelled() {
                    break;
                }
                continue;
            }

            tokio::select! {
                biased;

                // 1. Shutdown (highest priority)
                _ = shutdown_token.cancelled() => {
                    info!("Shutdown signal received");
                    break;
                }

                // 2. Periodic state flush
                _ = flush_interval.tick() => {
                    if let Err(e) = state_store.persist().await {
                        warn!(error = %e, "Failed to persist state");
                    }
                }

                // 3. Block reading
                _ = async {
                    // Get latest block
                    let latest = match block_reader.get_latest_block().await {
                        Ok(b) => b,
                        Err(e) => {
                            error!(error = %e, "Failed to get latest block");
                            needs_delay = true;
                            return;
                        }
                    };

                    // Check if we need to wait for new blocks
                    if next_block > latest {
                        needs_delay = true;
                        return;
                    }

                    let batch = block_reader
                        .read_batch_with_retry(next_block, latest)
                        .await;

                    for transaction in batch.transactions {
                        let span = info_span!(
                            "transaction",
                            tx_hash = transaction.transaction_hash,
                            block_number = transaction.block_number,
                            caller = format!("{:#x}", transaction.caller),
                            event_count = transaction.events.len()
                        );
                        let _guard = span.enter();

                        for event in &transaction.events {
                            log_event(event);
                        }
                    }

                    if batch.end_block >= batch.start_block {
                        state_store.update(batch.end_block);
                        next_block = batch.end_block + 1;
                    }

                    // Only delay if we're caught up (within 1 batch of head)
                    // Skip delay when catching up to maximize throughput
                    let gap = latest.saturating_sub(batch.end_block);
                    if gap <= 1 {
                        needs_delay = true;
                    } else {
                        debug!(gap, "Catching up, skipping poll delay");
                    }
                } => {}
            }
        }

        // Final cleanup with timeout to prevent hanging on shutdown
        info!("Persisting final state...");
        match timeout(SHUTDOWN_PERSIST_TIMEOUT, state_store.persist()).await {
            Ok(Ok(())) => debug!("Final state persisted successfully"),
            Ok(Err(e)) => error!(error = %e, "Failed to persist final state"),
            Err(_) => error!(
                "Timeout persisting final state after {:?}",
                SHUTDOWN_PERSIST_TIMEOUT
            ),
        }

        info!("Listener stopped gracefully");
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

    /// Setup signal handlers for graceful shutdown.
    ///
    /// Handles SIGINT (Ctrl+C) on all platforms.
    /// Additionally handles SIGTERM on Unix for container/systemd deployments.
    fn setup_signal_handler(&self) -> CancellationToken {
        let shutdown_token = CancellationToken::new();
        let token = shutdown_token.clone();

        tokio::spawn(async move {
            let shutdown_signal = Self::wait_for_shutdown_signal().await;
            info!(
                "{} received, initiating graceful shutdown...",
                shutdown_signal
            );
            token.cancel();
        });

        shutdown_token
    }

    /// Wait for a shutdown signal (SIGINT or SIGTERM on Unix, SIGINT only on other platforms).
    async fn wait_for_shutdown_signal() -> &'static str {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};

            let mut sigterm =
                signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");
            let mut sigint =
                signal(SignalKind::interrupt()).expect("Failed to install SIGINT handler");

            tokio::select! {
                _ = sigterm.recv() => "SIGTERM",
                _ = sigint.recv() => "SIGINT",
            }
        }

        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
            "Ctrl+C"
        }
    }
}

fn log_event(event: &TransactionEvent) {
    match &event.operator {
        Operator::Add(op) => {
            info!(
                log_index = event.log_index,
                caller = format!("{:#x}", event.caller),
                leftHandOperand = op.left_hand_operand,
                rightHandOperand = op.right_hand_operand,
                result = op.result,
                "Add"
            );
        }
        Operator::Sub(op) => {
            info!(
                log_index = event.log_index,
                caller = format!("{:#x}", event.caller),
                leftHandOperand = op.left_hand_operand,
                rightHandOperand = op.right_hand_operand,
                result = op.result,
                "Sub"
            );
        }
        Operator::Div(op) => {
            info!(
                log_index = event.log_index,
                caller = format!("{:#x}", event.caller),
                leftHandOperand = op.left_hand_operand,
                rightHandOperand = op.right_hand_operand,
                result = op.result,
                "Div"
            );
        }
        Operator::Select(op) => {
            info!(
                log_index = event.log_index,
                caller = format!("{:#x}", event.caller),
                condition = op.condition,
                if_true = op.if_true,
                if_false = op.if_false,
                result = op.result,
                "Select"
            );
        }
        Operator::PlaintextToEncrypted(op) => {
            info!(
                log_index = event.log_index,
                caller = format!("{:#x}", event.caller),
                value = op.value,
                tee_type = op.tee_type,
                handle = op.handle,
                "PlaintextToEncrypted"
            );
        }
    }
}

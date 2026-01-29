//! Block reader with batch support

use std::time::Duration;

use alloy::primitives::{Address, B256};
use alloy::rpc::types::Log;
use tokio::time::sleep;
use tracing::{debug, error, warn};

use crate::error::ChainError;

use super::client::ChainClient;

/// Result of reading a batch of blocks
#[derive(Debug)]
pub struct BatchResult {
    /// Logs from the batch
    pub logs: Vec<Log>,
    /// First block in the batch
    pub start_block: u64,
    /// Last block in the batch (inclusive)
    pub end_block: u64,
}

/// Block reader with batch support
pub struct BlockReader {
    client: ChainClient,
    batch_size: u64,
    poll_delay: Duration,
    retry_delay_ms: Duration,
}

impl BlockReader {
    /// Create a new block reader
    pub fn new(
        rpc_endpoint: &str,
        contract_address: Address,
        event_signatures: Vec<B256>,
        batch_size: u64,
        poll_delay_ms: u64,
        retry_delay_ms: u64,
    ) -> Result<Self, ChainError> {
        let client = ChainClient::new(rpc_endpoint, contract_address, event_signatures)?;

        Ok(Self {
            client,
            batch_size,
            poll_delay: Duration::from_millis(poll_delay_ms),
            retry_delay_ms: Duration::from_millis(retry_delay_ms),
        })
    }

    /// Get the latest block number with retry
    pub async fn get_latest_block(&self) -> Result<u64, ChainError> {
        loop {
            match self.client.get_latest_block().await {
                Ok(block) => return Ok(block),
                Err(e) => {
                    error!(error = %e, retry_delay_ms = %self.retry_delay_ms.as_millis(), "Failed to get latest block");
                    sleep(self.retry_delay_ms).await;
                }
            }
        }
    }

    /// Read a batch with retry on failure
    pub async fn read_batch_with_retry(&self, start_block: u64, latest_block: u64) -> BatchResult {
        loop {
            match self.read_batch(start_block, latest_block).await {
                Ok(result) => {
                    debug!(
                        start_block,
                        end_block = result.end_block,
                        tx_count = result.logs.len(),
                        "Batch read successfully"
                    );
                    return result;
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        start_block,
                        retry_delay_ms = %self.retry_delay_ms.as_millis(),
                        "Failed to read batch, retrying"
                    );
                    sleep(self.retry_delay_ms).await;
                }
            }
        }
    }

    /// Read a batch of blocks starting from `start_block`
    ///
    /// Returns transactions grouped from the batch.
    /// Events within each transaction are sorted by log_index.
    /// Transactions are sorted by (block_number, first_log_index).
    async fn read_batch(
        &self,
        start_block: u64,
        latest_block: u64,
    ) -> Result<BatchResult, ChainError> {
        if start_block > latest_block {
            // No blocks available yet
            return Ok(BatchResult {
                logs: Vec::new(),
                start_block,
                end_block: start_block.saturating_sub(1),
            });
        }

        // Calculate end block (clamped to latest and batch_size)
        let end_block = (start_block + self.batch_size - 1).min(latest_block);

        // Fetch logs for the entire range
        let logs = self.client.get_logs(start_block, end_block).await?;

        Ok(BatchResult {
            logs,
            start_block,
            end_block,
        })
    }

    /// Get the poll delay
    pub fn poll_delay(&self) -> Duration {
        self.poll_delay
    }
}

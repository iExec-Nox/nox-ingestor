//! Block reader with batch support

use std::collections::HashMap;
use std::time::Duration;

use alloy::primitives::{Address, FixedBytes};
use alloy::rpc::types::Log;
use tokio::time::sleep;
use tracing::{debug, error, warn};

use crate::error::ChainError;
use crate::events::{
    BinaryOperation, EncryptionOperation, Operator, SelectOperation, TransactionEvent,
    TransactionMessage,
};

use super::NoxEventParser;
use super::client::ChainClient;
use super::parser::NoxEvent;

/// Result of reading a batch of blocks
#[derive(Debug)]
pub struct BatchResult {
    /// Transactions from the batch
    pub transactions: Vec<TransactionMessage>,
    /// First block in the batch
    pub start_block: u64,
    /// Last block in the batch (inclusive)
    pub end_block: u64,
}

/// Block reader with batch support
pub struct BlockReader {
    client: ChainClient,
    parser: NoxEventParser,
    batch_size: u64,
    poll_delay: Duration,
    retry_delay_ms: Duration,
    chain_id: u32,
}

impl BlockReader {
    /// Create a new block reader
    pub fn new(
        rpc_endpoint: &str,
        parser: NoxEventParser,
        batch_size: u64,
        poll_delay_ms: u64,
        retry_delay_ms: u64,
        chain_id: u32,
    ) -> Result<Self, ChainError> {
        let client = ChainClient::new(
            rpc_endpoint,
            parser.contract_address(),
            parser.event_signatures(),
        )?;

        Ok(Self {
            client,
            parser,
            batch_size,
            poll_delay: Duration::from_millis(poll_delay_ms),
            retry_delay_ms: Duration::from_millis(retry_delay_ms),
            chain_id,
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
                        tx_count = result.transactions.len(),
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
                transactions: Vec::new(),
                start_block,
                end_block: start_block.saturating_sub(1),
            });
        }

        // Calculate end block (clamped to latest and batch_size)
        let end_block = (start_block + self.batch_size - 1).min(latest_block);

        // Fetch logs for the entire range
        let logs = self.client.get_logs(start_block, end_block).await?;

        // Parse all logs and convert to TransactionEvent with metadata
        // Tuple: (block_number, log_index, tx_hash, caller, event)
        let mut events: Vec<(u64, u64, String, Address, TransactionEvent)> = logs
            .iter()
            .filter_map(|log| {
                let nox_event = self.parser.parse(log)?;
                to_transaction_event(&nox_event, log)
            })
            .collect();

        // Sort by (block_number, log_index) for guaranteed order
        events.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        // Group events by transaction
        let transactions = self.group_by_transaction(events);

        Ok(BatchResult {
            transactions,
            start_block,
            end_block,
        })
    }

    /// Get the poll delay
    pub fn poll_delay(&self) -> Duration {
        self.poll_delay
    }

    /// Group events by transaction hash
    ///
    /// Returns transactions sorted by (block_number, first_log_index).
    /// Events within each transaction are sorted by log_index.
    ///
    /// Input: Vec of (block_number, log_index, tx_hash, caller, event)
    fn group_by_transaction(
        &self,
        events: Vec<(u64, u64, String, Address, TransactionEvent)>,
    ) -> Vec<TransactionMessage> {
        // Map: tx_hash -> (block_number, caller, events)
        let mut tx_map: HashMap<String, (u64, Address, Vec<TransactionEvent>)> = HashMap::new();

        for (block_number, _log_index, tx_hash, caller, event) in events {
            tx_map
                .entry(tx_hash)
                .or_insert_with(|| (block_number, caller, Vec::new()))
                .2
                .push(event);
        }

        let mut messages: Vec<TransactionMessage> = tx_map
            .into_iter()
            .map(|(tx_hash, (block_number, caller, events))| {
                let first_log_index = events.first().map(|e| e.log_index).unwrap_or(0);
                TransactionMessage::new(
                    self.chain_id,
                    caller,
                    block_number,
                    first_log_index,
                    tx_hash,
                    events,
                )
            })
            .collect();

        messages.sort_by(|a, b| {
            a.block_number
                .cmp(&b.block_number)
                .then_with(|| a.first_log_index.cmp(&b.first_log_index))
        });

        messages
    }
}

/// Convert bytes32 to hex string
fn to_handle(bytes: FixedBytes<32>) -> String {
    format!("{:#x}", bytes)
}

/// Convert a parsed `NoxEvent` with its source `Log` metadata into a `TransactionEvent`.
/// Returns (block_number, log_index, tx_hash, caller, event) or None if metadata is missing.
fn to_transaction_event(
    event: &NoxEvent,
    log: &Log,
) -> Option<(u64, u64, String, Address, TransactionEvent)> {
    let tx_hash = format!("{:#x}", log.transaction_hash?);
    let block_number = log.block_number?;
    let log_index = log.log_index?;
    let caller = event.caller();

    let operator = match event {
        NoxEvent::PlaintextToEncrypted(e) => Operator::PlaintextToEncrypted(EncryptionOperation {
            value: e.value.to_string(),
            value_type: e.valueType,
            handle: to_handle(e.handle),
        }),
        NoxEvent::Add(e) => Operator::Add(BinaryOperation {
            lhs: to_handle(e.lhs),
            rhs: to_handle(e.rhs),
            result: to_handle(e.result),
        }),
        NoxEvent::Sub(e) => Operator::Sub(BinaryOperation {
            lhs: to_handle(e.lhs),
            rhs: to_handle(e.rhs),
            result: to_handle(e.result),
        }),
        NoxEvent::Div(e) => Operator::Div(BinaryOperation {
            lhs: to_handle(e.lhs),
            rhs: to_handle(e.rhs),
            result: to_handle(e.result),
        }),
        NoxEvent::Select(e) => Operator::Select(SelectOperation {
            condition: to_handle(e.condition),
            if_true: to_handle(e.ifTrue),
            if_false: to_handle(e.ifFalse),
            result: to_handle(e.result),
        }),
    };

    let event = TransactionEvent {
        log_index,
        operator,
    };

    Some((block_number, log_index, tx_hash, caller, event))
}

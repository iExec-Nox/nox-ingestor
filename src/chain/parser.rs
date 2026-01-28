//! NOX event parser for extracting raw event data

use alloy::hex;
use alloy::primitives::{Address, B256, keccak256};
use alloy::rpc::types::Log;
use once_cell::sync::Lazy;
use tracing::{debug, info};

// Event signatures for NOX operations
static PLAINTEXT_TO_ENCRYPTED_SIG: Lazy<B256> =
    Lazy::new(|| keccak256("PlaintextToEncrypted(address,uint256,uint8,bytes32)"));
static ADD_SIG: Lazy<B256> = Lazy::new(|| keccak256("Add(address,bytes32,bytes32,bytes32)"));
static SUB_SIG: Lazy<B256> = Lazy::new(|| keccak256("Sub(address,bytes32,bytes32,bytes32)"));
static DIV_SIG: Lazy<B256> = Lazy::new(|| keccak256("Div(address,bytes32,bytes32,bytes32)"));
static SELECT_SIG: Lazy<B256> =
    Lazy::new(|| keccak256("Select(address,bytes32,bytes32,bytes32,bytes32)"));

/// NOX event parser
pub struct NoxEventParser {
    contract_address: Address,
}

impl NoxEventParser {
    /// Create a new NOX event parser
    pub fn new(contract_address: Address) -> Self {
        Self { contract_address }
    }

    /// Get the contract address
    pub fn contract_address(&self) -> Address {
        self.contract_address
    }

    /// Get all event signatures to filter
    pub fn event_signatures(&self) -> Vec<B256> {
        vec![
            *PLAINTEXT_TO_ENCRYPTED_SIG,
            *ADD_SIG,
            *SUB_SIG,
            *DIV_SIG,
            *SELECT_SIG,
        ]
    }

    /// Parse a log
    ///
    /// Extracts:
    /// - event_type from topic[0]
    /// - caller from topic[1]
    /// - raw_data as hex string from log data (non-indexed params)
    pub fn parse(&self, log: &Log) {
        let topics = log.topics();
        if topics.len() < 2 {
            return;
        }

        let topic0 = topics[0];

        // Determine event type
        let event_type = if topic0 == *PLAINTEXT_TO_ENCRYPTED_SIG {
            "plaintext_to_encrypted"
        } else if topic0 == *ADD_SIG {
            "add"
        } else if topic0 == *SUB_SIG {
            "sub"
        } else if topic0 == *DIV_SIG {
            "div"
        } else if topic0 == *SELECT_SIG {
            "select"
        } else {
            debug!("Unknown event type: {:?}", topic0);
            return;
        };

        // Validate data length based on event type
        let data = &log.data().data;

        // Extract caller from topic[1]
        let caller = Address::from_slice(&topics[1].as_slice()[12..]);

        // Get transaction hash and log index
        let tx_hash = log.transaction_hash.unwrap_or_default().to_string();
        let log_index = log.log_index.unwrap_or_default() as u32;
        let block_number = log.block_number.unwrap_or(0);

        // Convert raw data to hex string
        let raw_data = format!("0x{}", hex::encode(data));

        info!(
            event_type = event_type,
            block_number = block_number,
            tx_hash = tx_hash,
            log_index = log_index,
            caller = caller.to_string(),
            raw_data = raw_data,
            "Parsed event"
        );
    }
}

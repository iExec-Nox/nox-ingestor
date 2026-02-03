//! NOX event types and message format
//!
//! Transaction-based message grouping: one message per transaction
//! containing all NOX events from that transaction.

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

/// Handle type for encrypted values (hex-encoded bytes32)
pub type Handle = String;

/// Binary operation (add, sub, div)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryOperation {
    pub lhs: Handle,
    pub rhs: Handle,
    pub result: Handle,
}

/// Select operation (conditional)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOperation {
    pub condition: Handle,
    pub if_true: Handle,
    pub if_false: Handle,
    pub result: Handle,
}

/// Encryption operation (plaintext to encrypted)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionOperation {
    pub value: String,
    pub value_type: u8,
    pub handle: Handle,
}

/// Event payload with typed variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operator {
    PlaintextToEncrypted(EncryptionOperation),
    Add(BinaryOperation),
    Sub(BinaryOperation),
    Div(BinaryOperation),
    Select(SelectOperation),
}

/// Individual event within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEvent {
    pub log_index: u64,
    /// Caller address
    pub caller: Address,
    #[serde(flatten)]
    pub operator: Operator,
}

/// Message format grouping events by transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionMessage {
    /// Chain ID where the events occurred
    pub chain_id: u32,
    /// Caller address
    pub caller: Address,
    /// Block number
    pub block_number: u64,
    /// First log index in this transaction (used for ordering)
    #[serde(skip)]
    pub first_log_index: u64,
    /// Transaction hash
    pub transaction_hash: String,
    /// Events in this transaction, ordered by log_index
    pub events: Vec<TransactionEvent>,
}

impl TransactionMessage {
    /// Creates a new transaction message
    pub fn new(
        chain_id: u32,
        caller: Address,
        block_number: u64,
        first_log_index: u64,
        transaction_hash: String,
        events: Vec<TransactionEvent>,
    ) -> Self {
        Self {
            chain_id,
            caller,
            block_number,
            first_log_index,
            transaction_hash,
            events,
        }
    }
}

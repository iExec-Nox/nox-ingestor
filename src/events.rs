//! NOX event types and message format
//!
//! Transaction-based message grouping: one message per transaction
//! containing all NOX events from that transaction.

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
pub enum EventPayload {
    PlaintextToEncrypted(EncryptionOperation),
    Add(BinaryOperation),
    Sub(BinaryOperation),
    Div(BinaryOperation),
    Select(SelectOperation),
}

impl EventPayload {
    /// Returns the event type name
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::PlaintextToEncrypted(_) => "plaintext_to_encrypted",
            Self::Add(_) => "add",
            Self::Sub(_) => "sub",
            Self::Div(_) => "div",
            Self::Select(_) => "select",
        }
    }
}

/// Individual event within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEvent {
    pub log_index: u64,
    pub caller: String,
    #[serde(flatten)]
    pub payload: EventPayload,
}

/// Message format grouping events by transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionMessage {
    /// Chain ID where the events occurred
    pub chain_id: u64,
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
        chain_id: u64,
        block_number: u64,
        first_log_index: u64,
        transaction_hash: String,
        events: Vec<TransactionEvent>,
    ) -> Self {
        Self {
            chain_id,
            block_number,
            first_log_index,
            transaction_hash,
            events,
        }
    }
}

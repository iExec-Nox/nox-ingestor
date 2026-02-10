//! NOX event types and message format
//!
//! Transaction-based message grouping: one message per transaction
//! containing all NOX events from that transaction.

use alloy::primitives::{Address, keccak256};
use serde::{Deserialize, Serialize};

/// Handle type for encrypted values (hex-encoded bytes32)
pub type Handle = String;

/// Binary operation (add, sub, mul, div)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryOperation {
    pub left_hand_operand: Handle,
    pub right_hand_operand: Handle,
    pub result: Handle,
}

/// Safe binary operation (safe_add, safe_sub, safe_mul, safe_div)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeBinaryOperation {
    pub left_hand_operand: Handle,
    pub right_hand_operand: Handle,
    pub success: Handle,
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
    pub tee_type: u8,
    pub handle: Handle,
}

/// Event payload with typed variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operator {
    PlaintextToEncrypted(EncryptionOperation),
    Add(BinaryOperation),
    Sub(BinaryOperation),
    Mul(BinaryOperation),
    Div(BinaryOperation),
    SafeAdd(SafeBinaryOperation),
    SafeSub(SafeBinaryOperation),
    SafeMul(SafeBinaryOperation),
    SafeDiv(SafeBinaryOperation),
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

    /// Computes a unique checksum for deduplication
    /// Based on chain_id + tx_hash (no log_index since we group by tx)
    pub fn compute_checksum(&self) -> String {
        let input = format!("{}:{}", self.chain_id, self.transaction_hash);
        keccak256(input.as_bytes()).to_string()
    }

    /// Returns the subject for the transaction message
    pub fn subject(&self, base_subject: &str) -> String {
        format!("{}.{}", base_subject, self.transaction_hash)
    }

    /// Converts the transaction message to bytes for NATS
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

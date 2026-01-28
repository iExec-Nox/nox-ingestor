//! NOX event parser using alloy sol! macro for type-safe event parsing

use alloy::primitives::{Address, B256};
use alloy::rpc::types::Log;
use alloy::sol;
use alloy::sol_types::SolEvent;
use tracing::{debug, info};

// Define NOX events using sol! macro
// Signatures are computed at compile-time as constants
sol! {
    #[derive(Debug)]
    event PlaintextToEncrypted(
        address indexed caller,
        uint256 value,
        uint8 valueType,
        bytes32 handle
    );

    #[derive(Debug)]
    event Add(
        address indexed caller,
        bytes32 lhs,
        bytes32 rhs,
        bytes32 result
    );

    #[derive(Debug)]
    event Sub(
        address indexed caller,
        bytes32 lhs,
        bytes32 rhs,
        bytes32 result
    );

    #[derive(Debug)]
    event Div(
        address indexed caller,
        bytes32 lhs,
        bytes32 rhs,
        bytes32 result
    );

    #[derive(Debug)]
    event Select(
        address indexed caller,
        bytes32 condition,
        bytes32 ifTrue,
        bytes32 ifFalse,
        bytes32 result
    );
}

/// Parsed NOX event with strongly typed data
#[derive(Debug, Clone)]
pub enum NoxEvent {
    PlaintextToEncrypted(PlaintextToEncrypted),
    Add(Add),
    Sub(Sub),
    Div(Div),
    Select(Select),
}

impl NoxEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::PlaintextToEncrypted(_) => "plaintext_to_encrypted",
            Self::Add(_) => "add",
            Self::Sub(_) => "sub",
            Self::Div(_) => "div",
            Self::Select(_) => "select",
        }
    }

    pub fn caller(&self) -> Address {
        match self {
            Self::PlaintextToEncrypted(e) => e.caller,
            Self::Add(e) => e.caller,
            Self::Sub(e) => e.caller,
            Self::Div(e) => e.caller,
            Self::Select(e) => e.caller,
        }
    }
}

/// NOX event parser
pub struct NoxEventParser {
    contract_address: Address,
}

impl NoxEventParser {
    pub fn new(contract_address: Address) -> Self {
        Self { contract_address }
    }

    pub fn contract_address(&self) -> Address {
        self.contract_address
    }

    /// Get all event signatures to filter (compile-time constants)
    pub fn event_signatures(&self) -> Vec<B256> {
        vec![
            PlaintextToEncrypted::SIGNATURE_HASH,
            Add::SIGNATURE_HASH,
            Sub::SIGNATURE_HASH,
            Div::SIGNATURE_HASH,
            Select::SIGNATURE_HASH,
        ]
    }

    /// Parse a log into a strongly-typed NoxEvent
    pub fn parse(&self, log: &Log) -> Option<NoxEvent> {
        let topics = log.topics();
        if topics.is_empty() {
            return None;
        }

        let topic0 = topics[0];

        let event = match topic0 {
            PlaintextToEncrypted::SIGNATURE_HASH => PlaintextToEncrypted::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::PlaintextToEncrypted(e.data)),
            Add::SIGNATURE_HASH => Add::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Add(e.data)),
            Sub::SIGNATURE_HASH => Sub::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Sub(e.data)),
            Div::SIGNATURE_HASH => Div::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Div(e.data)),
            Select::SIGNATURE_HASH => Select::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Select(e.data)),
            _ => {
                debug!("Unknown event type: {:?}", topic0);
                None
            }
        }?;

        let tx_hash = log.transaction_hash.unwrap_or_default();
        let log_index = log.log_index.unwrap_or_default() as u32;
        let block_number = log.block_number.unwrap_or(0);

        info!(
            event_type = event.event_type(),
            block_number = block_number,
            tx_hash = %tx_hash,
            log_index = log_index,
            caller = %event.caller(),
            "Parsed event"
        );

        Some(event)
    }
}

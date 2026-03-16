//! NOX event parser using alloy sol! macro for type-safe event parsing

use alloy::primitives::{Address, B256};
use alloy::rpc::types::Log;
use alloy::sol;
use alloy::sol_types::SolEvent;
use tracing::debug;

// Define NOX events using sol! macro
// Signatures are computed at compile-time as constants
sol! {
    #[derive(Debug)]
    event PlaintextToEncrypted(
        address indexed caller,
        bytes32 value,
        uint8 teeType,
        bytes32 handle
    );

    #[derive(Debug)]
    event WrapPublicHandle(
        address indexed caller,
        bytes32 value,
        uint8 teeType,
        bytes32 handle,
    );

    #[derive(Debug)]
    event Add(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event Sub(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event Mul(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event Div(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event SafeAdd(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 success,
        bytes32 result
    );

    #[derive(Debug)]
    event SafeSub(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 success,
        bytes32 result
    );

    #[derive(Debug)]
    event SafeMul(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 success,
        bytes32 result
    );

    #[derive(Debug)]
    event SafeDiv(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 success,
        bytes32 result
    );

    #[derive(Debug)]
    event Eq(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event Ne(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event Ge(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event Gt(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event Le(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
        bytes32 result
    );

    #[derive(Debug)]
    event Lt(
        address indexed caller,
        bytes32 leftHandOperand,
        bytes32 rightHandOperand,
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

    #[derive(Debug)]
    event Transfer(
        address indexed caller,
        bytes32 balanceFrom,
        bytes32 balanceTo,
        bytes32 amount,
        bytes32 success,
        bytes32 newBalanceFrom,
        bytes32 newBalanceTo
    );

    #[derive(Debug)]
    event Mint(
        address indexed caller,
        bytes32 balanceTo,
        bytes32 amount,
        bytes32 totalSupply,
        bytes32 success,
        bytes32 newBalanceTo,
        bytes32 newTotalSupply
    );

    #[derive(Debug)]
    event Burn(
        address indexed caller,
        bytes32 balanceFrom,
        bytes32 amount,
        bytes32 totalSupply,
        bytes32 success,
        bytes32 newBalanceFrom,
        bytes32 newTotalSupply
    );
}

/// Parsed NOX event with strongly typed data
#[derive(Debug, Clone)]
pub enum NoxEvent {
    PlaintextToEncrypted(PlaintextToEncrypted),
    WrapPublicHandle(WrapPublicHandle),
    Add(Add),
    Sub(Sub),
    Mul(Mul),
    Div(Div),
    SafeAdd(SafeAdd),
    SafeSub(SafeSub),
    SafeMul(SafeMul),
    SafeDiv(SafeDiv),
    Eq(Eq),
    Ne(Ne),
    Ge(Ge),
    Gt(Gt),
    Le(Le),
    Lt(Lt),
    Select(Select),
    Transfer(Transfer),
    Mint(Mint),
    Burn(Burn),
}

impl NoxEvent {
    pub fn caller(&self) -> Address {
        match self {
            Self::PlaintextToEncrypted(e) => e.caller,
            Self::WrapPublicHandle(e) => e.caller,
            Self::Add(e) => e.caller,
            Self::Sub(e) => e.caller,
            Self::Mul(e) => e.caller,
            Self::Div(e) => e.caller,
            Self::SafeAdd(e) => e.caller,
            Self::SafeSub(e) => e.caller,
            Self::SafeMul(e) => e.caller,
            Self::SafeDiv(e) => e.caller,
            Self::Eq(e) => e.caller,
            Self::Ne(e) => e.caller,
            Self::Ge(e) => e.caller,
            Self::Gt(e) => e.caller,
            Self::Le(e) => e.caller,
            Self::Lt(e) => e.caller,
            Self::Select(e) => e.caller,
            Self::Transfer(e) => e.caller,
            Self::Mint(e) => e.caller,
            Self::Burn(e) => e.caller,
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
            WrapPublicHandle::SIGNATURE_HASH,
            Add::SIGNATURE_HASH,
            Sub::SIGNATURE_HASH,
            Mul::SIGNATURE_HASH,
            Div::SIGNATURE_HASH,
            SafeAdd::SIGNATURE_HASH,
            SafeSub::SIGNATURE_HASH,
            SafeMul::SIGNATURE_HASH,
            SafeDiv::SIGNATURE_HASH,
            Eq::SIGNATURE_HASH,
            Ne::SIGNATURE_HASH,
            Ge::SIGNATURE_HASH,
            Gt::SIGNATURE_HASH,
            Le::SIGNATURE_HASH,
            Lt::SIGNATURE_HASH,
            Select::SIGNATURE_HASH,
            Transfer::SIGNATURE_HASH,
            Mint::SIGNATURE_HASH,
            Burn::SIGNATURE_HASH,
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
            WrapPublicHandle::SIGNATURE_HASH => WrapPublicHandle::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::WrapPublicHandle(e.data)),
            Add::SIGNATURE_HASH => Add::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Add(e.data)),
            Sub::SIGNATURE_HASH => Sub::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Sub(e.data)),
            Mul::SIGNATURE_HASH => Mul::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Mul(e.data)),
            Div::SIGNATURE_HASH => Div::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Div(e.data)),
            SafeAdd::SIGNATURE_HASH => SafeAdd::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::SafeAdd(e.data)),
            SafeSub::SIGNATURE_HASH => SafeSub::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::SafeSub(e.data)),
            SafeMul::SIGNATURE_HASH => SafeMul::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::SafeMul(e.data)),
            SafeDiv::SIGNATURE_HASH => SafeDiv::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::SafeDiv(e.data)),
            Eq::SIGNATURE_HASH => Eq::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Eq(e.data)),
            Ne::SIGNATURE_HASH => Ne::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Ne(e.data)),
            Ge::SIGNATURE_HASH => Ge::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Ge(e.data)),
            Gt::SIGNATURE_HASH => Gt::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Gt(e.data)),
            Le::SIGNATURE_HASH => Le::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Le(e.data)),
            Lt::SIGNATURE_HASH => Lt::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Lt(e.data)),
            Select::SIGNATURE_HASH => Select::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Select(e.data)),
            Transfer::SIGNATURE_HASH => Transfer::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Transfer(e.data)),
            Mint::SIGNATURE_HASH => Mint::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Mint(e.data)),
            Burn::SIGNATURE_HASH => Burn::decode_log(&log.inner)
                .ok()
                .map(|e| NoxEvent::Burn(e.data)),
            _ => {
                debug!("Unknown event type: {:?}", topic0);
                None
            }
        }?;

        Some(event)
    }
}

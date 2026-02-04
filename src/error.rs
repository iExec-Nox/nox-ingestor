//! Error types for nox-events

use thiserror::Error;

/// Main error type for nox-events
#[derive(Error, Debug)]
pub enum NoxError {
    #[error("Chain error: {0}")]
    Chain(#[from] ChainError),

    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("No persisted state and initial_block=0. Set INITIAL_BLOCK to avoid missing events.")]
    NoInitialBlock,
}

/// Chain/RPC related errors
#[derive(Error, Debug)]
pub enum ChainError {
    #[error("Invalid RPC endpoint: {0}")]
    InvalidEndpoint(String),

    #[error("Provider error: {0}")]
    Provider(#[from] alloy::transports::TransportError),
}

/// State persistence errors
#[derive(Error, Debug)]
pub enum StateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Chain ID mismatch: expected {expected}, got {actual}")]
    ChainIdMismatch { expected: u32, actual: u32 },

    #[error("NATS error: {0}")]
    Nats(#[from] NatsError),
}

/// NATS related errors
#[derive(Error, Debug)]
pub enum NatsError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Publish error: {0}")]
    Publish(String),

    #[error("Stream setup error: {0}")]
    StreamSetup(String),
}

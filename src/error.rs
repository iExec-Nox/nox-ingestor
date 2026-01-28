//! Error types for nox-events

use thiserror::Error;

/// Main error type for nox-events
#[derive(Error, Debug)]
pub enum NoxError {
    #[error("Chain error: {0}")]
    Chain(#[from] ChainError),
}

/// Chain/RPC related errors
#[derive(Error, Debug)]
pub enum ChainError {
    #[error("Invalid RPC endpoint: {0}")]
    InvalidEndpoint(String),

    #[error("Provider error: {0}")]
    Provider(#[from] alloy::transports::TransportError),
}

//! RPC client wrapper using alloy

use alloy::primitives::{Address, B256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::{BlockNumberOrTag, Filter, Log};
use std::sync::Arc;
use tracing::info;

use crate::error::ChainError;

/// Chain client wrapping alloy HTTP provider
pub struct ChainClient {
    primary_provider: Arc<dyn Provider + Send + Sync>,
    contract_address: Address,
    event_signatures: Vec<B256>,
}

impl ChainClient {
    /// Create a new chain client
    pub fn new(
        rpc_endpoint: &str,
        contract_address: Address,
        event_signatures: Vec<B256>,
    ) -> Result<Self, ChainError> {
        let primary_url = rpc_endpoint
            .parse()
            .map_err(|e| ChainError::InvalidEndpoint(format!("{}: {}", rpc_endpoint, e)))?;

        let primary_provider = ProviderBuilder::new().connect_http(primary_url);

        info!(
            primary = %rpc_endpoint,
            "ChainClient initialized"
        );

        Ok(Self {
            primary_provider: Arc::new(primary_provider),
            contract_address,
            event_signatures,
        })
    }

    /// Get the latest block number
    pub async fn get_latest_block(&self) -> Result<u64, ChainError> {
        self.primary_provider
            .get_block_number()
            .await
            .map_err(Into::into)
    }

    /// Fetch logs for a range of blocks
    pub async fn get_logs(&self, from_block: u64, to_block: u64) -> Result<Vec<Log>, ChainError> {
        let filter = Filter::new()
            .address(self.contract_address)
            .event_signature(self.event_signatures.clone())
            .from_block(BlockNumberOrTag::Number(from_block))
            .to_block(BlockNumberOrTag::Number(to_block));

        self.primary_provider
            .get_logs(&filter)
            .await
            .map_err(Into::into)
    }
}

//! RPC client wrapper using alloy

use alloy::{
    primitives::{Address, B256},
    providers::{Provider, ProviderBuilder},
    rpc::types::{BlockNumberOrTag, Filter, Log},
};
use axum_prometheus::metrics::counter;
use std::future::Future;
use std::sync::Arc;
use tracing::{info, warn};

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

    /// Fetch logs for a range of blocks, bisecting the range whenever the provider rejects it
    /// for returning too many logs (see [`ChainError::is_log_response_too_large`]). This makes
    /// progress even when a permissionless flood of matching events packs more logs into the
    /// range than the provider allows, instead of failing the whole range every time.
    pub async fn get_logs_split_on_error(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<Log>, ChainError> {
        split_logs_on_error(from_block, to_block, |from, to| self.get_logs(from, to)).await
    }
}

/// Fetches logs for `[from_block, to_block]` via `fetch`, bisecting the range and retrying the
/// halves whenever `fetch` reports the provider's log-count cap was exceeded. Iterative (a work
/// stack), not recursive: a range bisects to width 1 in at most ~log2(range) steps, not worth
/// boxing futures for.
async fn split_logs_on_error<F, Fut>(
    from_block: u64,
    to_block: u64,
    mut fetch: F,
) -> Result<Vec<Log>, ChainError>
where
    F: FnMut(u64, u64) -> Fut,
    Fut: Future<Output = Result<Vec<Log>, ChainError>>,
{
    let mut logs = Vec::new();
    let mut stack = vec![(from_block, to_block)];
    while let Some((from, to)) = stack.pop() {
        match fetch(from, to).await {
            Ok(mut batch) => logs.append(&mut batch),
            Err(e) if e.is_log_response_too_large() && from < to => {
                let mid = from + (to - from) / 2;
                counter!("nox_ingestor_chain_log_range_splits_total").increment(1);
                // Pushed in this order so the left half is popped (and thus fetched) first.
                stack.push((mid + 1, to));
                stack.push((from, mid));
            }
            Err(e) if e.is_log_response_too_large() => {
                // Irreducible: a single block still exceeds the provider's cap. Propagate so the
                // caller's own retry loop keeps retrying — never drop events — but tag it
                // separately from a generic error so operators can see this specific condition.
                counter!("nox_ingestor_chain_log_range_irreducible_errors_total").increment(1);
                warn!(
                    block = from,
                    "single block log count still exceeds provider limit"
                );
                return Err(e);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(logs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::rpc::json_rpc::ErrorPayload;
    use alloy::transports::{RpcError, TransportErrorKind};
    use std::cell::Cell;

    fn too_large() -> ChainError {
        ChainError::Provider(RpcError::ErrorResp(ErrorPayload {
            code: -32005,
            message: "query returned more than 10000 results".to_string().into(),
            data: None,
        }))
    }

    fn other_error() -> ChainError {
        ChainError::Provider(RpcError::Transport(TransportErrorKind::BackendGone))
    }

    fn log_for_block(block_number: u64) -> Log {
        Log {
            block_number: Some(block_number),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn succeeds_without_splitting_when_whole_range_fits() {
        let calls = Cell::new(0u32);
        let result = split_logs_on_error(10, 20, |from, _to| {
            calls.set(calls.get() + 1);
            async move { Ok(vec![log_for_block(from)]) }
        })
        .await
        .unwrap();

        assert_eq!(calls.get(), 1);
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn splits_once_when_whole_range_is_too_large() {
        let calls = Cell::new(0u32);
        let result = split_logs_on_error(10, 11, |from, to| {
            calls.set(calls.get() + 1);
            async move {
                if from == to {
                    Ok(vec![log_for_block(from)])
                } else {
                    Err(too_large())
                }
            }
        })
        .await
        .unwrap();

        // Whole range failed once, then both width-1 halves succeeded.
        assert_eq!(calls.get(), 3);
        let mut blocks: Vec<u64> = result.iter().filter_map(|l| l.block_number).collect();
        blocks.sort();
        assert_eq!(blocks, vec![10, 11]);
    }

    #[tokio::test]
    async fn recurses_multiple_levels_and_partitions_the_range_exactly() {
        // Fails unless the sub-range is narrower than 4 blocks.
        let result = split_logs_on_error(10, 25, |from, to| async move {
            if to - from < 4 {
                Ok((from..=to).map(log_for_block).collect())
            } else {
                Err(too_large())
            }
        })
        .await
        .unwrap();

        let mut blocks: Vec<u64> = result.iter().filter_map(|l| l.block_number).collect();
        blocks.sort();
        assert_eq!(blocks, (10..=25).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn irreducible_single_block_propagates_without_further_splitting() {
        let calls = Cell::new(0u32);
        let result = split_logs_on_error(5, 5, |_from, _to| {
            calls.set(calls.get() + 1);
            async { Err(too_large()) }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn non_log_cap_error_propagates_immediately_without_splitting() {
        let calls = Cell::new(0u32);
        let result = split_logs_on_error(10, 20, |_from, _to| {
            calls.set(calls.get() + 1);
            async { Err(other_error()) }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }
}

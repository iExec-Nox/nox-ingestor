//! RPC client wrapper using alloy

use alloy::{
    network::BlockResponse,
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
        split_logs_on_error(
            from_block,
            to_block,
            |from, to| self.get_logs(from, to),
            |block| self.get_logs_via_transaction_receipts(block),
        )
        .await
    }

    /// Fetch this contract's matching logs for a single block by walking that block's
    /// transactions one receipt at a time, filtering client-side by contract address and topic0.
    ///
    /// Used as a fallback when even one block's worth of matching logs exceeds the provider's
    /// `eth_getLogs` cap. Fetching per transaction keeps each response small — the same
    /// subdivision strategy the block-range bisection uses, applied one level further down — so
    /// it clears response-*size* caps that a whole-block fetch would still trip, since a
    /// whole-block response is necessarily larger than the filtered `eth_getLogs` one that just
    /// failed. It also depends only on `eth_getBlockByNumber` and `eth_getTransactionReceipt`,
    /// core methods every provider implements, rather than the later-standardised
    /// `eth_getBlockReceipts`, which some providers still do not serve.
    ///
    /// Log-to-emitting-contract attribution is set by the EVM when the `LOG` opcode executes,
    /// regardless of call depth, so this captures events emitted by `NoxCompute` even when
    /// reached via `caller -> application contract -> NoxCompute` — the same semantics
    /// [`ChainClient::get_logs`]'s server-side address filter relies on for the normal path.
    ///
    /// Every "nothing found" outcome is an error rather than an empty success. The caller only
    /// reaches this fallback because `eth_getLogs` just reported *this exact block* as holding
    /// too many matching logs, so a missing block, a missing receipt, or an empty filter result
    /// all mean something is wrong (a lagging replica, a reorg, a filter bug) — never that the
    /// block is genuinely empty. Returning `Ok(vec![])` would let the caller advance its cursor
    /// past those events and drop them permanently.
    async fn get_logs_via_transaction_receipts(
        &self,
        block_number: u64,
    ) -> Result<Vec<Log>, ChainError> {
        let block = self
            .primary_provider
            .get_block_by_number(BlockNumberOrTag::Number(block_number))
            .await?
            .ok_or(ChainError::MissingBlock(block_number))?;
        // Hashes-only is `get_block_by_number`'s default, keeping this response small even for
        // a block packed with transactions.
        let tx_hashes: Vec<B256> = block.transactions().hashes().collect();

        // Sequential on purpose: this path only runs on an already-degraded block, where not
        // stampeding the provider matters more than latency.
        let mut logs = Vec::new();
        for tx_hash in tx_hashes {
            let receipt = match self.primary_provider.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => receipt,
                Ok(None) => return Err(ChainError::MissingTransactionReceipt(tx_hash)),
                Err(e) => {
                    let error = ChainError::from(e);
                    // A single transaction is the floor for this strategy: no standard RPC
                    // method returns part of a receipt, so name the transaction explicitly
                    // rather than reporting the whole block as irreducible.
                    return Err(if error.is_log_response_too_large() {
                        ChainError::TransactionLogsTooLarge(tx_hash)
                    } else {
                        error
                    });
                }
            };

            logs.extend(matching_logs(
                receipt.inner.into_logs(),
                self.contract_address,
                &self.event_signatures,
            ));
        }

        if logs.is_empty() {
            return Err(ChainError::NoMatchingLogsInBlock(block_number));
        }

        Ok(logs)
    }
}

/// Keeps only the logs emitted by `contract_address` whose topic0 is one of `event_signatures`.
///
/// The client-side equivalent of the server-side filter [`ChainClient::get_logs`] builds, for
/// the fallback path that fetches receipts unfiltered. Logs with no topics at all (anonymous
/// events) never match, since they carry no topic0 to compare.
fn matching_logs(logs: Vec<Log>, contract_address: Address, event_signatures: &[B256]) -> Vec<Log> {
    logs.into_iter()
        .filter(|log| {
            log.address() == contract_address
                && log
                    .topics()
                    .first()
                    .is_some_and(|topic0| event_signatures.contains(topic0))
        })
        .collect()
}

/// Fetches logs for `[from_block, to_block]` via `fetch`, bisecting the range and retrying the
/// halves whenever `fetch` reports the provider's log-count cap was exceeded. Iterative (a work
/// stack), not recursive: a range bisects to width 1 in at most ~log2(range) steps, not worth
/// boxing futures for.
///
/// When a single block still exceeds the cap, `fetch_via_receipts` is tried once as a fallback
/// (see [`ChainClient::get_logs_via_transaction_receipts`]) before giving up on that block.
async fn split_logs_on_error<F, Fut, G, GFut>(
    from_block: u64,
    to_block: u64,
    mut fetch: F,
    mut fetch_via_receipts: G,
) -> Result<Vec<Log>, ChainError>
where
    F: FnMut(u64, u64) -> Fut,
    Fut: Future<Output = Result<Vec<Log>, ChainError>>,
    G: FnMut(u64) -> GFut,
    GFut: Future<Output = Result<Vec<Log>, ChainError>>,
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
                // A single block still exceeds the cap. Walk that block's transactions one
                // receipt at a time before giving up — each receipt is a smaller response than
                // the whole-block filtered query that just failed.
                warn!(
                    block = from,
                    "single block log count still exceeds provider limit; \
                     falling back to per-transaction receipts"
                );
                match fetch_via_receipts(from).await {
                    Ok(mut batch) => {
                        counter!(
                            "nox_ingestor_chain_tx_receipts_fallback_total",
                            "outcome" => "ok"
                        )
                        .increment(1);
                        logs.append(&mut batch);
                    }
                    Err(fallback_err) => {
                        // Irreducible: neither eth_getLogs nor the receipts fallback worked for
                        // this block. Propagate the *original* too-large error so the caller's
                        // own retry loop keeps retrying — never drop events — but tag it
                        // separately so operators can see this specific condition.
                        counter!(
                            "nox_ingestor_chain_tx_receipts_fallback_total",
                            "outcome" => "err"
                        )
                        .increment(1);
                        counter!("nox_ingestor_chain_log_range_irreducible_errors_total")
                            .increment(1);
                        warn!(
                            block = from,
                            error = %fallback_err,
                            "per-transaction receipts fallback also failed"
                        );
                        return Err(e);
                    }
                }
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

    /// A fallback closure standing in for "receipts fallback unavailable/not needed" — used by
    /// every test that isn't specifically exercising the fallback itself.
    async fn failing_fallback(_block: u64) -> Result<Vec<Log>, ChainError> {
        Err(other_error())
    }

    #[tokio::test]
    async fn succeeds_without_splitting_when_whole_range_fits() {
        let calls = Cell::new(0u32);
        let result = split_logs_on_error(
            10,
            20,
            |from, _to| {
                calls.set(calls.get() + 1);
                async move { Ok(vec![log_for_block(from)]) }
            },
            failing_fallback,
        )
        .await
        .unwrap();

        assert_eq!(calls.get(), 1);
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn splits_once_when_whole_range_is_too_large() {
        let calls = Cell::new(0u32);
        let result = split_logs_on_error(
            10,
            11,
            |from, to| {
                calls.set(calls.get() + 1);
                async move {
                    if from == to {
                        Ok(vec![log_for_block(from)])
                    } else {
                        Err(too_large())
                    }
                }
            },
            failing_fallback,
        )
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
        let result = split_logs_on_error(
            10,
            25,
            |from, to| async move {
                if to - from < 4 {
                    Ok((from..=to).map(log_for_block).collect())
                } else {
                    Err(too_large())
                }
            },
            failing_fallback,
        )
        .await
        .unwrap();

        let mut blocks: Vec<u64> = result.iter().filter_map(|l| l.block_number).collect();
        blocks.sort();
        assert_eq!(blocks, (10..=25).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn single_block_is_not_split_further_and_falls_back_exactly_once() {
        let calls = Cell::new(0u32);
        let fallback_calls = Cell::new(0u32);
        let result = split_logs_on_error(
            5,
            5,
            |_from, _to| {
                calls.set(calls.get() + 1);
                async { Err(too_large()) }
            },
            |_block| {
                fallback_calls.set(fallback_calls.get() + 1);
                async { Err(other_error()) }
            },
        )
        .await;

        assert!(result.is_err());
        // A width-1 range cannot bisect, so `fetch` runs once and the fallback runs once —
        // never a second `fetch` on the same block.
        assert_eq!(calls.get(), 1);
        assert_eq!(fallback_calls.get(), 1);
    }

    #[tokio::test]
    async fn non_log_cap_error_propagates_immediately_without_splitting() {
        let calls = Cell::new(0u32);
        let result = split_logs_on_error(
            10,
            20,
            |_from, _to| {
                calls.set(calls.get() + 1);
                async { Err(other_error()) }
            },
            failing_fallback,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn receipts_fallback_recovers_an_irreducible_block() {
        let fallback_calls = Cell::new(0u32);
        let result = split_logs_on_error(
            5,
            5,
            |_from, _to| async { Err(too_large()) },
            |block| {
                fallback_calls.set(fallback_calls.get() + 1);
                async move { Ok(vec![log_for_block(block)]) }
            },
        )
        .await
        .unwrap();

        assert_eq!(fallback_calls.get(), 1);
        assert_eq!(
            result
                .iter()
                .filter_map(|l| l.block_number)
                .collect::<Vec<_>>(),
            vec![5]
        );
    }

    #[tokio::test]
    async fn receipts_fallback_failure_still_propagates_the_original_too_large_error() {
        let result = split_logs_on_error(
            5,
            5,
            |_from, _to| async { Err(too_large()) },
            |_block| async { Err(other_error()) },
        )
        .await;

        let err = result.expect_err("both eth_getLogs and the fallback failed");
        // The error surfaced to the caller is the original too-large classification, not
        // whatever unrelated error the fallback happened to fail with.
        assert!(err.is_log_response_too_large());
    }

    // `matching_logs` reimplements client-side what `get_logs` asks the node to do server-side.
    // These assert the two agree, since a divergence silently drops real events on the
    // fallback path.

    const NOX: Address = Address::new([0x11; 20]);
    const OTHER_CONTRACT: Address = Address::new([0x22; 20]);
    const WRAP_SIG: B256 = B256::new([0xaa; 32]);
    const ADD_SIG: B256 = B256::new([0xbb; 32]);
    const UNKNOWN_SIG: B256 = B256::new([0xcc; 32]);

    fn log_with(address: Address, topics: Vec<B256>) -> Log {
        Log {
            inner: alloy::primitives::Log::new_unchecked(address, topics, Default::default()),
            ..Default::default()
        }
    }

    #[test]
    fn matching_logs_keeps_this_contracts_known_events() {
        let logs = vec![log_with(NOX, vec![WRAP_SIG]), log_with(NOX, vec![ADD_SIG])];

        let matched = matching_logs(logs, NOX, &[WRAP_SIG, ADD_SIG]);

        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn matching_logs_rejects_another_contract_emitting_the_same_signature() {
        // The decisive case for the `caller -> app contract -> NoxCompute` path: an unrelated
        // contract emitting an identically-named event must not be ingested as ours.
        let logs = vec![log_with(OTHER_CONTRACT, vec![WRAP_SIG])];

        assert!(matching_logs(logs, NOX, &[WRAP_SIG]).is_empty());
    }

    #[test]
    fn matching_logs_rejects_unknown_signatures_from_this_contract() {
        let logs = vec![log_with(NOX, vec![UNKNOWN_SIG])];

        assert!(matching_logs(logs, NOX, &[WRAP_SIG, ADD_SIG]).is_empty());
    }

    #[test]
    fn matching_logs_rejects_anonymous_events_carrying_no_topic0() {
        let logs = vec![log_with(NOX, vec![])];

        assert!(matching_logs(logs, NOX, &[WRAP_SIG]).is_empty());
    }

    #[test]
    fn matching_logs_matches_on_topic0_only_ignoring_indexed_arguments() {
        // Indexed args occupy topics 1..=3; only topic0 identifies the event.
        let logs = vec![log_with(NOX, vec![WRAP_SIG, UNKNOWN_SIG, UNKNOWN_SIG])];

        assert_eq!(matching_logs(logs, NOX, &[WRAP_SIG]).len(), 1);
    }

    #[test]
    fn matching_logs_preserves_input_order_of_kept_logs() {
        let logs = vec![
            log_with(NOX, vec![ADD_SIG]),
            log_with(OTHER_CONTRACT, vec![WRAP_SIG]),
            log_with(NOX, vec![WRAP_SIG]),
        ];

        let matched = matching_logs(logs, NOX, &[WRAP_SIG, ADD_SIG]);

        assert_eq!(
            matched
                .iter()
                .map(|log| log.topics()[0])
                .collect::<Vec<_>>(),
            vec![ADD_SIG, WRAP_SIG]
        );
    }
}

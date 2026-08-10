//! Error types for nox-events

use alloy::primitives::B256;
use alloy::transports::{RpcError, TransportErrorKind};
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

    #[error("block {0} not found while walking its transaction receipts")]
    MissingBlock(u64),

    #[error("no receipt for transaction {0}, in a block expected to contain matching logs")]
    MissingTransactionReceipt(B256),

    #[error(
        "transaction {0} alone exceeds the provider's log response limit and cannot be split further"
    )]
    TransactionLogsTooLarge(B256),

    #[error(
        "no matching logs in block {0}, which eth_getLogs reported as exceeding the log response limit"
    )]
    NoMatchingLogsInBlock(u64),
}

/// Substrings seen in real-world `eth_getLogs` "range/response too large" rejections across
/// RPC providers (Alchemy, Infura, QuickNode, Ankr, geth, erigon, ...). Wording is not
/// standardized, so this is matched against a lowercased message rather than any error code.
/// Adapted from eRPC's production classifier (`architecture/evm/error_normalizer.go` in
/// github.com/erpc/erpc), which iExec already runs in front of some deployments.
const LOG_RANGE_TOO_LARGE_MARKERS: &[&str] = &[
    "try with this block range",
    "block range is too wide",
    "this block range should work",
    "range too large",
    "exceeds the range",
    "max block range",
    "max range",
    "logs over more",
    "response size should not",
    "returned more than",
    "exceeds max results",
    "range is too large",
    "too large, max is",
    "response too large",
    "query exceeds limit",
    "limit the query to",
    "maximum block range",
    "range limit exceeded",
    "too many results",
    "try paginating",
    "eth_getlogs is limited",
];

impl ChainError {
    /// True if the provider rejected `eth_getLogs` because the requested range would return
    /// too many logs, rather than a transient/network failure. Deliberately does NOT key off
    /// JSON-RPC error codes: e.g. Infura reuses code `-32005` for both this AND an unrelated
    /// "exceeded project rate limit" rejection, distinguishable only by message text.
    pub(crate) fn is_log_response_too_large(&self) -> bool {
        let ChainError::Provider(err) = self else {
            return false;
        };
        match err {
            RpcError::ErrorResp(payload) => contains_log_range_marker(&payload.message),
            RpcError::Transport(TransportErrorKind::HttpError(http)) => {
                // 429 is a standardized (RFC 6585) rate-limit signal, never a range/size
                // rejection. Check it before any message-text match, unlike JSON-RPC error
                // codes (provider-defined, unreliable — see the -32005 note above): HTTP status
                // codes are not something individual providers get to redefine.
                if http.status == 429 {
                    return false;
                }
                http.status == 413 || contains_log_range_marker(&http.body)
            }
            _ => false,
        }
    }
}

fn contains_log_range_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // eRPC's list is a flat OR of the substrings above, plus one compound condition it
    // deliberately does NOT flatten into two separate OR entries: "maximum" alone is far too
    // generic (matches unrelated things like "maximum fee per gas"), so it only counts alongside
    // "blocks distance".
    LOG_RANGE_TOO_LARGE_MARKERS
        .iter()
        .any(|m| lower.contains(m))
        || (lower.contains("maximum") && lower.contains("blocks distance"))
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
    #[error("TLS configuration error: {0}")]
    Tls(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Disconnected")]
    Disconnected,

    #[error("Publish error: {0}")]
    Publish(String),

    #[error("Stream setup error: {0}")]
    StreamSetup(String),

    #[error("Buffer full: capacity {capacity}, cannot accept more messages")]
    BufferFull { capacity: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::rpc::json_rpc::ErrorPayload;
    use alloy::transports::HttpError;

    fn error_resp(code: i64, message: &str) -> ChainError {
        ChainError::Provider(RpcError::ErrorResp(ErrorPayload {
            code,
            message: message.to_string().into(),
            data: None,
        }))
    }

    fn http_error(status: u16, body: &str) -> ChainError {
        ChainError::Provider(RpcError::Transport(TransportErrorKind::HttpError(
            HttpError {
                status,
                body: body.to_string(),
            },
        )))
    }

    #[test]
    fn detects_infura_style_too_many_results() {
        let err = error_resp(
            -32005,
            "query returned more than 10000 results. Try with this block range [0x1, 0x100].",
        );
        assert!(err.is_log_response_too_large());
    }

    #[test]
    fn does_not_flag_infura_rate_limit_sharing_the_same_error_code() {
        let err = error_resp(-32005, "exceeded project rate limit");
        assert!(!err.is_log_response_too_large());
    }

    #[test]
    fn detects_alchemy_style_log_response_size_exceeded() {
        let err = error_resp(
            -32000,
            "Log response size exceeded. this block range should work: [0x1, 0x64]",
        );
        assert!(err.is_log_response_too_large());
    }

    #[test]
    fn detects_http_413() {
        assert!(http_error(413, "").is_log_response_too_large());
    }

    #[test]
    fn detects_marker_in_http_body_on_non_413_status() {
        assert!(http_error(400, "block range is too large").is_log_response_too_large());
    }

    #[test]
    fn does_not_flag_unrelated_errors() {
        assert!(!error_resp(-32601, "method not found").is_log_response_too_large());
        assert!(
            !ChainError::Provider(RpcError::Transport(TransportErrorKind::BackendGone))
                .is_log_response_too_large()
        );
    }

    #[test]
    fn detects_compound_maximum_blocks_distance_marker() {
        let err = error_resp(-32000, "the maximum blocks distance allowed is 5000");
        assert!(err.is_log_response_too_large());
    }

    #[test]
    fn does_not_flag_maximum_or_blocks_distance_alone() {
        assert!(!error_resp(-32000, "maximum fee per gas exceeded").is_log_response_too_large());
        assert!(!error_resp(-32000, "blocks distance mismatch").is_log_response_too_large());
    }

    #[test]
    fn http_429_is_never_treated_as_too_large_even_with_a_marker_in_the_body() {
        // A rate-limit response is a standardized HTTP-level signal (RFC 6585) that must win
        // over message text, since providers occasionally phrase rate limits in ways that
        // could otherwise coincidentally match a marker.
        assert!(!http_error(429, "too many results, please slow down").is_log_response_too_large());
    }
}

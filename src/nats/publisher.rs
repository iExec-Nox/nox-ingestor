//! Simple NATS JetStream publisher

use async_nats::HeaderMap;
use async_nats::jetstream::Context as JetStreamContext;
use std::sync::Arc;
use tracing::debug;

use crate::config::NatsConfig;
use crate::error::NatsError;
use crate::events::TransactionMessage;

/// Publisher for sending transaction messages to NATS JetStream
pub struct Publisher {
    jetstream: Arc<JetStreamContext>,
    subject_prefix: String,
}

impl Publisher {
    /// Create a new publisher
    pub fn new(jetstream: Arc<JetStreamContext>, config: &NatsConfig) -> Self {
        Self {
            jetstream,
            subject_prefix: config.subject.clone(),
        }
    }

    /// Publish a transaction message to JetStream
    pub async fn publish(&self, message: &TransactionMessage) -> Result<(), NatsError> {
        let subject = message.subject(&self.subject_prefix);
        let payload = message
            .to_bytes()
            .map_err(|e| NatsError::Publish(format!("Serialization error: {}", e)))?;

        // Use checksum as Nats-Msg-Id for deduplication
        let mut headers = HeaderMap::new();
        headers.insert("Nats-Msg-Id", message.compute_checksum().as_str());

        let ack = self
            .jetstream
            .publish_with_headers(subject.clone(), headers, payload.into())
            .await
            .map_err(|e| NatsError::Publish(format!("Publish error: {}", e)))?
            .await
            .map_err(|e| NatsError::Publish(format!("Ack error: {}", e)))?;

        debug!(
            subject,
            checksum = message.compute_checksum(),
            event_count = message.events.len(),
            seq = ack.sequence,
            duplicate = ack.duplicate,
            "Published transaction message"
        );

        Ok(())
    }
}

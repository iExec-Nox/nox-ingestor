//! NATS client with JetStream support

use async_nats::jetstream::{self, Context as JetStreamContext};
use async_nats::{ConnectOptions, Event};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::config::NatsConfig;
use crate::error::NatsError;

/// NATS client with JetStream support
pub struct NatsClient {
    jetstream: Arc<JetStreamContext>,
}

impl NatsClient {
    /// Connect to NATS server
    pub async fn connect(config: &NatsConfig) -> Result<Self, NatsError> {
        let options = ConnectOptions::new()
            .event_callback(move |event| async move {
                match event {
                    Event::Connected => {
                        info!("NATS connected");
                    }
                    Event::Disconnected => {
                        warn!("NATS disconnected");
                    }
                    Event::ServerError(err) => {
                        error!(error = %err, "NATS server error");
                    }
                    Event::ClientError(err) => {
                        error!(error = %err, "NATS client error");
                    }
                    Event::LameDuckMode => {
                        warn!("NATS server in lame duck mode");
                    }
                    Event::SlowConsumer(sid) => {
                        warn!(subscription_id = sid, "NATS slow consumer");
                    }
                    _ => {}
                }
            })
            .retry_on_initial_connect();

        info!(url = config.url, "Connecting to NATS...");

        let client = options.connect(&config.url).await.map_err(|e| {
            NatsError::Connection(format!("Failed to connect to {}: {}", config.url, e))
        })?;

        let jetstream = jetstream::new(client.clone());

        info!("NATS connected successfully");

        Ok(Self {
            jetstream: Arc::new(jetstream),
        })
    }

    /// Setup JetStream stream
    pub async fn setup_stream(&self, config: &NatsConfig) -> Result<(), NatsError> {
        info!(stream = config.stream_name, "Setting up JetStream stream");

        let stream_config = jetstream::stream::Config {
            name: config.stream_name.clone(),
            subjects: vec![format!("{}.>", config.subject)],
            retention: jetstream::stream::RetentionPolicy::Limits,
            max_age: Duration::from_secs(config.retention_seconds),
            storage: jetstream::stream::StorageType::File,
            num_replicas: 1,
            duplicate_window: Duration::from_secs(config.duplicate_window_secs),
            ..Default::default()
        };

        self.jetstream
            .get_or_create_stream(stream_config)
            .await
            .map_err(|e| NatsError::StreamSetup(format!("Failed to setup stream: {}", e)))?;

        info!(
            stream = config.stream_name,
            retention_secs = config.retention_seconds,
            duplicate_window_secs = config.duplicate_window_secs,
            "Stream ready"
        );

        Ok(())
    }

    /// Get the JetStream context
    pub fn jetstream(&self) -> Arc<JetStreamContext> {
        Arc::clone(&self.jetstream)
    }
}

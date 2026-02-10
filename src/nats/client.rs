//! NATS client with JetStream support

use async_nats::jetstream::{self, Context as JetStreamContext};
use async_nats::{ConnectOptions, Event};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::config::NatsConfig;
use crate::error::NatsError;

/// Connection state for NATS client
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionState::Connected => write!(f, "Connected"),
            ConnectionState::Disconnected => write!(f, "Disconnected"),
        }
    }
}

/// NATS client with JetStream support
pub struct NatsClient {
    jetstream: Arc<JetStreamContext>,
    state_rx: watch::Receiver<ConnectionState>,
}

impl NatsClient {
    /// Connect to NATS server
    pub async fn connect(config: &NatsConfig) -> Result<Self, NatsError> {
        let (state_tx, state_rx) = watch::channel(ConnectionState::Disconnected);

        let state_tx_clone = state_tx.clone();
        let options = ConnectOptions::new()
            .event_callback(move |event| {
                let state_tx = state_tx_clone.clone();
                async move {
                    match event {
                        Event::Connected => {
                            info!("NATS connected");
                            let _ = state_tx.send(ConnectionState::Connected);
                        }
                        Event::Disconnected => {
                            warn!("NATS disconnected");
                            let _ = state_tx.send(ConnectionState::Disconnected);
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
                }
            })
            .retry_on_initial_connect();

        info!(url = config.url, "Connecting to NATS...");

        let client = options.connect(&config.url).await.map_err(|e| {
            NatsError::Connection(format!("Failed to connect to {}: {}", config.url, e))
        })?;

        // Mark as connected
        let _ = state_tx.send(ConnectionState::Connected);

        let jetstream = jetstream::new(client.clone());

        info!("NATS connected successfully");

        Ok(Self {
            jetstream: Arc::new(jetstream),
            state_rx,
        })
    }

    /// Setup JetStream stream
    pub async fn setup_stream(&self, config: &NatsConfig) -> Result<(), NatsError> {
        info!(stream = config.stream_name, "Setting up JetStream stream");

        let stream_config = jetstream::stream::Config {
            name: config.stream_name.clone(),
            subjects: vec![format!("{}.>", config.subject)],
            retention: jetstream::stream::RetentionPolicy::Limits,
            max_age: config.retention,
            storage: jetstream::stream::StorageType::File,
            num_replicas: 1,
            duplicate_window: config.duplicate_window,
            ..Default::default()
        };

        self.jetstream
            .get_or_create_stream(stream_config)
            .await
            .map_err(|e| NatsError::StreamSetup(format!("Failed to setup stream: {}", e)))?;

        info!(
            stream = config.stream_name,
            retention = config.retention.as_secs(),
            duplicate_window = config.duplicate_window.as_secs(),
            "Stream ready"
        );

        Ok(())
    }

    /// Get the JetStream context
    pub fn jetstream(&self) -> Arc<JetStreamContext> {
        Arc::clone(&self.jetstream)
    }

    /// Get a receiver for connection state changes
    pub fn state_receiver(&self) -> watch::Receiver<ConnectionState> {
        self.state_rx.clone()
    }

    /// Get the current connection state
    pub fn state(&self) -> ConnectionState {
        *self.state_rx.borrow()
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.state() == ConnectionState::Connected
    }
}

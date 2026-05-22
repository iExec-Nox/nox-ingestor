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

        for (label, path) in [
            ("ca", &config.tls.ca_path),
            ("cert", &config.tls.cert_path),
            ("key", &config.tls.key_path),
        ] {
            if !path.is_file() {
                return Err(NatsError::Tls(format!(
                    "{label} path is not a regular file: {}",
                    path.display()
                )));
            }
        }

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
                        Event::ServerError(err) => error!(error = %err, "NATS server error"),
                        Event::ClientError(err) => error!(error = %err, "NATS client error"),
                        Event::LameDuckMode => warn!("NATS server in lame duck mode"),
                        Event::SlowConsumer(sid) => {
                            warn!(subscription_id = sid, "NATS slow consumer")
                        }
                        _ => {}
                    }
                }
            })
            .add_root_certificates(config.tls.ca_path.clone())
            .add_client_certificate(config.tls.cert_path.clone(), config.tls.key_path.clone())
            .require_tls(true)
            .retry_on_initial_connect();

        info!(
            urls = ?config.urls,
            "Connecting to NATS cluster via mTLS"
        );

        let client = options.connect(&config.urls[..]).await.map_err(|e| {
            NatsError::Connection(format!(
                "Failed to connect to NATS cluster {:?}: {}",
                config.urls, e
            ))
        })?;

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

        let configured = config.num_replicas as usize;
        let desired_config = jetstream::stream::Config {
            name: config.stream_name.clone(),
            subjects: vec![format!("{}.>", config.subject)],
            retention: jetstream::stream::RetentionPolicy::Limits,
            max_age: config.retention,
            storage: jetstream::stream::StorageType::File,
            num_replicas: configured,
            duplicate_window: config.duplicate_window,
            ..Default::default()
        };

        match self.jetstream.get_stream(&config.stream_name).await {
            Ok(stream) => {
                let stored = stream.cached_info().config.num_replicas;
                if stored != configured {
                    warn!(
                        stored,
                        configured,
                        stream = config.stream_name,
                        "JetStream stream replica count mismatch — admin (ops) must update online via `nats stream update --replicas={configured}` with a privileged identity"
                    );
                } else {
                    info!(
                        stream = config.stream_name,
                        replicas = stored,
                        "Stream already exists with correct replica count"
                    );
                }
                Ok(())
            }
            Err(e)
                if matches!(
                    e.kind(),
                    jetstream::context::GetStreamErrorKind::JetStream(ref js_err)
                    if js_err.error_code() == jetstream::ErrorCode::STREAM_NOT_FOUND
                ) =>
            {
                self.jetstream
                    .create_stream(desired_config)
                    .await
                    .map_err(|e| NatsError::StreamSetup(format!("Failed to create stream: {e}")))?;
                info!(
                    stream = config.stream_name,
                    replicas = configured,
                    "Stream created"
                );
                Ok(())
            }
            Err(e) => Err(NatsError::StreamSetup(format!(
                "Failed to query stream {}: {e}",
                config.stream_name
            ))),
        }
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

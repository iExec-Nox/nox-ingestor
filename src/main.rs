use opentelemetry::{KeyValue, global, trace::TracerProvider};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, propagation::TraceContextPropagator,
    trace::SdkTracerProvider,
};
use tracing::error;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::application::Application;
use crate::config::Config;

pub mod application;
pub mod chain;
pub mod config;
pub mod error;
pub mod events;
pub mod handlers;
pub mod nats;
mod observability;
pub mod state;

fn init_logs(url: &str, resource: Resource) -> anyhow::Result<SdkLoggerProvider> {
    let exporter = LogExporter::builder()
        .with_http()
        .with_endpoint(format!("{}/v1/logs", url))
        .with_protocol(Protocol::HttpBinary)
        .build()?;

    Ok(SdkLoggerProvider::builder()
        .with_simple_exporter(exporter)
        .with_resource(resource)
        .build())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    async_nats::rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls ring crypto provider");

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let config = Config::load().map_err(|e| {
        error!("Failed to load configuration: {e}");
        e
    })?;

    let tracing_registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer());

    if config.otel.enabled {
        let exporter = SpanExporter::builder()
            .with_http()
            .with_endpoint(format!("{}/v1/traces", config.otel.url))
            .with_protocol(Protocol::HttpBinary)
            .build()?;

        let resource = Resource::builder()
            .with_attribute(KeyValue::new("service.name", "nox-ingestor"))
            .build();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter)
            .with_resource(resource.clone())
            .build();

        global::set_text_map_propagator(TraceContextPropagator::new());
        global::set_tracer_provider(provider.clone());

        let telemetry_layer =
            tracing_opentelemetry::layer().with_tracer(provider.tracer("nox-ingestor"));

        let log_provider = init_logs(&config.otel.url, resource.clone())?;

        tracing_registry
            .with(telemetry_layer)
            .with(OpenTelemetryTracingBridge::new(&log_provider))
            .init();
    } else {
        tracing_registry.init();
    }

    let app = Application::new(config)?;
    app.run().await?;

    Ok(())
}

//! NATS client module for publishing events to JetStream

mod buffer;
mod client;
mod publisher;

pub use buffer::MessageBuffer;
pub use client::NatsClient;
pub use publisher::Publisher;

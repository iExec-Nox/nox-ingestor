//! Bounded message buffer for NATS publishing
//!
//! FIFO buffer that rejects new messages when full to ensure no events are lost silently.

use std::collections::VecDeque;
use tracing::warn;

use crate::error::NatsError;
use crate::events::TransactionMessage;

/// Maximum capacity of the buffer
const MAX_CAPACITY: usize = 1000;

/// Bounded message buffer
///
/// Stores messages while NATS is disconnected. Rejects new messages when full
/// to ensure the caller knows messages cannot be accepted (blocking behavior).
pub struct MessageBuffer {
    buffer: VecDeque<TransactionMessage>,
    capacity: usize,
}

impl MessageBuffer {
    /// Create a new buffer with the given capacity
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.min(MAX_CAPACITY);
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a message to the buffer
    ///
    /// Returns an error if the buffer is full.
    pub fn push(&mut self, message: TransactionMessage) -> Result<(), NatsError> {
        if self.is_full() {
            warn!(
                capacity = self.capacity,
                len = self.buffer.len(),
                "Buffer full, rejecting message"
            );
            return Err(NatsError::BufferFull {
                capacity: self.capacity,
            });
        }

        self.buffer.push_back(message);
        Ok(())
    }

    /// Pop the oldest message from the buffer (FIFO)
    pub fn pop(&mut self) -> Option<TransactionMessage> {
        self.buffer.pop_front()
    }

    /// Push a message to the front of the buffer
    ///
    /// Used to re-buffer messages that failed to publish, maintaining FIFO order.
    /// This bypasses capacity checks since we are re-adding previously buffered messages.
    pub fn push_front(&mut self, message: TransactionMessage) {
        self.buffer.push_front(message);
    }

    /// Extend the front of the buffer with multiple messages
    ///
    /// Messages are added in reverse order so that the first message in the iterator
    /// ends up at the front of the buffer, maintaining FIFO order.
    /// This bypasses capacity checks since we are re-adding previously buffered messages.
    pub fn extend_front(&mut self, messages: impl IntoIterator<Item = TransactionMessage>) {
        let messages: Vec<_> = messages.into_iter().collect();
        for message in messages.into_iter().rev() {
            self.buffer.push_front(message);
        }
    }

    /// Get the number of messages in the buffer
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Check if the buffer is full
    pub fn is_full(&self) -> bool {
        self.buffer.len() >= self.capacity
    }

    /// Get the buffer capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Drain all messages from the buffer
    pub fn drain(&mut self) -> impl Iterator<Item = TransactionMessage> + '_ {
        self.buffer.drain(..)
    }
}

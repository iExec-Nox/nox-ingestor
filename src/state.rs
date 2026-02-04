//! State persistence for the event listener
//!
//! Tracks the last processed block and persists it to disk atomically.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

use crate::error::StateError;

/// Persisted state format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub last_block: u64,
    pub chain_id: u32,
    pub updated_at: u64,
}

/// State store for tracking last synced block.
pub struct StateStore {
    path: PathBuf,
    chain_id: u32,
    last_synced: AtomicU64,
    from_file: bool,
}

impl std::fmt::Debug for StateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateStore")
            .field("path", &self.path)
            .field("chain_id", &self.chain_id)
            .field("last_synced", &self.last_synced.load(Ordering::Relaxed))
            .field("from_file", &self.from_file)
            .finish()
    }
}

impl StateStore {
    /// Load state from file or create new with initial_block.
    ///
    /// # Errors
    ///
    /// Returns `StateError::ChainIdMismatch` if the state file exists but
    /// contains a different chain ID.
    pub async fn load(
        path: PathBuf,
        chain_id: u32,
        initial_block: u64,
    ) -> Result<Self, StateError> {
        // Read directly to avoid TOCTOU race condition
        match fs::read_to_string(&path).await {
            Ok(content) => match serde_json::from_str::<PersistedState>(&content) {
                Ok(state) => {
                    if state.chain_id != chain_id {
                        return Err(StateError::ChainIdMismatch {
                            expected: chain_id,
                            actual: state.chain_id,
                        });
                    }

                    info!(
                        path = %path.display(),
                        last_block = state.last_block,
                        "Loaded state from file"
                    );

                    Ok(Self {
                        path,
                        chain_id,
                        last_synced: AtomicU64::new(state.last_block),
                        from_file: true,
                    })
                }
                Err(e) => {
                    warn!(error = %e, "Failed to parse state file, starting fresh");
                    Ok(Self::new_fresh(path, chain_id, initial_block))
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    path = %path.display(),
                    initial_block,
                    "No existing state file, using initial_block"
                );
                Ok(Self::new_fresh(path, chain_id, initial_block))
            }
            Err(e) => {
                warn!(error = %e, "Failed to read state file, starting fresh");
                Ok(Self::new_fresh(path, chain_id, initial_block))
            }
        }
    }

    fn new_fresh(path: PathBuf, chain_id: u32, initial_block: u64) -> Self {
        Self {
            path,
            chain_id,
            last_synced: AtomicU64::new(initial_block),
            from_file: false,
        }
    }

    /// Update the last synced block (atomic, takes max of current and new)
    pub fn update(&self, block: u64) {
        self.last_synced.fetch_max(block, Ordering::Release);
    }

    /// Get the current last synced block
    pub fn get(&self) -> u64 {
        self.last_synced.load(Ordering::Acquire)
    }

    /// Check if state was loaded from an existing file
    pub fn was_loaded_from_file(&self) -> bool {
        self.from_file
    }

    /// Persist state to disk atomically (tmp + fsync + rename + dir sync)
    pub async fn persist(&self) -> Result<(), StateError> {
        let last_block = self.get();
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System clock is before UNIX epoch")
            .as_millis() as u64;

        let state = PersistedState {
            last_block,
            chain_id: self.chain_id,
            updated_at,
        };

        let json = serde_json::to_string_pretty(&state)?;
        let tmp_path = self.path.with_extension("json.tmp");

        // Write to temp file with cleanup on error
        let write_result = async {
            let mut file = fs::File::create(&tmp_path).await?;
            file.write_all(json.as_bytes()).await?;
            file.sync_all().await?;
            drop(file);
            fs::rename(&tmp_path, &self.path).await
        }
        .await;

        if let Err(e) = write_result {
            // Cleanup temp file on error (best effort)
            let _ = fs::remove_file(&tmp_path).await;
            return Err(e.into());
        }

        // Sync parent directory to ensure rename is durable
        if let Some(parent) = self.path.parent()
            && let Ok(dir) = fs::File::open(parent).await
        {
            let _ = dir.sync_all().await;
        }

        debug!(
            last_block,
            path = %self.path.display(),
            "Persisted state"
        );

        Ok(())
    }
}

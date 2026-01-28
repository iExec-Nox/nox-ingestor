use anyhow::Result;
use tracing::debug;

use crate::config::Config;
pub struct Application {
    config: Config,
}

impl Application {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self { config })
    }

    pub async fn run(self) -> Result<()> {
        debug!("Starting ingestor");
        debug!("Config: {:?}", self.config);
        Ok(())
    }
}

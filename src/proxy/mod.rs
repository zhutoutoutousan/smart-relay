pub mod xray_check;
pub mod core_download;
pub mod xray_adapter;

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub endpoint: String,
    pub latency_ms: u128,
    pub packet_loss: f32,
}

/// Abstraction over different core engines (xray, sing-box, etc.)
pub trait ProxyAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn configure(&self, serialized: &str) -> anyhow::Result<()>;
    fn start(&self) -> anyhow::Result<()>;
    fn stop(&self) -> anyhow::Result<()>;
    fn probe(&self, timeout: Duration) -> anyhow::Result<ProbeResult>;
}

/// Placeholder adapter until real core integration lands.
pub struct NullAdapter;

impl ProxyAdapter for NullAdapter {
    fn name(&self) -> &'static str {
        "null-adapter"
    }

    fn configure(&self, _serialized: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn probe(&self, _timeout: Duration) -> anyhow::Result<ProbeResult> {
        Ok(ProbeResult {
            endpoint: "none".into(),
            latency_ms: 0,
            packet_loss: 0.0,
        })
    }
}


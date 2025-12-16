use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyEndpoint {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub protocol: ProxyProtocol,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyProtocol {
    Vless,
    Vmess,
    Shadowsocks,
    Trojan,
    Socks5,
}

impl Default for ProxyProtocol {
    fn default() -> Self {
        ProxyProtocol::Vmess
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutingRule {
    pub name: String,
    pub destination_cidrs: Vec<String>,
    pub outbound: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SmartConfig {
    pub endpoints: Vec<ProxyEndpoint>,
    pub rules: Vec<RoutingRule>,
}

#[derive(Clone)]
pub struct ConfigStore {
    pub path: PathBuf,
    inner: Arc<RwLock<SmartConfig>>,
}

impl ConfigStore {
    pub async fn load(path: Option<&str>) -> Result<Self> {
        let resolved = match path {
            Some(p) => PathBuf::from(p),
            None => default_path()?,
        };

        let cfg = if resolved.exists() {
            let data = fs::read_to_string(&resolved)
                .with_context(|| format!("reading config from {}", resolved.display()))?;
            toml::from_str(&data)?
        } else {
            SmartConfig::default()
        };

        Ok(Self {
            path: resolved,
            inner: Arc::new(RwLock::new(cfg)),
        })
    }

    pub async fn save(&self) -> Result<()> {
        let data = {
            let guard = self.inner.read().await;
            toml::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, data)?;
        Ok(())
    }

    /// Helper for tests or injected configs.
    pub fn from_config(cfg: SmartConfig, path: PathBuf) -> Self {
        Self {
            path,
            inner: Arc::new(RwLock::new(cfg)),
        }
    }

    pub async fn get(&self) -> SmartConfig {
        self.inner.read().await.clone()
    }

    pub async fn add_endpoint(&self, endpoint: ProxyEndpoint) -> Result<()> {
        let mut guard = self.inner.write().await;
        guard.endpoints.push(endpoint);
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let guard = self
            .inner
            .try_read()
            .map_err(|_| anyhow!("config is currently mutating"))?;
        if guard.endpoints.is_empty() {
            tracing::warn!("no proxy endpoints configured yet");
        }
        for ep in &guard.endpoints {
            if ep.host.is_empty() {
                return Err(anyhow!("endpoint {} missing host", ep.name));
            }
        }
        Ok(())
    }
}

fn default_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "smart-relay", "smart-relay")
        .ok_or_else(|| anyhow!("cannot resolve config directory"))?;
    Ok(dirs.config_dir().join("config.toml"))
}


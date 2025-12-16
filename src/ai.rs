use crate::config::{ProxyEndpoint, SmartConfig};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use uuid::Uuid;

#[derive(Clone)]
pub struct AiClient {
    client: Client,
    base: String,
    model: String,
}

#[derive(Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

impl Default for AiClient {
    fn default() -> Self {
        let base = env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".into());
        let model = env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3".into());
        Self {
            client: Client::new(),
            base,
            model,
        }
    }
}

impl AiClient {
    pub async fn health(&self) -> Result<()> {
        let url = format!("{}/api/tags", self.base.trim_end_matches('/'));
        self.client
            .get(url)
            .send()
            .await
            .context("pinging ollama")?
            .error_for_status()
            .context("ollama returned error status")?;
        Ok(())
    }

    pub async fn propose_profile(
        &self,
        goal: &str,
        snapshot: Option<SmartConfig>,
    ) -> Result<String> {
        let prompt = build_prompt(goal, snapshot);
        let url = format!("{}/api/generate", self.base.trim_end_matches('/'));
        let body = GenerateRequest {
            model: self.model.clone(),
            prompt,
        };
        let resp: GenerateResponse = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .context("sending prompt to ollama")?
            .json()
            .await
            .context("reading ollama response")?;
        Ok(resp.response.trim().to_string())
    }

    pub async fn propose_endpoint(&self, region: &str) -> Result<ProxyEndpoint> {
        let template = format!(
            "Generate a single proxy endpoint for region {region}. \
             Use host placeholder, port 443, protocol vmess."
        );
        let raw = self.propose_profile(&template, None).await?;
        Ok(ProxyEndpoint {
            id: Uuid::new_v4(),
            name: format!("ai-{region}"),
            host: extract_host(&raw).unwrap_or_else(|| "example.com".into()),
            port: 443,
            protocol: crate::config::ProxyProtocol::Vmess,
            tags: vec!["ai".into(), region.into()],
        })
    }
}

fn build_prompt(goal: &str, snapshot: Option<SmartConfig>) -> String {
    let mut prompt = format!(
        "You are a networking expert designing a proxy profile.\n\
         Goal: {goal}\n\
         Output a concise plan with endpoints, routing rules and risk notes."
    );
    if let Some(cfg) = snapshot {
        prompt.push_str("\nCurrent endpoints:\n");
        for ep in cfg.endpoints {
            prompt.push_str(&format!(
                "- {} {}:{} {:?}\n",
                ep.name, ep.host, ep.port, ep.protocol
            ));
        }
    }
    prompt
}

fn extract_host(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|tok| tok.contains('.'))
        .map(|s| s.trim_matches(|c: char| c == ',' || c == '.').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_goal_and_endpoints() {
        let cfg = SmartConfig {
            endpoints: vec![ProxyEndpoint {
                id: Uuid::nil(),
                name: "test".into(),
                host: "example.com".into(),
                port: 443,
                protocol: crate::config::ProxyProtocol::Vmess,
                tags: vec![],
            }],
            rules: vec![],
        };
        let prompt = build_prompt("fast", Some(cfg));
        assert!(prompt.contains("fast"));
        assert!(prompt.contains("example.com"));
    }
}


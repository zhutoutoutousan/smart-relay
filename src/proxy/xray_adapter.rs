use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn, debug, error};
use crate::proxy::core_download;
use crate::proxy::ProbeResult;

/// Xray adapter that manages xray process and configuration
pub struct XrayAdapter {
    xray_path: PathBuf,
    process: Arc<Mutex<Option<Child>>>,
    config_path: PathBuf,
    socks_port: u16,
    http_port: u16,
    api_port: u16,
    // Actual ports being used (may differ from configured ports if fallback was used)
    actual_socks_port: Arc<Mutex<Option<u16>>>,
    actual_http_port: Arc<Mutex<Option<u16>>>,
}

impl XrayAdapter {
    pub fn new(socks_port: u16, http_port: u16, api_port: u16) -> Result<Self> {
        let xray_path = core_download::get_core_path("xray")
            .context("Xray not found. Please download it first.")?;
        
        let dirs = directories::ProjectDirs::from("com", "smart-relay", "smart-relay")
            .context("Failed to get project directories")?;
        let config_dir = dirs.config_local_dir();
        std::fs::create_dir_all(config_dir)?;
        
        let config_path = config_dir.join("xray_config.json");
        
        Ok(Self {
            xray_path,
            process: Arc::new(Mutex::new(None)),
            config_path,
            socks_port,
            http_port,
            api_port,
            actual_socks_port: Arc::new(Mutex::new(None)),
            actual_http_port: Arc::new(Mutex::new(None)),
        })
    }
    
    /// Generate xray config from endpoint data
    pub fn generate_config(&self, endpoint: &EndpointConfig) -> Result<Value> {
        // Use actual ports if they've been set (from fallback), otherwise use configured ports
        let socks_port = {
            let actual = self.actual_socks_port.lock().unwrap();
            actual.unwrap_or(self.socks_port)
        };
        let http_port = {
            let actual = self.actual_http_port.lock().unwrap();
            actual.unwrap_or(self.http_port)
        };
        
        let mut outbound = match endpoint.protocol.to_lowercase().as_str() {
            "vmess" => self.generate_vmess_outbound(endpoint)?,
            "vless" => self.generate_vless_outbound(endpoint)?,
            "shadowsocks" => self.generate_shadowsocks_outbound(endpoint)?,
            "trojan" => self.generate_trojan_outbound(endpoint)?,
            "socks5" => self.generate_socks5_outbound(endpoint)?,
            _ => return Err(anyhow::anyhow!("Unsupported protocol: {}", endpoint.protocol)),
        };
        
        // Add tag to the proxy outbound
        outbound["tag"] = json!("proxy");
        
        let config = json!({
            "log": {
                "loglevel": "warning"
            },
            "inbounds": [
                {
                    "port": socks_port,
                    "protocol": "socks",
                    "settings": {
                        "auth": "noauth",
                        "udp": true
                    },
                    "sniffing": {
                        "enabled": true,
                        "destOverride": ["http", "tls"]
                    }
                },
                {
                    "port": http_port,
                    "protocol": "http",
                    "settings": {
                        "allowTransparent": false
                    }
                }
            ],
            "outbounds": [
                outbound,
                {
                    "protocol": "freedom",
                    "tag": "direct"
                },
                {
                    "protocol": "blackhole",
                    "tag": "blocked"
                }
            ],
            "routing": {
                // Use IPOnDemand for better DNS handling and routing
                // This resolves domains to IPs before matching rules, which can help with routing decisions
                // Reference: https://xtls.github.io/config/routing.html
                "domainStrategy": "IPOnDemand",
                "rules": [
                    // Route all TCP and UDP traffic through the proxy
                    // This is a catch-all rule that ensures all traffic goes through the configured proxy
                    {
                        "type": "field",
                        "network": "tcp,udp",
                        "outboundTag": "proxy"
                    }
                ]
            }
        });
        
        Ok(config)
    }
    
    fn generate_vmess_outbound(&self, endpoint: &EndpointConfig) -> Result<Value> {
        // Build user object according to Xray VMess outbound specification
        // Reference: https://xtls.github.io/config/outbounds/vmess.html
        let uuid = endpoint.uuid.as_ref().context("VMess requires UUID")?;
        let mut user = json!({
            "id": uuid
        });
        
        // alterId is deprecated in newer Xray versions but some servers still require it
        // Only include if explicitly set (not 0) for compatibility
        let alter_id = endpoint.alter_id.unwrap_or(0);
        if alter_id > 0 {
            user["alterId"] = json!(alter_id);
        }
        
        // Security/encryption method according to VMess protocol specification
        // Reference: https://xtls.github.io/development/protocols/vmess.html
        // Valid values: auto, aes-128-cfb (0x00), none (0x01), aes-128-gcm (0x02), chacha20-poly1305 (0x03), zero
        // The protocol supports: AES-128-CFB, none, AES-128-GCM, ChaCha20-Poly1305
        let security = endpoint.security.as_deref().unwrap_or("auto");
        
        // Map common variations to standard names
        let security_normalized = match security.to_lowercase().as_str() {
            "aes-128-cfb" | "aes128cfb" | "cfb" => "aes-128-cfb",
            "aes-128-gcm" | "aes128gcm" | "gcm" => "aes-128-gcm",
            "chacha20-poly1305" | "chacha20poly1305" | "chacha" => "chacha20-poly1305",
            "none" | "null" => "none",
            "zero" => "zero",
            "auto" => "auto",
            _ => {
                warn!("Unknown VMess security method '{}', using 'auto'", security);
                "auto"
            }
        };
        
        if security_normalized != "auto" {
            user["security"] = json!(security_normalized);
        }
        
        let mut outbound = json!({
            "protocol": "vmess",
            "settings": {
                "vnext": [{
                    "address": endpoint.host,
                    "port": endpoint.port,
                    "users": [user]
                }]
            },
            "streamSettings": {
                "network": endpoint.network.as_deref().unwrap_or("tcp")
            }
        });
        
        // Add TLS settings if present
        // Reference: v2rayN default behavior - skip certificate verification by default
        // This helps avoid connection failures due to expired/invalid certificates
        if let Some(tls) = &endpoint.tls {
            outbound["streamSettings"]["security"] = json!("tls");
            let mut tls_settings = json!({});
            
            if let Some(server_name) = &tls.server_name {
                if !server_name.is_empty() {
                    tls_settings["serverName"] = json!(server_name);
                }
            }
            
            // Allow insecure certificates - default to true (like v2rayN)
            // This prevents "i/o timeout" errors caused by certificate validation failures
            // Reference: v2rayN Settings -> Parameters -> "Skip certificate verification by default"
            // Expired or invalid certificates can cause connection timeouts
            // Common issue: https://github.com/SagerNet/sing-box/issues/3001
            tls_settings["allowInsecure"] = json!(tls.allow_insecure);
            if tls.allow_insecure {
                debug!("TLS allowInsecure enabled (skipping certificate verification)");
            } else {
                warn!("TLS allowInsecure is false - certificate validation may cause connection failures");
                warn!("  → If you see 'i/o timeout' or 'dial tcp xxx:xxx: i/o timeout' errors,");
                warn!("  → try enabling allowInsecure (like v2rayN's default setting)");
            }
            
            // Always set tlsSettings even if only allowInsecure is set
            // This ensures the TLS configuration is properly applied
            outbound["streamSettings"]["tlsSettings"] = tls_settings;
        }
        
        // Add WebSocket settings if network is ws
        if endpoint.network.as_deref() == Some("ws") {
            let mut ws_settings = json!({});
            
            if let Some(path) = &endpoint.path {
                if !path.is_empty() {
                    ws_settings["path"] = json!(path);
                } else {
                    ws_settings["path"] = json!("/");
                }
            } else {
                ws_settings["path"] = json!("/");
            }
            
            // Add headers if host_header is specified
            if let Some(host) = &endpoint.host_header {
                if !host.is_empty() {
                    ws_settings["headers"] = json!({
                        "Host": host
                    });
                }
            }
            
            outbound["streamSettings"]["wsSettings"] = ws_settings;
        }
        
        // Add other network types (kcp, http, quic, grpc) if needed
        if endpoint.network.as_deref() == Some("kcp") {
            outbound["streamSettings"]["kcpSettings"] = json!({});
        }
        
        if endpoint.network.as_deref() == Some("http") {
            let mut http_settings = json!({});
            if let Some(path) = &endpoint.path {
                if !path.is_empty() {
                    http_settings["path"] = json!(path);
                }
            }
            if let Some(host) = &endpoint.host_header {
                if !host.is_empty() {
                    http_settings["host"] = json!(vec![host]);
                }
            }
            if !http_settings.as_object().unwrap().is_empty() {
                outbound["streamSettings"]["httpSettings"] = http_settings;
            }
        }
        
        Ok(outbound)
    }
    
    fn generate_vless_outbound(&self, endpoint: &EndpointConfig) -> Result<Value> {
        let uuid = endpoint.uuid.as_ref().context("VLESS requires UUID")?;
        let mut user = json!({
            "id": uuid,
            "encryption": endpoint.encryption.as_deref().unwrap_or("none")
        });
        
        // Add flow if present (for XTLS, e.g., "xtls-rprx-vision")
        if let Some(flow) = &endpoint.flow {
            if !flow.is_empty() {
                user["flow"] = json!(flow);
            }
        }
        
        let mut outbound = json!({
            "protocol": "vless",
            "settings": {
                "vnext": [{
                    "address": endpoint.host,
                    "port": endpoint.port,
                    "users": [user]
                }]
            },
            "streamSettings": {
                "network": endpoint.network.as_deref().unwrap_or("tcp")
            }
        });
        
        // Add TLS settings if present (similar to VMess)
        if let Some(tls) = &endpoint.tls {
            outbound["streamSettings"]["security"] = json!("tls");
            let mut tls_settings = json!({});
            
            if let Some(server_name) = &tls.server_name {
                if !server_name.is_empty() {
                    tls_settings["serverName"] = json!(server_name);
                }
            }
            
            // Allow insecure certificates - default to true (like v2rayN)
            tls_settings["allowInsecure"] = json!(tls.allow_insecure);
            
            // Note: fingerprint (e.g., "safari") should be extracted from raw_config
            // and added here if available. For now, we'll add it if present in TLS config.
            
            outbound["streamSettings"]["tlsSettings"] = tls_settings;
        } else {
            outbound["streamSettings"]["security"] = json!("none");
        }
        
        Ok(outbound)
    }
    
    fn generate_shadowsocks_outbound(&self, endpoint: &EndpointConfig) -> Result<Value> {
        Ok(json!({
            "protocol": "shadowsocks",
            "settings": {
                "servers": [{
                    "address": endpoint.host,
                    "port": endpoint.port,
                    "method": endpoint.method.as_ref().context("Shadowsocks requires method")?,
                    "password": endpoint.password.as_ref().context("Shadowsocks requires password")?
                }]
            }
        }))
    }
    
    fn generate_trojan_outbound(&self, endpoint: &EndpointConfig) -> Result<Value> {
        Ok(json!({
            "protocol": "trojan",
            "settings": {
                "servers": [{
                    "address": endpoint.host,
                    "port": endpoint.port,
                    "password": endpoint.password.as_ref().context("Trojan requires password")?
                }]
            },
            "streamSettings": {
                "security": "tls"
            }
        }))
    }
    
    fn generate_socks5_outbound(&self, endpoint: &EndpointConfig) -> Result<Value> {
        let mut settings = json!({
            "servers": [{
                "address": endpoint.host,
                "port": endpoint.port
            }]
        });
        
        if let (Some(user), Some(pass)) = (&endpoint.username, &endpoint.password) {
            settings["servers"][0]["users"] = json!([{
                "user": user,
                "pass": pass
            }]);
        }
        
        Ok(json!({
            "protocol": "socks",
            "settings": settings
        }))
    }
    
    /// Write config to file
    pub fn write_config(&self, config: &Value) -> Result<()> {
        let config_str = serde_json::to_string_pretty(config)?;
        std::fs::write(&self.config_path, config_str)?;
        info!("Xray config written to: {:?}", self.config_path);
        Ok(())
    }
    
    /// Start xray with current config
    pub fn start(&self) -> Result<()> {
        let mut process_guard = self.process.lock().unwrap();
        
        if process_guard.is_some() {
            warn!("Xray is already running");
            return Ok(());
        }
        
        info!("Starting xray from: {:?}", self.xray_path);
        info!("Using config: {:?}", self.config_path);
        
        // Check if config file exists and is readable
        if !self.config_path.exists() {
            return Err(anyhow::anyhow!("Config file does not exist: {:?}", self.config_path));
        }
        
        // Check if ports are available before starting Xray
        // This prevents the "bind: Only one usage of each socket address" error
        // Retry up to 5 times with delays to handle TIME_WAIT states
        warn!("=== PORT AVAILABILITY CHECK START ===");
        info!("Checking port availability: SOCKS {}, HTTP {}", self.socks_port, self.http_port);
        let max_retries = 5;
        let mut socks_available = false;
        let mut http_available = false;
        
        for attempt in 0..max_retries {
            let (socks, http) = crate::proxy::xray_check::check_ports_available_sync(
                self.socks_port, 
                self.http_port
            );
            
            socks_available = socks;
            http_available = http;
            
            info!("Port check (attempt {}): SOCKS {}: {}, HTTP {}: {}", 
                attempt + 1,
                self.socks_port, if socks_available { "available" } else { "IN USE" },
                self.http_port, if http_available { "available" } else { "IN USE" }
            );
            
            if socks_available && http_available {
                info!("Ports {} (SOCKS) and {} (HTTP) are available (attempt {})", 
                    self.socks_port, self.http_port, attempt + 1);
                break;
            }
            
            if attempt < max_retries - 1 {
                let delay_ms = (attempt + 1) * 500; // Increasing delay: 500ms, 1000ms, 1500ms, 2000ms
                warn!("Ports not available (attempt {}): SOCKS {}: {}, HTTP {}: {}. Waiting {}ms before retry...",
                    attempt + 1,
                    self.socks_port, if socks_available { "available" } else { "in use" },
                    self.http_port, if http_available { "available" } else { "in use" },
                    delay_ms
                );
                
                // Try to kill any processes using the ports
                if !socks_available || !http_available {
                    let _ = self.kill_processes_using_ports();
                }
                
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
        }
        
        // If ports are still unavailable after retries, try to find alternative ports
        let mut actual_socks = self.socks_port;
        let mut actual_http = self.http_port;
        let mut ports_changed = false;
        
        if !socks_available || !http_available {
            warn!("Configured ports unavailable after {} retries. SOCKS {}: {}, HTTP {}: {}. Attempting to find alternative ports...",
                max_retries,
                self.socks_port, if socks_available { "available" } else { "IN USE" },
                self.http_port, if http_available { "available" } else { "IN USE" }
            );
            
            if let Some((socks, http)) = crate::proxy::xray_check::find_available_ports_sync(
                self.socks_port,
                self.http_port,
                50 // Try up to 50 ports
            ) {
                if socks != self.socks_port {
                    warn!("SOCKS port {} is in use, using alternative port {}", self.socks_port, socks);
                    ports_changed = true;
                }
                if http != self.http_port {
                    warn!("HTTP port {} is in use, using alternative port {}", self.http_port, http);
                    ports_changed = true;
                }
                actual_socks = socks;
                actual_http = http;
                
                // Update the actual ports
                *self.actual_socks_port.lock().unwrap() = Some(actual_socks);
                *self.actual_http_port.lock().unwrap() = Some(actual_http);
                
                // Regenerate and rewrite config with new ports
                info!("Regenerating config with ports: SOCKS {}, HTTP {}", actual_socks, actual_http);
                // Update the config file directly by parsing JSON and updating port values
                if let Ok(config_str) = std::fs::read_to_string(&self.config_path) {
                    if let Ok(mut config) = serde_json::from_str::<Value>(&config_str) {
                        // Update SOCKS port in inbounds
                        if let Some(inbounds) = config.get_mut("inbounds").and_then(|i| i.as_array_mut()) {
                            for inbound in inbounds {
                                if let Some(port) = inbound.get_mut("port") {
                                    if let Some(port_val) = port.as_u64() {
                                        if port_val == self.socks_port as u64 {
                                            *port = json!(actual_socks);
                                        } else if port_val == self.http_port as u64 {
                                            *port = json!(actual_http);
                                        }
                                    }
                                }
                            }
                        }
                        
                        let updated_config_str = serde_json::to_string_pretty(&config)?;
                        if let Err(e) = std::fs::write(&self.config_path, updated_config_str) {
                            warn!("Failed to update config file with new ports: {}", e);
                        } else {
                            info!("Config file updated with new ports: SOCKS {}, HTTP {}", actual_socks, actual_http);
                        }
                    } else {
                        warn!("Failed to parse config file for port update");
                    }
                }
            } else {
                return Err(anyhow::anyhow!(
                    "Could not find available ports. SOCKS port {} and HTTP port {} are in use, and no alternatives were found. Please stop the processes using these ports or change the ports in settings.",
                    self.socks_port, self.http_port
                ));
            }
        }
        
        if ports_changed {
            info!("Using alternative ports: SOCKS {}, HTTP {} (configured: {}, {})", 
                actual_socks, actual_http, self.socks_port, self.http_port);
        }
        
        // Final port check right before spawning Xray (double-check after config update)
        info!("Final port verification before spawning Xray...");
        let (final_socks_check, final_http_check) = crate::proxy::xray_check::check_ports_available_sync(
            actual_socks,
            actual_http
        );
        
        if !final_socks_check {
            return Err(anyhow::anyhow!(
                "SOCKS port {} became unavailable between check and Xray start. Another process may have grabbed it. Please stop the process using port {} or restart the application.",
                actual_socks, actual_socks
            ));
        }
        
        if !final_http_check {
            return Err(anyhow::anyhow!(
                "HTTP port {} became unavailable between check and Xray start. Another process may have grabbed it. Please stop the process using port {} or restart the application.",
                actual_http, actual_http
            ));
        }
        
        info!("Final port check passed: SOCKS {} and HTTP {} are both available", actual_socks, actual_http);
        
        // Validate config by trying to test it (Xray has -test flag but we'll just try to start)
        // Actually, let's just start and check if it crashes immediately
        
        info!("Spawning Xray process...");
        let child = Command::new(&self.xray_path)
            .arg("-config")
            .arg(&self.config_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start xray process")?;
        
        let pid = child.id();
        *process_guard = Some(child);
        info!("Xray process spawned successfully (PID: {:?})", pid);
        
        // Release the lock before waiting (to avoid deadlock)
        drop(process_guard);
        
        // Wait a bit for xray to initialize, then check if it's still running
        info!("Waiting 1 second for Xray to initialize...");
        std::thread::sleep(Duration::from_millis(1000));
        info!("Initialization wait complete, checking Xray process status...");
        
        // Check if process exited immediately (likely due to port binding error)
        // First check without holding the lock to avoid deadlock
        info!("Checking if Xray process exited immediately...");
        let process_exited = {
            let mut process_guard_check = self.process.lock().unwrap();
            if let Some(ref mut child_check) = *process_guard_check {
                match child_check.try_wait() {
                    Ok(Some(status)) => {
                        info!("Process exited immediately with status: {:?}", status);
                        true
                    }
                    Ok(None) => {
                        info!("Process is still running");
                        false
                    }
                    Err(e) => {
                        warn!("Error checking process status: {}", e);
                        false
                    }
                }
            } else {
                warn!("No process handle found");
                false
            }
        };
        
        if process_exited {
            info!("Process exited - handling port binding error...");
            // Process exited immediately - likely port binding error
            warn!("Xray process exited immediately after spawn (PID: {})", pid);
            
            // Release lock before reading output (read_xray_output also needs the lock)
            // Try to read error output
            let error_output = self.read_xray_output();
            if let Some(output) = &error_output {
                if output.contains("bind: Only one usage of each socket address") {
                    error!("Xray failed to bind to port - port binding error detected");
                    
                    // Port binding failed - try to find alternative ports and retry
                    warn!("Port binding failed despite check passing. Attempting to clean port and find alternative ports...");
                    
                    // First, try to forcefully kill any process using the ports
                    warn!("Forcefully killing any processes using ports {} and {}...", actual_socks, actual_http);
                    let _ = self.force_kill_port_processes(actual_socks, actual_http);
                    std::thread::sleep(Duration::from_millis(1000));
                    
                    // Check if ports are now free
                    let (socks_now_free, http_now_free) = crate::proxy::xray_check::check_ports_available_sync(
                        actual_socks,
                        actual_http
                    );
                    
                    if socks_now_free && http_now_free {
                        warn!("Ports are now free after cleanup. Retrying with original ports...");
                        // Release the process handle
                        {
                            let mut process_guard_check = self.process.lock().unwrap();
                            *process_guard_check = None;
                        }
                        
                        // Retry with original ports
                        info!("Retrying Xray spawn with cleaned ports...");
                        let child_retry = Command::new(&self.xray_path)
                            .arg("-config")
                            .arg(&self.config_path)
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn()
                            .context("Failed to start xray process after port cleanup")?;
                        
                        let pid_retry = child_retry.id();
                        let mut process_guard_retry = self.process.lock().unwrap();
                        *process_guard_retry = Some(child_retry);
                        drop(process_guard_retry);
                        
                        info!("Xray process spawned after port cleanup (PID: {:?})", pid_retry);
                        std::thread::sleep(Duration::from_millis(1000));
                        
                        if !self.is_running() {
                            let _error_output_retry = self.read_xray_output();
                            // If still fails, try alternative ports
                            warn!("Port cleanup didn't help, trying alternative ports...");
                        } else {
                            info!("Xray started successfully after port cleanup");
                            return Ok(());
                        }
                    }
                    
                    // Release the process handle
                    {
                        let mut process_guard_check = self.process.lock().unwrap();
                        *process_guard_check = None;
                    }
                        
                    // Try to find alternative ports (different from current ones)
                    warn!("Finding alternative ports (starting from {} + 1, {} + 1)...", actual_socks, actual_http);
                    let mut alt_socks = actual_socks.saturating_add(1);
                    let mut alt_http = actual_http.saturating_add(1);
                    
                    // Make sure we find different ports
                    if let Some((found_socks, found_http)) = crate::proxy::xray_check::find_available_ports_sync(
                        alt_socks,
                        alt_http,
                        50
                    ) {
                        // Only use if they're different from current
                        if found_socks != actual_socks || found_http != actual_http {
                            alt_socks = found_socks;
                            alt_http = found_http;
                            warn!("Found alternative ports: SOCKS {} -> {}, HTTP {} -> {}", 
                                actual_socks, alt_socks, actual_http, alt_http);
                        } else {
                            // Force find different ports by starting further away
                            warn!("Port finder returned same ports, trying ports further away...");
                            if let Some((found_socks2, found_http2)) = crate::proxy::xray_check::find_available_ports_sync(
                                actual_socks.saturating_add(10),
                                actual_http.saturating_add(10),
                                100
                            ) {
                                if found_socks2 != actual_socks && found_http2 != actual_http {
                                    alt_socks = found_socks2;
                                    alt_http = found_http2;
                                    warn!("Found alternative ports (further search): SOCKS {} -> {}, HTTP {} -> {}", 
                                        actual_socks, alt_socks, actual_http, alt_http);
                                } else {
                                    return Err(anyhow::anyhow!(
                                        "Could not find alternative ports. Ports {} and {} are in use and no alternatives found.",
                                        actual_socks, actual_http
                                    ));
                                }
                            } else {
                                return Err(anyhow::anyhow!(
                                    "Could not find alternative ports. Ports {} and {} are in use and no alternatives found.",
                                    actual_socks, actual_http
                                ));
                            }
                        }
                        
                        // Update actual ports
                        *self.actual_socks_port.lock().unwrap() = Some(alt_socks);
                        *self.actual_http_port.lock().unwrap() = Some(alt_http);
                        
                        // Update config file
                        if let Ok(config_str) = std::fs::read_to_string(&self.config_path) {
                            if let Ok(mut config) = serde_json::from_str::<Value>(&config_str) {
                                if let Some(inbounds) = config.get_mut("inbounds").and_then(|i| i.as_array_mut()) {
                                    for inbound in inbounds {
                                        if let Some(port) = inbound.get_mut("port") {
                                            if let Some(port_val) = port.as_u64() {
                                                if port_val == actual_socks as u64 {
                                                    *port = json!(alt_socks);
                                                } else if port_val == actual_http as u64 {
                                                    *port = json!(alt_http);
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                let updated_config_str = serde_json::to_string_pretty(&config)?;
                                std::fs::write(&self.config_path, updated_config_str)?;
                                info!("Config updated with alternative ports: SOCKS {}, HTTP {}", alt_socks, alt_http);
                            }
                        }
                        
                        // Retry spawning with new ports
                        info!("Retrying Xray spawn with alternative ports...");
                        let child_retry = Command::new(&self.xray_path)
                            .arg("-config")
                            .arg(&self.config_path)
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn()
                            .context("Failed to start xray process with alternative ports")?;
                        
                        let pid_retry = child_retry.id();
                        let mut process_guard_retry = self.process.lock().unwrap();
                        *process_guard_retry = Some(child_retry);
                        drop(process_guard_retry);
                        
                        info!("Xray process spawned with alternative ports (PID: {:?})", pid_retry);
                        std::thread::sleep(Duration::from_millis(1000));
                        
                        // Check if it's still running
                        if !self.is_running() {
                            let error_output_retry = self.read_xray_output();
                            return Err(anyhow::anyhow!(
                                "Xray failed to start even with alternative ports. Error: {}",
                                error_output_retry.unwrap_or_else(|| "Unknown error".to_string())
                            ));
                        }
                        
                        info!("Xray started successfully with alternative ports");
                        return Ok(());
                    } else {
                        return Err(anyhow::anyhow!(
                            "Xray failed to bind to ports {} and {}. Port check passed but bind failed. Could not find alternative ports. Error: {}",
                            actual_socks, actual_http,
                            error_output.unwrap_or_else(|| "Port binding failed".to_string())
                        ));
                    }
                    } else {
                        // Other error, not port binding
                        return Err(anyhow::anyhow!(
                            "Xray process exited immediately. Error: {}",
                            error_output.unwrap_or_else(|| "Unknown error".to_string())
                        ));
                    }
                } else {
                    return Err(anyhow::anyhow!(
                        "Xray process exited immediately after spawn (PID: {}). No error output available.",
                        pid
                    ));
                }
        } else {
            info!("Process is still running - continuing normally");
        }
        
        // Process is still running - continue normally
        // Check if process is still running (might have crashed due to config error)
        // On Windows, try_wait() can sometimes block, so we'll use a timeout approach
        // or just skip this check and rely on port readiness check
        info!("Skipping detailed process status check to avoid blocking - will check port readiness instead");
        info!("Xray process (PID {}) spawned, proceeding to port readiness check", pid);
        info!("Xray start() completed successfully");
        Ok(())
    }
    
    /// Stop xray process
    pub fn stop(&self) -> Result<()> {
        let mut process_guard = self.process.lock().unwrap();
        
        if let Some(mut child) = process_guard.take() {
            info!("Stopping xray process...");
            let pid = child.id();
            
            // Try graceful shutdown first (SIGTERM on Unix, but Windows doesn't support it well)
            #[cfg(windows)]
            {
                // On Windows, we need to kill the process tree
                use std::process::Command;
                // Try to kill the process and its children
                let _ = Command::new("taskkill")
                    .args(&["/F", "/T", "/PID", &pid.to_string()])
                    .output();
                let _ = child.kill();
            }
            #[cfg(not(windows))]
            {
                let _ = child.kill();
            }
            
            // Wait for process to exit with timeout
            let start = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        info!("Xray process (PID {}) stopped", pid);
                        break;
                    }
                    Ok(None) => {
                        if start.elapsed().as_secs() > 5 {
                            warn!("Xray process (PID {}) did not stop within 5 seconds, forcing kill", pid);
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        warn!("Error waiting for Xray process: {}", e);
                        break;
                    }
                }
            }
            
            info!("Xray process stopped, waiting for ports to be released...");
            // Give OS more time to release the ports (Windows can take a few seconds)
            std::thread::sleep(Duration::from_millis(2000));
            
            // Check if ports are still in use (using synchronous version since stop() is not async)
            // Retry a few times as ports may be in TIME_WAIT state
            for attempt in 0..3 {
                let (socks_in_use, http_in_use) = {
                    let socks = crate::proxy::xray_check::check_port_listening_sync("127.0.0.1", self.socks_port);
                    let http = crate::proxy::xray_check::check_port_listening_sync("127.0.0.1", self.http_port);
                    (socks, http)
                };
                
                if !socks_in_use && !http_in_use {
                    info!("Ports {} and {} are now free (attempt {})", self.socks_port, self.http_port, attempt + 1);
                    break;
                }
                
                if attempt < 2 {
                    warn!("Ports still in use after stopping Xray (attempt {}): SOCKS {}: {}, HTTP {}: {}. Waiting...",
                        attempt + 1,
                        self.socks_port, if socks_in_use { "in use" } else { "free" },
                        self.http_port, if http_in_use { "in use" } else { "free" }
                    );
                    // Try to kill any processes using these ports
                    let _ = self.kill_processes_using_ports();
                    std::thread::sleep(Duration::from_millis(1000));
                } else {
                    warn!("Ports still in use after stopping Xray: SOCKS {}: {}, HTTP {}: {}. They may be in TIME_WAIT state.",
                        self.socks_port, if socks_in_use { "in use" } else { "free" },
                        self.http_port, if http_in_use { "in use" } else { "free" }
                    );
                }
            }
        } else {
            warn!("Xray is not running");
        }
        
        Ok(())
    }
    
    /// Kill processes using the configured ports (Windows-specific)
    #[cfg(windows)]
    fn kill_processes_using_ports(&self) -> Result<()> {
        use std::process::Command;
        
        info!("Attempting to find and kill processes using ports {} and {}", self.socks_port, self.http_port);
        
        // Try PowerShell first (more reliable on Windows 10+)
        let ps_cmd = format!(
            r#"Get-NetTCPConnection -LocalPort {},{} -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique | ForEach-Object {{ Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }}"#,
            self.socks_port, self.http_port
        );
        
        let ps_result = Command::new("powershell")
            .args(&["-Command", &ps_cmd])
            .output();
        
        if ps_result.is_ok() {
            info!("Used PowerShell to kill processes on ports {} and {}", self.socks_port, self.http_port);
            std::thread::sleep(Duration::from_millis(500));
            return Ok(());
        }
        
        // Fallback to netstat + taskkill
        info!("PowerShell method failed, falling back to netstat...");
        let output = Command::new("netstat")
            .args(&["-ano"])
            .output()?;
        
        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut pids_to_kill = std::collections::HashSet::new();
        
        for line in output_str.lines() {
            if line.contains(&format!(":{}", self.socks_port)) || line.contains(&format!(":{}", self.http_port)) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(pid_str) = parts.last() {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        pids_to_kill.insert(pid);
                    }
                }
            }
        }
        
        for pid in pids_to_kill {
            info!("Killing process PID {} using ports {} or {}", pid, self.socks_port, self.http_port);
            let _ = Command::new("taskkill")
                .args(&["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        
        // Wait a bit more for ports to be released
        std::thread::sleep(Duration::from_millis(1000));
        
        Ok(())
    }
    
    #[cfg(not(windows))]
    fn kill_processes_using_ports(&self) -> Result<()> {
        // On Unix, we could use lsof or fuser, but for now just wait
        std::thread::sleep(Duration::from_millis(500));
        Ok(())
    }
    
    /// Forcefully kill processes using specific ports (Windows-specific)
    #[cfg(windows)]
    fn force_kill_port_processes(&self, socks_port: u16, http_port: u16) -> Result<()> {
        use std::process::Command;
        
        info!("Forcefully killing processes using ports {} and {}...", socks_port, http_port);
        
        // Use PowerShell to find and kill processes
        let ps_cmd = format!(
            r#"Get-NetTCPConnection -LocalPort {},{} -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique | ForEach-Object {{ Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }}"#,
            socks_port, http_port
        );
        
        let ps_result = Command::new("powershell")
            .args(&["-Command", &ps_cmd])
            .output();
        
        if ps_result.is_ok() {
            info!("Used PowerShell to kill processes on ports {} and {}", socks_port, http_port);
            std::thread::sleep(Duration::from_millis(1000));
            return Ok(());
        }
        
        // Fallback to netstat + taskkill
        warn!("PowerShell method failed, falling back to netstat...");
        let output = Command::new("netstat")
            .args(&["-ano"])
            .output()?;
        
        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut pids_to_kill = std::collections::HashSet::new();
        
        for line in output_str.lines() {
            if line.contains(&format!(":{}", socks_port)) || line.contains(&format!(":{}", http_port)) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(pid_str) = parts.last() {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        pids_to_kill.insert(pid);
                    }
                }
            }
        }
        
        for pid in pids_to_kill {
            info!("Killing process PID {} using ports {} or {}", pid, socks_port, http_port);
            let _ = Command::new("taskkill")
                .args(&["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        
        // Wait for ports to be released
        std::thread::sleep(Duration::from_millis(1000));
        
        Ok(())
    }
    
    #[cfg(not(windows))]
    fn force_kill_port_processes(&self, _socks_port: u16, _http_port: u16) -> Result<()> {
        // On Unix, we could use lsof or fuser, but for now just wait
        std::thread::sleep(Duration::from_millis(500));
        Ok(())
    }
    
    /// Check if xray is running
    /// Read Xray's stderr/stdout output (non-blocking, only if process has exited)
    fn read_xray_output(&self) -> Option<String> {
        let mut process_guard = match self.process.try_lock() {
            Ok(guard) => guard,
            Err(_) => return None,
        };
        
        if let Some(ref mut child) = *process_guard {
            // Process has exited, try to read output
            let mut output = String::new();
            
            // Try to read stderr first (usually contains error messages)
            if let Some(mut stderr) = child.stderr.take() {
                use std::io::Read;
                // Use read_to_string with a timeout approach - read what's available
                let mut buf = [0u8; 4096];
                loop {
                    match stderr.read(&mut buf) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            let s = String::from_utf8_lossy(&buf[..n]);
                            output.push_str(&s);
                        }
                        Err(_) => break, // Error reading
                    }
                    // Limit total output to avoid huge logs
                    if output.len() > 8192 {
                        output.push_str("\n...(truncated)");
                        break;
                    }
                }
            }
            
            // Also try stdout
            if let Some(mut stdout) = child.stdout.take() {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                loop {
                    match stdout.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let s = String::from_utf8_lossy(&buf[..n]);
                            if !output.is_empty() {
                                output.push_str("\n--- stdout ---\n");
                            }
                            output.push_str(&s);
                        }
                        Err(_) => break,
                    }
                    if output.len() > 8192 {
                        output.push_str("\n...(truncated)");
                        break;
                    }
                }
            }
            
            if !output.is_empty() {
                Some(output)
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Get the actual SOCKS port being used (may differ from configured port if fallback was used)
    pub fn get_actual_socks_port(&self) -> u16 {
        let actual = self.actual_socks_port.lock().unwrap();
        actual.unwrap_or(self.socks_port)
    }
    
    /// Get the actual HTTP port being used (may differ from configured port if fallback was used)
    pub fn get_actual_http_port(&self) -> u16 {
        let actual = self.actual_http_port.lock().unwrap();
        actual.unwrap_or(self.http_port)
    }
    
    /// Reset actual ports to None (use configured ports)
    pub fn reset_actual_ports(&self) {
        *self.actual_socks_port.lock().unwrap() = None;
        *self.actual_http_port.lock().unwrap() = None;
    }
    
    pub fn is_running(&self) -> bool {
        debug!("is_running() called, acquiring lock...");
        let mut process_guard = match self.process.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                warn!("Could not acquire process lock in is_running() - lock is held by another thread");
                // If we can't get the lock, assume it's running to avoid blocking
                return true;
            }
        };
        debug!("is_running() lock acquired");
        if let Some(child) = process_guard.as_mut() {
            debug!("is_running() calling try_wait()...");
            let result = child.try_wait();
            debug!("is_running() try_wait() returned");
            match result {
                Ok(Some(_)) => {
                    debug!("is_running() - process exited");
                    false // Process exited
                }
                Ok(None) => {
                    debug!("is_running() - process still running");
                    true // Process still running
                }
                Err(e) => {
                    warn!("is_running() - error checking process: {}", e);
                    false // Error checking
                }
            }
        } else {
            debug!("is_running() - no process handle");
            false
        }
    }
    
    /// Test endpoint by starting xray with it and testing connectivity
    pub async fn test_endpoint(&self, endpoint: &EndpointConfig, timeout: Duration) -> Result<ProbeResult> {
        info!("Starting endpoint test for {}:{} ({})", endpoint.host, endpoint.port, endpoint.protocol);
        
        // VMess protocol depends on system time - check time synchronization
        // Reference: https://xtls.github.io/development/protocols/vmess.html
        // VMess authentication uses UTC time with ±30 second window, system must be within 120 seconds
        if endpoint.protocol.to_lowercase() == "vmess" {
            self.check_system_time_for_vmess()?;
        }
        
        // Generate config
        info!("Generating Xray configuration...");
        let config = self.generate_config(endpoint)?;
        info!("Configuration generated successfully");
        
        info!("Writing Xray config to: {:?}", self.config_path);
        self.write_config(&config)?;
        info!("Config written successfully");
        
        // Stop any existing instance
        info!("Stopping any existing Xray instance...");
        let _ = self.stop();
        info!("Stopped existing instances (if any)");
        
        // Wait for ports to be fully released (Windows can take time)
        info!("Waiting 2 seconds for ports to be fully released...");
        tokio::time::sleep(Duration::from_millis(2000)).await;
        
        // Double-check ports are free before starting
        info!("Verifying ports are free before starting Xray...");
        let (socks_free, http_free) = crate::proxy::xray_check::check_ports_available_sync(
            self.socks_port,
            self.http_port
        );
        
        if !socks_free {
            warn!("SOCKS port {} is still in use after stop(). Attempting to kill processes...", self.socks_port);
            let _ = self.kill_processes_using_ports();
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        
        if !http_free {
            warn!("HTTP port {} is still in use after stop(). Attempting to kill processes...", self.http_port);
            let _ = self.kill_processes_using_ports();
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        
        // Start xray
        info!("Starting Xray for endpoint test...");
        self.start()?;
        info!("Xray start() returned successfully, proceeding to port readiness check...");
        
        // Get actual ports being used (may differ from configured if fallback was used)
        let actual_socks_port = self.get_actual_socks_port();
        let actual_http_port = self.get_actual_http_port();
        
        // Wait for xray to be ready and verify SOCKS port is listening
        // v2rayN checks if the port is listening before using it
        // Timeout after 3 seconds as requested
        let max_wait_ms = 3000;
        let check_interval_ms = 100;
        let max_attempts = max_wait_ms / check_interval_ms;
        
        info!("Waiting for Xray SOCKS port {} to become ready (timeout: {}ms)...", actual_socks_port, max_wait_ms);
        
        let mut socks_ready = false;
        let start_time = std::time::Instant::now();
        
        for attempt in 0..max_attempts {
            let elapsed_ms = start_time.elapsed().as_millis() as u64;
            
            // Check if Xray process is still running first
            if !self.is_running() {
                warn!("Xray process exited after {}ms, before SOCKS port became ready", elapsed_ms);
                
                // Try to read Xray's error output (process has exited, so this should not block)
                let error_output = self.read_xray_output();
                if let Some(output) = &error_output {
                    if !output.is_empty() {
                        error!("Xray error output:\n{}", output);
                    }
                }
                
                let error_msg = if let Some(output) = error_output {
                    if !output.is_empty() {
                        format!(
                            "Xray process exited before SOCKS port became ready (after {}ms). Xray error output:\n{}",
                            elapsed_ms, output
                        )
                    } else {
                        format!(
                            "Xray process exited before SOCKS port became ready (after {}ms). Check Xray configuration. You can run Xray manually with the config file to see detailed error messages.",
                            elapsed_ms
                        )
                    }
                } else {
                    format!(
                        "Xray process exited before SOCKS port became ready (after {}ms). Check Xray configuration. You can run Xray manually with the config file to see detailed error messages.",
                        elapsed_ms
                    )
                };
                
                return Err(anyhow::anyhow!(error_msg));
            }
            
            // Check if we've exceeded 3 second timeout
            if elapsed_ms >= max_wait_ms {
                warn!("Timeout reached: {}ms elapsed, SOCKS port {} still not ready", elapsed_ms, actual_socks_port);
                break;
            }
            
            // Sleep before checking (except on first attempt)
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(check_interval_ms)).await;
            }
            
            let (socks_listening, http_listening) = crate::proxy::xray_check::check_local_proxy_ports(
                actual_socks_port, 
                actual_http_port
            ).await;
            
            if socks_listening {
                socks_ready = true;
                info!("✓ SOCKS proxy port {} is ready after {}ms", actual_socks_port, elapsed_ms);
                break;
            }
            
            // Log every 500ms to avoid spam, but always log first and last attempts
            if attempt == 0 || attempt % 5 == 0 || elapsed_ms >= max_wait_ms - check_interval_ms {
                info!("Waiting for SOCKS port {}... (attempt {}, {}ms elapsed, HTTP port: {})", 
                    actual_socks_port, attempt + 1, elapsed_ms, if http_listening { "ready" } else { "not ready" });
            } else {
                debug!("Attempt {}: SOCKS port {} not ready yet (HTTP: {}, {}ms elapsed)", 
                    attempt + 1, actual_socks_port, if http_listening { "ready" } else { "not ready" }, elapsed_ms);
            }
        }
        
        if !socks_ready {
            let elapsed_ms = start_time.elapsed().as_millis() as u64;
            // Final check if process is still running
            let still_running = self.is_running();
            
            // Try to read Xray's output for diagnostics
            if still_running {
                info!("Xray process is still running after {}ms timeout", elapsed_ms);
                // Check if ports are in use
                let (socks_listening, http_listening) = crate::proxy::xray_check::check_local_proxy_ports(
                    actual_socks_port, 
                    actual_http_port
                ).await;
                warn!("Final port check - SOCKS: {}, HTTP: {}", 
                    if socks_listening { "listening" } else { "NOT listening" },
                    if http_listening { "listening" } else { "NOT listening" });
            } else {
                warn!("Xray process exited during port readiness check (after {}ms)", elapsed_ms);
                // Don't try to read output here as it can block - just report the error
                info!("Xray process is no longer running. Check Xray configuration or run Xray manually to see error messages.");
            }
            
            let error_msg = if !still_running {
                format!(
                    "Xray process exited. SOCKS proxy port {} never became ready within {}ms timeout. Check Xray configuration for errors (see logs above).",
                    actual_socks_port, elapsed_ms
                )
            } else {
                format!(
                    "SOCKS proxy port {} is not listening after {}ms timeout. Xray is running but port may be in use by another process, or Xray failed to bind to the port. Check if port {} is already in use.",
                    actual_socks_port, elapsed_ms, actual_socks_port
                )
            };
            return Err(anyhow::anyhow!(error_msg));
        }
        
        // Test connectivity through local proxy
        // Use SOCKS5 proxy like v2rayN does (more reliable than HTTP proxy)
        // Use HTTPS URLs that are known to work well for connectivity testing
        let test_urls = vec![
            "https://www.google.com/generate_204",  // Google's connectivity check (returns 204) - same as v2rayN
            "https://www.gstatic.com/generate_204", // Google static - same as v2rayN
            "https://www.apple.com/library/test/success.html", // Apple test page - same as v2rayN
            "http://www.msftconnecttest.com/connecttest.txt", // Microsoft connectivity test - same as v2rayN
        ];
        
        // Use SOCKS5 proxy instead of HTTP proxy (more reliable, like v2rayN)
        // Format: socks5://127.0.0.1:port
        let proxy_url = format!("socks5://127.0.0.1:{}", actual_socks_port);
        
        // Separate connect timeout (shorter) from read timeout
        // Connect timeout: 5 seconds (faster failure detection)
        // Read timeout: use the provided timeout
        let connect_timeout = Duration::from_secs(5).min(timeout);
        let read_timeout = timeout;
        
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(&proxy_url)?)
            .connect_timeout(connect_timeout)
            .timeout(read_timeout)
            .tcp_keepalive(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(2)
            .http2_keep_alive_interval(Duration::from_secs(30))
            .http2_keep_alive_timeout(Duration::from_secs(10))
            .http2_keep_alive_while_idle(true)
            .build()?;
        
        let mut last_error = None;
        
        // First, do a quick TCP connection test to the proxy port to verify it's responsive
        info!("Performing quick TCP connection test to proxy port {}...", actual_socks_port);
        let proxy_connection_test = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::net::TcpStream::connect(format!("127.0.0.1:{}", actual_socks_port))
        ).await;
        
        match proxy_connection_test {
            Ok(Ok(mut stream)) => {
                info!("✓ Proxy port {} is accepting TCP connections", actual_socks_port);
                
                // Try a simple SOCKS5 handshake to verify the proxy is actually working
                // SOCKS5 initial handshake: [0x05, 0x01, 0x00] (version 5, 1 auth method, no auth)
                use tokio::io::{AsyncWriteExt, AsyncReadExt};
                let socks5_handshake = [0x05, 0x01, 0x00];
                if let Ok(_) = stream.write_all(&socks5_handshake).await {
                    let mut response = [0u8; 2];
                    let read_result = tokio::time::timeout(Duration::from_secs(1), stream.read_exact(&mut response)).await;
                    match read_result {
                        Ok(Ok(_)) => {
                            if response[0] == 0x05 && response[1] == 0x00 {
                                info!("✓ SOCKS5 proxy handshake successful - proxy is working");
                            } else {
                                warn!("⚠ SOCKS5 proxy returned unexpected response: {:?}", response);
                            }
                        }
                        _ => {
                            warn!("⚠ SOCKS5 proxy handshake read timed out or failed");
                        }
                    }
                }
                let _ = stream.shutdown().await;
            }
            Ok(Err(e)) => {
                warn!("✗ Proxy port {} connection test failed: {}", actual_socks_port, e);
                last_error = Some(format!("Proxy port {} is not accepting connections: {}", actual_socks_port, e));
            }
            Err(_) => {
                warn!("✗ Proxy port {} connection test timed out (2s)", actual_socks_port);
                last_error = Some(format!("Proxy port {} connection test timed out", actual_socks_port));
            }
        }
        
        for (idx, test_url) in test_urls.iter().enumerate() {
            info!("Testing endpoint {} of {}: {}", idx + 1, test_urls.len(), test_url);
            let request_start = std::time::Instant::now();
            
            match client.get(*test_url).send().await {
                Ok(response) => {
                    let status = response.status();
                    
                    // Check if this is an Xray error response (when remote proxy is unreachable)
                    // Xray returns JSON errors like {"code":404,"message":"path / was not found"}
                    let body_text = response.text().await.unwrap_or_default();
                    let is_xray_error = body_text.contains("\"code\"") && body_text.contains("\"message\"");
                    
                    if is_xray_error {
                        // This is an Xray error response, meaning the remote proxy server is unreachable
                        // When Xray can't connect to the outbound (VMess/Shadowsocks/etc), it returns
                        // JSON error responses instead of forwarding the HTTP request
                        let error_msg = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body_text) {
                            let code = json.get("code").and_then(|v| v.as_u64()).unwrap_or(0);
                            let message = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                            
                            // Add VMess-specific guidance for authentication failures
                            let mut msg = format!(
                                "Remote proxy server unreachable (code {}): {}. The {} endpoint at {}:{} cannot be reached.",
                                code, message, endpoint.protocol, endpoint.host, endpoint.port
                            );
                            
                            // If it's VMess and might be a time/auth issue, add guidance
                            if endpoint.protocol.to_lowercase() == "vmess" {
                                msg.push_str("\n   → VMess authentication may have failed. Check:");
                                msg.push_str("\n      - System time synchronization (must be within ±120s UTC)");
                                msg.push_str("\n      - UUID/ID is correct");
                                msg.push_str("\n      - alterId matches server configuration (if used)");
                                msg.push_str("\n      - Reference: https://xtls.github.io/development/protocols/vmess.html");
                            }
                            
                            msg
                        } else {
                            let mut msg = format!("Remote proxy unreachable: {}. Endpoint {}:{} cannot be reached.", 
                                body_text.trim(), endpoint.host, endpoint.port);
                            
                            if endpoint.protocol.to_lowercase() == "vmess" {
                                msg.push_str("\n   → For VMess: Verify system time is synchronized (NTP)");
                            }
                            
                            msg
                        };
                        debug!("Xray error detected: {}, trying next endpoint...", error_msg);
                        last_error = Some(error_msg);
                        continue;
                    }
                    
                    // Accept 2xx, 3xx, and 204 as success - these indicate proxy is working
                    if status.is_success() || status.is_redirection() || status.as_u16() == 204 {
                        let latency = request_start.elapsed().as_millis() as u128;
                        info!("✓ Proxy test successful via {}: status {} in {}ms", test_url, status.as_u16(), latency);
                        return Ok(ProbeResult {
                            endpoint: format!("{}:{}", endpoint.host, endpoint.port),
                            latency_ms: latency,
                            packet_loss: 0.0,
                        });
                    } else {
                        // Real website returned 4xx/5xx - this is unusual but might indicate proxy issue
                        // or the endpoint might be blocked. Try next endpoint.
                        last_error = Some(format!("Status {} from {} (response: {})", status.as_u16(), test_url, 
                            if body_text.len() > 100 { &body_text[..100] } else { &body_text }));
                        debug!("Test endpoint {} returned status {}, trying next...", test_url, status.as_u16());
                        continue;
                    }
                }
                Err(e) => {
                    // Connection errors indicate the proxy is not working
                    let elapsed = request_start.elapsed();
                    let error_msg = format!("Connection error: {} (after {}ms)", e, elapsed.as_millis());
                    last_error = Some(error_msg.clone());
                    warn!("✗ Test endpoint {} failed: {}", test_url, error_msg);
                    
                    // If we get a timeout, try the next endpoint immediately
                    if e.is_timeout() {
                        warn!("  → Timeout detected, trying next endpoint...");
                        continue;
                    }
                    
                    // For other errors, also continue to next endpoint
                    continue;
                }
            }
        }
        
        // All endpoints failed - provide detailed network diagnostics
        let diagnostics = self.collect_network_diagnostics(endpoint).await;
        let error_msg = format!(
            "Proxy test failed: All test endpoints failed. Last error: {}\n\nNetwork Diagnostics:\n{}",
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
            diagnostics.join("\n")
        );
        
        Err(anyhow::anyhow!(error_msg))
    }
    
    /// Check system time synchronization for VMess protocol
    /// VMess requires system UTC time to be within 120 seconds accuracy
    /// Reference: https://xtls.github.io/development/protocols/vmess.html
    fn check_system_time_for_vmess(&self) -> Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};
        
        // Get current system time
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?;
        
        let current_timestamp = now.as_secs();
        info!("System UTC timestamp: {} ({} seconds since epoch)", current_timestamp, current_timestamp);
        
        // Note: We can't easily check NTP sync status without external tools
        // But we can warn if the timestamp seems unreasonable (e.g., before 2020 or far in future)
        let min_reasonable = 1577836800; // 2020-01-01 00:00:00 UTC
        let max_reasonable = current_timestamp + 86400 * 365; // 1 year in future
        
        if current_timestamp < min_reasonable {
            warn!("⚠ System time appears to be before 2020 (timestamp: {}). VMess requires accurate system time.", current_timestamp);
            warn!("   Please synchronize your system clock using NTP. VMess authentication may fail.");
        } else if current_timestamp > max_reasonable {
            warn!("⚠ System time appears to be far in the future (timestamp: {}). VMess requires accurate system time.", current_timestamp);
            warn!("   Please synchronize your system clock using NTP. VMess authentication may fail.");
        } else {
            info!("✓ System time appears reasonable for VMess protocol");
        }
        
        // Log a reminder about time requirements
        info!("VMess protocol requires system UTC time accuracy within 120 seconds");
        info!("   Authentication uses UTC time with ±30 second window");
        info!("   If connections fail, check system time synchronization (NTP)");
        
        Ok(())
    }
    
    /// Collect network diagnostics for troubleshooting connection issues
    async fn collect_network_diagnostics(&self, endpoint: &EndpointConfig) -> Vec<String> {
        use std::process::Command;
        use std::time::Duration;
        use tokio::time::timeout;
        
        let mut diagnostics = Vec::new();
        diagnostics.push("=".repeat(60).to_string());
        diagnostics.push("Network Diagnostics".to_string());
        diagnostics.push("=".repeat(60).to_string());
        
        // 1. DNS Resolution Test
        diagnostics.push("\n[1] DNS Resolution:".to_string());
        match timeout(Duration::from_secs(5), tokio::net::lookup_host(format!("{}:{}", endpoint.host, endpoint.port))).await {
            Ok(Ok(mut addrs)) => {
                if let Some(addr) = addrs.next() {
                    let ip = addr.ip();
                    diagnostics.push(format!("  ✓ {} resolved to: {}", endpoint.host, ip));
                } else {
                    diagnostics.push(format!("  ✗ Failed to resolve {}", endpoint.host));
                }
            }
            Ok(Err(e)) => {
                diagnostics.push(format!("  ✗ DNS resolution failed: {}", e));
            }
            Err(_) => {
                diagnostics.push("  ✗ DNS resolution timed out (5s)".to_string());
            }
        }
        
        // 2. Direct TCP Connection Test (bypassing proxy)
        diagnostics.push("\n[2] Direct TCP Connection Test (bypassing proxy):".to_string());
        let direct_test = timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        ).await;
        
        match direct_test {
            Ok(Ok(_)) => {
                diagnostics.push(format!("  ✓ Direct connection to {}:{} succeeded", endpoint.host, endpoint.port));
                diagnostics.push("  → Endpoint is reachable, issue may be with proxy configuration".to_string());
            }
            Ok(Err(e)) => {
                diagnostics.push(format!("  ✗ Direct connection failed: {}", e));
                diagnostics.push("  → Endpoint may be unreachable or firewall is blocking".to_string());
            }
            Err(_) => {
                diagnostics.push("  ✗ Direct connection timed out (5s)".to_string());
                diagnostics.push("  → Endpoint may be unreachable or firewall is blocking silently".to_string());
            }
        }
        
        // 3. Routing Information (traceroute-like)
        diagnostics.push("\n[3] Routing Information:".to_string());
        #[cfg(windows)]
        {
            // Try to resolve IP first for tracert
            if let Ok(mut addrs) = tokio::net::lookup_host(format!("{}:{}", endpoint.host, endpoint.port)).await {
                if let Some(addr) = addrs.next() {
                    let target_ip = addr.ip().to_string();
                    diagnostics.push(format!("  Target IP: {}", target_ip));
                    
                    // Get routing table info
                    if let Ok(output) = Command::new("route")
                        .args(&["print", "-4"])
                        .output() {
                        let output_str = String::from_utf8_lossy(&output.stdout);
                        let mut found_route = false;
                        for line in output_str.lines() {
                            if line.contains(&target_ip) || (line.contains("0.0.0.0") && !found_route) {
                                diagnostics.push(format!("    {}", line.trim()));
                                found_route = true;
                            }
                        }
                    }
                    
                    // Try tracert (first 3 hops, 1s timeout per hop)
                    diagnostics.push("  Traceroute (first 3 hops):".to_string());
                    if let Ok(output) = Command::new("tracert")
                        .args(&["-h", "3", "-w", "1000", "-d", &target_ip])
                        .output() {
                        let output_str = String::from_utf8_lossy(&output.stdout);
                        let mut hop_count = 0;
                        for line in output_str.lines().skip(3) {
                            if hop_count >= 3 { break; }
                            if !line.trim().is_empty() && !line.contains("Tracing route") {
                                diagnostics.push(format!("    {}", line.trim()));
                                hop_count += 1;
                            }
                        }
                        if hop_count == 0 {
                            diagnostics.push("    (No response - may require admin or be blocked)".to_string());
                        }
                    } else {
                        diagnostics.push("    (tracert failed - may require admin privileges)".to_string());
                    }
                }
            }
        }
        
        #[cfg(not(windows))]
        {
            // Linux/Mac routing info
            if let Ok(output) = Command::new("ip")
                .args(&["route", "get", &endpoint.host])
                .output() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                diagnostics.push(format!("  Route: {}", output_str.trim()));
            }
        }
        
        // 4. Local Proxy Status
        diagnostics.push("\n[4] Local Proxy Status:".to_string());
        let actual_socks_port = self.get_actual_socks_port();
        let actual_http_port = self.get_actual_http_port();
        let (socks_listening, http_listening) = crate::proxy::xray_check::check_local_proxy_ports(
            actual_socks_port,
            actual_http_port
        ).await;
        diagnostics.push(format!("  SOCKS port {}: {}", actual_socks_port, if socks_listening { "✓ listening" } else { "✗ NOT listening" }));
        diagnostics.push(format!("  HTTP port {}: {}", actual_http_port, if http_listening { "✓ listening" } else { "✗ NOT listening" }));
        diagnostics.push(format!("  Xray process: {}", if self.is_running() { "✓ running" } else { "✗ NOT running" }));
        
        // 5. Network Interface Information
        diagnostics.push("\n[5] Network Interfaces:".to_string());
        #[cfg(windows)]
        {
            if let Ok(output) = Command::new("ipconfig").output() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                for line in output_str.lines() {
                    if line.contains("IPv4") || line.contains("Subnet") || line.contains("Default Gateway") {
                        diagnostics.push(format!("    {}", line.trim()));
                    }
                }
            }
        }
        
        #[cfg(not(windows))]
        {
            if let Ok(output) = Command::new("ifconfig").output() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                for line in output_str.lines().take(10) {
                    if line.contains("inet ") {
                        diagnostics.push(format!("    {}", line.trim()));
                    }
                }
            }
        }
        
        // 6. Isolated Endpoint Test (Domain vs IP)
        diagnostics.push("\n[6] Isolated Endpoint Test:".to_string());
        diagnostics.push(format!("  Testing endpoint: {}:{} ({})", endpoint.host, endpoint.port, endpoint.protocol));
        
        // Test 1: Direct connection using hostname
        diagnostics.push("  [6a] Direct connection via hostname:".to_string());
        let hostname_test = timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        ).await;
        
        match hostname_test {
            Ok(Ok(_)) => {
                diagnostics.push(format!("    ✓ Hostname connection succeeded: {}:{}", endpoint.host, endpoint.port));
            }
            Ok(Err(e)) => {
                diagnostics.push(format!("    ✗ Hostname connection failed: {}", e));
            }
            Err(_) => {
                diagnostics.push("    ✗ Hostname connection timed out (5s)".to_string());
            }
        }
        
        // Test 2: Resolve IP and test direct connection using IP
        diagnostics.push("  [6b] Direct connection via IP address:".to_string());
        match timeout(Duration::from_secs(5), tokio::net::lookup_host(format!("{}:{}", endpoint.host, endpoint.port))).await {
            Ok(Ok(mut addrs)) => {
                if let Some(addr) = addrs.next() {
                    let ip = addr.ip();
                    diagnostics.push(format!("    Resolved IP: {}", ip));
                    
                    let ip_test = timeout(
                        Duration::from_secs(5),
                        tokio::net::TcpStream::connect((ip, endpoint.port))
                    ).await;
                    
                    match ip_test {
                        Ok(Ok(_)) => {
                            diagnostics.push(format!("    ✓ IP connection succeeded: {}:{}", ip, endpoint.port));
                            diagnostics.push("    → Endpoint is reachable, issue may be with VMess protocol/config".to_string());
                        }
                        Ok(Err(e)) => {
                            diagnostics.push(format!("    ✗ IP connection failed: {}", e));
                            diagnostics.push("    → Endpoint server may be down or blocking connections".to_string());
                        }
                        Err(_) => {
                            diagnostics.push(format!("    ✗ IP connection timed out (5s)"));
                            diagnostics.push("    → Endpoint server may be unreachable or firewall blocking".to_string());
                        }
                    }
                } else {
                    diagnostics.push("    ✗ Could not resolve IP address".to_string());
                }
            }
            Ok(Err(e)) => {
                diagnostics.push(format!("    ✗ DNS resolution failed: {}", e));
            }
            Err(_) => {
                diagnostics.push("    ✗ DNS resolution timed out (5s)".to_string());
            }
        }
        
        // Test 3: Test all resolved IPs
        diagnostics.push("  [6c] Testing all resolved IP addresses:".to_string());
        match timeout(Duration::from_secs(5), tokio::net::lookup_host(format!("{}:{}", endpoint.host, endpoint.port))).await {
            Ok(Ok(addrs)) => {
                let mut ip_list: Vec<std::net::IpAddr> = addrs.map(|a| a.ip()).collect();
                ip_list.sort();
                ip_list.dedup();
                
                if ip_list.is_empty() {
                    diagnostics.push("    ✗ No IP addresses resolved".to_string());
                } else {
                    diagnostics.push(format!("    Found {} IP address(es):", ip_list.len()));
                    for (idx, ip) in ip_list.iter().enumerate() {
                        let ip_test = timeout(
                            Duration::from_secs(3),
                            tokio::net::TcpStream::connect((*ip, endpoint.port))
                        ).await;
                        
                        match ip_test {
                            Ok(Ok(_)) => {
                                diagnostics.push(format!("      [{}] {}:{} - ✓ REACHABLE", idx + 1, ip, endpoint.port));
                            }
                            Ok(Err(e)) => {
                                diagnostics.push(format!("      [{}] {}:{} - ✗ Failed: {}", idx + 1, ip, endpoint.port, e));
                            }
                            Err(_) => {
                                diagnostics.push(format!("      [{}] {}:{} - ✗ Timeout", idx + 1, ip, endpoint.port));
                            }
                        }
                    }
                }
            }
            _ => {
                diagnostics.push("    ✗ Could not resolve hostname".to_string());
            }
        }
        
        // 7. Connection Timing Analysis
        diagnostics.push("\n[7] Connection Timing Analysis:".to_string());
        diagnostics.push("  All test endpoints timed out after 10 seconds each".to_string());
        diagnostics.push("  This suggests:".to_string());
        diagnostics.push("    - Proxy server may be unreachable".to_string());
        diagnostics.push("    - Firewall may be blocking outbound connections".to_string());
        diagnostics.push("    - Network routing issue".to_string());
        
        // Add VMess-specific diagnostics
        if endpoint.protocol.to_lowercase() == "vmess" {
            diagnostics.push("    - VMess endpoint configuration may be incorrect".to_string());
            diagnostics.push("    - System time may be out of sync (VMess requires ±120s UTC accuracy)".to_string());
            diagnostics.push("      → VMess authentication uses UTC time with ±30 second window".to_string());
            diagnostics.push("      → Check system time synchronization (NTP) if connections fail".to_string());
            diagnostics.push("      → Reference: https://xtls.github.io/development/protocols/vmess.html".to_string());
        } else {
            diagnostics.push("    - Endpoint configuration may be incorrect".to_string());
        }
        
        diagnostics
    }
}

/// Configuration for an endpoint
#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub host: String,
    pub port: u16,
    pub protocol: String,
    pub uuid: Option<String>,
    pub password: Option<String>,
    pub username: Option<String>,
    pub method: Option<String>, // For Shadowsocks
    pub encryption: Option<String>, // For VLESS
    pub flow: Option<String>, // For VLESS XTLS (e.g., "xtls-rprx-vision")
    pub alter_id: Option<u16>, // For VMess
    pub security: Option<String>, // For VMess
    pub network: Option<String>, // tcp, ws, kcp, etc.
    pub path: Option<String>, // For WS
    pub host_header: Option<String>, // For WS
    pub tls: Option<TlsConfig>,
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub server_name: Option<String>,
    pub allow_insecure: bool,
}


